//! `cargo xtask parity`: the summary in docs/parity-checklist.md, recomputed from its rows (DeckCraft's
//! scoring: D / P / M weighted by P0 / P1 / P2).

use xtask::ci::Outcome;
use xtask::parity::{check, render, rows, summary};

const ROWS: &str = "| Area | Feature | Priority | Status |\n|---|---|---|---|\n| A | x | P0 | D |\n| A | y | P2 | M |\n| B | z | P1 | P |\n";

fn checklist(summary: &str) -> String {
    format!("# Parity checklist\n\n<!-- SUMMARY -->\n{summary}\n## Rows\n\n{ROWS}")
}

/// A checklist whose table is `ROWS` and then `extra`. The first line of `extra` is on line `EXTRA_LINE`: the title, a blank, the
/// marker, a blank, `## Rows`, a blank, then the header, the separator and the three rows of `ROWS`.
fn checklist_with(extra: &str) -> String {
    checklist("") + extra
}

const EXTRA_LINE: usize = 12;

#[test]
fn scores_rows() {
    let r = rows(ROWS).unwrap();
    assert_eq!(r.len(), 3);
    let s = summary(&r);
    // (3·1 + 1·0 + 2·0.5) / 6 = 67%
    assert!(s.contains("**Weighted breadth parity: 67%**"), "{s}");
    assert!(s.contains("Open P0 items: ."));
    assert!(s.contains("| A | 75% |"), "{s}");
}

/// A line that looks like a row (four cells, or a first cell that an area of a row that read has) and cannot be read must fail
/// the gate. Dropping it would lower the feature count and move every percentage with nothing to say so: the summary rendered
/// from the surviving rows would match the file.
#[test]
fn a_row_that_looks_like_a_row_and_cannot_be_read_fails() {
    for bad in [
        "| A | w | P3 | D |",
        "| A | w | p0 | D |",
        "| A | w | P0 | X |",
        "| A | w | P0 | Done |",
        "| A | w | P0 | d |",
        "| A | w |  | D |",
        // Four cells in an area nobody has used.
        "| Brand new area | w | P4 | M |",
    ] {
        let text = checklist_with(&format!("{bad}\n"));
        let error = render(&text).unwrap_err();
        assert!(error.contains(bad) && error.contains(&format!("line {EXTRA_LINE}")), "{bad}: {error}");
        assert!(error.contains("P0, P1 or P2") && error.contains("D, P or M"), "{bad}: the error does not say what is allowed: {error}");
        // `check` sees it too, though the summary on disk was written from the rows that did read.
        let dir = crate::TempDir::new("parity-bad-row");
        dir.write(
            "docs/parity-checklist.md",
            &render(&checklist_with("")).unwrap().replace("| B | z | P1 | P |\n", &format!("| B | z | P1 | P |\n{bad}\n")),
        );
        let error = check(dir.path()).unwrap_err();
        assert!(error.contains(bad), "{bad}: a bad row did not fail the check: {error}");
    }
}

/// A known area with another number of cells is a row that went wrong too. An area is known by a row that read.
#[test]
fn a_known_area_with_the_wrong_number_of_cells_fails() {
    for bad in ["| A | too | few |", "| A | z | P0 | D | extra |", "| B | only the feature |", "| A |"] {
        let error = render(&checklist_with(&format!("{bad}\n"))).unwrap_err();
        assert!(error.contains(bad) && error.contains(&format!("line {EXTRA_LINE}")), "{bad}: {error}");
    }
}

/// The same cells under `## Rows`, whoever the area is: four that read are rows and count, anything else fails. (Three or five cells
/// in an area no row has used to pass as prose, and so did a table of four cells that was not a rows table.)
#[test]
fn under_rows_the_same_cells_are_a_row_or_a_failure_whoever_the_area_is() {
    let rendered = render(&checklist_with("| Other | w | P0 | D |\n")).unwrap();
    assert!(rendered.contains("over 4 features"), "{rendered}");
    for extra in [
        "| Other | w | P0 | D | extra |\n",
        "| A | w | P0 | D | extra |\n",
        "| Other | w | P0 |\n",
        "| B | w | P0 |\n",
        "| Other | w | P0 | Q |\n",
        "| Item | Note | Who | When |\n|---|---|---|---|\n",
    ] {
        assert!(render(&checklist_with(extra)).is_err(), "{extra:?} passed under ## Rows");
    }
}

/// Under `## Rows` a row is a row however it is written: the old reading took one with no leading pipe (and counted it), and
/// this one must too. A row that is dropped lowers the count and moves every percentage with nothing to say so.
#[test]
fn a_row_with_no_leading_pipe_is_counted_and_a_bad_one_fails() {
    for row in ["B | w | P1 | D", "B | w | P1 | D |", "B | w | P1 | D  "] {
        let rendered = render(&checklist_with(&format!("{row}\n"))).unwrap();
        assert!(rendered.contains("over 4 features"), "`{row}` was not counted: {rendered}");
    }
    for bad in ["B | w | P9 | D", "B | w | P1", "B | w | P1 | D | extra"] {
        let error = render(&checklist_with(&format!("{bad}\n"))).unwrap_err();
        assert!(error.contains(bad) && error.contains(&format!("line {EXTRA_LINE}")), "{bad}: {error}");
    }
}

/// A separator belongs to a header, directly under it. Any other is a stray: the old reading took the line before it for a
/// header and dropped it, so a good row and a bad row alike went uncounted with the gate still green.
#[test]
fn a_stray_separator_fails_and_no_row_is_taken_for_a_header() {
    let separator = "|---|---|---|---|\n";
    for (what, extra) in [
        ("a good row, then a separator", format!("| B | w | P1 | D |\n{separator}")),
        ("a bad row, then a separator", format!("| B | w | P9 | D |\n{separator}")),
        ("a good row first in a table, then a separator", format!("\n| B | w | P1 | D |\n{separator}| B | v | P0 | M |\n")),
        ("a bad row first in a table, then a separator", format!("\n| B | w | P9 | D |\n{separator}| B | v | P0 | M |\n")),
        ("a separator among the rows", format!("| B | w | P1 | D |\n{separator}| B | v | P0 | M |\n")),
        ("a second separator", separator.to_owned()),
        ("a separator with no pipe at its start", "---|---|---|---\n".to_owned()),
        // A row whose area and feature happen to be `Area` and `Feature` is a row, so the separator under it is a stray.
        ("a row named Area and Feature, then a separator", format!("\n| Area | Feature | P0 | D |\n{separator}")),
        // A header is the first line of a table: one glued under a row, with a separator under it, is none.
        ("a header under a row", format!("| B | w | P1 | D |\n| Area | Feature | Priority | Status |\n{separator}")),
    ] {
        let error = render(&checklist_with(&extra)).unwrap_err();
        assert!(error.contains("line "), "{what}: {error}");
    }
    // A header and its separator, and a second table with its own, are fine, and every row under them is counted.
    let two_tables = format!("\n| Area | Feature | P0/P1/P2 | D/P/M |\n{separator}| B | w | P1 | D |\n| B | v | P0 | M |\n");
    let rendered = render(&checklist_with(&two_tables)).unwrap();
    assert!(rendered.contains("over 5 features"), "{rendered}");
    // The outer pipes of a header and of its separator may be left out, as a row's may.
    let bare = "\nArea | Feature | Priority | Status\n---|---|---|---\nB | w | P1 | D\n";
    assert!(render(&checklist_with(bare)).unwrap().contains("over 4 features"));
    // A line that is not a header does not become one by being followed by a separator: only `Area` and `Feature` in the first
    // two cells make one.
    for not_a_header in ["| Item | Note | Who | When |", "| A | y | P3 | D |", "| B | w | P1 | D |"] {
        assert!(render(&checklist_with(&format!("\n{not_a_header}\n{separator}"))).is_err(), "{not_a_header} passed as a header");
    }
}

/// Under `## Rows` every line that holds a `|` is a row (four cells that read), or a header with its separator: a table of three
/// or five cells in an area that no row has, and a sentence with a pipe in it, are not left alone there, whatever they look like.
#[test]
fn under_rows_a_line_with_a_pipe_is_a_row_or_it_fails() {
    for extra in [
        "| Other | w | P0 |\n",
        "| Other | w | P0 | D | extra |\n",
        "| One | Two |\n|---|---|\n| a | b |\n",
        "Text that mentions | A | w | P9 | X | in a sentence.\n",
        "a | b\n",
        "|\n",
        "| | | | |\n",
    ] {
        assert!(render(&checklist_with(extra)).is_err(), "{extra:?} passed under ## Rows");
    }
    // A line with no pipe is no table line, and a blank line ends nothing: both are free.
    let rendered = render(&checklist_with("\nSome words that hold no pipe.\n\n| B | w | P1 | D |\n")).unwrap();
    assert!(rendered.contains("over 4 features"), "{rendered}");
}

/// `## Rows` ends at the next heading. After it, and before the summary marker, tables of any shape and a stray separator are
/// prose and are free, as the revision history is; before it, a bad row still fails.
#[test]
fn outside_rows_prose_tables_stay_free() {
    let prose = "\n## Revision history\n\n| Date | Change | Summary |\n|---|---|---|\n| 2026-10-31 | major | Created |\n|---|---|---|\n\
                 | One | Two |\n| P | Q | R | S | T |\n| A | w | P9 | X |\n| A | w | P0 |\nText with | a pipe | in it.\n";
    let rendered = render(&checklist_with(prose)).unwrap();
    assert!(rendered.contains("over 3 features"), "{rendered}");
    // Before the marker is the head: not read at all.
    let head = checklist_with("").replacen(
        "# Parity checklist\n",
        "# Parity checklist\n\n| Date | Change |\n|---|---|\n| a | b | c |\n| A | w | P9 | X |\n",
        1,
    );
    let rendered = render(&head).unwrap();
    assert!(rendered.contains("over 3 features") && rendered.contains("| A | w | P9 | X |"), "{rendered}");
    // A bad row before the next heading fails, and the same row after it does not.
    assert!(render(&checklist_with("| A | w | P9 | X |\n\n## Notes\n")).is_err());
    assert!(render(&checklist_with("\n## Notes\n\n| A | w | P9 | X |\n")).is_ok());
}

/// A heading under `## Rows` that is deeper than the second level (`###` and below) is a sub-heading of the rows, not the end of
/// them: the real checklist can group its rows by area under `###` lines. A row after one of them is read, a bad one fails, and
/// 977 of them are 977 features. The reading used to end the section at any heading, so everything below the first `###` was
/// not read at all (the gate said "over 0 features" and exit 0).
#[test]
fn rows_under_deeper_headings_are_part_of_the_rows() {
    for heading in ["###", "### Sub", "#### Sub area", "##### x", "###### y", "### Document | Layers"] {
        // A heading with a pipe in it is a line under Rows like any other, and fails as one: leave that form to the test below.
        if heading.contains('|') {
            continue;
        }
        let rendered = render(&checklist_with(&format!("{heading}\n| B | w | P1 | D |\n"))).unwrap();
        assert!(rendered.contains("over 4 features"), "a row under `{heading}` was not read: {rendered}");
        let error = render(&checklist_with(&format!("{heading}\n| B | w | P1 | D |\n| Document | After two | P9 | X |\n"))).unwrap_err();
        assert!(error.contains("| Document | After two | P9 | X |") && error.contains(&format!("line {}", EXTRA_LINE + 2)), "{heading}: {error}");
    }
    // The checklist as M1 will write it: 977 rows, grouped under sub-headings of every depth.
    let mut grouped = String::new();
    for area in 0..40 {
        grouped.push_str(&format!("\n{} Area {area}\n\n", ["###", "####", "###", "#####"][area % 4]));
        for feature in 0..25 {
            if area * 25 + feature < 977 {
                grouped.push_str(&format!("| Area {area} | Feature {feature} | P{} | {} |\n", feature % 3, ["D", "P", "M"][feature % 3]));
            }
        }
    }
    let rendered = render(&checklist_with(&grouped)).unwrap();
    assert!(rendered.contains("over 980 features"), "977 rows and the three of the fixture: {}", &rendered[..rendered.len().min(400)]);
    assert_eq!(render(&rendered).unwrap(), rendered);
}

/// Only a heading of the first or second level, on a line with no `|`, ends the rows. `# Document | Layers | P1 | M |` is a row
/// that happens to begin with a `#`, and `## Document | Layers | P9 | X |` a bad one: neither is a heading.
#[test]
fn only_a_first_or_second_level_heading_without_a_pipe_ends_the_rows() {
    let rendered = render(&checklist_with("# Document | Layers | P1 | M |\n")).unwrap();
    assert!(rendered.contains("over 4 features"), "a row that starts with # was taken for a heading: {rendered}");
    assert!(render(&checklist_with("## Document | Layers | P9 | X |\n")).is_err(), "a bad row that starts with ## was taken for a heading");
    assert!(render(&checklist_with("### Document | Layers | P9 | X |\n")).is_err());
    // Real headings of the first and second level end it; what follows is free.
    for heading in ["# Top", "## Notes", "#", "##"] {
        assert!(render(&checklist_with(&format!("\n{heading}\n\n| A | w | P9 | X |\n"))).is_ok(), "`{heading}` did not end the rows");
    }
    // A hash with no space after it is not a heading at all.
    assert!(render(&checklist_with("\n#Notes\n\n| A | w | P9 | X |\n")).is_err(), "#Notes is no heading");
}

/// A checklist with no rows under `## Rows` is a failure, not zero features: an empty table, an empty section, a section whose
/// rows were all lost. (A checklist of 0 features read as 0% and passed.)
#[test]
fn a_checklist_with_no_rows_fails() {
    let empty = |rows: &str| {
        format!(
            "# Parity checklist\n\n<!-- SUMMARY -->\n\n## Rows\n\n{rows}\n## Revision history\n\n| Date | Change | Summary |\n|---|---|---|\n| a | b | c |\n"
        )
    };
    for rows in ["", "| Area | Feature | Priority | Status |\n|---|---|---|---|\n", "Some words and no table.\n", "### Sub\n"] {
        let error = render(&empty(rows)).unwrap_err();
        assert!(error.contains("no rows") && error.contains("## Rows"), "{rows:?}: {error}");
    }
    assert!(render(&empty("| A | x | P0 | D |\n")).is_ok());
    // Through the gate and through the writer.
    let dir = crate::TempDir::new("parity-no-rows");
    dir.write("docs/parity-checklist.md", &empty(""));
    assert!(check(dir.path()).unwrap_err().contains("no rows"));
    assert!(xtask::parity::run(dir.path()).unwrap_err().contains("no rows"));
    assert_eq!(std::fs::read_to_string(dir.path().join("docs/parity-checklist.md")).unwrap(), empty(""), "the file was written");
}

/// A row names its area and its feature. A row with either empty counted as a feature, and moved the percentages.
#[test]
fn a_row_with_an_empty_area_or_feature_fails() {
    for bad in ["| | w | P0 | D |", "| A | | P0 | D |", "|  |  | P0 | D |", "| | | P1 | M |", " | w | P0 | D", "A | | P0 | D"] {
        let error = render(&checklist_with(&format!("{bad}\n"))).unwrap_err();
        assert!(error.contains(bad.trim()) && error.contains(&format!("line {EXTRA_LINE}")), "{bad}: {error}");
    }
    assert!(render(&checklist_with("| a | b | P0 | D |\n")).unwrap().contains("over 4 features"));
}

/// More bad rows than a message should list are counted, not all printed.
#[test]
fn many_bad_rows_are_counted_and_the_message_stays_short() {
    let bad: String = (0..100).map(|i| format!("| A | w{i} | P9 | D |\n")).collect();
    let error = render(&checklist_with(&bad)).unwrap_err();
    assert!(error.contains("100 lines"), "{error}");
    assert!(error.contains("w0 ") && error.contains("w19 ") && !error.contains("w20 "), "the first 20 are listed: {error}");
    assert!(error.contains("80 more"), "{error}");
    assert!(error.len() < 3_000, "{} bytes", error.len());
}

/// The committed checklist reads whole under this reading, and has rows.
#[test]
fn the_committed_checklist_passes() {
    assert!(matches!(check(&xtask::root()).unwrap(), Outcome::Ok));
    let text = std::fs::read_to_string(xtask::root().join("docs/parity-checklist.md")).unwrap();
    let rows = text.split("\n## Rows").nth(1).unwrap().lines().filter(|l| l.split('|').count() == 6 && !l.contains("---")).count();
    assert!(rows > 1, "the checklist has no rows to read");
}

#[test]
fn render_replaces_the_summary_between_the_markers_and_is_idempotent() {
    let text = checklist("stale\n");
    let once = render(&text).unwrap();
    assert!(once.starts_with("# Parity checklist\n\n<!-- SUMMARY -->\n**Weighted breadth parity: 67%**"), "{once}");
    assert!(once.ends_with(&format!("## Rows\n\n{ROWS}")), "{once}");
    assert!(!once.contains("stale"));
    assert_eq!(render(&once).unwrap(), once);
}

#[test]
fn render_needs_both_markers_in_order() {
    assert!(render("# x\n\n## Rows\n").unwrap_err().contains("<!-- SUMMARY -->"));
    assert!(render("# x\n<!-- SUMMARY -->\n").unwrap_err().contains("## Rows"));
    assert!(render("# x\n## Rows\n\n<!-- SUMMARY -->\n").is_err(), "rows before the summary marker");
}

/// The checklist has been committed: a missing one is a failure, not a skip (a gate that passes on nothing would
/// pass on a deleted file).
#[test]
fn check_fails_without_a_checklist_and_on_a_stale_summary() {
    let dir = crate::TempDir::new("parity");
    let error = match check(dir.path()) {
        Err(error) => error,
        Ok(Outcome::Skipped(why)) => panic!("an absent checklist skipped: {why}"),
        Ok(Outcome::Ok) => panic!("an absent checklist passed"),
    };
    assert!(error.contains("docs/parity-checklist.md") && error.contains("does not exist"), "{error}");
    dir.write("docs/parity-checklist.md", &checklist("stale\n"));
    let err = check(dir.path()).unwrap_err();
    assert!(err.contains("out of date") && err.contains("cargo xtask parity"), "{err}");
    // The check never rewrites the file.
    assert!(std::fs::read_to_string(dir.path().join("docs/parity-checklist.md")).unwrap().contains("stale"));
    dir.write("docs/parity-checklist.md", &render(&checklist("stale\n")).unwrap());
    assert!(matches!(check(dir.path()).unwrap(), Outcome::Ok));
}

/// `cargo xtask parity` (no `--check`) writes the file: it must not write one it cannot read whole.
#[test]
fn run_refuses_to_rewrite_a_checklist_with_a_bad_row() {
    let dir = crate::TempDir::new("parity-run");
    let text = checklist_with("| A | w | P9 | D |\n");
    dir.write("docs/parity-checklist.md", &text);
    let error = xtask::parity::run(dir.path()).unwrap_err();
    assert!(error.contains("| A | w | P9 | D |"), "{error}");
    assert_eq!(std::fs::read_to_string(dir.path().join("docs/parity-checklist.md")).unwrap(), text, "the file was written");
}
