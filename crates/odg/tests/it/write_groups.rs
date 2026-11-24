//! New groups: a group of new shapes is written as a `draw:g` holding its members, placed on the page by the groups around
//! them, and reads back as the group the model has; what the reader cannot rebuild is refused in words.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Shape, ShapeList, ShapeType};
use diacraft_odg::SaveNote;

use crate::drawing::{Drawing, cm};
use crate::write_core::{Open, custom, num_of, open, open_bytes, part, real, set_num, shape_mut};
use crate::write_new::{connector, push_shape, rectangle, with_side_middles};

pub(crate) fn cell(formula: Option<&str>, value: f64) -> Cell {
    Cell::read(formula, Some(&value.to_string()), Some("IN"), None, &mut 1_000)
}

/// A rectangle as `write_new::rectangle` makes it, put in a group's frame: its pin at (`x`, `y`) centimetres from the group's lower-left corner.
pub(crate) fn member(id: u32, name: &str, size: (f64, f64), pin: (f64, f64)) -> Shape {
    let mut shape = rectangle(id, name, (0.0, 0.0, size.0, size.1));
    set_num(&mut shape, "PinX", cm(pin.0));
    set_num(&mut shape, "PinY", cm(pin.1));
    shape
}

/// A group whose box is `(x0, y0, x1, y1)` centimetres on the page (y up) holding `members`, as `shape.group` makes one: the pin at the middle, the local pin by the
/// formulas `Width*0.5` and `Height*0.5`, the angle 0.
pub(crate) fn group(id: u32, (x0, y0, x1, y1): (f64, f64, f64, f64), members: Vec<Shape>) -> Shape {
    let (w, h) = (x1 - x0, y1 - y0);
    let mut shape = Shape::new(id);
    shape.kind = ShapeType::Group;
    let sheet = Arc::make_mut(&mut shape.sheet);
    for (name, formula, value) in [
        ("PinX", None, cm((x0 + x1) / 2.0)),
        ("PinY", None, cm((y0 + y1) / 2.0)),
        ("Width", None, cm(w)),
        ("Height", None, cm(h)),
        ("LocPinX", Some("Width*0.5"), cm(w) / 2.0),
        ("LocPinY", Some("Height*0.5"), cm(h) / 2.0),
    ] {
        sheet.cells.insert(CellName::new(name), cell(formula, value));
    }
    shape.children = ShapeList::from_vec(members.into_iter().map(Arc::new).collect());
    shape
}

/// A page with the shape `s1` and, after it, a group of two rectangles: A (4 by 2 centimetres) at the group's lower-left and B (3 by 3) up and to the right of it, on
/// a box 8 by 6 centimetres whose lower-left corner is at (10, 12) from the page's lower-left.
fn with_a_group(open: &mut Open) -> (u32, u32) {
    let first = open.index.next_id(0).unwrap();
    let members = vec![member(first, "A", (4.0, 2.0), (2.0, 1.0)), member(first + 1, "B", (3.0, 3.0), (6.5, 4.5))];
    push_shape(&mut open.doc, group(first + 2, (10.0, 12.0, 18.0, 18.0), members));
    (first, first + 2)
}

/// The cells of a shape by name, as numbers.
pub(crate) fn num(doc: &Document, path: &[usize], name: &str) -> f64 {
    let mut shapes = &doc.pages.get(0).unwrap().shapes;
    let mut shape = shapes.get(path[0]).unwrap();
    for at in &path[1..] {
        shapes = &shape.children;
        shape = shapes.get(*at).unwrap();
    }
    shape.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN)
}

pub(crate) fn element(content: &str, tag: &str) -> String {
    let at = content.find(&format!("<draw:{tag}")).unwrap_or_else(|| panic!("no {tag}: {content}"));
    let end = content[at..].find(&format!("</draw:{tag}>")).map_or(content.len(), |e| at + e + format!("</draw:{tag}>").len());
    content[at..end].to_owned()
}

/// A new group is written as a `draw:g` holding its members, whose attributes are the page's; it reads back as a group of two members, with the frame the model has,
/// the members in its frame, no data of DiaCraft's, and a save of what was read is no edit.
#[test]
fn a_new_group_is_written_as_a_draw_g_with_its_members_placed_on_the_page_and_reads_back() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    with_a_group(&mut o);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&saved.bytes, "content.xml");
    let written = element(&content, "g");
    assert_eq!(written.matches("<draw:custom-shape").count(), 2, "{written}");
    // A is 4 by 2 with its pin 2 and 1 from the box's corner (10, 12): its left edge is at 10, its top at 29.7 - 12 - 2 = 15.7 from the page's top.
    assert!(written.contains(r#"svg:width="4cm" svg:height="2cm" svg:x="10cm" svg:y="15.7cm""#), "{written}");
    // B is 3 by 3 with its pin 6.5 and 4.5 from the corner: its left edge is at 10 + 5 = 15, its top at 29.7 - 12 - 6 = 11.7.
    assert!(written.contains(r#"svg:width="3cm" svg:height="3cm" svg:x="15cm" svg:y="11.7cm""#), "{written}");
    assert!(!written.contains("draw:z-index") && !content.contains("diacraft:version"), "a plain group carries no DiaCraft data: {content}");
    assert!(!part_exists(&saved.bytes, "DiaCraft/document.xml"), "and no document file");
    let back = open_bytes(saved.bytes.clone());
    let page = back.doc.pages.get(0).unwrap();
    assert_eq!(page.shapes.len(), 2, "the shape and the group");
    let group = page.shapes.get(1).unwrap();
    assert_eq!((group.kind.clone(), group.children.len()), (ShapeType::Group, 2));
    for name in ["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY"] {
        let (made, read) = (num(&o.doc, &[1], name), num(&back.doc, &[1], name));
        assert!((made - read).abs() < 1e-6, "the group's {name}: {made} against {read}");
    }
    for (at, name) in [(0, "A"), (1, "B")] {
        for cellname in ["PinX", "PinY", "Width", "Height"] {
            let (made, read) = (num(&o.doc, &[1, at], cellname), num(&back.doc, &[1, at], cellname));
            assert!((made - read).abs() < 1e-6, "{name}'s {cellname}: {made} against {read}");
        }
    }
    // The members' ids are the model's: the ones that follow the shape of the file in the pre-order the reader numbers.
    let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes, "what was read is saved as no edit");
}

fn part_exists(bytes: &[u8], name: &str) -> bool {
    let package = diacraft_odg::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    package.part(name).is_some()
}

/// Two new groups, one of them after the shape of the file and one first; the second written in the same save as the first, in the model's order; and a second save of
/// the file read is no edit.
#[test]
fn two_new_groups_are_written_in_the_models_order_and_numbered_with_their_members() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    let one =
        group(first + 2, (10.0, 12.0, 18.0, 18.0), vec![member(first, "A", (4.0, 2.0), (2.0, 1.0)), member(first + 1, "B", (3.0, 3.0), (6.5, 4.5))]);
    let two =
        group(first + 5, (2.0, 2.0, 7.0, 6.0), vec![member(first + 3, "C", (2.0, 2.0), (1.0, 1.0)), member(first + 4, "D", (2.0, 2.0), (4.0, 3.0))]);
    push_shape(&mut o.doc, one);
    push_shape(&mut o.doc, two);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&saved.bytes, "content.xml");
    assert_eq!(content.matches("<draw:g").count(), 2, "{content}");
    let back = open_bytes(saved.bytes.clone());
    let page = back.doc.pages.get(0).unwrap();
    assert_eq!((page.shapes.len(), page.shapes.get(1).unwrap().children.len(), page.shapes.get(2).unwrap().children.len()), (3, 2, 2));
    // The index holds the members: the model's ids, by the ordinals of their elements (the shape of the file, a group, its two members, a group, its two).
    let held: Vec<Option<u32>> =
        [first + 2, first, first + 1, first + 5, first + 3, first + 4].iter().map(|id| saved.index.ordinal(0, *id)).collect();
    assert_eq!(held, [Some(2), Some(3), Some(4), Some(5), Some(6), Some(7)], "{held:?}");
    let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
}

/// A group inside a group, eight deep, is written nested, numbered in the pre-order of the file, and read back as the tree the model has.
#[test]
fn a_new_group_eight_deep_is_written_nested_and_reads_back_as_the_tree_the_model_has() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    // The innermost member, and seven groups round it, each with the same box (10, 12) to (14, 14): the frame of each is the box of the one inside.
    let mut inner = member(first, "Deep", (4.0, 2.0), (2.0, 1.0));
    for level in 0..7u32 {
        let local_box = (0.0, 0.0, 4.0, 2.0);
        let mut g = group(first + 1 + level, (10.0, 12.0, 14.0, 14.0), vec![inner]);
        // The member of a group is in the group's frame, where the box starts at the origin: a nested group's frame is its box in the frame it is in.
        set_num(&mut g, "PinX", cm((local_box.0 + local_box.2) / 2.0 + if level == 6 { 10.0 } else { 0.0 }));
        set_num(&mut g, "PinY", cm((local_box.1 + local_box.3) / 2.0 + if level == 6 { 12.0 } else { 0.0 }));
        inner = g;
    }
    push_shape(&mut o.doc, inner);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&saved.bytes, "content.xml");
    assert_eq!(content.matches("<draw:g").count(), 7, "{content}");
    let back = open_bytes(saved.bytes.clone());
    let mut shape = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().clone();
    let mut depth = 0;
    while let Some(next) = shape.children.get(0).cloned() {
        shape = next;
        depth += 1;
    }
    assert_eq!(depth, 7, "seven groups and the shape inside them");
    let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
}

/// A group with a connector as a member is written with the connector's ends and route on the page, through the group's frame.
#[test]
fn a_connector_in_a_new_group_is_written_through_the_groups_frame() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    let mut flow = connector(first, (0.0, 0.0, 6.0, 0.0));
    // Its ends are in the group's frame: 0 to 6 centimetres along the box's bottom edge, which is at the origin; put in the frame by the origin's y.
    for (name, v) in [("BeginY", 1.0), ("EndY", 1.0), ("PinY", 1.0)] {
        set_num(&mut flow, name, cm(v));
    }
    for (name, v) in [("BeginX", 0.0), ("EndX", 6.0), ("PinX", 3.0)] {
        set_num(&mut flow, name, cm(v));
    }
    let tall = member(first + 1, "T", (1.0, 2.0), (3.0, 1.0));
    push_shape(&mut o.doc, group(first + 2, (10.0, 12.0, 16.0, 14.0), vec![flow, tall]));
    let saved = real(&o.doc, &o.bytes, &o.index);
    // The members' box is (0, 0) to (6, 2): the connector's ends at height 1 are inside it, and the rectangle fills the middle of it.
    let saved = saved.unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&saved.bytes, "content.xml");
    let written = element(&content, "connector");
    assert!(written.contains(r#"svg:x1="10cm" svg:y1="16.7cm" svg:x2="16cm" svg:y2="16.7cm""#), "{written}");
}

/// What the writer cannot write as a `draw:g` is refused in words and nothing is written: a group turned or flipped, a member or a frame that is not numbers, and a shape of
/// the file in a group. (A frame that is not the box of its members, and a pin that is not at its middle, are written as the members and said in a note:
/// `a_new_group_whose_frame_is_not_the_box_of_its_members_is_written_as_its_members_with_one_note` below.)
#[test]
fn a_new_group_the_reader_cannot_rebuild_is_refused_in_words() {
    let why = |build: &dyn Fn(&mut Open)| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        build(&mut o);
        match real(&o.doc, &o.bytes, &o.index) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("saved"),
        }
    };
    let members = |first: u32| vec![member(first, "A", (4.0, 2.0), (2.0, 1.0)), member(first + 1, "B", (3.0, 3.0), (6.5, 4.5))];
    // A group turned: the group's angle.
    let words = why(&|o| {
        let first = o.index.next_id(0).unwrap();
        let mut g = group(first + 2, (10.0, 12.0, 18.0, 18.0), members(first));
        Arc::make_mut(&mut g.sheet).cells.insert(CellName::new("Angle"), cell(None, 0.5));
        push_shape(&mut o.doc, g);
    });
    assert!(words.contains("a group that is turned or flipped") && !words.contains("later task"), "{words}");
    // A group flipped.
    let words = why(&|o| {
        let first = o.index.next_id(0).unwrap();
        let mut g = group(first + 2, (10.0, 12.0, 18.0, 18.0), members(first));
        Arc::make_mut(&mut g.sheet).cells.insert(CellName::new("FlipX"), cell(None, 1.0));
        push_shape(&mut o.doc, g);
    });
    assert!(words.contains("a group that is turned or flipped"), "{words}");
    // A member whose frame is not numbers (a text where a width should be), in the group itself and in a group in it; and a frame of the group that is not numbers.
    let words = why(&|o| {
        let first = o.index.next_id(0).unwrap();
        let mut bad = member(first, "A", (4.0, 2.0), (2.0, 1.0));
        Arc::make_mut(&mut bad.sheet).cells.insert(CellName::new("Width"), Cell::constant(diacraft_doc::Value::from_text("wide")));
        push_shape(&mut o.doc, group(first + 1, (10.0, 12.0, 14.0, 14.0), vec![bad]));
    });
    assert!(words.contains("a new group with a member whose frame is not numbers"), "{words}");
    let words = why(&|o| {
        let first = o.index.next_id(0).unwrap();
        let mut bad = member(first, "A", (4.0, 2.0), (2.0, 1.0));
        Arc::make_mut(&mut bad.sheet).cells.insert(CellName::new("PinY"), Cell::constant(diacraft_doc::Value::from_text("up")));
        let inner = group(first + 1, (0.0, 0.0, 4.0, 2.0), vec![bad]);
        push_shape(&mut o.doc, group(first + 2, (10.0, 12.0, 14.0, 14.0), vec![inner]));
    });
    assert!(words.contains("a new group with a member whose frame is not numbers"), "{words}");
    let words = why(&|o| {
        let first = o.index.next_id(0).unwrap();
        let mut g = group(first + 2, (10.0, 12.0, 18.0, 18.0), members(first));
        Arc::make_mut(&mut g.sheet).cells.insert(CellName::new("PinX"), Cell::constant(diacraft_doc::Value::from_text("middle")));
        push_shape(&mut o.doc, g);
    });
    assert!(words.contains("a new group whose frame is not numbers"), "{words}");
    // A shape of the file put into a group (the model's shape `s1`, which the file holds, as a member of a new group).
    let words = why(&|o| {
        let first = o.index.next_id(0).unwrap();
        let held = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
        let mut inside = member(held, "s1", (4.0, 2.0), (2.0, 1.0));
        inside.kind = ShapeType::Shape;
        let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        page.shapes = ShapeList::from_vec(vec![Arc::new(group(first, (10.0, 12.0, 14.0, 14.0), vec![inside]))]);
    });
    assert!(words.contains("a shape of the file put into a new group"), "{words}");
    let _ = shape_mut;
}

/// The box of a member that is turned is its turned frame's, which the group's frame is checked against: a rectangle 4 by 2 turned a quarter, with its pin at (1, 2) in
/// the group, takes up 2 by 4 from the origin, so a group of that size is the box and is saved with no note (a new shape that is turned is written). A group
/// of the size the unturned rectangle would give, or turned the other way, is **not** the box of its member: it is saved all the same, as the member, with a note. Each
/// term of the extent is engaged (the unturned members engage the others).
#[test]
fn the_box_of_a_turned_member_is_its_turned_frames_and_the_groups_frame_is_checked_against_it() {
    let turned = |first: u32| {
        let mut m = member(first, "T", (4.0, 2.0), (1.0, 2.0));
        Arc::make_mut(&mut m.sheet).cells.insert(CellName::new("Angle"), cell(None, std::f64::consts::FRAC_PI_2));
        m
    };
    let notes = |box_: (f64, f64, f64, f64)| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let first = o.index.next_id(0).unwrap();
        push_shape(&mut o.doc, group(first + 1, box_, vec![turned(first)]));
        real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}")).notes
    };
    assert_eq!(notes((10.0, 12.0, 12.0, 16.0)), [], "the frame is the turned member's box: 2 by 4");
    let wrong = notes((10.0, 12.0, 14.0, 14.0));
    assert!(
        matches!(wrong.as_slice(), [SaveNote::GroupFrameNotKept { page: 0, .. }]),
        "the unturned rectangle's size is not the turned one's box: {wrong:?}"
    );
    // A box turned the other way is neither.
    let sideways = notes((10.0, 12.0, 14.0, 16.0));
    assert!(matches!(sideways.as_slice(), [SaveNote::GroupFrameNotKept { page: 0, .. }]), "{sideways:?}");
}

/// A group whose name the file cannot hold, or on a style sheet, is refused in words and nothing is written: each of the two bad names (the `NameU` and the `Name`
/// with a character XML forbids) and each of the three links.
#[test]
fn a_new_group_with_a_name_the_file_cannot_hold_or_a_link_to_a_style_sheet_is_refused_in_words() {
    let why = |change: &dyn Fn(&mut Shape)| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let first = o.index.next_id(0).unwrap();
        let mut g = group(
            first + 2,
            (10.0, 12.0, 18.0, 18.0),
            vec![member(first, "A", (4.0, 2.0), (2.0, 1.0)), member(first + 1, "B", (3.0, 3.0), (6.5, 4.5))],
        );
        change(&mut g);
        push_shape(&mut o.doc, g);
        match real(&o.doc, &o.bytes, &o.index) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("saved"),
        }
    };
    let bad = "a\u{1}b";
    let words = why(&|g| g.name_u = Some(Arc::from(bad)));
    assert!(words.contains("character") || words.contains("XML"), "a NameU that XML forbids: {words}");
    let words = why(&|g| g.name = Some(Arc::from(bad)));
    assert!(words.contains("character") || words.contains("XML"), "a Name that XML forbids: {words}");
    for (what, set) in [
        ("line", (|s: &mut diacraft_doc::Sheet| s.links.line = Some(3)) as fn(&mut diacraft_doc::Sheet)),
        ("fill", |s| s.links.fill = Some(3)),
        ("text", |s| s.links.text = Some(3)),
    ] {
        let words = why(&|g| set(Arc::make_mut(&mut g.sheet)));
        assert!(words.contains("a new group on a style sheet"), "the {what} link: {words}");
    }
    // The links of the root style (0) are what a new group has.
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    let mut g = pair(first, 10.0, None);
    let sheet = Arc::make_mut(&mut g.sheet);
    (sheet.links.line, sheet.links.fill, sheet.links.text) = (Some(0), Some(0), Some(0));
    push_shape(&mut o.doc, g);
    assert!(real(&o.doc, &o.bytes, &o.index).is_ok());
}

/// A connector glued to a new group names it: the group's element has `xml:id` and `draw:id` (equal), the connector's start shape is that name, and the file reads back
/// with the connector glued to the group.
#[test]
fn a_connector_glued_to_a_new_group_names_it_by_an_id_on_the_groups_element() {
    use crate::write_new::{connector, glue_part};
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    let g = pair(first, 10.0, None);
    let group_id = g.id;
    push_shape(&mut o.doc, g);
    let flow = first + 3;
    push_shape(&mut o.doc, connector(flow, (2.0, 22.0, 10.0, 15.0)));
    glue_part(&mut o.doc, flow, true, group_id, 3);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&saved.bytes, "content.xml");
    let group_tag = &content[content.find("<draw:g").unwrap()..];
    let group_tag = &group_tag[..group_tag.find('>').unwrap()];
    assert!(group_tag.contains("xml:id=\"id1\"") && group_tag.contains("draw:id=\"id1\""), "{group_tag}");
    assert!(content.contains("draw:start-shape=\"id1\""), "the connector names it: {content}");
    let back = open_bytes(saved.bytes.clone());
    let page = back.doc.pages.get(0).unwrap();
    let group = page.shapes.iter().find(|s| s.kind == ShapeType::Group).expect("the group");
    assert!(page.connects.iter().any(|c| c.to_sheet == group.id && c.to_part == Some(3)), "glued to the group: {:?}", page.connects);
}

/// What the check of the bytes made reads back is counted: a new connector's and a new line's paragraphs and the attributes written on them. A part of what a writer
/// wrote that it does not hand to the check is not checked (a mutant that left them out was not caught by any other test).
#[test]
fn the_check_of_the_bytes_made_reads_back_what_a_new_line_and_connector_wrote() {
    use crate::write_new::connector;
    use diacraft_odg::{OriginalPackage, WriteOptions, WriteWork, write_counted};
    let work_of = |o: &Open| {
        let mut work = WriteWork::default();
        write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work).unwrap();
        (work.checked_paragraphs, work.checked_attributes)
    };
    let words = |s: Shape| {
        let mut s = s;
        s.text = Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from("x\ny"))], ..Default::default() }));
        s
    };
    let on = |shape: Shape| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        push_shape(&mut o.doc, shape);
        o
    };
    let flow = || connector(5, (3.0, 20.0, 8.0, 20.0));
    let (paras, attrs) = work_of(&on(words(flow())));
    assert_eq!(paras, 2, "the two paragraphs of a connector's text are read back");
    assert!(attrs >= 4, "and its four ends and its route: {attrs}");
    let line = || {
        let mut l = connector(5, (3.0, 20.0, 8.0, 21.0));
        let sheet = Arc::make_mut(&mut l.sheet);
        for name in ["ShapeRouteStyle", "ConLineRouteExt"] {
            sheet.cells.remove(name);
        }
        sheet.sections.retain(|s| s.name.as_str() != "User");
        l
    };
    let (paras, attrs) = work_of(&on(words(line())));
    assert_eq!((paras, attrs), (2, 8), "a line's two paragraphs and its four ends (each is counted at the two looks the check takes)");
    let (paras, _) = work_of(&on(flow()));
    assert_eq!(paras, 1, "the one empty paragraph of a connector with no text is read back");
}

// ------------------------------------------------------------------------------------------------------------------ a group's own data (the document file)

const PATH: &str = "DiaCraft/document.xml";

/// The text of the document file of a package.
fn doc_file(bytes: &[u8]) -> String {
    part(bytes, PATH)
}

/// A group of two members whose box is at (`x`, 12) centimetres: the same shape as `with_a_group` makes, for a page of several.
pub(crate) fn pair(first: u32, x: f64, name: Option<&str>) -> Shape {
    let members = vec![member(first, "A", (4.0, 2.0), (2.0, 1.0)), member(first + 1, "B", (3.0, 3.0), (6.5, 4.5))];
    let mut g = group(first + 2, (x, 12.0, x + 8.0, 18.0), members);
    g.name = name.map(Arc::from);
    g.name_u = name.map(Arc::from);
    g
}

/// A group that holds a cell of its own and a section is written with them in DiaCraft's document file, under the page's name and the group's, and the file reads
/// back with the group holding them; a save of what was read is no edit, and the group's element holds none of DiaCraft's attributes.
#[test]
fn a_new_group_with_data_of_its_own_carries_it_in_the_document_file_and_reads_back_with_it() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    // The four connection points the reader gives every group: a group with none would keep an entry that says so.
    let mut g = with_side_middles(pair(first, 10.0, Some("Pair")));
    {
        let sheet = Arc::make_mut(&mut g.sheet);
        sheet.cells.insert(CellName::new("Cost"), cell(None, 3.0));
        let mut user = diacraft_doc::Section::new("User", None);
        let mut row = diacraft_doc::Row { name: Some(Arc::from("Tag")), ..diacraft_doc::Row::default() };
        row.cells.insert(CellName::new("Value"), cell(Some("Width*2"), 16.0));
        user.rows.push(Arc::new(row));
        sheet.sections.push(Arc::new(user));
    }
    push_shape(&mut o.doc, g);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let file = doc_file(&saved.bytes);
    assert!(
        file.contains("<diacraft:group page=\"page1\" name=\"Pair\">") || file.contains("<diacraft:group page=\"page1\" name=\"Pair\""),
        "{file}"
    );
    assert!(file.contains("Cost") && file.contains("Tag"), "{file}");
    assert!(!part(&saved.bytes, "content.xml").contains("diacraft:version"), "the group's element carries none of DiaCraft's attributes");
    let back = open_bytes(saved.bytes.clone());
    let read = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().clone();
    assert_eq!(read.name_u.as_deref(), Some("Pair"));
    assert_eq!(read.sheet.cell("Cost").and_then(|c| c.value.as_ref()?.number()), Some(3.0));
    let tag = read.sheet.section(&diacraft_doc::SectionKey::new("User", None)).expect("the User section");
    assert_eq!(tag.rows.len(), 1);
    assert_eq!(tag.rows[0].cells.get("Value").and_then(|c| c.formula.text()), Some("Width*2"));
    // The frame is the markup's.
    for name in ["PinX", "PinY", "Width", "Height"] {
        assert!((num(&o.doc, &[1], name) - num(&back.doc, &[1], name)).abs() < 1e-6, "{name}");
    }
    let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes, "what was read is saved as no edit");
    // The data is taken away: the entry and, with nothing else in it, the file go.
    let mut plain = back;
    let sheet = Arc::make_mut(&mut Arc::make_mut(Arc::make_mut(plain.doc.pages.get_mut(0).unwrap()).shapes.get_mut(1).unwrap()).sheet);
    sheet.cells.remove("Cost");
    sheet.sections.retain(|s| s.name.as_str() != "User");
    let cleared = real(&plain.doc, &saved.bytes, &plain.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(cleared.edited && !part_exists(&cleared.bytes, PATH), "the group's data gone, the file is");
}

/// A group another shape's formula names keeps its `ID` in the document file, and one nothing names has no entry (and no file): the rule for the `ID` of a shape
/// applies to a group.
#[test]
fn a_group_that_a_formula_names_keeps_its_id_and_one_that_nothing_names_has_no_entry() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    // The group is numbered 9 and the shape after it reads its width; the shape of the file is numbered 1.
    let mut g = pair(first, 10.0, Some("Box"));
    g.id = 9;
    let mut reader = rectangle(first + 10, "Reader", (2.0, 2.0, 3.0, 3.0));
    Arc::make_mut(&mut reader.sheet).cells.insert(CellName::new("Cost"), cell(Some("Sheet.9!Width"), 8.0));
    push_shape(&mut o.doc, g);
    push_shape(&mut o.doc, reader);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let file = doc_file(&saved.bytes);
    assert!(file.contains("<diacraft:group page=\"page1\" name=\"Box\" id=\"9\"") && file.matches("<diacraft:group").count() == 1, "{file}");
    assert!(!file.contains(" nth="), "a name that is unique is the key by itself: {file}");
    let back = open_bytes(saved.bytes.clone());
    let ids: Vec<u32> = back.doc.pages.get(0).unwrap().shapes.iter().map(|s| s.id).collect();
    assert_eq!(ids[1], 9, "the group has the ID the formula names, and the file's own shape is numbered by its place: {ids:?}");
    assert_eq!(back.doc.pages.get(0).unwrap().shapes.get(2).unwrap().sheet.cell("Cost").and_then(|c| c.formula.text()), Some("Sheet.9!Width"));
    let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
    // Nothing names it: no entry, no file (it holds the four connection points the reader gives every group: one with none has an entry that says so).
    let mut quiet = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = quiet.index.next_id(0).unwrap();
    let mut g = with_side_middles(pair(first, 10.0, Some("Box")));
    g.id = 9;
    push_shape(&mut quiet.doc, g);
    let saved = real(&quiet.doc, &quiet.bytes, &quiet.index).unwrap();
    assert!(!part_exists(&saved.bytes, PATH));
    // A group with no name that a formula names has no key an entry could be found by: its `ID` is not kept (it is numbered by its place), and nothing is refused.
    let mut unnamed = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = unnamed.index.next_id(0).unwrap();
    let mut g = pair(first, 10.0, None);
    g.id = 9;
    let mut reader = rectangle(first + 10, "Reader", (2.0, 2.0, 3.0, 3.0));
    Arc::make_mut(&mut reader.sheet).cells.insert(CellName::new("Cost"), cell(Some("Sheet.9!Width"), 8.0));
    push_shape(&mut unnamed.doc, g);
    push_shape(&mut unnamed.doc, reader);
    let saved = real(&unnamed.doc, &unnamed.bytes, &unnamed.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(!part_exists(&saved.bytes, PATH), "no entry for a group with no key and nothing but an `ID` to keep");
}

/// The review: a group's data is found by its name, which is unique among the page's groups, and not by its place among the groups of a name
/// (another program that reorders, adds or takes away groups moved the data to another group). Three groups with a name each and a cell each read back with their own, by
/// their names, and the entries have no `nth`; groups of one name, or with none, that hold data have no key and the save is refused in words, **the old key being
/// order-fragile**; the same groups with no data save.
#[test]
fn groups_with_data_are_found_by_their_unique_names_and_a_group_with_no_key_that_is_given_data_is_refused() {
    let page_of = |names: [Option<&str>; 3], data: bool| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let first = o.index.next_id(0).unwrap();
        for (k, cost) in [(0u32, 11.0), (1, 22.0), (2, 33.0)] {
            let mut g = pair(first + k * 3, 2.0 + f64::from(k) * 5.0, names[k as usize]);
            if data {
                Arc::make_mut(&mut g.sheet).cells.insert(CellName::new("Cost"), cell(None, cost));
            }
            push_shape(&mut o.doc, g);
        }
        let saved = real(&o.doc, &o.bytes, &o.index).map_err(|e| e.to_string());
        (o, saved)
    };
    let (_, saved) = page_of([Some("One"), Some("Two"), Some("Three")], true);
    let saved = saved.unwrap_or_else(|e| panic!("not saved: {e}"));
    let file = doc_file(&saved.bytes);
    assert_eq!(file.matches("<diacraft:group").count(), 3, "{file}");
    assert!(!file.contains("nth="), "a name that is unique is a key by itself: {file}");
    let back = open_bytes(saved.bytes.clone());
    let costs: Vec<Option<f64>> = (1..=3)
        .map(|at| back.doc.pages.get(0).unwrap().shapes.get(at).unwrap().sheet.cell("Cost").and_then(|c| c.value.as_ref()?.number()))
        .collect();
    assert_eq!(costs, [Some(11.0), Some(22.0), Some(33.0)]);
    let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
    // Two of one name, and one with none: no key, so no entry is made, in words.
    for names in [[Some("Twin"), Some("Twin"), Some("Other")], [Some("One"), Some("Two"), None], [None, None, None]] {
        let (_, refused) = page_of(names, true);
        let said = refused.unwrap_err();
        assert!(said.contains("a group with no name that is unique among the page's groups"), "{names:?}: {said}");
        let (_, quiet) = page_of(names, false);
        assert!(quiet.is_ok(), "{names:?}: the same groups with no data save: {quiet:?}");
    }
}

/// A group nested in a group, each with a name of its own and a cell: each reads back with its own (the key is the name, and the nesting counts for nothing).
#[test]
fn a_group_in_a_group_with_a_name_each_reads_back_with_its_own_data() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    // An inner group whose box is the member's, inside an outer group with a second member beside it.
    let inner_member = member(first, "A", (4.0, 2.0), (2.0, 1.0));
    let mut inner = group(first + 1, (0.0, 0.0, 4.0, 2.0), vec![inner_member]);
    inner.name = Some(Arc::from("Inner"));
    inner.name_u = Some(Arc::from("Inner"));
    Arc::make_mut(&mut inner.sheet).cells.insert(CellName::new("Cost"), cell(None, 5.0));
    let beside = member(first + 2, "B", (2.0, 2.0), (5.0, 1.0));
    let mut outer = group(first + 3, (10.0, 12.0, 16.0, 14.0), vec![inner, beside]);
    outer.name = Some(Arc::from("Outer"));
    outer.name_u = Some(Arc::from("Outer"));
    Arc::make_mut(&mut outer.sheet).cells.insert(CellName::new("Cost"), cell(None, 7.0));
    push_shape(&mut o.doc, outer);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let back = open_bytes(saved.bytes.clone());
    let outer = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
    let inner = outer.children.get(0).unwrap();
    assert_eq!(outer.sheet.cell("Cost").and_then(|c| c.value.as_ref()?.number()), Some(7.0));
    assert_eq!(inner.sheet.cell("Cost").and_then(|c| c.value.as_ref()?.number()), Some(5.0));
    // The same names on both: the groups have no key, and the data is refused in words.
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    let mut inner = group(first + 1, (0.0, 0.0, 4.0, 2.0), vec![member(first, "A", (4.0, 2.0), (2.0, 1.0))]);
    inner.name_u = Some(Arc::from("Twin"));
    Arc::make_mut(&mut inner.sheet).cells.insert(CellName::new("Cost"), cell(None, 5.0));
    let mut outer = group(first + 3, (10.0, 12.0, 14.0, 14.0), vec![inner]);
    outer.name_u = Some(Arc::from("Twin"));
    push_shape(&mut o.doc, outer);
    let said = real(&o.doc, &o.bytes, &o.index).map(|_| ()).unwrap_err().to_string();
    assert!(said.contains("a group with no name that is unique among the page's groups"), "{said}");
}

/// The review: a group's own text is never dropped: a new group whose text holds characters, a field or a mark is refused in words (R45 writes it as
/// one more member, which is not written yet); a text with nothing in it is no text, and the group saves.
#[test]
fn the_text_of_a_new_group_is_refused_in_words_and_a_text_with_nothing_in_it_is_none() {
    let with = |parts: Vec<diacraft_doc::TextPart>| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let first = o.index.next_id(0).unwrap();
        let mut g = pair(first, 10.0, None);
        g.text = Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() }));
        push_shape(&mut o.doc, g);
        real(&o.doc, &o.bytes, &o.index).map(|_| ()).map_err(|e| e.to_string())
    };
    let said = with(vec![diacraft_doc::TextPart::Chars(Arc::from("the group says"))]).unwrap_err();
    assert!(said.contains("a new group with text of its own"), "{said}");
    let mixed = with(vec![diacraft_doc::TextPart::Chars(Arc::from("")), diacraft_doc::TextPart::Chars(Arc::from("x"))]).unwrap_err();
    assert!(mixed.contains("a new group with text of its own"), "{mixed}");
    assert_eq!(with(vec![diacraft_doc::TextPart::Chars(Arc::from(""))]), Ok(()), "an empty run is no text");
    assert_eq!(with(Vec::new()), Ok(()), "no part is no text");
}

// ------------------------------------------------------------------------------------------------------------------: the frame against the box of the members

/// The places `(page, group)` of the notes of a save that say a group's own frame is not kept, in the order the save gave them.
pub(crate) fn unkept(notes: &[SaveNote]) -> Vec<(usize, u32)> {
    notes
        .iter()
        .filter_map(|n| match n {
            SaveNote::GroupFrameNotKept { page, shape, .. } => Some((*page, *shape)),
            _ => None,
        })
        .collect()
}

/// A number as the engine writes a frame cell: to nine places (`stencil::number_text`, which `shape.group` and the copy a save is given both use).
fn nine(v: f64) -> f64 {
    (v * 1e9).round() / 1e9
}

/// A rectangle `w` by `h` inches, turned `angle` radians about its middle, whose middle is at (`x`, `y`) inches in its group's frame: every cell at nine places, as the
/// copy of a save holds them.
fn turned_rectangle(id: u32, name: &str, (w, h): (f64, f64), angle: f64, (x, y): (f64, f64)) -> Shape {
    let mut shape = rectangle(id, name, (0.0, 0.0, 1.0, 1.0));
    for (cell_name, v) in
        [("Width", nine(w)), ("Height", nine(h)), ("LocPinX", nine(w / 2.0)), ("LocPinY", nine(h / 2.0)), ("PinX", nine(x)), ("PinY", nine(y))]
    {
        set_num(&mut shape, cell_name, v);
    }
    if angle != 0.0 {
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Angle"), cell(None, nine(angle)));
    }
    shape
}

/// The box of a rectangle `w` by `h` turned `angle` about its middle `(x, y)`: `(x0, y0, x1, y1)`.
fn box_of_turned(w: f64, h: f64, angle: f64, (x, y): (f64, f64)) -> (f64, f64, f64, f64) {
    let (s, c) = angle.sin_cos();
    let (ex, ey) = ((c.abs() * w + s.abs() * h) / 2.0, (s.abs() * w + c.abs() * h) / 2.0);
    (x - ex, y - ey, x + ex, y + ey)
}

/// A group of two members, one turned, in inches on the page, as `shape.group` makes it from the exact box and the copy of a save holds it: the frame cells at nine
/// places, the pins of the members measured from the box's lower-left corner and rounded. Gives the group and how far the members' box (worked from those cells) is from
/// the group's frame, the farthest of the four edges.
fn turned_pair(first: u32, degrees: f64) -> (Shape, f64) {
    let angle = degrees.to_radians();
    let (turned, upright) = ((2.31, 0.97, angle, (4.0, 5.0)), (1.7, 1.3, 0.0, (6.1, 6.0)));
    let (a, b) = (box_of_turned(turned.0, turned.1, turned.2, turned.3), box_of_turned(upright.0, upright.1, upright.2, upright.3));
    let (x0, y0, x1, y1) = (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3));
    let (w, h) = (nine(x1 - x0), nine(y1 - y0));
    let local = |p: (f64, f64)| (p.0 - x0, p.1 - y0);
    let members = vec![
        turned_rectangle(first, "A", (turned.0, turned.1), turned.2, local(turned.3)),
        turned_rectangle(first + 1, "B", (upright.0, upright.1), upright.2, local(upright.3)),
    ];
    let mut g = Shape::new(first + 2);
    g.kind = ShapeType::Group;
    let sheet = Arc::make_mut(&mut g.sheet);
    for (name, formula, value) in [
        ("PinX", None, nine((x0 + x1) / 2.0)),
        ("PinY", None, nine((y0 + y1) / 2.0)),
        ("Width", None, w),
        ("Height", None, h),
        ("LocPinX", Some("Width*0.5"), nine(w / 2.0)),
        ("LocPinY", Some("Height*0.5"), nine(h / 2.0)),
    ] {
        sheet.cells.insert(CellName::new(name), cell(formula, value));
    }
    // The box the members make, worked from the cells they hold now (pin, size and angle each rounded), against the group's frame.
    let from = |m: &Shape| {
        let n = |name: &str| num_of(m, name);
        box_of_turned(n("Width"), n("Height"), n("Angle"), (n("PinX"), n("PinY")))
    };
    let (p, q) = (from(&members[0]), from(&members[1]));
    let (u0, v0, u1, v1) = (p.0.min(q.0), p.1.min(q.1), p.2.max(q.2), p.3.max(q.3));
    let off = [u0.abs(), v0.abs(), (u1 - w).abs(), (v1 - h).abs()].into_iter().fold(0.0, f64::max);
    g.children = ShapeList::from_vec(members.into_iter().map(Arc::new).collect());
    (g, off)
}

/// The finding of concern 4: a new group holding a member turned by any angle but 0 was refused at save, since the writer held the group's frame to the box of
/// its members at 1E-12 while the cells it is given are at nine places, so the box worked from a turned member's rounded pin, size and angle was never the frame (the group
/// made by `shape.group` has a frame rounded to nine places too). Each angle: the box is off the frame by more than 1E-12 for the old check, and by far less than a millionth
/// of an inch, so the group is the box, is written with no note, and reads back as the model has it.
#[test]
fn a_group_with_a_turned_member_whose_cells_are_at_nine_places_is_the_box_of_its_members_and_saves_with_no_note() {
    let mut worst = 0.0f64;
    for degrees in [0.0, 30.0, 45.0, 90.0, 133.0, 359.999, 200.5, -17.25] {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let first = o.index.next_id(0).unwrap();
        let (g, off) = turned_pair(first, degrees);
        assert!(off < 1e-7, "{degrees} degrees: the rounding of nine places is a few parts in a hundred million: {off}");
        worst = worst.max(off);
        push_shape(&mut o.doc, g);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{degrees} degrees: not saved: {e}"));
        assert_eq!(saved.notes, [], "{degrees} degrees: the group is the box of its members, and the save says nothing");
        let back = open_bytes(saved.bytes.clone());
        for name in ["PinX", "PinY", "Width", "Height"] {
            let (made, read) = (num(&o.doc, &[1], name), num(&back.doc, &[1], name));
            assert!((made - read).abs() < 1e-6, "{degrees} degrees: the group's {name}: {made} against {read}");
        }
        for (at, name) in [(0, "A"), (1, "B")] {
            for cell_name in ["PinX", "PinY", "Width", "Height"] {
                let (made, read) = (num(&o.doc, &[1, at], cell_name), num(&back.doc, &[1, at], cell_name));
                assert!((made - read).abs() < 1e-6, "{degrees} degrees: {name}'s {cell_name}: {made} against {read}");
            }
        }
        let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
        assert!(!again.edited && again.bytes == saved.bytes, "{degrees} degrees: what was read is saved as no edit");
    }
    assert!(worst > 1e-12, "the frames of these groups are past what the old check allowed (1E-12), or the test shows nothing: {worst}");
}

/// The precision a group's frame is held to is a millionth of an inch, in each of the six places it is judged: the box's four edges (the members' lower-left corner against
/// the frame's origin, and their far corner against its size) and the pin's two cells. Half a millionth is the box; a millionth and a half is not, and the group is written as
/// its members with a note.
#[test]
fn a_frame_a_millionth_of_an_inch_from_the_box_is_the_box_and_one_further_is_not_in_each_of_its_six_places() {
    let build = |place: &str, by: f64| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let first = o.index.next_id(0).unwrap();
        let mut g = pair(first, 10.0, None);
        let (w, h) = (cm(8.0), cm(6.0));
        let tweak = |shape: &mut Shape, name: &str, to: f64| set_num(shape, name, to);
        let members: Vec<Arc<Shape>> = g.children.iter().cloned().collect();
        let mut members: Vec<Shape> = members.iter().map(|m| (**m).clone()).collect();
        match place {
            // The far corner of the members against the frame's size, the pin kept at the middle of the size it is given.
            "width" => {
                tweak(&mut g, "Width", w + by);
                tweak(&mut g, "LocPinX", (w + by) / 2.0);
            }
            "height" => {
                tweak(&mut g, "Height", h + by);
                tweak(&mut g, "LocPinY", (h + by) / 2.0);
            }
            // The members' lower-left corner against the frame's origin: A is the member at the corner.
            "x0" => tweak(&mut members[0], "PinX", cm(2.0) + by),
            "y0" => tweak(&mut members[0], "PinY", cm(1.0) + by),
            // The far corner by the member that reaches it (B), the frame as it was.
            "x1" => tweak(&mut members[1], "PinX", cm(6.5) - by),
            "y1" => tweak(&mut members[1], "PinY", cm(4.5) - by),
            "pin x" => tweak(&mut g, "LocPinX", w / 2.0 + by),
            "pin y" => tweak(&mut g, "LocPinY", h / 2.0 + by),
            other => panic!("{other}"),
        }
        g.children = ShapeList::from_vec(members.into_iter().map(Arc::new).collect());
        let id = g.id;
        push_shape(&mut o.doc, g);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{place} by {by}: not saved: {e}"));
        (id, unkept(&saved.notes))
    };
    for place in ["width", "height", "x0", "y0", "x1", "y1", "pin x", "pin y"] {
        for sign in [1.0, -1.0] {
            let (_, near) = build(place, sign * 0.5e-6);
            assert_eq!(near, [], "{place} by half a millionth of an inch (sign {sign}): the box");
            let (id, far) = build(place, sign * 1.5e-6);
            assert_eq!(far, [(0, id)], "{place} by a millionth and a half (sign {sign}): not the box, and one note names the group");
        }
    }
}

/// A group whose frame is not the box of its members is written as its members, with one note, **whatever the frame is**: a margin on its right and top, a
/// margin on its left and bottom (the members are measured from the frame's corner, so they are where they were on the page), and a pin set away from the middle (which
/// moves the group's frame, and the members with it, by the pin's place). The `draw:g` holds the members at the page places the group's map gives them, the same markup the
/// group that is the box gets, and the file read again has the box of the members, with the pin at its middle.
#[test]
fn a_new_group_whose_frame_is_not_the_box_of_its_members_is_written_as_its_members_with_one_note() {
    let on_the_page = |build: &dyn Fn(u32) -> Shape| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let first = o.index.next_id(0).unwrap();
        let g = build(first);
        let id = g.id;
        push_shape(&mut o.doc, g);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
        (o, id, saved)
    };
    let members =
        |first: u32, by: f64| vec![member(first, "A", (4.0, 2.0), (2.0 + by, 1.0 + by)), member(first + 1, "B", (3.0, 3.0), (6.5 + by, 4.5 + by))];
    // The group that is the box: the markup every other group here is written as.
    let (_, _, whole) = on_the_page(&|first| group(first + 2, (10.0, 12.0, 18.0, 18.0), members(first, 0.0)));
    assert_eq!(whole.notes, [], "the box is no note");
    let markup = element(&part(&whole.bytes, "content.xml"), "g");
    for (what, build) in [
        (
            "a margin on the right and top",
            Box::new(|first: u32| group(first + 2, (10.0, 12.0, 20.0, 19.0), members(first, 0.0))) as Box<dyn Fn(u32) -> Shape>,
        ),
        ("a margin on the left and bottom", Box::new(|first: u32| group(first + 2, (8.0, 10.0, 18.0, 18.0), members(first, 2.0)))),
        ("a margin all round", Box::new(|first: u32| group(first + 2, (9.0, 11.0, 19.0, 19.0), members(first, 1.0)))),
        ("a frame narrower than its members", Box::new(|first: u32| group(first + 2, (10.0, 12.0, 14.0, 18.0), members(first, 0.0)))),
    ] {
        let (o, id, saved) = on_the_page(&*build);
        assert_eq!(unkept(&saved.notes), [(0, id)], "{what}: one note, naming the page and the group");
        assert_eq!(saved.notes.len(), 1, "{what}: and nothing else is said: {:?}", saved.notes);
        assert_eq!(
            element(&part(&saved.bytes, "content.xml"), "g"),
            markup,
            "{what}: the members are written where they were on the page, as for the group that is the box"
        );
        let back = open_bytes(saved.bytes.clone());
        for (name, expected) in
            [("PinX", cm(14.0)), ("PinY", cm(15.0)), ("Width", cm(8.0)), ("Height", cm(6.0)), ("LocPinX", cm(4.0)), ("LocPinY", cm(3.0))]
        {
            let read = num(&back.doc, &[1], name);
            assert!((read - expected).abs() < 1e-6, "{what}: the reopened group's {name} is {read} and the box's {expected}");
        }
        let _ = o;
        let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
        assert!(!again.edited && again.bytes == saved.bytes && again.notes.is_empty(), "{what}: what was read is saved as no edit, and says nothing");
    }
    // A pin set away from the middle: the group's map moves its members by the pin, which the cells say, so they are written where the cells put them.
    let (_, id, pinned) = on_the_page(&|first| {
        let mut g = group(first + 2, (10.0, 12.0, 18.0, 18.0), members(first, 0.0));
        set_num(&mut g, "LocPinX", cm(1.0));
        g
    });
    assert_eq!(unkept(&pinned.notes), [(0, id)], "a pin set away from the middle of its box");
    let written = element(&part(&pinned.bytes, "content.xml"), "g");
    assert!(written.contains(r#"svg:width="4cm" svg:height="2cm" svg:x="13cm" svg:y="15.7cm""#), "A is where the group's cells put it: {written}");
    let back = open_bytes(pinned.bytes.clone());
    for (name, expected) in [("PinX", cm(17.0)), ("Width", cm(8.0))] {
        let read = num(&back.doc, &[1], name);
        assert!((read - expected).abs() < 1e-6, "the reopened group's {name} is {read} and the box's {expected}");
    }
}

/// Each new group that is not the box of its members has its own note, in the order the page holds them, and the page it is on: two on one page, and one on the second page.
#[test]
fn each_group_that_is_not_the_box_of_its_members_has_a_note_naming_it_and_its_page() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")).page(&custom("s2", "1cm")));
    let (first, second) = (o.index.next_id(0).unwrap(), o.index.next_id(1).unwrap());
    let wide = |first: u32, x: f64| {
        let members = vec![member(first, "A", (4.0, 2.0), (2.0, 1.0)), member(first + 1, "B", (3.0, 3.0), (6.5, 4.5))];
        group(first + 2, (x, 12.0, x + 9.0, 18.0), members)
    };
    // The first page: a margin, the box, a margin (the groups are numbered first + 2, first + 5 and first + 8). The second page: one margin.
    push_shape(&mut o.doc, wide(first, 10.0));
    push_shape(&mut o.doc, pair(first + 3, 10.0, None));
    push_shape(&mut o.doc, wide(first + 6, 2.0));
    Arc::make_mut(o.doc.pages.get_mut(1).unwrap()).shapes.push(Arc::new(wide(second, 4.0)));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert_eq!(
        unkept(&saved.notes),
        [(0, first + 2), (0, first + 8), (1, second + 2)],
        "the two margins of the first page, in its order, and not the box between them; then the second page's: {:?}",
        saved.notes
    );
}

/// A group in a group is judged by the leaves: the reader makes each group's frame from its members, so the frame of a group round another is the box of the other's
/// members, and **not of the other's own frame**, which the file does not keep. (a) The inner group has a margin and the outer is the box of the leaves: one note, for the
/// inner group. (b) The outer's frame is the box of the inner's frame, margin and all, and so is not the box of the leaves: a note for each, the outer first. (c) A chain of
/// groups, each with a margin, has a note for each; and **a member that is a group turned is refused** in the words of its turn, whatever the group round it is.
#[test]
fn a_group_in_a_group_is_judged_by_the_box_of_the_leaves_and_not_by_the_frame_of_the_group_inside_it() {
    let nested = |outer_box: (f64, f64, f64, f64)| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let first = o.index.next_id(0).unwrap();
        // The inner group: a frame 5 by 3 round a member 4 by 2 at the frame's corner; beside it in the outer a member 2 by 2 that reaches (6, 2).
        let inner = group(first + 1, (0.0, 0.0, 5.0, 3.0), vec![member(first, "A", (4.0, 2.0), (2.0, 1.0))]);
        let inner_id = inner.id;
        let outer = group(first + 3, outer_box, vec![inner, member(first + 2, "B", (2.0, 2.0), (5.0, 1.0))]);
        let outer_id = outer.id;
        push_shape(&mut o.doc, outer);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
        (inner_id, outer_id, saved)
    };
    // (a) The leaves' box is 6 by 2: the outer is that, the inner is not its member's box.
    let (inner, outer, saved) = nested((10.0, 12.0, 16.0, 14.0));
    assert_eq!(unkept(&saved.notes), [(0, inner)], "only the inner group's frame is not kept ({outer} is the box of the leaves): {:?}", saved.notes);
    let back = open_bytes(saved.bytes.clone());
    let inner_read = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().children.get(0).unwrap().clone();
    assert_eq!((inner_read.kind.clone(), inner_read.children.len()), (ShapeType::Group, 1));
    let read = |name: &str| inner_read.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN);
    assert!(
        (read("Width") - cm(4.0)).abs() < 1e-6 && (read("Height") - cm(2.0)).abs() < 1e-6,
        "the inner group is read back as the box of its member: {} by {}",
        read("Width"),
        read("Height")
    );
    // (b) The outer's frame is the box of the inner's frame and the beside member (6 by 3): not the box of the leaves.
    let (inner, outer, saved) = nested((10.0, 12.0, 16.0, 15.0));
    assert_eq!(unkept(&saved.notes), [(0, outer), (0, inner)], "a note for each, the outer first: {:?}", saved.notes);
    // (c) A chain of five groups, each with a margin of 1 on the right: a note for each, outermost first.
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    let mut chain = member(first, "Deep", (4.0, 2.0), (2.0, 1.0));
    let mut ids = Vec::new();
    for level in 0..5u32 {
        let g = group(first + 1 + level, (0.0, 0.0, 4.0 + f64::from(level + 1), 2.0), vec![chain]);
        ids.push(g.id);
        chain = g;
    }
    push_shape(&mut o.doc, chain);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    ids.reverse();
    assert_eq!(unkept(&saved.notes), ids.iter().map(|id| (0, *id)).collect::<Vec<_>>(), "{:?}", saved.notes);
    // A member group that is turned.
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    let mut inner = group(first + 1, (0.0, 0.0, 4.0, 2.0), vec![member(first, "A", (4.0, 2.0), (2.0, 1.0))]);
    Arc::make_mut(&mut inner.sheet).cells.insert(CellName::new("Angle"), cell(None, 0.5));
    push_shape(&mut o.doc, group(first + 2, (10.0, 12.0, 14.0, 14.0), vec![inner]));
    let words = real(&o.doc, &o.bytes, &o.index).map(|_| ()).unwrap_err().to_string();
    assert!(words.contains("a group that is turned or flipped"), "{words}");
}

/// An inner group that is not at its outer group's corner is carried into the outer's frame by its own place (its pin less its local pin, in x and in y), and the box of the
/// leaves is the box of that: the inner's frame is 4 by 2 at (2, 1) in the outer's frame round a member of the same size, and a member 2 by 2 sits at the outer's corner, so the
/// leaves take up (0, 0) to (6, 3). An outer frame of that size is the box (no note), and one of 6 by 2 or of 4 by 3 is not.
#[test]
fn an_inner_group_away_from_the_outers_corner_is_carried_into_its_frame_by_its_own_place() {
    let notes = |outer_box: (f64, f64, f64, f64)| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let first = o.index.next_id(0).unwrap();
        let inner = group(first + 1, (2.0, 1.0, 6.0, 3.0), vec![member(first, "A", (4.0, 2.0), (2.0, 1.0))]);
        let outer = group(first + 3, outer_box, vec![inner, member(first + 2, "B", (2.0, 2.0), (1.0, 1.0))]);
        let id = outer.id;
        push_shape(&mut o.doc, outer);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
        (id, unkept(&saved.notes))
    };
    assert_eq!(notes((10.0, 12.0, 16.0, 15.0)).1, [], "the box of the leaves is 6 by 3");
    let (id, wrong) = notes((10.0, 12.0, 16.0, 14.0));
    assert_eq!(wrong, [(0, id)], "6 by 2 is not that box");
    let (id, wrong) = notes((10.0, 12.0, 14.0, 15.0));
    assert_eq!(wrong, [(0, id)], "4 by 3 is not that box");
}

/// What the pass that finds each group's box costs, counted and never timed: one step for each new shape of the page, whatever the groups hold and however deep they are
/// nested, and none for a page whose new shapes are in no group.
#[test]
fn the_pass_that_finds_the_boxes_of_the_groups_takes_a_step_for_each_new_shape() {
    use diacraft_odg::{OriginalPackage, WriteOptions, WriteWork, write_counted};
    let steps = |groups: usize, deep: usize| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let first = o.index.next_id(0).unwrap();
        let mut next = first;
        let mut made = 0usize;
        for _ in 0..groups {
            if deep == 0 {
                // A new shape in no group.
                push_shape(&mut o.doc, rectangle(next, "plain", (2.0, 2.0, 3.0, 3.0)));
                next += 1;
                continue;
            }
            let mut chain = member(next, "M", (4.0, 2.0), (2.0, 1.0));
            next += 1;
            made += 1;
            for _ in 0..deep {
                chain = group(next, (0.0, 0.0, 4.0, 2.0), vec![chain]);
                next += 1;
                made += 1;
            }
            push_shape(&mut o.doc, chain);
        }
        let mut work = WriteWork::default();
        write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work)
            .unwrap_or_else(|e| panic!("{groups} chains of {deep}: not saved: {e}"));
        (work.group_boxes, made)
    };
    for (groups, deep) in [(1, 1), (10, 1), (10, 3), (100, 1), (7, 40)] {
        let (counted, made) = steps(groups, deep);
        assert_eq!(counted, made as u64, "{groups} chains of {deep} groups: a step for each of the {made} new shapes");
    }
    assert_eq!(steps(25, 0).0, 0, "new shapes in no group are not walked");
}

/// The words of the note and the class a harness compares.
#[test]
fn the_note_that_a_groups_frame_is_not_kept_says_which_group_on_which_page_and_why() {
    let note = SaveNote::GroupFrameNotKept { page: 2, shape: 7, glued: Vec::new(), glued_total: 0 };
    assert_eq!(note.class(), "group_frame_not_kept");
    let words = note.to_string();
    assert!(words.contains("group 7 on page 2") && words.contains("not kept"), "{words}");
    assert!(words.contains("not the box of the group's members") && words.contains("no place for a group's frame"), "{words}");
    assert!(!words.contains("glued"), "no connector is glued to it: {words}");
    // A connector glued to the group itself is named, and so are several, and the rest are counted.
    let one = SaveNote::GroupFrameNotKept { page: 0, shape: 7, glued: vec![12], glued_total: 1 }.to_string();
    assert!(one.contains("the connector glued to the group itself (12) keeps its end where it was in the file"), "{one}");
    let some = SaveNote::GroupFrameNotKept { page: 0, shape: 7, glued: vec![12, 15], glued_total: 2 }.to_string();
    assert!(some.contains("the connectors glued to the group itself (12, 15) keep their ends where they were in the file"), "{some}");
    let many = SaveNote::GroupFrameNotKept { page: 0, shape: 7, glued: vec![1, 2, 3, 4, 5, 6, 7, 8], glued_total: 11 }.to_string();
    assert!(many.contains("(1, 2, 3, 4, 5, 6, 7, 8 and 3 more)"), "{many}");
}
