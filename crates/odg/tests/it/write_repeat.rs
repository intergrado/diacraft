//! The writer owns repeat saves: saving a model that has not changed, through the index of the save before, is no
//! edit and gives the same bytes, for every kind of edit and every save after it. The second save of a new shape, a new connector, a page that moved
//! to a copy of its master page, and a page whose master page holds shapes, was refused for changes the user had not made. Each case saves three
//! times in a row; a model edited between two saves is written as an edit of that model, and an index is refused for bytes it does not go with.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Text, TextPart, Value};
use diacraft_odg::{WriteError, Written};

use crate::drawing::{Drawing, cm};
use crate::write_core::{Open, custom, move_by, open, open_bytes, part, real, shape_mut};
use crate::write_new::{connector, glue, push_shape, rectangle};

fn set_page_cell(doc: &mut Document, page: usize, cell: &str, value: f64) {
    let p = Arc::make_mut(doc.pages.get_mut(page).expect("no such page"));
    let sheet = Arc::make_mut(&mut p.sheet);
    sheet.cells.get_mut(cell).unwrap_or_else(|| panic!("no cell {cell}")).value = Value::from_number(value);
}

/// Saves `o`'s model, then the same model through each index, three times in all: the first is an edit, the others are no edit and give its bytes.
#[track_caller]
fn repeats(label: &str, o: &Open) -> Written {
    let first = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{label}: the first save was refused: {} ({e})", e.class()));
    assert!(first.edited, "{label}: the first save is an edit");
    let mut last = first.clone();
    for n in 2..=3 {
        let again = real(&o.doc, &last.bytes, &last.index).unwrap_or_else(|e| panic!("{label}: save {n} was refused: {} ({e})", e.class()));
        assert!(!again.edited, "{label}: save {n} of the model that was saved is no edit");
        assert_eq!(again.bytes, last.bytes, "{label}: save {n} gives the bytes of the save before");
        last = again;
    }
    first
}

/// An edit of the model of an open drawing, with its name.
type Named<'a> = (&'a str, &'a dyn Fn(&mut Open));

const TWO_SHAPES: &str = r#"<draw:custom-shape draw:name="s1" xml:id="xa" draw:id="dA" svg:x="1cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape><draw:custom-shape draw:name="s2" svg:x="10cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#;

#[test]
fn a_second_and_a_third_save_of_a_new_shape_are_no_edit() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, rectangle(id, "Rectangle.1", (2.0, 3.0, 4.0, 2.0)));
    repeats("new rectangle", &o);
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    let mut r = rectangle(id, "Rectangle.1", (2.0, 3.0, 4.0, 2.0));
    r.text = Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from("hello  world\nsecond"))], ..Default::default() }));
    push_shape(&mut o.doc, r);
    repeats("new rectangle with text", &o);
}

#[test]
fn a_second_and_a_third_save_of_a_new_connector_are_no_edit() {
    // The connector alone, with its route marker, and glued at both ends to shapes of the file: one has ids of its own and one has none.
    let user = |c: &mut diacraft_doc::Shape| {
        let mut user = diacraft_doc::Section::new("User", None);
        let mut r = diacraft_doc::Row { name: Some(Arc::from("DiaCraftRoute")), ..diacraft_doc::Row::default() };
        r.cells.insert(CellName::new("Value"), Cell::read(None, Some("1"), None, None, &mut 1000));
        user.rows.push(Arc::new(r));
        // (`connector` holds this section already: it is put again as a section of the same key and never as a second one.)
        Arc::make_mut(&mut c.sheet).sections.retain(|s| s.name.as_str() != "User");
        Arc::make_mut(&mut c.sheet).sections.push(Arc::new(user));
    };
    for variant in 0..3 {
        let mut o = open(&Drawing::a4().page(TWO_SHAPES));
        let (a, b) = (o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id, o.doc.pages.get(0).unwrap().shapes.get(1).unwrap().id);
        let id = o.index.next_id(0).unwrap();
        let mut c = connector(id, (5.0, 25.95, 10.0, 25.95));
        if variant >= 1 {
            user(&mut c);
        }
        push_shape(&mut o.doc, c);
        if variant == 2 {
            glue(&mut o.doc, id, true, a, 3);
            glue(&mut o.doc, id, false, b, 1);
        }
        let first = repeats(&format!("new connector, variant {variant}"), &o);
        if variant == 2 {
            // The ids the connector's two shapes got are in the file; the model does not hold them, and still saves as it was.
            assert!(
                part(&first.bytes, "content.xml").contains(r#"draw:name="s2" svg:x="10cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm" xml:id="#)
            );
        }
    }
    // A new rectangle and a new connector from it to a shape of the file.
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let (r, c) = (o.index.next_id(0).unwrap(), o.index.next_id(0).unwrap() + 1);
    push_shape(&mut o.doc, rectangle(r, "Rectangle.1", (10.0, 3.0, 4.0, 2.0)));
    push_shape(&mut o.doc, connector(c, (10.0, 25.7, 5.0, 25.45)));
    glue(&mut o.doc, c, true, r, 1);
    glue(&mut o.doc, c, false, 1, 3);
    repeats("new rectangle and connector", &o);
}

#[test]
fn a_second_and_a_third_save_of_a_page_that_moved_to_a_copy_are_no_edit() {
    // Two pages on one master page, the second resized: it names the copy and the model still has the master page's name.
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")).page(&custom("s2", "2cm")));
    set_page_cell(&mut o.doc, 1, "PageHeight", cm(42.0));
    let first = repeats("page split", &o);
    // The same model after an edit that reaches the copy: the page is resized again, and it edits its own layout where it is.
    let mut again = o.doc.clone();
    set_page_cell(&mut again, 1, "PageHeight", cm(30.0));
    let saved = real(&again, &first.bytes, &first.index).unwrap_or_else(|e| panic!("a second resize through the index: {e}"));
    let styles = part(&saved.bytes, "styles.xml");
    assert!(styles.contains("fo:page-height=\"30cm\"") && !styles.contains("PM3") && !styles.contains("Default_3"), "{styles}");
    let back = open_bytes(saved.bytes);
    assert!((back.doc.pages.get(1).unwrap().sheet.cell("PageHeight").and_then(|c| c.value.as_ref()?.number()).unwrap() - cm(30.0)).abs() < 1e-9);
    // Masters that share a layout, the master page left unnamed taken out: the model's page is as it was.
    let mut d = Drawing::a4();
    d.masters.push_str(r#"<style:master-page style:name="Other" style:page-layout-name="PM1"/>"#);
    d = d.page(&custom("s1", "1cm")).page(&custom("s2", "1cm"));
    d.pages = d.pages.replace(r#"draw:name="page2" draw:master-page-name="Default""#, r#"draw:name="page2" draw:master-page-name="Other""#);
    let mut o = open(&d);
    set_page_cell(&mut o.doc, 1, "PageHeight", cm(42.0));
    repeats("page moved off a master page that is taken out", &o);
}

#[test]
fn a_second_and_a_third_save_of_a_page_whose_master_page_holds_shapes_resized_with_its_background_are_no_edit() {
    let mut d = Drawing::a4();
    d.masters = d.masters.replace(
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#,
        &format!(r#"<style:master-page style:name="Default" style:page-layout-name="PM1">{}</style:master-page>"#, custom("bg", "1cm")),
    );
    let mut o = open(&d.page(&custom("s1", "1cm")));
    set_page_cell(&mut o.doc, 0, "PageHeight", cm(40.0));
    set_page_cell(&mut o.doc, 1, "PageHeight", cm(40.0));
    repeats("page and background page resized", &o);
}

#[test]
fn a_second_and_a_third_save_of_every_other_kind_of_edit_are_no_edit_and_every_kind_at_once() {
    let gr = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"/></style:style>"##;
    let shape = |n: &str, x: &str| {
        format!(
            r#"<draw:custom-shape draw:name="{n}" draw:style-name="gr1" svg:x="{x}" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"><text:p>text {n}</text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#
        )
    };
    let poly = r#"<draw:polygon draw:name="pg" draw:style-name="gr1" svg:x="1cm" svg:y="10cm" svg:width="4cm" svg:height="4cm" svg:viewBox="0 0 1000 1000" draw:points="0,0 1000,0 500,1000"/>"#;
    let base = || open(&Drawing::a4().auto(gr).page(&format!("{}{}{}", shape("a", "1cm"), shape("b", "8cm"), poly)).page(&custom("p2", "1cm")));
    let id_of = |o: &Open, at: usize| o.doc.pages.get(0).unwrap().shapes.get(at).unwrap().id;
    let restyle = |o: &mut Open| {
        let id = id_of(o, 0);
        Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet)
            .cells
            .insert(CellName::new("FillForegnd"), Cell::constant(Value::from_text("#00ff00")));
    };
    let retext = |o: &mut Open| {
        let id = id_of(o, 1);
        shape_mut(&mut o.doc, 0, id).text = Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from("text  b2"))], ..Default::default() }));
    };
    let outline = |o: &mut Open| {
        let id = id_of(o, 2);
        let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
        let g = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Geometry").unwrap();
        let row = Arc::make_mut(Arc::make_mut(g).rows.get_mut(2).unwrap());
        row.cells.get_mut("X").unwrap().value = Value::from_number(0.25);
    };
    let delete = |o: &mut Open| {
        let id = id_of(o, 1);
        let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|s| s.id == id).unwrap();
        page.shapes.remove(at);
    };
    let moved = |o: &mut Open| {
        let id = id_of(o, 0);
        move_by(&mut o.doc, 0, id, 1.0, 0.5);
    };
    let resize_in_place = |o: &mut Open| set_page_cell(&mut o.doc, 1, "PageHeight", cm(35.0));
    let resize_split = |o: &mut Open| set_page_cell(&mut o.doc, 0, "PageHeight", cm(35.0));
    let new_shape = |o: &mut Open| {
        let id = o.index.next_id(0).unwrap();
        push_shape(&mut o.doc, rectangle(id, "New", (2.0, 15.0, 4.0, 2.0)));
    };
    let edits: [Named<'_>; 8] = [
        ("restyle", &restyle),
        ("retext", &retext),
        ("outline", &outline),
        ("delete", &delete),
        ("move", &moved),
        ("page resized in place", &resize_in_place),
        ("page resized and split", &resize_split),
        ("new shape", &new_shape),
    ];
    for (label, edit) in edits {
        let mut o = base();
        edit(&mut o);
        repeats(label, &o);
    }
    // Every kind at once: the page resized (a split), a new shape on it, a restyle, a retext, a moved point and a shape moved, in one save.
    let mut o = base();
    new_shape(&mut o);
    for (_, edit) in &edits[..3] {
        edit(&mut o);
    }
    moved(&mut o);
    resize_split(&mut o);
    repeats("every kind at once", &o);
}

/// A model edited between two saves is written as an edit of the model that was saved: a new rectangle moved and restyled is two attributes and a
/// style, and a new connector's shape moved moves the connector's end where the index of the save says it is.
#[test]
fn a_new_shape_edited_after_the_save_that_made_it_is_written_as_that_edit() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, rectangle(id, "Rectangle.1", (2.0, 3.0, 4.0, 2.0)));
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    move_by(&mut o.doc, 0, id, 1.0, 0.0);
    let second = real(&o.doc, &first.bytes, &first.index).unwrap_or_else(|e| panic!("the edit of a shape the save before made: {e}"));
    assert!(second.edited);
    let (before, after) = (part(&first.bytes, "content.xml"), part(&second.bytes, "content.xml"));
    assert_eq!(after, before.replacen(r#"svg:x="2cm""#, r#"svg:x="3cm""#, 1), "one attribute of the new element changed");
    // And saved again it is no edit.
    let third = real(&o.doc, &second.bytes, &second.index).unwrap();
    assert!(!third.edited && third.bytes == second.bytes);
}

/// An index goes with the bytes the save that made it gave: the index of one save with the bytes of another is refused in words, and nothing is
/// written.
#[test]
fn the_index_of_a_save_is_refused_for_bytes_it_does_not_go_with() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, rectangle(id, "Rectangle.1", (2.0, 3.0, 4.0, 2.0)));
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    move_by(&mut o.doc, 0, id, 1.0, 0.0);
    let second = real(&o.doc, &first.bytes, &first.index).unwrap();
    match real(&o.doc, &second.bytes, &first.index) {
        Err(e @ WriteError::Mismatch(words)) => {
            assert_eq!(e.class(), "write_mismatch");
            assert!(words.contains("other bytes"), "{words}");
        }
        other => panic!("expected a refusal: {other:?}"),
    }
    // The bytes the index goes with are accepted.
    assert!(real(&o.doc, &second.bytes, &second.index).is_ok_and(|w| !w.edited));
}
