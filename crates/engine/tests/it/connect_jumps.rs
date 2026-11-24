//! Line jumps (and the vectors).
//!
//! Two connectors with free ends cross at (5, 4): connector 1 runs level from (2, 4) to (8, 4), connector 2 upright from (5, 1) to (5, 7). The
//! default chord is 0.125 (`LineToLineX`) times 2/3 (`LineJumpFactorX`), which the root of every file holds, so a jump is 0.083333 across.

use diacraft_doc::{CellAddr, CellEdit, Document, Owner, Resolver, RowKey, SectionKey, SheetRef, Value, WriteOrigin};
use diacraft_engine::Session;
use diacraft_engine::draw::{JumpChange, line_jump_settings};
use serde_json::json;

use crate::edit::{doc, drop_shape, id_of};
use crate::{blank_session, check_session, run, snapshot};

const CHORD: f64 = 0.125 * 2.0 / 3.0;
const HALF: f64 = CHORD / 2.0;

/// One row of a route: its type, `X`, `Y`, the number in `A` (an arc's bow), and the text of `A` (a polyline's constructor).
type Row = (String, f64, f64, Option<f64>, Option<String>);

fn rows(d: &Document, id: u32) -> Vec<Row> {
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), id).unwrap());
    let found = Resolver::new(d).effective_rows(&sheet, &SectionKey::new("Geometry", Some(0))).unwrap();
    found
        .iter()
        .map(|row| {
            let cell = |name: &str| row.cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(_, r)| r.value.cell?.value.clone());
            let number = |name: &str| cell(name).and_then(|v| v.number());
            (
                row.row_type.as_deref().unwrap().to_owned(),
                number("X").unwrap(),
                number("Y").unwrap(),
                number("A"),
                cell("A").map(|v| v.text().to_owned()),
            )
        })
        .collect()
}

fn kinds(d: &Document, id: u32) -> Vec<String> {
    rows(d, id).into_iter().map(|r| r.0).collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

fn connect(s: &mut Session, from: (f64, f64), to: (f64, f64)) -> u32 {
    id_of(&run(s, "connector.add", json!({"from": {"x": from.0, "y": from.1}, "to": {"x": to.0, "y": to.1}})))
}

/// The two crossing connectors; the level one is 1 and the upright one, added second and so higher in the drawing order, is 2.
fn crossing() -> Session {
    let mut s = blank_session();
    assert_eq!(connect(&mut s, (2.0, 4.0), (8.0, 4.0)), 1);
    assert_eq!(connect(&mut s, (5.0, 1.0), (5.0, 7.0)), 2);
    s
}

fn write_page(s: &mut Session, name: &str, value: f64) {
    s.edit("page cell", |d| {
        d.write_cell(&SheetRef::Page(1), &CellAddr::direct(name), CellEdit::Value(Value::from_text(&value.to_string())), WriteOrigin::Drawing)
            .map(|_| ())
            .map_err(|e| diacraft_engine::EngineError::Internal { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
}

#[test]
fn a_level_leg_that_crosses_an_upright_one_jumps_with_an_arc_by_the_roots_defaults() {
    // No `LineJumpCode` on the page, as the root holds 1: the jump is on the level leg only.
    let s = crossing();
    let d = doc(&s);
    assert_eq!(kinds(d, 1), ["MoveTo", "LineTo", "ArcTo", "LineTo"]);
    let r = rows(d, 1);
    assert!(near(r[1].1, 3.0 - HALF) && near(r[2].1, 3.0 + HALF) && near(r[3].1, 6.0), "{r:?}");
    // Running toward +x a jump bulges up, which is a negative `A` (the half chord), and the cells are plain numbers with `U` DL.
    assert!(near(r[2].3.unwrap(), -HALF), "{r:?}");
    assert_eq!(kinds(d, 2), ["MoveTo", "LineTo"], "the upright leg is plain");
    // The arc's cell is the plain number the files hold, with the unit DL and no formula.
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), 1).unwrap());
    let found = Resolver::new(d).effective_rows(&sheet, &SectionKey::new("Geometry", Some(0))).unwrap();
    let (_, a) = found[2].cells.iter().find(|(n, _)| n.as_str() == "A").unwrap();
    let cell = a.value.cell.unwrap();
    assert_eq!(cell.unit.as_deref(), Some("DL"));
    assert!(cell.formula.text().is_none());
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_leg_running_toward_minus_x_bulges_up_with_a_positive_a_and_an_upright_one_bulges_left() {
    // A level leg from (8, 4) to (2, 4) gets +; an upright leg running up gets - (left of the way it runs is a negative `A`).
    let mut s = blank_session();
    connect(&mut s, (8.0, 4.0), (2.0, 4.0));
    connect(&mut s, (5.0, 1.0), (5.0, 7.0));
    let r = rows(doc(&s), 1);
    assert!(near(r[2].3.unwrap(), HALF), "{r:?}");
    // The upright one carries it when the code says the upright one does (2): a bulge to the left.
    run(&mut s, "connector.line_jumps", json!({"code": 2}));
    let up = rows(doc(&s), 2);
    assert_eq!(up.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["MoveTo", "LineTo", "ArcTo", "LineTo"], "{up:?}");
    assert!(near(up[2].3.unwrap(), -HALF), "running up, left is the negative side: {up:?}");
    assert_eq!(kinds(doc(&s), 1), ["MoveTo", "LineTo"], "and the level one is plain now");
    // The sides: `dir_x` 2 puts a jump on a level leg down, `dir_y` 2 puts one on an upright leg to the right.
    run(&mut s, "connector.line_jumps", json!({"code": 1, "dir_x": 2}));
    assert!(near(rows(doc(&s), 1)[2].3.unwrap(), -HALF), "down, running toward -x, is the negative side");
    run(&mut s, "connector.line_jumps", json!({"code": 2, "dir_y": 2}));
    assert!(near(rows(doc(&s), 2)[2].3.unwrap(), HALF), "right, running up, is the positive side");
}

#[test]
fn which_connector_jumps_is_the_codes_and_none_is_none() {
    let mut s = crossing();
    for (code, jumper) in [(1, 1), (2, 2), (3, 2), (4, 2), (5, 1)] {
        run(&mut s, "connector.line_jumps", json!({"code": code}));
        let (a, b) = (kinds(doc(&s), 1).len(), kinds(doc(&s), 2).len());
        assert_eq!((a == 4, b == 4), (jumper == 1, jumper == 2), "code {code}: connector 1 has {a} rows and 2 has {b}");
    }
    run(&mut s, "connector.line_jumps", json!({"code": 0}));
    assert_eq!((kinds(doc(&s), 1).len(), kinds(doc(&s), 2).len()), (2, 2), "Given the page holding 0, none");
    assert_eq!(line_jump_settings(doc(&s), 1).code, 0);
    // The Show Line Jumps toggle: on makes 1 of a 0, off makes 0.
    run(&mut s, "connector.line_jumps", json!({"show": true}));
    assert_eq!((line_jump_settings(doc(&s), 1).code, kinds(doc(&s), 1).len()), (1, 4));
    run(&mut s, "connector.line_jumps", json!({"code": 4}));
    run(&mut s, "connector.line_jumps", json!({"show": true}));
    assert_eq!(line_jump_settings(doc(&s), 1).code, 4, "on leaves another code as it is");
    run(&mut s, "connector.line_jumps", json!({"show": false}));
    assert_eq!((line_jump_settings(doc(&s), 1).code, kinds(doc(&s), 1).len(), kinds(doc(&s), 2).len()), (0, 2, 2));
}

#[test]
fn two_legs_at_the_same_slant_are_decided_by_the_drawing_order_and_a_slanted_leg_jumps_over_by_steepness() {
    let mut s = blank_session();
    // A straight connector at about 20 degrees, crossed by one at about 70: code 1 gives it to the flatter, which is the first.
    run(&mut s, "connector.style", json!({"style": "straight"}));
    let a = connect(&mut s, (2.0, 2.0), (8.0, 4.19));
    let b = connect(&mut s, (4.0, 1.0), (5.5, 7.0));
    let ra = rows(doc(&s), a);
    assert_eq!(ra.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["MoveTo", "LineTo", "ArcTo", "LineTo"], "the flatter jumps: {ra:?}");
    assert_eq!(kinds(doc(&s), b), ["MoveTo", "LineTo"]);
    // The jump is along the leg and a half chord each side of the crossing.
    let along = |p: &Row| ((p.1 - ra[0].1).powi(2) + (p.2 - ra[0].2).powi(2)).sqrt();
    assert!((along(&ra[2]) - along(&ra[1]) - CHORD).abs() < 1e-6, "{ra:?}");
}

#[test]
fn the_style_makes_the_rows_a_gap_a_square_or_two_sides() {
    let mut s = crossing();
    run(&mut s, "connector.line_jumps", json!({"style": 2}));
    let r = rows(doc(&s), 1);
    assert_eq!(r.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["MoveTo", "LineTo", "MoveTo", "LineTo"], "A gap is a move");
    assert!(near(r[1].1, 3.0 - HALF) && near(r[2].1, 3.0 + HALF), "{r:?}");
    run(&mut s, "connector.line_jumps", json!({"style": 4}));
    let r = rows(doc(&s), 1);
    assert_eq!(r.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["MoveTo", "LineTo", "PolylineTo", "LineTo"], "two sides: a polyline");
    // One control point half a chord off the crossing, to the side of the jump: up (+y) in the page, and the connector's own frame has the same sense.
    let numbers = |text: &str| -> Vec<f64> {
        text.strip_prefix("POLYLINE(").and_then(|t| t.strip_suffix(')')).unwrap().split(',').map(|n| n.parse().unwrap()).collect()
    };
    let two_sides = numbers(&r[2].4.clone().unwrap());
    assert_eq!(two_sides.len(), 4, "the two types and one control point: {two_sides:?}");
    assert!(two_sides[..2] == [1.0, 1.0] && near(two_sides[2], 3.0) && near(two_sides[3], HALF), "{two_sides:?}");
    run(&mut s, "connector.line_jumps", json!({"style": 3}));
    let r = rows(doc(&s), 1);
    assert_eq!(r[2].0, "PolylineTo");
    let square = numbers(&r[2].4.clone().unwrap());
    assert_eq!(square.len(), 6, "the two types and two control points: {square:?}");
    assert!(
        near(square[2], 3.0 - HALF) && near(square[3], HALF) && near(square[4], 3.0 + HALF) && near(square[5], HALF),
        "a square: half a chord above each end: {square:?}"
    );
    for arc_style in [0, 1, 5, 9] {
        run(&mut s, "connector.line_jumps", json!({"style": arc_style}));
        assert_eq!(kinds(doc(&s), 1)[2], "ArcTo", "style {arc_style}, an arc");
    }
}

#[test]
fn the_size_is_the_line_to_line_spacing_times_the_factor_for_each_direction() {
    // `LineToLineX` 0.12 and `LineJumpFactorX` 0.5 give a chord of 0.06 on a level leg; `LineToLineY` 0.2 and a factor 0.25 give 0.05 upright.
    let mut s = crossing();
    write_page(&mut s, "LineToLineX", 0.12);
    write_page(&mut s, "LineToLineY", 0.2);
    run(&mut s, "connector.line_jumps", json!({"factor_x": 0.5, "factor_y": 0.25}));
    let r = rows(doc(&s), 1);
    assert!(near(r[2].1 - r[1].1, 0.06), "{r:?}");
    run(&mut s, "connector.line_jumps", json!({"code": 2}));
    let r = rows(doc(&s), 2);
    assert!(near(r[2].1 - r[1].1, 0.05), "the rows are in the connector's own frame, along its length: {r:?}");
    // A factor of 0 is no jump.
    run(&mut s, "connector.line_jumps", json!({"factor_y": 0.0}));
    assert_eq!(kinds(doc(&s), 2).len(), 2);
    let e = s.execute("connector.line_jumps", &json!({"factor_x": 11.0})).unwrap_err();
    assert!(e.to_string().contains("factor_x"), "{e}");
}

#[test]
fn a_crossing_too_near_the_end_of_its_leg_gets_no_jump_and_one_with_room_does() {
    // Half a chord of room on both sides.
    for (x, jumps) in [(2.02, false), (2.05, true), (7.98, false), (7.95, true)] {
        let mut s = blank_session();
        write_page(&mut s, "LineToLineX", 0.12);
        write_page(&mut s, "LineJumpFactorX", 0.5);
        connect(&mut s, (2.0, 4.0), (8.0, 4.0));
        connect(&mut s, (x, 1.0), (x, 7.0));
        assert_eq!(kinds(doc(&s), 1).len() == 4, jumps, "a crossing at x {x}");
    }
}

#[test]
fn a_curved_connector_neither_jumps_nor_is_jumped_and_a_connector_with_a_code_of_its_own_overrides_the_page() {
    let mut s = crossing();
    run(&mut s, "connector.style", json!({"style": "curved", "ids": [2]}));
    assert_eq!(kinds(doc(&s), 1).len(), 2, "A curved connector takes no part");
    assert_eq!(kinds(doc(&s), 2), ["MoveTo", "LineTo"], "a straight run of a curve is a line, with no jump row");
    run(&mut s, "connector.style", json!({"style": "right_angle", "ids": [2]}));
    assert_eq!(kinds(doc(&s), 1).len(), 4);
    // Connector 2 asks for its jumps always (2), on a page whose code is 0: it jumps, and 1 does not.
    run(&mut s, "connector.line_jumps", json!({"code": 0}));
    run(&mut s, "connector.line_jumps", json!({"ids": [2], "code": 2}));
    assert_eq!((kinds(doc(&s), 1).len(), kinds(doc(&s), 2).len()), (2, 4));
    // Its style of its own is the arc where the page says gap.
    run(&mut s, "connector.line_jumps", json!({"style": 2}));
    run(&mut s, "connector.line_jumps", json!({"ids": [2], "style": 1}));
    assert_eq!(kinds(doc(&s), 2)[2], "ArcTo");
    run(&mut s, "connector.line_jumps", json!({"ids": [2], "style": 0}));
    assert_eq!(kinds(doc(&s), 2)[2], "MoveTo", "0 gives the page's back");
    // 1 never, 3 the other instead, 4 neither.
    run(&mut s, "connector.line_jumps", json!({"code": 1, "style": 0}));
    run(&mut s, "connector.line_jumps", json!({"ids": [2], "code": 0}));
    run(&mut s, "connector.line_jumps", json!({"ids": [1], "code": 1}));
    assert_eq!(kinds(doc(&s), 1).len(), 2, "1: never");
    run(&mut s, "connector.line_jumps", json!({"ids": [1], "code": 3}));
    assert_eq!((kinds(doc(&s), 1).len(), kinds(doc(&s), 2).len()), (2, 4), "3: never itself, and the connector it crosses jumps instead");
    run(&mut s, "connector.line_jumps", json!({"ids": [1], "code": 4}));
    assert_eq!((kinds(doc(&s), 1).len(), kinds(doc(&s), 2).len()), (2, 2), "4: neither");
}

#[test]
fn moving_a_shape_rebuilds_the_jumps_of_the_connectors_it_moves_and_of_those_they_crossed_and_no_others() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 4.0);
    let level = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    // An upright connector across it, and a far-away one nothing of this touches.
    let up = connect(&mut s, (5.0, 1.0), (5.0, 7.0));
    let far = connect(&mut s, (1.0, 10.0), (3.0, 10.0));
    assert_eq!(kinds(doc(&s), level).len(), 4, "the level one jumps over the upright one");
    let far_before = doc(&s).shape(&doc(&s).find_shape(Owner::Page(1), far).unwrap()).unwrap().clone();
    // B goes up: the level connector becomes a Z whose upright leg is at x 6.5 and the crossing moves, still a jump.
    run(&mut s, "shape.move", json!({"ids": [b], "dx": 0.0, "dy": 2.0}));
    let r = rows(doc(&s), level);
    assert!(r.iter().any(|r| r.0 == "ArcTo"), "the Z still crosses the upright one: {r:?}");
    // B goes far up and right of the upright one: no crossing, no jump, and the upright connector is plain.
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 5.0, "dy": 0.0}));
    assert!(rows(doc(&s), level).iter().all(|r| r.0 != "ArcTo"), "{:?}", rows(doc(&s), level));
    assert_eq!(kinds(doc(&s), up), ["MoveTo", "LineTo"]);
    let far_after = doc(&s).shape(&doc(&s).find_shape(Owner::Page(1), far).unwrap()).unwrap().clone();
    assert_eq!(far_after, far_before, "A connector nothing of this reached is as it was");
    assert_eq!(check_session(&s), Ok(()));
}

/// For a drawing that is a `.vsdx`'s (class F2): a connector the rebuild reaches because its legs cross the old or the new route
/// of one that was routed, and which has no hop and needs none, keeps the rows it has, however they are written. Here the upright connector's end is written a
/// ten-millionth of an inch off the line (a route a file made, which a rebuild would write back level), and a move of a shape that the level connector is glued
/// to leaves the upright one, which the level one still crosses, as it was: in the session, and in the `.vsdx` saved and opened again.
#[test]
fn a_neighbour_a_rebuild_reaches_that_has_no_hop_and_needs_none_keeps_its_rows_in_the_session_and_the_vsdx() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 4.0);
    let level = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    let up = connect(&mut s, (5.0, 1.0), (5.0, 7.0));
    assert_eq!(kinds(doc(&s), level).len(), 4, "the level one jumps over the upright one, which is plain");
    assert_eq!(kinds(doc(&s), up), ["MoveTo", "LineTo"]);
    let off = 1.0e-7;
    s.edit("an end off the line", |d| {
        let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), up).unwrap());
        let address = CellAddr::in_row("Geometry", Some(0), RowKey::Ix(2), "Y");
        d.write_cell(&sheet, &address, CellEdit::Value(Value::from_text(&off.to_string())), WriteOrigin::Drawing)
            .map(|_| ())
            .map_err(|e| diacraft_engine::EngineError::Internal { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
    assert_eq!(rows(doc(&s), up)[1].2, off, "the end is off the line, as a file may have it");
    let held = |s: &Session| doc(s).shape(&doc(s).find_shape(Owner::Page(1), up).unwrap()).unwrap().clone();
    let before = held(&s);
    // B goes up: the level connector is a Z whose lower leg still crosses the upright one. It is routed, the upright one is reached and keeps its rows.
    run(&mut s, "shape.move", json!({"ids": [b], "dx": 0.0, "dy": 2.0}));
    assert!(rows(doc(&s), level).iter().any(|r| r.0 == "ArcTo"), "{:?}", rows(doc(&s), level));
    assert!(held(&s) == before, "The neighbour that needs no hop is as it was");
    assert_eq!(check_session(&s), Ok(()));
    // The same in a `.vsdx` saved and opened again.
    let dir = crate::edit_foreign::Dir::new();
    let path = dir.path("kept.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    let saved = rows(doc(&t), up);
    assert_eq!(saved.len(), 2);
    assert!((saved[1].2 - off).abs() < 1.0e-12, "the saved file holds the row as it was: {saved:?}");
}

#[test]
fn a_connector_that_the_upright_one_jumped_over_loses_the_jump_when_the_other_goes() {
    let mut s = crossing();
    assert_eq!(kinds(doc(&s), 1).len(), 4);
    run(&mut s, "shape.delete", json!({"ids": [2]}));
    assert_eq!(kinds(doc(&s), 1), ["MoveTo", "LineTo"], "the connector it crossed is plain again");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(kinds(doc(&s), 1).len(), 4, "and undo brings the jump with the connector");
}

#[test]
fn the_settings_are_one_undo_step_and_a_repeat_changes_nothing() {
    let mut s = crossing();
    let before = snapshot(&s);
    run(&mut s, "connector.line_jumps", json!({"style": 2}));
    assert_eq!(snapshot(&s).undo_steps, before.undo_steps + 1);
    let once = snapshot(&s);
    let r = run(&mut s, "connector.line_jumps", json!({"style": 2}));
    assert_eq!(r["changed"], json!(false));
    assert_eq!(snapshot(&s).document, once.document);
    assert_eq!(snapshot(&s).undo_steps, once.undo_steps);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, before.document, "the cell and the rows of every connector come back together");
    for bad in [
        json!({}),
        json!({"code": 6}),
        json!({"style": 10}),
        json!({"dir_x": 3}),
        json!({"factor_y": -1.0}),
        json!({"show": "yes"}),
        json!({"ids": [1], "show": true}),
        json!({"ids": [1], "factor_x": 1.0}),
        json!({"ids": [99], "code": 1}),
        json!({"code": 1, "page": 7}),
    ] {
        let was = snapshot(&s).document;
        assert!(s.execute("connector.line_jumps", &bad).is_err(), "{bad}");
        assert_eq!(snapshot(&s).document, was, "{bad}");
    }
    let _ = JumpChange::default();
}

#[test]
fn a_page_with_more_connectors_than_the_cap_has_no_jumps_made_and_costs_no_more() {
    // The cap counts the connectors that take part (`MAX_JUMP_PARTICIPANTS`); this only checks that the constant is what the module documents.
    assert_eq!(diacraft_engine::draw::MAX_JUMP_PARTICIPANTS, 2_000);
}

#[test]
fn jumps_curves_and_a_hand_edited_route_are_saved_and_read_back_row_for_row() {
    // The rows are the file's own kind: a jump's `ArcTo` with `U` DL, a polyline's constructor with `U` POLYLINE, a curve's `NURBSTo` with `E` and `U`
    // NURBS, a fixed route's last `X` as `Width*1`. A save writes them and an open reads the same cells.
    let mut s = crossing();
    let curve_ends = (connect(&mut s, (1.0, 8.0), (3.0, 9.5)), connect(&mut s, (4.0, 8.0), (9.0, 9.5)));
    run(&mut s, "connector.style", json!({"style": "curved", "ids": [curve_ends.0]}));
    run(&mut s, "connector.edit_path", json!({"id": curve_ends.1, "leg": 1, "to": 8.6}));
    let dir = crate::edit_foreign::Dir::new();
    let path = dir.path("rows.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    for id in [1, 2, curve_ends.0, curve_ends.1] {
        let (a, b) = (rows(doc(&s), id), rows(doc(&t), id));
        assert_eq!(a.len(), b.len(), "connector {id}");
        for (x, y) in a.iter().zip(&b) {
            assert!(
                x.0 == y.0 && near(x.1, y.1) && near(x.2, y.2) && x.3.zip(y.3).is_none_or(|(p, q)| near(p, q)) && x.4 == y.4,
                "connector {id}: {x:?} against {y:?}"
            );
        }
    }
    assert!(rows(doc(&t), 1).iter().any(|r| r.0 == "ArcTo"), "the jump came back");
    run(&mut t, "connector.style", json!({"style": "right_angle", "ids": [curve_ends.0]}));
    assert_eq!(check_session(&t), Ok(()));
}
