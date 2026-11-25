//! `cargo xtask brands`: no other vendor's product or company names in what users and agents read.
//!
//! DiaCraft names its own features; where a comparison is needed, text says "the reference app".
//! Scanned:
//! * string literals in Rust sources (command labels and params docs, menus, panels, MCP tool
//!   definitions…). Comments and test code (`tests*.rs`, `*_tests.rs`, `tests/` and `benches/`
//!   folders, items under `#[cfg(test)]`) and the Kani harness files (`kani_proofs.rs`) are skipped;
//! * the `description` of every `Cargo.toml`;
//! * every text file under `packaging/`, and under `docs/` except Markdown pages. Markdown docs
//!   (like `README.md`, `ROADMAP.md` and `docs/*.md`) may name the reference app and compare
//!   against it, so no Markdown is scanned;
//! * every text file of the website under `site/` (its pages, script, style and data; not its pictures). A page (an `.html` file) may name the
//!   product anywhere since the owner's ruling of 2026-11-25 ([`SITE_PAGE_NAMES`]); the company's name and the other vendors' are hits there
//!   as elsewhere, and so is the product's name in the site's other files.
//!
//! A line that has to keep a name (a file type or MIME type that another project defines) says
//! `brand-ok` in a comment on that line or, in a file listed in `MARK_ABOVE`, on a `#` comment line
//! just above it.
//!
//! **The uses the owner allows** (2026-11-23, "Labels + site/docs format lists"): the file-type labels of the dialogs, the lists of the
//! formats DiaCraft opens and saves in the site and the docs, and one trademark notice ([`TRADEMARK`]). Each is an exact text in the files it may be in
//! ([`PRODUCT_USES`]); a name inside one of them is no hit, and a name anywhere else is one. An entry whose file is there and holds
//! no line with its text is stale, and an error, so the list stays what the files say.
//!
//! A second scan is wider: the names of the suite DiaCraft follows ([`BANNED_NAMES`]: its brand, its link
//! domains and its app id prefix) may not be in any file of the repository, in any kind of text and any case.
//! The few lines that say DiaCraft is not that suite's product, or keep a copyright notice, are listed with
//! their text in [`NAME_EXCEPTIONS`].

use std::path::Path;

/// Vendor product and company names, lowercase, matched case-insensitively as whole words.
pub const DENY: &[&str] = &["microsoft", "visio", "lucidchart", "lucid"];

/// The names of [`DENY`] that a page of the website may hold anywhere and in any words (owner, 2026-11-25: "Yes use it for SEO with no restrictions", a ruling for the site's pages
/// only): the product's name. In a page ([`is_site_page`]) it is no hit; the company's name and the other vendors' still are, outside the trademark notice. The product's name stays
/// a hit in every other file (the site's style, script and data, the docs' text files, the program) and in a file's name.
pub const SITE_PAGE_NAMES: [&str; 1] = ["visio"];

/// Whether `path` (from the repository's root) is a page of the website: an `.html` file under `site/`.
pub fn is_site_page(path: &str) -> bool {
    path.starts_with("site/") && path.ends_with(".html")
}

/// The names of [`DENY`] that are hits in the file `path`: every one, but the [`SITE_PAGE_NAMES`] in a page of the website.
pub fn denied_in(path: &str) -> Vec<&'static str> {
    let page = is_site_page(path);
    DENY.iter().copied().filter(|name| !(page && SITE_PAGE_NAMES.contains(name))).collect()
}

/// Marks a line that keeps a name on purpose.
const ALLOW: &str = "brand-ok";

/// The notice, the same words wherever it is: NOTICE, the About box and the footer of the site's pages. Its first sentence is the trademark line it began with (an allowed use of the
/// product's name); the sentences after it say that OpenDocument Format is an OASIS Standard, that LibreOffice and OpenOffice are registered trademarks of their owners and that
/// DiaCraft is not affiliated with or endorsed by the four owners (owner, 2026-11-23). The whole of it is the allowed text, in one line of each file, so that the export's allowlist keeps it.
pub const TRADEMARK: &str = "Visio is a trademark of the Microsoft group of companies. OpenDocument Format (ODF) is an OASIS Standard. LibreOffice is a registered trademark of The Document Foundation. OpenOffice and Apache OpenOffice are registered trademarks of The Apache Software Foundation. DiaCraft is not affiliated with or endorsed by Microsoft, OASIS, The Document Foundation or The Apache Software Foundation.";

/// The Save dialog's kinds (the desktop's own dialog, the browser's chooser and the window's name card take their labels from it).
const SAVE_KINDS: &str = "crates/ui-egui/src/save_kinds.rs";
/// The desktop app's Open dialog.
const OPEN_DIALOG: &str = "apps/diacraft/src/dialogs.rs";
/// The command line's help.
const CLI_HELP: &str = "apps/diacraft-cli/src/args.rs";
/// The About box.
const ABOUT: &str = "crates/ui-egui/src/splash.rs";

/// The website's pages, each of which has the trademark line in its footer.
pub const SITE_PAGES: [&str; 14] = [
    "site/404.html",
    "site/architecture-diagrams.html",
    "site/browser.html",
    "site/changelog.html",
    "site/flowcharts.html",
    "site/guides/agents.html",
    "site/guides/command-line.html",
    "site/guides/file-formats.html",
    "site/guides/getting-started.html",
    "site/guides/index.html",
    "site/guides/native-format.html",
    "site/index.html",
    "site/privacy.html",
    "site/vsdx-editor.html",
];

/// The places that name the reference product on purpose, as (file, exact text): the owner's ruling of 2026-11-23 ("Labels + site/docs
/// format lists"), which changes the rule of 2026-11-07 (no mention at all) in these places only. An occurrence of a
/// denied name that lies inside an occurrence of a text listed for its file is no hit; any other occurrence is one. The texts are
/// case-sensitive and whole: a label is the label, a list is the list. An entry whose file is there and does not hold its text is stale.
///
/// `cargo xtask site` lets a page hold its entries and the [`SITE_PAGE_NAMES`] and nothing else, and the export's allowlist (`xtask/src/export/allowlist.txt`)
/// keeps the lines that hold them in the public tree (a test holds the two lists together): a page of the site whole, by a `free` entry, and any other
/// file's line by line.
pub const PRODUCT_USES: &[(&str, &str)] = &[
    // The file-type labels: the Save dialog's kinds, the Open dialog's filter and the list of types in the command line's help.
    (SAVE_KINDS, "Visio Drawing (.vsdx)"),
    (SAVE_KINDS, "Visio Macro-Enabled Drawing (.vsdm)"),
    (SAVE_KINDS, "Visio Stencil (.vssx)"),
    (SAVE_KINDS, "Visio Macro-Enabled Stencil (.vssm)"),
    (SAVE_KINDS, "Visio Template (.vstx)"),
    (SAVE_KINDS, "Visio Macro-Enabled Template (.vstm)"),
    (OPEN_DIALOG, "Visio and OpenDocument drawings, stencils and templates"),
    (CLI_HELP, "the Visio types .vsdx, .vsdm, .vssx, .vssm, .vstx and .vstm"),
    // The lists of the formats DiaCraft opens and saves, in the docs. The site's pages need none since the owner's ruling of 2026-11-25
    // ([`SITE_PAGE_NAMES`]): a page may name the product anywhere, and the notice in its footer, which names the company too, is the one
    // entry a page has.
    ("README.md", "Visio drawings, stencils and templates (`.vsdx`, `.vsdm`, `.vssx`, `.vssm`, `.vstx`, `.vstm`)"),
    ("docs/file-format-parity.md", "Visio drawings, stencils and templates (`.vsdx`, `.vsdm`, `.vssx`, `.vssm`, `.vstx`, `.vstm`)"),
    // The trademark line.
    ("NOTICE", TRADEMARK),
    (ABOUT, TRADEMARK),
    (SITE_PAGES[0], TRADEMARK),
    (SITE_PAGES[1], TRADEMARK),
    (SITE_PAGES[2], TRADEMARK),
    (SITE_PAGES[3], TRADEMARK),
    (SITE_PAGES[4], TRADEMARK),
    (SITE_PAGES[5], TRADEMARK),
    (SITE_PAGES[6], TRADEMARK),
    (SITE_PAGES[7], TRADEMARK),
    (SITE_PAGES[8], TRADEMARK),
    (SITE_PAGES[9], TRADEMARK),
    (SITE_PAGES[10], TRADEMARK),
    (SITE_PAGES[11], TRADEMARK),
    (SITE_PAGES[12], TRADEMARK),
    (SITE_PAGES[13], TRADEMARK),
];

/// The texts of [`PRODUCT_USES`] that `path` may hold.
pub fn allowed_in(path: &str) -> Vec<&'static str> {
    PRODUCT_USES.iter().filter(|(file, _)| *file == path).map(|(_, text)| *text).collect()
}

/// `text` with every occurrence of the `allowed` texts blanked (each byte of it a space), so that what is left is what the scan of
/// names judges. The texts are ASCII, so a blanked text keeps the places of the rest.
pub fn without_allowed(text: &str, allowed: &[&str]) -> String {
    let mut text = text.to_owned();
    for allowed in allowed.iter().filter(|a| !a.is_empty()) {
        if text.contains(allowed) {
            text = text.replace(allowed, &" ".repeat(allowed.len()));
        }
    }
    text
}

/// The entries of `uses` whose file is among `files` (paths relative to `root`) and does not hold its text: a reworded line, which the
/// entry would no longer describe. A file that cannot be read is an error.
pub fn stale_uses(root: &Path, files: &[String], uses: &[(&str, &str)]) -> Result<Vec<(String, String)>, String> {
    let mut stale = Vec::new();
    for (path, text) in uses {
        if !files.iter().any(|f| f == path) {
            continue;
        }
        let bytes = std::fs::read(root.join(path)).map_err(|e| format!("{path}: cannot read: {e}"))?;
        if !String::from_utf8_lossy(&bytes).contains(text) {
            stale.push(((*path).to_owned(), (*text).to_owned()));
        }
    }
    Ok(stale)
}

/// This file lists the names it rejects.
const SELF: &str = "xtask/src/brands.rs";

/// The lists that copy, whole, lines holding a banned name, each of which the scan of names reads in its own file: the export's
/// allowlist of the public tree (`cargo xtask export-public`). The scan of names does not read them, as it does not read this file.
pub const NAME_LISTS: [&str; 1] = ["xtask/src/export/allowlist.txt"];

/// Packaging files whose comments take whole lines (a desktop entry has no end-of-line comments):
/// in these, a `#` line marked `brand-ok` also covers the line just below it.
pub const MARK_ABOVE: &[&str] = &["packaging/linux/app.diacraft.desktop"];

/// The names of the project DiaCraft follows and is not part of, lowercase and matched anywhere in a line, in any case:
/// its brand, the domain of its site, its chat invitation links and the prefix of its app ids (AGENTS.md: no brand, link
/// or id anywhere). `getartcraft` holds `artcraft`; it is listed because the rule names a link to the site.
pub const BANNED_NAMES: &[&str] = &["artcraft", "getartcraft", "ai.storyteller", "discord.gg"];

/// The lines that may hold a banned name, as (file, text the line contains). Each is a statement that DiaCraft is not the
/// other project's product or may not use its marks, a deny list that checks for them, the notice of the copyright of
/// code that was copied (the licence keeps it), or a line of the design and the plans, which record what was decided. A new
/// line that holds a name is an error, not an entry here: change the line. An entry whose file is there and whose text is
/// not is stale, and an error too.
pub const NAME_EXCEPTIONS: &[(&str, &str)] = &[
    ("AGENTS.md", "independent project, not an ArtCraft product"),
    ("AGENTS.md", "No ArtCraft name, logo or link appears anywhere"),
    ("NOTICE", "ArtCraft Team and the respective app's contributors"),
    // The MIT texts of the two craft-libs checkouts the lock pins: `cargo xtask licenses` copies each licence text as the crate ships it, and the licence keeps the
    // copyright line (a line that is kept this way must stay as the licence has it, so it is listed here, not changed).
    ("THIRD-PARTY-LICENSES.txt", "ArtCraft Team and the craft-libs contributors"),
    ("THIRD-PARTY-LICENSES.txt", "ArtCraft Team and the PhotoCraft contributors"),
    ("THIRD-PARTY-LICENSES-WEB.txt", "ArtCraft Team and the craft-libs contributors"),
    ("apps/diacraft-web/tests/it/html.rs", "const OTHER_NAMES"),
];

/// Most bytes of a file the scan of names reads: a larger file is an error, not a pass.
const MOST_SCANNED_BYTES: u64 = 32 * 1024 * 1024;

/// A denied name found in a file.
#[derive(Debug, PartialEq)]
pub struct Hit {
    pub path: String,
    /// 1-based.
    pub line: usize,
    pub term: &'static str,
    /// The literal or line it was found in.
    pub context: String,
}

pub fn run(root: &Path) -> Result<(), String> {
    let files = crate::repo_files(root)?;
    let hits = scan(root, &files)?;
    let names = scan_names(root, &files, BANNED_NAMES, NAME_EXCEPTIONS)?;
    let uses = stale_uses(root, &files, PRODUCT_USES)?;
    if hits.is_empty() && names.hits.is_empty() && names.stale.is_empty() && uses.is_empty() {
        println!(
            "brands: no vendor names in user-visible text but the {} uses the owner allows, and none of {} in any file ({} lines excepted)",
            PRODUCT_USES.len(),
            BANNED_NAMES.join(", "),
            names.exempt.len()
        );
        return Ok(());
    }
    let list = |hits: &[Hit]| {
        hits.iter().map(|h| format!("{}:{}: \"{}\" in {}", h.path, h.line, h.term, short(&h.context))).collect::<Vec<_>>().join("\n  ")
    };
    let mut problems = Vec::new();
    if !hits.is_empty() {
        problems.push(format!(
            "{} vendor name(s) in user-visible text (say \"the reference app\" or name the feature; mark a required file or MIME type name `{ALLOW}`; the uses the owner \
             allows are PRODUCT_USES in xtask/src/brands.rs):\n  {}",
            hits.len(),
            list(&hits)
        ));
    }
    if !names.hits.is_empty() {
        problems.push(format!(
            "{} name(s) of another project (AGENTS.md: no brand, link or app id of it anywhere; change the line, or if it says DiaCraft is not that project's \
             product add it to NAME_EXCEPTIONS in xtask/src/brands.rs):\n  {}",
            names.hits.len(),
            list(&names.hits)
        ));
    }
    if !names.stale.is_empty() {
        let stale: Vec<String> = names.stale.iter().map(|(path, text)| format!("{path}: no line contains \"{}\"", short(text))).collect();
        problems.push(format!("{} entries of NAME_EXCEPTIONS match no line (remove or update them):\n  {}", stale.len(), stale.join("\n  ")));
    }
    if !uses.is_empty() {
        let stale: Vec<String> = uses.iter().map(|(path, text)| format!("{path}: does not hold \"{}\"", short(text))).collect();
        problems.push(format!(
            "{} entries of PRODUCT_USES are not in their files (update the entry, and the export's allowlist, with the line):\n  {}",
            stale.len(),
            stale.join("\n  ")
        ));
    }
    Err(problems.join("\n"))
}

/// What the scan of names found.
#[derive(Debug, Default)]
pub struct Names {
    /// A banned name on a line that no exception covers.
    pub hits: Vec<Hit>,
    /// A banned name on a line that an exception covers: what the list lets through, for the record.
    pub exempt: Vec<Hit>,
    /// The (file, text) of each exception whose file is among the files and none of whose lines holds its text.
    pub stale: Vec<(String, String)>,
}

/// The lines of `files` (paths relative to `root`) that hold one of the `banned` names in any case, split by whether one of the
/// `exceptions` (file, text) covers them: an exception covers a line of its file that contains its text. A file that cannot be
/// read, or is larger than 32 MiB, is an error and not a pass. Files are read as text, lossily: a name is ASCII in any encoding.
pub fn scan_names(root: &Path, files: &[String], banned: &[&'static str], exceptions: &[(&str, &str)]) -> Result<Names, String> {
    let mut names = Names::default();
    let mut used = vec![false; exceptions.len()];
    for path in files.iter().filter(|path| path.as_str() != SELF && !NAME_LISTS.contains(&path.as_str())) {
        let file = root.join(path);
        let size = std::fs::metadata(&file).map_err(|e| format!("{path}: cannot read: {e}"))?.len();
        if size > MOST_SCANNED_BYTES {
            return Err(format!("{path}: is {size} bytes, more than the {MOST_SCANNED_BYTES} the scan of names reads"));
        }
        let bytes = std::fs::read(&file).map_err(|e| format!("{path}: cannot read: {e}"))?;
        let text = String::from_utf8_lossy(&bytes);
        for (n, line) in text.lines().enumerate() {
            let lower = line.to_ascii_lowercase();
            let terms: Vec<&'static str> = banned.iter().copied().filter(|name| lower.contains(name)).collect();
            if terms.is_empty() {
                continue;
            }
            let covering: Vec<usize> =
                (0..exceptions.len()).filter(|&i| exceptions.get(i).is_some_and(|(p, t)| p == path && line.contains(t))).collect();
            for &i in &covering {
                if let Some(flag) = used.get_mut(i) {
                    *flag = true;
                }
            }
            for term in terms {
                let hit = Hit { path: path.clone(), line: n + 1, term, context: line.trim().to_string() };
                if covering.is_empty() { names.hits.push(hit) } else { names.exempt.push(hit) }
            }
        }
    }
    for (i, (path, text)) in exceptions.iter().enumerate() {
        if !used.get(i).copied().unwrap_or(false) && files.iter().any(|f| f == path) {
            names.stale.push(((*path).to_owned(), (*text).to_owned()));
        }
    }
    Ok(names)
}

/// Every hit in `files` (paths relative to `root`). A file of a scanned kind that cannot be read is an
/// error, not a pass: a gate must not skip what it cannot see. Text files (packaging, docs) may be in
/// another encoding or be binary, and are scanned lossily; Rust sources and manifests must be UTF-8.
pub fn scan(root: &Path, files: &[String]) -> Result<Vec<Hit>, String> {
    let mut hits = vec![];
    for path in files {
        let Some(kind) = kind_of(path) else { continue };
        let bytes = std::fs::read(root.join(path)).map_err(|e| format!("{path}: cannot read: {e}"))?;
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(e) if kind == Kind::Text => String::from_utf8_lossy(e.as_bytes()).into_owned(),
            Err(_) => return Err(format!("{path}: is not valid UTF-8, which Rust sources and manifests must be")),
        };
        hits.extend(scan_text(path, &text));
    }
    Ok(hits)
}

/// The hits in one file's `text`; what is scanned depends on `path` (see [`kind_of`]), and a name inside a use [`PRODUCT_USES`] allows in
/// `path` is none.
pub fn scan_text(path: &str, text: &str) -> Vec<Hit> {
    scan_text_with(path, text, &allowed_in(path))
}

/// [`scan_text`] with the texts `allowed` in place of what [`PRODUCT_USES`] allows in `path`: a name inside an occurrence of one of them
/// (in a literal, or in a line) is no hit.
pub fn scan_text_with(path: &str, text: &str, allowed: &[&str]) -> Vec<Hit> {
    let Some(kind) = kind_of(path) else { return vec![] };
    let denied = denied_in(path);
    let found = match kind {
        Kind::Rust => scan_rust(text, allowed, &denied),
        Kind::Manifest => scan_lines(text, |l| l.trim_start().starts_with("description"), false, allowed, &denied),
        Kind::Text => scan_lines(text, |_| true, MARK_ABOVE.contains(&path), allowed, &denied),
    };
    found.into_iter().map(|(line, term, context)| Hit { path: path.to_owned(), line, term, context }).collect()
}

/// How a file is scanned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// String literals outside comments and test code.
    Rust,
    /// `description` lines.
    Manifest,
    /// Every line.
    Text,
}

/// The extensions of the website's text files (its pages, script, style, data, robots and sitemap); `_headers` is one too.
const SITE_TEXT: [&str; 7] = ["html", "css", "js", "json", "txt", "xml", "webmanifest"];

/// How `path` is scanned, or `None` when it is not (Markdown, tests, pictures, everything else).
pub fn kind_of(path: &str) -> Option<Kind> {
    if path.ends_with(".md") {
        None
    } else if path.starts_with("docs/") || path.starts_with("packaging/") {
        Some(Kind::Text)
    } else if let Some(site) = path.strip_prefix("site/") {
        let name = site.rsplit('/').next().unwrap_or_default();
        let text = name == "_headers" || name.rsplit_once('.').is_some_and(|(_, extension)| SITE_TEXT.contains(&extension));
        text.then_some(Kind::Text)
    } else if path == "Cargo.toml" || path.ends_with("/Cargo.toml") {
        Some(Kind::Manifest)
    } else if path.ends_with(".rs") && ["crates/", "apps/", "xtask/"].iter().any(|d| path.starts_with(d)) && path != SELF && !is_test_path(path) {
        Some(Kind::Rust)
    } else {
        None
    }
}

/// Whether `path` is test code by its place or name: a `tests.rs`, `tests_*` or `*_tests.rs` file, a Kani harness file (`kani_proofs.rs`,
/// behind `#[cfg(kani)]`, which no ordinary build compiles), or anything under a `tests` or `benches` folder.
pub fn is_test_path(path: &str) -> bool {
    let mut parts = path.split('/').rev();
    let file = parts.next().unwrap_or_default();
    file == "tests.rs"
        || file == crate::kani::HARNESS_FILE
        || file.starts_with("tests_")
        || file.ends_with("_tests.rs")
        || parts.any(|d| d == "tests" || d == "benches")
}

/// (line, term, line text) for each name of `denied` on the lines `keep` selects, but those marked
/// `brand-ok` on the line or, with `mark_above`, on a `#` comment line just above, and those inside an occurrence of an `allowed` text.
fn scan_lines(
    text: &str,
    keep: impl Fn(&str) -> bool,
    mark_above: bool,
    allowed: &[&str],
    denied: &[&'static str],
) -> Vec<(usize, &'static str, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let marked_above =
        |i: usize| mark_above && i.checked_sub(1).and_then(|j| lines.get(j)).is_some_and(|l| l.trim_start().starts_with('#') && l.contains(ALLOW));
    lines
        .iter()
        .enumerate()
        .filter(|&(i, l)| keep(l) && !l.contains(ALLOW) && !marked_above(i))
        .flat_map(|(i, l)| terms_in(&without_allowed(l, allowed), denied).into_iter().map(move |t| (i + 1, t, l.trim().to_string())))
        .collect()
}

/// (first line, term, literal) for each name of `denied` in a Rust string literal outside comments and
/// test code, but inside an occurrence of an `allowed` text.
fn scan_rust(src: &str, allowed: &[&str], denied: &[&'static str]) -> Vec<(usize, &'static str, String)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = vec![];
    for lit in rust_literals(src) {
        let covered = lines.get(lit.first.saturating_sub(1)..lit.last.min(lines.len()));
        if covered.is_some_and(|ls| ls.iter().any(|l| l.contains(ALLOW))) {
            continue;
        }
        for t in terms_in(&without_allowed(&lit.text, allowed), denied) {
            out.push((lit.first, t, lit.text.clone()));
        }
    }
    out
}

/// The `terms` (lowercase) that occur in `text` as whole words, ignoring case.
fn terms_in(text: &str, terms: &[&'static str]) -> Vec<&'static str> {
    let lower = text.to_ascii_lowercase();
    let b = lower.as_bytes();
    let word = |c: Option<&u8>| c.is_some_and(u8::is_ascii_alphanumeric);
    terms
        .iter()
        .copied()
        .filter(|t| lower.match_indices(t).any(|(i, _)| !word(i.checked_sub(1).and_then(|j| b.get(j))) && !word(b.get(i + t.len()))))
        .collect()
}

fn short(s: &str) -> String {
    let s = s.replace('\n', " ");
    match s.char_indices().nth(100) {
        Some((i, _)) => format!("{}…", s.get(..i).unwrap_or_default()),
        None => s,
    }
}

/// A Rust string literal: its first and last line (1-based) and its text (escapes as spaces).
#[derive(Debug)]
struct Lit {
    first: usize,
    last: usize,
    text: String,
}

/// String literals of a Rust source, skipping comments and items under `#[cfg(test)]`.
fn rust_literals(src: &str) -> Vec<Lit> {
    let b = src.as_bytes();
    let mut out = vec![];
    let (mut i, mut line) = (0usize, 1usize);
    let mut depth = 0usize;
    // `#[cfg(test)]` seen, its item not opened yet (the paren and bracket depth it was seen at); brace
    // depth the test item was opened at.
    let (mut test_attr, mut test_depth) = (None::<usize>, None::<usize>);
    let mut parens = 0usize;
    while i < b.len() {
        match b[i] {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let mut nest = 0usize;
                while i < b.len() {
                    if b[i..].starts_with(b"/*") {
                        nest += 1;
                        i += 2;
                    } else if b[i..].starts_with(b"*/") {
                        nest = nest.saturating_sub(1);
                        i += 2;
                        if nest == 0 {
                            break;
                        }
                    } else {
                        line += usize::from(b[i] == b'\n');
                        i += 1;
                    }
                }
            }
            b'"' => {
                let first = line;
                let (text, end) = quoted(b, i + 1, &mut line);
                i = end;
                if test_depth.is_none() {
                    out.push(Lit { first, last: line, text });
                }
            }
            b'\'' => i = after_char_or_lifetime(src, i),
            b'#' if b[i..].starts_with(b"#[cfg(test)]") => {
                test_attr = Some(parens);
                i += "#[cfg(test)]".len();
            }
            b'{' => {
                if test_attr.is_some() && test_depth.is_none() {
                    test_depth = Some(depth);
                }
                test_attr = None;
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                if test_depth == Some(depth) {
                    test_depth = None;
                }
                // An attribute still waiting for its block belonged to the last field, variant or arm of
                // the scope that just closed.
                test_attr = None;
                i += 1;
            }
            b'(' | b'[' => {
                parens += 1;
                i += 1;
            }
            b')' | b']' => {
                parens = parens.saturating_sub(1);
                i += 1;
            }
            // A field, variant, match arm or argument ends at its comma; the commas of its own
            // parentheses, brackets and generics do not (a wrong reset only reports more, never less).
            b',' => {
                if test_attr == Some(parens) {
                    test_attr = None;
                }
                i += 1;
            }
            b';' => {
                test_attr = None;
                i += 1;
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                let hashes = b[i..].iter().take_while(|&&h| h == b'#').count();
                // Raw strings (r"…", r#"…"#, br…, cr…); b"…" and c"…" are read as plain strings next.
                if matches!(src.get(start..i), Some("r" | "br" | "cr")) && b.get(i + hashes) == Some(&b'"') {
                    let first = line;
                    let body = i + hashes + 1;
                    let close: Vec<u8> = std::iter::once(b'"').chain(std::iter::repeat_n(b'#', hashes)).collect();
                    let end = (body..b.len()).find(|&j| b[j..].starts_with(&close)).unwrap_or(b.len());
                    let text = String::from_utf8_lossy(b.get(body..end).unwrap_or_default()).into_owned();
                    line += text.matches('\n').count();
                    i = (end + close.len()).min(b.len());
                    if test_depth.is_none() {
                        out.push(Lit { first, last: line, text });
                    }
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// Read a `"…"` literal whose body starts at `i`: (text with escapes as spaces, index after the
/// closing quote).
fn quoted(b: &[u8], mut i: usize, line: &mut usize) -> (String, usize) {
    let mut text = Vec::new();
    while i < b.len() && b[i] != b'"' {
        if b[i] == b'\\' {
            match b.get(i + 1) {
                Some(&c @ (b'"' | b'\\' | b'\'')) => text.push(c),
                Some(b'\n') => {
                    *line += 1;
                    text.push(b' ');
                }
                _ => text.push(b' '),
            }
            i += 2;
        } else {
            *line += usize::from(b[i] == b'\n');
            text.push(b[i]);
            i += 1;
        }
    }
    (String::from_utf8_lossy(&text).into_owned(), (i + 1).min(b.len()))
}

/// Skip a char literal (`'x'`, `'\n'`, `'\u{…}'`) at `i`, or just the quote of a lifetime/label.
fn after_char_or_lifetime(src: &str, i: usize) -> usize {
    let b = src.as_bytes();
    if b.get(i + 1) == Some(&b'\\') {
        let close = (i + 3..b.len()).find(|&j| b[j] == b'\'').unwrap_or(b.len());
        return (close + 1).min(b.len());
    }
    match src.get(i + 1..).and_then(|rest| rest.chars().next()) {
        Some(c) if b.get(i + 1 + c.len_utf8()) == Some(&b'\'') => i + 2 + c.len_utf8(),
        _ => i + 1,
    }
}
