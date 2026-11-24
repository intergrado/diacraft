//! A new group saved to an `.odg` through the commands. **A member turned by any angle** used to refuse the save, even straight after `shape.group`: the
//! writer held the group's frame to the box of its members at 1E-12 inch, while a drawing handed to it has every frame cell at nine places (the copy of a save, `flatten.rs`), so
//! a turned member's box (its corners worked from a pin, a size and an angle each rounded) never matched the group's exactly. And **a group whose frame is not the box of its
//! members** (a `LockCalcWH` group after a member moved out, a frame wider than its members, a pin set elsewhere) was refused: it is written as its members now, with a note
//! that its own frame is not kept, since a `draw:g` has no frame of its own, and the file read again gives the box of the members.
//!
//! The groups are made by commands (`shape.drop`, `shape.resize`, `shape.rotate`, `shape.flip`, `shape.group`); the one cell no command writes (`LockCalcWH`, a frame wider
//! than its members) is written by an edit of the session. None of it depends on the refit of a group's frame after an edit of its members: a group whose own cells were
//! edited, or that holds `LockCalcWH`, is left as it is by that rule.

use diacraft_doc::{CellAddr, CellEdit, Document, Owner, SheetRef, Value, WriteOrigin};
use diacraft_engine::{EngineError, Session};
use diacraft_render::{Renderer, View};
use serde_json::{Value as Json, json};

use crate::odg_build::{Dir, Drawing, open_bytes_in, open_drawing, rect, same_frame};
use crate::odg_file::{assert_saves_again_as_no_edit, four, frame_at, refused, save};
use crate::run;

/// The first page drawn at 50 pixels to the inch, on white paper: its size and its premultiplied pixels.
fn drawn(doc: &Document) -> (u32, u32, Vec<u8>) {
    let page = doc.pages.get(0).unwrap().id;
    let view = View::fit_page(doc, page, 50.0).unwrap();
    let mut renderer = Renderer::new();
    renderer.set_paper(Some(diacraft_color::Rgba::WHITE));
    let frame = renderer.render(doc, page, &view);
    assert!(frame.skipped.is_empty(), "everything was drawn: {:?}", frame.skipped);
    (frame.width, frame.height, frame.rgba_premul)
}

/// Asserts that the drawing the file holds draws as the session's does, to the hundredths of a millimetre a path is written in.
#[track_caller]
fn assert_draws_alike(made: &Document, read: &Document, what: &str) {
    let (a, b) = (drawn(made), drawn(read));
    assert_eq!((a.0, a.1), (b.0, b.1), "{what}: the picture's size");
    let worst = a.2.iter().zip(b.2.iter()).map(|(x, y)| (i32::from(*x) - i32::from(*y)).abs()).max().unwrap_or(0);
    assert!(worst <= 8, "{what}: the file draws it differently from the session (the worst channel differs by {worst})");
}

/// The file `a.odg` read into a session of its own.
fn reopened(dir: &Dir) -> Session {
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
    again
}

/// A rectangle dropped with its centre at (`x`, `y`), `w` by `h` inches, flipped as `flips` says (`h` horizontal, `v` vertical) and then turned to `angle` degrees: its `ID`.
fn rectangle(s: &mut Session, (x, y): (f64, f64), (w, h): (f64, f64), flips: &str, angle: f64) -> u32 {
    let dropped = run(s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": x, "y": y }));
    let id = u32::try_from(dropped["id"].as_u64().unwrap()).unwrap();
    run(s, "shape.resize", json!({ "id": id, "x": x - w / 2.0, "y": y - h / 2.0, "width": w, "height": h }));
    for flip in flips.chars() {
        let axis = if flip == 'h' { "horizontal" } else { "vertical" };
        run(s, "shape.flip", json!({ "ids": [id], "axis": axis }));
    }
    if angle != 0.0 {
        run(s, "shape.rotate", json!({ "ids": [id], "angle": angle }));
    }
    id
}

/// `shape.group` of the shapes: the group's `ID`.
fn group(s: &mut Session, ids: &[u32]) -> u32 {
    let made = run(s, "shape.group", json!({ "page": 0, "ids": ids }));
    u32::try_from(made["id"].as_u64().unwrap()).unwrap()
}

/// The group at `at` on the first page and its members, as `(id, page corners)` in the order the group holds them.
fn members_on_the_page(doc: &Document, at: usize) -> Vec<(u32, [(f64, f64); 4])> {
    let page = doc.pages.get(0).unwrap();
    let group = page.shapes.get(at).unwrap();
    group.children.iter().map(|m| (m.id, diacraft_engine::draw::shape_corners(doc, page.id, m.id).unwrap())).collect()
}

/// The smallest upright box that holds the corners, as a frame `[PinX, PinY, Width, Height, Angle]` of the page.
fn box_frame(members: &[(u32, [(f64, f64); 4])]) -> [f64; 5] {
    let points: Vec<(f64, f64)> = members.iter().flat_map(|(_, corners)| corners.iter().copied()).collect();
    let (x0, x1) = points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
    let (y0, y1) = points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
    [(x0 + x1) / 2.0, (y0 + y1) / 2.0, x1 - x0, y1 - y0, 0.0]
}

/// What the session made of a new group is what the file gives back: the save says nothing, the file read again holds the group at the same place in the same tree with
/// the session's frame and the members where the session has them on the page, draws as the session does, and a second save of the session is no edit.
#[track_caller]
fn assert_reads_back_as_made(dir: &Dir, s: &mut Session, at: usize, what: &str) {
    let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{what}: the save was refused: {e}"));
    assert_eq!(saved["edited"], true, "{what}: {saved}");
    assert_eq!(saved["warning_count"], 0, "{what}: the save says nothing: {saved}");
    assert_eq!(saved["notes"].as_array().map(Vec::len), Some(0), "{what}: {saved}");
    let again = reopened(dir);
    let (made, read) = (s.document().unwrap(), again.document().unwrap());
    same_frame(frame_at(made, 0, at), frame_at(read, 0, at), &format!("{what}: the group's frame"));
    let (was, now) = (members_on_the_page(made, at), members_on_the_page(read, at));
    assert_eq!(was.len(), now.len(), "{what}: the members");
    for (k, ((_, a), (_, b))) in was.iter().zip(&now).enumerate() {
        for (c, (p, q)) in a.iter().zip(b).enumerate() {
            assert!((p.0 - q.0).abs() < 1e-6 && (p.1 - q.1).abs() < 1e-6, "{what}: member {k}, corner {c}: the session has {p:?} and the file {q:?}");
        }
    }
    same_frame(frame_at(read, 0, at), box_frame(&now), &format!("{what}: the file's group is the box of its members"));
    assert_draws_alike(made, read, what);
    assert_saves_again_as_no_edit(dir, s, "a.odg");
}

/// The angles of the review's finding and its neighbours: nothing, a third of a quarter turn, an eighth, a quarter, an angle with no exact radian, and one a thousandth of a degree
/// short of a whole turn.
const ANGLES: [f64; 6] = [0.0, 30.0, 45.0, 90.0, 133.0, 359.999];

/// The finding (concern 4): a new group holding a member turned by any angle but 0 was refused at save, straight after `shape.group`. Each angle, with the turned
/// member beside one that is not, and with both turned (to different angles, of different sizes): the group saves and reads back as the session has it.
#[test]
fn a_new_group_with_a_turned_member_saves_at_every_angle_and_reads_back_as_the_session_has_it() {
    for angle in ANGLES {
        for (what, other_angle) in [("one turned, one upright", 0.0), ("both turned", angle / 2.0 + 7.0)] {
            let dir = Dir::new();
            let mut s = open_drawing(&dir, &four());
            let a = rectangle(&mut s, (6.0, 6.0), (2.31, 0.97), "", angle);
            let b = rectangle(&mut s, (8.4, 7.1), (1.7, 1.3), "", other_angle);
            group(&mut s, &[a, b]);
            assert_reads_back_as_made(&dir, &mut s, 4, &format!("{what} at {angle} degrees"));
        }
    }
}

/// A member flipped, in each of the three ways, and flipped and then turned: the group saves and reads back as the session has it.
#[test]
fn a_new_group_with_a_flipped_member_saves_and_reads_back_as_the_session_has_it() {
    for flips in ["h", "v", "hv"] {
        for angle in [0.0, 30.0, 133.0] {
            let dir = Dir::new();
            let mut s = open_drawing(&dir, &four());
            let a = rectangle(&mut s, (6.0, 6.0), (2.31, 0.97), flips, angle);
            let b = rectangle(&mut s, (8.4, 7.1), (1.7, 1.3), "", 0.0);
            group(&mut s, &[a, b]);
            assert_reads_back_as_made(&dir, &mut s, 4, &format!("flipped {flips} and turned {angle}"));
        }
    }
}

/// A group in a group: the inner group holds turned members and the outer holds it and one more turned member; both save, and both read back with the session's frame.
#[test]
fn a_new_group_in_a_new_group_of_turned_members_saves_and_reads_back_as_the_session_has_it() {
    for angle in ANGLES {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &four());
        let a = rectangle(&mut s, (6.0, 6.0), (2.31, 0.97), "", angle);
        let b = rectangle(&mut s, (8.4, 7.1), (1.7, 1.3), "h", angle / 3.0);
        let inner = group(&mut s, &[a, b]);
        let c = rectangle(&mut s, (7.0, 9.3), (1.9, 0.6), "", 90.0 - angle / 2.0);
        group(&mut s, &[inner, c]);
        assert_reads_back_as_made(&dir, &mut s, 4, &format!("nested, the members turned {angle} degrees"));
        // The inner group is the outer's first member: its frame is the session's too.
        let again = reopened(&dir);
        let (made, read) = (s.document().unwrap(), again.document().unwrap());
        let frame_in = |doc: &Document| {
            let page = doc.pages.get(0).unwrap();
            let inside = page.shapes.get(4).unwrap().children.iter().find(|m| !m.children.is_empty()).unwrap().id;
            let f = diacraft_engine::draw::shape_frame(doc, page.id, inside).unwrap();
            [f.pin_x, f.pin_y, f.width, f.height, f.angle]
        };
        same_frame(frame_in(made), frame_in(read), &format!("the inner group, the members turned {angle} degrees"));
    }
}

/// Two groups, each of turned members, one after the other and one beside the other: the second save writes the second group and nothing else changes.
#[test]
fn two_groups_of_turned_members_are_saved_together_and_a_second_is_saved_after_the_first() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = rectangle(&mut s, (6.0, 6.0), (2.31, 0.97), "", 30.0);
    let b = rectangle(&mut s, (8.4, 7.1), (1.7, 1.3), "", 133.0);
    group(&mut s, &[a, b]);
    let c = rectangle(&mut s, (3.0, 11.0), (1.1, 0.7), "", 45.0);
    let d = rectangle(&mut s, (4.5, 12.3), (0.9, 1.2), "v", 359.999);
    group(&mut s, &[c, d]);
    assert_reads_back_as_made(&dir, &mut s, 4, "the first group");
    let again = reopened(&dir);
    same_frame(frame_at(s.document().unwrap(), 0, 5), frame_at(again.document().unwrap(), 0, 5), "the second group's frame");
}

/// A group that is itself turned is still refused in words (its angle is kept in DiaCraft's document file with its members written turned, which is not written yet),
/// with nothing written and the undo as the way out, however its members are turned; and a turn of the members that is undone and made again saves.
#[test]
fn a_new_group_that_is_itself_turned_is_refused_in_words_and_the_members_turned_inside_it_do_not_change_that() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let before = dir.read("a.odg");
    let a = rectangle(&mut s, (6.0, 6.0), (2.31, 0.97), "", 30.0);
    let b = rectangle(&mut s, (8.4, 7.1), (1.7, 1.3), "", 133.0);
    let g = group(&mut s, &[a, b]);
    run(&mut s, "shape.rotate", json!({ "ids": [g], "angle": 45 }));
    let words = refused(&mut s);
    assert!(words.contains("a group that is turned or flipped") && words.contains("Undo the change (edit.undo)"), "{words}");
    assert_eq!(dir.read("a.odg"), before, "nothing was written");
    run(&mut s, "edit.undo", json!({}));
    assert_reads_back_as_made(&dir, &mut s, 4, "the group, no longer turned");
}

// ------------------------------------------------------------------------------------------------------------------ a frame that is not the box of the members

/// The cell `name` of the shape `id` on the first page, set to the number `value` as the drawing's own write (the edit of a session).
fn set_cell(s: &mut Session, id: u32, name: &str, value: f64) {
    s.edit("set a cell", |d| {
        let at = d.find_shape(Owner::Page(0), id).unwrap();
        d.write_cell(&SheetRef::Shape(at), &CellAddr::direct(name), CellEdit::Value(Value::from_text(&value.to_string())), WriteOrigin::Drawing)
            .map(|_| ())
            .map_err(|e| EngineError::Internal { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
}

/// The warnings of a save, as text.
fn warnings(saved: &Json) -> Vec<String> {
    saved["warnings"].as_array().unwrap_or_else(|| panic!("a save has `warnings`: {saved}")).iter().map(|w| w.as_str().unwrap().to_owned()).collect()
}

/// The notes of a save about the frame of a group: the ones that say a group's own frame is not kept.
fn frame_notes(saved: &Json) -> Vec<String> {
    warnings(saved).into_iter().filter(|w| w.contains("frame") && w.contains("not kept")).collect()
}

/// A group whose frame is not the box of its members (here `LockCalcWH` on it, and a member moved out of the frame) is saved as its members and the save says
/// in one note, naming the group and its page, that its frame is not kept; the file read again holds the group as the box of its members, and the members are where they
/// were on the page.
#[test]
fn a_lockcalcwh_group_with_a_member_moved_out_saves_as_its_members_with_one_note_and_reads_back_as_their_box() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = rectangle(&mut s, (6.0, 6.0), (2.31, 0.97), "", 30.0);
    let b = rectangle(&mut s, (8.4, 7.1), (1.7, 1.3), "", 0.0);
    let g = group(&mut s, &[a, b]);
    set_cell(&mut s, g, "LockCalcWH", 1.0);
    run(&mut s, "shape.move", json!({ "ids": [b], "dx": 3.0, "dy": -1.5 }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let said = frame_notes(&saved);
    assert_eq!(said.len(), 1, "one note for the one group: {saved}");
    assert!(said[0].contains(&format!("group {g} on page 0")), "it names the group and its page: {}", said[0]);
    let again = reopened(&dir);
    let (made, read) = (s.document().unwrap(), again.document().unwrap());
    let members = members_on_the_page(made, 4);
    same_frame(frame_at(read, 0, 4), box_frame(&members), "the reopened group is the box of its members");
    let moved_out = frame_at(made, 0, 4);
    assert!(
        (moved_out[2] - box_frame(&members)[2]).abs() > 1.0,
        "the session's group is not that box (it was held to its size): {moved_out:?} against {:?}",
        box_frame(&members)
    );
    for (k, ((_, a), (_, b))) in members.iter().zip(&members_on_the_page(read, 4)).enumerate() {
        for (p, q) in a.iter().zip(b) {
            assert!((p.0 - q.0).abs() < 1e-6 && (p.1 - q.1).abs() < 1e-6, "member {k} is where it was on the page: {p:?} against {q:?}");
        }
    }
    assert_draws_alike(made, read, "the members are drawn where they were");
    // The save said it once: the file's group is the box, the same save again is no edit and says nothing.
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A frame wider than its members (a margin someone gave the group: the cells of its width and height and its pin written, so that the members stay where they are) is
/// written as its members too, with one note; and the file read again holds the box of the members.
#[test]
fn a_group_with_a_margin_saves_as_its_members_with_one_note_and_reads_back_as_their_box() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = rectangle(&mut s, (6.0, 6.0), (2.31, 0.97), "", 30.0);
    let b = rectangle(&mut s, (8.4, 7.1), (1.7, 1.3), "", 0.0);
    let g = group(&mut s, &[a, b]);
    let frame = frame_at(s.document().unwrap(), 0, 4);
    // A margin of half an inch on the right and the top: the frame's lower-left corner stays where it was, so the members stay.
    set_cell(&mut s, g, "Width", frame[2] + 0.5);
    set_cell(&mut s, g, "Height", frame[3] + 0.5);
    set_cell(&mut s, g, "PinX", frame[0] + 0.25);
    set_cell(&mut s, g, "PinY", frame[1] + 0.25);
    let saved = save(&mut s);
    let said = frame_notes(&saved);
    assert_eq!(said.len(), 1, "one note for the one group: {saved}");
    assert!(said[0].contains(&format!("group {g} on page 0")), "{}", said[0]);
    let again = reopened(&dir);
    let (made, read) = (s.document().unwrap(), again.document().unwrap());
    let members = members_on_the_page(made, 4);
    same_frame(frame_at(read, 0, 4), box_frame(&members), "the reopened group is the box of its members, with no margin");
    assert_draws_alike(made, read, "the members are drawn where they were");
}

/// A pin put elsewhere than the middle of the box (`shape.pin`) is part of the frame a group cannot keep in the file: the reader puts the pin at the middle. One note; the
/// members and the box are where they were.
#[test]
fn a_group_whose_pin_is_set_elsewhere_saves_as_its_members_with_one_note() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = rectangle(&mut s, (6.0, 6.0), (2.31, 0.97), "", 30.0);
    let b = rectangle(&mut s, (8.4, 7.1), (1.7, 1.3), "", 0.0);
    let g = group(&mut s, &[a, b]);
    run(&mut s, "shape.pin", json!({ "ids": [g], "at": "top-left" }));
    let saved = save(&mut s);
    let said = frame_notes(&saved);
    assert_eq!(said.len(), 1, "one note for the one group: {saved}");
    assert!(said[0].contains(&format!("group {g} on page 0")), "{}", said[0]);
    let again = reopened(&dir);
    let (made, read) = (s.document().unwrap(), again.document().unwrap());
    let members = members_on_the_page(made, 4);
    same_frame(frame_at(read, 0, 4), box_frame(&members), "the reopened group is the box of its members, its pin at the middle");
    assert_draws_alike(made, read, "the members are drawn where they were");
}

/// A group in a group, the inner one with `LockCalcWH` and a member moved out of it: the inner group's frame is not the box of its members, and so the outer's, which was made
/// from the inner's frame, is not the box of the members it will be read as either. One note for each of the two, each naming its group; the file read again has both as boxes.
#[test]
fn nested_groups_that_are_not_the_box_of_their_members_have_a_note_each_and_read_back_as_boxes() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = rectangle(&mut s, (6.0, 6.0), (2.31, 0.97), "", 30.0);
    let b = rectangle(&mut s, (8.4, 7.1), (1.7, 1.3), "", 0.0);
    let inner = group(&mut s, &[a, b]);
    let c = rectangle(&mut s, (7.0, 9.3), (1.9, 0.6), "", 45.0);
    let outer = group(&mut s, &[inner, c]);
    set_cell(&mut s, inner, "LockCalcWH", 1.0);
    run(&mut s, "shape.move", json!({ "ids": [b], "dx": 4.0, "dy": 0.0 }));
    let saved = save(&mut s);
    let said = frame_notes(&saved);
    assert_eq!(said.len(), 2, "one note for each of the two groups: {saved}");
    assert!(said.iter().any(|w| w.contains(&format!("group {inner} on page 0"))), "the inner group is named: {said:?}");
    assert!(said.iter().any(|w| w.contains(&format!("group {outer} on page 0"))), "the outer group is named: {said:?}");
    let again = reopened(&dir);
    let (made, read) = (s.document().unwrap(), again.document().unwrap());
    // The page's boxes of the members: the outer group is the box of all three leaves, the inner one the box of the two.
    let leaf = |doc: &Document, id: u32| (id, diacraft_engine::draw::shape_corners(doc, 0, id).unwrap());
    let all = box_frame(&[leaf(made, a), leaf(made, b), leaf(made, c)]);
    same_frame(frame_at(read, 0, 4), all, "the reopened outer group is the box of the three");
    let page = read.pages.get(0).unwrap();
    let outer_read = page.shapes.get(4).unwrap();
    let inner_read = outer_read.children.iter().find(|m| !m.children.is_empty()).unwrap();
    let inner_frame = diacraft_engine::draw::shape_frame(read, page.id, inner_read.id).unwrap();
    let two = box_frame(&[leaf(made, a), leaf(made, b)]);
    // The inner group's frame is in the outer's: its page place is the outer's lower-left corner moved by its own pin less its local pin.
    let (ox, oy) = (all[0] - all[2] / 2.0, all[1] - all[3] / 2.0);
    let page_pin = (ox + inner_frame.pin_x, oy + inner_frame.pin_y);
    assert!(
        (page_pin.0 - two[0]).abs() < 1e-6 && (page_pin.1 - two[1]).abs() < 1e-6,
        "the inner group's pin is the middle of the box of its two: {page_pin:?} against {two:?}"
    );
    assert!((inner_frame.width - two[2]).abs() < 1e-6 && (inner_frame.height - two[3]).abs() < 1e-6, "and its size is that box's");
    assert_draws_alike(made, read, "the members are drawn where they were");
}

// ------------------------------------------------------------------------------------------------------------------ the review of c14de39

/// A small PNG.
fn small_png() -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&[200u8; 16]).unwrap();
    out
}

/// An `.odg` with a picture frame `P` (4 by 3 centimetres at (2, 3)) and a rectangle `A`, whose picture is in `Pictures/p.png`.
fn drawing_with_a_picture() -> Drawing {
    let frame = "<draw:frame draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"P\" svg:width=\"4cm\" svg:height=\"3cm\" svg:x=\"2cm\" svg:y=\"3cm\"><draw:image xlink:href=\"Pictures/p.png\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/></draw:frame>";
    Drawing::new().page(&[frame.to_owned(), rect("A", (2.0, 10.0, 4.0, 2.0), "hello")].concat()).file("Pictures/p.png", &small_png())
}

/// Every shape of the first page, depth first, with its corners on the page.
fn leaves_on_the_page(doc: &Document) -> Vec<(u32, [(f64, f64); 4])> {
    let page = doc.pages.get(0).unwrap();
    let mut out = Vec::new();
    let mut stack: Vec<&std::sync::Arc<diacraft_doc::Shape>> = page.shapes.iter().rev().collect();
    while let Some(shape) = stack.pop() {
        out.push((shape.id, diacraft_engine::draw::shape_corners(doc, page.id, shape.id).unwrap()));
        stack.extend(shape.children.iter().rev());
    }
    out
}

/// BLOCKER of the review: a picture that was a member of a new group was written at its place in the group's frame, as if that were the page, so the file read again had it
/// moved by the group's lower-left corner (-1.04, -5.5 inches in the review's drawing) and the save said nothing. A picture duplicated, grouped with a turned rectangle and saved
/// reopens at the session's corners, with the picture upright and with the picture turned.
#[test]
fn a_duplicated_picture_in_a_new_group_with_a_turned_rectangle_reopens_where_the_session_has_it() {
    for (what, picture_angle, rectangle_angle) in [
        ("upright picture, rectangle turned 30", 0.0, 30.0),
        ("picture turned 30, rectangle turned 133", 30.0, 133.0),
        ("picture turned 90, rectangle upright", 90.0, 0.0),
    ] {
        let dir = Dir::new();
        let mut s = open_bytes_in(&dir, "a.odg", &drawing_with_a_picture().bytes());
        let picture = s.document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().id;
        let copy = run(&mut s, "shape.duplicate", json!({ "ids": [picture] }));
        let copy = u32::try_from(copy["ids"][0].as_u64().unwrap()).unwrap();
        if picture_angle != 0.0 {
            run(&mut s, "shape.rotate", json!({ "ids": [copy], "angle": picture_angle }));
        }
        let t = rectangle(&mut s, (6.0, 6.0), (2.31, 0.97), "", rectangle_angle);
        group(&mut s, &[copy, t]);
        let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{what}: refused: {e}"));
        assert_eq!(saved["warning_count"], 0, "{what}: {saved}");
        let again = reopened(&dir);
        let (made, read) = (leaves_on_the_page(s.document().unwrap()), leaves_on_the_page(again.document().unwrap()));
        assert_eq!(made.len(), read.len(), "{what}: the shapes");
        for (k, ((_, a), (_, b))) in made.iter().zip(&read).enumerate() {
            for (p, q) in a.iter().zip(b) {
                assert!(
                    (p.0 - q.0).abs() < 1e-6 && (p.1 - q.1).abs() < 1e-6,
                    "{what}: shape {k} of the page's tree: the session has {a:?} and the file {b:?}"
                );
            }
        }
        assert_draws_alike(s.document().unwrap(), again.document().unwrap(), what);
    }
}

/// A connection point of its own on the top-level group `group` of the first page (`X` is half its width, `Y` its foot), made as a file or a ShapeSheet edit makes one: a group
/// has none from `shape.group`.
fn give_a_connection_point(s: &mut Session, group: u32) {
    let width = frame_at(s.document().unwrap(), 0, 4)[2];
    s.edit("give the group a connection point", |doc| {
        let page = doc.pages.get_mut(0).map(std::sync::Arc::make_mut).expect("a first page");
        let at = page.shapes.iter().position(|x| x.id == group).expect("the group is on the page");
        let shape = page.shapes.get_mut(at).map(std::sync::Arc::make_mut).expect("the shape");
        let cell = |formula: &str, v: f64| diacraft_doc::Cell::with_formula(formula, Some(Value::from_text(&format!("{v}")))).with_unit("IN");
        let mut cells = diacraft_doc::Cells::new();
        cells.insert(diacraft_doc::CellName::new("X"), cell("Width*0.5", width / 2.0));
        cells.insert(diacraft_doc::CellName::new("Y"), cell("Height*0", 0.0));
        let mut section = diacraft_doc::Section::new("Connection", None);
        section.rows.push(std::sync::Arc::new(diacraft_doc::Row { ix: Some(0), cells, ..diacraft_doc::Row::default() }));
        std::sync::Arc::make_mut(&mut shape.sheet).sections.push(std::sync::Arc::new(section));
        Ok(())
    })
    .unwrap();
}

/// MEDIUM of the review: a connector glued to a group's own connection point when the group's frame is not kept. The reopened group has its points on the box, and the
/// connector's end stays where it was in the file, so the save names the connector. With a group that is the box, nothing is said.
#[test]
fn a_connector_glued_to_a_group_whose_frame_is_not_kept_is_named_in_the_note() {
    for (margin, expected) in [(true, 1usize), (false, 0)] {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &four());
        let a = rectangle(&mut s, (6.0, 6.0), (2.31, 0.97), "", 30.0);
        let b = rectangle(&mut s, (8.4, 7.1), (1.7, 1.3), "", 0.0);
        let g = group(&mut s, &[a, b]);
        give_a_connection_point(&mut s, g);
        let outside = rectangle(&mut s, (2.0, 12.0), (1.0, 1.0), "", 0.0);
        let wire = run(&mut s, "connector.add", json!({ "from": { "id": outside }, "to": { "id": g, "point": 0 } }));
        let wire = u32::try_from(wire["id"].as_u64().unwrap()).unwrap();
        if margin {
            let frame = frame_at(s.document().unwrap(), 0, 4);
            set_cell(&mut s, g, "Width", frame[2] + 1.0);
            set_cell(&mut s, g, "Height", frame[3] + 1.0);
            set_cell(&mut s, g, "PinX", frame[0] + 0.5);
            set_cell(&mut s, g, "PinY", frame[1] + 0.5);
        }
        let saved = save(&mut s);
        let said = frame_notes(&saved);
        assert_eq!(said.len(), expected, "margin {margin}: {saved}");
        if margin {
            assert!(
                said[0].contains(&format!("the connector glued to the group itself ({wire}) keeps its end where it was in the file")),
                "{}",
                said[0]
            );
        } else {
            assert_eq!(saved["warning_count"], 0, "{saved}");
        }
    }
}

/// MEDIUM of the review: a `LockCalcWH` group that was saved (so a group of the file) and whose member then turned was saved with no note, and read back as the box with the
/// frame off by 0.033 inch in the review's drawing. A group of the file that a save changed something under is told as a new one is; one that nothing under it changed is not,
/// and a group that the refit keeps the box of (no lock) is never told.
#[test]
fn a_lockcalcwh_group_that_was_saved_and_whose_member_then_turns_is_noted_again() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = rectangle(&mut s, (6.0, 6.0), (2.31, 0.97), "", 30.0);
    let b = rectangle(&mut s, (8.4, 7.1), (1.7, 1.3), "", 0.0);
    let g = group(&mut s, &[a, b]);
    set_cell(&mut s, g, "LockCalcWH", 1.0);
    run(&mut s, "shape.move", json!({ "ids": [b], "dx": 3.0, "dy": -1.5 }));
    let first = save(&mut s);
    assert_eq!(frame_notes(&first).len(), 1, "{first}");
    // A shape beside the group moves: the group was not touched, and the page says nothing of it.
    crate::odg_file::shift(&mut s, "D", 1.0, 0.0);
    let beside = save(&mut s);
    assert_eq!(beside["edited"], true, "{beside}");
    assert_eq!(frame_notes(&beside), Vec::<String>::new(), "{beside}");
    // A member of it turns: the box of the members is another, and the frame the session holds is not it.
    run(&mut s, "shape.rotate", json!({ "ids": [a], "angle": 70 }));
    let turned = save(&mut s);
    let said = frame_notes(&turned);
    assert_eq!(said.len(), 1, "{turned}");
    assert!(said[0].contains(&format!("group {g} on page 0")), "{}", said[0]);
    let again = reopened(&dir);
    let (made, read) = (s.document().unwrap(), again.document().unwrap());
    let box_now = box_frame(&members_on_the_page(made, 4));
    same_frame(frame_at(read, 0, 4), box_now, "the reopened group is the box of its members");
    let held = frame_at(made, 0, 4);
    assert!(
        (held[2] - box_now[2]).abs() > 0.01 || (held[3] - box_now[3]).abs() > 0.01,
        "the session holds a frame that is not the box: {held:?} against {box_now:?}"
    );
    // A group that is not locked is refitted by the edit of a member and is the box all the time: no note, saved or not.
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = rectangle(&mut s, (6.0, 6.0), (2.31, 0.97), "", 30.0);
    let b = rectangle(&mut s, (8.4, 7.1), (1.7, 1.3), "", 0.0);
    group(&mut s, &[a, b]);
    run(&mut s, "shape.move", json!({ "ids": [b], "dx": 3.0, "dy": -1.5 }));
    assert_eq!(frame_notes(&save(&mut s)), Vec::<String>::new());
    run(&mut s, "shape.rotate", json!({ "ids": [a], "angle": 70 }));
    let after = save(&mut s);
    assert_eq!(after["edited"], true);
    assert_eq!(frame_notes(&after), Vec::<String>::new(), "{after}");
}
