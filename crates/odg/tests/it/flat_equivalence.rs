//! A flat document is saved as the package is: the same edit made to the model of a package and to the model of
//! the flat document made of the same pieces, and saved by each form's own save, reads back as **one model**. The oracle is not the writer's own text: the two
//! saves are laid out differently (two parts, one document; the new styles in one container or in two), and what they must agree on is what a reader finds.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, ShapeType, Text, TextPart, Value};
use diacraft_odg::{Limits, ShapeIndex, read};

use crate::drawing::{Drawing, cm};
use crate::flat::Flat;
use crate::flat_write::save;
use crate::write_core::{custom, move_by, open_bytes, shape_mut};
use crate::write_new::{push_shape, rectangle};

/// The model with each picture said to come from one place (a package part and inline data are one picture).
fn named_alike(mut doc: Document) -> Document {
    for i in 0..doc.pages.len() {
        let page = Arc::make_mut(doc.pages.get_mut(i).expect("a page"));
        for j in 0..page.shapes.len() {
            let shape = Arc::make_mut(page.shapes.get_mut(j).expect("a shape"));
            if let Some(foreign) = shape.foreign.as_mut() {
                let foreign = Arc::make_mut(foreign);
                if let Some(picture) = foreign.picture.as_mut() {
                    let mut media = (**picture).clone();
                    media.part = Arc::from("a picture");
                    *picture = Arc::new(media);
                }
            }
        }
    }
    crate::drawing::without_package(&doc)
}

/// The drawing's model after `edit` and a save, read back, as a package and as a flat document.
fn both(d: &Drawing, edit: &dyn Fn(&mut Document, &ShapeIndex)) -> (Document, Document) {
    let go = |bytes: Vec<u8>| {
        let mut o = open_bytes(bytes);
        edit(&mut o.doc, &o.index);
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save: {} ({e})", e.class()));
        assert!(saved.edited);
        let (back, _) = read(&saved.bytes, &Limits::default()).unwrap_or_else(|e| panic!("the save does not read: {} ({e})", e.class()));
        crate::flat_write::second_save_is_no_edit(&o.doc, &saved);
        named_alike(back)
    };
    (go(d.bytes()), go(Flat::of(d).bytes()))
}

fn same(d: &Drawing, what: &str, edit: &dyn Fn(&mut Document, &ShapeIndex)) {
    let (package, flat) = both(d, edit);
    assert_eq!(package, flat, "{what}: the package's save and the flat document's read back as one model");
}

fn set_text(doc: &mut Document, id: u32, cell: &str, text: &str) {
    Arc::make_mut(&mut shape_mut(doc, 0, id).sheet).cells.insert(CellName::new(cell), Cell::constant(Value::from_text(text)));
}

fn set_page_cell(doc: &mut Document, page: usize, cell: &str, value: f64) {
    let p = Arc::make_mut(doc.pages.get_mut(page).expect("a page"));
    Arc::make_mut(&mut p.sheet).cells.get_mut(cell).unwrap_or_else(|| panic!("no cell {cell}")).value = Value::from_number(value);
}

fn gr(name: &str, fill: &str) -> String {
    format!(
        "<style:style style:name=\"{name}\" style:family=\"graphic\" style:parent-style-name=\"Standard\"><style:graphic-properties draw:fill=\"solid\" draw:fill-color=\"{fill}\" svg:stroke-width=\"0.1cm\"/></style:style>"
    )
}

/// A shape on the automatic style `style`, laid out as a writer lays a page out.
fn rect(name: &str, style: &str, x: &str) -> String {
    format!(
        "\n    <draw:rect draw:name=\"{name}\" draw:style-name=\"{style}\" svg:x=\"{x}\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\"><text:p>Hello</text:p></draw:rect>"
    )
}

fn styled() -> Drawing {
    Drawing::a4()
        .common(r#"<style:style style:name="Standard" style:family="graphic"/>"#)
        .auto(&format!("{}{}", gr("gr1", "#ff0000"), gr("gr7", "#00ff00")))
        .page(&format!("{}{}{}", rect("a", "gr1", "1cm"), rect("b", "gr1", "6cm"), rect("c", "gr7", "11cm")))
}

fn id_of(doc: &Document, name: &str) -> u32 {
    doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("no shape {name}")).id
}

#[test]
fn a_move_reads_back_as_one_model() {
    same(&styled(), "a move", &|doc, _| {
        let id = id_of(doc, "b");
        move_by(doc, 0, id, 1.0, -0.5)
    });
}

#[test]
fn a_restyle_that_clones_a_shared_style_reads_back_as_one_model() {
    same(&styled(), "a restyle of a shape that shares its style", &|doc, _| {
        let id = id_of(doc, "a");
        set_text(doc, id, "FillForegnd", "#336699");
    });
    same(&styled(), "a restyle of a shape that is alone on its style", &|doc, _| {
        let id = id_of(doc, "c");
        set_text(doc, id, "FillForegnd", "#336699");
    });
}

#[test]
fn a_delete_that_orphans_a_style_reads_back_as_one_model() {
    same(&styled(), "a delete", &|doc, _| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|s| s.name.as_deref() == Some("c")).unwrap();
        let mut shapes: Vec<_> = page.shapes.iter().cloned().collect();
        shapes.remove(at);
        page.shapes = diacraft_doc::SharedVec::from_vec(shapes);
    });
}

#[test]
fn a_retext_reads_back_as_one_model() {
    same(&styled(), "a retext", &|doc, _| {
        let id = id_of(doc, "a");
        shape_mut(doc, 0, id).text = Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from("Changed\nand two lines"))], ..Default::default() }));
    });
}

#[test]
fn new_shapes_read_back_as_one_model() {
    same(&styled(), "a new rectangle", &|doc, index| {
        let id = index.next_id(0).unwrap();
        push_shape(doc, rectangle(id, "Rectangle.1", (2.0, 8.0, 4.0, 2.0)));
    });
    // Two new shapes, and a move in the same save.
    same(&styled(), "two new rectangles and a move", &|doc, index| {
        let id = index.next_id(0).unwrap();
        push_shape(doc, rectangle(id, "Rectangle.1", (2.0, 8.0, 4.0, 2.0)));
        push_shape(doc, rectangle(id + 1, "Rectangle.2", (8.0, 8.0, 4.0, 2.0)));
        let a = id_of(doc, "a");
        move_by(doc, 0, a, 1.0, 0.0);
    });
}

/// A page's size is in the layout of its master page: in place when no other page uses it, a copy of the master page and the layout when another does (the
/// styles part of a package and the master styles of a flat document are where each goes; the pages in `office:body` name the copy).
#[test]
fn a_page_resize_reads_back_as_one_model() {
    let one = Drawing::a4().page(&custom("s1", "1cm"));
    same(&one, "the resize of the only page", &|doc, _| {
        set_page_cell(doc, 0, "PageHeight", cm(21.0));
    });
    let two = Drawing::a4().page(&custom("s1", "1cm")).page(&custom("s2", "2cm"));
    same(&two, "the resize of a page that shares its master page", &|doc, _| {
        set_page_cell(doc, 1, "PageWidth", cm(29.7));
        set_page_cell(doc, 1, "PageHeight", cm(42.0));
    });
    // A restyle and a resize together: the content pass and the styles pass of one save.
    same(&styled().page(&custom("s9", "1cm")), "a resize and a restyle", &|doc, _| {
        let id = id_of(doc, "a");
        set_text(doc, id, "FillForegnd", "#336699");
        set_page_cell(doc, 1, "PageHeight", cm(42.0));
    });
}

/// A master page's shape is read as a background page: an edit of it is the styles pass of the save.
#[test]
fn an_edit_of_a_master_pages_shape_reads_back_as_one_model() {
    let mut d = styled();
    d.auto.push_str(&gr("gr9", "#112233"));
    d.styles_auto.push_str(&gr("gr9m", "#112233"));
    d.masters = d.masters.replace(
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#,
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"><draw:rect draw:name="m1" draw:style-name="gr9m" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"/></style:master-page>"#,
    );
    same(&d, "a master page's shape moved", &|doc, _| {
        let at = doc.pages.iter().position(|p| p.background).expect("a background page");
        let page = Arc::make_mut(doc.pages.get_mut(at).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let sheet = Arc::make_mut(&mut shape.sheet);
        let x = sheet.cells.get("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0);
        sheet.cells.get_mut("PinX").unwrap().value = Value::from_number(x + cm(1.0));
    });
    // A shape of a page and of a master page edited in one save: both passes.
    same(&d, "a page's shape and a master page's shape", &|doc, _| {
        let id = id_of(doc, "a");
        move_by(doc, 0, id, 1.0, 0.0);
        let at = doc.pages.iter().position(|p| p.background).expect("a background page");
        let page = Arc::make_mut(doc.pages.get_mut(at).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let sheet = Arc::make_mut(&mut shape.sheet);
        let y = sheet.cells.get("PinY").and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0);
        sheet.cells.get_mut("PinY").unwrap().value = Value::from_number(y + cm(1.0));
    });
}

/// A new connector with line ends: the markers go into `office:styles` (the styles pass follows the content pass that named them).
#[test]
fn a_new_connector_with_line_ends_reads_back_as_one_model() {
    let mut d = styled();
    d.common.push_str("<draw:stroke-dash draw:name=\"D\"/>");
    same(&d, "a new connector with an arrow", &|doc, index| {
        let id = index.next_id(0).unwrap();
        let mut connector = crate::write_new::connector(id, (1.0, 1.0, 6.0, 2.0));
        let sheet = Arc::make_mut(&mut connector.sheet);
        for (name, v) in [("EndArrow", 13.0), ("EndArrowSize", 2.0), ("BeginArrow", 0.0), ("LineWeight", 0.02)] {
            sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_number(v).unwrap()));
        }
        push_shape(doc, connector);
        assert_eq!(doc.pages.get(0).unwrap().shapes.iter().filter(|s| s.kind == ShapeType::Shape).count(), 4);
    });
}

/// A document that has no `office:styles` is not given one by a save that has nothing to put in it: the second pass of a resize leaves the children as they are.
#[test]
fn a_save_with_no_marker_to_write_makes_no_office_styles() {
    let one = Drawing::a4().page(&custom("s1", "1cm"));
    let flat = Flat::of(&one);
    assert!(!flat.text().contains("office:styles"), "the flat drawing has none");
    let mut o = open_bytes(flat.bytes());
    set_page_cell(&mut o.doc, 0, "PageHeight", cm(21.0));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(saved.edited);
    let text = crate::flat_write::text_of(&saved.bytes);
    assert!(text.contains("21") && !text.contains("office:styles"), "{text}");
    // With a marker to write, the element is made: the other side of the same condition.
    let mut o = open_bytes(flat.bytes());
    let id = o.index.next_id(0).unwrap();
    let mut connector = crate::write_new::connector(id, (1.0, 1.0, 6.0, 2.0));
    crate::write_core::set_num_new(&mut connector, "EndArrow", 13.0);
    push_shape(&mut o.doc, connector);
    set_page_cell(&mut o.doc, 0, "PageHeight", cm(21.0));
    let text = crate::flat_write::text_of(&save(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    assert!(text.contains("<office:styles>"), "{text}");
}
