//! A cell that reads a connector's route is worked out in the edit that writes the route, for each edit that writes one: a resize, a reconnect, a leg moved by hand, the jumps a route
//! makes on the connectors it crosses. `route_readers.rs` holds the move and the reset; these are the others, with the cost of each counted on a page where nothing reads a route (the
//! commits it made before the readers of a route were worked out are the commits it makes).

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

/// A at (3, 4) and B at (9, 6) joined right to left (a right-angle route of three legs); shapes `C` (2, 9) and `D` (12, 1) to reconnect to.
struct Pair {
    s: Session,
    a: u32,
    b: u32,
    c: u32,
    other: u32,
    far: u32,
}

fn pair() -> Pair {
    let mut s = crate::blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 6.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    let other = drop_shape(&mut s, "rectangle", 2.0, 9.0);
    let far = drop_shape(&mut s, "rectangle", 12.0, 1.0);
    Pair { s, a, b, c, other, far }
}

/// A small shape whose pin is the point `at` of the way along the route of connector `c` (`POINTALONGPATH`, as a label tied to a route has it).
fn tie(s: &mut Session, c: u32, at: f64) -> u32 {
    let tag = drop_shape(s, "rectangle", 1.0, 1.0);
    for (cell, axis) in [("PinX", "PNTX"), ("PinY", "PNTY")] {
        let formula = format!("{axis}(PAR(POINTALONGPATH(Sheet.{c}!Geometry1.Path,{at},0.1)))");
        run(s, "sheet.set_cell", json!({"id": tag, "page": 1, "cell": cell, "formula": formula}));
    }
    tag
}

fn pin(s: &Session, tag: u32) -> (f64, f64) {
    (num(doc(s), tag, "PinX"), num(doc(s), tag, "PinY"))
}

fn went(before: (f64, f64), after: (f64, f64)) -> bool {
    (after.0 - before.0).abs() > 0.05 || (after.1 - before.1).abs() > 0.05
}

#[test]
fn a_resize_works_out_a_shape_tied_to_the_route_in_the_same_edit() {
    let Pair { mut s, b, c, .. } = pair();
    let tag = tie(&mut s, c, 0.5);
    assert_eq!(stale(doc(&s), &[c, tag]), Vec::<String>::new(), "tied: the drawing starts as the evaluator has it");
    let before = pin(&s, tag);
    run(&mut s, "shape.resize", json!({"id": b, "x": 9.0, "y": 6.0, "width": 3.0, "height": 2.0}));
    assert_eq!(stale(doc(&s), &[c, tag]), Vec::<String>::new(), "after the resize the tied shape holds what a full evaluation gives");
    assert!(went(before, pin(&s, tag)), "the tied shape went with the route: {before:?} then {:?}", pin(&s, tag));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_reconnect_works_out_a_shape_tied_to_the_route_in_the_same_edit() {
    let Pair { mut s, c, other, far, .. } = pair();
    let tag = tie(&mut s, c, 0.5);
    let mut seen = vec![pin(&s, tag)];
    for target in [json!({"id": other, "point": 1}), json!({"x": 12.0, "y": 3.0}), json!({"id": far, "point": 1})] {
        run(&mut s, "connector.reconnect", json!({"id": c, "end": "end", "target": target}));
        assert_eq!(stale(doc(&s), &[c, tag]), Vec::<String>::new(), "after a reconnect to {target}");
        seen.push(pin(&s, tag));
    }
    assert!(seen.windows(2).any(|w| went(w[0], w[1])), "the tied shape went with the route: {seen:?}");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(stale(doc(&s), &[c, tag]), Vec::<String>::new(), "after an undo");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_leg_moved_by_hand_works_out_a_shape_tied_to_the_route_in_the_same_edit() {
    let Pair { mut s, c, .. } = pair();
    let tag = tie(&mut s, c, 0.5);
    let before = pin(&s, tag);
    // The middle leg of the Z, which runs up between the shapes, goes to the right.
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 7.5}));
    assert_eq!(stale(doc(&s), &[c, tag]), Vec::<String>::new(), "after the leg moved the tied shape holds what a full evaluation gives");
    assert!(went(before, pin(&s, tag)), "the tied shape went with the route: {before:?} then {:?}", pin(&s, tag));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_jumps_a_move_makes_on_a_connector_it_crosses_work_out_a_shape_tied_to_that_connector() {
    let Pair { mut s, a, c, .. } = pair();
    // A line down the page across the first leg of the route, tied to by a shape of its own; the page's jumps are on.
    let line = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 4.0, "y": 1.0}, "to": {"x": 4.0, "y": 8.0}})));
    run(&mut s, "connector.line_jumps", json!({"code": 4}));
    let (tag_c, tag_line) = (tie(&mut s, c, 0.5), tie(&mut s, line, 0.5));
    let before = (pin(&s, tag_c), pin(&s, tag_line));
    let rows = geometry(doc(&s), line).len();
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 0.0, "dy": 1.0}));
    assert_eq!(
        stale(doc(&s), &[c, line, tag_c, tag_line]),
        Vec::<String>::new(),
        "after the move both tied shapes hold what a full evaluation gives"
    );
    let after = (pin(&s, tag_c), pin(&s, tag_line));
    eprintln!("the line had {rows} rows and has {}; the tied shapes {before:?} then {after:?}", geometry(doc(&s), line).len());
    assert!(went(before.0, after.0), "the shape tied to the connector that moved went: {before:?} then {after:?}");
    assert_eq!(check_session(&s), Ok(()));
}

/// The recalculation the commands made, counted: the commits and the steps of `diacraft_calc::work`.
fn counted(s: &mut Session, command: &str, params: Value) -> (u64, u64) {
    diacraft_calc::work::reset();
    run(s, command, params);
    (diacraft_calc::work::commits(), diacraft_calc::work::steps())
}

#[test]
fn an_edit_on_a_page_where_nothing_reads_a_route_makes_the_commits_it_made() {
    let Pair { mut s, a, b, c, other, .. } = pair();
    let edits = [
        ("a move", "shape.move", json!({"ids": [b], "dx": 1.5, "dy": -2.0})),
        ("a move of both", "shape.move", json!({"ids": [a, b], "dx": 0.5, "dy": -0.5})),
        ("a resize", "shape.resize", json!({"id": b, "x": 9.0, "y": 6.0, "width": 3.0, "height": 2.0})),
        ("a reconnect to a shape", "connector.reconnect", json!({"id": c, "end": "end", "target": {"id": other, "point": 1}})),
        ("a reconnect to a point", "connector.reconnect", json!({"id": c, "end": "end", "target": {"x": 12.0, "y": 3.0}})),
        ("a leg moved by hand", "connector.edit_path", json!({"id": c, "leg": 0, "to": 3.5})),
        ("a reset", "connector.reset", json!({"ids": [c]})),
    ];
    for ((what, command, params), (named, commits_before, steps_before)) in edits.into_iter().zip(COST_BEFORE) {
        assert_eq!(what, named);
        let (commits, steps) = counted(&mut s, command, params);
        eprintln!("{what}: {commits} commits of {steps} steps (before readers of a route were worked out: {commits_before} of {steps_before})");
        assert_eq!(commits, commits_before, "{what} makes the commits it made");
        // A commit for the readers of a route that finds none is not counted. It is the one that brings the index of what reads what up to date with the rows just written (as
        // `Affected::new` does first), which the next edit's own commit did before, so the steps of the commits counted are no more than they were, and a few fewer.
        // (And one step more for the commit the cross-page work of adds to what a commit looks at: a move took 670 on the merged code.)
        assert!(steps <= steps_before + 1, "{what} takes {steps} steps of the commits counted, it took {steps_before}");
    }
}

#[test]
fn a_move_on_a_page_with_a_tied_shape_makes_one_commit_more_for_what_reads_the_route() {
    let Pair { mut s, b, c, .. } = pair();
    tie(&mut s, c, 0.5);
    let (commits, steps) = counted(&mut s, "shape.move", json!({"ids": [b], "dx": 1.5, "dy": -2.0}));
    assert_eq!(commits, 2, "the commit of the edit, and one for the cells that read the route ({steps} steps)");
}

/// A hub with `n` shapes in a column to its right, each joined to it by a connector, and with `tied` a small shape tied to the middle of each route. → the session, the hub and the
/// connectors and tied shapes.
fn hub(n: u32, tied: bool) -> (Session, u32, Vec<u32>) {
    let mut s = crate::blank_session();
    let hub = drop_shape(&mut s, "rectangle", 2.0, 4.0);
    let mut every = Vec::new();
    for i in 0..n {
        let y = 1.0 + 6.0 * f64::from(i) / f64::from(n);
        let end = drop_shape(&mut s, "rectangle", 9.0, y);
        let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": hub, "point": 3}, "to": {"id": end, "point": 1}})));
        every.push(c);
        if tied {
            every.push(tie(&mut s, c, 0.5));
        }
    }
    (s, hub, every)
}

#[test]
fn the_readers_of_many_routes_are_worked_out_in_step_with_the_routes_and_in_at_most_two_commits() {
    // A move of the hub routes every connector again, and the connectors cross, so the jump pass rewrites some of them too. Counted, not timed: the cells that read the routes are one
    // commit more than the same move makes where nothing reads them for the routes it writes and one for the hops it makes, never more, they take steps in step with the routes they
    // read (twice the routes, less than three times the steps), and the page holds what a full evaluation gives.
    let mut steps_of = Vec::new();
    for n in [30, 60] {
        let (mut plain, plain_hub, _) = hub(n, false);
        let (commits_plain, _) = counted(&mut plain, "shape.move", json!({"ids": [plain_hub], "dx": 0.0, "dy": 0.5}));
        let (mut s, tied_hub, every) = hub(n, true);
        let (commits, steps) = counted(&mut s, "shape.move", json!({"ids": [tied_hub], "dx": 0.0, "dy": 0.5}));
        assert_eq!(stale(doc(&s), &every), Vec::<String>::new(), "{n} routes: after the move every tied shape holds what a full evaluation gives");
        assert!(
            commits > commits_plain && commits <= commits_plain + 2,
            "{n} routes: {commits} commits where {commits_plain} are made with nothing to read the routes ({steps} steps)"
        );
        steps_of.push(steps);
    }
    assert!(steps_of[1] < 3 * steps_of[0], "30 routes took {} steps and 60 took {}", steps_of[0], steps_of[1]);
}

/// What the edits above cost on a page where nothing reads a route, as `(what, commits, steps)` of `diacraft_calc::work`: measured on the code before readers of a route were worked out.
const COST_BEFORE: [(&str, u64, u64); 7] = [
    ("a move", 1, 669),
    ("a move of both", 1, 537),
    ("a resize", 1, 713),
    ("a reconnect to a shape", 1, 585),
    ("a reconnect to a point", 1, 451),
    ("a leg moved by hand", 0, 0),
    ("a reset", 1, 0),
];
