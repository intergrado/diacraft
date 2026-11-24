//! `cargo xtask docs`: progress docs damaged by merges. The repeat checks are FigCraft's; the
//! stacked-status-line check is DiaCraft's own (docs/progress-doc-templates.md).

use xtask::docs_check::{check, repeats, run};

const STATUS: &str = "> **Last updated:** 2026-10-31 · **Change:** minor (a; b)";

#[test]
fn repeated_table_row_is_reported() {
    let doc = "| Item | Status |\n|---|---|\n| one | done |\n| two | done |\n\n| Item | Status |\n|---|---|\n| one | done |\n";
    let found = repeats(doc);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("8: the table row of line 3 again"), "{found:?}");
}

#[test]
fn table_headers_are_not_repeats() {
    // Tables share header rows, and their separator rows are alike.
    assert!(repeats("| A | B |\n|---|---|\n| 1 | 2 |\n\n| A | B |\n|---|---|\n| 3 | 4 |\n").is_empty());
}

#[test]
fn repeated_change_item_is_reported() {
    let doc = "> **Last updated:** 2026-10-31 · **Change:** minor (a (x; y); b; a (x; y)) · **Target:** t\n";
    let found = repeats(doc);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("1: status item listed twice: a (x; y)"), "{found:?}");
    // Distinct items, and a parenthesis that never closes, are left alone.
    assert!(repeats("**Change:** minor (a; b (c; c)) · x\n| a | b |\n| a | c |\n").is_empty());
    assert!(repeats("**Change:** minor (a; a (never closed\n").is_empty());
}

#[test]
fn two_status_lines_in_one_doc_are_reported() {
    for second in ["> **Last updated:** 2026-10-30 · **Change:** minor (c)", "> **Last reviewed:** 2026-10-30 · **Change:** minor (c)"] {
        let found = repeats(&format!("# Title\n\n{STATUS}\n{second}\n\ntext\n"));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("4: a second status line (the first is on line 3)"), "{found:?}");
    }
    // Three lines: both extra ones are named.
    let three = format!("{STATUS}\ntext\n{STATUS}\n> **Last reviewed:** 2026-10-29\n");
    let found = repeats(&three);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].starts_with("3: a second status line (the first is on line 1)"), "{found:?}");
    assert!(found[1].starts_with("4: a second status line (the first is on line 1)"), "{found:?}");
}

#[test]
fn one_status_line_passes() {
    assert!(repeats(&format!("# Title\n\n{STATUS}\n\ntext\n")).is_empty());
    // `Last reviewed` is optional: a status line without it is valid (docs/provenance.md).
    assert!(repeats("> **Last updated:** 2026-10-31 · **Change:** major (created)\n").is_empty());
    // One line with both dates is still one line.
    assert!(repeats("> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-24 · **Change:** minor (x) · **Target:** t\n").is_empty());
    // Other quotes are not status lines.
    assert!(repeats("> **Note:** a\n> **Note:** b\n> a **Last updated:** mention\n").is_empty());
}

/// The reference notes that once sat in `docs/` have left the repository, and the gate keeps no list of docs it does not
/// read: whatever page is in `docs/` is checked, whatever it is called.
#[test]
fn every_page_in_docs_is_checked_even_one_named_like_a_note_that_used_to_be_skipped() {
    let quoting = format!("{STATUS}\n{STATUS}\n| a | b |\n| a | b |\n| a | b |\n");
    for name in ["docs/craft-standards.md", "docs/craft-architecture-notes.md", "docs/gaps.md"] {
        let dir = crate::TempDir::new("docs-no-skip");
        dir.write(name, &quoting);
        let found = check(dir.path());
        assert!(!found.is_empty() && found.iter().all(|f| f.starts_with(&format!("{name}:"))), "{name}: {found:?}");
        assert!(run(dir.path()).is_err(), "{name}");
    }
}

#[test]
fn only_the_root_roadmap_and_docs_pages_are_scanned() {
    let bad = format!("{STATUS}\n{STATUS}\n");
    let dir = crate::TempDir::new("docs-scope");
    dir.write("docs/notes/plans/design.md", &bad);
    dir.write("docs/notes.txt", &bad);
    dir.write("notes/README.md", &bad);
    dir.write("README.md", &bad);
    assert!(check(dir.path()).is_empty(), "{:?}", check(dir.path()));
    dir.write("ROADMAP.md", &bad);
    dir.write("docs/provenance.md", &bad);
    let found = check(dir.path());
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].starts_with("ROADMAP.md:") && found[1].starts_with("docs/provenance.md:"), "{found:?}");
}

/// A doc the gate cannot read is a finding, not a pass: only a `ROADMAP.md` or `docs/` that does not
/// exist (yet) is tolerated.
#[test]
fn a_doc_that_is_not_utf8_is_reported() {
    let dir = crate::TempDir::new("docs-latin1");
    // Two status lines (a finding on their own) and one Latin-1 byte.
    let mut text = format!("{STATUS}\n{STATUS}\ncaf").into_bytes();
    text.extend_from_slice(b"\xe9\n");
    dir.write_bytes("docs/gaps.md", &text);
    let found = check(dir.path());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("docs/gaps.md:") && found[0].contains("cannot read"), "{found:?}");
    assert!(run(dir.path()).is_err());
}

#[test]
fn an_unreadable_roadmap_or_docs_folder_is_reported() {
    // A directory where the file should be.
    let dir = crate::TempDir::new("docs-roadmap-dir");
    std::fs::create_dir_all(dir.path().join("ROADMAP.md")).unwrap();
    let found = check(dir.path());
    assert!(found.len() == 1 && found[0].starts_with("ROADMAP.md:") && found[0].contains("cannot read"), "{found:?}");

    // `docs` is a file, so it cannot be listed.
    let dir = crate::TempDir::new("docs-not-a-dir");
    dir.write("docs", "not a directory");
    let found = check(dir.path());
    assert!(found.len() == 1 && found[0].starts_with("docs:") && found[0].contains("cannot list"), "{found:?}");

    // A `docs/x.md` that is a directory.
    let dir = crate::TempDir::new("docs-md-dir");
    std::fs::create_dir_all(dir.path().join("docs/odd.md")).unwrap();
    let found = check(dir.path());
    assert!(found.len() == 1 && found[0].starts_with("docs/odd.md:") && found[0].contains("cannot read"), "{found:?}");
}

#[cfg(unix)]
#[test]
fn dangling_links_and_unreadable_files_are_reported() {
    use std::os::unix::fs::PermissionsExt;
    // A dangling `ROADMAP.md` is not an absent one.
    let dir = crate::TempDir::new("docs-dangling-roadmap");
    std::os::unix::fs::symlink("nowhere", dir.path().join("ROADMAP.md")).unwrap();
    let found = check(dir.path());
    assert!(found.len() == 1 && found[0].starts_with("ROADMAP.md:") && found[0].contains("cannot read"), "{found:?}");

    // Nor is a dangling `docs/x.md`.
    let dir = crate::TempDir::new("docs-dangling-doc");
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::os::unix::fs::symlink("nowhere", dir.path().join("docs/x.md")).unwrap();
    let found = check(dir.path());
    assert!(found.len() == 1 && found[0].starts_with("docs/x.md:") && found[0].contains("cannot read"), "{found:?}");

    let dir = crate::TempDir::new("docs-denied");
    dir.write("docs/secret.md", "text\n");
    let path = dir.path().join("docs/secret.md");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Root reads anything: there is no permission error to test then.
    if std::fs::read(&path).is_err() {
        let found = check(dir.path());
        assert!(found.len() == 1 && found[0].starts_with("docs/secret.md:") && found[0].contains("cannot read"), "{found:?}");
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
}

#[test]
fn a_tree_with_no_docs_passes() {
    assert!(check(crate::TempDir::new("docs-none").path()).is_empty());
}

#[test]
fn the_tree_is_clean() {
    let found = check(&xtask::root());
    assert!(found.is_empty(), "{found:#?}");
}
