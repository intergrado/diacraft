//! After an edit of a member of a group, the group's frame in the session is the box of its members again, as it
//! was when the group was made (`shape.group`) and as a drawing read from a `.odg` has it. The members stay where they are
//! on the page: their cells are carried into the new frame.
//!
//! Every test asks the engine, through its commands, for the frame of the group (`shape.placement`: `Width`, `Height`, `PinX`, `PinY` and the local
//! pin), and holds it to two things that do not read the code under test: the union of the corners of the members on the page, and what the file says
//! when the drawing is saved to a `.odg` and read again (a group has no frame in that file: the reader makes it from the members).

use diacraft_doc::Document;
#[cfg(target_os = "linux")]
use diacraft_engine::History;
use diacraft_engine::Session;
use diacraft_engine::draw::shape_corners;
use serde_json::{Value, json};

use crate::odg_build::{Dir, open_drawing};
use crate::odg_file::{assert_saves_again_as_no_edit, four, save};
use crate::run;

type P = (f64, f64);

/// The first page of a drawing read from a `.odg`: the reader numbers pages from 0.
const PAGE: u32 = 0;

/// How near two numbers of a page, in inches, are the same when both are in the session: a cell holds nine places.
const TOL: f64 = 1.0e-7;

/// How near the session's frame is to the one a file gives it: a `.odg` holds lengths in centimetres to five places.
const FILE_TOL: f64 = 1.0e-4;

/// A group's frame as `shape.placement` gives it, in inches.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Frame {
    pin: P,
    local_pin: P,
    width: f64,
    height: f64,
}

impl Frame {
    /// The corners of the box on the page (an unturned group).
    fn low(&self) -> P {
        (self.pin.0 - self.local_pin.0, self.pin.1 - self.local_pin.1)
    }

    fn high(&self) -> P {
        let low = self.low();
        (low.0 + self.width, low.1 + self.height)
    }
}

fn number(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap_or_else(|| panic!("no number `{key}` in {v}"))
}

/// The frame of shape `id` on the first page of the session.
fn frame_in(s: &mut Session, id: u32) -> Frame {
    let answer = run(s, "shape.placement", json!({ "ids": [id], "page": PAGE }));
    let shape = &answer["shapes"][0];
    Frame {
        pin: (number(shape, "x"), number(shape, "y")),
        local_pin: (number(&shape["pin"], "x"), number(&shape["pin"], "y")),
        width: number(shape, "width"),
        height: number(shape, "height"),
    }
}

/// The box that holds the corners of `ids` on the page.
fn union_of(doc: &Document, ids: &[u32]) -> (P, P) {
    let mut low = (f64::MAX, f64::MAX);
    let mut high = (f64::MIN, f64::MIN);
    for id in ids {
        for c in shape_corners(doc, PAGE, *id).unwrap_or_else(|| panic!("no shape {id}")) {
            low = (low.0.min(c.0), low.1.min(c.1));
            high = (high.0.max(c.0), high.1.max(c.1));
        }
    }
    (low, high)
}

#[track_caller]
fn assert_near(what: &str, got: P, want: P, tol: f64) {
    assert!((got.0 - want.0).abs() < tol && (got.1 - want.1).abs() < tol, "{what}: got {got:?}, want {want:?}");
}

/// The group's frame is the box of its members `members`, the ids of shapes on the first page.
#[track_caller]
fn assert_framed_by(s: &mut Session, group: u32, members: &[u32], what: &str) {
    let frame = frame_in(s, group);
    let (low, high) = union_of(s.document().unwrap(), members);
    assert_near(&format!("{what}: the group's lower-left corner"), frame.low(), low, TOL);
    assert_near(&format!("{what}: the group's upper-right corner"), frame.high(), high, TOL);
    assert_near(&format!("{what}: the pin is the middle of the box"), frame.pin, ((low.0 + high.0) / 2.0, (low.1 + high.1) / 2.0), TOL);
    assert_near(&format!("{what}: the local pin is half the size"), frame.local_pin, (frame.width / 2.0, frame.height / 2.0), TOL);
}

/// A session of the file `a.odg` that the first one saved: what a reader of that file makes of the drawing.
fn reopened(dir: &Dir) -> Session {
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
    again
}

/// Saves the session's drawing as its `.odg`, reads it again, and holds the frame of the group `group` (in the session) to the frame of the group at
/// `nth` among the groups of the first page of the file. Returns the saved answer.
#[track_caller]
fn assert_file_agrees(dir: &Dir, s: &mut Session, group: u32, nth: usize, what: &str) -> Value {
    let saved = save(s);
    let mut again = reopened(dir);
    let in_file = *groups_preorder(again.document().unwrap()).get(nth).unwrap_or_else(|| panic!("{what}: the file holds no group number {nth}"));
    let (mine, theirs) = (frame_in(s, group), frame_in(&mut again, in_file));
    assert_near(&format!("{what}: the lower-left corner the file gives"), mine.low(), theirs.low(), FILE_TOL);
    assert_near(&format!("{what}: the size the file gives"), (mine.width, mine.height), (theirs.width, theirs.height), FILE_TOL);
    assert_near(&format!("{what}: the pin the file gives"), mine.pin, theirs.pin, FILE_TOL);
    saved
}

/// Three shapes dropped on the first page of the `.odg` a session has open, and two of them grouped: `(a, b, c, group)`, `c` beside the group.
fn grouped_pair(s: &mut Session) -> (u32, u32, u32, u32) {
    let id = |v: Value| u32::try_from(v["id"].as_u64().unwrap()).unwrap();
    let a = id(run(s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 6.0, "y": 6.0 })));
    let b = id(run(s, "shape.drop", json!({ "master": "ellipse", "page": 0, "x": 8.0, "y": 7.0 })));
    let c = id(run(s, "shape.drop", json!({ "master": "diamond", "page": 0, "x": 4.0, "y": 9.0 })));
    let group = id(run(s, "shape.group", json!({ "page": 0, "ids": [a, b] })));
    (a, b, c, group)
}

#[test]
fn a_member_moved_out_of_its_group_takes_the_groups_frame_with_it() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let (a, b, _c, group) = grouped_pair(&mut s);
    assert_framed_by(&mut s, group, &[a, b], "as grouped");
    let corners_of_b = shape_corners(s.document().unwrap(), PAGE, b).unwrap();
    run(&mut s, "shape.move", json!({ "ids": [a], "dx": -3.0, "dy": 2.5 }));
    assert_framed_by(&mut s, group, &[a, b], "after a is moved out to the left and up");
    assert_eq!(shape_corners(s.document().unwrap(), PAGE, b).unwrap(), corners_of_b, "the member that was not moved stays where it was on the page");
    let saved = assert_file_agrees(&dir, &mut s, group, 0, "after the move");
    assert_eq!(saved["edited"], true, "{saved}");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

// ---- the edits of a member -------------------------------------------------------------------------------------------------------------

/// The groups of the first page in the pre-order of the file: a group, then the groups inside it.
fn groups_preorder(doc: &Document) -> Vec<u32> {
    let mut out = Vec::new();
    let mut stack: Vec<&std::sync::Arc<diacraft_doc::Shape>> = doc.pages.get(0).unwrap().shapes.iter().rev().collect();
    while let Some(shape) = stack.pop() {
        if !shape.children.is_empty() {
            out.push(shape.id);
            stack.extend(shape.children.iter().rev());
        }
    }
    out
}

/// A new id from the answer of a command that makes a shape.
fn made(answer: &Value) -> u32 {
    u32::try_from(answer["id"].as_u64().unwrap()).unwrap()
}

/// Writes the cell `name` of shape `id` on the first page straight into the document, as an edit of the ShapeSheet would.
fn write_cell(s: &mut Session, id: u32, name: &str, cell: diacraft_doc::Cell) {
    s.edit("write a cell", |doc| {
        let at = doc.find_shape(diacraft_doc::Owner::Page(PAGE), id).expect("the shape is on the page");
        let write = diacraft_doc::CellWrite {
            addr: diacraft_doc::CellAddr::direct(name),
            edit: diacraft_doc::CellEdit::Cell(cell),
            origin: diacraft_doc::WriteOrigin::Drawing,
        };
        doc.write_batch(vec![(diacraft_doc::SheetRef::Shape(at), write)]).unwrap();
        Ok(())
    })
    .unwrap();
}

/// The cell `name` the shape `id` holds itself: its formula's text and its number.
fn own_cell(doc: &Document, id: u32, name: &str) -> Option<(Option<String>, Option<f64>)> {
    let at = doc.find_shape(diacraft_doc::Owner::Page(PAGE), id)?;
    let cell = doc.shape(&at)?.sheet.cells.get(&diacraft_doc::CellName::new(name))?;
    Some((cell.formula.text().map(str::to_owned), cell.value.as_ref().and_then(diacraft_doc::Value::number)))
}

#[test]
fn a_member_resized_makes_the_frame_larger_and_then_smaller_and_the_file_agrees_each_time() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let (a, b, _c, group) = grouped_pair(&mut s);
    run(&mut s, "shape.size", json!({ "ids": [a], "width": 4.0, "height": 3.0, "page": PAGE }));
    assert_framed_by(&mut s, group, &[a, b], "after a is made 4 by 3");
    assert_file_agrees(&dir, &mut s, group, 0, "the larger frame");
    // The group is now one of the file: its next edit is an edit of a file's group.
    run(&mut s, "shape.size", json!({ "ids": [a], "width": 0.5, "height": 0.5, "page": PAGE }));
    assert_framed_by(&mut s, group, &[a, b], "after a is made small");
    let saved = assert_file_agrees(&dir, &mut s, group, 0, "the smaller frame");
    assert_eq!(saved["edited"], true, "{saved}");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

#[test]
fn a_member_turned_makes_the_frame_the_box_of_its_turned_corners() {
    // Not saved: a new group with a member that is turned is refused by the writer as soon as it is made, with or without this rule (the file holds an angle
    // to nine places and a cell holds thirteen, so the box of the member's corners in the file is not the one in the session to a trillionth of an inch).
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let (a, b, _c, group) = grouped_pair(&mut s);
    for angle in [90.0, 45.0, 20.0, 133.0, -70.0] {
        run(&mut s, "shape.rotate", json!({ "ids": [a], "angle": angle, "page": PAGE }));
        assert_framed_by(&mut s, group, &[a, b], &format!("after a is turned to {angle} degrees"));
    }
}

#[test]
fn a_member_moved_along_one_axis_only_takes_the_frame_with_it_along_that_axis() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let (a, b, _c, group) = grouped_pair(&mut s);
    let b_was = shape_corners(s.document().unwrap(), PAGE, b).unwrap();
    for (dx, dy) in [(-3.0, 0.0), (0.0, 2.5), (2.0, 0.0), (0.0, -4.0)] {
        run(&mut s, "shape.move", json!({ "ids": [a], "dx": dx, "dy": dy, "page": PAGE }));
        assert_framed_by(&mut s, group, &[a, b], &format!("after a is moved by ({dx}, {dy})"));
        assert_eq!(shape_corners(s.document().unwrap(), PAGE, b).unwrap(), b_was, "b stays where it was on the page");
    }
    assert_file_agrees(&dir, &mut s, group, 0, "after the moves");
}

#[test]
fn a_member_deleted_makes_the_frame_the_box_of_the_ones_that_are_left() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let id = |v: Value| made(&v);
    let a = id(run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": PAGE, "x": 6.0, "y": 6.0 })));
    let b = id(run(&mut s, "shape.drop", json!({ "master": "ellipse", "page": PAGE, "x": 8.0, "y": 7.0 })));
    let c = id(run(&mut s, "shape.drop", json!({ "master": "diamond", "page": PAGE, "x": 4.0, "y": 9.0 })));
    let group = id(run(&mut s, "shape.group", json!({ "page": PAGE, "ids": [a, b, c] })));
    assert_framed_by(&mut s, group, &[a, b, c], "as grouped");
    run(&mut s, "shape.delete", json!({ "ids": [c], "page": PAGE }));
    assert_framed_by(&mut s, group, &[a, b], "after c is deleted");
    assert_file_agrees(&dir, &mut s, group, 0, "the group of two");
    // One member left: the group is its box.
    run(&mut s, "shape.delete", json!({ "ids": [a], "page": PAGE }));
    assert_framed_by(&mut s, group, &[b], "after a is deleted too");
}

#[test]
fn a_member_whose_text_sizes_it_takes_the_frame_with_it() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let (a, b, _c, group) = grouped_pair(&mut s);
    // The width of `a` is the width of its text and an inch (a formula, so it is no tie to the group).
    let width = diacraft_doc::Cell::with_formula("TEXTWIDTH(TheText)+1", Some(diacraft_doc::Value::from_text("1.5"))).with_unit("IN");
    write_cell(&mut s, a, "Width", width);
    assert_framed_by(&mut s, group, &[a, b], "with the formula");
    let narrow = frame_in(&mut s, group).width;
    run(&mut s, "shape.set_text", json!({ "id": a, "text": "a good deal of text that makes a wide shape of a narrow one", "page": PAGE }));
    assert_framed_by(&mut s, group, &[a, b], "after the text of a is long");
    let wide = frame_in(&mut s, group).width;
    assert!(wide > narrow + 1.0, "the text made the member, and with it the group, wider: {narrow} then {wide}");
    run(&mut s, "shape.set_text", json!({ "id": a, "text": "", "page": PAGE }));
    assert_framed_by(&mut s, group, &[a, b], "after the text of a is taken away");
    assert!(frame_in(&mut s, group).width < wide - 1.0);
}

#[test]
fn a_member_of_a_group_in_a_group_refits_both_and_the_file_agrees_with_each() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let (a, b, c, inner) = grouped_pair(&mut s);
    let outer = made(&run(&mut s, "shape.group", json!({ "page": PAGE, "ids": [inner, c] })));
    assert_framed_by(&mut s, inner, &[a, b], "inner, as grouped");
    assert_framed_by(&mut s, outer, &[a, b, c], "outer, as grouped");
    let corners_of = |s: &Session, id: u32| shape_corners(s.document().unwrap(), PAGE, id).unwrap();
    let (b_was, c_was) = (corners_of(&s, b), corners_of(&s, c));
    run(&mut s, "shape.move", json!({ "ids": [a], "dx": 2.0, "dy": -3.0, "page": PAGE }));
    assert_framed_by(&mut s, inner, &[a, b], "inner, after a is moved");
    assert_framed_by(&mut s, outer, &[a, b, c], "outer, after a is moved");
    assert_eq!((corners_of(&s, b), corners_of(&s, c)), (b_was, c_was), "the others stay where they were on the page");
    // The pre-order of the file: the outer group, then the inner.
    assert_eq!(groups_preorder(s.document().unwrap()), [outer, inner]);
    assert_file_agrees(&dir, &mut s, outer, 0, "the outer group");
    assert_file_agrees(&dir, &mut s, inner, 1, "the inner group");
    // Resized from the inside, and a member of the inner taken away: the same.
    run(&mut s, "shape.size", json!({ "ids": [b], "width": 5.0, "height": 0.4, "page": PAGE }));
    assert_framed_by(&mut s, inner, &[a, b], "inner, after b is made wide");
    assert_framed_by(&mut s, outer, &[a, b, c], "outer, after b is made wide");
    run(&mut s, "shape.delete", json!({ "ids": [b], "page": PAGE }));
    assert_framed_by(&mut s, inner, &[a], "inner, after b is deleted");
    assert_framed_by(&mut s, outer, &[a, c], "outer, after b is deleted");
}

#[test]
fn undo_and_redo_take_the_frame_and_the_member_back_and_forth_in_one_step() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let (a, b, _c, group) = grouped_pair(&mut s);
    let (doc_before, frame_before) = (s.document().unwrap().clone(), frame_in(&mut s, group));
    let steps = s.history().unwrap().undo_len();
    run(&mut s, "shape.move", json!({ "ids": [a], "dx": -4.0, "dy": 3.0, "page": PAGE }));
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "the member and the group's frame are one undo step");
    let (doc_after, frame_after) = (s.document().unwrap().clone(), frame_in(&mut s, group));
    assert_ne!(frame_before, frame_after);
    assert_framed_by(&mut s, group, &[a, b], "after the move");
    run(&mut s, "edit.undo", json!({}));
    assert!(*s.document().unwrap() == doc_before, "undo gives the document it was, the group's frame with it");
    assert_eq!(frame_in(&mut s, group), frame_before);
    assert_framed_by(&mut s, group, &[a, b], "after the undo");
    run(&mut s, "edit.redo", json!({}));
    assert!(*s.document().unwrap() == doc_after, "redo gives the document the edit made");
    assert_eq!(frame_in(&mut s, group), frame_after);
    assert_framed_by(&mut s, group, &[a, b], "after the redo");
}

#[test]
fn a_member_of_a_turned_group_refits_the_frame_in_the_groups_own_frame() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let (a, b, _c, group) = grouped_pair(&mut s);
    run(&mut s, "shape.rotate", json!({ "ids": [group], "angle": 30.0, "page": PAGE }));
    let b_was = shape_corners(s.document().unwrap(), PAGE, b).unwrap();
    run(&mut s, "shape.move", json!({ "ids": [a], "dx": 1.5, "dy": 2.0, "page": PAGE }));
    // The members' corners on the page, turned back by the group's angle: their box is the group's size, and the group's pin is the middle of that box
    // carried forward again.
    let (sin, cos) = (30.0_f64.to_radians().sin(), 30.0_f64.to_radians().cos());
    let back = |p: P| (p.0 * cos + p.1 * sin, -p.0 * sin + p.1 * cos);
    let mut low = (f64::MAX, f64::MAX);
    let mut high = (f64::MIN, f64::MIN);
    for id in [a, b] {
        for corner in shape_corners(s.document().unwrap(), PAGE, id).unwrap() {
            let p = back(corner);
            low = (low.0.min(p.0), low.1.min(p.1));
            high = (high.0.max(p.0), high.1.max(p.1));
        }
    }
    let frame = frame_in(&mut s, group);
    assert_near("the group's size is the box of its members in its own frame", (frame.width, frame.height), (high.0 - low.0, high.1 - low.1), TOL);
    let middle = ((low.0 + high.0) / 2.0, (low.1 + high.1) / 2.0);
    assert_near("the group's pin is the middle of the box", frame.pin, (middle.0 * cos - middle.1 * sin, middle.0 * sin + middle.1 * cos), TOL);
    assert_near("the local pin is half the size", frame.local_pin, (frame.width / 2.0, frame.height / 2.0), TOL);
    assert_eq!(shape_corners(s.document().unwrap(), PAGE, b).unwrap(), b_was, "the member that was not moved stays where it was");
}

#[test]
fn a_tie_of_a_member_to_the_size_of_its_group_is_written_again_for_the_new_size_and_the_member_keeps_its_own() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let (a, b, _c, group) = grouped_pair(&mut s);
    for name in ["Width", "Height", "PinX", "PinY"] {
        let (formula, _) = own_cell(s.document().unwrap(), b, name).unwrap();
        assert!(formula.is_some_and(|f| f.starts_with(&format!("Sheet.{group}!"))), "{name} of b is a tie to the group as it is grouped");
    }
    let numbers = |s: &Session| ["Width", "Height"].map(|name| own_cell(s.document().unwrap(), b, name).unwrap().1.unwrap());
    let size_of_b = numbers(&s);
    run(&mut s, "shape.move", json!({ "ids": [a], "dx": -3.0, "dy": 2.5, "page": PAGE }));
    assert_eq!(numbers(&s), size_of_b, "b is the size it was");
    let frame = frame_in(&mut s, group);
    for (name, extent) in [("Width", frame.width), ("Height", frame.height), ("PinX", frame.width), ("PinY", frame.height)] {
        let (formula, number) = own_cell(s.document().unwrap(), b, name).unwrap();
        let formula = formula.unwrap_or_else(|| panic!("{name} of b lost its tie"));
        let axis = if name.ends_with('X') || name == "Width" { "Width" } else { "Height" };
        let prefix = format!("Sheet.{group}!{axis}*");
        let factor: f64 = formula.strip_prefix(prefix.as_str()).unwrap_or_else(|| panic!("{name}: {formula}")).parse().unwrap();
        assert!((factor * extent - number.unwrap()).abs() < 1.0e-7, "{name} is {number:?} and the tie {formula} gives {}", factor * extent);
    }
}

#[test]
fn a_group_that_was_not_the_box_of_its_members_is_left_as_it_is() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let (a, _b, _c, group) = grouped_pair(&mut s);
    // A person gave the group a frame of its own: wider than its members.
    write_cell(&mut s, group, "Width", diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("9")).with_unit("IN"));
    let given = frame_in(&mut s, group);
    assert_eq!(given.width, 9.0);
    run(&mut s, "shape.move", json!({ "ids": [a], "dx": -3.0, "dy": 2.5, "page": PAGE }));
    assert_eq!(frame_in(&mut s, group), given, "the frame the group has is not imposed over");
}

#[test]
fn a_group_none_of_whose_members_an_edit_changed_is_not_touched() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let (_a, _b, c, group) = grouped_pair(&mut s);
    let held = |s: &Session| s.document().unwrap().pages.get(0).unwrap().shapes.iter().find(|shape| shape.id == group).cloned().unwrap();
    let before = held(&s);
    run(&mut s, "shape.move", json!({ "ids": [c], "dx": 1.0, "dy": 1.0, "page": PAGE }));
    assert!(std::sync::Arc::ptr_eq(&before, &held(&s)), "the group is the very shape it was");
    // The group itself moved: its members are the ones it carries, and its frame is what the move made it.
    let low = frame_in(&mut s, group).low();
    run(&mut s, "shape.move", json!({ "ids": [group], "dx": 1.0, "dy": 1.0, "page": PAGE }));
    assert_near("the group moved by (1, 1)", frame_in(&mut s, group).low(), (low.0 + 1.0, low.1 + 1.0), TOL);
}

#[test]
fn a_member_with_a_formula_for_its_pin_that_is_no_tie_keeps_the_frame_as_it_was_and_the_edit_is_made() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let (a, b, _c, group) = grouped_pair(&mut s);
    let pin_x = own_cell(s.document().unwrap(), b, "PinX").unwrap().1.unwrap();
    let held = diacraft_doc::Cell::with_formula(&format!("{pin_x}"), Some(diacraft_doc::Value::from_text(&format!("{pin_x}")))).with_unit("IN");
    write_cell(&mut s, b, "PinX", held);
    let was = frame_in(&mut s, group);
    let corners_of_a = shape_corners(s.document().unwrap(), PAGE, a).unwrap();
    run(&mut s, "shape.move", json!({ "ids": [a], "dx": -3.0, "dy": 2.5, "page": PAGE }));
    assert_ne!(shape_corners(s.document().unwrap(), PAGE, a).unwrap(), corners_of_a, "the edit of the member was made");
    assert_eq!(frame_in(&mut s, group), was, "a member whose pin is a formula of its own is data, and the frame is left");
}

#[test]
fn a_connector_glued_to_a_member_stays_on_its_connection_point_when_the_frame_around_the_member_changes() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let (a, b, c, group) = grouped_pair(&mut s);
    let wire = made(&run(&mut s, "connector.add", json!({ "from": { "id": a, "point": 3 }, "to": { "id": c, "point": 1 }, "page": PAGE })));
    let end_of = |s: &mut Session, which: &str| -> P {
        let shape = run(s, "shape.placement", json!({ "ids": [wire], "page": PAGE }))["shapes"][0].clone();
        (number(&shape[which], "x"), number(&shape[which], "y"))
    };
    // b goes far to the left of the group and up: the frame's origin moves, and a's cells with it.
    run(&mut s, "shape.move", json!({ "ids": [b], "dx": -7.0, "dy": 4.0, "page": PAGE }));
    assert_framed_by(&mut s, group, &[a, b], "after b is moved");
    let point = |s: &Session, id: u32, n: usize| {
        let points = diacraft_engine::draw::connection_points(s.document().unwrap(), PAGE, id);
        let p = points.get(n).expect("a connection point");
        (p.x, p.y)
    };
    assert_near("the begin is on a's third point", end_of(&mut s, "begin"), point(&s, a, 3), 1.0e-6);
    assert_near("the end is on c's first point", end_of(&mut s, "end"), point(&s, c, 1), 1.0e-6);
    // And it still follows c when c is moved: the formulas read the new frame.
    run(&mut s, "shape.move", json!({ "ids": [c], "dx": 1.0, "dy": -2.0, "page": PAGE }));
    assert_near("the begin is still on a's third point", end_of(&mut s, "begin"), point(&s, a, 3), 1.0e-6);
    assert_near("the end followed c", end_of(&mut s, "end"), point(&s, c, 1), 1.0e-6);
}

// ---- a group of a file ------------------------------------------------------------------------------------------------------------------

/// The `ID` of the shape called `name` anywhere on the first page.
fn id_named(doc: &Document, name: &str) -> u32 {
    let mut stack: Vec<&std::sync::Arc<diacraft_doc::Shape>> = doc.pages.get(0).unwrap().shapes.iter().collect();
    while let Some(shape) = stack.pop() {
        if shape.name_u.as_deref() == Some(name) {
            return shape.id;
        }
        stack.extend(shape.children.iter());
    }
    panic!("no shape called {name}")
}

/// `draw:g` of two rectangles and a rectangle beside it, in an `.odg` that no DiaCraft wrote: the group is the kind the review of the first milestone found
/// with a frame that stayed as the file made it once a member was edited.
fn drawing_with_a_group() -> crate::odg_build::Drawing {
    use crate::odg_build::{Drawing, rect};
    let members = [rect("M1", (2.0, 3.0, 4.0, 2.0), ""), rect("M2", (8.0, 3.0, 4.0, 2.0), "")].concat();
    Drawing::new().page(&format!("<draw:g draw:name=\"G\">{members}</draw:g>{}", rect("Z", (2.0, 12.0, 4.0, 2.0), "")))
}

#[test]
fn a_group_of_a_file_is_the_box_of_its_members_after_each_edit_of_one_and_the_saved_file_is_right() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &drawing_with_a_group());
    let group = groups_preorder(s.document().unwrap())[0];
    let doc = s.document().unwrap();
    let (m1, m2, z) = (id_named(doc, "M1"), id_named(doc, "M2"), id_named(doc, "Z"));
    assert_framed_by(&mut s, group, &[m1, m2], "as the file has it");
    let z_was = shape_corners(s.document().unwrap(), PAGE, z).unwrap();
    let original = crate::odg_build::content_of(&dir.read("a.odg"));
    // M1 goes out to the left and up, then is made small, then M2 is moved in below it.
    run(&mut s, "shape.move", json!({ "ids": [m1], "dx": -1.0, "dy": 2.0, "page": PAGE }));
    assert_framed_by(&mut s, group, &[m1, m2], "after m1 is moved");
    let saved = assert_file_agrees(&dir, &mut s, group, 0, "after the move");
    assert_eq!(saved["edited"], true, "{saved}");
    // What the move did not touch is as the file had it: M2's element, byte for byte, and Z's.
    let content = crate::odg_build::content_of(&dir.read("a.odg"));
    for name in ["M2", "Z"] {
        let start = original.find(&format!("draw:name=\"{name}\"")).unwrap();
        let element =
            &original[original[..start].rfind("<draw:custom-shape").unwrap()..start + original[start..].find("</draw:custom-shape>").unwrap()];
        assert!(content.contains(element), "{name} was rewritten by the save of a move of M1");
    }
    run(&mut s, "shape.size", json!({ "ids": [m1], "width": 0.5, "height": 0.5, "page": PAGE }));
    assert_framed_by(&mut s, group, &[m1, m2], "after m1 is made small");
    assert_file_agrees(&dir, &mut s, group, 0, "after m1 is made small");
    run(&mut s, "shape.delete", json!({ "ids": [m1], "page": PAGE }));
    assert_framed_by(&mut s, group, &[m2], "after m1 is deleted");
    assert_file_agrees(&dir, &mut s, group, 0, "after m1 is deleted");
    assert_eq!(shape_corners(s.document().unwrap(), PAGE, z).unwrap(), z_was, "the shape beside the group never moved");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

// ---- the worst case, in a child under `ulimit -v 3000000` -------------------------------------------------------------------------------

/// A group that holds the group after it, `levels` deep, round the two leaves `chain_a` and `chain_b`, then `siblings` groups of two leaves each, and `wires`
/// connectors that join a leaf of one sibling group to a leaf of the next (glued at both ends, so that every group has glued connectors round its members).
#[cfg(target_os = "linux")]
fn crowded_drawing(levels: usize, siblings: usize, wires: usize) -> Vec<u8> {
    use crate::odg_build::{Drawing, glued, rect};
    let named = |name: &str, frame: (f64, f64, f64, f64)| {
        rect(name, frame, "").replacen("<draw:custom-shape ", &format!("<draw:custom-shape xml:id=\"{name}\" draw:id=\"{name}\" "), 1)
    };
    let mut body = String::new();
    for level in 0..levels {
        body.push_str(&format!("<draw:g draw:name=\"C{level}\">"));
    }
    body.push_str(&named("chain_a", (2.0, 3.0, 1.0, 1.0)));
    body.push_str(&named("chain_b", (4.0, 3.0, 1.0, 1.0)));
    body.push_str(&"</draw:g>".repeat(levels));
    for j in 0..siblings {
        let (x, y) = ((j % 40) as f64 * 0.5, 6.0 + (j / 40) as f64 * 0.8);
        body.push_str(&format!(
            "<draw:g draw:name=\"S{j}\">{}{}</draw:g>",
            named(&format!("sa{j}"), (x, y, 0.3, 0.3)),
            named(&format!("sb{j}"), (x + 0.3, y, 0.3, 0.3))
        ));
    }
    for w in 0..wires {
        let (a, b) = (w % siblings, (w + 1) % siblings);
        body.push_str(&glued(&format!("W{w}"), &format!("sb{a}"), &format!("sa{b}"), (0.0, 6.0, 1.0, 6.5)));
    }
    Drawing::new().page(&body).bytes()
}

/// What the child does, named in the environment.
#[cfg(target_os = "linux")]
const SCENARIO: &str = "GROUP_FRAME_SCENARIO";

#[cfg(target_os = "linux")]
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("this test needs /proc/self/status (Linux): {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or_else(|| panic!("no {key} line in /proc/self/status"))
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    let (levels, siblings, wires) = (180, 1_000, 1_500);
    let dir = Dir::new();
    let path = dir.write("a.odg", &crowded_drawing(levels, siblings, wires));
    let mut s = Session::new();
    s.execute("file.open", &json!({ "path": path })).unwrap_or_else(|e| panic!("the file did not open: {e}"));
    let doc = s.document().unwrap();
    let chain: Vec<u32> = ["chain_a", "chain_b"].iter().map(|name| id_named(doc, name)).collect();
    let leaves_a: Vec<u32> = (0..siblings).map(|j| id_named(doc, &format!("sa{j}"))).collect();
    let moved: Vec<u32> = match scenario.as_str() {
        // The member of the chain and the first leaf of every sibling group, one command.
        "worst" => std::iter::once(chain[0]).chain(leaves_a.iter().copied()).collect(),
        // The yardstick: one leaf of one sibling group.
        "yardstick" => vec![leaves_a[0]],
        other => panic!("no scenario {other}"),
    };
    println!("\nSTART VmSize={}KiB", status_kib("VmSize"));
    let started = std::time::Instant::now();
    let result = s.execute("shape.move", &json!({ "ids": moved, "dx": -3.0, "dy": 1.0, "page": PAGE }));
    let took = started.elapsed();
    let class = match &result {
        Ok(_) => "moved".to_owned(),
        Err(e) => format!("refused({})", e.to_string().chars().take(160).collect::<String>()),
    };
    println!(
        "\nRESULT {scenario} class={class} groups={} ms={} VmHWM={}KiB VmPeak={}KiB",
        moved.len(),
        took.as_millis(),
        status_kib("VmHWM"),
        status_kib("VmPeak")
    );
    // A move is made, or refused in words: never any other end, and when it is made every group is the box of its members.
    if result.is_ok() {
        let doc = s.document().unwrap();
        let outer = groups_preorder(doc)[0];
        assert_framed_by(&mut s, outer, &chain, "the outermost group of the chain");
        let first = id_named(s.document().unwrap(), "S0");
        let members = [id_named(s.document().unwrap(), "sa0"), id_named(s.document().unwrap(), "sb0")];
        assert_framed_by(&mut s, first, &members, "the first sibling group");
    }
}

/// Runs the child's `scenario` under `ulimit -v 3000000` and gives the line it printed.
#[cfg(target_os = "linux")]
fn run_child(scenario: &str) -> String {
    use std::process::Command;
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = "ulimit -v 3000000 && exec \"$0\" --exact group_frame::child --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    assert!(out.status.success(), "the child `{scenario}` aborted, or its assertion failed: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    let line = stdout.lines().find(|l| l.starts_with("RESULT")).unwrap_or("no RESULT line").to_owned();
    eprintln!("{line}");
    line
}

/// **The worst case ends as a move or as a refusal in words, and does not abort** at 3 GB of address space: a member nested 180 deep (the file reader's
/// own depth is 190) and the first leaf of each of a thousand sibling groups moved in one command, with 1,500 connectors glued to their members, beside the
/// yardstick of one leaf of one group. The counted cost of the same page is in `draw/refit.rs` (the work, the `Connect` rows passed over, the commits and the
/// evaluator's steps); the line each child prints carries its time and memory for a person to read.
#[cfg(target_os = "linux")]
#[test]
fn a_member_nested_180_deep_and_a_thousand_sibling_groups_with_glued_connectors_end_as_a_move_or_a_refusal_in_words_under_ulimit_v() {
    let yardstick = run_child("yardstick");
    assert!(yardstick.contains("class=moved"), "{yardstick}");
    let worst = run_child("worst");
    assert!(worst.contains("class=moved") || worst.contains("class=refused("), "{worst}");
}

// ---- the cost of a member's move in a big group ------------------------------------------------------------------------------------------

/// An `.odg` of one group `G` of `n` small rectangles in a grid, and one rectangle far off.
fn big_group_drawing(n: usize) -> Vec<u8> {
    use crate::odg_build::{Drawing, rect};
    let mut body = String::from("<draw:g draw:name=\"G\">");
    for k in 0..n {
        let (x, y) = (2.0 + (k % 100) as f64 * 0.15, 3.0 + (k / 100) as f64 * 0.1);
        body.push_str(&rect(&format!("m{k}"), (x, y, 0.1, 0.05), ""));
    }
    body.push_str("</draw:g>");
    body.push_str(&rect("far", (20.0, 20.0, 1.0, 1.0), ""));
    Drawing::new().page(&body).bytes()
}

/// The time a member's move takes in a group of `n` members, beside the move of a shape that is in no group. For a person to read (`--ignored --nocapture`):
/// the gate does not assert time.
#[test]
#[ignore = "real time: run by hand with --ignored --nocapture; RV_N and RV_MOVES set the size"]
fn a_move_of_a_member_of_a_big_group_takes_about_what_the_members_cost() {
    let n: usize = std::env::var("RV_N").ok().and_then(|v| v.parse().ok()).unwrap_or(5_000);
    let moves: usize = std::env::var("RV_MOVES").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    let dir = Dir::new();
    let path = dir.write("a.odg", &big_group_drawing(n));
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    let doc = s.document().unwrap();
    let (first, second, far) = (id_named(doc, "m0"), id_named(doc, "m1"), id_named(doc, "far"));
    let t = std::time::Instant::now();
    run(&mut s, "shape.move", json!({ "ids": [far], "dx": 0.5, "dy": 0.0, "page": PAGE }));
    eprintln!("PROBE yardstick, a shape in no group: {:?}", t.elapsed());
    for k in 0..moves {
        let id = if k % 2 == 0 { first } else { second };
        let t = std::time::Instant::now();
        run(&mut s, "shape.move", json!({ "ids": [id], "dx": -0.5 - k as f64, "dy": 0.0, "page": PAGE }));
        eprintln!("PROBE member move {k} in a group of {n}: {:?}", t.elapsed());
    }
}

// ----: the box rule, the routing, what is said, and the history ------------------------------------------------------------

/// The review's input: three shapes, a connector between two of them, the three grouped. A one-dimensional member counts by its two ends and nothing
/// else, so the group is the box of the two rectangles whatever the route does, and the session's frame is the one a reopen of the saved file gives.
#[test]
fn a_group_holding_a_connector_is_the_box_of_its_ends_and_the_session_is_the_reopen() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = made(&run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 6.0, "y": 6.0 })));
    let b = made(&run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 9.0, "y": 6.0 })));
    let wire = made(&run(&mut s, "connector.add", json!({ "from": { "id": a, "point": 3 }, "to": { "id": b, "point": 1 }, "page": PAGE })));
    let group = made(&run(&mut s, "shape.group", json!({ "page": 0, "ids": [a, b, wire] })));
    assert_framed_by(&mut s, group, &[a, b], "as grouped");
    assert_file_agrees(&dir, &mut s, group, 0, "as grouped");
    // b goes down and to the left of a: the route of the connector now goes round, beyond the box of the ends.
    for (dx, dy) in [(-5.0, -3.0), (0.0, 6.0), (4.0, -1.0)] {
        run(&mut s, "shape.move", json!({ "ids": [b], "dx": dx, "dy": dy, "page": PAGE }));
        assert_framed_by(&mut s, group, &[a, b], &format!("after b is moved by ({dx}, {dy})"));
        assert_file_agrees(&dir, &mut s, group, 0, &format!("after b is moved by ({dx}, {dy})"));
    }
}

/// The rows of the `Geometry` sections of the shape `id` on the first page, as text.
fn route_of(doc: &Document, id: u32) -> String {
    let at = doc.find_shape(diacraft_doc::Owner::Page(PAGE), id).unwrap();
    doc.shape(&at).unwrap().sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").map(|s| format!("{s:?}")).collect()
}

/// A connection point of its own on the group (`X` is half its width, `Y` its foot), made as a file or a ShapeSheet edit makes one: a group has none from
/// `shape.group`.
fn give_a_connection_point(s: &mut Session, group: u32) {
    let width = frame_in(s, group).width;
    change_group(s, group, |shape| {
        let cell =
            |formula: &str, v: f64| diacraft_doc::Cell::with_formula(formula, Some(diacraft_doc::Value::from_text(&format!("{v}")))).with_unit("IN");
        let mut cells = diacraft_doc::Cells::new();
        cells.insert(diacraft_doc::CellName::new("X"), cell("Width*0.5", width / 2.0));
        cells.insert(diacraft_doc::CellName::new("Y"), cell("Height*0", 0.0));
        let mut section = diacraft_doc::Section::new("Connection", None);
        section.rows.push(std::sync::Arc::new(diacraft_doc::Row { ix: Some(0), cells, ..diacraft_doc::Row::default() }));
        std::sync::Arc::make_mut(&mut shape.sheet).sections.push(std::sync::Arc::new(section));
    });
}

/// The review's input for the connectors: a connector glued to a group is routed again when a member's move refits the group, and what it is routed to is what
/// Reset Connector gives.
#[test]
fn a_connector_glued_to_a_refitted_group_is_routed_again_as_a_reset_routes_it() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let (a, b, c, group) = grouped_pair(&mut s);
    give_a_connection_point(&mut s, group);
    let wire = made(&run(&mut s, "connector.add", json!({ "from": { "id": c, "point": 1 }, "to": { "id": group, "point": 0 }, "page": PAGE })));
    let route_before = route_of(s.document().unwrap(), wire);
    let answer = run(&mut s, "shape.move", json!({ "ids": [a], "dx": -4.0, "dy": -2.0, "page": PAGE }));
    assert!(answer.get("groups_left").is_none(), "nothing was left: {answer}");
    assert_framed_by(&mut s, group, &[a, b], "after a is moved");
    let routed = route_of(s.document().unwrap(), wire);
    assert_ne!(routed, route_before, "the route went with the end");
    // What the refit made of the route is what Reset Connector makes of it.
    run(&mut s, "connector.reset", json!({ "ids": [wire], "page": PAGE }));
    assert_eq!(route_of(s.document().unwrap(), wire), routed, "the route of the refit is the route of a reset");
}

/// An `.odg` of one group `G` of `n` rectangles (`m0` to `m{n-1}`) and one far rectangle, opened in a session.
fn open_big_group(n: usize) -> (Dir, Session) {
    let dir = Dir::new();
    let path = dir.write("a.odg", &big_group_drawing(n));
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    (dir, s)
}

#[test]
fn a_group_left_as_it_was_is_in_the_answer_and_a_group_that_fits_is_not() {
    // 10,001 members are more than an edit may read (10,000): the group the file holds is left, the member's move is made, and the answer names the group.
    let (_dir, mut s) = open_big_group(10_001);
    let doc = s.document().unwrap();
    let (m0, group) = (id_named(doc, "m0"), id_named(doc, "G"));
    let corners = shape_corners(doc, PAGE, m0).unwrap();
    let answer = run(&mut s, "shape.move", json!({ "ids": [m0], "dx": -1.0, "dy": 0.0, "page": PAGE }));
    let left = answer["groups_left"].as_array().unwrap_or_else(|| panic!("no groups_left in {answer}"));
    assert_eq!(left.len(), 1, "{answer}");
    assert_eq!((left[0]["id"].as_u64(), left[0]["kind"].as_str(), left[0]["page"].as_u64()), (Some(u64::from(group)), Some("group"), Some(0)));
    assert!(left[0]["why"].as_str().unwrap().contains("it has 10001 members and an edit may read 10000"), "{answer}");
    assert!(answer.get("groups_left_unlisted").is_none());
    // The edit was made.
    let moved = shape_corners(s.document().unwrap(), PAGE, m0).unwrap();
    assert!(
        corners.iter().zip(&moved).all(|(was, now)| (was.0 - 1.0 - now.0).abs() < 1.0e-6 && (was.1 - now.1).abs() < 1.0e-6),
        "{corners:?} {moved:?}"
    );
    // A group of the size the rule takes is refitted, and the answer says nothing.
    let (_dir, mut s) = open_big_group(500);
    let m0 = id_named(s.document().unwrap(), "m0");
    let answer = run(&mut s, "shape.move", json!({ "ids": [m0], "dx": -1.0, "dy": 0.0, "page": PAGE }));
    assert!(answer.get("groups_left").is_none(), "{answer}");
}

/// An edit that leaves a group the file does not hold as it was **is made**, and the group is listed in the answer as any group left is. It was
/// refused, because the `.odg` writer refused such a group at every save; the writer writes a group whose frame is not the box of its members as its members and says so in
/// a note, so the refusal's reason is gone and it only stopped the edit.
#[test]
fn an_edit_that_leaves_a_group_the_file_does_not_hold_is_made_and_the_group_is_listed() {
    // The group the file holds takes the whole of the edit's allowance (10,000 members); the group the session made is then left, and the file does not hold it.
    let (_dir, mut s) = open_big_group(10_000);
    let a = made(&run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 6.0, "y": 6.0 })));
    let b = made(&run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 9.0, "y": 6.0 })));
    let new = made(&run(&mut s, "shape.group", json!({ "page": 0, "ids": [a, b] })));
    let m0 = id_named(s.document().unwrap(), "m0");
    let corners = shape_corners(s.document().unwrap(), PAGE, a).unwrap();
    let before = crate::snapshot(&s);
    let answer = run(&mut s, "shape.move", json!({ "ids": [m0, a], "dx": -1.0, "dy": 0.0, "page": PAGE }));
    let left = answer["groups_left"].as_array().unwrap_or_else(|| panic!("no groups_left in {answer}"));
    assert_eq!(left.len(), 1, "{answer}");
    assert_eq!((left[0]["id"].as_u64(), left[0]["kind"].as_str()), (Some(u64::from(new)), Some("group")), "{answer}");
    assert!(left[0]["why"].as_str().unwrap().contains("an edit may read"), "{answer}");
    let after = crate::snapshot(&s);
    assert!(after.document != before.document && after.undo_steps == before.undo_steps + 1, "the edit was made, in one step");
    let moved = shape_corners(s.document().unwrap(), PAGE, a).unwrap();
    assert!(
        corners.iter().zip(&moved).all(|(was, now)| (was.0 - 1.0 - now.0).abs() < 1.0e-6 && (was.1 - now.1).abs() < 1.0e-6),
        "the member moved: {corners:?} {moved:?}"
    );
    // Undone, the same edit of the group's member alone is made: the group is refitted.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "shape.move", json!({ "ids": [a], "dx": -1.0, "dy": 0.0, "page": PAGE }));
    assert_framed_by(&mut s, new, &[a, b], "after a is moved alone");
}

#[test]
fn the_history_is_cut_by_its_bytes_and_an_undo_past_what_is_left_is_refused_in_words() {
    let (_dir, mut s) = open_big_group(300);
    let doc = s.document().unwrap();
    let (m0, m1) = (id_named(doc, "m0"), id_named(doc, "m1"));
    // A history that holds 60,000 bytes in each direction: a move that rewrites the 300 members of the group is more than that.
    s.set_history_limits(1_000, 60_000);
    for k in 0..6 {
        run(&mut s, "shape.move", json!({ "ids": [if k % 2 == 0 { m0 } else { m1 }], "dx": -1.0 - f64::from(k), "dy": 0.0, "page": PAGE }));
    }
    let history = s.history().unwrap();
    assert_eq!(history.undo_len(), 1, "each step holds more than the budget, so the newest alone is kept");
    assert!(history.undo_was_cut() && history.undo_bytes() > 60_000, "{} bytes", history.undo_bytes());
    run(&mut s, "edit.undo", json!({}));
    // The step that takes the place of the one taken back is charged what the version left holds beside the one gone back to: the redo step is not free.
    assert!(s.history().unwrap().redo_bytes() > 0, "the undone step was charged to the redo steps");
    let refused = match s.execute("edit.undo", &json!({})) {
        Err(diacraft_engine::EngineError::Disabled { reason, .. }) => reason,
        other => panic!("the second undo was not refused as disabled: {other:?}"),
    };
    assert!(
        refused.contains("the older steps were let go to keep the history within 60000 bytes")
            && refused.contains("nothing before them can be undone"),
        "{refused}"
    );
    // The redo of the one step that was taken back is still there.
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.history().unwrap().undo_len(), 1);
    assert!(s.history().unwrap().undo_bytes() > 0, "and the redone step is charged to the undo steps again");
    // A history with no limit but the count never says it was cut.
    s.set_history_limits(1_000, u64::MAX);
    run(&mut s, "shape.move", json!({ "ids": [m0], "dx": -20.0, "dy": 0.0, "page": PAGE }));
    assert!(s.history().unwrap().undo_len() >= 2 && !s.history().unwrap().redo_was_cut());
}

/// `n` groups of two rectangles each (`a{k}` and `b{k}`) in an `.odg`.
fn groups_drawing(n: usize) -> Vec<u8> {
    use crate::odg_build::{Drawing, rect};
    let mut body = String::new();
    for k in 0..n {
        let (x, y) = (1.0 + (k % 20) as f64 * 0.5, 1.0 + (k / 20) as f64 * 0.5);
        body.push_str("<draw:g>");
        body.push_str(&rect(&format!("a{k}"), (x, y, 0.1, 0.1), ""));
        body.push_str(&rect(&format!("b{k}"), (x + 0.2, y, 0.1, 0.1), ""));
        body.push_str("</draw:g>");
    }
    Drawing::new().page(&body).bytes()
}

#[test]
fn the_groups_left_by_the_edits_of_a_batch_are_said_up_to_the_list_and_the_rest_counted_across_the_edits() {
    // 150 groups, each with a section of 20,001 rows of its own (one allocation for all of them: more than an edit may carry), so that every one that an edit moves
    // a member of is left as it was. The first edit of the batch leaves 90 of them and the second 60: the answer lists 100 and counts 50, whatever edit each came from.
    let dir = Dir::new();
    let path = dir.write("a.odg", &groups_drawing(150));
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    let mut section = diacraft_doc::Section::new("Geometry", Some(0));
    for k in 0..20_001_u32 {
        let mut cells = diacraft_doc::Cells::new();
        cells.insert(diacraft_doc::CellName::new("X"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1")));
        section.rows.push(std::sync::Arc::new(diacraft_doc::Row {
            ix: Some(k),
            row_type: Some(std::sync::Arc::from("LineTo")),
            cells,
            ..diacraft_doc::Row::default()
        }));
    }
    let section = std::sync::Arc::new(section);
    for group in groups_preorder(s.document().unwrap()) {
        change_group(&mut s, group, |shape| std::sync::Arc::make_mut(&mut shape.sheet).sections.push(std::sync::Arc::clone(&section)));
    }
    let leaves: Vec<u32> = (0..150).map(|k| id_named(s.document().unwrap(), &format!("a{k}"))).collect();
    let step = |ids: &[u32]| json!({ "command": "shape.move", "params": { "ids": ids, "dx": -0.05, "dy": 0.0, "page": PAGE } });
    let answer = run(&mut s, "command.batch", json!({ "commands": [step(&leaves[..90]), step(&leaves[90..])] }));
    assert_eq!(answer["groups_left"].as_array().map(Vec::len), Some(100), "{answer}");
    assert_eq!(answer["groups_left_unlisted"].as_u64(), Some(50), "{answer}");
}

// ---- the children of the findings, under `ulimit -v 3000000` ------------------------------------------------------------------------------

/// Scenarios the children of the fix round run, named in the environment.
#[cfg(target_os = "linux")]
const F1_SCENARIO: &str = "GROUP_FRAME_F1_SCENARIO";

/// `chains` chains of three groups one in the other, round two rectangles each (`a{k}` and `b{k}`), in an `.odg`.
#[cfg(target_os = "linux")]
fn chains_drawing(chains: usize) -> Vec<u8> {
    use crate::odg_build::{Drawing, rect};
    let mut body = String::new();
    for k in 0..chains {
        let (x, y) = (1.0 + (k % 64) as f64 * 0.4, 1.0 + (k / 64) as f64 * 0.4);
        body.push_str("<draw:g><draw:g><draw:g>");
        body.push_str(&rect(&format!("a{k}"), (x, y, 0.1, 0.1), ""));
        body.push_str(&rect(&format!("b{k}"), (x + 0.2, y, 0.1, 0.1), ""));
        body.push_str("</draw:g></draw:g></draw:g>");
    }
    Drawing::new().page(&body).bytes()
}

/// The top-level shape `group` of the session's first page, to change: `f` is given the shape.
fn change_group(s: &mut Session, group: u32, f: impl FnOnce(&mut diacraft_doc::Shape)) {
    s.edit("change the group", |doc| {
        let page = doc.pages.get_mut(0).map(std::sync::Arc::make_mut).expect("a first page");
        let at = page.shapes.iter().position(|x| x.id == group).expect("the group is on the page");
        f(page.shapes.get_mut(at).map(std::sync::Arc::make_mut).expect("the shape"));
        Ok(())
    })
    .unwrap();
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child_1() {
    let scenario = std::env::var(F1_SCENARIO).expect("run by a parent test, with the scenario in the environment");
    let moves: usize = std::env::var("F1_MOVES").ok().and_then(|n| n.parse().ok()).unwrap_or(40);
    println!("\nSTART VmSize={}KiB", status_kib("VmSize"));
    let started = std::time::Instant::now();
    let line = match scenario.as_str() {
        // The review's blocker: a group of 5,000 rectangles, one member moved past the group's left edge again and again, each move rewriting every member. The
        // history holds what its budget says and no more, and the undo past it is refused in words.
        "history" | "unbounded" => {
            let (_dir, mut s) = open_big_group(5_000);
            let doc = s.document().unwrap();
            let (m0, m1) = (id_named(doc, "m0"), id_named(doc, "m1"));
            if scenario == "unbounded" {
                // The old behaviour, for a person to see: a count of 1,000 steps and no byte budget. It aborts at about 137 moves.
                s.set_history_limits(1_000, u64::MAX);
            }
            for k in 0..moves {
                let id = if k % 2 == 0 { m0 } else { m1 };
                run(&mut s, "shape.move", json!({ "ids": [id], "dx": -0.5 - k as f64, "dy": 0.0, "page": PAGE }));
            }
            let history = s.history().unwrap();
            let (held, steps, cut) = (history.undo_bytes(), history.undo_len(), history.undo_was_cut());
            if scenario == "history" {
                assert!(held <= 2 * History::DEFAULT_BYTES, "the history holds {held} bytes past its budget of {}", History::DEFAULT_BYTES);
                assert!(steps < moves && cut, "{steps} steps of {moves} moves, cut: {cut}");
                // Back through the steps that are left, and then the words.
                let mut undone = 0;
                loop {
                    match s.execute("edit.undo", &json!({})) {
                        Ok(_) => undone += 1,
                        Err(e) => {
                            assert!(e.to_string().contains("the older steps were let go"), "{e}");
                            break;
                        }
                    }
                }
                assert_eq!(undone, steps);
            }
            format!("moves={moves} steps_kept={steps} bytes_held={held}")
        }
        // The scan's first finding: a group with 100,000 rows of its own geometry and a member moved out. The group is left before a row is read, and said.
        "own_rows" => {
            let (_dir, mut s) = open_big_group(1_000);
            let (m0, group) = (id_named(s.document().unwrap(), "m0"), id_named(s.document().unwrap(), "G"));
            change_group(&mut s, group, |shape| {
                let mut section = diacraft_doc::Section::new("Geometry", Some(0));
                for k in 0..100_000_u32 {
                    let mut cells = diacraft_doc::Cells::new();
                    for name in ["X", "Y"] {
                        cells.insert(diacraft_doc::CellName::new(name), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1")));
                    }
                    section.rows.push(std::sync::Arc::new(diacraft_doc::Row {
                        ix: Some(k),
                        row_type: Some(std::sync::Arc::from("LineTo")),
                        cells,
                        ..diacraft_doc::Row::default()
                    }));
                }
                std::sync::Arc::make_mut(&mut shape.sheet).sections.push(std::sync::Arc::new(section));
            });
            let answer = run(&mut s, "shape.move", json!({ "ids": [m0], "dx": -1.0, "dy": 0.0, "page": PAGE }));
            let why = answer["groups_left"][0]["why"].as_str().unwrap_or_else(|| panic!("no group left in {answer}")).to_owned();
            // The group a `.odg` is read into has four rows of its own, besides the 100,000.
            assert!(why.contains("it has 100004 rows of its own"), "{why}");
            format!("left=1 why={why}")
        }
        // The scan's second finding: a page of thousands of groups, three deep, a member of each moved in one command: some are refitted, the rest are said.
        "many_groups" => {
            let dir = Dir::new();
            let path = dir.write("a.odg", &chains_drawing(3_000));
            let mut s = Session::new();
            run(&mut s, "file.open", json!({ "path": path }));
            let ids: Vec<u32> = (0..3_000).map(|k| id_named(s.document().unwrap(), &format!("a{k}"))).collect();
            let t = std::time::Instant::now();
            let answer = run(&mut s, "shape.move", json!({ "ids": ids, "dx": -0.05, "dy": 0.0, "page": PAGE }));
            let took = t.elapsed().as_millis();
            let listed = answer["groups_left"].as_array().map_or(0, Vec::len);
            let unlisted = answer["groups_left_unlisted"].as_u64().unwrap_or(0) as usize;
            // Each group of a file has rows of its own: the 20,000 rows of the edit's allowance are those of 1,666 chains of three groups and two of the next, and the
            // 834 chains from there to the 2,500 that the 10,000 members of the allowance read (2, 1 and 1 for a chain) are left at their inner group (the groups round it
            // see it as it was, and are not asked for), and the 500 chains after them are left at each of their three groups: 834 + 1,500.
            assert_eq!(listed + unlisted, 834 + 500 * 3, "{answer}");
            format!("left={} move_ms={took}", listed + unlisted)
        }
        // The scan's third finding: 5,000 connectors glued to one group whose frame a member's move changes: the most are routed again, the others are said.
        "connectors" => {
            let (_dir, mut s) = open_big_group(2);
            let (m0, group, far) =
                (id_named(s.document().unwrap(), "m0"), id_named(s.document().unwrap(), "G"), id_named(s.document().unwrap(), "far"));
            give_a_connection_point(&mut s, group);
            let wire =
                made(&run(&mut s, "connector.add", json!({ "from": { "id": far, "point": 1 }, "to": { "id": group, "point": 0 }, "page": PAGE })));
            s.edit("many connectors", |doc| {
                let page = doc.pages.get_mut(0).map(std::sync::Arc::make_mut).expect("a first page");
                let original = page.shapes.iter().find(|x| x.id == wire).cloned().expect("the connector");
                let rows: Vec<diacraft_doc::Connect> = page.connects.iter().filter(|c| c.from_sheet == wire).cloned().collect();
                let mut connects: Vec<diacraft_doc::Connect> = page.connects.iter().cloned().collect();
                let first = page.shapes.iter().map(|x| x.id).max().unwrap_or(0) + 1;
                for k in 0..4_999 {
                    page.shapes.push(std::sync::Arc::new(diacraft_doc::Shape { id: first + k, ..(*original).clone() }));
                    connects.extend(rows.iter().map(|c| diacraft_doc::Connect { from_sheet: first + k, ..c.clone() }));
                }
                page.connects = diacraft_doc::Connects::from_vec(connects);
                Ok(())
            })
            .unwrap();
            let t = std::time::Instant::now();
            let answer = run(&mut s, "shape.move", json!({ "ids": [m0], "dx": -1.0, "dy": 0.0, "page": PAGE }));
            let took = t.elapsed().as_millis();
            let listed = answer["groups_left"].as_array().map_or(0, Vec::len);
            let unlisted = answer["groups_left_unlisted"].as_u64().unwrap_or(0) as usize;
            assert!(answer["groups_left"].as_array().is_some_and(|l| l.iter().all(|e| e["kind"] == "connector")), "{answer}");
            // At the most 500 are routed again (a try each) and every other one is said; or the budget did not cover the routing and all are said.
            let said = listed + unlisted;
            assert!((5_000 - 500..=5_000).contains(&said), "{said} connectors said: {answer}");
            format!("not_routed={said} move_ms={took}")
        }
        other => panic!("no scenario {other}"),
    };
    println!("\nRESULT {scenario} {line} ms={} VmHWM={}KiB VmPeak={}KiB", started.elapsed().as_millis(), status_kib("VmHWM"), status_kib("VmPeak"));
}

/// Runs the child of `scenario` under `ulimit -v 3000000` and gives the line it printed; or the child's end when it did not end as a success.
#[cfg(target_os = "linux")]
fn run_child_1(scenario: &str, moves: usize) -> Result<String, String> {
    use std::process::Command;
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = "ulimit -v 3000000 && exec \"$0\" --exact group_frame::child_1 --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg(exe)
        .env(F1_SCENARIO, scenario)
        .env("F1_MOVES", moves.to_string())
        .output()
        .expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        return Err(format!("the child `{scenario}` ended as {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status));
    }
    let line = stdout.lines().find(|l| l.starts_with("RESULT")).unwrap_or("no RESULT line").to_owned();
    eprintln!("{line}");
    Ok(line)
}

/// **The blocker of the review.** A group of 5,000 rectangles, one member moved past the group's left edge 40 times (each move rewrites the 5,000 members and
/// leaves a step of about 20 MB: the history of 1,000 steps aborted after about 137 moves under 3 GB). The history is held to its bytes, and the undo past the
/// steps left is refused in words.
#[cfg(target_os = "linux")]
#[test]
fn forty_moves_of_a_member_of_a_group_of_5000_end_within_the_budget_of_the_history_under_ulimit_v() {
    let line = run_child_1("history", 40).unwrap_or_else(|e| panic!("{e}"));
    assert!(line.contains("moves=40") && line.contains("steps_kept="), "{line}");
}

/// The same moves with no byte budget, for a person to see the abort the review found (about 137 moves under 3 GB): not run by the gate.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "aborts by design (the old behaviour, with the byte budget taken off): run by hand with --ignored"]
fn without_the_byte_budget_the_same_moves_abort_under_ulimit_v() {
    let ended = run_child_1("unbounded", 200);
    eprintln!("{ended:?}");
    assert!(ended.is_err(), "the child did not abort");
}

#[cfg(target_os = "linux")]
#[test]
fn a_group_with_a_hundred_thousand_rows_of_its_own_is_left_and_said_under_ulimit_v() {
    let line = run_child_1("own_rows", 0).unwrap_or_else(|e| panic!("{e}"));
    assert!(line.contains("left=1"), "{line}");
}

#[cfg(target_os = "linux")]
#[test]
fn three_thousand_chains_of_three_groups_moved_at_once_are_refitted_by_the_allowance_and_the_rest_said_under_ulimit_v() {
    let line = run_child_1("many_groups", 0).unwrap_or_else(|e| panic!("{e}"));
    assert!(line.contains("left=2334"), "{line}");
}

#[cfg(target_os = "linux")]
#[test]
fn five_thousand_connectors_glued_to_a_refitted_group_are_routed_by_the_most_and_the_rest_said_under_ulimit_v() {
    let line = run_child_1("connectors", 0).unwrap_or_else(|e| panic!("{e}"));
    assert!(line.contains("not_routed="), "{line}");
}
