//! Finding 1: **a cell that reads a connector's route is worked out after the route is written.** A move routes the connectors it moved again and writes their
//! rows after the evaluation of that edit, so a shape tied to the route by `POINTALONGPATH` (a label, or any formula over `Sheet.<c>!Geometry1.Path`) read the route the connector had
//! until the edit after. The drawing then held a value that a full evaluation changes, and the shape drew where the connector no longer was.

use diacraft_calc::{Budget, EvalOptions, recalc_all_with};
use diacraft_doc::Document;
use serde_json::json;

use crate::edit::{doc, drop_shape, id_of, num};
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

/// A at (3, 4) and B at (9, 6) joined right to left: a right-angle route of three legs, and a small shape (`tag`) whose pin is the point half way along the route.
fn tied_pair() -> (diacraft_engine::Session, u32, u32, u32, u32) {
    let mut s = crate::blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 6.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    let tag = drop_shape(&mut s, "rectangle", 1.0, 1.0);
    for (cell, axis) in [("PinX", "PNTX"), ("PinY", "PNTY")] {
        let formula = format!("{axis}(PAR(POINTALONGPATH(Sheet.{c}!Geometry1.Path,0.5,0.1)))");
        run(&mut s, "sheet.set_cell", json!({"id": tag, "page": 1, "cell": cell, "formula": formula}));
    }
    (s, a, b, c, tag)
}

#[test]
fn a_shape_tied_to_a_route_is_worked_out_after_the_move_that_routes_the_connector_again() {
    let (mut s, _a, b, c, tag) = tied_pair();
    assert_eq!(stale(doc(&s), &[c, tag]), Vec::<String>::new(), "tied: the drawing starts as the evaluator has it");
    let before = (num(doc(&s), tag, "PinX"), num(doc(&s), tag, "PinY"));
    run(&mut s, "shape.move", json!({"ids": [b], "dx": 1.5, "dy": -2.0}));
    assert_eq!(stale(doc(&s), &[c, tag]), Vec::<String>::new(), "after the move the tied shape holds what a full evaluation gives");
    let after = (num(doc(&s), tag, "PinX"), num(doc(&s), tag, "PinY"));
    assert!((after.0 - before.0).abs() > 0.2 || (after.1 - before.1).abs() > 0.2, "the tied shape went with the route: {before:?} then {after:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_tied_shape_holds_what_a_full_evaluation_gives_after_every_edit_that_routes_and_after_an_undo_and_a_redo() {
    let (mut s, a, b, c, tag) = tied_pair();
    let edits = [
        ("shape.move", json!({"ids": [a], "dx": -1.0, "dy": 1.0})),
        ("shape.resize", json!({"id": b, "x": 9.0, "y": 6.0, "width": 3.0, "height": 1.0})),
        ("shape.move", json!({"ids": [a, b], "dx": 0.5, "dy": -0.5})),
        ("connector.reset", json!({"ids": [c]})),
    ];
    let mut seen = vec![(num(doc(&s), tag, "PinX"), num(doc(&s), tag, "PinY"))];
    for (command, params) in edits {
        run(&mut s, command, params.clone());
        assert_eq!(stale(doc(&s), &[c, tag]), Vec::<String>::new(), "after {command} {params}");
        seen.push((num(doc(&s), tag, "PinX"), num(doc(&s), tag, "PinY")));
    }
    assert!(seen.windows(2).any(|w| w[0] != w[1]), "the tied shape went somewhere: {seen:?}");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(stale(doc(&s), &[c, tag]), Vec::<String>::new(), "after an undo");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(stale(doc(&s), &[c, tag]), Vec::<String>::new(), "after a redo");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn one_undo_takes_the_route_and_the_tied_shape_back_together() {
    let (mut s, _a, b, _c, tag) = tied_pair();
    let before = (doc(&s).clone(), num(doc(&s), tag, "PinX"), num(doc(&s), tag, "PinY"));
    run(&mut s, "shape.move", json!({"ids": [b], "dx": 0.0, "dy": 2.0}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(*doc(&s), before.0, "the drawing is as it was, the route and the tied shape with it");
    assert_eq!((num(doc(&s), tag, "PinX"), num(doc(&s), tag, "PinY")), (before.1, before.2));
}

/// Reverse Ends writes the path walked the other way after its own recalculation, like a leg moved by hand: **a shape tied to the connector's path** by `POINTALONGPATH`
/// (a quarter of the way along, so that the point it names is another when the path is walked from the other end) is worked out after the path is written, and holds what a full
/// evaluation gives, with the connector reversed and again, and with an undo. The route is an L, which is not the same line when it is turned half way round: a Z is, and a tied
/// shape is where it should be in the state between the swap of the ends and the write of the path, so that a Z cannot show the path read too early.
#[test]
fn a_shape_tied_to_a_route_holds_what_a_full_evaluation_gives_after_the_connector_is_reversed() {
    let mut s = crate::blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 6.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 2}})));
    let tag = drop_shape(&mut s, "rectangle", 1.0, 1.0);
    for (cell, axis) in [("PinX", "PNTX"), ("PinY", "PNTY")] {
        let formula = format!("{axis}(PAR(POINTALONGPATH(Sheet.{c}!Geometry1.Path,0.25,0.1)))");
        run(&mut s, "sheet.set_cell", json!({"id": tag, "page": 1, "cell": cell, "formula": formula}));
    }
    assert_eq!(stale(doc(&s), &[c, tag]), Vec::<String>::new(), "tied: the drawing starts as the evaluator has it");
    let mut seen = vec![(num(doc(&s), tag, "PinX"), num(doc(&s), tag, "PinY"))];
    for step in ["reversed", "reversed again"] {
        run(&mut s, "connector.reverse_ends", json!({"id": c}));
        assert_eq!(stale(doc(&s), &[c, tag]), Vec::<String>::new(), "the tied shape holds what a full evaluation gives, {step}");
        seen.push((num(doc(&s), tag, "PinX"), num(doc(&s), tag, "PinY")));
    }
    let near = |p: &(f64, f64), q: &(f64, f64)| (p.0 - q.0).abs() < 1.0e-6 && (p.1 - q.1).abs() < 1.0e-6;
    assert!(!near(&seen[0], &seen[1]) && near(&seen[0], &seen[2]), "the tied shape went to the other quarter and came back: {seen:?}");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(stale(doc(&s), &[c, tag]), Vec::<String>::new(), "after an undo");
    assert_eq!(check_session(&s), Ok(()));
}
