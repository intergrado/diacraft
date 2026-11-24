//! Where a new group goes on a page: after a shape of the file, on a page whose shapes carry `draw:z-index` (the group has a value and its members none), in
//! the same save as a reorder, and the group of the file moved and deleted by a second save. Each is read back and saved again as no edit.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Shape, ShapeList, ShapeType};

use crate::drawing::{Drawing, cm};
use crate::write_core::{custom, move_by, open, open_bytes, part, real, set_num};
use crate::write_new::{push_shape, rectangle};

fn cell(formula: Option<&str>, value: f64) -> Cell {
    Cell::read(formula, Some(&value.to_string()), Some("IN"), None, &mut 1_000)
}

fn member(id: u32, name: &str, size: (f64, f64), pin: (f64, f64)) -> Shape {
    let mut shape = rectangle(id, name, (0.0, 0.0, size.0, size.1));
    set_num(&mut shape, "PinX", cm(pin.0));
    set_num(&mut shape, "PinY", cm(pin.1));
    shape
}

/// A group of two members whose box is 8 by 6 centimetres, its lower-left corner at (`x`, 12).
fn pair(first: u32, x: f64) -> Shape {
    let members = vec![member(first, "A", (4.0, 2.0), (2.0, 1.0)), member(first + 1, "B", (3.0, 3.0), (6.5, 4.5))];
    let (w, h) = (8.0, 6.0);
    let mut shape = Shape::new(first + 2);
    shape.kind = ShapeType::Group;
    let sheet = Arc::make_mut(&mut shape.sheet);
    for (name, formula, value) in [
        ("PinX", None, cm(x + w / 2.0)),
        ("PinY", None, cm(12.0 + h / 2.0)),
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

fn top_names(doc: &Document) -> Vec<String> {
    doc.pages.get(0).unwrap().shapes.iter().map(|s| s.name_u.as_deref().unwrap_or("[group]").to_owned()).collect()
}

/// A page whose shapes carry `draw:z-index`: the group is written with the value of the shape before it (as a new shape of the top level is), its members with none,
/// and the order read back is the model's.
#[test]
fn a_new_group_on_a_page_that_carries_z_index_has_a_value_and_its_members_have_none() {
    let shape = |name: &str, z: i64, x: u32| {
        format!(
            "<draw:custom-shape draw:name=\"{name}\" draw:z-index=\"{z}\" svg:x=\"{x}cm\" svg:y=\"3cm\" svg:width=\"1cm\" svg:height=\"1cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>"
        )
    };
    let mut o = open(&Drawing::a4().page(&format!("{}{}", shape("a", 3, 1), shape("b", 7, 2))));
    let first = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, pair(first, 10.0));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&saved.bytes, "content.xml");
    let at = content.find("<draw:g").expect("a group");
    let written = &content[at..content[at..].find("</draw:g>").map_or(content.len(), |e| at + e)];
    assert!(written.starts_with("<draw:g draw:z-index=\"7\""), "the value of `b`, which it follows: {written}");
    assert_eq!(written.matches("draw:z-index").count(), 1, "its members have none: {written}");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(top_names(&back.doc), ["a", "b", "[group]"]);
    assert_eq!(back.doc.pages.get(0).unwrap().shapes.get(2).unwrap().children.len(), 2);
    let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
    // The `draw:z-index` the group is written with is among what the check of the bytes made reads back: one attribute more than the same group on a page with none, and none of the
    // members'.
    let checked = |body: &str| {
        let mut o = open(&Drawing::a4().page(body));
        let first = o.index.next_id(0).unwrap();
        push_shape(&mut o.doc, pair(first, 10.0));
        let mut work = diacraft_odg::WriteWork::default();
        let options = diacraft_odg::WriteOptions::default();
        diacraft_odg::write_counted(&o.doc, Some(&diacraft_odg::OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work).unwrap();
        work.checked_attributes
    };
    let plain = |name: &str, x: u32| {
        format!(
            "<draw:custom-shape draw:name=\"{name}\" svg:x=\"{x}cm\" svg:y=\"3cm\" svg:width=\"1cm\" svg:height=\"1cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>"
        )
    };
    let with_z = checked(&format!("{}{}", shape("a", 3, 1), shape("b", 7, 2)));
    let without = checked(&format!("{}{}", plain("a", 1), plain("b", 2)));
    assert_eq!(with_z, without + 1, "the group's value is read back: {with_z} against {without}");
}

/// A new group and a reorder in one save: the shapes that move and the group are put in the model's order, the group's members with it, and what is read back is the
/// model's order, the group whole.
#[test]
fn a_new_group_in_the_same_save_as_a_reorder_is_put_in_the_models_order_with_its_members() {
    let body: String = (1..=4).map(|k| custom(&format!("s{k}"), &format!("{k}cm"))).collect();
    let mut o = open(&Drawing::a4().page(&body));
    let first = o.index.next_id(0).unwrap();
    let old: Vec<Arc<Shape>> = o.doc.pages.get(0).unwrap().shapes.iter().cloned().collect();
    let by = |name: &str| Arc::clone(old.iter().find(|s| s.name_u.as_deref() == Some(name)).unwrap());
    // `s1` last, `s4` first, and a new group between the two that stay.
    let order = vec![by("s4"), by("s2"), Arc::new(pair(first, 10.0)), by("s3"), by("s1")];
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes = ShapeList::from_vec(order);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(top_names(&back.doc), ["s4", "s2", "[group]", "s3", "s1"]);
    assert_eq!(back.doc.pages.get(0).unwrap().shapes.get(2).unwrap().children.len(), 2);
    let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
}

/// A group of the file, read again, is moved (its members follow it on the page), then given a cell, then deleted: each is one save, each is read back, and the one after
/// each is no edit.
#[test]
fn a_saved_group_is_moved_given_data_and_deleted_by_the_saves_after_it() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    // The group that is given data below is found by its name, which is unique on the page.
    let mut named = pair(first, 10.0);
    named.name = Some(Arc::from("Box"));
    named.name_u = named.name.clone();
    push_shape(&mut o.doc, named);
    let one = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let mut back = open_bytes(one.bytes.clone());
    let id = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().id;
    move_by(&mut back.doc, 0, id, 2.0, -1.0);
    let two = real(&back.doc, &one.bytes, &back.index).unwrap_or_else(|e| panic!("a group moved was not saved: {e}"));
    assert!(two.edited);
    let moved = open_bytes(two.bytes.clone());
    let members: Vec<f64> = moved
        .doc
        .pages
        .get(0)
        .unwrap()
        .shapes
        .get(1)
        .unwrap()
        .children
        .iter()
        .map(|m| m.sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap())
        .collect();
    assert!(
        (members[0] - cm(2.0)).abs() < 1e-6 && (members[1] - cm(6.5)).abs() < 1e-6,
        "the members keep their places in the group's frame: {members:?}"
    );
    let written = part(&two.bytes, "content.xml");
    assert!(written.contains(r#"svg:x="12cm""#) && written.contains(r#"svg:x="17cm""#), "and are 2 centimetres further right on the page: {written}");
    assert!(!real(&moved.doc, &two.bytes, &moved.index).unwrap().edited);
    // Data on it.
    let mut again = moved;
    let id = again.doc.pages.get(0).unwrap().shapes.get(1).unwrap().id;
    let page = Arc::make_mut(again.doc.pages.get_mut(0).unwrap());
    let group = Arc::make_mut(page.shapes.get_mut(1).unwrap());
    Arc::make_mut(&mut group.sheet).cells.insert(CellName::new("Cost"), cell(None, 3.0));
    let three = real(&again.doc, &two.bytes, &again.index).unwrap_or_else(|e| panic!("data on a group was not saved: {e}"));
    assert!(three.edited && part(&three.bytes, "DiaCraft/document.xml").contains("Cost"));
    // Deleted: the group goes with its members, and the file with the entry.
    let mut last = open_bytes(three.bytes.clone());
    Arc::make_mut(last.doc.pages.get_mut(0).unwrap()).shapes =
        ShapeList::from_vec(vec![Arc::clone(last.doc.pages.get(0).unwrap().shapes.get(0).unwrap())]);
    let four = real(&last.doc, &three.bytes, &last.index).unwrap_or_else(|e| panic!("a deleted group was not saved: {e}"));
    assert!(four.edited);
    assert!(!part(&four.bytes, "content.xml").contains("<draw:g"), "the group is gone");
    assert_eq!(open_bytes(four.bytes.clone()).doc.pages.get(0).unwrap().shapes.len(), 1);
    let _ = id;
}

/// A group of the file that has no name and is given data is refused in words (its entry would be found by its place, which another program can change), and the same
/// group with a name is saved; nothing is written by the refusal.
#[test]
fn a_group_of_the_file_with_no_name_that_is_given_data_is_refused_in_words() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, pair(first, 10.0));
    let one = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let mut again = open_bytes(one.bytes.clone());
    let page = Arc::make_mut(again.doc.pages.get_mut(0).unwrap());
    let group = Arc::make_mut(page.shapes.get_mut(1).unwrap());
    Arc::make_mut(&mut group.sheet).cells.insert(CellName::new("Cost"), cell(None, 3.0));
    let said = real(&again.doc, &one.bytes, &again.index).map(|_| ()).unwrap_err().to_string();
    assert!(said.contains("a group with no name that is unique among the page's groups"), "{said}");
}

/// The review: an angle or a flip set on a group of the file is refused in words, each of the three cells alone, and one that is the same number is
/// not a change.
#[test]
fn an_angle_or_a_flip_set_on_a_group_of_the_file_is_refused_in_words() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, pair(first, 10.0));
    let one = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let set = |cell_name: &str, value: f64| {
        let mut again = open_bytes(one.bytes.clone());
        let page = Arc::make_mut(again.doc.pages.get_mut(0).unwrap());
        let group = Arc::make_mut(page.shapes.get_mut(1).unwrap());
        Arc::make_mut(&mut group.sheet).cells.insert(CellName::new(cell_name), cell(None, value));
        real(&again.doc, &one.bytes, &again.index).map_err(|e| e.to_string())
    };
    for name in ["Angle", "FlipX", "FlipY"] {
        let said = set(name, 1.0).map(|_| ()).unwrap_err();
        assert!(said.contains("a group that is turned or flipped"), "{name}: {said}");
    }
    // The same number is no change: a cell of 0 that was not there, and one a rounding apart.
    assert!(!set("Angle", 0.0).unwrap().edited, "an angle of 0 is the angle there was");
    assert!(!set("FlipX", 0.0).unwrap().edited);
    assert!(!set("Angle", 1e-12).unwrap().edited, "a rounding is no change");
    assert!(set("Angle", 1e-9).is_ok(), "the least that is no change to the writer: 1e-9 from the angle there was is not refused");
    assert!(set("Angle", 1e-8).is_err(), "and ten times that is one");
}

/// The survivors of the mutation run of the fix round: a turn a group already holds (its entry in the document file says so) is not a change, so a move of the group is
/// saved; a flip or an angle that is a rounding (1e-9, the least that is a change is more) is none either; and a text with nothing in it set on a group of the file is
/// refused as a text edit of the group is (the retext of a `draw:g` is not written).
#[test]
fn a_group_that_holds_a_turn_already_is_moved_and_an_empty_text_on_a_group_of_the_file_is_refused() {
    let mut drawing = Drawing::a4()
        .page(r#"<draw:g draw:name="Box"><draw:rect svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><text:p/></draw:rect></draw:g>"#);
    let entry = r#"<diacraft:group page="page1" name="Box"><diacraft:sheet><diacraft:Cell N="Angle" V="0.5"/></diacraft:sheet></diacraft:group>"#;
    drawing.files.push(("DiaCraft/document.xml".to_owned(), crate::docfile_cost::file_of(entry).into_bytes()));
    let bytes = drawing.bytes();
    let mut o = open_bytes(bytes.clone());
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    assert_eq!(
        o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("Angle").and_then(|c| c.value.as_ref()?.number()),
        Some(0.5),
        "the entry gives the turn"
    );
    move_by(&mut o.doc, 0, id, 1.0, 0.0);
    let saved = real(&o.doc, &bytes, &o.index).unwrap_or_else(|e| panic!("a group that holds a turn and is moved was not saved: {e}"));
    assert!(saved.edited);
    // The turn it holds is the same number: a change of 1e-9 is none, and one of 1e-8 is.
    let mut same = open_bytes(bytes.clone());
    let page = Arc::make_mut(same.doc.pages.get_mut(0).unwrap());
    let group = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    Arc::make_mut(&mut group.sheet).cells.insert(CellName::new("Angle"), cell(None, 0.5 + 1e-9));
    assert!(!real(&same.doc, &bytes, &same.index).unwrap().edited, "a rounding of 1e-9 is no change");
    let page = Arc::make_mut(same.doc.pages.get_mut(0).unwrap());
    let group = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    Arc::make_mut(&mut group.sheet).cells.insert(CellName::new("Angle"), cell(None, 0.5 + 1e-8));
    assert!(real(&same.doc, &bytes, &same.index).is_err(), "a change of 1e-8 is one");
    // A text with something in it is refused as a text of the group; an empty run and no part are no text.
    for (parts, refused) in [
        (vec![diacraft_doc::TextPart::Chars(Arc::from(""))], false),
        (Vec::new(), false),
        (vec![diacraft_doc::TextPart::Chars(Arc::from("x"))], true),
    ] {
        let mut o = open_bytes(bytes.clone());
        let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        Arc::make_mut(page.shapes.get_mut(0).unwrap()).text = Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() }));
        let said = real(&o.doc, &bytes, &o.index).map(|_| ()).map_err(|e| e.to_string());
        assert_eq!(said.is_err(), refused, "{said:?}");
    }
}

/// The review, on a group of the file, and the mutation run's O2: the guard that refuses a text on a group of the file **in the words of the group's
/// text**. A group of the file has no paragraph, so a text with characters or a field is refused before the guard, by the check of the paragraphs (in other words); a text
/// that holds only a marker (a character format on no characters) gets past that check and is no change of its own, so it is the guard that refuses it, alone and beside
/// another edit (the group moved). With the guard gone the marker was dropped and the move saved. A text with nothing in it is no text.
#[test]
fn a_marker_text_on_a_group_of_the_file_is_refused_in_the_words_of_the_groups_text_alone_and_beside_another_edit() {
    use diacraft_doc::TextPart;
    let bytes = Drawing::a4()
        .page(r#"<draw:g draw:name="Box"><draw:rect svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><text:p/></draw:rect></draw:g>"#)
        .bytes();
    let with = |parts: Vec<TextPart>, moved: bool| {
        let mut o = open_bytes(bytes.clone());
        let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
        let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        Arc::make_mut(page.shapes.get_mut(0).unwrap()).text = Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() }));
        if moved {
            move_by(&mut o.doc, 0, id, 1.0, 0.0);
        }
        real(&o.doc, &bytes, &o.index).map(|w| w.edited).map_err(|e| e.to_string())
    };
    for parts in [vec![TextPart::Cp(0)], vec![TextPart::Chars(Arc::from("")), TextPart::Pp(0)], vec![TextPart::Cp(0), TextPart::Chars(Arc::from(""))]]
    {
        for moved in [false, true] {
            let said = with(parts.clone(), moved).expect_err(&format!("{parts:?} (moved: {moved}) was saved"));
            assert!(said.contains("a text on a group of the file"), "{parts:?} (moved: {moved}): {said}");
        }
    }
    // A text with nothing in it is no text: nothing to save, and the move alone is saved.
    assert_eq!(with(Vec::new(), false), Ok(false));
    assert_eq!(with(vec![TextPart::Chars(Arc::from(""))], false), Ok(false));
    assert_eq!(with(Vec::new(), true), Ok(true));
    assert_eq!(with(vec![TextPart::Chars(Arc::from(""))], true), Ok(true));
}
