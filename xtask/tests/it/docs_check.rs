//! `cargo xtask docs`: what merges leave in the progress docs, and the form of the status line. The repeat checks are FigCraft's;
//! the stacked-status-line check and the status line's form are DiaCraft's own (docs/progress-doc-templates.md): only the scored
//! docs have a status line, and it holds the date their scores were last checked against evidence and nothing else.

use xtask::docs_check::{SCORED, check, repeats, run, status_lines};

/// A scored doc's status line.
const STATUS: &str = "> **Last reviewed:** 2026-10-31";

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
fn two_status_lines_in_one_doc_are_reported() {
    for second in ["> **Last reviewed:** 2026-10-30", "> **Last updated:** 2026-10-30 · **Change:** minor (c)"] {
        let found = repeats(&format!("# Title\n\n{STATUS}\n{second}\n\ntext\n"));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("4: a second status line (the first is on line 3); a doc has one at most"), "{found:?}");
    }
    // Three lines: both extra ones are named.
    let three = format!("{STATUS}\ntext\n{STATUS}\n> **Last reviewed:** 2026-10-29\n");
    let found = repeats(&three);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].starts_with("3: a second status line (the first is on line 1)"), "{found:?}");
    assert!(found[1].starts_with("4: a second status line (the first is on line 1)"), "{found:?}");
}

#[test]
fn one_status_line_is_no_repeat() {
    assert!(repeats(&format!("# Title\n\n{STATUS}\n\ntext\n")).is_empty());
    // Other quotes are not status lines.
    assert!(repeats("> **Note:** a\n> **Note:** b\n> a **Last reviewed:** mention\n").is_empty());
}

#[test]
fn the_ten_scored_docs_each_keep_one_line_with_their_review_date() {
    assert_eq!(
        SCORED,
        [
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
        ]
    );
    for name in SCORED {
        assert!(status_lines(name, &format!("# Title\n\n{STATUS}\n\ntext\n")).is_empty(), "{name}");
        // The checker does not ask for the line.
        assert!(status_lines(name, "# Title\n\ntext\n").is_empty(), "{name}");
    }
}

#[test]
fn a_last_updated_date_is_refused_in_every_doc() {
    for name in ["docs/gaps.md", "docs/provenance.md"] {
        for line in ["> **Last updated:** 2026-11-24", "> **Last reviewed:** 2026-11-01 · **Last updated:** 2026-11-24"] {
            let found = status_lines(name, &format!("# Title\n\n{line}\n"));
            assert!(found.iter().any(|f| f.starts_with("3: a `Last updated` date in the status line")), "{name}: {line}: {found:?}");
        }
    }
    // A scored doc's line is refused for that alone.
    let found = status_lines("docs/gaps.md", "> **Last updated:** 2026-11-24\n");
    assert_eq!(found.len(), 1, "{found:?}");
    // Words that only mention it, past the start of a quote or outside one, are no status line.
    assert!(status_lines("docs/gaps.md", "> a **Last updated:** mention\n**Last updated:** in prose\n").is_empty());
}

#[test]
fn a_change_list_or_a_history_in_a_status_line_is_refused() {
    for line in [
        "> **Last reviewed:** 2026-11-01 · **Change:** minor (a; b)",
        "> **Last reviewed:** 2026-11-01 (a; before that, b)",
        "> **Last reviewed:** 2026-11-01 (a; Before that, b)",
    ] {
        let found = status_lines("ROADMAP.md", &format!("# Roadmap\n\n{line}\n"));
        assert_eq!(found.len(), 1, "{line}: {found:?}");
        assert!(found[0].starts_with("3: a change list or a history in the status line"), "{line}: {found:?}");
    }
    // The line the docs had until 2026-11-24 is refused for its date and for its history, each named once.
    let old = "> **Last reviewed:** 2026-11-01 · **Last updated:** 2026-11-24 · **Change:** minor (a; before that, b) · **Target:** t";
    let found = status_lines("docs/roadmap.md", &format!("{old}\n"));
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].starts_with("1: a `Last updated` date") && found[1].starts_with("1: a change list or a history"), "{found:?}");
    // Elsewhere in a doc the words are free: in a revision row or in prose.
    assert!(status_lines("docs/gaps.md", "| 2026-11-24 | minor | a; before that, b |\n**Change:** a\n").is_empty());
}

#[test]
fn a_scored_doc_status_line_holds_its_review_date_and_nothing_else() {
    for line in [
        "> **Last reviewed:** 2026-11-01 · **Target:** the reference app's desktop edition",
        "> **Last reviewed:** 2026-11-01 ",
        "> **Last reviewed:** 2026-11-1",
        "> **Last reviewed:** 2026/11/01",
        "> **Last reviewed:** soon",
        "> **Last reviewed:**",
    ] {
        let found = status_lines("docs/ui-parity.md", &format!("# UI parity\n\n{line}\n"));
        assert_eq!(found.len(), 1, "{line:?}: {found:?}");
        assert!(found[0].starts_with("3: a status line that is not `> **Last reviewed:** YYYY-MM-DD` alone"), "{line:?}: {found:?}");
    }
}

#[test]
fn a_status_line_in_a_doc_without_scores_is_refused() {
    // The reference docs, a new doc, and names that are not a scored doc's whole path from the root.
    for name in ["docs/provenance.md", "docs/editing-commands.md", "docs/new-page.md", "gaps.md", "docs/sub/gaps.md", "docs/Gaps.md", "roadmap.md"] {
        let found = status_lines(name, &format!("# Title\n\n{STATUS}\n\ntext\n"));
        assert_eq!(found.len(), 1, "{name}: {found:?}");
        assert!(found[0].starts_with("3: a status line in a doc that has no scores"), "{name}: {found:?}");
    }
    // Each line is named.
    let found = status_lines("docs/cli.md", &format!("{STATUS}\n{STATUS}\n"));
    assert!(found.len() == 2 && found[0].starts_with("1: ") && found[1].starts_with("2: "), "{found:?}");
}

#[test]
fn the_tree_check_applies_the_status_rules_by_path() {
    let dir = crate::TempDir::new("docs-status-scope");
    dir.write("ROADMAP.md", &format!("# Roadmap\n\n{STATUS}\n"));
    dir.write("docs/gaps.md", &format!("# Gaps\n\n{STATUS}\n"));
    dir.write("docs/cli.md", "# Command line\n\ntext\n");
    assert!(check(dir.path()).is_empty(), "{:?}", check(dir.path()));
    assert!(run(dir.path()).is_ok());
    dir.write("docs/cli.md", &format!("# Command line\n\n{STATUS}\n"));
    let found = check(dir.path());
    assert!(found.len() == 1 && found[0].starts_with("docs/cli.md:3: a status line in a doc that has no scores"), "{found:?}");
    assert!(run(dir.path()).is_err());
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
    let bad = format!("{STATUS}\n| a | b |\n| a | b |\n");
    let dir = crate::TempDir::new("docs-scope");
    dir.write("docs/notes/plans/design.md", &bad);
    dir.write("docs/notes.txt", &bad);
    dir.write("notes/README.md", &bad);
    dir.write("README.md", &bad);
    assert!(check(dir.path()).is_empty(), "{:?}", check(dir.path()));
    dir.write("ROADMAP.md", &bad);
    dir.write("docs/gaps.md", &bad);
    let found = check(dir.path());
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].starts_with("ROADMAP.md:") && found[1].starts_with("docs/gaps.md:"), "{found:?}");
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

/// A scored doc that moved, or lost its line, would leave the list and its date behind.
#[test]
fn every_scored_doc_is_in_the_tree_with_its_review_date() {
    let root = xtask::root();
    for name in SCORED {
        let text = std::fs::read_to_string(root.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let lines: Vec<&str> = text.lines().filter(|l| l.starts_with("> **Last reviewed:**")).collect();
        assert_eq!(lines.len(), 1, "{name}: {lines:?}");
        assert!(status_lines(name, &text).is_empty(), "{name}: {:?}", status_lines(name, &text));
    }
}

#[test]
fn the_tree_is_clean() {
    let found = check(&xtask::root());
    assert!(found.is_empty(), "{found:#?}");
}
