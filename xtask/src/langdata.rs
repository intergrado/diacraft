//! `cargo xtask langdata`: the tables of `diacraft_doc::lang`, generated from two public sources and committed with this generator.
//!
//! Two sources, both data under the Unicode License V3 (SPDX `Unicode-3.0`), at pinned releases ([`PINS`]):
//!
//! * **ICU4C's `icu4c/source/common/locmap.cpp`** pairs LCID numbers with language tags (design call: the pairing is never copied from a table of the reference app's file format).
//!   The generator reads the file's own tables (`ILCID_POSIX_ELEMENT_ARRAY` and `ILCID_POSIX_SUBTABLE`, in the order of `gPosixIDmap`) and the way ICU reads them
//!   (`uprv_convertToPosix` and `uprv_convertToLCID`), and writes `lcid.rs`.
//! * **Unicode CLDR's JSON** (`cldr-localenames-full`, `cldr-misc-full`, `cldr-core`) gives the English names of languages, scripts and regions (`names.rs`), each
//!   language's quotation marks (`quotes.rs`) and its likely script (`scripts.rs`). The release is the one craft-i18n's data was generated from
//!   (`crates/i18n/data/source.txt` of the craft-libs revision DiaCraft pins; `--i18n-source` checks it), so that the two never disagree.
//!
//! The generator fetches nothing: `--cldr` and `--icu` name folders the person made (docs/development.md), and each is checked before it is read: the CLDR version and
//! licence from `cldr-core/package.json`, the ICU licence from its `LICENSE`, and `locmap.cpp` by its SHA-256. The output is deterministic (sorted maps, no hash order,
//! no clock), so `--check` can say a committed table is what the sources give.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

/// What `cargo xtask langdata` prints for a mistake in its command line, and what `cargo xtask help` says of it.
pub const USAGE: &str = "usage: cargo xtask langdata --cldr <DIR> --icu <DIR> [--i18n-source <FILE>] [--check]

  --cldr <DIR>          the folder the CLDR JSON archive (cldr-<release>-json-full) unpacks to: cldr-core, cldr-localenames-full and cldr-misc-full, at the CLDR
                        release this tool is pinned to
  --icu <DIR>           a folder with ICU4C's source/common/locmap.cpp and locmap.h of the release this tool is pinned to, and ICU's LICENSE file
  --i18n-source <FILE>  craft-i18n's record of its data (crates/i18n/data/source.txt): refuses a CLDR release other than the tool's
  --check               write nothing: fail naming each committed table that is not what the sources give
";

/// The sources and releases the tables are made from. Changing a pin means reading the new source again: the licence, the file and what its tables hold.
#[derive(Clone, Copy, Debug)]
pub struct Pins<'a> {
    /// The tag of ICU's repository.
    pub icu_release: &'a str,
    /// The SHA-256 of `icu4c/source/common/locmap.cpp` at that tag.
    pub locmap_sha256: &'a str,
    /// The CLDR release (the `version` of `cldr-core/package.json`), which is the release craft-i18n's data came from.
    pub cldr_release: &'a str,
}

/// ICU 79.1 (CLDR 49's own ICU, the release ICU4X 2.4's data export is built from) and CLDR 49.0.0 (craft-i18n's: `crates/i18n/data/source.txt`).
pub const PINS: Pins<'static> =
    Pins { icu_release: "release-79.1", locmap_sha256: "12ca2c25b7bc24da6ace5fdb8cf61bfa71817a916ede611d869b149fa19f6f4e", cldr_release: "49.0.0" };

/// Where the tables go, from the root of the repository.
pub const OUTPUT_DIR: &str = "crates/doc/src/lang";

/// The most a source file may hold: a hostile or wrong file is refused before it is read whole.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// The most locale folders `cldr-misc-full/main` may hold (CLDR 49 has 657).
const MAX_LOCALES: usize = 5_000;
/// The most characters a quotation mark may be.
const MAX_MARK_CHARS: usize = 4;
/// The longest token read as a number in `locmap.cpp`.
const MAX_NUMBER_CHARS: usize = 16;
/// ICU's language ID of a number: the macro `LANGUAGE_LCID` of `locmap.h`.
const LANGUAGE_ID_MASK: u32 = 0x3FF;
/// A `LangID` is 16 bits; a larger number is a sort order stacked on a language ID.
const MAX_LANGUAGE_ID: u32 = 0xFFFF;
/// The line of `locmap.h` that fixes [`LANGUAGE_ID_MASK`], with its white space made single spaces.
const LANGUAGE_LCID_MACRO: &str = "#define LANGUAGE_LCID(hostID) (uint16_t)(0x03FF & hostID)";
/// The ids of ICU's table that name no language: the root and the invariant locale (`en_US_POSIX`, kept for round trips).
const NO_LANGUAGE_IDS: [&str; 2] = ["root", "en_US_POSIX"];

/// The four generated files, as text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Files {
    pub lcid: String,
    pub names: String,
    pub quotes: String,
    pub scripts: String,
}

impl Files {
    /// The files with their names, in the order they are written and reported.
    pub fn named(&self) -> [(&'static str, &str); 4] {
        [("lcid.rs", &self.lcid), ("names.rs", &self.names), ("quotes.rs", &self.quotes), ("scripts.rs", &self.scripts)]
    }
}

// ---- the command --------------------------------------------------------------------------------------------------------------------

struct Options {
    cldr: PathBuf,
    icu: PathBuf,
    i18n_source: Option<PathBuf>,
    check: bool,
}

fn parse_args(args: &[&str]) -> Result<Options, String> {
    let (mut cldr, mut icu, mut i18n_source, mut check) = (None, None, None, false);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match *arg {
            "--check" => check = true,
            "--cldr" | "--icu" | "--i18n-source" => {
                let value = rest.next().ok_or_else(|| format!("{arg} needs a path\n\n{USAGE}"))?;
                match *arg {
                    "--cldr" => cldr = Some(PathBuf::from(value)),
                    "--icu" => icu = Some(PathBuf::from(value)),
                    _ => i18n_source = Some(PathBuf::from(value)),
                }
            }
            other => return Err(format!("unknown option `{other}`\n\n{USAGE}")),
        }
    }
    let cldr = cldr.ok_or_else(|| format!("--cldr <DIR> is needed: the folder CLDR's JSON archive unpacks to\n\n{USAGE}"))?;
    let icu = icu.ok_or_else(|| format!("--icu <DIR> is needed: the folder with ICU's locmap.cpp, locmap.h and LICENSE\n\n{USAGE}"))?;
    Ok(Options { cldr, icu, i18n_source, check })
}

/// `cargo xtask langdata`: writes the four tables of `diacraft_doc::lang` under `root`, or with `--check` says whether they are what the sources give.
pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    run_with(root, args, &PINS)
}

/// [`run`] with the pins named, for the tests' miniature sources.
pub fn run_with(root: &Path, args: &[&str], pins: &Pins) -> Result<(), String> {
    let options = parse_args(args)?;
    if let Some(record) = &options.i18n_source {
        check_i18n_source(&read_text(record)?, pins)?;
    }
    let files = generate(&options.cldr, &options.icu, pins)?;
    let folder = root.join(OUTPUT_DIR);
    if options.check {
        let stale: Vec<&str> = files
            .named()
            .into_iter()
            .filter(|(name, text)| std::fs::read_to_string(folder.join(name)).ok().as_deref() != Some(*text))
            .map(|(name, _)| name)
            .collect();
        if !stale.is_empty() {
            return Err(format!(
                "{} of the committed tables in {OUTPUT_DIR} are not what the sources give (CLDR {}, ICU {}): {}; run `cargo xtask langdata --cldr <DIR> --icu <DIR>` and commit them",
                stale.len(),
                pins.cldr_release,
                pins.icu_release,
                stale.join(", ")
            ));
        }
        println!("langdata: the four tables are what the sources give (CLDR {}, ICU {})", pins.cldr_release, pins.icu_release);
        return Ok(());
    }
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    for (name, text) in files.named() {
        std::fs::write(folder.join(name), text).map_err(|e| format!("{}: {e}", folder.join(name).display()))?;
        println!("langdata: wrote {OUTPUT_DIR}/{name}: {} rows", count_rows(text));
    }
    Ok(())
}

/// The rows of a table: the lines that begin a tuple or a string.
fn count_rows(text: &str) -> usize {
    text.lines().filter(|line| line.trim_start().starts_with(['(', '"'])).count()
}

/// Checks craft-i18n's record of its data (`icu4x-datagen 2.4.0` and `cldr 49.0.0`, one per line) names the CLDR release of these tables.
pub fn check_i18n_source(text: &str, pins: &Pins) -> Result<(), String> {
    let found = text.lines().find_map(|line| line.strip_prefix("cldr ")).map(str::trim);
    match found {
        Some(release) if release == pins.cldr_release => Ok(()),
        Some(release) => Err(format!(
            "craft-i18n's data record says CLDR {release}, but these tables are CLDR {}: regenerate both from one release, so that they do not disagree",
            pins.cldr_release
        )),
        None => Err("craft-i18n's data record has no `cldr` line, so the release of its data cannot be checked".to_owned()),
    }
}

// ---- reading ------------------------------------------------------------------------------------------------------------------------

fn read_text(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(format!("{}: {} bytes is more than the {MAX_FILE_BYTES} this tool reads", path.display(), meta.len()));
    }
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn read_json(path: &Path) -> Result<Value, String> {
    serde_json::from_str(&read_text(path)?).map_err(|e| format!("{}: not JSON: {e}", path.display()))
}

fn at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(value, |v, key| v.get(*key))
}

/// The object at `path` of a CLDR file, as a map of strings.
fn string_map(file: &Path, value: &Value, path: &[&str]) -> Result<BTreeMap<String, String>, String> {
    let object = at(value, path).and_then(Value::as_object).ok_or_else(|| format!("{}: no object at {}", file.display(), path.join(".")))?;
    Ok(object.iter().filter_map(|(key, value)| value.as_str().map(|text| (key.clone(), text.to_owned()))).collect())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

// ---- tags ---------------------------------------------------------------------------------------------------------------------------

/// A language tag split into its parts, in canonical case.
struct Parts {
    text: String,
    language: String,
    script: Option<String>,
    region: Option<String>,
    variants: Vec<String>,
}

fn title_case(sub: &str) -> String {
    let mut chars = sub.chars();
    chars.next().map(|first| first.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase()).unwrap_or_default()
}

/// `language[sep script][sep region][sep variant]*` (RFC 5646's order, no extensions) in canonical case, or `None` for any other shape.
fn parse_id(id: &str, sep: char) -> Option<Parts> {
    let mut subs = id.split(sep);
    let language = subs.next()?;
    if !(2..=3).contains(&language.len()) || !language.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let mut parts =
        Parts { text: language.to_ascii_lowercase(), language: language.to_ascii_lowercase(), script: None, region: None, variants: Vec::new() };
    for sub in subs {
        let alpha = sub.bytes().all(|b| b.is_ascii_alphabetic());
        let digits = sub.bytes().all(|b| b.is_ascii_digit());
        let canonical = if alpha && sub.len() == 4 && parts.script.is_none() && parts.region.is_none() && parts.variants.is_empty() {
            let script = title_case(sub);
            parts.script = Some(script.clone());
            script
        } else if ((alpha && sub.len() == 2) || (digits && sub.len() == 3)) && parts.region.is_none() && parts.variants.is_empty() {
            let region = sub.to_ascii_uppercase();
            parts.region = Some(region.clone());
            region
        } else if sub.bytes().all(|b| b.is_ascii_alphanumeric())
            && ((5..=8).contains(&sub.len()) || (sub.len() == 4 && sub.bytes().next().is_some_and(|b| b.is_ascii_digit())))
        {
            let variant = sub.to_ascii_lowercase();
            parts.variants.push(variant.clone());
            variant
        } else {
            return None;
        };
        parts.text.push('-');
        parts.text.push_str(&canonical);
    }
    Some(parts)
}

fn is_region(sub: &str) -> bool {
    (sub.len() == 2 && sub.bytes().all(|b| b.is_ascii_alphabetic())) || (sub.len() == 3 && sub.bytes().all(|b| b.is_ascii_digit()))
}

// ---- CLDR ---------------------------------------------------------------------------------------------------------------------------

/// CLDR's English names, from `cldr-localenames-full/main/en`.
struct Names {
    pattern: String,
    separator: String,
    languages: BTreeMap<String, String>,
    scripts: BTreeMap<String, String>,
    territories: BTreeMap<String, String>,
    variants: BTreeMap<String, String>,
}

struct Cldr {
    names: Names,
    /// `likelySubtags.json`: a tag to its likely full form.
    likely: BTreeMap<String, String>,
    /// Each locale of `cldr-misc-full` with its resolved marks, in canonical case. The root (`und`) is apart.
    marks: BTreeMap<String, [String; 4]>,
    root: [String; 4],
}

const DELIMITER_KEYS: [&str; 4] = ["quotationStart", "quotationEnd", "alternateQuotationStart", "alternateQuotationEnd"];

fn check_cldr_source(dir: &Path, pins: &Pins) -> Result<(), String> {
    let package_path = dir.join("cldr-core/package.json");
    let package = read_json(&package_path)?;
    let version = package.get("version").and_then(Value::as_str).unwrap_or("(none)");
    if version != pins.cldr_release {
        return Err(format!(
            "{}: CLDR {version}, but these tables are pinned to CLDR {} (the release craft-i18n's data came from): use that release's JSON",
            package_path.display(),
            pins.cldr_release
        ));
    }
    let license = package.get("license").and_then(Value::as_str).unwrap_or("(none)");
    if license != "Unicode-3.0" {
        return Err(format!(
            "{}: the licence is `{license}`, not Unicode-3.0: the tables may only be made from data under the Unicode License V3",
            package_path.display()
        ));
    }
    let licence_file = dir.join("LICENSE");
    if !read_text(&licence_file)?.contains("UNICODE LICENSE V3") {
        return Err(format!("{}: it is not the Unicode License V3, which the CLDR data must come under", licence_file.display()));
    }
    Ok(())
}

fn read_names(dir: &Path) -> Result<Names, String> {
    let folder = dir.join("cldr-localenames-full/main/en");
    let section = |file: &str, key: &str| -> Result<BTreeMap<String, String>, String> {
        let path = folder.join(file);
        string_map(&path, &read_json(&path)?, &["main", "en", "localeDisplayNames", key])
    };
    let display = folder.join("localeDisplayNames.json");
    let patterns = string_map(&display, &read_json(&display)?, &["main", "en", "localeDisplayNames", "localeDisplayPattern"])?;
    let pattern = |key: &str| patterns.get(key).cloned().ok_or_else(|| format!("{}: no `{key}`", display.display()));
    let (pattern, separator) = (pattern("localePattern")?, pattern("localeSeparator")?);
    for text in [&pattern, &separator] {
        if !text.contains("{0}") || !text.contains("{1}") {
            return Err(format!("{}: a pattern without {{0}} and {{1}}: `{text}`", display.display()));
        }
    }
    // Plain codes only: a language of two or three letters, a script of four, a region of two letters or three digits, a variant in upper case. The dialect names
    // (`en-GB`) and the alternates (`-alt-short`) are keys with a hyphen.
    let keep = |map: BTreeMap<String, String>, plain: fn(&str) -> bool| -> BTreeMap<String, String> {
        map.into_iter().filter(|(key, value)| plain(key) && !value.is_empty()).collect()
    };
    let languages = keep(section("languages.json", "languages")?, |k| (2..=3).contains(&k.len()) && k.bytes().all(|b| b.is_ascii_lowercase()));
    let scripts = keep(section("scripts.json", "scripts")?, |k| k.len() == 4 && k.bytes().all(|b| b.is_ascii_alphabetic()));
    let territories = keep(section("territories.json", "territories")?, is_region);
    let variants = keep(section("variants.json", "variants")?, |k| k.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()));
    if languages.get("en").map(String::as_str) != Some("English") {
        return Err(format!("{}: no English name `English` for `en`: this is not CLDR's English data", folder.join("languages.json").display()));
    }
    Ok(Names {
        pattern,
        separator,
        languages,
        scripts,
        territories,
        variants: variants.into_iter().map(|(key, value)| (key.to_ascii_lowercase(), value)).collect(),
    })
}

fn read_likely(dir: &Path) -> Result<BTreeMap<String, String>, String> {
    let path = dir.join("cldr-core/supplemental/likelySubtags.json");
    string_map(&path, &read_json(&path)?, &["supplemental", "likelySubtags"])
}

/// Each locale of `cldr-misc-full` with its marks, and the root's.
type MarksRead = (BTreeMap<String, [String; 4]>, [String; 4]);

fn read_marks(dir: &Path) -> Result<MarksRead, String> {
    let main = dir.join("cldr-misc-full/main");
    let mut folders: Vec<String> = std::fs::read_dir(&main)
        .map_err(|e| format!("{}: {e}", main.display()))?
        .filter_map(|entry| entry.ok().and_then(|entry| entry.file_name().into_string().ok()))
        .collect();
    if folders.len() > MAX_LOCALES {
        return Err(format!("{}: {} locales is more than the {MAX_LOCALES} this tool reads", main.display(), folders.len()));
    }
    folders.sort();
    let (mut marks, mut root) = (BTreeMap::new(), None);
    for locale in folders {
        let path = main.join(&locale).join("delimiters.json");
        let value = marks_of(&path, &read_json(&path)?, &locale)?;
        if locale == "und" {
            root = Some(value);
        } else {
            let id = parse_id(&locale, '-').ok_or_else(|| format!("{}: `{locale}` is no language tag", path.display()))?;
            marks.insert(id.text, value);
        }
    }
    let root = root.ok_or_else(|| format!("{}: no `und` folder, CLDR's root: the marks of a language with none are the root's", main.display()))?;
    Ok((marks, root))
}

fn marks_of(path: &Path, value: &Value, locale: &str) -> Result<[String; 4], String> {
    let delimiters = string_map(path, value, &["main", locale, "delimiters"])?;
    let mut marks: [String; 4] = Default::default();
    for (slot, key) in marks.iter_mut().zip(DELIMITER_KEYS) {
        let mark = delimiters.get(key).ok_or_else(|| format!("{}: locale `{locale}` has no `{key}`", path.display()))?;
        if mark.is_empty() || mark.chars().count() > MAX_MARK_CHARS {
            return Err(format!("{}: locale `{locale}`: `{key}` is `{mark}`, which is not 1 to {MAX_MARK_CHARS} characters", path.display()));
        }
        slot.clone_from(mark);
    }
    Ok(marks)
}

fn read_cldr(dir: &Path, pins: &Pins) -> Result<Cldr, String> {
    check_cldr_source(dir, pins)?;
    let (marks, root) = read_marks(dir)?;
    Ok(Cldr { names: read_names(dir)?, likely: read_likely(dir)?, marks, root })
}

// ---- ICU ----------------------------------------------------------------------------------------------------------------------------

/// One table of `locmap.cpp`: the rows `{number, id}`, the first of which is the language's own.
struct Subtable {
    name: String,
    rows: Vec<(u32, String)>,
}

struct Icu {
    tables: BTreeMap<String, Subtable>,
    /// The names in the order of `gPosixIDmap`.
    map: Vec<String>,
}

fn check_icu_source(dir: &Path, pins: &Pins) -> Result<String, String> {
    let licence = dir.join("LICENSE");
    if !read_text(&licence)?.contains("SPDX-License-Identifier: Unicode-3.0") {
        return Err(format!(
            "{}: ICU's licence file does not say `SPDX-License-Identifier: Unicode-3.0`: the table may only be made from data under it",
            licence.display()
        ));
    }
    let header = dir.join("locmap.h");
    let squeezed = read_text(&header)?.split_whitespace().collect::<Vec<_>>().join(" ");
    if !squeezed.contains(LANGUAGE_LCID_MACRO) {
        return Err(format!(
            "{}: the macro LANGUAGE_LCID is not `{LANGUAGE_LCID_MACRO}`: this tool reads the language ID of a number as its low ten bits",
            header.display()
        ));
    }
    let path = dir.join("locmap.cpp");
    let text = read_text(&path)?;
    let found = sha256_hex(text.as_bytes());
    if found != pins.locmap_sha256 {
        return Err(format!(
            "{}: SHA-256 is {found}, but ICU {} has {}: use that release's file (the tables are made from it alone)",
            path.display(),
            pins.icu_release,
            pins.locmap_sha256
        ));
    }
    Ok(text)
}

/// What the lexer is reading.
#[derive(Clone, Copy, PartialEq)]
enum State {
    Code,
    Line,
    Block,
    Text,
    Char,
    Directive,
}

struct Lexer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    out: String,
    state: State,
    line: usize,
    /// Only white space so far on this line of code: a `#` here begins a directive.
    at_start: bool,
    /// The directive's last non-space character was a backslash, so it goes on to the next line.
    continued: bool,
}

impl Lexer<'_> {
    fn push_blank(&mut self, c: char) {
        self.out.push(if c == '\n' { '\n' } else { ' ' });
    }

    fn code(&mut self, c: char) {
        match (c, self.chars.peek().copied()) {
            ('/', Some('/')) | ('/', Some('*')) => {
                self.state = if self.chars.next() == Some('/') { State::Line } else { State::Block };
                self.out.push_str("  ");
            }
            ('#', _) if self.at_start => {
                self.out.push(' ');
                self.state = State::Directive;
                self.continued = false;
            }
            ('"', _) => {
                self.out.push(c);
                self.state = State::Text;
            }
            ('\'', _) => {
                self.out.push(' ');
                self.state = State::Char;
            }
            _ => {
                self.out.push(c);
                self.at_start = c == '\n' || (self.at_start && c.is_whitespace());
            }
        }
    }

    fn line_comment(&mut self, c: char) {
        self.push_blank(c);
        if c == '\n' {
            self.state = State::Code;
            self.at_start = true;
        }
    }

    fn block_comment(&mut self, c: char) {
        if c == '*' && self.chars.peek() == Some(&'/') {
            self.chars.next();
            self.out.push_str("  ");
            self.state = State::Code;
            self.at_start = false;
        } else {
            self.push_blank(c);
        }
    }

    fn text(&mut self, c: char) -> Result<(), String> {
        if c == '\n' {
            return Err(format!("locmap.cpp: line {}: a string is not closed on its line", self.line));
        }
        self.out.push(c);
        if c == '\\' {
            self.out.extend(self.chars.next());
        } else if c == '"' {
            self.state = State::Code;
            self.at_start = false;
        }
        Ok(())
    }

    fn character(&mut self, c: char) -> Result<(), String> {
        if c == '\n' {
            return Err(format!("locmap.cpp: line {}: a character literal is not closed on its line", self.line));
        }
        self.out.push(' ');
        if c == '\\' {
            if self.chars.next().is_some() {
                self.out.push(' ');
            }
        } else if c == '\'' {
            self.state = State::Code;
            self.at_start = false;
        }
        Ok(())
    }

    fn directive(&mut self, c: char) {
        self.push_blank(c);
        if c == '\n' {
            if !self.continued {
                self.state = State::Code;
                self.at_start = true;
            }
            self.continued = false;
        } else if c == '\\' {
            self.continued = true;
        } else if !c.is_whitespace() {
            self.continued = false;
        }
    }

    fn step(&mut self, c: char) -> Result<(), String> {
        match self.state {
            State::Code => self.code(c),
            State::Line => self.line_comment(c),
            State::Block => self.block_comment(c),
            State::Text => self.text(c)?,
            State::Char => self.character(c)?,
            State::Directive => self.directive(c),
        }
        if c == '\n' {
            self.line += 1;
        }
        Ok(())
    }
}

/// The text with every comment, preprocessor line and character literal made into spaces (new lines kept, so a line number is still the line of the file).
/// A string literal is kept and must close on its line.
fn blank_noise(src: &str) -> Result<String, String> {
    let mut lexer =
        Lexer { chars: src.chars().peekable(), out: String::with_capacity(src.len()), state: State::Code, line: 1, at_start: true, continued: false };
    while let Some(c) = lexer.chars.next() {
        lexer.step(c)?;
    }
    if lexer.state == State::Block {
        return Err("locmap.cpp: a comment is not closed".to_owned());
    }
    Ok(lexer.out)
}

/// A cursor over the cleaned text, with the line of any place in it.
struct Cursor<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Cursor<'a> {
    fn line(&self, at: usize) -> usize {
        self.text.get(..at).map_or(1, |before| before.bytes().filter(|&b| b == b'\n').count() + 1)
    }

    fn fail(&self, at: usize, what: &str) -> String {
        format!("locmap.cpp: line {}: {what}", self.line(at))
    }

    fn rest(&self) -> &'a str {
        self.text.get(self.at..).unwrap_or("")
    }

    fn skip_space(&mut self) {
        self.at += self.rest().len() - self.rest().trim_start().len();
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_space();
        self.rest().chars().next()
    }

    fn expect(&mut self, wanted: char, context: &str) -> Result<(), String> {
        if self.peek() == Some(wanted) {
            self.at += wanted.len_utf8();
            Ok(())
        } else {
            Err(self.fail(self.at, &format!("expected `{wanted}` {context}")))
        }
    }

    /// Reads up to (not including) `stop`, at most `cap` bytes.
    fn until(&mut self, stop: char, cap: usize, context: &str) -> Result<&'a str, String> {
        self.skip_space();
        let rest = self.rest();
        let end = rest.find(stop).filter(|&end| end <= cap).ok_or_else(|| self.fail(self.at, &format!("expected `{stop}` {context}")))?;
        let token = rest.get(..end).unwrap_or("");
        self.at += end;
        Ok(token)
    }
}

fn parse_number(token: &str, cursor: &Cursor, at: usize) -> Result<u32, String> {
    let token = token.trim();
    token
        .strip_prefix("0x")
        .filter(|digits| !digits.is_empty() && digits.len() <= MAX_NUMBER_CHARS)
        .and_then(|digits| u32::from_str_radix(digits, 16).ok())
        .ok_or_else(|| cursor.fail(at, &format!("expected a number like 0x0409, found `{token}`")))
}

/// `{ {0x07, "de"}, {0x0c07, "de_AT"}, ... }` after `ILCID_POSIX_SUBTABLE(name)`.
fn parse_rows(cursor: &mut Cursor) -> Result<Vec<(u32, String)>, String> {
    cursor.expect('{', "to open the table")?;
    let mut rows = Vec::new();
    loop {
        match cursor.peek() {
            Some('}') => {
                cursor.at += 1;
                return Ok(rows);
            }
            Some('{') => {
                let start = cursor.at;
                cursor.at += 1;
                let number = parse_number(cursor.until(',', MAX_NUMBER_CHARS + 8, "after the number")?, cursor, start)?;
                cursor.at += 1;
                cursor.expect('"', "to open the id")?;
                let id = cursor.until('"', 128, "to close the id")?.to_owned();
                cursor.at += 1;
                cursor.expect('}', "to close the row")?;
                if cursor.peek() == Some(',') {
                    cursor.at += 1;
                }
                rows.push((number, id));
            }
            _ => return Err(cursor.fail(cursor.at, "expected a row `{number, \"id\"}` or `}`")),
        }
    }
}

/// The macro names and what each begins.
#[derive(Clone, Copy, PartialEq)]
enum Macro {
    Element,
    Subtable,
    Map,
}

fn next_macro(text: &str, from: usize) -> Option<(usize, Macro, usize)> {
    const NAMES: [(&str, Macro); 3] =
        [("ILCID_POSIX_ELEMENT_ARRAY", Macro::Element), ("ILCID_POSIX_SUBTABLE", Macro::Subtable), ("ILCID_POSIX_MAP", Macro::Map)];
    let rest = text.get(from..)?;
    NAMES
        .iter()
        .filter_map(|(name, kind)| {
            let mut search = 0;
            while let Some(found) = rest.get(search..).and_then(|tail| tail.find(name)) {
                let start = search + found;
                let before = rest.get(..start).and_then(|head| head.chars().next_back());
                let after = rest.get(start + name.len()..)?;
                if !before.is_some_and(|c| c.is_alphanumeric() || c == '_') && after.trim_start().starts_with('(') {
                    return Some((from + start, *kind, from + start + name.len()));
                }
                search = start + name.len();
            }
            None
        })
        .min_by_key(|(start, _, _)| *start)
}

fn parse_locmap(source: &str) -> Result<Icu, String> {
    let cleaned = blank_noise(source)?;
    let mut cursor = Cursor { text: &cleaned, at: 0 };
    let (mut tables, mut map) = (BTreeMap::new(), Vec::new());
    while let Some((start, kind, after)) = next_macro(cursor.text, cursor.at) {
        cursor.at = after;
        cursor.expect('(', "after the macro name")?;
        let args = cursor.until(')', 256, "to close the macro's arguments")?.to_owned();
        cursor.at += 1;
        let args: Vec<&str> = args.split(',').map(str::trim).collect();
        let table = match (kind, args.as_slice()) {
            (Macro::Map, [name]) => {
                map.push((*name).to_owned());
                continue;
            }
            (Macro::Subtable, [name]) => Subtable { name: (*name).to_owned(), rows: parse_rows(&mut cursor)? },
            (Macro::Element, [host, language, id]) => {
                let host = parse_number(host, &cursor, start)?;
                Subtable { name: (*language).to_owned(), rows: vec![(host & LANGUAGE_ID_MASK, (*language).to_owned()), (host, (*id).to_owned())] }
            }
            _ => return Err(cursor.fail(start, "a macro with the wrong number of arguments")),
        };
        if table.rows.is_empty() {
            return Err(cursor.fail(start, &format!("the table `{}` has no rows", table.name)));
        }
        if let Some(earlier) = tables.insert(table.name.clone(), table) {
            return Err(cursor.fail(start, &format!("the table `{}` is made twice", earlier.name)));
        }
    }
    if tables.is_empty() {
        return Err(
            "locmap.cpp: it holds no ILCID_POSIX_SUBTABLE or ILCID_POSIX_ELEMENT_ARRAY: this is not the file the tables are made from".to_owned()
        );
    }
    if let Some(missing) = map.iter().find(|name| !tables.contains_key(*name)) {
        return Err(format!("locmap.cpp: ILCID_POSIX_MAP({missing}) names a table that is not made"));
    }
    if map.is_empty() {
        return Err("locmap.cpp: it holds no ILCID_POSIX_MAP entries: the order of the tables is unknown".to_owned());
    }
    Ok(Icu { tables, map })
}

// ---- the table of numbers -----------------------------------------------------------------------------------------------------------

/// The tag of an ICU id (`zh_Hant_TW`) in canonical case, and whether CLDR also names its script, region and variants in English. `None` when the id is no
/// well-formed tag (`no_NO_NY`) or CLDR has no English name for its language (the retired `qut`, the old `iw`: ICU keeps them for round trips). A tag whose region is
/// not named (`sr-Cyrl-CS`: Serbia and Montenegro, which no longer exists) is paired with its number, so that a file made when it did reads; it is not one a person is
/// offered (`known`), as its name would hold the bare code.
fn eligible(id: &str, names: &Names) -> Option<(Parts, bool)> {
    let parts = parse_id(id, '_')?;
    if !names.languages.contains_key(&parts.language) {
        return None;
    }
    let complete = parts.script.as_ref().is_none_or(|s| names.scripts.contains_key(s))
        && parts.region.as_ref().is_none_or(|r| names.territories.contains_key(r))
        && parts.variants.iter().all(|v| names.variants.contains_key(v));
    Some((parts, complete))
}

struct Lcid {
    /// Number to tag: the tag ICU lists first for it whose language CLDR names, by `uprv_convertToPosix`'s subtable and order.
    from: BTreeMap<u16, String>,
    /// Tag to number: the subtable of the tag's own language first, then the table's order (`uprv_convertToLCID`).
    to: BTreeMap<String, u16>,
    /// The tags of `from` that CLDR names whole: the ones a person can pick.
    known: BTreeSet<String>,
}

fn language_of(table: &Subtable) -> String {
    table.rows.first().map(|(_, id)| id.split(['_', '@']).next().unwrap_or("").to_ascii_lowercase()).unwrap_or_default()
}

fn build_lcid(icu: &Icu, names: &Names) -> Lcid {
    let ordered: Vec<&Subtable> = icu.map.iter().filter_map(|name| icu.tables.get(name)).collect();
    let (mut from, mut known) = (BTreeMap::new(), BTreeSet::new());
    let mut seen = BTreeSet::new();
    for table in &ordered {
        let Some(&(language_id, _)) = table.rows.first() else { continue };
        // ICU reads a number in the first subtable whose language ID is the number's, and in that subtable only.
        if !seen.insert(language_id) {
            continue;
        }
        for (host, id) in &table.rows {
            if *host > MAX_LANGUAGE_ID || host & LANGUAGE_ID_MASK != language_id || NO_LANGUAGE_IDS.contains(&id.as_str()) {
                continue;
            }
            // A sort-order keyword (`es_ES@collation=traditional`) is no part of the language.
            let base = id.split('@').next().unwrap_or("");
            if let (Some((parts, complete)), Ok(number)) = (eligible(base, names), u16::try_from(*host))
                && let Entry::Vacant(slot) = from.entry(number)
            {
                if complete {
                    known.insert(parts.text.clone());
                }
                slot.insert(parts.text);
            }
        }
    }
    let mut to = BTreeMap::new();
    for own_language_first in [true, false] {
        for table in &ordered {
            let own = language_of(table);
            for (host, id) in &table.rows {
                if *host > MAX_LANGUAGE_ID || id.contains('@') || NO_LANGUAGE_IDS.contains(&id.as_str()) {
                    continue;
                }
                if let (Some((parts, _)), Ok(number)) = (eligible(id, names), u16::try_from(*host))
                    && (!own_language_first || parts.language == own)
                {
                    to.entry(parts.text).or_insert(number);
                }
            }
        }
    }
    Lcid { from, to, known }
}

// ---- the other tables ---------------------------------------------------------------------------------------------------------------

/// What `parent_chain` finds: the entry of the key, else of the key with its last subtag dropped, and so on, else `root`.
fn lookup<'a>(table: &'a BTreeMap<String, [String; 4]>, root: &'a [String; 4], key: &str) -> &'a [String; 4] {
    let mut key = key;
    loop {
        if let Some(found) = table.get(key) {
            return found;
        }
        match key.rfind('-') {
            Some(cut) => key = key.get(..cut).unwrap_or(""),
            None => return root,
        }
    }
}

/// The rows of `quotes.rs`: the locales whose marks differ from what the chain of their parent says, plus the keys the likely subtags add.
///
/// The JSON of a release is resolved, so a locale that says what its parent says is redundant, and the run-time lookup (the key, then the key without its last
/// subtag, then the root) gives the CLDR answer. CLDR names a script where a tag gives a region alone (`zh-TW` is `zh-Hant-TW`, no locale of the release), so the
/// keys `language-region` whose likely form adds a script are resolved through that form and kept where the chain would say something else.
fn quote_rows(cldr: &Cldr) -> Result<Vec<(String, [String; 4])>, String> {
    let mut resolved = cldr.marks.clone();
    for (key, full) in &cldr.likely {
        let (Some(id), Some(full)) = (parse_id(key, '-'), parse_id(full, '-')) else { continue };
        if id.script.is_some()
            || id.region.is_none()
            || !id.variants.is_empty()
            || id.language == "und"
            || full.script.is_none()
            || cldr.marks.contains_key(&id.text)
        {
            continue;
        }
        resolved.insert(id.text, lookup(&cldr.marks, &cldr.root, &full.text).clone());
    }
    let mut order: Vec<&String> = resolved.keys().collect();
    order.sort_by(|a, b| (a.matches('-').count(), *a).cmp(&(b.matches('-').count(), *b)));
    let mut kept: BTreeMap<String, [String; 4]> = BTreeMap::new();
    for key in order {
        let parent = key.rsplit_once('-').map_or("", |(parent, _)| parent);
        let inherited = lookup(&kept, &cldr.root, parent);
        if let Some(value) = resolved.get(key)
            && value != inherited
        {
            kept.insert(key.clone(), value.clone());
        }
    }
    for (key, value) in &resolved {
        if lookup(&kept, &cldr.root, key) != value {
            return Err(format!("internal error: the marks kept for `{key}` do not give what CLDR resolves for it"));
        }
    }
    Ok(kept.into_iter().collect())
}

/// The script of a likely full form (`zh-Hant-TW` gives `Hant`).
fn script_of(full: &str) -> Option<String> {
    parse_id(full, '-').and_then(|parts| parts.script)
}

/// The rows of `scripts.rs`: each language CLDR names in English with its likely script, and each `language-region` whose likely script is another one.
fn script_rows(cldr: &Cldr) -> BTreeMap<String, String> {
    let mut rows = BTreeMap::new();
    for (key, full) in &cldr.likely {
        let Some(script) = script_of(full) else { continue };
        match key.split_once('-') {
            None if cldr.names.languages.contains_key(key) => {
                rows.insert(key.clone(), script);
            }
            Some((language, region)) if is_region(region) && cldr.names.languages.contains_key(language) => {
                let own = cldr.likely.get(language).and_then(|full| script_of(full));
                if own.as_deref() != Some(script.as_str()) {
                    rows.insert(key.clone(), script);
                }
            }
            _ => {}
        }
    }
    rows
}

// ---- rendering ----------------------------------------------------------------------------------------------------------------------

/// Characters that are written as an escape, whatever they are: they are invisible, change the direction of the text around them, or are a kind of space.
fn is_format(c: char) -> bool {
    matches!(
        c,
        '\u{ad}' | '\u{600}'..='\u{605}' | '\u{61c}' | '\u{6dd}' | '\u{70f}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{206f}' | '\u{feff}' | '\u{fff9}'..='\u{fffb}' | '\u{e0000}'..='\u{e007f}'
    )
}

/// A Rust string literal: printable text as itself, every control, space or format character other than a plain space as `\u{..}`.
fn rust_str(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ' ' => out.push(' '),
            c if c.is_control() || c.is_whitespace() || is_format(c) => out.push_str(&format!("\\u{{{:x}}}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn marks_literal(marks: &[String; 4]) -> String {
    format!("[{}]", marks.iter().map(|m| rust_str(m)).collect::<Vec<_>>().join(", "))
}

/// The first lines of a file: the marker, the module doc and the sources.
fn header(what: &str, sources: &[String], rules: &[&str]) -> String {
    let mut out = String::from(
        "// @generated by `cargo xtask langdata` (xtask/src/langdata.rs). Do not edit it: change the generator or its sources, run it again and commit the result.\n",
    );
    out.push_str(&format!(
        "//! {what}\n//!\n//! Sources (data under the Unicode License V3, SPDX `Unicode-3.0`; the notices are in NOTICE and ATTRIBUTION.md):\n"
    ));
    for source in sources {
        out.push_str(&format!("//! - {source}\n"));
    }
    if !rules.is_empty() {
        out.push_str("//!\n");
        for rule in rules {
            out.push_str(&format!("//! {rule}\n"));
        }
    }
    out.push('\n');
    out
}

fn icu_source(pins: &Pins) -> String {
    format!("ICU4C `icu4c/source/common/locmap.cpp` at {} (SHA-256 {}), https://github.com/unicode-org/icu", pins.icu_release, pins.locmap_sha256)
}

fn cldr_source(pins: &Pins, what: &str) -> String {
    format!(
        "Unicode CLDR {} JSON ({what}), https://github.com/unicode-org/cldr-json, the release craft-i18n's data (`crates/i18n/data/source.txt`) was made from",
        pins.cldr_release
    )
}

fn render_lcid(lcid: &Lcid, pins: &Pins) -> String {
    let mut out = header(
        "LCID numbers and language tags: the pairs ICU4C's table holds, as `lang::from_lcid`, `lang::to_lcid` and `lang::known` read them.",
        &[icu_source(pins), cldr_source(pins, "cldr-localenames-full: the English names a tag must have")],
        &[
            "Rules (xtask/src/langdata.rs): a number is read as ICU reads it, in the first subtable whose language ID is the number's low ten bits, at the first of its rows that is a tag",
            "whose language CLDR names; a sort-order keyword is dropped (a number past 16 bits is a sort order stacked on a language ID, so it is no pair); the invariant and no-language",
            "entries are left out; a tag is looked up in the subtable of its own language first, then in the order of ICU's table. A tag whose region, script or variant CLDR has no name",
            "for (`sr-Cyrl-CS`) is paired but not listed in KNOWN.",
        ],
    );
    out.push_str("/// Number to tag: every language ID the table pairs, with the tag ICU lists first for it. Sorted by number.\n#[rustfmt::skip]\npub(super) const FROM_LCID: &[(u16, &str)] = &[\n");
    for (number, tag) in &lcid.from {
        out.push_str(&format!("    (0x{number:04x}, {}),\n", rust_str(tag)));
    }
    out.push_str("];\n\n/// Tag to number: every tag the table pairs, spellings that share a number included. Sorted by the bytes of the tag.\n#[rustfmt::skip]\npub(super) const TO_LCID: &[(&str, u16)] = &[\n");
    for (tag, number) in &lcid.to {
        out.push_str(&format!("    ({}, 0x{number:04x}),\n", rust_str(tag)));
    }
    out.push_str("];\n\n/// The tags a person can pick: those of `FROM_LCID` that CLDR names whole, once each. Sorted by the bytes of the tag.\n#[rustfmt::skip]\npub(super) const KNOWN: &[&str] = &[\n");
    for tag in &lcid.known {
        out.push_str(&format!("    {},\n", rust_str(tag)));
    }
    out.push_str("];\n");
    out
}

fn render_names(names: &Names, pins: &Pins) -> String {
    let mut out = header(
        "English names of languages, scripts, regions and variants, and CLDR's patterns that join them: `lang::english_name` writes `English (United Kingdom)` from them.",
        &[cldr_source(pins, "cldr-localenames-full/main/en: languages, scripts, territories, variants, localeDisplayPattern")],
        &[
            "Only the plain codes are here (a language of two or three letters, a script of four, a region of two letters or three digits, a variant keyed in lower case): CLDR's",
            "dialect names (`en-GB`) and alternates (`-alt-short`) are not. Each table is sorted by the bytes of its key.",
        ],
    );
    out.push_str(&format!(
        "/// CLDR's pattern for a language with what qualifies it.\npub(super) const LOCALE_PATTERN: &str = {};\n\n",
        rust_str(&names.pattern)
    ));
    out.push_str(&format!(
        "/// CLDR's pattern for two qualifiers (a script and a region).\npub(super) const LOCALE_SEPARATOR: &str = {};\n",
        rust_str(&names.separator)
    ));
    for (doc, name, map) in [
        ("Languages: the code and its English name.", "LANGUAGES", &names.languages),
        ("Scripts: the ISO 15924 code in title case and its English name.", "SCRIPTS", &names.scripts),
        ("Regions: the code and its English name.", "TERRITORIES", &names.territories),
        ("Variants: the code in lower case and its English name.", "VARIANTS", &names.variants),
    ] {
        out.push_str(&format!("\n/// {doc}\n#[rustfmt::skip]\npub(super) const {name}: &[(&str, &str)] = &[\n"));
        for (key, value) in map {
            out.push_str(&format!("    ({}, {}),\n", rust_str(key), rust_str(value)));
        }
        out.push_str("];\n");
    }
    out
}

fn render_quotes(rows: &[(String, [String; 4])], root: &[String; 4], pins: &Pins) -> String {
    let mut out = header(
        "Quotation marks by language: the outer pair and the inner pair, as `lang::quotes` reads them.",
        &[cldr_source(pins, "cldr-misc-full/main/*/delimiters.json and cldr-core/supplemental/likelySubtags.json")],
        &[
            "A row is a locale whose marks differ from what its parent says, and the keys `language-region` that CLDR gives a script (`zh-TW` is `zh-Hant-TW`). The lookup",
            "drops the last subtag of a tag until a row is found, and ends at ROOT, CLDR's own root and the English marks. Sorted by the bytes of the key.",
        ],
    );
    out.push_str(&format!(
        "/// CLDR's root: the marks of a language with none. Outer open and close, inner open and close.\npub(super) const ROOT: [&str; 4] = {};\n\n",
        marks_literal(root)
    ));
    out.push_str("/// The marks of each locale that differs from its parent: outer open and close, inner open and close.\n#[rustfmt::skip]\npub(super) const QUOTES: &[(&str, [&str; 4])] = &[\n");
    for (key, marks) in rows {
        out.push_str(&format!("    ({}, {}),\n", rust_str(key), marks_literal(marks)));
    }
    out.push_str("];\n");
    out
}

fn render_scripts(rows: &BTreeMap<String, String>, pins: &Pins) -> String {
    let mut out = header(
        "The likely script of a language, for `lang::script_of_language`.",
        &[cldr_source(pins, "cldr-core/supplemental/likelySubtags.json, for the languages cldr-localenames-full names")],
        &[
            "A row is a language and its likely script, or a `language-region` whose likely script is another one (`pa-PK` is Arabic where `pa` is Gurmukhi). Sorted by the bytes of the key.",
        ],
    );
    out.push_str("/// A language, or a language and a region, with the ISO 15924 code of its likely script.\n#[rustfmt::skip]\npub(super) const LIKELY_SCRIPT: &[(&str, &str)] = &[\n");
    for (key, script) in rows {
        out.push_str(&format!("    ({}, {}),\n", rust_str(key), rust_str(script)));
    }
    out.push_str("];\n");
    out
}

// ---- the whole ----------------------------------------------------------------------------------------------------------------------

/// Reads the two sources (after checking their release, licence and file) and gives the four generated files. Nothing is written and nothing is fetched; the same
/// sources give the same bytes.
pub fn generate(cldr: &Path, icu: &Path, pins: &Pins) -> Result<Files, String> {
    let cldr = read_cldr(cldr, pins)?;
    let icu = parse_locmap(&check_icu_source(icu, pins)?)?;
    let lcid = build_lcid(&icu, &cldr.names);
    if lcid.from.is_empty() || lcid.to.is_empty() {
        return Err("locmap.cpp: no pair of a number and a tag is left once the ids CLDR does not name are dropped".to_owned());
    }
    Ok(Files {
        lcid: render_lcid(&lcid, pins),
        names: render_names(&cldr.names, pins),
        quotes: render_quotes(&quote_rows(&cldr)?, &cldr.root, pins),
        scripts: render_scripts(&script_rows(&cldr), pins),
    })
}
