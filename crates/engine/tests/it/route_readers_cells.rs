//! A cell that reads **one cell** of a connector's route is worked out in the edit that writes the route (`route_readers.rs` and `route_readers_edits.rs` hold the cells that read the whole
//! path through `POINTALONGPATH`): a shape whose pin is `Sheet.<c>!Geometry1.X2`, the last row's `X`, a row the route does not have until another edit gives it one, and a cell of the
//! connector itself that reads its own row. A route is written whole, so the rows that were added or taken away are read as a full evaluation reads them.

use diacraft_calc::{Budget, EvalOptions, recalc_all_with};
use diacraft_doc::Document;
use diacraft_engine::Session;
use serde_json::{Value, json};

use crate::edit::{doc, drop_shape, geometry, id_of, num};
use crate::{check_session, run};

/// The cells of the shapes `ids` that a full evaluation of the drawing changes, as `shape cell saved actual`.
fn stale(d: &Document, ids: &[u32]) -> Vec<String> {
    let report = recalc_all_with(d, &Budget::default(), EvalOptions::default());
    report
        .differed
        .iter()
        .filter(|x| ids.iter().any(|id| x.sheet == format!("page:1/shape:{id}")))
        .map(|x| format!("{} {} saved {:?} actual {:?}", x.sheet, x.cell, x.saved, x.actual))
        .collect()
}

/// A at (3, 4) and B at (9, 6) joined right to left: a right-angle route of three legs. `middle` is a small shape whose pin is the second row's point of the route, `last` one whose pin is
/// the fourth row's (the last of a route of three legs, and a row a straight route does not have), `beyond` one whose pin is the sixth's (a row only a route that goes round has), and
/// the connector holds `User.Tip`, which reads its own third row.
struct Tied {
    s: Session,
    a: u32,
    b: u32,
    c: u32,
    middle: u32,
    last: u32,
    beyond: u32,
}

fn tied() -> Tied {
    let mut s = crate::blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 6.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    let mut pinned = |row: u32| {
        let tag = drop_shape(&mut s, "rectangle", 1.0, 1.0);
        for (cell, axis) in [("PinX", "X"), ("PinY", "Y")] {
            run(&mut s, "sheet.set_cell", json!({"id": tag, "page": 1, "cell": cell, "formula": format!("Sheet.{c}!Geometry1.{axis}{row}")}));
        }
        tag
    };
    let (middle, last, beyond) = (pinned(2), pinned(4), pinned(6));
    run(&mut s, "sheet.add_row", json!({"id": c, "page": 1, "section": "User", "name": "Tip"}));
    run(&mut s, "sheet.set_cell", json!({"id": c, "page": 1, "cell": "User.Tip.Value", "formula": "Geometry1.X3"}));
    Tied { s, a, b, c, middle, last, beyond }
}

impl Tied {
    fn readers(&self) -> Vec<u32> {
        vec![self.c, self.middle, self.last, self.beyond]
    }

    fn pins(&self) -> Vec<f64> {
        [self.middle, self.last, self.beyond].iter().flat_map(|t| [num(doc(&self.s), *t, "PinX"), num(doc(&self.s), *t, "PinY")]).collect()
    }

    fn rows(&self) -> usize {
        geometry(doc(&self.s), self.c).len()
    }
}

#[test]
fn the_pins_read_the_cells_of_the_route_the_connector_has_when_they_are_made() {
    let t = tied();
    assert_eq!(t.rows(), 4, "a Z of three legs is four rows");
    assert_eq!(stale(doc(&t.s), &t.readers()), Vec::<String>::new(), "tied: the drawing starts as the evaluator has it");
}

#[test]
fn a_move_works_out_the_cells_that_read_one_cell_of_the_route_in_the_same_edit() {
    let mut t = tied();
    let before = t.pins();
    run(&mut t.s, "shape.move", json!({"ids": [t.b], "dx": 1.5, "dy": 3.0}));
    assert_eq!(stale(doc(&t.s), &t.readers()), Vec::<String>::new(), "after the move every reader holds what a full evaluation gives");
    assert_ne!(t.pins(), before, "the readers went with the route");
    assert_eq!(check_session(&t.s), Ok(()));
}

#[test]
fn rows_the_route_gains_and_loses_are_read_as_a_full_evaluation_reads_them() {
    let mut t = tied();
    let mut counts = vec![t.rows()];
    let edits = [
        ("a move to the height of A: a straight route", "shape.move", json!({"ids": [t.b], "dx": 0.0, "dy": -2.0})),
        ("a move up: a Z again", "shape.move", json!({"ids": [t.b], "dx": 0.0, "dy": 5.0})),
        ("a move to the left of A: a route that goes round", "shape.move", json!({"ids": [t.b], "dx": -10.0, "dy": -3.0})),
        ("a move back", "shape.move", json!({"ids": [t.b], "dx": 10.0, "dy": 0.0})),
        ("a reset", "connector.reset", json!({"ids": [t.c]})),
    ];
    for (what, command, params) in edits {
        run(&mut t.s, command, params);
        assert_eq!(stale(doc(&t.s), &t.readers()), Vec::<String>::new(), "after {what}");
        counts.push(t.rows());
    }
    let fewest = counts.iter().min().copied().unwrap_or(0);
    let most = counts.iter().max().copied().unwrap_or(0);
    assert!(fewest < 4 && most > 4, "the route lost rows and gained them, which the readers of the fourth and the sixth row saw: {counts:?}");
    run(&mut t.s, "edit.undo", json!({}));
    assert_eq!(stale(doc(&t.s), &t.readers()), Vec::<String>::new(), "after an undo");
    run(&mut t.s, "edit.redo", json!({}));
    assert_eq!(stale(doc(&t.s), &t.readers()), Vec::<String>::new(), "after a redo");
    assert_eq!(check_session(&t.s), Ok(()));
}

#[test]
fn every_edit_that_writes_a_route_works_out_the_readers_of_one_cell() {
    let mut t = tied();
    let other = drop_shape(&mut t.s, "rectangle", 2.0, 9.0);
    let edits = [
        ("a resize", "shape.resize", json!({"id": t.b, "x": 9.0, "y": 6.0, "width": 3.0, "height": 2.0})),
        ("a move of both", "shape.move", json!({"ids": [t.a, t.b], "dx": 0.5, "dy": -0.5})),
        ("a reconnect to a shape", "connector.reconnect", json!({"id": t.c, "end": "end", "target": {"id": other, "point": 1}})),
        ("a reconnect to a point", "connector.reconnect", json!({"id": t.c, "end": "end", "target": {"x": 12.0, "y": 3.0}})),
        ("a reset", "connector.reset", json!({"ids": [t.c]})),
        ("a leg moved by hand", "connector.edit_path", json!({"id": t.c, "leg": 0, "to": 4.5})),
    ];
    let mut seen = vec![t.pins()];
    for (what, command, params) in edits {
        run(&mut t.s, command, params);
        assert_eq!(stale(doc(&t.s), &t.readers()), Vec::<String>::new(), "after {what}");
        seen.push(t.pins());
    }
    assert!(seen.windows(2).any(|w| w[0] != w[1]), "the readers went somewhere: {seen:?}");
    assert_eq!(check_session(&t.s), Ok(()));
}

#[test]
fn the_hops_a_move_gives_a_connector_it_crosses_are_read_by_the_cells_that_read_one_cell() {
    let mut t = tied();
    // A line down the page across the first leg of the route, whose rows a pin reads; the page's jumps are on.
    let line = id_of(&run(&mut t.s, "connector.add", json!({"from": {"x": 4.0, "y": 1.0}, "to": {"x": 4.0, "y": 8.0}})));
    run(&mut t.s, "connector.line_jumps", json!({"code": 4}));
    let rows = geometry(doc(&t.s), line).len();
    let tag = drop_shape(&mut t.s, "rectangle", 1.0, 1.0);
    for (cell, axis) in [("PinX", "X"), ("PinY", "Y")] {
        run(&mut t.s, "sheet.set_cell", json!({"id": tag, "page": 1, "cell": cell, "formula": format!("Sheet.{line}!Geometry1.{axis}3")}));
    }
    let mut ids = t.readers();
    ids.extend([line, tag]);
    assert_eq!(stale(doc(&t.s), &ids), Vec::<String>::new(), "tied");
    for dy in [1.0, -2.0, 3.0] {
        run(&mut t.s, "shape.move", json!({"ids": [t.a], "dx": 0.0, "dy": dy}));
        assert_eq!(stale(doc(&t.s), &ids), Vec::<String>::new(), "after a move of {dy} the hops of the line are read");
    }
    eprintln!("the line had {rows} rows and has {}", geometry(doc(&t.s), line).len());
    assert_eq!(check_session(&t.s), Ok(()));
}

/// The recalculation the commands made, counted: the commits and the steps of `diacraft_calc::work`.
fn counted(s: &mut Session, command: &str, params: Value) -> (u64, u64) {
    diacraft_calc::work::reset();
    run(s, command, params);
    (diacraft_calc::work::commits(), diacraft_calc::work::steps())
}

#[test]
fn a_cell_that_reads_a_route_the_edit_did_not_write_makes_no_commit() {
    // Two connectors on one page, and a pin that reads a cell of the first: a move of a shape only the second is glued to routes the second again, which nothing reads.
    let mut t = tied();
    let c = drop_shape(&mut t.s, "rectangle", 2.0, 12.0);
    let d = drop_shape(&mut t.s, "rectangle", 9.0, 12.0);
    run(&mut t.s, "connector.add", json!({"from": {"id": c, "point": 3}, "to": {"id": d, "point": 1}}));
    let (commits, _) = counted(&mut t.s, "shape.move", json!({"ids": [d], "dx": 0.0, "dy": 2.0}));
    let (plain, _) = {
        let mut p = crate::blank_session();
        let c = drop_shape(&mut p, "rectangle", 2.0, 12.0);
        let d = drop_shape(&mut p, "rectangle", 9.0, 12.0);
        run(&mut p, "connector.add", json!({"from": {"id": c, "point": 3}, "to": {"id": d, "point": 1}}));
        counted(&mut p, "shape.move", json!({"ids": [d], "dx": 0.0, "dy": 2.0}))
    };
    assert_eq!(commits, plain, "the pins that read the first route are no reason for a commit on the second");
}

#[test]
fn a_move_that_routes_the_connector_one_cell_of_which_is_read_makes_one_commit_more() {
    let mut plain = tied();
    // The same page with nothing that reads the route: the pins' formulas are given constants.
    for tag in [plain.middle, plain.last, plain.beyond] {
        for cell in ["PinX", "PinY"] {
            run(&mut plain.s, "sheet.set_cell", json!({"id": tag, "page": 1, "cell": cell, "formula": "1"}));
        }
    }
    run(&mut plain.s, "sheet.set_cell", json!({"id": plain.c, "page": 1, "cell": "User.Tip.Value", "formula": "1"}));
    let (without, _) = counted(&mut plain.s, "shape.move", json!({"ids": [plain.b], "dx": 1.5, "dy": 3.0}));
    let mut t = tied();
    let (with, steps) = counted(&mut t.s, "shape.move", json!({"ids": [t.b], "dx": 1.5, "dy": 3.0}));
    // The commit of the edit, and one for the cells that read the route the edit wrote: no more. The jump pass writes no route again where the route it computes is the one that was
    // written (it did where the rows differed by the sign of a zero, `-0` and `0`: a commit more for nothing), as for the cells that read the whole path (`route_readers_edits.rs`).
    assert_eq!(without, 1, "with nothing to read the route the edit makes the one commit it made");
    assert_eq!(with, without + 1, "{with} commits where {without} are made with nothing to read the route ({steps} steps)");
}
