//! Finding a crate's licence and notice files on disk, and reading them.
//!
//! A crate's own files are the licence-like files in its folder (`LICENSE`, `LICENSE-MIT`, `COPYING`, `NOTICE`, ...) and in a `licenses` folder there. A crate that
//! lives in a git checkout of a larger repository (the egui family, the craft libraries) also gets the files of the folders above it, up to the checkout's root:
//! its licence is usually kept there. **Nested** files are the ones deeper in the crate, kept with code or data that the crate carries (a vendored C library, font
//! data, an ICC profile, a protocol description): they travel with the program too. The tests, benchmarks, examples and docs of a crate are not looked in: they are
//! not compiled into the program.

use std::path::{Component, Path, PathBuf};

/// The most bytes of one licence file that is read: a bigger one is an error, not a silent cut.
pub const MOST_FILE_BYTES: u64 = 1024 * 1024;

/// The most licence files kept for one crate.
const MOST_FILES: usize = 64;

/// How deep in a crate nested files are looked for, and the most entries looked at, so that a crate that holds a huge tree cannot make the tool hang.
const NESTED_DEPTH: usize = 6;
const MOST_ENTRIES: usize = 50_000;

/// Folders of a crate that are not compiled into a program.
const SKIPPED_DIRS: &[&str] = &["target", ".git", ".github", "node_modules", "tests", "test", "benches", "examples", "docs", "doc", "fuzz"];

/// Extensions that make a file source code or data and not a licence, whatever its name (`src/copying.rs`).
const NOT_TEXT: &[&str] =
    &["rs", "c", "h", "cc", "cpp", "hpp", "toml", "json", "yaml", "yml", "lock", "py", "js", "ts", "sh", "html", "css", "xml", "ttf", "otf"];

/// Where a file is, from the crate's own folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// In the crate's folder, or in a `licenses` folder there.
    Own,
    /// In a folder above the crate, in the checkout of the repository that holds it: the repository's file, for all the crates in it.
    Above,
    /// Deeper in the crate than its folder.
    Nested,
}

/// A licence or notice file of a crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Where it is, from the crate's folder (`LICENSE-MIT`, `c_src/mimalloc/v2/LICENSE`) or, for a file above it in a checkout, from the checkout's root.
    pub rel: String,
    /// The text, with line ends made `\n`.
    pub text: String,
    pub place: Place,
}

/// Whether a file name is that of a licence, a copyright notice or a NOTICE file: it starts with one of those words and goes on after `-`, `_` or `.`.
pub fn is_licence_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let stem = lower.split(['.', '-', '_']).next().unwrap_or_default();
    if !matches!(stem, "license" | "licence" | "copying" | "notice" | "unlicense" | "copyright") {
        return false;
    }
    match lower.rsplit_once('.') {
        Some((_, ext)) => !NOT_TEXT.contains(&ext),
        None => true,
    }
}

/// Whether the file is a NOTICE file (kept whatever it says).
pub fn is_notice_name(name: &str) -> bool {
    name.to_ascii_lowercase().split(['.', '-', '_']).next() == Some("notice")
}

/// Reads a licence file: at most [`MOST_FILE_BYTES`], decoded as UTF-8 or, when it is not, as Latin-1 (where a byte is the character of its number), with
/// line ends made `\n`.
pub fn read_text(path: &Path) -> Result<String, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if size > MOST_FILE_BYTES {
        return Err(format!("{}: {size} bytes, a licence file over {MOST_FILE_BYTES} bytes is not read", path.display()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => e.into_bytes().into_iter().map(char::from).collect(),
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// A path relative to `base`, with `/` between its parts.
fn relative(base: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(base).unwrap_or(path);
    rel.components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// The licence-like files directly in `dir`, by name; in a folder that is for licences (`any`), every text file.
fn files_in(dir: &Path, any: bool) -> Result<Vec<PathBuf>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    };
    let mut out = Vec::new();
    for entry in entries.take(MOST_ENTRIES) {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = entry.path();
        // `file_type` does not follow a link: a link is not read, so that a crate cannot make the tool read a file elsewhere on the machine.
        let is_file = entry.file_type().map_err(|e| format!("{}: {e}", path.display()))?.is_file();
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_file && (is_licence_name(&name) || (any && is_text_name(&name))) {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// Whether a file name is that of a text (not a source or data file): no extension, or one that is not in [`NOT_TEXT`].
fn is_text_name(name: &str) -> bool {
    match name.to_ascii_lowercase().rsplit_once('.') {
        Some((_, ext)) => !NOT_TEXT.contains(&ext),
        None => true,
    }
}

/// The folders of a crate where licences are kept in a folder of their own (`licenses/`, `LICENSES/` of the REUSE convention).
///
/// The names are matched against the folder's own list of entries, not probed: on a file system that ignores case (macOS's, by default), a probe of
/// `LICENSES` finds `licenses` too, and the files of the one folder were listed twice.
fn licence_dirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    // The kind of the entry itself: a link to a folder is not followed, as before.
    let folders: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    ["licenses", "LICENSES", "licences", "LICENCES", "license", "licence"]
        .into_iter()
        .filter(|name| folders.iter().any(|f| f == name))
        .map(|name| dir.join(name))
        .collect()
}

fn read_all(base: &Path, paths: &[PathBuf], place: Place, out: &mut Vec<Found>) -> Result<(), String> {
    for path in paths {
        if out.len() >= MOST_FILES {
            return Err(format!("{}: more than {MOST_FILES} licence files", base.display()));
        }
        out.push(Found { rel: relative(base, path), text: read_text(path)?, place });
    }
    Ok(())
}

/// A crate's licence files: its own (and those above it up to `stop`, for a crate in a checkout), then the nested ones. Sorted by place, so the result does not
/// depend on the order the file system lists them in.
pub fn discover(dir: &Path, stop: Option<&Path>) -> Result<Vec<Found>, String> {
    let mut out = Vec::new();
    read_all(dir, &files_in(dir, false)?, Place::Own, &mut out)?;
    for licence_dir in licence_dirs(dir) {
        read_all(dir, &files_in(&licence_dir, true)?, Place::Own, &mut out)?;
    }
    if let Some(stop) = stop {
        for above in dir.ancestors().skip(1) {
            read_all(stop, &files_in(above, false)?, Place::Above, &mut out)?;
            if above == stop {
                break;
            }
        }
    }
    let own: Vec<PathBuf> = out.iter().map(|f| dir.join(&f.rel)).collect();
    let mut nested = Vec::new();
    let mut seen = 0usize;
    walk(dir, dir, 0, &mut nested, &mut seen)?;
    nested.retain(|p| !own.contains(p));
    nested.sort();
    read_all(dir, &nested, Place::Nested, &mut out)?;
    Ok(out)
}

fn walk(base: &Path, dir: &Path, depth: usize, out: &mut Vec<PathBuf>, seen: &mut usize) -> Result<(), String> {
    if depth > NESTED_DEPTH {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        *seen += 1;
        if *seen > MOST_ENTRIES {
            return Err(format!("{}: more than {MOST_ENTRIES} entries", base.display()));
        }
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type().map_err(|e| format!("{}: {e}", path.display()))?;
        if kind.is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_str()) {
                walk(base, &path, depth + 1, out, seen)?;
            }
        } else if kind.is_file() && depth > 0 && is_licence_name(&name) {
            out.push(path);
        }
    }
    Ok(())
}

/// Reads the named files of a crate that are licences although their names do not say so (the licences of the fonts in a font crate). A name that is not
/// there is an error: the list must not go stale.
pub fn read_listed(dir: &Path, rels: &[&str]) -> Result<Vec<Found>, String> {
    let mut out = Vec::new();
    for rel in rels {
        if rel.split('/').any(|part| part == ".." || part.is_empty()) {
            return Err(format!("the listed licence file `{rel}` is not a path inside the crate"));
        }
        let path = dir.join(rel);
        let text = read_text(&path).map_err(|e| format!("the listed licence file `{rel}` cannot be read: {e}"))?;
        out.push(Found { rel: (*rel).to_owned(), text, place: Place::Nested });
    }
    Ok(out)
}
