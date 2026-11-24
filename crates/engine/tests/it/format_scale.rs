//! How the format commands grow with what a file holds: a text of many runs, shapes named many times, and a group larger than a
//! command formats. A file may hold 320,000 rows in a section and a million shapes on a page, so each of these is a size a hostile file can reach.
//!
//! **How the growth is judged.** By a count of the work, which does not move with the load on the machine as a time does (a ratio of two timings,
//! eight times the size under twenty-four times the time, failed these tests under load): the commits of the recalculation and their steps
//! (`diacraft_calc::work`), and the lookups, tables of rows and scans the model made (`diacraft_doc::work`). At eight times the size, work that
//! grows with the runs is eight times the count, and work that grows with their square, sixty-four times; a ratio of up to ten is allowed, for the
//! fixed part of a command. The runs are built in code from rows of the shape's own, one for each run, as a text written by an editor holds them.
//! The counts of the passes over the section, the rows read and the rows planned are in the unit tests of `formatting/tests.rs`.

use diacraft_doc::{RowKey, TextPart};
use diacraft_engine::Session;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::json;

use crate::{check_session, run, snapshot};

/// A session whose one page holds shape 1 with a text of `runs` runs, each of two letters and each with a row of `Character` of its own.
fn session_with_runs(runs: u32) -> Session {
    let mut parts = Vec::with_capacity(runs as usize * 2);
    let mut shape = ShapeBuilder::new(1);
    for ix in 0..runs {
        parts.push(TextPart::Cp(ix));
        parts.push(TextPart::Chars("ab".into()));
        shape =
            shape.row_cell("Character", None, RowKey::Ix(ix), "Size", "0.1666666666666667").row_cell("Character", None, RowKey::Ix(ix), "Style", "0");
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape.text_parts(parts))).build();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.edit("fixture", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    s
}

/// What a command did, in counts: the commits of the recalculation, its steps, the tables of rows the model built and the lookups its resolvers built.
#[derive(Clone, Copy, Debug)]
struct Work {
    commits: u64,
    steps: u64,
    tables: u64,
    lookups: u64,
}

/// The work `f` does on `session`.
fn work_of(mut session: Session, f: impl FnOnce(&mut Session)) -> Work {
    diacraft_calc::work::reset();
    diacraft_doc::work::reset();
    f(&mut session);
    Work {
        commits: diacraft_calc::work::commits(),
        steps: diacraft_calc::work::steps(),
        tables: diacraft_doc::work::row_tables_built(),
        lookups: diacraft_doc::work::resolver_builds(),
    }
}

/// Whether `big` (the work at eight times the size) is within ten times `small`: what grows with the runs is eight times, what grows with their
/// square sixty-four. The tables of rows are held to a few however many runs there are, and the commits to one.
fn grows_with_the_size(what: &str, small: Work, big: Work) {
    eprintln!("{what}: {small:?} at one size, {big:?} at eight times the size");
    assert!(big.commits <= 1 && small.commits <= 1, "{what}: more than one commit");
    assert!(big.steps <= small.steps.max(1) * 10, "{what}: {} steps against {}: more than the size grows by", big.steps, small.steps);
    assert!(big.lookups <= small.lookups.max(8) * 10, "{what}: {} lookups against {}: more than the size grows by", big.lookups, small.lookups);
    assert!(big.tables <= small.tables.max(4) * 10, "{what}: {} tables of rows against {}: more than the size grows by", big.tables, small.tables);
}

#[test]
fn formatting_the_whole_text_of_many_runs_grows_with_the_runs() {
    // Five cells for each run: 60,000 cells planned for one shape at 12,000 runs.
    let format = |s: &mut Session| {
        let r = run(s, "text.format", json!({"ids": [1], "size": 11, "bold": true, "italic": true, "strike": true, "color": "#336699"}));
        assert_eq!(r["formatted"], 1);
    };
    let (small, big) = (work_of(session_with_runs(1_500), format), work_of(session_with_runs(12_000), format));
    grows_with_the_size("the whole text, five cells a run", small, big);
}

#[test]
fn a_split_before_many_runs_that_numbers_them_again_grows_with_the_runs() {
    // `b` of the first run: the run is cut, and every row after it moves one down (text order), so a row for each run is written.
    let format = |s: &mut Session| {
        run(s, "text.format", json!({"ids": [1], "start": 1, "end": 2, "bold": true}));
    };
    let (small, big) = (work_of(session_with_runs(1_500), format), work_of(session_with_runs(12_000), format));
    grows_with_the_size("a split before the runs, numbering them again", small, big);
    // And the result is right at the large size: the markers are 0 to 12,000 in order, and the last run's row holds the last run's cells.
    let mut s = session_with_runs(12_000);
    format(&mut s);
    let doc = s.document().unwrap();
    let shape = doc.shape(&doc.find_shape(diacraft_doc::Owner::Page(1), 1).unwrap()).unwrap();
    let markers: Vec<u32> =
        shape.text.as_ref().unwrap().parts.iter().filter_map(|p| if let TextPart::Cp(ix) = p { Some(*ix) } else { None }).collect();
    assert_eq!(markers.len(), 12_001);
    assert!(markers.iter().copied().eq(0..=12_000), "the markers are numbered in text order");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_range_over_many_runs_is_read_with_nothing_worked_out_whatever_their_number() {
    // `shape.formatting` over every run of a text (each run is a row of its own, and they agree, so nothing stops the reading early): one read
    // of each row, and a walk of the parts. The counts of those reads are in `formatting/tests.rs` (they are of the module's own work, which this
    // file cannot see); this counts what a read makes of the rest, and that is nothing: no commit, no step, no table of rows built, at either size.
    let ask = |runs: u32| {
        move |s: &mut Session| {
            let r = run(s, "shape.formatting", json!({"ids": [1], "start": 0, "end": runs * 2}));
            assert_eq!(r["shapes"][0]["mixed"], json!([]), "{r}");
        }
    };
    let (small, big) = (work_of(session_with_runs(1_500), ask(1_500)), work_of(session_with_runs(12_000), ask(12_000)));
    grows_with_the_size("a range over every run", small, big);
    assert_eq!((big.commits, big.steps, big.tables), (0, 0, 0), "a read works nothing out: {big:?}");
    // And over a part of them, at the far end of the text: the same cost for the walk, a handful of rows to read.
    let (small, big) =
        (work_of(session_with_runs(1_500), |s| part_of(s, 2_990, 3_000)), work_of(session_with_runs(12_000), |s| part_of(s, 23_990, 24_000)));
    grows_with_the_size("a range at the end of the runs", small, big);
    assert_eq!((big.commits, big.steps, big.tables), (0, 0, 0), "a read works nothing out: {big:?}");
}

/// `shape.formatting` over `start..end` of shape 1.
fn part_of(s: &mut Session, start: u32, end: u32) {
    let r = run(s, "shape.formatting", json!({"ids": [1], "start": start, "end": end}));
    assert_eq!(r["shapes"][0]["mixed"], json!([]), "{r}");
}

/// A session whose one page holds shape 1 with a text of one run of two letters on row 0 and then `n` runs of one letter that name the rows `40,000` and
/// up, each with a row of its own: a text from a file that numbers its runs far apart (a reader follows the `IX`). A split in the first run
/// numbers the rows again from 0 and so leaves all `n` rows behind, to be taken out.
fn session_with_far_rows(n: u32) -> Session {
    let mut parts = vec![TextPart::Cp(0), TextPart::Chars("ab".into())];
    let mut shape = ShapeBuilder::new(1).row_cell("Character", None, RowKey::Ix(0), "Size", "0.1666666666666667");
    for ix in 40_000..40_000 + n {
        parts.push(TextPart::Cp(ix));
        parts.push(TextPart::Chars("c".into()));
        shape = shape.row_cell("Character", None, RowKey::Ix(ix), "Size", "0.1666666666666667");
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape.text_parts(parts))).build();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.edit("fixture", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    s
}

#[test]
fn a_split_that_leaves_many_rows_behind_takes_them_out_in_work_that_grows_with_the_rows() {
    // 3,000 and 24,000 rows to take out of a section of as many (the reviewer's text: `cp` 0, then `cp` 40,000 and up): taking each out with a pass over
    // the section costs the rows times the rows (and the lookup table of the section is built again after each pass). Counts of the passes are in the
    // unit tests of `formatting/tests.rs`; this counts the tables of rows built and the steps of the recalculation.
    let format = |s: &mut Session| {
        run(s, "text.format", json!({"ids": [1], "start": 1, "end": 2, "bold": true}));
    };
    let (small, big) = (work_of(session_with_far_rows(3_000), format), work_of(session_with_far_rows(24_000), format));
    grows_with_the_size("a split that leaves rows behind", small, big);
    // The result at the large size: the runs are numbered 0 to 24,001 in order, the section holds exactly those rows, and every run kept its cells.
    let mut s = session_with_far_rows(24_000);
    format(&mut s);
    let doc = s.document().unwrap();
    let shape = doc.shape(&doc.find_shape(diacraft_doc::Owner::Page(1), 1).unwrap()).unwrap();
    let markers: Vec<u32> =
        shape.text.as_ref().unwrap().parts.iter().filter_map(|p| if let TextPart::Cp(ix) = p { Some(*ix) } else { None }).collect();
    assert!(markers.iter().copied().eq(0..=24_001), "the markers are 0 to 24,001 in text order");
    let section = shape.sheet.section(&diacraft_doc::SectionKey::new("Character", None)).unwrap();
    let mut rows: Vec<u32> = section.rows.iter().filter_map(|row| row.ix).collect();
    rows.sort_unstable();
    assert!(rows.iter().copied().eq(0..=24_001), "the rows are 0 to 24,001 and no row of the old numbers is left");
    assert_eq!(check_session(&s), Ok(()));
}

/// A page of one group, shape 1, holding `members` shapes, ids 2 and up.
fn session_with_group(members: u32) -> Session {
    let mut group = ShapeBuilder::new(1).group();
    for id in 2..2 + members {
        group = group.child(ShapeBuilder::new(id));
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(group)).build();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.edit("fixture", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    s
}

#[test]
fn a_group_named_many_times_is_walked_once() {
    // 4,096 names of one group of 5,000: the group was walked once for each name (20 million shapes) before, and its members are formatted once.
    let mut s = session_with_group(5_000);
    diacraft_calc::work::reset();
    let r = run(&mut s, "shape.format", json!({"ids": vec![1; 4_096], "fill": "#336699"}));
    let (commits, steps) = (diacraft_calc::work::commits(), diacraft_calc::work::steps());
    eprintln!("4,096 names of a group of 5,000 members: {commits} commits of {steps} steps");
    assert_eq!(r["formatted"], 5_000);
    // Counted, not timed: the members are formatted once, one commit for 5,000 of them (a walk of the group for each name, 20 million shapes, would
    // be 4,096 times the steps of one, and the commits are one).
    assert_eq!(commits, 1, "one commit for the members");
    assert!(steps <= 30 * 5_000, "{steps} steps for 5,000 members (twenty-two measured for each)");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn what_a_command_looks_at_is_charged_to_one_budget_whatever_it_keeps() {
    // 150 groups, each inside the one before and each holding 100 shapes of its own, all named: every group is walked down to the bottom, a million
    // shapes looked at for about 15,000 kept. The budget refuses it as `Failed`, and nothing changes.
    let mut inner: Option<ShapeBuilder> = None;
    for level in (0..150_u32).rev() {
        let mut group = ShapeBuilder::new(1 + level * 101).group();
        for leaf in 1..=100 {
            group = group.child(ShapeBuilder::new(2 + level * 101 + (leaf - 1)));
        }
        if let Some(below) = inner.take() {
            group = group.child(below);
        }
        inner = Some(group);
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(inner.unwrap())).build();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.edit("fixture", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    let ids: Vec<u32> = (0..150_u32).map(|level| 1 + level * 101).collect();
    let before = snapshot(&s);
    let e = s.execute("shape.format", &json!({"ids": ids, "fill": "#336699"})).unwrap_err();
    assert!(matches!(e, diacraft_engine::EngineError::Failed { .. }), "{e:?}");
    assert!(e.to_string().contains("to look through"), "{e}");
    assert_eq!(snapshot(&s), before);
    // The same groups named once each, the outermost, are fine: the walk is of what it holds, once.
    let r = run(&mut s, "shape.format", json!({"ids": [1], "fill": "#336699"}));
    assert_eq!(r["formatted"], 15_000);
}

#[test]
fn the_formatting_of_a_group_is_its_first_member_even_when_the_group_is_larger_than_a_command_formats() {
    // 25,000 members, past the 20,000 `shape.format` takes: the query goes down to the first member and answers, and 4,096 names cost 4,096 steps down.
    let mut s = session_with_group(25_000);
    diacraft_calc::work::reset();
    let r = run(&mut s, "shape.formatting", json!({"ids": vec![1; 4_096]}));
    eprintln!(
        "the formatting of a group of 25,000 named 4,096 times: {} commits of {} steps",
        diacraft_calc::work::commits(),
        diacraft_calc::work::steps()
    );
    assert_eq!(r["shapes"].as_array().unwrap().len(), 4_096);
    assert_eq!(r["shapes"][0]["id"], 1, "the answer is for the shape named, whatever member it was read from");
    // Counted, not timed: a read works nothing out.
    assert_eq!((diacraft_calc::work::commits(), diacraft_calc::work::steps()), (0, 0), "a read recalculates nothing");
    // The command that writes is still limited to what it formats.
    let before = snapshot(&s);
    let e = s.execute("shape.format", &json!({"ids": [1], "fill": "#336699"})).unwrap_err();
    assert!(e.to_string().contains("more than"), "{e}");
    assert_eq!(snapshot(&s), before);
}
