//! What the writer refuses and does not drop, and what it reads back: a change it does not write is refused in words and
//! nothing is written; a change it writes is written against every other change of the same save; and the check of the bytes made is shown to run
//! for each kind of edit, by what it counted.

use std::sync::Arc;

use diacraft_doc::{Document, Value};
use diacraft_odg::{OriginalPackage, WriteError, WriteOptions, WriteWork, write_counted};

use crate::drawing::{Drawing, cm};
use crate::write_core::{custom, move_by, open, open_bytes, part, real, shape_mut};
use crate::write_new::{a_line, connector, push_shape, rectangle};

fn set_page_cell(doc: &mut Document, page: usize, cell: &str, value: f64) {
    let p = Arc::make_mut(doc.pages.get_mut(page).expect("no such page"));
    let sheet = Arc::make_mut(&mut p.sheet);
    sheet.cells.get_mut(cell).unwrap_or_else(|| panic!("no cell {cell}")).value = Value::from_number(value);
}

#[track_caller]
fn refused(saved: Result<diacraft_odg::Written, WriteError>, words: &str) {
    match saved {
        Err(WriteError::Unsupported(what)) => assert!(what.contains(words), "{what}"),
        other => panic!("expected a refusal of {words}: {other:?}"),
    }
}

/// A shape put into a group of the file is a new shape in a group, which a save does not write: refused, and not left out of the file as the
/// comparison once did for every shape the index did not know (compare.rs, the walk of the model).
#[test]
fn a_new_shape_put_into_a_group_of_the_file_is_refused_and_not_dropped() {
    let d = Drawing::a4().page(
        r#"<draw:g draw:name="g"><draw:rect draw:name="m" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"/><draw:rect draw:name="n" svg:x="4cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"/></draw:g>"#,
    );
    let mut o = open(&d);
    let group = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    let id = o.index.next_id(0).unwrap();
    shape_mut(&mut o.doc, 0, group).children.push(Arc::new(rectangle(id, "New", (2.0, 3.0, 4.0, 2.0))));
    refused(real(&o.doc, &o.bytes, &o.index), "group");
    // A new shape on the page beside the group is written: it is the page's top level the writer puts shapes in.
    let mut o = open(&d);
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, rectangle(id, "New", (2.0, 8.0, 4.0, 2.0)));
    assert!(real(&o.doc, &o.bytes, &o.index).is_ok_and(|w| w.edited));
}

/// A cell of a page that is not its size, margins or orientation is not written into the file's markup (it is DiaCraft's document file's): the save
/// is made and the file read again gives the cell as the model has it, with the page's size still the markup's.
#[test]
fn a_change_of_a_page_cell_that_is_no_size_is_carried_by_the_document_file_and_not_dropped() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    set_page_cell(&mut o.doc, 0, "PageScale", cm(2.0));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(saved.edited);
    let back = open_bytes(saved.bytes.clone());
    let scale = |d: &Document| d.pages.get(0).unwrap().sheet.cell("PageScale").and_then(|c| c.value.as_ref()?.number()).unwrap();
    assert!((scale(&back.doc) - cm(2.0)).abs() < 1e-9);
    assert!(part(&saved.bytes, "DiaCraft/document.xml").contains("PageScale"));
    assert!(!real(&back.doc, &saved.bytes, &back.index).unwrap().edited);
}

/// A shape moved and its page made taller in one save: the move is the model's move and the page's growth shifts every shape, and the element's `svg:y`
/// is written against both (compare.rs, the translation of a shape on a page whose height changed).
#[test]
fn a_shape_moved_and_its_page_made_taller_in_one_save_is_written_against_both() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    // 10 cm taller, the shape 1 cm up and 2 cm right: it keeps its distance from the bottom of the page less its move, so it is 10 cm lower than where
    // the 1 cm move alone puts it (3 cm to 2 cm), and 12 cm from the top.
    set_page_cell(&mut o.doc, 0, "PageHeight", cm(39.7));
    move_by(&mut o.doc, 0, id, 2.0, 1.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains(r#"draw:name="s1" svg:x="3cm" svg:y="12cm""#), "{after}");
    let back = open_bytes(saved.bytes);
    let pin =
        |d: &Document, cell: &str| d.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).unwrap();
    assert!((pin(&back.doc, "PinY") - pin(&o.doc, "PinY")).abs() < 1e-9 && (pin(&back.doc, "PinX") - pin(&o.doc, "PinX")).abs() < 1e-9);
    assert!((back.doc.pages.get(0).unwrap().sheet.cell("PageHeight").and_then(|c| c.value.as_ref()?.number()).unwrap() - cm(39.7)).abs() < 1e-9);
}

/// The check of the bytes made is shown to run for each kind of edit, by what it counted: the paragraphs of a retext and of a new shape, the geometry
/// element of a moved handle and of a new custom shape, and the attributes of the tags written on.
#[test]
fn the_check_of_the_bytes_made_reads_back_each_kind_of_edit_and_counts_what_it_read() {
    let save = |o: &crate::write_core::Open| {
        let mut work = WriteWork::default();
        let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work);
        assert!(saved.is_ok_and(|w| w.edited));
        work
    };
    // A retext of two paragraphs.
    let d = Drawing::a4().page(r#"<draw:custom-shape draw:name="t" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"><text:p>one</text:p><text:p>two</text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#);
    let mut o = open(&d);
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    shape_mut(&mut o.doc, 0, id).text =
        Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from("ONE\nTWO"))], ..Default::default() }));
    let work = save(&o);
    assert_eq!((work.checked_paragraphs, work.checked_outlines), (2, 0), "the two paragraphs rewritten");
    // A new rectangle with a text of two paragraphs: its paragraphs, its geometry element and the attributes of its tag.
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    let mut r = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
    r.text = Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from("a\nb"))], ..Default::default() }));
    push_shape(&mut o.doc, r);
    let work = save(&o);
    assert_eq!((work.checked_paragraphs, work.checked_outlines), (2, 1), "a new shape's paragraphs and its geometry element");
    assert!(
        work.checked_attributes >= 4 + 3,
        "the width, height, x and y of its tag and the three attributes of its geometry element: {}",
        work.checked_attributes
    );
    // A handle moved: the modifiers written on the geometry element are read back.
    let d = Drawing::a4().page(r#"<draw:custom-shape draw:name="h" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="4cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:modifiers="5400" draw:enhanced-path="M 0 0 L $0 0 21600 21600 Z N"><draw:handle draw:handle-position-x="$0" draw:handle-position-y="top"/></draw:enhanced-geometry></draw:custom-shape>"#);
    let mut o = open(&d);
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    {
        let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
        let user = sheet.sections.iter_mut().find(|s| s.name.as_str() == "User").unwrap();
        let row = Arc::make_mut(Arc::make_mut(user).rows.get_mut(0).unwrap());
        row.cells.get_mut("Value").unwrap().value = Value::from_number(2700.0);
    }
    let work = save(&o);
    assert_eq!((work.checked_paragraphs, work.checked_outlines), (0, 1), "the modifiers of the geometry element");
    // A move: the attributes of the tag, and no paragraph and no outline.
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    move_by(&mut o.doc, 0, id, 1.0, 0.0);
    let work = save(&o);
    assert_eq!((work.checked_paragraphs, work.checked_outlines), (0, 0));
    assert!(work.checked_attributes >= 2, "{}", work.checked_attributes);
    // A new line: the four ends of its tag are read back. A new connector: the four ends, the path and the view box.
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, a_line(id));
    let line = save(&o).checked_attributes;
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, connector(id, (2.0, 5.0, 6.0, 5.0)));
    let joined = save(&o).checked_attributes;
    // What the check reads of the styles a new shape names is 4 for the line and 4 for the connector, and what it reads of the tag is the rest.
    assert!(line >= 4 + 4, "the four ends of a new line's tag, and the styles it names: {line}");
    assert!(joined >= 4 + 2 + 4, "the four ends, the path and the view box of a new connector's tag, and the styles it names: {joined}");
}
