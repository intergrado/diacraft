//! **A fixed route follows its ends square** (replaces the "the bends
//! stay in the connector's frame"). A connector set to reroute as needed (1), never (2) or on crossover (3), or one whose path was edited by hand (3), keeps its route when
//! a shape it is glued to moves, unless 1 or 3 find a leg that would pass through a shape it is glued to. The route it keeps has its **interior corners where they were on
//! the page** and its end legs stretched: each end's neighbouring corner moves across the end's leg, so that the leg stays level or upright, and the leg next to it stretches.
//!
//! The numbers are paper inches on a page of the default scale. Box A is at (3, 4) and box B at (9, 4); a rectangle is 1.5 by 1, with its connection points 0 top, 1 left,
//! 2 bottom and 3 right (`stencil/kit.rs`).

use std::sync::Arc;

use diacraft_doc::{CellAddr, CellEdit, CellWrite, Document, Owner, Section, Shape, SheetRef, Value, WriteOrigin};
use diacraft_engine::Session;
use diacraft_engine::draw::{Reroute, connector_reroute};
use serde_json::json;

use crate::connect_style::{cell, z_pair};
use crate::edit::{doc, drop_shape, geometry, id_of, route};
use crate::{check_session, run, snapshot};

type P = (f64, f64);

/// The corners stay where they were to a billionth of an inch on a page whose connectors are level (the rows are written to nine places).
const TIGHT: f64 = 1.0e-9;

fn assert_route_within(what: &str, got: &[P], want: &[P], tolerance: f64) {
    let ok = got.len() == want.len() && got.iter().zip(want).all(|(g, w)| (g.0 - w.0).abs() < tolerance && (g.1 - w.1).abs() < tolerance);
    assert!(ok, "{what}: got {got:?}, want {want:?}");
}

fn assert_route(what: &str, got: &[P], want: &[P]) {
    assert_route_within(what, got, want, TIGHT);
}

/// Every leg is level or upright, and none is neither (to a millionth of an inch: the rows of a turned connector are carried through its turn).
fn square(route: &[P]) -> bool {
    route.windows(2).all(|w| (w[0].0 - w[1].0).abs() < 1.0e-6 || (w[0].1 - w[1].1).abs() < 1.0e-6)
}

/// Box A at (3, 4) and box B at (9, 4), joined from A's right point to B's left point, and the path edited by hand: out a quarter inch, down an inch and across, up, and in.
/// Its corners are (3.75, 4), (4, 4), (4, 3), (8, 3), (8, 4), (8.25, 4); the two in the middle, (4, 3) and (8, 3), are no end's neighbour. It holds `ConFixedCode` 3.
fn hand_pair() -> (Session, u32, u32, u32) {
    let mut s = crate::blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 4.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 0, "to": 3.0}));
    assert_route("the path edited by hand", &route(doc(&s), c), &[(3.75, 4.0), (4.0, 4.0), (4.0, 3.0), (8.0, 3.0), (8.0, 4.0), (8.25, 4.0)]);
    assert_eq!(cell(doc(&s), c, "ConFixedCode"), Some(3.0));
    (s, a, b, c)
}

/// Takes the marker of ours (`User.DiaCraftRoute`) off connector `c`: it is then a connector of a file, which a command that is about it does not route again.
fn as_a_files(s: &mut Session, c: u32) {
    s.edit("a connector of a file", |d| {
        let page = d.pages.get_mut(0).map(Arc::make_mut).unwrap();
        let at = page.shapes.iter().position(|x| x.id == c).unwrap();
        let mut shape = Shape::clone(page.shapes.get(at).unwrap());
        Arc::make_mut(&mut shape.sheet).sections.retain(|x| x.name.as_str() != "User");
        page.shapes.set(at, Arc::new(shape)).unwrap();
        Ok(())
    })
    .unwrap();
}

fn set(s: &mut Session, c: u32, word: &str) {
    run(s, "connector.reroute", json!({"ids": [c], "mode": word}));
}

fn mv(s: &mut Session, id: u32, dx: f64, dy: f64) {
    run(s, "shape.move", json!({"ids": [id], "dx": dx, "dy": dy}));
}

/// The first `Geometry` section the connector holds itself, as the model holds it: two reads that give the same pointer have not been written between.
fn route_section(d: &Document, id: u32) -> Arc<Section> {
    let at = d.find_shape(Owner::Page(1), id).unwrap();
    d.shape(&at).unwrap().sheet.sections.iter().find(|s| s.name.as_str() == "Geometry" && s.ix == Some(0)).cloned().unwrap()
}

fn end_cells(d: &Document, id: u32) -> (P, P) {
    ((crate::edit::num(d, id, "BeginX"), crate::edit::num(d, id, "BeginY")), (crate::edit::num(d, id, "EndX"), crate::edit::num(d, id, "EndY")))
}

/// "at 2, expect the interior corners unchanged and only the ends and their neighbouring corners moved". An end moved up an inch takes the corner next to it up
/// with it, across its level leg; the corner keeps its place along the leg, and the leg after it stretches. Never means never: the setting is still 2, and the move and the
/// stretch are one undo step.
#[test]
fn reroute_v1_a_connector_set_never_keeps_its_interior_corners_and_stretches_the_legs_at_its_ends() {
    let (mut s, a, b, c) = hand_pair();
    set(&mut s, c, "never");
    let was = snapshot(&s);
    mv(&mut s, b, 0.0, 1.0);
    assert_route("B up an inch", &route(doc(&s), c), &[(3.75, 4.0), (4.0, 4.0), (4.0, 3.0), (8.0, 3.0), (8.0, 5.0), (8.25, 5.0)]);
    assert_eq!(cell(doc(&s), c, "ConFixedCode"), Some(2.0), "the setting is kept");
    assert_eq!(snapshot(&s).undo_steps, was.undo_steps + 1, "the move and the stretch are one undo step");
    // The begin end: A up an inch takes the first corner up, and the second corner (4, 3) stays.
    mv(&mut s, a, 0.0, 1.0);
    assert_route("A up an inch too", &route(doc(&s), c), &[(3.75, 5.0), (4.0, 5.0), (4.0, 3.0), (8.0, 3.0), (8.0, 5.0), (8.25, 5.0)]);
    let (begin, end) = end_cells(doc(&s), c);
    let r = route(doc(&s), c);
    assert_route("the route runs from the begin to the end", &[r[0], r[r.len() - 1]], &[begin, end]);
    // One undo takes the stretch back with the move.
    run(&mut s, "edit.undo", json!({}));
    assert_route("undone", &route(doc(&s), c), &[(3.75, 4.0), (4.0, 4.0), (4.0, 3.0), (8.0, 3.0), (8.0, 5.0), (8.25, 5.0)]);
    assert_eq!(check_session(&s), Ok(()));
}

/// At 1 and 3: with no leg that passes through a shape the connector is glued to, the route is kept and followed square, as at 2, and the setting stays.
#[test]
fn the_same_at_as_needed_and_on_crossover_when_no_leg_crosses_a_glued_shape() {
    for (word, code) in [("as_needed", 1.0), ("on_crossover", 3.0)] {
        let (mut s, a, b, c) = hand_pair();
        set(&mut s, c, word);
        mv(&mut s, b, 0.0, 1.0);
        assert_route(word, &route(doc(&s), c), &[(3.75, 4.0), (4.0, 4.0), (4.0, 3.0), (8.0, 3.0), (8.0, 5.0), (8.25, 5.0)]);
        mv(&mut s, a, 0.0, 1.0);
        assert_route(word, &route(doc(&s), c), &[(3.75, 5.0), (4.0, 5.0), (4.0, 3.0), (8.0, 3.0), (8.0, 5.0), (8.25, 5.0)]);
        assert_eq!(cell(doc(&s), c, "ConFixedCode"), Some(code), "{word}: the setting stays");
        assert_eq!(check_session(&s), Ok(()), "{word}");
    }
}

/// A route of three legs, the Z a new connector gets: both its corners are next to an end, so each moves across its end's leg and the middle leg stretches.
#[test]
fn a_z_moves_both_corners_across_their_end_legs() {
    let (mut s, _, b, c) = z_pair();
    set(&mut s, c, "never");
    mv(&mut s, b, 1.0, 1.0);
    assert_route("B up and along", &route(doc(&s), c), &[(3.75, 4.0), (6.0, 4.0), (6.0, 7.0), (9.25, 7.0)]);
    assert_eq!(check_session(&s), Ok(()));
}

/// A route of two legs has one corner, next to both ends: it goes where the two end legs meet, on the line of the begin's level leg and the end's upright one.
#[test]
fn a_route_of_two_legs_puts_its_corner_where_the_two_end_legs_meet() {
    let mut s = crate::blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 7.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 2}})));
    assert_route("an L", &route(doc(&s), c), &[(3.75, 4.0), (9.0, 4.0), (9.0, 6.5)]);
    set(&mut s, c, "never");
    mv(&mut s, b, 1.0, 1.0);
    assert_route("B moved", &route(doc(&s), c), &[(3.75, 4.0), (10.0, 4.0), (10.0, 7.5)]);
    mv(&mut s, a, 0.0, 1.0);
    assert_route("A moved", &route(doc(&s), c), &[(3.75, 5.0), (10.0, 5.0), (10.0, 7.5)]);
    assert_eq!(cell(doc(&s), c, "ConFixedCode"), Some(2.0));
    assert_eq!(check_session(&s), Ok(()));
}

/// A route of one leg has no corner: it stays one leg, and where an end moves off its line it slants (finds saved right-angle connectors with one
/// slanted leg). It always runs from the begin to the end: its last row follows the width, where a route of constants would be left short.
#[test]
fn a_route_of_one_leg_stays_one_leg_and_slants_when_an_end_moves_off_its_line() {
    let mut s = crate::blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 4.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    assert_route("one level leg", &route(doc(&s), c), &[(3.75, 4.0), (8.25, 4.0)]);
    set(&mut s, c, "never");
    mv(&mut s, b, 0.0, 1.0);
    assert_route_within("one slanted leg", &route(doc(&s), c), &[(3.75, 4.0), (8.25, 5.0)], 1.0e-6);
    // An end moved along the leg's own line leaves it level.
    mv(&mut s, b, 0.0, -1.0);
    mv(&mut s, b, 1.0, 0.0);
    assert_route_within("level again", &route(doc(&s), c), &[(3.75, 4.0), (9.25, 4.0)], 1.0e-6);
    assert_eq!(check_session(&s), Ok(()));
}

/// An end moved past its corner folds the route back on itself. At 1 and 3 it is made again by the connector's style, **keeping the setting** (a leg would pass
/// through the shape), and at 2 the fold is kept, since never means never.
#[test]
fn a_folded_route_is_made_again_at_as_needed_and_on_crossover_and_kept_at_never() {
    // B an inch and a half to the left and an inch up: its left point (6.75, 5) is short of the corner (8, 4) the last leg came out of, so the last leg, which ran right to B,
    // now runs left to it.
    let (mut s, _, b, c) = hand_pair();
    set(&mut s, c, "never");
    mv(&mut s, b, -1.5, 1.0);
    assert_route("never: the fold is kept", &route(doc(&s), c), &[(3.75, 4.0), (4.0, 4.0), (4.0, 3.0), (8.0, 3.0), (8.0, 5.0), (6.75, 5.0)]);
    assert_eq!(cell(doc(&s), c, "ConFixedCode"), Some(2.0));
    for (word, code) in [("as_needed", 1.0), ("on_crossover", 3.0)] {
        let (mut s, _, b, c) = hand_pair();
        set(&mut s, c, word);
        mv(&mut s, b, -1.5, 1.0);
        assert_route(word, &route(doc(&s), c), &[(3.75, 4.0), (5.25, 4.0), (5.25, 5.0), (6.75, 5.0)]);
        assert_eq!(cell(doc(&s), c, "ConFixedCode"), Some(code), "{word}: the setting is kept, a route made so is no hand edit undone");
        assert_eq!(check_session(&s), Ok(()), "{word}");
    }
}

/// A leg that shrinks to nothing as an end moves is dropped, and that is no fold: the end came level with the corner, and the route is one leg shorter.
#[test]
fn a_leg_that_shrinks_to_nothing_is_dropped_and_is_no_fold() {
    for word in ["never", "as_needed", "on_crossover"] {
        let (mut s, _, b, c) = hand_pair();
        set(&mut s, c, word);
        // B down an inch: its left point is at (8.25, 3), level with the corner (8, 3), which the last leg's corner takes: the leg of an inch between them is gone.
        mv(&mut s, b, 0.0, -1.0);
        assert_route(word, &route(doc(&s), c), &[(3.75, 4.0), (4.0, 4.0), (4.0, 3.0), (8.25, 3.0)]);
        assert_eq!(check_session(&s), Ok(()), "{word}");
    }
}

/// A turned glued shape moves its connection point, and the legs stay level and upright **on the page**, where they turned with the connector's frame before (the frame
/// is the line between the ends, which the turn tilts).
#[test]
fn a_glued_shape_turned_leaves_the_legs_level_and_upright() {
    let (mut s, _, b, c) = hand_pair();
    set(&mut s, c, "never");
    run(&mut s, "shape.rotate", json!({"ids": [b], "angle": 90.0}));
    let (_, end) = end_cells(doc(&s), c);
    assert!(
        (end.0 - 9.0).abs() < 1.0e-6 && (end.1 - 3.25).abs() < 1.0e-6,
        "the left point of B, turned a quarter about its middle, is below it: {end:?}"
    );
    let r = route(doc(&s), c);
    assert!(square(&r), "the legs are level and upright on the page: {r:?}");
    assert_route_within(
        "the corners stay, the end leg stretches",
        &r,
        &[(3.75, 4.0), (4.0, 4.0), (4.0, 3.0), (8.0, 3.0), (8.0, 3.25), (9.0, 3.25)],
        1.0e-6,
    );
    assert_eq!(check_session(&s), Ok(()));
}

/// The route of a connector of a file (no marker of ours) is followed square the same way.
#[test]
fn a_connector_of_a_file_follows_its_ends_square_too() {
    let (mut s, _, b, c) = hand_pair();
    as_a_files(&mut s, c);
    set(&mut s, c, "never");
    mv(&mut s, b, 0.0, 1.0);
    assert_route("a file's", &route(doc(&s), c), &[(3.75, 4.0), (4.0, 4.0), (4.0, 3.0), (8.0, 3.0), (8.0, 5.0), (8.25, 5.0)]);
    assert_eq!(check_session(&s), Ok(()));
}

/// A route nobody moved is the **same rows**: an edit that moves a shape no end is glued to, and one that changes a glued shape and leaves the connection point where it
/// was (B wider to the right, its left point where it is), write nothing of the connector's route.
#[test]
fn the_rows_of_a_fixed_route_nobody_moved_are_the_same_arcs() {
    for word in ["never", "as_needed", "on_crossover"] {
        let (mut s, _, b, c) = hand_pair();
        set(&mut s, c, word);
        let other = drop_shape(&mut s, "rectangle", 6.0, 9.0);
        let kept = route_section(doc(&s), c);
        mv(&mut s, other, 1.0, 1.0);
        assert!(Arc::ptr_eq(&kept, &route_section(doc(&s), c)), "{word}: a shape no end is glued to moved");
        // B wider to the right: x and y are the corner at (0, 0) of its frame, the lower left, (8.25, 3.5), so the left point (8.25, 4) is where it was.
        run(&mut s, "shape.resize", json!({"id": b, "x": 8.25, "y": 3.5, "width": 3.0, "height": 1.0}));
        assert_eq!(end_cells(doc(&s), c).1, (8.25, 4.0), "{word}: the glued point did not move");
        assert!(Arc::ptr_eq(&kept, &route_section(doc(&s), c)), "{word}: a glued shape changed and the end stayed");
        assert_eq!(check_session(&s), Ok(()), "{word}");
    }
}

/// Both ends moved by one vector (both shapes moved together) carry the route along as it is: a move of the whole drawing does not leave a fixed route's corners behind.
/// Its rows are not written.
#[test]
fn both_ends_moved_by_one_vector_carry_the_route_along_unwritten() {
    for word in ["never", "as_needed", "on_crossover"] {
        let (mut s, a, b, c) = hand_pair();
        set(&mut s, c, word);
        let kept = route_section(doc(&s), c);
        run(&mut s, "shape.move", json!({"ids": [a, b], "dx": 2.0, "dy": 1.0}));
        assert_route(word, &route(doc(&s), c), &[(5.75, 5.0), (6.0, 5.0), (6.0, 4.0), (10.0, 4.0), (10.0, 5.0), (10.25, 5.0)]);
        assert!(Arc::ptr_eq(&kept, &route_section(doc(&s), c)), "{word}: the rows are the same");
    }
}

/// A connector of a file with no glued end goes with its own move as it is, and the route is not followed square: there is no shape it follows. (One of ours that a command
/// is about is routed again.)
#[test]
fn a_connector_with_no_glued_end_goes_with_its_move_as_it_is() {
    let mut s = crate::blank_session();
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 2.0, "y": 2.0}, "to": {"x": 6.0, "y": 4.0}})));
    run(&mut s, "connector.edit_path", json!({"id": c, "leg": 1, "to": 3.0}));
    as_a_files(&mut s, c);
    set(&mut s, c, "never");
    let before = route(doc(&s), c);
    let kept = route_section(doc(&s), c);
    mv(&mut s, c, 1.0, 1.0);
    let moved: Vec<P> = before.iter().map(|p| (p.0 + 1.0, p.1 + 1.0)).collect();
    assert_route_within("carried by the move", &route(doc(&s), c), &moved, 1.0e-6);
    assert!(Arc::ptr_eq(&kept, &route_section(doc(&s), c)), "the rows are the same");
}

/// The page's dynamics off: the ends follow their shapes and the route's rows are unchanged, whatever the setting. (The last row's `X` is `Width*1`, a formula
/// the recalculation works out again as the width changes: it is the one value that differs.)
#[test]
fn a_page_with_dynamics_off_leaves_a_fixed_routes_rows_as_they_are() {
    let (mut s, _, b, c) = hand_pair();
    set(&mut s, c, "never");
    s.edit("dynamics off", |d| {
        let write = CellWrite { addr: CellAddr::direct("DynamicsOff"), edit: CellEdit::Value(Value::from_text("1")), origin: WriteOrigin::Drawing };
        d.write_batch(vec![(SheetRef::Page(1), write)]).unwrap();
        Ok(())
    })
    .unwrap();
    let rows = geometry(doc(&s), c);
    mv(&mut s, b, 0.0, 1.0);
    let after = geometry(doc(&s), c);
    assert_eq!(after.len(), rows.len(), "No row is added or taken away");
    assert_eq!(after[..after.len() - 1], rows[..rows.len() - 1], "The connector's rows are unchanged");
    assert_eq!(end_cells(doc(&s), c).1, (8.25, 5.0), "and its end follows");
}

/// The rows a route is written in, in the connector's own frame, with the last row's `X` as `Width*1`: the end of the route stays on the end of the connector.
#[test]
fn the_rows_written_end_at_the_connectors_end_by_the_width() {
    let (mut s, _, b, c) = hand_pair();
    set(&mut s, c, "never");
    mv(&mut s, b, 2.0, 1.0);
    let rows = geometry(doc(&s), c);
    let width = crate::edit::num(doc(&s), c, "Width");
    let last = rows.last().unwrap();
    assert!((last.1 - width).abs() < 1.0e-9 && last.2.abs() < 1.0e-9, "the last row is the end, at (Width, 0): {last:?}, width {width}");
    let section = route_section(doc(&s), c);
    let x_of_last =
        section.rows.iter().last().and_then(|r| r.cells.iter().find(|(n, _)| n.as_str() == "X").map(|(_, c)| c.formula.text().map(str::to_owned)));
    assert_eq!(x_of_last, Some(Some("Width*1".to_owned())));
}

/// What the setting says is read from the same cell the reroute command writes: a freely connector is not followed square, it is routed again as before.
#[test]
fn a_freely_connector_is_routed_again_and_not_followed() {
    let (mut s, _, b, c) = hand_pair();
    set(&mut s, c, "freely");
    assert_eq!(connector_reroute(doc(&s), 1, c), Some(Reroute::Freely));
    mv(&mut s, b, 0.0, 1.0);
    // A new route by the style: out a quarter inch, up, in: not the hand path's corners.
    let r = route(doc(&s), c);
    assert!(r.len() <= 4, "routed again by the style: {r:?}");
}
