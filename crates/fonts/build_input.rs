//! The pure part of the optional craft-fonts build input, shared by `build.rs` (which runs it) and the crate's tests (which
//! include this file, so the parsing is tested as the build script uses it).
//!
//! `CRAFT_FONTS_DIR` names a checkout of `storytold/craft-fonts`. Its `fonts/manifest.txt` and `fonts/latin-manifest.txt` list
//! one face to a line, `family | style | file | scripts | ...`, with the file relative to the checkout. The format is the one
//! DeckCraft's build script reads (`docs/provenance.md`). Nothing here can fail the build: a bad line, a missing file or an
//! unreadable manifest is reported and skipped, because every build must work without craft-fonts.

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// The largest single font file embedded, and the most bytes embedded in all. A face past either is skipped with a warning.
pub const MAX_FILE_BYTES: u64 = 48 << 20;
pub const MAX_TOTAL_BYTES: u64 = 192 << 20;
/// The most manifest lines read from one manifest, and the most bytes of a manifest file read.
pub const MAX_ROWS: usize = 4096;
pub const MAX_MANIFEST_BYTES: u64 = 1 << 20;

/// One face a manifest lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub family: String,
    pub style: String,
    pub file: String,
    pub scripts: Vec<String>,
}

/// Families never embedded: Adobe-authored typefaces, even openly licensed ones, and Source Han in any rebrand (the policy
/// of the sibling this build input comes from, kept as it is).
pub fn is_excluded_family(family: &str) -> bool {
    ["Source Sans", "Source Serif", "Source Han", "Noto Sans CJK", "Noto Serif CJK", "Noto Sans Mono CJK"].iter().any(|p| family.starts_with(p))
}

/// The rows of one manifest, and what was wrong with the lines that gave none. Blank lines and `#` comments are skipped.
pub fn parse_manifest(text: &str) -> (Vec<Row>, Vec<String>) {
    let mut rows = Vec::new();
    let mut problems = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        if rows.len() >= MAX_ROWS {
            problems.push(format!("more than {MAX_ROWS} rows: the rest are left out"));
            break;
        }
        let fields: Vec<&str> = line.split(" | ").map(str::trim).collect();
        let [family, style, file, scripts, ..] = fields.as_slice() else {
            problems.push(format!("malformed manifest line: {}", line.chars().take(80).collect::<String>()));
            continue;
        };
        if family.is_empty() || file.is_empty() {
            problems.push(format!("a manifest line has no family or file: {}", line.chars().take(80).collect::<String>()));
            continue;
        }
        if is_excluded_family(family) {
            continue;
        }
        rows.push(Row {
            family: (*family).to_owned(),
            style: (*style).to_owned(),
            file: (*file).to_owned(),
            scripts: scripts.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect(),
        });
    }
    (rows, problems)
}

/// Does this file start as a font does: a TrueType or OpenType font, or a collection of them? The manifest is data, and a name in it
/// is not a reason to embed a file that is not a font.
pub fn is_font_header(head: &[u8]) -> bool {
    matches!(head.get(..4), Some([0, 1, 0, 0]) | Some(b"OTTO") | Some(b"true") | Some(b"ttcf"))
}

/// The file a row names, if it is a regular file inside `dir` of a size that can be embedded, with a font's header, at a path that
/// can be written into Rust source. A name that leaves the checkout (`../`, a symlink out of it) is refused: the manifest is data,
/// not a way to embed any file on the machine.
pub fn resolve_file(dir: &Path, row: &Row) -> Result<(PathBuf, u64), String> {
    let root = dir.canonicalize().map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = root.join(&row.file).canonicalize().map_err(|e| format!("{}: {e}", row.file))?;
    if !path.starts_with(&root) {
        return Err(format!("{}: outside CRAFT_FONTS_DIR", row.file));
    }
    if path.to_str().is_none() {
        return Err(format!("{}: the path is not valid UTF-8", row.file));
    }
    let meta = std::fs::metadata(&path).map_err(|e| format!("{}: {e}", row.file))?;
    if !meta.is_file() || meta.len() < 12 || meta.len() > MAX_FILE_BYTES {
        return Err(format!("{}: not a regular file of 12 bytes to {} MiB", row.file, MAX_FILE_BYTES >> 20));
    }
    let mut head = [0u8; 4];
    let read = std::fs::File::open(&path).and_then(|mut f| f.read_exact(&mut head));
    if read.is_err() || !is_font_header(&head) {
        return Err(format!("{}: not a font file (TrueType, OpenType or a collection)", row.file));
    }
    Ok((path, meta.len()))
}

/// The checkout `CRAFT_FONTS_DIR` names, if it can be used, and a warning if it cannot. A value that is not valid UTF-8, or holds a
/// control character such as a newline, would end up in the generated source or in a line cargo reads as directives, so it is refused:
/// the build goes on without craft-fonts.
pub fn checked_dir(raw: Option<&OsStr>) -> (Option<PathBuf>, Vec<String>) {
    let Some(raw) = raw else { return (None, Vec::new()) };
    match raw.to_str() {
        None => (None, vec!["CRAFT_FONTS_DIR is not valid UTF-8: building without craft-fonts".to_owned()]),
        Some(s) if s.chars().any(char::is_control) => {
            (None, vec!["CRAFT_FONTS_DIR holds a control character: building without craft-fonts".to_owned()])
        }
        Some("") => (None, Vec::new()),
        Some(s) => (Some(PathBuf::from(s)), Vec::new()),
    }
}

/// The lines for cargo that say when to build again, for a checkout that passed [`checked_dir`].
pub fn rerun_directives(dir: Option<&Path>) -> Vec<String> {
    let mut lines = vec!["cargo::rerun-if-env-changed=CRAFT_FONTS_DIR".to_owned(), "cargo::rerun-if-changed=build_input.rs".to_owned()];
    if let Some(dir) = dir {
        lines.push(format!("cargo::rerun-if-changed={}", dir.join("fonts").display()));
    }
    lines
}

/// A manifest's text, if it is a regular file of at most [`MAX_MANIFEST_BYTES`]: a pipe or a device in its place would hang the
/// build, and a huge file would fill its memory.
pub fn read_manifest(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".to_owned());
    }
    if meta.len() > MAX_MANIFEST_BYTES {
        return Err(format!("more than {} MiB", MAX_MANIFEST_BYTES >> 20));
    }
    let mut text = String::new();
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    file.take(MAX_MANIFEST_BYTES).read_to_string(&mut text).map_err(|e| e.to_string())?;
    Ok(text)
}

/// The Rust source of `CRAFT_FONTS` for the faces chosen: one `CraftFont` per row, the bytes by `include_bytes!`.
pub fn render(faces: &[(Row, PathBuf)]) -> String {
    let mut src = String::from("pub(crate) static CRAFT_FONTS: &[CraftFont] = &[\n");
    for (row, path) in faces {
        let scripts: Vec<String> = row.scripts.iter().map(|s| format!("{s:?}")).collect();
        // `write!` into a `String` cannot fail.
        let _ = writeln!(
            src,
            "    CraftFont {{ family: {:?}, style: {:?}, scripts: &[{}], bytes: include_bytes!({:?}) }},",
            row.family,
            row.style,
            scripts.join(", "),
            path.display().to_string()
        );
    }
    src.push_str("];\n");
    src
}

/// Everything the build script does with `CRAFT_FONTS_DIR`, apart from writing the file: the source to write and the warnings
/// to print. `dir` is `None` when the variable is unset (an empty list, no warning), and `wasm` leaves the list empty, to keep
/// the web build small (the browser shell may supply faces another way).
pub fn build(dir: Option<&Path>, wasm: bool) -> (String, Vec<String>) {
    let mut warnings = Vec::new();
    let Some(dir) = dir.filter(|_| !wasm) else { return (render(&[]), warnings) };
    let mut chosen: Vec<(Row, PathBuf)> = Vec::new();
    let mut total = 0u64;
    for manifest in ["fonts/latin-manifest.txt", "fonts/manifest.txt"] {
        let text = match read_manifest(&dir.join(manifest)) {
            Ok(t) => t,
            Err(e) => {
                warnings.push(format!("building without {manifest}: {e}"));
                continue;
            }
        };
        let (rows, problems) = parse_manifest(&text);
        warnings.extend(problems.into_iter().map(|p| format!("{manifest}: {p}")));
        for row in rows {
            match resolve_file(dir, &row) {
                Ok((path, len)) if total + len <= MAX_TOTAL_BYTES => {
                    total += len;
                    chosen.push((row, path));
                }
                Ok(_) => warnings.push(format!("{}: more than {} MiB of fonts: left out", row.file, MAX_TOTAL_BYTES >> 20)),
                Err(e) => warnings.push(e),
            }
        }
    }
    (render(&chosen), warnings)
}
