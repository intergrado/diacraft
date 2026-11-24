//! What an edit costs on a large page: one move on a page of 500 shapes and 300 connectors.
//!
//! The recalculation of the whole page (`calc::commit_in` with `Scope::Page`) is what an edit cost before it worked out only what it changed, so
//! its steps are the number the cost of an edit is set against. The cost is counted (`diacraft_calc::work`: the commits an edit makes and the
//! steps they take), not timed: a time moves with the load on the machine, and a comparison of two times failed this test at a load of 46.

use diacraft_engine::draw::{End, attach_connector, drop_shape, settle};
use diacraft_engine::stencil::find_master;
use diacraft_engine::{EngineError, Session};
use serde_json::json;

use crate::{blank_session, check_session};

pub(crate) const COLUMNS: u32 = 25;
pub(crate) const ROWS: u32 = 20;
pub(crate) const CONNECTORS: usize = 300;

/// A page of `COLUMNS` by `ROWS` rectangles (500) joined left to right by `CONNECTORS` connectors, made in one edit.
pub(crate) fn big_page() -> Session {
    let mut s = blank_session();
    let rectangle = find_master("rectangle").unwrap();
    s.edit("build", |doc| {
        let fail = |p: diacraft_engine::draw::Problem| EngineError::Failed { cmd: "test".into(), msg: p.message };
        let before = doc.clone();
        let mut ids = Vec::new();
        for r in 0..ROWS {
            for c in 0..COLUMNS {
                let info = drop_shape(doc, 1, rectangle, 1.5 + 3.0 * f64::from(c), 1.0 + 2.0 * f64::from(r)).map_err(fail)?;
                ids.push(info.id);
            }
        }
        let mut connectors = Vec::new();
        'rows: for r in 0..ROWS {
            for c in 0..COLUMNS - 1 {
                if connectors.len() == CONNECTORS {
                    break 'rows;
                }
                let (a, b) = (ids[(r * COLUMNS + c) as usize], ids[(r * COLUMNS + c + 1) as usize]);
                let info = attach_connector(doc, 1, &End::Shape { id: a, point: None }, &End::Shape { id: b, point: None }).map_err(fail)?;
                connectors.push(info.id);
            }
        }
        settle(doc, &before, 1, &[], &connectors).map_err(fail)
    })
    .unwrap();
    s
}

/// The steps the recalculation of the whole page takes (`calc::commit_in` with `Scope::Page`), which is what an edit did before it worked out only
/// what it changed: the cost that holds on any machine at any load, and the figure the cost of an edit is set against.
pub(crate) fn steps_of_the_whole_page(s: &Session) -> u64 {
    let mut copy = s.document().unwrap().clone();
    let report =
        diacraft_calc::commit_in(&mut copy, &diacraft_calc::Budget::default(), diacraft_calc::EvalOptions::default(), diacraft_calc::Scope::Page(1));
    assert!(!report.budget_exhausted, "{report:?}");
    report.stats.steps
}

/// Runs `command` and gives the commits the recalculation made for it and the steps they took: counted, not timed (`diacraft_calc::work`).
pub(crate) fn recalculation_of(s: &mut Session, command: &str, params: &serde_json::Value) -> (u64, u64) {
    diacraft_calc::work::reset();
    s.execute(command, params).unwrap();
    (diacraft_calc::work::commits(), diacraft_calc::work::steps())
}

#[test]
fn one_move_on_a_page_of_500_shapes_and_300_connectors() {
    let mut s = big_page();
    let doc = s.document().unwrap();
    let page = doc.page(1).unwrap();
    assert_eq!(page.shapes.len(), 500 + CONNECTORS);
    assert_eq!(page.connects.len(), 2 * CONNECTORS);
    // The fifth shape of the sixth row has a connector on each side.
    let moved = 5 * COLUMNS + 4 + 1;
    // What the move replaced: the recalculation of every formula of the page, in steps, which is the number that holds when other work is
    // running (a time moves with the load, and failed this test when it was compared with the time of the same work).
    let whole = steps_of_the_whole_page(&s);
    let mut worst = 0;
    for round in 0..7 {
        let (commits, steps) =
            recalculation_of(&mut s, "shape.move", &json!({"ids": [moved], "dx": 0.05, "dy": if round % 2 == 0 { 0.1 } else { -0.1 }}));
        eprintln!("one move on 500 shapes and {CONNECTORS} connectors: {commits} commits of {steps} steps; the whole page is {whole} steps");
        assert_eq!(commits, 1, "a move works out what it changed in one commit");
        worst = worst.max(steps);
    }
    // A move works out the cells that read what it wrote and no others: under a third of the page (measured: a fifth; a recalculation of the
    // whole page would be `whole`).
    assert!(worst * 3 < whole, "a move took {worst} steps against {whole} for the whole page");
    assert_eq!(check_session(&s), Ok(()));
    // The neighbours' connectors followed.
    let d = s.document().unwrap();
    let connector_ends = d.page(1).unwrap().connects.iter().filter(|c| c.to_sheet == moved).count();
    assert_eq!(connector_ends, 2);
}

#[test]
fn a_move_of_the_most_ids_one_command_takes_on_a_page_of_that_many_shapes_is_bounded() {
    // The hostile size: 4,096 ids (the cap of `shape.move`) on a page of 4,100 shapes, each one written and the page recalculated once.
    let mut s = blank_session();
    let rectangle = find_master("rectangle").unwrap();
    s.edit("build", |doc| {
        for i in 0..4_100_u32 {
            drop_shape(doc, 1, rectangle, 1.0 + f64::from(i % 80) * 2.0, 1.0 + f64::from(i / 80) * 2.0)
                .map_err(|p| EngineError::Failed { cmd: "test".into(), msg: p.message })?;
        }
        Ok(())
    })
    .unwrap();
    let whole = steps_of_the_whole_page(&s);
    let ids: Vec<u32> = (1..=4_096).collect();
    // Counted, not timed: one commit for the whole command, which takes no more steps than the page has (a commit for each id would be
    // 4,096 commits, and a recalculation of the page for each, 4,096 times `whole`).
    let (commits, steps) = recalculation_of(&mut s, "shape.move", &json!({"ids": ids, "dx": 0.5, "dy": 0.5}));
    eprintln!("moving 4,096 shapes on a page of 4,100: {commits} commits of {steps} steps; the whole page is {whole} steps");
    assert_eq!(commits, 1, "one commit for the 4,096 ids");
    assert!(steps < whole, "{steps} steps for the move of 4,096 shapes, {whole} for the whole page");
    let (commits, steps) = recalculation_of(&mut s, "shape.delete", &json!({"ids": (1..=4_096).collect::<Vec<u32>>()}));
    eprintln!("deleting 4,096 shapes from a page of 4,100: {commits} commits of {steps} steps");
    assert!(commits <= 1, "{commits} commits for the delete of 4,096 shapes");
    assert!(steps < whole, "{steps} steps for the delete of 4,096 shapes, {whole} for the whole page");
    assert_eq!(s.document().unwrap().page(1).unwrap().shapes.len(), 4);
}

#[test]
fn the_clipboard_group_and_ungroup_commands_on_a_page_of_500_shapes_and_300_connectors() {
    // The same page as above: 25 columns by 20 rows of rectangles, a connector between neighbours in a row, 24 to a row for the first 12 rows
    // and 12 in the 13th. Each connector is ours, glued at both ends by `PAR` formulas: no `_WALKGLUE`, no group, no text.
    // Counted, not timed: each command's recalculation is one commit at the most, and takes fewer steps than the recalculation of the page would.
    let mut s = big_page();
    let whole = steps_of_the_whole_page(&s);
    // The first four rows and the 96 connectors between their shapes: a selection of 196 shapes.
    let mut four_rows: Vec<u32> = (1..=100).collect();
    four_rows.extend(501..=596);
    let one = COLUMNS * 5 + 5;
    let ids_of_one = [one];
    let mut report = Vec::new();
    let mut check = |s: &mut Session, what: &str, command: &str, params: serde_json::Value| -> serde_json::Value {
        diacraft_calc::work::reset();
        let out = s.execute(command, &params).unwrap();
        let (commits, steps) = (diacraft_calc::work::commits(), diacraft_calc::work::steps());
        report.push(format!("{what}: {command} made {commits} commits of {steps} steps (the whole page is {whole})"));
        assert!(commits <= 1, "{what}: {command} made {commits} commits");
        assert!(steps < whole, "{what}: {command} took {steps} steps, the whole page {whole}");
        out
    };
    for (what, ids) in [("one shape with a connector on each side", &ids_of_one[..]), ("four rows of shapes and their 96 connectors", &four_rows[..])]
    {
        check(&mut s, what, "edit.copy", json!({"ids": ids}));
        check(&mut s, what, "edit.paste", json!({}));
        s.execute("edit.undo", &json!({})).unwrap();
        check(&mut s, what, "shape.duplicate", json!({"ids": ids}));
        s.execute("edit.undo", &json!({})).unwrap();
        check(&mut s, what, "edit.cut", json!({"ids": ids}));
        s.execute("edit.undo", &json!({})).unwrap();
    }
    // Group and ungroup the four rows (the connectors inside, glued to shapes inside).
    check(&mut s, "four rows", "shape.group", json!({"ids": four_rows}));
    s.execute("edit.undo", &json!({})).unwrap();
    let g = check(&mut s, "four rows", "shape.group", json!({"ids": four_rows}))["id"].as_u64().unwrap();
    check(&mut s, "the group", "shape.ungroup", json!({"ids": [g]}));
    s.execute("edit.undo", &json!({})).unwrap();
    // Moving the group: the connectors inside follow, and those between rows 4 and 5 (glued to a shape outside it) are routed again.
    check(&mut s, "the group", "shape.move", json!({"ids": [g], "dx": 0.1, "dy": 0.1}));
    s.execute("edit.undo", &json!({})).unwrap();
    for line in &report {
        eprintln!("{line}");
    }
    assert_eq!(check_session(&s), Ok(()));
}
