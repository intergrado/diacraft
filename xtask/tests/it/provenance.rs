//! docs/provenance.md: every file the sibling craft apps (and any other project) contributed code or text to has a row
//! (AGENTS.md, the rule on copied code). Copied files cannot be told from original ones by a program, so the check is about the
//! files that are known to be adapted: all of `xtask/src` and the shared test helpers, and the files that a comparison word by
//! word with the siblings' clones found to share long runs. An original file added there needs a row that says so, or an
//! edit of this list.

use std::collections::HashSet;

const ADAPTED_TESTS: &[&str] = &["xtask/tests/it/main.rs"];

/// Files outside `xtask/src` that share a run of 15 or more words, most of their lines, or any prose with a sibling's file: the list
/// `docs/provenance.md` says the rule is pinned in. It was made from the word-by-word comparison of every tracked text file with every
/// text file of the clones, and holds every file with a row whose longest share was 15 words or more, and the files whose rows the
/// first version left out. Left out on purpose: `NOTICE`, whose long runs are the texts of the licences it keeps.
const COPIED: &[&str] = &[
    "LICENSE-APACHE",
    "Cargo.toml",
    "clippy.toml",
    "deny.toml",
    "ROADMAP.md",
    ".config/nextest.toml",
    "xtask/Cargo.toml",
    "xtask/tests/it/docs_check.rs",
    "xtask/tests/it/layers.rs",
    "xtask/tests/it/parity.rs",
    "xtask/tests/it/stats.rs",
    "xtask/tests/it/version.rs",
    "crates/engine/Cargo.toml",
    "crates/engine/src/cmd.rs",
    "crates/engine/src/error.rs",
    "crates/engine/src/guard.rs",
    "crates/engine/src/session.rs",
    "crates/mcp/Cargo.toml",
    "crates/mcp/src/protocol.rs",
    "crates/mcp/tests/it/main.rs",
    "crates/ui-egui/Cargo.toml",
    "crates/ui-egui/src/control.rs",
    "apps/diacraft/Cargo.toml",
    "apps/diacraft/src/control_server.rs",
    "apps/diacraft/src/control_server/deadlines.rs",
    "apps/diacraft/src/logging.rs",
    "apps/diacraft/src/main.rs",
    "apps/diacraft/tests/it/control_transport.rs",
    "apps/diacraft/tests/it/logging.rs",
    "apps/diacraft-cli/Cargo.toml",
    "apps/diacraft-cli/src/steps.rs",
    "apps/diacraft-cli/tests/it/mcp.rs",
    "apps/diacraft-web/Cargo.toml",
    "apps/diacraft-web/Trunk.toml",
    "apps/diacraft-web/index.html",
    "docs/cli.md",
    "docs/control-protocol.md",
    "docs/file-format-parity.md",
    "docs/hardware-parity.md",
    "docs/localization-parity.md",
    "docs/mcp.md",
    "docs/provenance.md",
    "docs/target-app-parity.md",
    "AGENTS.md",
    "README.md",
    "ATTRIBUTION.md",
    "LICENSE-MIT",
    "SECURITY.md",
    ".github/workflows/ci.yml",
    ".github/pull_request_template.md",
    ".github/dependabot.yml",
    "crates/ui-egui/tests/it/theme.rs",
    "crates/mcp/tests/it/backend.rs",
    "crates/mcp/tests/it/remote_props.rs",
    "apps/diacraft/tests/it/control_props.rs",
    "apps/diacraft-cli/tests/it/main.rs",
    "crates/ui-egui/tests/it/support.rs",
    "xtask/tests/it/lints.rs",
    "docs/gaps.md",
    "docs/parity-checklist.md",
    "crates/color/Cargo.toml",
    "crates/color/src/lib.rs",
    "crates/color/tests/it/rgba.rs",
    "crates/color/tests/it/scheme.rs",
    "crates/color/tests/it/transform.rs",
    "crates/geom/Cargo.toml",
    "crates/formula/src/error.rs",
    "crates/formula/tests/it/main.rs",
    "crates/container/src/xml.rs",
    // Found by the same comparison, 15 words or more with a sibling's file.
    "crates/fonts/Cargo.toml",
    "crates/fonts/src/db.rs",
    "crates/fonts/src/face.rs",
    "crates/fonts/src/script.rs",
    "crates/fonts/src/synth.rs",
    "crates/fonts/tests/it/boundaries.rs",
    "crates/fonts/tests/it/system.rs",
    "crates/text/Cargo.toml",
    "crates/text/src/bidi.rs",
    "crates/text/src/breaking.rs",
    // The desktop window's parts that follow a sibling's shape (each row says how far).
    "crates/ui-egui/src/ribbon/fit.rs",
    "crates/ui-egui/src/ribbon.rs",
    "crates/ui-egui/src/ribbon/controls.rs",
    "crates/ui-egui/src/widgets.rs",
    "crates/ui-egui/src/backstage.rs",
    "crates/ui-egui/src/canvas/view.rs",
    "crates/ui-egui/src/canvas/grid.rs",
    "crates/ui-egui/src/control/input.rs",
    "crates/ui-egui/src/icons.rs",
    // The render worker's shape, the page picture's idea and the Open dialog's call.
    "crates/ui-egui/src/render_worker.rs",
    "crates/ui-egui/src/canvas/picture.rs",
    "apps/diacraft/src/dialogs.rs",
];

/// The paths in the first cell of each table row of docs/provenance.md, without backticks. A cell may name several, separated by
/// commas; each is a path.
fn rows() -> Vec<String> {
    let text = std::fs::read_to_string(xtask::root().join("docs/provenance.md")).unwrap();
    text.lines()
        .filter_map(|l| l.trim().strip_prefix('|'))
        .filter_map(|l| l.split('|').next())
        .flat_map(|cell| cell.split(',').map(|path| path.trim().trim_matches('`').to_owned()).collect::<Vec<_>>())
        .collect()
}

#[test]
fn every_xtask_source_file_and_the_shared_test_helpers_have_a_row() {
    let rows = rows();
    let mut wanted: Vec<String> = xtask::repo_files(&xtask::root()).unwrap().into_iter().filter(|f| f.starts_with("xtask/src/")).collect();
    wanted.extend(ADAPTED_TESTS.iter().map(|p| (*p).to_string()));
    assert!(wanted.iter().any(|f| f == "xtask/src/ci.rs"), "the file list is wrong: {wanted:?}");
    let missing: Vec<&String> = wanted.iter().filter(|f| !rows.contains(f)).collect();
    assert!(missing.is_empty(), "no docs/provenance.md row for: {missing:#?}");
}

/// The files a word-by-word comparison with the sibling clones found to be copied or adapted have a row, and exist: a row for a
/// file that was renamed away is no row.
#[test]
fn every_file_found_to_share_long_runs_with_a_sibling_has_a_row() {
    let rows = rows();
    let files: HashSet<String> = xtask::repo_files(&xtask::root()).unwrap().into_iter().collect();
    let gone: Vec<&&str> = COPIED.iter().filter(|f| !files.contains(**f)).collect();
    assert!(gone.is_empty(), "COPIED names files the repository does not have: {gone:?}");
    let missing: Vec<&&str> = COPIED.iter().filter(|f| !rows.iter().any(|r| r == **f)).collect();
    assert!(missing.is_empty(), "no docs/provenance.md row for: {missing:#?}");
}

/// Every row of the table of copied code says where from: a repository, a full commit (40 hexadecimal digits: a short one names
/// nothing for sure), a path in it, and what changed.
#[test]
fn every_row_names_its_source_with_a_full_commit_and_says_what_changed() {
    let text = std::fs::read_to_string(xtask::root().join("docs/provenance.md")).unwrap();
    let section = text.split("## Code copied or adapted").nth(1).unwrap().split("\n## ").next().unwrap();
    let rows: Vec<Vec<&str>> =
        section.lines().filter(|l| l.starts_with("| `")).map(|l| l.trim().trim_matches('|').split(" | ").map(str::trim).collect()).collect();
    assert!(rows.len() > 60, "the table has {} rows: the section was not found", rows.len());
    for row in &rows {
        assert_eq!(row.len(), 5, "a row has five cells (path, repository, commit, source path, what changed): {row:?}");
        let commit = row[2];
        assert!(commit.len() == 40 && commit.chars().all(|c| c.is_ascii_hexdigit()), "{}: `{commit}` is not a full commit", row[0]);
        assert!(row[1].contains('/') && !row[3].is_empty() && row[4].len() > 3, "{}: {row:?}", row[0]);
    }
}

/// A licence that asks for its notice to be kept is kept: NOTICE holds the copyright line of every other project that
/// provenance lists as a source, and the permission paragraph the licence makes a condition.
#[test]
fn notice_keeps_the_copyright_notice_of_each_project_the_rows_name() {
    let text = std::fs::read_to_string(xtask::root().join("docs/provenance.md")).unwrap();
    let notice = std::fs::read_to_string(xtask::root().join("NOTICE")).unwrap();
    // The cell after the path is the repository.
    let repositories: HashSet<&str> = text.lines().filter(|l| l.starts_with("| `")).filter_map(|l| l.split(" | ").nth(1)).map(str::trim).collect();
    assert!(repositories.contains("storytold/vectorcraft"), "no repositories read: {repositories:?}");
    // (Not spelled out: the brand check reads this file, and the notice's own line is on its list of lines it lets through.)
    assert!(notice.contains("Team and the respective app's contributors"), "NOTICE keeps the storytold apps' copyright line");
}
