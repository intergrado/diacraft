//! Several shapes resized and turned as one, and a group resized: `shape.resize` with `ids`, a group by its own box,
//! `shape.rotate` with `by` and `about` for a set, and the protection locks that leave a shape alone.
//!
//! Numbers are paper inches on pages of the default scale. The expected places are worked out from the shapes' own corners on the page, before the command, by the rule the
//! module documentation of `draw/scale.rs` gives: every place goes through the stretch that takes the old joint box to the new one.

use diacraft_doc::{CellAddr, CellEdit, Document, Owner, SheetRef, Value, WriteOrigin};
use diacraft_engine::draw::{connection_points, shape_corners};
use diacraft_engine::{EngineError, Session};
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape, id_of, num};
use crate::group_geometry::{four, group};
use crate::{blank_session, check_session, run, snapshot};

type P = (f64, f64);

const TOL: f64 = 1.0e-7;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < TOL
}

fn near_p(a: P, b: P) -> bool {
    near(a.0, b.0) && near(a.1, b.1)
}

/// The four corners of shape `id` on page 1, in the order of its own frame.
fn corners(d: &Document, id: u32) -> [P; 4] {
    shape_corners(d, 1, id).unwrap_or_else(|| panic!("no shape {id}"))
}

/// The joint box of the shapes: (left, bottom, right, top).
fn joint(d: &Document, ids: &[u32]) -> (f64, f64, f64, f64) {
    let points: Vec<P> = ids.iter().flat_map(|id| corners(d, *id)).collect();
    (
        points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
        points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
        points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max),
        points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
    )
}

fn size(d: &Document, id: u32) -> P {
    (num(d, id, "Width"), num(d, id, "Height"))
}

fn pin(d: &Document, id: u32) -> P {
    (num(d, id, "PinX"), num(d, id, "PinY"))
}

/// The stretch of the page that takes the box `from` to the box `(x, y, width, height)`: where it puts a point.
fn stretch(from: (f64, f64, f64, f64), (x, y, width, height): (f64, f64, f64, f64)) -> impl Fn(P) -> P {
    let (sx, sy) = (width / (from.2 - from.0), height / (from.3 - from.1));
    move |p: P| (x + (p.0 - from.0) * sx, y + (p.1 - from.1) * sy)
}

fn assert_corners_stretched(what: &str, before: &Document, after: &Document, ids: &[u32], map: &dyn Fn(P) -> P) {
    for id in ids {
        let was = corners(before, *id);
        let now = corners(after, *id);
        for (w, n) in was.iter().zip(&now) {
            assert!(near_p(map(*w), *n), "{what}: shape {id}: a corner at {w:?} should be at {:?}, it is at {n:?}", map(*w));
        }
    }
}

/// Writes a cell of shape `id` as a person's edit of the ShapeSheet does.
fn set_cell(s: &mut Session, id: u32, cell: &str, value: &str) {
    s.edit("set a cell", |d| {
        let failed = |e: String| EngineError::Failed { cmd: "test".into(), msg: e };
        let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), id).ok_or_else(|| failed("no shape".into()))?);
        d.write_cell(&sheet, &CellAddr::direct(cell), CellEdit::Value(Value::from_text(value)), WriteOrigin::Drawing)
            .map_err(|e| failed(e.to_string()))?;
        Ok(())
    })
    .unwrap();
}

fn error_of(s: &mut Session, id: &str, params: &Json) -> EngineError {
    s.execute(id, params).expect_err("the command is refused")
}

fn message(e: &EngineError) -> String {
    e.to_string()
}

// ---- resize ------------------------------------------------------------------------------------------------------------------------

#[test]
fn two_shapes_are_scaled_from_a_corner_about_the_opposite_corner() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "ellipse", 6.0, 3.0));
    let before = doc(&s).clone();
    let (x0, y0, x1, y1) = joint(&before, &[a, b]);
    let (w, h) = (x1 - x0, y1 - y0);
    // The north-east handle dragged out: the lower left corner stays, the box comes to twice the width and one and a half times the height.
    let to = (x0, y0, 2.0 * w, 1.5 * h);
    let answer = run(&mut s, "shape.resize", json!({"ids": [a, b], "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    assert_eq!(answer["resized"], 2, "{answer}");
    let d = doc(&s);
    for id in [a, b] {
        let (was_w, was_h) = size(&before, id);
        assert!(near_p(size(d, id), (was_w * 2.0, was_h * 1.5)), "shape {id} is {:?}", size(d, id));
        let was = pin(&before, id);
        assert!(near_p(pin(d, id), (x0 + (was.0 - x0) * 2.0, y0 + (was.1 - y0) * 1.5)), "the pin of shape {id} is {:?}", pin(d, id));
    }
    assert_corners_stretched("a corner handle", &before, d, &[a, b], &stretch((x0, y0, x1, y1), to));
    let now = joint(d, &[a, b]);
    assert!(near(now.0, x0) && near(now.1, y0) && near(now.2, x0 + 2.0 * w) && near(now.3, y0 + 1.5 * h), "the joint box is {now:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_side_handle_stretches_in_one_axis_and_leaves_the_other_alone() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "rectangle", 6.0, 4.0));
    let before = doc(&s).clone();
    let (x0, y0, x1, y1) = joint(&before, &[a, b]);
    // The west handle dragged left: the right edge stays and the box is a half wider; the heights and the places up the page are as they were.
    let to = (x1 - 1.5 * (x1 - x0), y0, 1.5 * (x1 - x0), y1 - y0);
    run(&mut s, "shape.resize", json!({"ids": [a, b], "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    let d = doc(&s);
    for id in [a, b] {
        assert!(near(size(d, id).1, size(&before, id).1), "the height of shape {id} is not changed");
        assert!(near(size(d, id).0, size(&before, id).0 * 1.5), "the width of shape {id} is a half more");
        assert!(near(pin(d, id).1, pin(&before, id).1), "shape {id} stays where it was up the page");
    }
    assert_corners_stretched("a side handle", &before, d, &[a, b], &stretch((x0, y0, x1, y1), to));
    assert!(near(joint(d, &[a, b]).2, x1), "the opposite side stays");
}

#[test]
fn a_shape_turned_a_quarter_is_stretched_along_its_own_axes() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "rectangle", 6.0, 2.0));
    run(&mut s, "shape.rotate", json!({"ids": [a], "angle": 90}));
    let before = doc(&s).clone();
    let (x0, y0, x1, y1) = joint(&before, &[a, b]);
    let to = (x0, y0, 2.0 * (x1 - x0), y1 - y0);
    run(&mut s, "shape.resize", json!({"ids": [a, b], "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    let d = doc(&s);
    // The upright rectangle is twice as wide; the one turned a quarter has its own height along the page's x, so that is what is doubled, and it keeps its turn.
    assert!(near_p(size(d, b), (size(&before, b).0 * 2.0, size(&before, b).1)), "upright: {:?}", size(d, b));
    assert!(near_p(size(d, a), (size(&before, a).0, size(&before, a).1 * 2.0)), "turned: {:?}", size(d, a));
    assert!(near(num(d, a, "Angle"), std::f64::consts::FRAC_PI_2), "the turn is kept");
    assert_corners_stretched("a turned shape", &before, d, &[a, b], &stretch((x0, y0, x1, y1), to));
}

#[test]
fn the_same_stretch_on_both_axes_scales_a_shape_turned_any_angle_by_it() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "ellipse", 6.0, 3.0));
    run(&mut s, "shape.rotate", json!({"ids": [a], "angle": 30}));
    let before = doc(&s).clone();
    let (x0, y0, x1, y1) = joint(&before, &[a, b]);
    let to = (x0 + 1.0, y0 - 0.5, 0.5 * (x1 - x0), 0.5 * (y1 - y0));
    run(&mut s, "shape.resize", json!({"ids": [a, b], "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    let d = doc(&s);
    for id in [a, b] {
        assert!(near_p(size(d, id), (size(&before, id).0 * 0.5, size(&before, id).1 * 0.5)), "shape {id}");
    }
    assert!(near(num(d, a, "Angle"), 30.0_f64.to_radians()));
    assert_corners_stretched("half size", &before, d, &[a, b], &stretch((x0, y0, x1, y1), to));
}

#[test]
fn a_set_resize_is_one_undo_step_and_undo_restores_everything() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "diamond", 6.0, 3.0));
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    let before = snapshot(&s);
    let (x0, y0, x1, y1) = joint(doc(&s), &[a, b]);
    run(&mut s, "shape.resize", json!({"ids": [a, b, c], "x": x0, "y": y0, "width": 2.0 * (x1 - x0), "height": 2.0 * (y1 - y0)}));
    let after = snapshot(&s);
    assert_eq!(after.undo_steps, before.undo_steps + 1, "one undo step");
    assert_ne!(after.document, before.document);
    run(&mut s, "edit.undo", json!({}));
    let undone = snapshot(&s);
    assert_eq!(undone.document, before.document, "everything is as it was");
    assert_eq!(undone.undo_steps, before.undo_steps);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(snapshot(&s).document, after.document, "redo gives the resize back");
}

#[test]
fn a_resize_to_the_box_the_shapes_have_is_no_resize_and_no_undo_step() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "rectangle", 6.0, 4.0));
    let before = snapshot(&s);
    let (x0, y0, x1, y1) = joint(doc(&s), &[a, b]);
    let answer = run(&mut s, "shape.resize", json!({"ids": [a, b], "x": x0, "y": y0, "width": x1 - x0, "height": y1 - y0}));
    assert_eq!(answer["resized"], 0);
    let now = snapshot(&s);
    assert_eq!((now.document, now.undo_steps, now.revision), (before.document, before.undo_steps, before.revision));
}

#[test]
fn connectors_glued_to_the_shapes_are_routed_again_and_a_free_end_is_carried() {
    let mut s = blank_session();
    let (a, b, c) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "rectangle", 6.0, 2.0), drop_shape(&mut s, "rectangle", 6.0, 5.0));
    let glued = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    let free = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": 7.0}, "to": {"x": 3.0, "y": 7.0}})));
    let before = doc(&s).clone();
    // The shapes a and c are resized and the connector between a and b is not named: it follows a.
    let (x0, y0, x1, y1) = joint(&before, &[a, c, free]);
    let to = (x0, y0, 2.0 * (x1 - x0), 1.5 * (y1 - y0));
    run(&mut s, "shape.resize", json!({"ids": [a, c, free], "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    let d = doc(&s);
    let map = stretch((x0, y0, x1, y1), to);
    let end = |d: &Document, id: u32, x: &str, y: &str| (num(d, id, x), num(d, id, y));
    assert!(near_p(end(d, free, "BeginX", "BeginY"), map(end(&before, free, "BeginX", "BeginY"))), "the free connector's begin is carried");
    assert!(near_p(end(d, free, "EndX", "EndY"), map(end(&before, free, "EndX", "EndY"))), "and its end");
    // The glued connector's ends are at the connection points they are glued to, on the page.
    for c in d.page(1).unwrap().connects.iter().filter(|c| c.from_sheet == glued) {
        let (x, y) = if c.from_cell.as_deref() == Some("BeginX") { ("BeginX", "BeginY") } else { ("EndX", "EndY") };
        let Some(index) = c.to_part.and_then(|part| part.checked_sub(100)) else { continue };
        let point = connection_points(d, 1, c.to_sheet).into_iter().find(|p| p.index == index).unwrap();
        assert!(near_p(end(d, glued, x, y), (point.x, point.y)), "the glued connector's {x} is at its connection point");
    }
    assert!(near(size(d, a).0, size(&before, a).0 * 2.0));
    assert!(near_p(size(d, b), size(&before, b)), "a shape not named is not resized");
    assert_eq!(check_session(&s), Ok(()));
}

// ---- groups ------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_group_resized_is_one_shape_and_its_members_follow_by_their_own_formulas() {
    let mut s = four();
    let g = group(&mut s, &[1, 2]);
    let before = doc(&s).clone();
    let (x0, y0, x1, y1) = joint(&before, &[g]);
    let to = (x0, y0, 2.0 * (x1 - x0), 3.0 * (y1 - y0));
    // By its own frame, as one shape.
    run(&mut s, "shape.resize", json!({"id": g, "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    let d = doc(&s);
    assert!(near_p(size(d, g), (size(&before, g).0 * 2.0, size(&before, g).1 * 3.0)), "the group's own size");
    assert_corners_stretched("a group", &before, d, &[g, 1, 2], &stretch((x0, y0, x1, y1), to));
    // The members are tied to the group: their cells are still formulas of it.
    for member in [1_u32, 2] {
        let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), member).unwrap());
        let formula = d
            .shape(&d.find_shape(Owner::Page(1), member).unwrap())
            .unwrap()
            .sheet
            .cells
            .get(&diacraft_doc::CellName::new("Width"))
            .and_then(|c| c.formula.text().map(str::to_owned));
        assert!(
            formula.is_some_and(|f| f.starts_with(&format!("Sheet.{g}!Width"))),
            "the width of member {member} is still a tie to the group ({sheet:?})"
        );
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_group_named_in_a_set_is_resized_with_the_others() {
    let mut s = four();
    let g = group(&mut s, &[1, 2]);
    let before = doc(&s).clone();
    let (x0, y0, x1, y1) = joint(&before, &[g, 3, 4]);
    let to = (x0, y0, 1.5 * (x1 - x0), 2.0 * (y1 - y0));
    let answer = run(&mut s, "shape.resize", json!({"ids": [g, 3, 4], "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    assert_eq!(answer["resized"], 3, "{answer}");
    assert_corners_stretched("a set with a group", &before, doc(&s), &[g, 1, 2, 3, 4], &stretch((x0, y0, x1, y1), to));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_group_in_a_group_is_carried_by_its_numbers_and_its_own_members_follow_it() {
    let mut s = four();
    let inner = group(&mut s, &[1, 2]);
    let outer = group(&mut s, &[inner, 3]);
    let before = doc(&s).clone();
    let (x0, y0, x1, y1) = joint(&before, &[outer]);
    let to = (x0 + 0.5, y0 - 0.5, 2.0 * (x1 - x0), 1.5 * (y1 - y0));
    run(&mut s, "shape.resize", json!({"ids": [outer], "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    let d = doc(&s);
    assert_corners_stretched("a group in a group", &before, d, &[outer, inner, 1, 2, 3], &stretch((x0, y0, x1, y1), to));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_group_resized_is_one_undo_step_and_undo_restores_it_with_its_members() {
    let mut s = four();
    let g = group(&mut s, &[1, 2]);
    let before = snapshot(&s);
    let (x0, y0, x1, y1) = joint(doc(&s), &[g]);
    run(&mut s, "shape.resize", json!({"id": g, "x": x0, "y": y0, "width": x1 - x0 + 2.0, "height": y1 - y0 + 1.0}));
    assert_eq!(snapshot(&s).undo_steps, before.undo_steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, before.document);
}

// ---- turn --------------------------------------------------------------------------------------------------------------------------

#[test]
fn three_shapes_turn_ninety_degrees_about_the_centre_of_their_joint_box() {
    let mut s = blank_session();
    let ids = [drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "ellipse", 6.0, 3.0), drop_shape(&mut s, "diamond", 4.0, 6.0)];
    let before = doc(&s).clone();
    let (x0, y0, x1, y1) = joint(&before, &ids);
    let centre = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let answer = run(&mut s, "shape.rotate", json!({"ids": ids, "by": 90}));
    assert_eq!(answer["rotated"], 3);
    let d = doc(&s);
    for id in ids {
        assert!(near(num(d, id, "Angle"), std::f64::consts::FRAC_PI_2), "shape {id}");
        let was = pin(&before, id);
        let want = (centre.0 - (was.1 - centre.1), centre.1 + (was.0 - centre.0));
        assert!(near_p(pin(d, id), want), "the pin of shape {id} is {:?}, want {want:?}", pin(d, id));
    }
    // The joint box is the old one turned a quarter about its centre.
    let now = joint(d, &ids);
    let (hw, hh) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    assert!(near(now.0, centre.0 - hh) && near(now.2, centre.0 + hh) && near(now.1, centre.1 - hw) && near(now.3, centre.1 + hw), "{now:?}");
}

#[test]
fn a_set_turns_about_the_point_it_is_given() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "rectangle", 6.0, 2.0));
    run(&mut s, "shape.rotate", json!({"ids": [a, b], "by": 90, "about": {"x": 0.0, "y": 0.0}}));
    let d = doc(&s);
    assert!(near_p(pin(d, a), (-2.0, 2.0)) && near_p(pin(d, b), (-2.0, 6.0)), "{:?} {:?}", pin(d, a), pin(d, b));
    // One shape too: its pin goes round the point, and its angle goes up.
    run(&mut s, "shape.rotate", json!({"ids": [a], "by": 90, "about": {"x": -2.0, "y": 0.0}}));
    assert!(near_p(pin(doc(&s), a), (-4.0, 0.0)), "{:?}", pin(doc(&s), a));
    assert!(near(num(doc(&s), a, "Angle").abs(), std::f64::consts::PI), "{}", num(doc(&s), a, "Angle"));
    // `about` goes with `by`, and is a point.
    let e = error_of(&mut s, "shape.rotate", &json!({"ids": [a], "angle": 10, "about": {"x": 0.0, "y": 0.0}}));
    assert!(message(&e).contains("`about` goes with `by`"), "{e}");
    let e = error_of(&mut s, "shape.rotate", &json!({"ids": [a], "by": 10, "about": {"x": 0.0}}));
    assert!(message(&e).contains("missing `y`"), "{e}");
    assert_eq!(check_session(&s), Ok(()));
}

// ---- locks -------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_shape_locked_against_sizing_is_left_alone_and_the_others_are_resized() {
    // A shape is left alone when both its size locks are set, or one of them with `LockAspect`; one lock alone sizes it along its other axis (`resize_locks.rs`).
    let mut s = blank_session();
    let (a, b, c) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "rectangle", 6.0, 2.0), drop_shape(&mut s, "ellipse", 4.0, 5.0));
    set_cell(&mut s, b, "LockWidth", "1");
    set_cell(&mut s, b, "LockHeight", "1");
    let before = doc(&s).clone();
    let (x0, y0, x1, y1) = joint(&before, &[a, b, c]);
    let to = (x0, y0, 2.0 * (x1 - x0), 2.0 * (y1 - y0));
    let answer = run(&mut s, "shape.resize", json!({"ids": [a, b, c], "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    assert_eq!((answer["resized"].as_u64(), &answer["locked"]), (Some(2), &json!([b])), "{answer}");
    let d = doc(&s);
    assert_eq!((pin(d, b), size(d, b)), (pin(&before, b), size(&before, b)), "the locked shape is as it was, in size and in place");
    assert_corners_stretched("the others", &before, d, &[a, c], &stretch((x0, y0, x1, y1), to));
    // A height lock with the aspect lock does the same.
    set_cell(&mut s, c, "LockHeight", "1");
    set_cell(&mut s, c, "LockAspect", "1");
    let now = doc(&s).clone();
    let (x0, y0, x1, y1) = joint(&now, &[a, b, c]);
    let answer = run(&mut s, "shape.resize", json!({"ids": [a, b, c], "x": x0, "y": y0, "width": 0.5 * (x1 - x0), "height": 0.5 * (y1 - y0)}));
    assert_eq!((answer["resized"].as_u64(), &answer["locked"]), (Some(1), &json!([b, c])), "{answer}");
    assert_eq!(pin(doc(&s), c), pin(&now, c));
}

#[test]
fn when_every_shape_is_locked_against_sizing_the_resize_is_refused_in_words_and_changes_nothing() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "rectangle", 6.0, 2.0));
    set_cell(&mut s, a, "LockWidth", "1");
    set_cell(&mut s, a, "LockHeight", "1");
    set_cell(&mut s, b, "LockHeight", "1");
    set_cell(&mut s, b, "LockAspect", "1");
    let before = snapshot(&s);
    let e = error_of(&mut s, "shape.resize", &json!({"ids": [a, b], "x": 0.0, "y": 0.0, "width": 20.0, "height": 10.0}));
    assert!(
        message(&e).contains(
            "the shapes are locked against sizing (`LockWidth` with `LockHeight`, or either with `LockAspect`, or either on a connector): shape "
        ) && message(&e).contains(&format!("{a}, {b}")),
        "{e}"
    );
    let now = snapshot(&s);
    assert_eq!((now.document, now.undo_steps, now.revision), (before.document, before.undo_steps, before.revision));
}

#[test]
fn a_shape_locked_to_its_proportions_is_stretched_by_the_same_factor_on_both_axes() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "ellipse", 6.0, 2.0));
    set_cell(&mut s, a, "LockAspect", "1");
    let before = doc(&s).clone();
    let (x0, y0, x1, y1) = joint(&before, &[a, b]);
    run(&mut s, "shape.resize", json!({"ids": [a, b], "x": x0, "y": y0, "width": 4.0 * (x1 - x0), "height": (y1 - y0)}));
    let d = doc(&s);
    let (was, now) = (size(&before, a), size(d, a));
    // The factors are 4 along x and 1 along y: a shape that keeps its proportions comes to the geometric mean of the two, 2, on both.
    assert!(near_p(now, (was.0 * 2.0, was.1 * 2.0)), "{now:?} from {was:?}");
    assert!(near_p(size(d, b), (size(&before, b).0 * 4.0, size(&before, b).1)), "the other shape is stretched as the box is");
}

#[test]
fn a_shape_locked_against_turning_is_left_alone_and_the_others_turn() {
    let mut s = blank_session();
    let (a, b, c) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "rectangle", 6.0, 2.0), drop_shape(&mut s, "rectangle", 4.0, 6.0));
    set_cell(&mut s, b, "LockRotate", "1");
    let before = doc(&s).clone();
    let (x0, y0, x1, y1) = joint(&before, &[a, b, c]);
    let centre = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let answer = run(&mut s, "shape.rotate", json!({"ids": [a, b, c], "by": 90}));
    assert_eq!((answer["rotated"].as_u64(), &answer["locked"]), (Some(2), &json!([b])), "{answer}");
    let d = doc(&s);
    assert_eq!((pin(d, b), num(d, b, "Angle")), (pin(&before, b), 0.0), "the locked shape stays");
    // The others turn about the centre of the joint box of all three, the locked one's box included.
    let was = pin(&before, a);
    assert!(near_p(pin(d, a), (centre.0 - (was.1 - centre.1), centre.1 + (was.0 - centre.0))), "{:?}", pin(d, a));
    assert!(near(num(d, c, "Angle"), std::f64::consts::FRAC_PI_2));
}

#[test]
fn a_turn_of_shapes_all_locked_is_refused_in_words() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    set_cell(&mut s, a, "LockRotate", "1");
    let before = snapshot(&s);
    let e = error_of(&mut s, "shape.rotate", &json!({"ids": [a], "angle": 45}));
    assert!(message(&e).contains("the shape is locked against turning (`LockRotate`): shape "), "{e}");
    let now = snapshot(&s);
    assert_eq!((now.document, now.undo_steps), (before.document, before.undo_steps));
}

// ---- refusals and snapping ---------------------------------------------------------------------------------------------------------

#[test]
fn the_params_of_a_set_resize_are_checked() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "rectangle", 6.0, 2.0));
    let both = error_of(&mut s, "shape.resize", &json!({"id": a, "ids": [a, b], "x": 0.0, "y": 0.0, "width": 2.0, "height": 2.0}));
    assert!(message(&both).contains("not both"), "{both}");
    let flat = error_of(&mut s, "shape.resize", &json!({"ids": [a, b], "x": 0.0, "y": 0.0, "width": 0.0, "height": 2.0}));
    assert!(message(&flat).contains("`width` and `height` must be more than 0"), "{flat}");
    let missing = error_of(&mut s, "shape.resize", &json!({"ids": [a, b], "x": 0.0, "y": 0.0, "width": 2.0}));
    assert!(message(&missing).contains("missing `height`"), "{missing}");
    let nowhere = error_of(&mut s, "shape.resize", &json!({"ids": [a, 99], "x": 0.0, "y": 0.0, "width": 2.0, "height": 2.0}));
    assert!(message(&nowhere).contains("no shape 99"), "{nowhere}");
    // Two shapes side by side have a joint box of height: a lone line along x has none to stretch.
    let line = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": 8.0}, "to": {"x": 3.0, "y": 8.0}})));
    let thin = error_of(&mut s, "shape.resize", &json!({"ids": [line], "x": 1.0, "y": 8.0, "width": 4.0, "height": 2.0}));
    assert!(message(&thin).contains("the shapes' joint box has no height to stretch"), "{thin}");
    let past = error_of(&mut s, "shape.resize", &json!({"ids": [a, b], "x": 0.0, "y": 0.0, "width": 1.0e9, "height": 2.0}));
    assert!(message(&past).contains("must be a number within"), "{past}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_set_resize_with_snap_keeps_the_proportions_of_the_joint_box_and_names_what_it_snapped_to() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "rectangle", 6.0, 4.0));
    let (x0, y0, x1, y1) = joint(doc(&s), &[a, b]);
    // The box as a corner handle made it, wider than it is tall; the engine keeps the proportions the shapes have.
    let answer = run(
        &mut s,
        "shape.resize",
        json!({"ids": [a, b], "x": x0, "y": y0, "width": 2.2 * (x1 - x0), "height": 1.1 * (y1 - y0), "snap": {"zoom": 96, "keep": "proportions"}}),
    );
    assert!(answer.get("snap").is_some(), "the answer tells the snap: {answer}");
    let (nx0, ny0, nx1, ny1) = joint(doc(&s), &[a, b]);
    let (before_ratio, ratio) = ((x1 - x0) / (y1 - y0), (nx1 - nx0) / (ny1 - ny0));
    assert!((ratio - before_ratio).abs() < 1e-6, "the ratio is kept: {ratio} against {before_ratio}");
    assert!(near(nx0, x0) && near(ny0, y0), "the opposite corner stays");
    // Alt off the snap words: a set of more than 512 shapes is refused when it asks for a snap, and `lock` is a word of the move.
    let e = error_of(&mut s, "shape.resize", &json!({"ids": [a, b], "x": x0, "y": y0, "width": 2.0, "height": 2.0, "snap": {"lock": "x"}}));
    assert!(message(&e).contains("lock"), "{e}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_member_of_a_turned_group_is_not_resized_as_part_of_a_set_either() {
    let mut s = four();
    let g = group(&mut s, &[1, 2]);
    run(&mut s, "shape.rotate", json!({"ids": [g], "angle": 30}));
    let before = snapshot(&s);
    let e = error_of(&mut s, "shape.resize", &json!({"ids": [1, 3], "x": 0.0, "y": 0.0, "width": 9.0, "height": 9.0}));
    assert!(message(&e).contains("a member of a turned or flipped group cannot be resized"), "{e}");
    assert_eq!(snapshot(&s).document, before.document);
}

#[test]
fn shapes_flipped_either_way_are_stretched_by_the_same_rule_and_keep_their_flips() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "diamond", 6.0, 3.0));
    run(&mut s, "shape.flip", json!({"ids": [a], "axis": "horizontal"}));
    run(&mut s, "shape.flip", json!({"ids": [b], "axis": "vertical"}));
    let before = doc(&s).clone();
    let (x0, y0, x1, y1) = joint(&before, &[a, b]);
    let to = (x0 - 1.0, y0 + 0.5, 1.5 * (x1 - x0), 2.0 * (y1 - y0));
    run(&mut s, "shape.resize", json!({"ids": [a, b], "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    let d = doc(&s);
    assert_corners_stretched("flipped shapes", &before, d, &[a, b], &stretch((x0, y0, x1, y1), to));
    assert_eq!((num(d, a, "FlipX"), num(d, a, "FlipY")), (1.0, 0.0), "the flip of the first is kept");
    assert_eq!((num(d, b, "FlipX"), num(d, b, "FlipY")), (0.0, 1.0), "and of the second");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_member_of_a_group_that_is_not_turned_is_stretched_with_the_page_and_the_group_refits() {
    let mut s = four();
    let g = group(&mut s, &[1, 2]);
    // The member 1 (inside the group) and the loose shape 3: a set of a member and a loose shape.
    let before = doc(&s).clone();
    let (x0, y0, x1, y1) = joint(&before, &[1, 3]);
    let to = (x0, y0, 1.5 * (x1 - x0), 1.5 * (y1 - y0));
    run(&mut s, "shape.resize", json!({"ids": [1, 3], "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    let d = doc(&s);
    assert_corners_stretched("a member and a loose shape", &before, d, &[1, 3], &stretch((x0, y0, x1, y1), to));
    // The group is the box of its members again.
    let (gx0, gy0, gx1, gy1) = joint(d, &[1, 2]);
    let frame = joint(d, &[g]);
    assert!(near(frame.0, gx0) && near(frame.1, gy0) && near(frame.2, gx1) && near(frame.3, gy1), "{frame:?} against {:?}", (gx0, gy0, gx1, gy1));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_inside_a_group_is_routed_again_when_the_group_is_resized() {
    // The route of a connector is rows of its own frame: a group's stretch changes the frame's numbers, so the route is made again with the rest of the edit.
    let mut s = four();
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 3.5, "y": 3.0}, "to": {"x": 6.0, "y": 4.5}})));
    let g = group(&mut s, &[1, 4, c]);
    let (x0, y0, x1, y1) = joint(doc(&s), &[g]);
    run(&mut s, "shape.resize", json!({"id": g, "x": x0, "y": y0, "width": 1.7 * (x1 - x0), "height": 2.3 * (y1 - y0)}));
    let d = doc(&s);
    let route = crate::edit::route(d, c);
    let (begin, end) = ((num(d, c, "BeginX"), num(d, c, "BeginY")), (num(d, c, "EndX"), num(d, c, "EndY")));
    assert!(near_p(route[0], begin), "the route begins at {:?}, the connector at {begin:?}", route[0]);
    assert!(near_p(*route.last().unwrap(), end), "the route ends at {:?}, the connector at {end:?}", route.last());
    assert_eq!(check_session(&s), Ok(()));
}

// ---- the Size & Position window sizes a group --------------------------------------------------------------------------

/// Where each corner of shape `id` is in the box of `of`, as fractions of its width and height (0 at the left and the bottom).
fn fractions(d: &Document, id: u32, of: u32) -> Vec<P> {
    let (x0, y0, x1, y1) = joint(d, &[of]);
    corners(d, id).iter().map(|c| ((c.0 - x0) / (x1 - x0), (c.1 - y0) / (y1 - y0))).collect()
}

/// Our rule: `shape.size` takes a group as one shape, as `shape.resize` does. The group keeps its pin, and its members follow
/// the stretch of its frame, so each stays at the same place in its box. It said "a group cannot be resized yet" before, and the Size & Position window drew Width and Height quiet for one.
#[test]
fn shape_size_sizes_a_group_and_its_members_follow() {
    let mut s = four();
    let g = group(&mut s, &[1, 2]);
    let before = doc(&s).clone();
    let held = snapshot(&s);
    let (w, h) = size(&before, g);
    let answer = run(&mut s, "shape.size", json!({"ids": [g], "width": w * 2.0, "height": h * 1.5}));
    assert_eq!(answer["sized"], 1, "{answer}");
    let d = doc(&s);
    assert!(near_p(size(d, g), (w * 2.0, h * 1.5)), "the group's own size is {:?}", size(d, g));
    assert!(near_p(pin(d, g), pin(&before, g)), "the group keeps its pin: {:?} was {:?}", pin(d, g), pin(&before, g));
    for member in [1_u32, 2] {
        let (was, now) = (fractions(&before, member, g), fractions(d, member, g));
        for (a, b) in was.iter().zip(&now) {
            assert!(near_p(*a, *b), "member {member} stays at the same place of the group's box: {a:?} became {b:?}");
        }
    }
    assert_eq!(check_session(&s), Ok(()));
    // One undo step, and the members come back with it.
    assert_eq!(snapshot(&s).undo_steps, held.undo_steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, held.document);
}

/// A group with a width only, and a group named beside a plain shape, are sized in one command; a group that already has the size is not counted.
#[test]
fn shape_size_sizes_a_group_beside_a_plain_shape_and_counts_only_what_changed() {
    let mut s = four();
    let g = group(&mut s, &[1, 2]);
    let before = doc(&s).clone();
    let answer = run(&mut s, "shape.size", json!({"ids": [g, 3], "width": 4.0}));
    assert_eq!(answer["sized"], 2, "{answer}");
    let d = doc(&s);
    assert!(near(size(d, g).0, 4.0) && near(size(d, 3).0, 4.0));
    assert!(near(size(d, g).1, size(&before, g).1), "the height was not asked for");
    let again = run(&mut s, "shape.size", json!({"ids": [g], "width": 4.0}));
    assert_eq!(again["sized"], 0, "{again}");
    assert_eq!(check_session(&s), Ok(()));
}
