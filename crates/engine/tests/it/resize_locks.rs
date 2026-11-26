//! A shape with one size lock is sized along its other axis: `LockHeight` keeps the shape's own height and the stretch sizes its
//! width, `LockWidth` the reverse; both locks, or one of them with `LockAspect`, leave the shape alone and list it in `locked`.
//!
//! Numbers are paper inches on pages of the default scale. A shape's middle goes where the stretch of the joint box puts it, locked or not (the rule of `draw/scale.rs`, which
//! a shape with `LockAspect` already follows), so the expected places are worked out from the corners the shapes had before the command.

use diacraft_doc::{CellAddr, CellEdit, Document, Owner, SheetRef, Value, WriteOrigin};
use diacraft_engine::draw::shape_corners;
use diacraft_engine::{EngineError, Session};
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape, id_of, num};
use crate::group_geometry::{group, shape_of};
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

/// The box of the shapes on the page: (left, bottom, right, top).
fn joint(d: &Document, ids: &[u32]) -> (f64, f64, f64, f64) {
    let points: Vec<P> = ids.iter().flat_map(|id| corners(d, *id)).collect();
    (
        points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
        points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
        points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max),
        points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
    )
}

/// The middle of the box shape `id` draws, on the page.
fn middle(d: &Document, id: u32) -> P {
    let c = corners(d, id);
    ((c[0].0 + c[2].0) / 2.0, (c[0].1 + c[2].1) / 2.0)
}

fn size(d: &Document, id: u32) -> P {
    (num(d, id, "Width"), num(d, id, "Height"))
}

fn pin(d: &Document, id: u32) -> P {
    (num(d, id, "PinX"), num(d, id, "PinY"))
}

/// Where the stretch that takes the box `from` to the box `(x, y, width, height)` puts a point.
fn stretch(from: (f64, f64, f64, f64), (x, y, width, height): (f64, f64, f64, f64)) -> impl Fn(P) -> P {
    let (sx, sy) = (width / (from.2 - from.0), height / (from.3 - from.1));
    move |p: P| (x + (p.0 - from.0) * sx, y + (p.1 - from.1) * sy)
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

/// Gives a cell of shape `id` a formula, as the formula bar does (the cells that read it are worked out).
fn set_formula(s: &mut Session, id: u32, cell: &str, formula: &str) {
    run(s, "sheet.set_cell", json!({"id": id, "cell": cell, "formula": formula}));
}

/// A rectangle 2 inches wide and 1 high with its lower-left corner at (1, 1): the shape of the specification's vectors.
fn two_by_one(s: &mut Session, x: f64, y: f64) -> u32 {
    let id = drop_shape(s, "rectangle", 5.0, 5.0);
    run(s, "shape.resize", json!({"id": id, "x": x, "y": y, "width": 2.0, "height": 1.0}));
    id
}

/// Resizes the joint box of `ids` by the factors `fx`, `fy` about its lower-left corner (a corner handle dragged out), and answers the command's result.
fn stretched_by(s: &mut Session, ids: &[u32], fx: f64, fy: f64) -> Json {
    let (x0, y0, x1, y1) = joint(doc(s), ids);
    run(s, "shape.resize", json!({"ids": ids, "x": x0, "y": y0, "width": fx * (x1 - x0), "height": fy * (y1 - y0)}))
}

fn error_of(s: &mut Session, id: &str, params: &Json) -> EngineError {
    s.execute(id, params).expect_err("the command is refused")
}

#[test]
fn a_shape_with_lock_height_is_widened_by_shape_resize_and_keeps_its_height() {
    let mut s = blank_session();
    let a = two_by_one(&mut s, 1.0, 1.0);
    set_cell(&mut s, a, "LockHeight", "1");
    let before = doc(&s).clone();
    let from = joint(&before, &[a]);
    assert!(near_p((from.2 - from.0, from.3 - from.1), (2.0, 1.0)), "the shape starts 2 by 1: {from:?}");
    // A corner dragged out to 3 by 4 inches: the shape comes to 3 by 1.
    let to = (1.0, 1.0, 3.0, 4.0);
    let answer = run(&mut s, "shape.resize", json!({"ids": [a], "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    assert_eq!(answer["resized"], 1, "{answer}");
    assert!(answer.get("locked").is_none(), "a shape that is sized along one axis is not one left alone: {answer}");
    let d = doc(&s);
    assert!(near_p(size(d, a), (3.0, 1.0)), "3 by 1, not 3 by 4 and not as it was: {:?}", size(d, a));
    assert!(near_p(middle(d, a), stretch(from, to)(middle(&before, a))), "the middle goes where the stretch puts it: {:?}", middle(d, a));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_shape_with_lock_width_is_made_taller_and_keeps_its_width() {
    let mut s = blank_session();
    let a = two_by_one(&mut s, 1.0, 1.0);
    set_cell(&mut s, a, "LockWidth", "1");
    let before = doc(&s).clone();
    let from = joint(&before, &[a]);
    // The same drag: the shape comes to 2 by 4.
    let to = (1.0, 1.0, 3.0, 4.0);
    let answer = run(&mut s, "shape.resize", json!({"ids": [a], "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    assert_eq!(answer["resized"], 1, "{answer}");
    assert!(answer.get("locked").is_none(), "{answer}");
    let d = doc(&s);
    assert!(near_p(size(d, a), (2.0, 4.0)), "2 by 4: {:?}", size(d, a));
    assert!(near_p(middle(d, a), stretch(from, to)(middle(&before, a))), "{:?}", middle(d, a));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_shape_with_both_locks_is_left_alone_and_listed_in_locked() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "rectangle", 6.0, 3.0));
    set_cell(&mut s, a, "LockWidth", "1");
    set_cell(&mut s, a, "LockHeight", "1");
    let before = doc(&s).clone();
    let from = joint(&before, &[a, b]);
    let to = (from.0, from.1, 2.0 * (from.2 - from.0), 2.0 * (from.3 - from.1));
    let answer = run(&mut s, "shape.resize", json!({"ids": [a, b], "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    assert_eq!((answer["resized"].as_u64(), &answer["locked"]), (Some(1), &json!([a])), "{answer}");
    let d = doc(&s);
    assert_eq!((pin(d, a), size(d, a)), (pin(&before, a), size(&before, a)), "the shape with both locks is as it was, in size and in place");
    let map = stretch(from, to);
    assert!(near_p(size(d, b), (size(&before, b).0 * 2.0, size(&before, b).1 * 2.0)), "the other shape is stretched as the box is");
    assert!(near_p(middle(d, b), map(middle(&before, b))), "{:?}", middle(d, b));
    // When every shape named is left alone the resize is refused in words and nothing changes.
    let before = snapshot(&s);
    let e = error_of(&mut s, "shape.resize", &json!({"ids": [a], "x": 0.0, "y": 0.0, "width": 20.0, "height": 10.0}));
    assert!(e.to_string().contains("the shape is locked against sizing (") && e.to_string().contains(&format!("shape {a};")), "{e}");
    let now = snapshot(&s);
    assert_eq!((now.document, now.undo_steps, now.revision), (before.document, before.undo_steps, before.revision));
}

#[test]
fn lock_aspect_with_one_lock_is_left_alone_and_listed() {
    let mut s = blank_session();
    let (a, b, c) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "rectangle", 5.0, 2.0), drop_shape(&mut s, "ellipse", 8.0, 5.0));
    set_cell(&mut s, a, "LockAspect", "1");
    set_cell(&mut s, a, "LockHeight", "1");
    set_cell(&mut s, b, "LockAspect", "1");
    set_cell(&mut s, b, "LockWidth", "1");
    let before = doc(&s).clone();
    let from = joint(&before, &[a, b, c]);
    let to = (from.0, from.1, 2.0 * (from.2 - from.0), 3.0 * (from.3 - from.1));
    let answer = run(&mut s, "shape.resize", json!({"ids": [a, b, c], "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    assert_eq!((answer["resized"].as_u64(), &answer["locked"]), (Some(1), &json!([a, b])), "{answer}");
    let d = doc(&s);
    for id in [a, b] {
        assert_eq!((pin(d, id), size(d, id)), (pin(&before, id), size(&before, id)), "shape {id} is as it was");
    }
    assert!(near_p(size(d, c), (size(&before, c).0 * 2.0, size(&before, c).1 * 3.0)), "the free shape is stretched as the box is");
    // With LockAspect alone the shape is still stretched by one factor on both axes (the geometric mean), as before.
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "ellipse", 6.0, 2.0));
    set_cell(&mut s, a, "LockAspect", "1");
    let before = doc(&s).clone();
    stretched_by(&mut s, &[a, b], 4.0, 1.0);
    let d = doc(&s);
    assert!(near_p(size(d, a), (size(&before, a).0 * 2.0, size(&before, a).1 * 2.0)), "{:?}", size(d, a));
}

#[test]
fn a_selection_of_a_locked_and_a_free_shape_sizes_each_by_its_own_locks() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 2.0);
    let c = drop_shape(&mut s, "ellipse", 2.0, 6.0);
    let d4 = drop_shape(&mut s, "rectangle", 6.0, 6.0);
    set_cell(&mut s, a, "LockHeight", "1");
    set_cell(&mut s, c, "LockWidth", "1");
    set_cell(&mut s, d4, "LockWidth", "1");
    set_cell(&mut s, d4, "LockHeight", "1");
    let before = doc(&s).clone();
    let ids = [a, b, c, d4];
    let from = joint(&before, &ids);
    let (fx, fy) = (2.0, 3.0);
    let to = (from.0, from.1, fx * (from.2 - from.0), fy * (from.3 - from.1));
    let answer = run(&mut s, "shape.resize", json!({"ids": ids, "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    assert_eq!((answer["resized"].as_u64(), &answer["locked"]), (Some(3), &json!([d4])), "only the shape left alone is listed: {answer}");
    let now = doc(&s);
    let map = stretch(from, to);
    let (was, got) = (|id: u32| size(&before, id), |id: u32| size(now, id));
    assert!(near_p(got(a), (was(a).0 * fx, was(a).1)), "LockHeight: the width is stretched, the height kept: {:?}", got(a));
    assert!(near_p(got(b), (was(b).0 * fx, was(b).1 * fy)), "no lock: both axes: {:?}", got(b));
    assert!(near_p(got(c), (was(c).0, was(c).1 * fy)), "LockWidth: the height is stretched, the width kept: {:?}", got(c));
    assert_eq!((pin(now, d4), got(d4)), (pin(&before, d4), was(d4)), "both locks: as it was");
    for id in [a, b, c] {
        assert!(near_p(middle(now, id), map(middle(&before, id))), "the middle of shape {id} is carried by the stretch: {:?}", middle(now, id));
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_turned_shape_keeps_its_own_height_and_not_the_pages() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "rectangle", 6.0, 4.0));
    run(&mut s, "shape.rotate", json!({"ids": [a], "angle": 90}));
    set_cell(&mut s, a, "LockHeight", "1");
    let before = doc(&s).clone();
    stretched_by(&mut s, &[a, b], 2.0, 3.0);
    let d = doc(&s);
    // The shape's own height lies along the page's x axis, where the stretch is 2, and its own width along y, where it is 3: the own width is stretched, the own height kept.
    assert!(near_p(size(d, a), (size(&before, a).0 * 3.0, size(&before, a).1)), "{:?} from {:?}", size(d, a), size(&before, a));
    assert!(near(num(d, a, "Angle"), std::f64::consts::FRAC_PI_2), "the turn is kept");
    assert!(near_p(size(d, b), (size(&before, b).0 * 2.0, size(&before, b).1 * 3.0)), "the upright shape is stretched on both");
}

#[test]
fn a_connector_with_one_size_lock_is_left_alone_and_listed() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "rectangle", 6.0, 4.0));
    let line = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": 8.0}, "to": {"x": 3.0, "y": 8.5}})));
    set_cell(&mut s, line, "LockWidth", "1");
    let before = doc(&s).clone();
    let answer = stretched_by(&mut s, &[a, b, line], 2.0, 2.0);
    assert_eq!((answer["resized"].as_u64(), &answer["locked"]), (Some(2), &json!([line])), "a line has no other axis to size: {answer}");
    assert_eq!(pin(doc(&s), line), pin(&before, line));
    assert_eq!((num(doc(&s), line, "EndX"), num(doc(&s), line, "EndY")), (num(&before, line, "EndX"), num(&before, line, "EndY")));
}

#[test]
fn a_group_with_lock_height_and_a_height_formula_keeps_the_formula_after_a_width_resize() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "rectangle", 4.0, 2.0));
    let g = group(&mut s, &[a, b]);
    // A group whose height is a formula of its width, as a class's is of its text's lines: the height is the formula's and not the drag's.
    set_formula(&mut s, g, "Height", "Width*0.5");
    set_cell(&mut s, g, "LockHeight", "1");
    let before = doc(&s).clone();
    assert!(near(num(&before, g, "Height"), num(&before, g, "Width") * 0.5), "the formula is worked out");
    let from = joint(&before, &[g]);
    let to = (from.0, from.1, 2.0 * (from.2 - from.0), 3.0 * (from.3 - from.1));
    let answer = run(&mut s, "shape.resize", json!({"ids": [g], "x": to.0, "y": to.1, "width": to.2, "height": to.3}));
    assert_eq!(answer["resized"], 1, "{answer}");
    assert!(answer.get("locked").is_none(), "{answer}");
    let d = doc(&s);
    let height = shape_of(d, g).sheet.cells.iter().find(|(n, _)| n.as_str() == "Height").and_then(|(_, c)| c.formula.text().map(str::to_owned));
    assert_eq!(height.as_deref(), Some("Width*0.5"), "the formula is still the cell's");
    assert!(near(num(d, g, "Width"), 2.0 * num(&before, g, "Width")), "the width is stretched: {}", num(d, g, "Width"));
    assert!(near(num(d, g, "Height"), num(d, g, "Width") * 0.5), "the height is what the formula says: {}", num(d, g, "Height"));
    // The middle goes where the stretch puts it, though the formula gave the height another value than the drag's plan.
    assert!(near_p(middle(d, g), stretch(from, to)(middle(&before, g))), "{:?}", middle(d, g));
    // The members follow the group by their own ties.
    for id in [a, b] {
        let share = |dd: &Document| num(dd, id, "Height") / num(dd, g, "Height");
        assert!(near(share(d), share(&before)), "member {id} keeps its share of the group's height");
    }
    assert_eq!(check_session(&s), Ok(()));
}
