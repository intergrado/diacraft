//! Progress docs damaged by merges: a table row that appears twice, a status line (`**Change:**
//! kind (…; …)`) that lists the same item twice, or a doc with more than one status line (stacked
//! status lines are merge damage too; docs/progress-doc-templates.md). All happen when a merge
//! keeps both sides of a conflict; they are never wanted.

use std::collections::HashMap;
use std::path::Path;

/// A status line starts with one of these.
const STATUS_STARTS: &[&str] = &["> **Last reviewed:**", "> **Last updated:**"];

/// Checks `ROADMAP.md` and `docs/*.md` under `root` (not the folders below `docs/`); prints each problem and fails
/// if there were any. No page is left out.
pub fn run(root: &Path) -> Result<(), String> {
    let found = check(root);
    if found.is_empty() {
        println!("docs: ok");
        return Ok(());
    }
    Err(format!("docs: repeats left by a merge, or docs that cannot be read:\n  {}", found.join("\n  ")))
}

/// `file:line: what` for each repeat in the checked docs, and `file: why` for each doc or folder that
/// cannot be read. A gate that cannot read a doc has not checked it, so only a `ROADMAP.md` or a
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
            Ok(text) => found.extend(repeats(&text).into_iter().map(|r| format!("{name}:{r}"))),
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

/// `line: what` for each table row that repeats an earlier one (header rows apart: tables share
/// them), each status-line item listed twice, and each status line after the first.
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
        if STATUS_STARTS.iter().any(|s| l.starts_with(s)) {
            match first_status {
                Some(first) => {
                    out.push(format!("{}: a second status line (the first is on line {first}); keep one and replace it on every edit", i + 1))
                }
                None => first_status = Some(i + 1),
            }
        }
        if let Some(body) = status_items(l) {
            let mut seen = Vec::new();
            for item in body {
                if seen.contains(&item) {
                    out.push(format!("{}: status item listed twice: {}", i + 1, item.chars().take(100).collect::<String>()));
                } else {
                    seen.push(item);
                }
            }
        }
    }
    out
}

/// The items of a status line's `**Change:** kind (…)`, split at `; ` outside brackets. `None`
/// for other lines, or when the brackets never close.
fn status_items(line: &str) -> Option<Vec<&str>> {
    let open = line.find("**Change:**").and_then(|i| line.get(i..)?.find('(').map(|o| i + o))?;
    let mut depth = 0usize;
    let mut items = Vec::new();
    let mut start = open + 1;
    for (j, c) in line.char_indices().skip_while(|(j, _)| *j < open) {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    items.push(line.get(start..j)?);
                    return Some(items);
                }
            }
            ';' if depth == 1 && line.get(j..)?.starts_with("; ") => {
                items.push(line.get(start..j)?);
                start = j + 2;
            }
            _ => {}
        }
    }
    None
}
