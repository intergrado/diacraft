//! The text of a new line or connector (a Data Flow's label) is written as the `text:p` the element holds and read back, and a connector of the file whose
//! ends moved has its route written where it went (`svg:d` is in the page's coordinates, so a move of the connector itself, which leaves its route in its own frame
//! as it was, is a new path).

use std::sync::Arc;

use diacraft_doc::{CellName, Shape, Text, TextPart};

use crate::drawing::Drawing;
use crate::write_core::{custom, num_of, open, open_bytes, part, real, set_num, shape_mut};
use crate::write_new::{connector, push_shape};

/// The text of a shape as one string.
fn words(shape: &Shape) -> String {
    shape
        .text
        .as_ref()
        .map(|t| t.parts.iter().filter_map(|p| if let TextPart::Chars(c) = p { Some(&**c) } else { None }).collect::<Vec<_>>().join(""))
        .unwrap_or_default()
}

fn with_text(mut shape: Shape, text: &str) -> Shape {
    shape.text = Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from(text))], ..Default::default() }));
    shape
}

/// A line: a connector of the helper without the cells that make it one (`ShapeRouteStyle`, `ConLineRouteExt`, the marker row).
fn line(id: u32, ends: (f64, f64, f64, f64)) -> Shape {
    let mut shape = connector(id, ends);
    let sheet = Arc::make_mut(&mut shape.sheet);
    for name in ["ShapeRouteStyle", "ConLineRouteExt"] {
        sheet.cells.remove(name);
    }
    sheet.sections.retain(|s| s.name.as_str() != "User");
    shape.name = Some(Arc::from("Line.1"));
    shape.name_u = Some(Arc::from("Line.1"));
    shape
}

fn element(content: &str, tag: &str) -> String {
    let at = content.find(&format!("<draw:{tag} ")).unwrap_or_else(|| panic!("no {tag}: {content}"));
    let end = content[at..].find(&format!("</draw:{tag}>")).map_or(content.len(), |e| at + e);
    content[at..end].to_owned()
}

/// A new connector with a text holds it as `text:p`, reads back with it, carries nothing of DiaCraft's namespace for it, and a save of what was read is no edit.
#[test]
fn the_text_of_a_new_connector_is_written_as_text_p_and_read_back_with_no_tier_2() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, with_text(connector(id, (3.0, 20.0, 8.0, 20.0)), "Order data"));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&saved.bytes, "content.xml");
    let written = element(&content, "connector");
    assert!(written.contains("<text:p>Order data</text:p>"), "{written}");
    assert!(!content.contains("diacraft:text"), "the text is the file's own: {content}");
    let back = open_bytes(saved.bytes.clone());
    let shape = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
    assert_eq!(words(shape), "Order data");
    let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes, "what was read is saved as no edit");
}

/// A new line with a text, and one with several paragraphs, likewise.
#[test]
fn the_text_of_a_new_line_is_written_as_text_p_one_paragraph_each_and_read_back() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, with_text(line(id, (3.0, 20.0, 8.0, 21.0)), "one\ntwo"));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let written = element(&part(&saved.bytes, "content.xml"), "line");
    assert!(written.contains("<text:p>one</text:p><text:p>two</text:p>"), "{written}");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(words(back.doc.pages.get(0).unwrap().shapes.get(1).unwrap()), "one\ntwo");
    let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
}

/// A new line and a new connector with no text keep the empty paragraph they were written with.
#[test]
fn a_new_line_and_a_new_connector_with_no_text_keep_their_empty_paragraph() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, line(first, (3.0, 20.0, 8.0, 21.0)));
    push_shape(&mut o.doc, connector(first + 1, (3.0, 10.0, 8.0, 10.0)));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&saved.bytes, "content.xml");
    assert!(element(&content, "line").contains("<text:p/>") && element(&content, "connector").contains("<text:p/>"), "{content}");
}

/// A connector of the file that is moved (its ends and its pin change, its own route does not) has `svg:d` written at the new place; it was left where it had
/// been, and the file drew the connector there.
#[test]
fn a_connector_of_the_file_that_is_moved_has_its_route_written_where_it_went() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, connector(id, (3.0, 20.0, 8.0, 20.0)));
    let first = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let mut back = open_bytes(first.bytes.clone());
    let route = |content: &str| {
        let written = element(content, "connector");
        let d = written.split("svg:d=\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or_default().to_owned();
        let x1 = written.split("svg:x1=\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or_default().to_owned();
        (d, x1)
    };
    let (before_d, before_x1) = route(&part(&first.bytes, "content.xml"));
    assert!(before_d.starts_with("M 3000 ") && before_x1 == "3cm", "{before_d} {before_x1}");
    // Moved 2 cm right: the ends and the pin, which is what `shape.move` does to a connector that is not glued.
    let shape = shape_mut(&mut back.doc, 0, 2);
    for cell in ["BeginX", "EndX", "PinX"] {
        let at = num_of(shape, cell);
        set_num(shape, cell, at + 2.0 / 2.54);
    }
    assert!(shape.sheet.cell("Geometry").is_none() && shape.sheet.cells.contains_key(&CellName::new("BeginX")));
    let moved = real(&back.doc, &first.bytes, &back.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let (after_d, after_x1) = route(&part(&moved.bytes, "content.xml"));
    assert_eq!(after_x1, "5cm");
    assert!(after_d.starts_with("M 5000 "), "the route is where the connector went: {after_d}");
    // And what is read of it is where it went.
    let again = open_bytes(moved.bytes.clone());
    let shape = again.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
    assert!((num_of(shape, "BeginX") - 5.0 / 2.54).abs() < 1e-6, "{}", num_of(shape, "BeginX"));
    let second = real(&again.doc, &moved.bytes, &again.index).unwrap();
    assert!(!second.edited && second.bytes == moved.bytes);
}

/// A connector of the file whose own route changes (a point of its path moved) and whose ends do not is written again too: the route is rewritten when the route is
/// what changed, as when the ends are (a mutant of the condition survived a test of the ends alone).
#[test]
fn a_connector_of_the_file_whose_route_alone_changed_has_its_route_written_again() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, connector(id, (3.0, 20.0, 8.0, 20.0)));
    let first = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let mut back = open_bytes(first.bytes.clone());
    let d_of = |bytes: &[u8]| {
        let content = part(bytes, "content.xml");
        let written = element(&content, "connector");
        written.split("svg:d=\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or_default().to_owned()
    };
    // The second point of the path, a `LineTo` along the connector's frame, is moved 1 centimetre further: its ends are as they were.
    let shape = shape_mut(&mut back.doc, 0, 2);
    let sheet = Arc::make_mut(&mut shape.sheet);
    let geometry = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Geometry").map(Arc::make_mut).expect("the route");
    let row = geometry.rows.iter_mut().map(Arc::make_mut).find(|r| r.row_type.as_deref() == Some("LineTo")).expect("a LineTo");
    let x = row.cells.get_mut("X").expect("its X");
    x.value = diacraft_doc::Value::from_number(x.value.as_ref().and_then(diacraft_doc::Value::number).unwrap() + 1.0 / 2.54);
    let moved = real(&back.doc, &first.bytes, &back.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert_ne!(d_of(&moved.bytes), d_of(&first.bytes), "the route is written again: {} against {}", d_of(&moved.bytes), d_of(&first.bytes));
}
