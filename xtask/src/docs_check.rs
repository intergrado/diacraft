//! The progress docs' status lines, and what merges leave in the docs (docs/progress-doc-templates.md).
//!
//! A merge that keeps both sides of a conflict leaves a table row twice, or a second status line; neither is ever wanted. The
//! status line itself belongs to the scored docs ([`SCORED`]) alone, and holds the date their scores were last checked against
//! evidence and nothing else: `> **Last reviewed:** YYYY-MM-DD`. Git records when a doc changed and what changed, so a `Last
//! updated` date, a list of changes or a history in a status line is refused, and so is a status line in any other doc.

use std::collections::HashMap;
use std::path::Path;

/// A status line starts with one of these.
const STATUS_STARTS: &[&str] = &["> **Last reviewed:**", "> **Last updated:**"];

/// What a scored doc's status line holds before its date.
const REVIEWED: &str = "> **Last reviewed:** ";

/// The docs with scores, by their path from the root: each may have one status line, its review date. Every other doc has none.
pub const SCORED: &[&str] = &[
    "ROADMAP.md",
    "docs/gaps.md",
    "docs/roadmap.md",
    "docs/parity-checklist.md",
    "docs/parity-evidence.md",
    "docs/target-app-parity.md",
    "docs/ui-parity.md",
    "docs/file-format-parity.md",
    "docs/hardware-parity.md",
    "docs/localization-parity.md",
];

/// Checks `ROADMAP.md` and `docs/*.md` under `root` (not the folders below `docs/`); prints each problem and fails
/// if there were any. No page is left out.
pub fn run(root: &Path) -> Result<(), String> {
    let found = check(root);
    if found.is_empty() {
        println!("docs: ok");
        return Ok(());
    }
    Err(format!("docs: repeats left by a merge, status lines the docs may not have, or docs that cannot be read:\n  {}", found.join("\n  ")))
}

/// `file:line: what` for each repeat and each status line out of place or out of form in the checked docs, and `file: why` for
/// each doc or folder that cannot be read. A gate that cannot read a doc has not checked it, so only a `ROADMAP.md` or a
/// `docs/` that does not exist (yet) is let through; a dangling link is not an absent file.
pub fn check(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut files = Vec::new();
    if exists(&root.join("ROADMAP.md"), "ROADMAP.md", &mut found) {
        files.push(root.join("ROADMAP.md"));
    }
    if exists(&root.join("docs"), "docs", &mut found) {
        match std::fs::read_dir(root.join("docs")) {
            Ok(rd) => {
                let mut docs = Vec::new();
                for entry in rd {
                    match entry {
                        Ok(e) if e.path().extension().is_some_and(|x| x == "md") => docs.push(e.path()),
                        Ok(_) => {}
                        Err(e) => found.push(format!("docs: cannot list: {e}")),
                    }
                }
                docs.sort();
                files.extend(docs);
            }
            Err(e) => found.push(format!("docs: cannot list: {e}")),
        }
    }
    for f in &files {
        let name = f.strip_prefix(root).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        match std::fs::read_to_string(f) {
            Ok(text) => {
                found.extend(repeats(&text).into_iter().map(|r| format!("{name}:{r}")));
                found.extend(status_lines(&name, &text).into_iter().map(|r| format!("{name}:{r}")));
            }
            Err(e) => found.push(format!("{name}: cannot read: {e}")),
        }
    }
    found
}

/// Whether `path` is there to be read. `Ok(false)` only for a path that does not exist at all (a link
/// is looked at, not followed); any other failure is a finding for `name`, and the path is not read.
fn exists(path: &Path, name: &str, found: &mut Vec<String>) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => {
            found.push(format!("{name}: cannot read: {e}"));
            false
        }
    }
}

/// Whether `line` is a status line: a quote that starts with a `Last reviewed` or a `Last updated` date.
fn is_status(line: &str) -> bool {
    STATUS_STARTS.iter().any(|s| line.starts_with(s))
}

/// `line: what` for each table row that repeats an earlier one (header rows apart: tables share
/// them), and each status line after the first.
pub fn repeats(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let separator = |l: &str| l.starts_with('|') && l.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '));
    let mut out = Vec::new();
    let mut rows: HashMap<&str, usize> = HashMap::new();
    let mut first_status: Option<usize> = None;
    for (i, l) in lines.iter().enumerate() {
        let header = lines.get(i + 1).is_some_and(|n| separator(n));
        if l.starts_with('|') && !separator(l) && !header {
            match rows.get(l) {
                Some(first) => out.push(format!("{}: the table row of line {first} again", i + 1)),
                None => {
                    rows.insert(l, i + 1);
                }
            }
        }
        if is_status(l) {
            match first_status {
                Some(first) => out.push(format!("{}: a second status line (the first is on line {first}); a doc has one at most", i + 1)),
                None => first_status = Some(i + 1),
            }
        }
    }
    out
}

/// `line: what` for each status line of the doc `name` (its path from the root, with `/`) that the doc may not have: one that
/// holds a `Last updated` date, a `**Change:**` list or a history (`before that`), whatever the doc; one in a doc that is not in
/// [`SCORED`]; and, in a scored doc, one that is anything but `> **Last reviewed:** YYYY-MM-DD`.
pub fn status_lines(name: &str, text: &str) -> Vec<String> {
    let scored = SCORED.contains(&name);
    let mut out = Vec::new();
    for (i, l) in text.lines().enumerate().filter(|(_, l)| is_status(l)) {
        let n = i + 1;
        let mut refused = false;
        if l.contains("**Last updated:**") {
            out.push(format!(
                "{n}: a `Last updated` date in the status line: git records when a doc changed, and a status line holds only a scored doc's review date"
            ));
            refused = true;
        }
        if l.contains("**Change:**") || l.to_ascii_lowercase().contains("before that") {
            out.push(format!("{n}: a change list or a history in the status line: git records what changed, and the revision table says it"));
            refused = true;
        }
        if !scored {
            out.push(format!(
                "{n}: a status line in a doc that has no scores: only the scored progress docs have one, and git records the history of the others"
            ));
        } else if !refused && !l.strip_prefix(REVIEWED).is_some_and(is_date) {
            out.push(format!(
                "{n}: a status line that is not `> **Last reviewed:** YYYY-MM-DD` alone: it holds the date the doc's scores were last checked against evidence, and nothing else"
            ));
        }
    }
    out
}

/// Whether `s` is a date written `YYYY-MM-DD`: ten characters, digits with a hyphen after the fourth and the sixth. The
/// calendar is not checked.
fn is_date(s: &str) -> bool {
    s.len() == 10 && s.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() })
}
