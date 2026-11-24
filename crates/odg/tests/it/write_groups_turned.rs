//! The review of c14de39: a picture in a new group is written where the group's map puts it; a one-dimensional shape that holds members is refused; a
//! connector glued to a group whose frame is not kept is named in the note; a group of the file that a save changed something under is judged against the box of its members
//! as a new one is; and the precision a frame is held to grows with the size of the group.

use std::sync::Arc;

use diacraft_doc::{CellName, Document, Shape, ShapeList};
use diacraft_odg::SaveNote;

use crate::drawing::{Drawing, cm};
use crate::flat_write::{picture_shape, small_png};
use crate::write_core::{custom, move_by, open, open_bytes, part, real, set_num};
use crate::write_groups::{cell, element, group, member, num, pair, unkept};
use crate::write_new::{connector, glue_part, push_shape};

/// The words of the notes of a save about a group's frame.
fn frame_words(notes: &[SaveNote]) -> Vec<String> {
    notes.iter().filter(|n| matches!(n, SaveNote::GroupFrameNotKept { .. })).map(ToString::to_string).collect()
}

/// The number a cell of the shape at `path` of page 0 holds, 0 for a cell it does not have (a frame's turn is there only when it is not 0).
fn num0(doc: &Document, path: &[usize], name: &str) -> f64 {
    let mut shape = doc.pages.get(0).unwrap().shapes.get(path[0]).unwrap();
    for at in &path[1..] {
        shape = shape.children.get(*at).unwrap();
    }
    shape.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0)
}

/// A picture 4 by 2 centimetres whose pin is at (`pin.0`, `pin.1`) centimetres in its group's frame, turned `angle` radians.
fn picture(id: u32, pin: (f64, f64), angle: f64) -> Shape {
    let mut p = picture_shape(id, "Pic", (0.0, 0.0, 4.0, 2.0), &small_png());
    set_num(&mut p, "PinX", cm(pin.0));
    set_num(&mut p, "PinY", cm(pin.1));
    if angle != 0.0 {
        Arc::make_mut(&mut p.sheet).cells.insert(CellName::new("Angle"), cell(None, angle));
    }
    p
}

/// BLOCKER of the review: `picture_text` placed a picture by its own cells, which are in its group's frame, and not on the page, so a picture in a new group was written at
/// the group's lower-left corner less where it was (and the save said nothing). A picture 4 by 2 with its pin at (2, 1) in the group whose box is (10, 12) to (18, 18) is
/// written at (10, 15.7) as a rectangle of the same size is, and the file reads back with the picture where the model has it in the group; **turned a quarter**, with its pin at
/// (1, 2) as the test of a turned rectangle has it, it is written turned about its middle on the page, and reads back turned in the group's frame.
#[test]
fn a_picture_that_is_a_member_of_a_new_group_is_written_where_the_groups_map_puts_it() {
    for (what, angle, pin, frame, beside) in [
        ("upright", 0.0, (2.0, 1.0), (10.0, 12.0, 18.0, 18.0), true),
        ("turned a quarter", std::f64::consts::FRAC_PI_2, (1.0, 2.0), (10.0, 12.0, 12.0, 16.0), false),
    ] {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let first = o.index.next_id(0).unwrap();
        let mut members = vec![picture(first, pin, angle)];
        if beside {
            members.push(member(first + 1, "B", (3.0, 3.0), (6.5, 4.5)));
        }
        push_shape(&mut o.doc, group(first + 2, frame, members));
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{what}: not saved: {e}"));
        assert_eq!(saved.notes, [], "{what}: the frame is the box of the members, and the picture says nothing");
        let content = part(&saved.bytes, "content.xml");
        let written = element(&content, "frame");
        if angle == 0.0 {
            assert!(
                written.contains(r#"svg:width="4cm" svg:height="2cm" svg:x="10cm" svg:y="15.7cm""#),
                "{what}: the picture is on the page where the group puts it: {written}"
            );
        } else {
            // A quarter turn about the middle of the frame, which is at (11, 14) centimetres from the page's lower-left corner: the translation puts the turned corner.
            assert!(written.contains("draw:transform=\"rotate (1.5707963267948966) translate ("), "{what}: {written}");
        }
        let back = open_bytes(saved.bytes.clone());
        for name in ["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY"] {
            let (made, read) = (num(&o.doc, &[1], name), num(&back.doc, &[1], name));
            assert!((made - read).abs() < 1e-6, "{what}: the group's {name}: {made} against {read}");
        }
        for name in ["PinX", "PinY", "Width", "Height", "Angle"] {
            let (made, read) = (num0(&o.doc, &[1, 0], name), num0(&back.doc, &[1, 0], name));
            assert!((made - read).abs() < 1e-6, "{what}: the picture's {name}: {made} against {read}");
        }
        let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
        assert!(!again.edited && again.bytes == saved.bytes, "{what}: what was read is saved as no edit");
    }
}

/// A new line or connector that holds members is refused in words, as it was before a change (main refused it for its frame): written as a `draw:g` it reads back as a
/// group whose moves the session's shape does not follow, and its next save is refused. Both the line whose ends are along the box and one elsewhere.
#[test]
fn a_new_line_or_connector_that_holds_members_is_refused_in_words() {
    for (what, route, ends) in [
        ("a line along the box", false, (10.0, 12.0, 18.0, 12.0)),
        ("a line elsewhere", false, (2.0, 2.0, 5.0, 2.0)),
        ("a connector along the box", true, (10.0, 12.0, 18.0, 12.0)),
        ("a connector elsewhere", true, (2.0, 2.0, 5.0, 2.0)),
    ] {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let first = o.index.next_id(0).unwrap();
        let mut c = connector(first + 2, ends);
        if !route {
            for name in ["ShapeRouteStyle", "ConLineRouteExt"] {
                Arc::make_mut(&mut c.sheet).cells.remove(&CellName::new(name));
            }
        }
        c.children =
            ShapeList::from_vec(vec![Arc::new(member(first, "A", (4.0, 2.0), (2.0, 1.0))), Arc::new(member(first + 1, "B", (3.0, 3.0), (6.5, 4.5)))]);
        push_shape(&mut o.doc, c);
        let words = real(&o.doc, &o.bytes, &o.index).map(|_| ()).expect_err(what).to_string();
        assert!(words.contains("a new line or connector that is a group"), "{what}: {words}");
    }
}

/// A connector glued to a group whose frame is not kept: the group's connection points are those of the box the file gives back, and the connector's end stays where it
/// was in the file, so the note names the connector (one, several, and a few too many to name), a connector glued to a member is not named, a group that is the box has
/// no note at all, and an inner group's connector is named in the inner group's note and not in the outer's.
#[test]
fn a_connector_glued_to_a_group_whose_frame_is_not_kept_is_named_in_the_note() {
    let build = |frame: (f64, f64, f64, f64), glued: &[(u32, u32)], to_member: bool| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let first = o.index.next_id(0).unwrap();
        let members = vec![member(first, "A", (4.0, 2.0), (2.0, 1.0)), member(first + 1, "B", (3.0, 3.0), (6.5, 4.5))];
        push_shape(&mut o.doc, group(first + 2, frame, members));
        let mut wires = Vec::new();
        for (k, (part_of, _)) in glued.iter().enumerate() {
            let wire = first + 10 + u32::try_from(k).unwrap();
            push_shape(&mut o.doc, connector(wire, (2.0, 22.0, 10.0, 15.0)));
            glue_part(&mut o.doc, wire, true, first + 2, *part_of);
            wires.push(wire);
        }
        if to_member {
            let wire = first + 40;
            push_shape(&mut o.doc, connector(wire, (2.0, 25.0, 11.0, 14.0)));
            glue_part(&mut o.doc, wire, true, first, 3);
        }
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
        (first + 2, wires, saved)
    };
    let (margin, the_box) = ((10.0, 12.0, 20.0, 19.0), (10.0, 12.0, 18.0, 18.0));
    // One connector, glued to the whole group.
    let (g, wires, saved) = build(margin, &[(3, 0)], false);
    assert_eq!(unkept(&saved.notes), [(0, g)]);
    let words = frame_words(&saved.notes);
    assert!(
        words[0].contains(&format!("the connector glued to the group itself ({}) keeps its end where it was in the file", wires[0])),
        "{words:?}"
    );
    // Two connectors, one glued to the whole group and one to a connection point of it (the points are 100 and up): named in order, once each.
    let (_, wires, saved) = build(margin, &[(3, 0), (101, 0)], false);
    assert!(
        frame_words(&saved.notes)[0].contains(&format!("the connectors glued to the group itself ({}, {}) keep their ends", wires[0], wires[1])),
        "{:?}",
        saved.notes
    );
    // Ten: the first eight are named and the rest counted.
    let ten: Vec<(u32, u32)> = (0..10).map(|_| (3, 0)).collect();
    let (_, wires, saved) = build(margin, &ten, false);
    let named: Vec<String> = wires.iter().take(8).map(u32::to_string).collect();
    assert!(frame_words(&saved.notes)[0].contains(&format!("({} and 2 more)", named.join(", "))), "{:?}", saved.notes);
    // A connector glued to a member of the group is no concern of the group's frame: the group's note names none.
    let (g, _, saved) = build(margin, &[], true);
    assert_eq!(unkept(&saved.notes), [(0, g)]);
    assert!(!frame_words(&saved.notes)[0].contains("glued"), "{:?}", saved.notes);
    // A group that is the box has no note, whatever is glued to it.
    let (_, _, saved) = build(the_box, &[(3, 0), (101, 0)], false);
    assert_eq!(saved.notes, []);
    // Nested: the inner group's frame is not kept and a connector is glued to it; the outer group is the box of the leaves and has no note.
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    let inner = group(first + 1, (2.0, 1.0, 7.0, 4.0), vec![member(first, "A", (4.0, 2.0), (2.0, 1.0))]);
    let outer = group(first + 3, (10.0, 12.0, 16.0, 15.0), vec![inner, member(first + 2, "B", (2.0, 2.0), (1.0, 1.0))]);
    push_shape(&mut o.doc, outer);
    push_shape(&mut o.doc, connector(first + 10, (2.0, 22.0, 10.0, 15.0)));
    glue_part(&mut o.doc, first + 10, true, first + 1, 3);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert_eq!(unkept(&saved.notes), [(0, first + 1)], "the inner group's frame is the one not kept: {:?}", saved.notes);
    assert!(frame_words(&saved.notes)[0].contains(&format!("({})", first + 10)), "{:?}", saved.notes);
}

/// The cell `name` of the member `k` of the group at place `at` of page 0 of `doc`, set to `value` inches (its formula stays).
fn set_member(doc: &mut Document, at: usize, k: usize, name: &str, value: f64) {
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let group = Arc::make_mut(page.shapes.get_mut(at).unwrap());
    set_num(Arc::make_mut(group.children.get_mut(k).unwrap()), name, value);
}

/// MEDIUM of the review: a group already saved (so a group of the file) that is held to a size and has a member turn or move gave no note, and read back as the box with the
/// frame off by what the member changed. A group of the file that a save changed something under (a member's pose, a member taken away, or its own pose) and whose frame is not
/// the box of its members has a note, as a new group has; **a group nothing under it changed has none**, on a page that was edited elsewhere, and a group whose frame was made
/// the box again (what the refit does) has none.
#[test]
fn a_group_of_the_file_that_a_save_changed_something_under_is_judged_against_the_box_of_its_members() {
    let written = || {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let first = o.index.next_id(0).unwrap();
        push_shape(&mut o.doc, pair(first, 10.0, None));
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
        assert_eq!(saved.notes, []);
        let back = open_bytes(saved.bytes.clone());
        let id = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().id;
        (back, saved.bytes, id)
    };
    let said = |back: &crate::write_core::Open, bytes: &[u8]| real(&back.doc, bytes, &back.index).unwrap_or_else(|e| panic!("not saved: {e}")).notes;
    // A member moved out of the frame, the frame held: the group is not the box of its members.
    let (mut back, bytes, id) = written();
    set_member(&mut back.doc, 1, 1, "PinX", cm(9.5));
    assert_eq!(unkept(&said(&back, &bytes)), [(0, id)], "a member moved out of a frame that was held");
    // A member turned: the box is another, and the frame held is not it.
    let (mut back, bytes, id) = written();
    {
        let page = Arc::make_mut(back.doc.pages.get_mut(0).unwrap());
        let group = Arc::make_mut(page.shapes.get_mut(1).unwrap());
        let a = Arc::make_mut(group.children.get_mut(0).unwrap());
        Arc::make_mut(&mut a.sheet).cells.insert(CellName::new("Angle"), cell(None, 0.5));
    }
    assert_eq!(unkept(&said(&back, &bytes)), [(0, id)], "a member turned");
    // A member taken away: the frame is the box of two and the group holds one.
    let (mut back, bytes, id) = written();
    {
        let page = Arc::make_mut(back.doc.pages.get_mut(0).unwrap());
        let group = Arc::make_mut(page.shapes.get_mut(1).unwrap());
        let first = Arc::clone(group.children.get_mut(0).unwrap());
        group.children = ShapeList::from_vec(vec![first]);
    }
    assert_eq!(unkept(&said(&back, &bytes)), [(0, id)], "a member taken away");
    // The same member moved out and the frame made the box again (the refit): no note.
    let (mut back, bytes, _) = written();
    set_member(&mut back.doc, 1, 1, "PinX", cm(9.5));
    {
        // B reaches 11 centimetres now: the box is 11 by 6 from the same corner, so the pin is at 5.5 and 3 from it.
        let page = Arc::make_mut(back.doc.pages.get_mut(0).unwrap());
        let group = Arc::make_mut(page.shapes.get_mut(1).unwrap());
        for (name, v) in [("PinX", cm(15.5)), ("Width", cm(11.0)), ("LocPinX", cm(5.5))] {
            set_num(group, name, v);
        }
    }
    assert_eq!(said(&back, &bytes), [], "the frame is the box of the members again");
    // A group saved while it was held to a size (a margin: the save said so), then a shape beside it moved: the group is as the save left it, nothing under it changed, and
    // a page edited elsewhere says nothing of it. A member of it that turns after that is said again.
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    let held =
        group(first + 2, (10.0, 12.0, 20.0, 19.0), vec![member(first, "A", (4.0, 2.0), (2.0, 1.0)), member(first + 1, "B", (3.0, 3.0), (6.5, 4.5))]);
    let held_id = held.id;
    push_shape(&mut o.doc, held);
    let first_save = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert_eq!(unkept(&first_save.notes), [(0, held_id)], "the margin is said when it is written");
    let s1 = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    move_by(&mut o.doc, 0, s1, 1.0, 0.0);
    let beside = real(&o.doc, &first_save.bytes, &first_save.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(beside.edited, "the shape beside the group moved");
    assert_eq!(beside.notes, [], "nothing under the group changed");
    {
        let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        let group = Arc::make_mut(page.shapes.get_mut(1).unwrap());
        let a = Arc::make_mut(group.children.get_mut(0).unwrap());
        Arc::make_mut(&mut a.sheet).cells.insert(CellName::new("Angle"), cell(None, 0.5));
    }
    let turned = real(&o.doc, &beside.bytes, &beside.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert_eq!(unkept(&turned.notes), [(0, held_id)], "a member of the group turned: said again");
    // A group inside a group: the member moved out is under both, the outer's frame is held (the box of the inner's and the beside member) and the inner's is not touched by
    // it: only the groups under which something changed are judged, and each by its leaves.
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    let inner = group(first + 1, (2.0, 1.0, 6.0, 3.0), vec![member(first, "A", (4.0, 2.0), (2.0, 1.0))]);
    let outer = group(first + 3, (10.0, 12.0, 16.0, 15.0), vec![inner, member(first + 2, "B", (2.0, 2.0), (1.0, 1.0))]);
    push_shape(&mut o.doc, outer);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert_eq!(saved.notes, []);
    let mut back = open_bytes(saved.bytes.clone());
    let outer_id = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().id;
    {
        let page = Arc::make_mut(back.doc.pages.get_mut(0).unwrap());
        let group = Arc::make_mut(page.shapes.get_mut(1).unwrap());
        // B is the outer's second member: it moves right by 3 centimetres, beyond the outer's frame.
        set_num(Arc::make_mut(group.children.get_mut(1).unwrap()), "PinX", cm(4.0));
    }
    assert_eq!(unkept(&said(&back, &saved.bytes)), [(0, outer_id)], "the outer group only: nothing under the inner changed");
}

/// LOW of the review: the precision a frame is held to is a millionth of an inch **or a billionth of the group's largest length**, whichever is more: a group of 3,150 by
/// 2,360 inches is the box at 3E-6 and not at 4E-6, and a group of 8 by 6 centimetres is held to a millionth as before.
#[test]
fn the_precision_of_a_groups_frame_grows_with_its_size() {
    let notes = |by: f64| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let first = o.index.next_id(0).unwrap();
        let members = vec![member(first, "A", (4000.0, 2000.0), (2000.0, 1000.0)), member(first + 1, "B", (3000.0, 3000.0), (6500.0, 4500.0))];
        let mut g = group(first + 2, (0.0, 0.0, 8000.0, 6000.0), members);
        let w = num_of_shape(&g, "Width");
        set_num(&mut g, "Width", w + by);
        set_num(&mut g, "LocPinX", (w + by) / 2.0);
        push_shape(&mut o.doc, g);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
        unkept(&saved.notes)
    };
    assert_eq!(notes(2.5e-6), [], "3,150 inches wide: a billionth of that is 3E-6");
    assert_eq!(notes(-2.5e-6), [], "either way");
    assert_eq!(notes(4e-6).len(), 1, "4E-6 is past it");
    assert_eq!(notes(-4e-6).len(), 1);
}

fn num_of_shape(shape: &Shape, name: &str) -> f64 {
    shape.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0)
}
