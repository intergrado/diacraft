//! New shapes and connectors ("add a connector"):
//! written as tier 1, and read back as the model has them.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Connect, Document, Row, Section, Shape, ShapeType};
use diacraft_odg::WriteError;

use crate::drawing::{Drawing, cm};
use crate::write_core::{Saver, compare, content_changes, custom, open, open_bytes, part, real};

const PAGE_HEIGHT_CM: f64 = 29.7;

fn cell(formula: Option<&str>, value: f64, unit: Option<&str>) -> Cell {
    Cell::read(formula, Some(&value.to_string()), unit, None, &mut 1_000)
}

fn row(kind: &str, ix: u32, cells: &[(&str, Option<&str>, f64)]) -> Arc<Row> {
    let mut r = Row { row_type: Some(Arc::from(kind)), ix: Some(ix), ..Row::default() };
    for (name, formula, value) in cells {
        r.cells.insert(CellName::new(name), cell(*formula, *value, Some("IN")));
    }
    Arc::new(r)
}

fn at<'a>(x: Option<&'a str>, xv: f64, y: Option<&'a str>, yv: f64) -> [(&'static str, Option<&'a str>, f64); 2] {
    [("X", x, xv), ("Y", y, yv)]
}

/// DiaCraft's rectangle as the engine makes it: the rows, the four side-middle connection rows, and a fill and a line.
pub fn rectangle(id: u32, name: &str, (x, y, w, h): (f64, f64, f64, f64)) -> Shape {
    let (w_in, h_in) = (cm(w), cm(h));
    let mut shape = Shape::new(id);
    shape.name = Some(Arc::from(name));
    shape.name_u = Some(Arc::from(name));
    shape.kind = ShapeType::Shape;
    let sheet = Arc::make_mut(&mut shape.sheet);
    let mut put = |n: &str, c: Cell| {
        sheet.cells.insert(CellName::new(n), c);
    };
    put("Width", cell(None, w_in, Some("IN")));
    put("Height", cell(None, h_in, Some("IN")));
    put("LocPinX", cell(Some("Width*0.5"), w_in / 2.0, Some("IN")));
    put("LocPinY", cell(Some("Height*0.5"), h_in / 2.0, Some("IN")));
    put("PinX", cell(None, cm(x + w / 2.0), Some("IN")));
    put("PinY", cell(None, cm(PAGE_HEIGHT_CM - y - h / 2.0), Some("IN")));
    put("FillPattern", cell(None, 1.0, None));
    put("FillForegnd", Cell::constant(diacraft_doc::Value::from_text("#336699")));
    put("LinePattern", cell(None, 1.0, None));
    let mut geometry = Section::new("Geometry", Some(0));
    for (i, (kind, xf, xv, yf, yv)) in [
        ("MoveTo", None, 0.0, None, 0.0),
        ("LineTo", Some("Width"), w_in, None, 0.0),
        ("LineTo", Some("Width"), w_in, Some("Height"), h_in),
        ("LineTo", None, 0.0, Some("Height"), h_in),
        ("LineTo", None, 0.0, None, 0.0),
    ]
    .into_iter()
    .enumerate()
    {
        geometry.rows.push(row(kind, i as u32 + 1, &at(xf, xv, yf, yv)));
    }
    sheet.sections.push(Arc::new(geometry));
    let mut connection = Section::new("Connection", None);
    let sides = [
        (Some("Width*0.5"), w_in / 2.0, Some("Height"), h_in),
        (None, 0.0, Some("Height*0.5"), h_in / 2.0),
        (Some("Width*0.5"), w_in / 2.0, None, 0.0),
        (Some("Width"), w_in, Some("Height*0.5"), h_in / 2.0),
    ];
    // With the directions a connector leaves by and `Type` 0, as the engine's rows hold them.
    let directions = [(0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0)];
    for (i, ((xf, xv, yf, yv), (dx, dy))) in sides.into_iter().zip(directions).enumerate() {
        let [x, y] = at(xf, xv, yf, yv);
        connection.rows.push(row("", i as u32, &[x, y, ("DirX", None, dx), ("DirY", None, dy), ("Type", None, 0.0)]));
    }
    sheet.sections.push(Arc::new(connection));
    shape
}

/// `shape` holding the four side-middle connection rows of [`rectangle`], which the reader gives every shape it reads, a line's and a connector's too: a shape
/// that holds them carries nothing of them, and one that holds none carries that it has none.
pub fn with_side_middles(mut shape: Shape) -> Shape {
    let rows = rectangle(0, "", (0.0, 0.0, 1.0, 1.0)).sheet.sections.iter().find(|s| s.name.as_str() == "Connection").cloned().unwrap();
    Arc::make_mut(&mut shape.sheet).sections.push(rows);
    shape
}

/// A straight connector from (`x1`, `y1`) to (`x2`, `y2`), in centimetres from the page's lower left, as the engine's `.odg` save hands one to the
/// writer. `connector.add` itself writes neither `ShapeRouteStyle` nor `ConLineRouteExt`, which the writer reads to tell a connector from a line:
/// the engine gives a connector those two cells in the copy it saves, and this helper has them.
pub fn connector(id: u32, (x1, y1, x2, y2): (f64, f64, f64, f64)) -> Shape {
    let mut shape = Shape::new(id);
    shape.name = Some(Arc::from("Connector.1"));
    shape.name_u = Some(Arc::from("Connector.1"));
    shape.kind = ShapeType::Shape;
    let sheet = Arc::make_mut(&mut shape.sheet);
    let length = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
    let mut put = |n: &str, c: Cell| {
        sheet.cells.insert(CellName::new(n), c);
    };
    put("BeginX", cell(None, cm(x1), Some("IN")));
    put("BeginY", cell(None, cm(y1), Some("IN")));
    put("EndX", cell(None, cm(x2), Some("IN")));
    put("EndY", cell(None, cm(y2), Some("IN")));
    put("Width", cell(Some("SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"), cm(length), Some("IN")));
    put("Height", cell(None, 0.0, Some("IN")));
    put("Angle", cell(Some("ATAN2(EndY-BeginY,EndX-BeginX)"), (y2 - y1).atan2(x2 - x1), Some("DEG")));
    put("PinX", cell(Some("(BeginX+EndX)/2"), cm((x1 + x2) / 2.0), Some("IN")));
    put("PinY", cell(Some("(BeginY+EndY)/2"), cm((y1 + y2) / 2.0), Some("IN")));
    put("LocPinX", cell(Some("Width*0.5"), cm(length) / 2.0, Some("IN")));
    put("LocPinY", cell(Some("Height*0.5"), 0.0, Some("IN")));
    put("ShapeRouteStyle", cell(None, 1.0, None));
    put("ConLineRouteExt", cell(None, 1.0, None));
    let mut geometry = Section::new("Geometry", Some(0));
    geometry.rows.push(row("MoveTo", 1, &[("X", None, 0.0), ("Y", None, 0.0)]));
    geometry.rows.push(row("LineTo", 2, &[("X", None, cm(length)), ("Y", None, 0.0)]));
    sheet.sections.push(Arc::new(geometry));
    let mut user = Section::new("User", None);
    user.rows.push({
        let mut r = Row { name: Some(Arc::from("DiaCraftRoute")), ..Row::default() };
        r.cells.insert(CellName::new("Value"), cell(None, 1.0, None));
        Arc::new(r)
    });
    sheet.sections.push(Arc::new(user));
    shape
}

pub fn push_shape(doc: &mut Document, shape: Shape) {
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes.push(Arc::new(shape));
}

pub fn glue(doc: &mut Document, from: u32, begin: bool, to: u32, row: u32) {
    glue_part(doc, from, begin, to, 100 + row);
}

/// Glues an end of connector `from` to shape `to` at `part` of the shape (the whole shape is 3, a connection row is 100 and its number).
pub fn glue_part(doc: &mut Document, from: u32, begin: bool, to: u32, part: u32) {
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let mut rows: Vec<Connect> = page.connects.iter().cloned().collect();
    rows.push(Connect {
        from_sheet: from,
        from_cell: Some(Arc::from(if begin { "BeginX" } else { "EndX" })),
        from_part: Some(if begin { 9 } else { 12 }),
        to_sheet: to,
        to_cell: Some(Arc::from("PinX")),
        to_part: Some(part),
        ..Connect::default()
    });
    page.connects = diacraft_doc::Connects::from_vec(rows);
}

fn text_of(content: &str, tag: &str) -> String {
    let start = content.find(&format!("<draw:{tag} ")).unwrap_or_else(|| panic!("no <draw:{tag} in {content}"));
    let end = content[start..].find(&format!("</draw:{tag}>")).unwrap() + format!("</draw:{tag}>").len();
    content[start..start + end].to_owned()
}

/// The plan's `new_rectangle_is_written_as_libreoffice_writes_one`: the rectangle on the page, as the elements and attributes the specs give
/// (`draw:custom-shape`, its `draw:enhanced-geometry`, an empty `text:p`, `draw:layer`, `draw:name`, a new automatic graphic style and paragraph style), in
/// the places the reader's index says, with no tier 2.
fn new_rectangle_case(save: &Saver) {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, rectangle(id, "Rectangle.1", (2.0, 3.0, 4.0, 2.0)));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    // The new element, after the shape that was there (the last of the page), the first style the file has of each family.
    let new = r#"<draw:custom-shape draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="Rectangle.1" svg:width="4cm" svg:height="2cm" svg:x="2cm" svg:y="3cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="non-primitive" draw:enhanced-path="M 0 21600 L 21600 21600 21600 0 0 0 0 21600 Z N"/></draw:custom-shape>"#;
    assert!(after.contains(new), "{after}");
    assert!(after.contains(&format!("</draw:custom-shape>{new}</draw:page>")), "put last, as the model has it: {after}");
    let graphic = after.split("<style:style style:name=\"gr1\"").nth(1).unwrap().split("</style:style>").next().unwrap();
    for attr in ["style:family=\"graphic\"", "draw:fill=\"solid\"", "draw:fill-color=\"#336699\"", "draw:stroke=\"solid\""] {
        assert!(graphic.contains(attr), "{attr} in {graphic}");
    }
    assert!(!graphic.contains("auto-grow-height"), "a shape with no text needs none");
    assert!(after.contains(r#"<style:style style:name="P1" style:family="paragraph"/>"#), "{after}");
    // Nothing else of the part changed but the two styles and the shape, and no DiaCraft attribute is written.
    let rest = after.replacen(new, "", 1);
    assert_eq!(
        rest.replace(r#"<style:style style:name="P1" style:family="paragraph"/>"#, "")
            .replace(&format!("<style:style style:name=\"gr1\"{graphic}</style:style>"), ""),
        before
    );
    assert!(!after.contains(" diacraft:"), "tier 1: no attribute of DiaCraft's namespace is written");
    let diff = compare(&o.bytes, &saved.bytes);
    let (changed, added, removed) = content_changes(&diff);
    assert_eq!((changed.len(), added, removed), (0, 3, 0), "the shape and its two styles: {diff:?}");
    // The index maps the new shape to the element, and it reads back as the model has it.
    assert_eq!(saved.index.ordinal(0, id), Some(2));
    let back = open_bytes(saved.bytes);
    let read = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
    assert_eq!(read.name_u.as_deref(), Some("Rectangle.1"));
    let value = |s: &Shape, n: &str| s.sheet.cell(n).and_then(|c| c.value.as_ref()?.number()).unwrap();
    let made = o.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
    for n in ["Width", "Height", "PinX", "PinY"] {
        assert!((value(read, n) - value(made, n)).abs() < 1e-9, "{n}");
    }
    let rows = |s: &Shape| {
        s.sheet
            .sections
            .iter()
            .find(|x| x.name.as_str() == "Geometry")
            .unwrap()
            .rows
            .iter()
            .map(|r| r.row_type.as_deref().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(rows(read), rows(made), "The outline reads back as the rows it was made of");
    assert_eq!(read.sheet.cell("FillForegnd").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()), Some("#336699".to_owned()));
}

/// The plan's `new_connector_glued_to_new_shape_reads_back_glued`: a new rectangle and a new connector from the rectangle's left side to an
/// existing shape's right side. The two targets get `xml:id` and `draw:id` in the order the connector names them; the connector names them and
/// their glue ids; read back, it is glued to the same two shapes by the same rows.
fn new_connector_case(save: &Saver) {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let (r, c) = (o.index.next_id(0).unwrap(), o.index.next_id(0).unwrap() + 1);
    push_shape(&mut o.doc, rectangle(r, "Rectangle.1", (10.0, 3.0, 4.0, 2.0)));
    // From the new rectangle's left side middle (row 1) to the first shape's right side middle (row 3): from (10, 25.7) to (5, 25.45).
    push_shape(&mut o.doc, connector(c, (10.0, PAGE_HEIGHT_CM - 4.0, 5.0, PAGE_HEIGHT_CM - 4.25)));
    glue(&mut o.doc, c, true, r, 1);
    glue(&mut o.doc, c, false, 1, 3);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    // The existing shape that was named gets both ids; the new rectangle has them from the start; ids are numbered by the order the connector names them.
    assert!(after.contains(r#"draw:name="s1" svg:x="1cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm" xml:id="id2" draw:id="id2""#), "{after}");
    let rect = text_of(&after, "custom-shape");
    let rect = after.split("<draw:custom-shape ").nth(2).map_or(rect, |s| format!("<draw:custom-shape {s}"));
    assert!(rect.contains(r#"xml:id="id1" draw:id="id1""#), "{rect}");
    let connector = text_of(&after, "connector");
    assert!(connector.contains(r#"draw:start-shape="id1" draw:start-glue-point="3" draw:end-shape="id2" draw:end-glue-point="1""#), "{connector}");
    // The ends are measured from the top of the page: (10, 25.7) and (5, 25.45) from the bottom of one 29.7 cm high are 4 and 4.25 from the top.
    assert!(connector.contains(r#"svg:x1="10cm" svg:y1="4cm" svg:x2="5cm" svg:y2="4.25cm""#), "{connector}");
    assert!(connector.contains("svg:viewBox=\"0 0 ") && connector.contains("svg:d=\"M "), "The form: {connector}");
    assert!(connector.contains("<text:p/>") && connector.contains(r#"draw:layer="layout""#), "{connector}");
    assert!(!connector.contains("draw:type"), "a standard connector leaves it out");
    // Read back: the connector is glued to the rectangle by the begin and to the first shape by the end, by the rows the model had.
    let back = open_bytes(saved.bytes);
    let page = back.doc.pages.get(0).unwrap();
    let ids: Vec<u32> = page.shapes.iter().map(|s| s.id).collect();
    assert_eq!(ids.len(), 3);
    let glued: Vec<(u32, Option<String>, u32, Option<u32>)> =
        page.connects.iter().map(|k| (k.from_sheet, k.from_cell.as_deref().map(str::to_owned), k.to_sheet, k.to_part)).collect();
    assert!(glued.contains(&(ids[2], Some("BeginX".to_owned()), ids[1], Some(101))), "{glued:?}");
    assert!(glued.contains(&(ids[2], Some("EndX".to_owned()), ids[0], Some(103))), "{glued:?}");
}

type Case = (&'static str, fn(&Saver));

const CASES: [Case; 2] = [
    ("new_rectangle_is_written_as_libreoffice_writes_one", new_rectangle_case),
    ("new_connector_glued_to_new_shape_reads_back_glued", new_connector_case),
];

#[test]
fn new_rectangle_is_written_as_libreoffice_writes_one() {
    new_rectangle_case(&real);
}

#[test]
fn new_connector_glued_to_new_shape_reads_back_glued() {
    new_connector_case(&real);
}

#[test]
fn echo_control_fails_every_new_shape_case() {
    crate::write_core::echo_control(&CASES);
}

#[test]
fn a_new_shape_that_cannot_be_written_is_refused_in_words_and_nothing_is_written() {
    let base = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = base.index.next_id(0).unwrap();
    let refused = |mutate: &dyn Fn(&mut Shape), words: &str| {
        let mut doc = base.doc.clone();
        let mut shape = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
        mutate(&mut shape);
        push_shape(&mut doc, shape);
        match real(&doc, &base.bytes, &base.index) {
            Err(WriteError::Unsupported(what)) => assert!(what.contains(words), "{what}"),
            other => panic!("expected a refusal of {words}: {other:?}"),
        }
    };
    // (A shape that is turned or flipped is no longer among them: it is written, `write_turned.rs`.)
    refused(
        &|s| {
            Arc::make_mut(&mut s.sheet).links.fill = Some(3);
        },
        "style sheet",
    );
    refused(
        &|s| {
            s.text = Some(Arc::new(diacraft_doc::Text {
                parts: vec![diacraft_doc::TextPart::Chars(Arc::from("x".repeat((16 << 20) + 1)))],
                ..Default::default()
            }));
        },
        "16 MiB",
    );
    // A shape that holds members is written as a group, and a group's own text is refused (it would be one more member, which is not written yet). (Its frame that is
    // not the box of its members is no refusal: the group is written as its members and said in a note, `write_groups.rs`.)
    refused(
        &|s| {
            s.children.push(Arc::new(Shape::new(99)));
            s.text = Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from("x"))], ..Default::default() }));
        },
        "group",
    );
}

/// What a new shape holds that the file's own markup cannot say is tier 2: it is saved, the shape's own style carries it, and the file read
/// again gives the shape's cells back. Each of what was refused as "not tier 1" before is carried now.
#[test]
fn a_new_shape_that_is_not_tier_1_carries_the_difference_as_tier_2() {
    let base = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = base.index.next_id(0).unwrap();
    let carried = |mutate: &dyn Fn(&mut Shape), check: &dyn Fn(&Shape)| {
        let mut doc = base.doc.clone();
        let mut shape = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
        mutate(&mut shape);
        push_shape(&mut doc, shape);
        let saved = real(&doc, &base.bytes, &base.index).unwrap_or_else(|e| panic!("not saved: {e}"));
        assert!(part(&saved.bytes, "content.xml").contains(" diacraft:version=\"1\""));
        let back = open_bytes(saved.bytes);
        let read = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().clone();
        check(&read);
    };
    carried(
        &|s| {
            Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("Comment"), cell(None, 1.0, None));
        },
        &|r| assert!(r.sheet.cell("Comment").is_some()),
    );
    carried(
        &|s| {
            Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("LayerMember"), cell(None, 0.0, None));
        },
        &|r| assert!(r.sheet.cell("LayerMember").is_some()),
    );
    carried(
        &|s| {
            s.text = Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Cp(1)], ..Default::default() }));
        },
        &|r| assert_eq!(r.text.as_ref().map(|t| t.parts.clone()), Some(vec![diacraft_doc::TextPart::Cp(1)])),
    );
    carried(
        &|s| {
            Arc::make_mut(&mut s.sheet).sections.retain(|x| x.name.as_str() != "Connection");
        },
        &|r| assert!(r.sheet.section(&diacraft_doc::SectionKey::new("Connection", None)).is_none()),
    );
    carried(&|s| s.master = Some(1), &|r| assert_eq!(r.master, Some(1)));
    carried(&|s| s.master_shape = Some(1), &|r| assert_eq!(r.master_shape, Some(1)));
    // Connection rows that are not the four side middles: one that has the right x and the wrong y, and the reverse.
    for (cell_name, formula) in [("Y", "Height*0.75"), ("X", "Width*0.75")] {
        carried(
            &|s| {
                let sheet = Arc::make_mut(&mut s.sheet);
                let section = sheet.sections.iter_mut().find(|x| x.name.as_str() == "Connection").unwrap();
                let first = Arc::make_mut(section).rows.iter_mut().next().unwrap();
                Arc::make_mut(first).cells.insert(CellName::new(cell_name), cell(Some(formula), 0.0, Some("IN")));
            },
            &|r| {
                let section = r.sheet.section(&diacraft_doc::SectionKey::new("Connection", None)).unwrap();
                let first = section.rows.iter().next().unwrap();
                assert_eq!(first.cells.get(cell_name).and_then(|c| c.formula.text()), Some(formula));
            },
        );
    }
    carried(&|s| Arc::make_mut(&mut s.sheet).sections.push(Arc::new(Section::new("User", None))), &|r| {
        assert!(r.sheet.section(&diacraft_doc::SectionKey::new("User", None)).is_some())
    });
    // A page whose shapes carry `draw:z-index` takes a new shape now, written with the attribute (`write_order.rs`).
}

/// A connector to a shape of the file that has an id already names that id, and the shape is not given another (a second `draw:id` on its tag).
#[test]
fn a_new_connector_names_the_id_an_existing_shape_already_has_and_gives_it_no_other() {
    let shape = r#"<draw:custom-shape draw:name="s1" xml:id="keep" draw:id="keep" svg:x="1cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#;
    let mut o = open(&Drawing::a4().page(shape));
    let c = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, connector(c, (10.0, PAGE_HEIGHT_CM - 4.0, 5.0, PAGE_HEIGHT_CM - 4.25)));
    glue(&mut o.doc, c, false, 1, 3);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains(shape), "the shape's tag is as it was: {after}");
    let connector = text_of(&after, "connector");
    assert!(connector.contains(r#"draw:end-shape="keep" draw:end-glue-point="1""#), "{connector}");
    assert_eq!(after.matches("draw:id=").count(), 1, "{after}");
}

/// The page's own bindings give the prefixes of a new element, and not the declarations on the start tag of the shape it follows: the root binds
/// `svg` to the namespace of the W3C, that shape binds it to ODF's for itself alone, and the new element, outside it, still needs a prefix of its own.
#[test]
fn a_new_shape_takes_the_prefixes_the_page_binds_and_not_those_a_shape_before_it_declares() {
    const ODF_SVG: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
    let shape = format!(
        r#"<draw:custom-shape xmlns:svg="{ODF_SVG}" draw:name="s1" svg:x="1cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#
    );
    let mut d = Drawing::a4().page(&shape);
    d.ns = d.ns.replace(ODF_SVG, "http://www.w3.org/2000/svg");
    let mut o = open(&d);
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, rectangle(id, "Rectangle.1", (2.0, 3.0, 4.0, 2.0)));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    let new = after.split("</draw:custom-shape>").nth(1).unwrap();
    assert!(new.starts_with(&format!("<draw:custom-shape xmlns:svg1=\"{ODF_SVG}\" ")), "{new}");
    assert!(new.contains(" svg1:width=\"4cm\"") && new.contains(" svg1:viewBox=") && !new.contains(" svg:"), "{new}");
    let back = open_bytes(saved.bytes);
    assert_eq!(back.doc.pages.get(0).unwrap().shapes.len(), 2);
}

/// A margin is written as the four paddings of the new shape's graphic style, with the others left out.
#[test]
fn a_new_shape_with_a_margin_has_it_as_a_padding_of_its_style() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    let mut shape = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("LeftMargin"), cell(None, cm(0.5), Some("IN")));
    push_shape(&mut o.doc, shape);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    let graphic = after.split("<style:style style:name=\"gr1\"").nth(1).unwrap().split("</style:style>").next().unwrap();
    assert!(graphic.contains(" fo:padding-left=\"0.5cm\""), "{graphic}");
    assert!(!graphic.contains("padding-top") && !graphic.contains("fo:padding="), "{graphic}");
}

/// The text of a new shape may be as long as a retext's bound, 16 MiB, and is refused one byte past it.
#[test]
fn a_new_shape_with_the_longest_text_is_written_and_one_byte_more_is_refused() {
    let o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    let save = |bytes: usize| {
        let mut doc = o.doc.clone();
        let mut shape = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
        shape.text =
            Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from("x".repeat(bytes)))], ..Default::default() }));
        push_shape(&mut doc, shape);
        real(&doc, &o.bytes, &o.index)
    };
    let saved = save(16 << 20).unwrap();
    assert!(part(&saved.bytes, "content.xml").contains(&format!("<text:p>{}</text:p>", "x".repeat(16 << 20))));
    assert!(matches!(save((16 << 20) + 1), Err(WriteError::Unsupported(what)) if what.contains("16 MiB")));
}

/// The shown text of a field counts toward the longest text of a new shape, as characters do: a text of more than 16 MiB is refused in the words of that bound
/// whether the bytes are characters, a field's shown text, or both (a text with a field is carried by the extension, whose attribute holds far less, so the
/// refusal that is not the bound's would be another in other words).
#[test]
fn the_shown_text_of_a_field_counts_toward_the_longest_text_of_a_new_shape() {
    let o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    let save = |chars: usize, field: usize| {
        let mut doc = o.doc.clone();
        let mut shape = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
        let parts = vec![
            diacraft_doc::TextPart::Chars(Arc::from("x".repeat(chars))),
            diacraft_doc::TextPart::Fld { ix: 0, text: Arc::from("y".repeat(field)) },
        ];
        shape.text = Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() }));
        push_shape(&mut doc, shape);
        real(&doc, &o.bytes, &o.index)
    };
    let half = 8 << 20;
    assert!(matches!(save(half, half + 1), Err(WriteError::Unsupported(what)) if what.contains("16 MiB")), "half and half, a byte over");
    assert!(matches!(save(0, (16 << 20) + 1), Err(WriteError::Unsupported(what)) if what.contains("16 MiB")), "a field alone");
}

/// The text of a new shape is its paragraphs as R24 writes them: a line break between paragraphs, an empty one as `text:p/`, a second space as
/// `text:s` and a tab as `text:tab`; the style of a shape with text does not grow it.
#[test]
fn a_new_shape_with_text_has_a_paragraph_for_each_line_and_its_white_space_as_elements() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    let mut shape = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
    shape.text =
        Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from("a\n\nb  c\td & <e>"))], ..Default::default() }));
    push_shape(&mut o.doc, shape);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    let new = after.split("</draw:custom-shape>").nth(1).unwrap();
    assert!(new.contains("<text:p>a</text:p><text:p/><text:p>b <text:s/>c<text:tab/>d &amp; &lt;e&gt;</text:p><draw:enhanced-geometry"), "{new}");
    let graphic = after.split("<style:style style:name=\"gr1\"").nth(1).unwrap().split("</style:style>").next().unwrap();
    assert!(graphic.contains("draw:auto-grow-height=\"false\""), "{graphic}");
    let back = open_bytes(saved.bytes);
    let read = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
    let chars: String =
        read.text.as_ref().unwrap().parts.iter().filter_map(|p| if let diacraft_doc::TextPart::Chars(c) = p { Some(&**c) } else { None }).collect();
    assert_eq!(chars, "a\n\nb  c\td & <e>");
}

/// A new connector that cannot be written is refused in words, each condition by itself, and a link of 0 or a route of another kind is no refusal. What a
/// new connector holds that its markup cannot say (a layer, a master, a text, other sections, a route kind the standard's three types cannot say) is tier 2.
#[test]
fn a_new_connector_that_cannot_be_written_is_refused_in_words_and_nothing_is_written() {
    let base = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = base.index.next_id(0).unwrap();
    let attempt = |mutate: &dyn Fn(&mut Shape)| {
        let mut doc = base.doc.clone();
        let mut shape = connector(id, (10.0, PAGE_HEIGHT_CM - 4.0, 5.0, PAGE_HEIGHT_CM - 4.25));
        mutate(&mut shape);
        push_shape(&mut doc, shape);
        real(&doc, &base.bytes, &base.index)
    };
    let refused = |mutate: &dyn Fn(&mut Shape), words: &str| match attempt(mutate) {
        Err(WriteError::Unsupported(what)) => assert!(what.contains(words), "{what}"),
        other => panic!("expected a refusal of {words}: {other:?}"),
    };
    refused(&|s| Arc::make_mut(&mut s.sheet).links.line = Some(3), "style sheet");
    refused(&|s| Arc::make_mut(&mut s.sheet).links.fill = Some(3), "style sheet");
    refused(&|s| Arc::make_mut(&mut s.sheet).links.text = Some(3), "style sheet");
    refused(&|s| s.children.push(Arc::new(Shape::new(99))), "");
    // A layer, a master, a text and other sections are carried, and read back.
    let carried = |mutate: &dyn Fn(&mut Shape), check: &dyn Fn(&Shape)| {
        let saved = attempt(mutate).unwrap_or_else(|e| panic!("not saved: {e}"));
        assert!(part(&saved.bytes, "content.xml").contains(" diacraft:version=\"1\""));
        let back = open_bytes(saved.bytes);
        let read = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().clone();
        check(&read);
    };
    carried(
        &|s| {
            Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("LayerMember"), cell(None, 0.0, None));
        },
        &|r| assert!(r.sheet.cell("LayerMember").is_some()),
    );
    carried(&|s| s.master = Some(1), &|r| assert_eq!(r.master, Some(1)));
    // A text is the file's own (`text:p` in the `draw:connector`), so it needs no DiaCraft attribute and is read back all the same.
    let words = |s: &mut Shape| {
        s.text = Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from("x"))], ..Default::default() }))
    };
    let saved = attempt(&words).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(part(&saved.bytes, "content.xml").contains("<text:p>x</text:p>"));
    let back = open_bytes(saved.bytes);
    assert_eq!(back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().text.as_ref().map(|t| t.parts.len()), Some(1));
    carried(
        &|s| {
            Arc::make_mut(&mut s.sheet).sections.push(Arc::new(Section::new("Control", None)));
        },
        &|r| assert!(r.sheet.section(&diacraft_doc::SectionKey::new("Control", None)).is_some()),
    );
    carried(
        &|s| {
            let mut other = Row { name: Some(Arc::from("Other")), ..Row::default() };
            other.cells.insert(CellName::new("Value"), cell(None, 1.0, None));
            let sheet = Arc::make_mut(&mut s.sheet);
            let user = sheet.sections.iter_mut().find(|x| x.name.as_str() == "User").unwrap();
            Arc::make_mut(user).rows.push(Arc::new(other));
        },
        &|r| {
            let user = r.sheet.section(&diacraft_doc::SectionKey::new("User", None)).unwrap();
            assert!(user.rows.iter().any(|row| row.name.as_deref() == Some("Other")));
        },
    );
    // A route kind the standard's three types cannot say (it was refused, "a new connector of a route kind the writer does not write") is written as the default type, and
    // the cells that say it are carried and read back; so is a shape with one of the two route cells (a connector: a line has neither), the other being gone.
    carried(
        &|s| {
            Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("ShapeRouteStyle"), cell(None, 3.0, None));
        },
        &|r| {
            let number = |name: &str| r.sheet.cell(name).and_then(|c| c.value.as_ref()?.number());
            assert_eq!((number("ShapeRouteStyle"), number("ConLineRouteExt")), (Some(3.0), Some(1.0)));
        },
    );
    for gone in ["ShapeRouteStyle", "ConLineRouteExt"] {
        carried(
            &|s| {
                Arc::make_mut(&mut s.sheet).cells.remove(gone);
            },
            &|r| {
                assert!(r.sheet.cell(gone).is_none(), "{gone} is gone, as the drawing had it");
                let other = if gone == "ShapeRouteStyle" { "ConLineRouteExt" } else { "ShapeRouteStyle" };
                assert!(r.sheet.cell(other).is_some(), "{other} stays");
            },
        );
    }
    // Links that are 0 are no links; and the routes that are lines and curves are written as the type they are.
    let saved = attempt(&|s| {
        let links = &mut Arc::make_mut(&mut s.sheet).links;
        (links.line, links.fill, links.text) = (Some(0), Some(0), Some(0));
    })
    .unwrap();
    assert!(text_of(&part(&saved.bytes, "content.xml"), "connector").contains("<draw:connector "));
    for (route, ext, written) in [(2.0, 1.0, "line"), (1.0, 2.0, "curve")] {
        let saved = attempt(&|s| {
            let cells = &mut Arc::make_mut(&mut s.sheet).cells;
            cells.insert(CellName::new("ShapeRouteStyle"), cell(None, route, None));
            cells.insert(CellName::new("ConLineRouteExt"), cell(None, ext, None));
        })
        .unwrap();
        let connector = text_of(&part(&saved.bytes, "content.xml"), "connector");
        assert!(connector.contains(&format!(" draw:type=\"{written}\"")), "{connector}");
    }
}

/// An end glued to a whole shape names it and no glue point; a part that is neither the whole shape nor a connection row is refused; a connection
/// row is a glue id by DiaCraft's order of the four sides (rows 0 to 3 are glue ids 0, 3, 2 and 1).
#[test]
fn a_new_connector_glued_to_a_whole_shape_has_no_glue_point_and_the_rows_map_to_glue_ids() {
    let o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let c = o.index.next_id(0).unwrap();
    let build = |part: u32| {
        let mut doc = o.doc.clone();
        push_shape(&mut doc, connector(c, (10.0, PAGE_HEIGHT_CM - 4.0, 5.0, PAGE_HEIGHT_CM - 4.25)));
        glue_part(&mut doc, c, false, 1, part);
        real(&doc, &o.bytes, &o.index)
    };
    let whole = text_of(&part(&build(3).unwrap().bytes, "content.xml"), "connector");
    assert!(whole.contains(r#"draw:end-shape="id1""#) && !whole.contains("glue-point"), "{whole}");
    assert!(matches!(build(5), Err(WriteError::Unsupported(what)) if what.contains("a kind the writer does not know")));
    for (part_number, glue_id) in [(100, "0"), (101, "3"), (102, "2"), (103, "1"), (104, "4"), (110, "10")] {
        let connector = text_of(&part(&build(part_number).unwrap().bytes, "content.xml"), "connector");
        assert!(connector.contains(&format!(r#"draw:end-glue-point="{glue_id}""#)), "part {part_number}: {connector}");
    }
}

/// A new line drawn with the line tool, a 1-D shape with no route cells, is a
/// `draw:line` holding its two ends flipped by R2, and not a `draw:connector`. It has its layer, its name, an empty `text:p`, a new automatic
/// graphic style and paragraph style, no geometry element and no `draw:auto-grow-height`; it reads back with the ends it was made with, and a second
/// save of the model is no edit.
#[test]
fn a_new_line_is_written_as_a_draw_line_holding_its_two_ends() {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    let mut line = connector(id, (2.0, PAGE_HEIGHT_CM - 4.0, 6.0, PAGE_HEIGHT_CM - 4.0));
    {
        let sheet = Arc::make_mut(&mut line.sheet);
        sheet.cells.remove("ShapeRouteStyle");
        sheet.cells.remove("ConLineRouteExt");
        sheet.sections.retain(|s| s.name.as_str() == "Geometry");
    }
    line.name_u = Some(Arc::from("Line.1"));
    push_shape(&mut o.doc, line);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save of a new line was refused: {e}"));
    let after = part(&saved.bytes, "content.xml");
    let element = text_of(&after, "line");
    assert!(
        element.starts_with(r#"<draw:line draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="Line.1" "#),
        "{element}"
    );
    assert!(element.contains(r#"svg:x1="2cm" svg:y1="4cm" svg:x2="6cm" svg:y2="4cm""#), "{element}");
    assert!(element.ends_with("<text:p/></draw:line>") && !element.contains("viewBox") && !element.contains("svg:d="), "{element}");
    assert!(
        !after.contains("<draw:connector") && after.matches("<draw:custom-shape ").count() == 1,
        "no connector and no custom shape for it: {after}"
    );
    assert!(!after.contains("auto-grow-height"), "R48 asks that of custom shapes only: {after}");
    let back = open_bytes(saved.bytes.clone());
    let read = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
    let cm_of = |name: &str| read.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap() / cm(1.0);
    let ends = [cm_of("BeginX"), cm_of("BeginY"), cm_of("EndX"), cm_of("EndY")];
    assert!((ends[0] - 2.0).abs() < 1e-6 && (ends[1] - (PAGE_HEIGHT_CM - 4.0)).abs() < 1e-6 && (ends[2] - 6.0).abs() < 1e-6, "{ends:?}");
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
    // The same shape with a route's cells is a connector, and so is a line glued to a shape (it was refused in words, "a new line glued to a shape").
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, connector(id, (2.0, PAGE_HEIGHT_CM - 4.0, 6.0, PAGE_HEIGHT_CM - 4.0)));
    assert!(part(&real(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml").contains("<draw:connector "));
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    let mut line = connector(id, (2.0, PAGE_HEIGHT_CM - 4.0, 6.0, PAGE_HEIGHT_CM - 4.0));
    {
        let sheet = Arc::make_mut(&mut line.sheet);
        sheet.cells.remove("ShapeRouteStyle");
        sheet.cells.remove("ConLineRouteExt");
        sheet.sections.retain(|s| s.name.as_str() == "Geometry");
    }
    push_shape(&mut o.doc, line);
    glue(&mut o.doc, id, true, 1, 3);
    let glued = real(&o.doc, &o.bytes, &o.index).expect("a line glued to a shape is written");
    let after = part(&glued.bytes, "content.xml");
    assert!(after.contains("<draw:connector ") && after.contains("draw:start-shape="), "a glued line is a connector glued to the shape: {after}");
    assert!(!after.contains("<draw:line "), "{after}");
}

/// A new line as the line tool makes one: a connector's shape without its route cells or its sections beside the geometry.
pub fn a_line(id: u32) -> Shape {
    let mut line = connector(id, (2.0, PAGE_HEIGHT_CM - 4.0, 6.0, PAGE_HEIGHT_CM - 4.0));
    let sheet = Arc::make_mut(&mut line.sheet);
    sheet.cells.remove("ShapeRouteStyle");
    sheet.cells.remove("ConLineRouteExt");
    sheet.sections.retain(|s| s.name.as_str() == "Geometry");
    line
}

/// What a `draw:line` the writer makes cannot hold is refused in words, each by itself: a link to a style sheet in any of the three families, a layer,
/// a master, children, text, a section beside the geometry, ends that are no numbers. A link of 0 (no style sheet) and nothing else is written.
#[test]
fn a_new_line_that_holds_what_a_draw_line_made_here_cannot_is_refused_each_by_itself() {
    let try_line = |change: &dyn Fn(&mut Shape)| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let id = o.index.next_id(0).unwrap();
        let mut line = a_line(id);
        change(&mut line);
        push_shape(&mut o.doc, line);
        real(&o.doc, &o.bytes, &o.index)
    };
    let refused = |what: &str, words: &str, saved: Result<diacraft_odg::Written, WriteError>| match saved {
        Err(e @ WriteError::Unsupported(found)) => {
            assert_eq!(e.class(), "write_unsupported", "{what}");
            assert!(found.contains(words), "{what}: {found}");
        }
        other => panic!("{what}: expected a refusal: {:?}", other.map(|w| w.edited)),
    };
    let sheet_words = "links name different style sheets";
    refused("a line style", sheet_words, try_line(&|s| Arc::make_mut(&mut s.sheet).links.line = Some(2)));
    refused("a fill style", sheet_words, try_line(&|s| Arc::make_mut(&mut s.sheet).links.fill = Some(2)));
    refused("a text style", sheet_words, try_line(&|s| Arc::make_mut(&mut s.sheet).links.text = Some(2)));
    // A group is refused before the line is looked at, as a new group (here for its text, which a group has no place for: a group's frame that is not the box of its
    // members is no refusal and a line with a member would be written as a group).
    refused(
        "a child and a text",
        "a new group",
        try_line(&|s| {
            s.children.push(Arc::new(rectangle(9, "R", (1.0, 1.0, 1.0, 1.0))));
            s.text = Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from("x"))], ..Default::default() }));
        }),
    );
    // A layer, a master, a text and another section are tier 2: the line is saved and each is read back.
    let carried = |what: &str, change: &dyn Fn(&mut Shape), check: &dyn Fn(&Shape)| {
        let saved = try_line(change).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert!(part(&saved.bytes, "content.xml").contains("<draw:line "), "{what}");
        let back = open_bytes(saved.bytes);
        let read = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().clone();
        check(&read);
    };
    carried(
        "a layer",
        &|s| {
            Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("LayerMember"), cell(None, 0.0, None));
        },
        &|r| assert!(r.sheet.cell("LayerMember").is_some()),
    );
    carried("a master", &|s| s.master = Some(3), &|r| assert_eq!(r.master, Some(3)));
    carried(
        "a text",
        &|s| s.text = Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from("x"))], ..Default::default() })),
        &|r| assert_eq!(r.text.as_ref().map(|t| t.parts.len()), Some(1)),
    );
    carried("a section", &|s| Arc::make_mut(&mut s.sheet).sections.push(Arc::new(Section::new("User", None))), &|r| {
        assert!(r.sheet.section(&diacraft_doc::SectionKey::new("User", None)).is_some())
    });
    refused(
        "ends",
        "ends are not numbers",
        try_line(&|s| {
            Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("BeginX"), Cell::constant(diacraft_doc::Value::from_text("x")));
        }),
    );
    // No style sheet (a link of 0) and nothing else is written as the line it is.
    let saved = try_line(&|s| {
        let links = &mut Arc::make_mut(&mut s.sheet).links;
        (links.line, links.fill, links.text) = (Some(0), Some(0), Some(0));
    })
    .unwrap_or_else(|e| panic!("a line with no style sheet was refused: {e}"));
    assert!(part(&saved.bytes, "content.xml").contains("<draw:line "));
}

/// A name with a character XML does not allow is refused in words, whichever of the two names holds it and whichever shape (a rectangle, a line, a connector)
/// it is.
#[test]
fn a_new_shape_whose_name_or_universal_name_holds_a_character_xml_cannot_hold_is_refused_for_each_alone() {
    let save = |kind: u8, name: Option<&str>, name_u: Option<&str>| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let id = o.index.next_id(0).unwrap();
        let mut shape = match kind {
            0 => rectangle(id, "R", (2.0, 3.0, 4.0, 2.0)),
            1 => a_line(id),
            _ => connector(id, (2.0, PAGE_HEIGHT_CM - 4.0, 6.0, PAGE_HEIGHT_CM - 4.0)),
        };
        shape.name = name.map(Arc::from);
        shape.name_u = name_u.map(Arc::from);
        push_shape(&mut o.doc, shape);
        real(&o.doc, &o.bytes, &o.index)
    };
    for kind in [0u8, 1, 2] {
        for (name, name_u) in [(Some("a\u{1}b"), Some("ok")), (Some("ok"), Some("a\u{1}b")), (None, Some("a\u{1}b")), (Some("a\u{1}b"), None)] {
            match save(kind, name, name_u) {
                Err(WriteError::Unsupported(words)) => {
                    assert!(words.contains("character XML 1.0 does not allow"), "shape {kind} {name:?} {name_u:?}: {words}")
                }
                other => panic!("shape {kind} {name:?} {name_u:?}: expected a refusal: {:?}", other.map(|w| w.edited)),
            }
        }
        assert!(save(kind, Some("ok"), Some("ok")).is_ok(), "shape {kind}: names that are fine are written");
    }
}

/// A formula of a new shape that names a shape of the file is refused, one that names a shape that is new or is nowhere is not.
#[test]
fn a_formula_of_a_new_shape_that_names_a_shape_of_the_file_is_refused_and_one_that_names_no_such_shape_or_a_new_one_is_not() {
    let save = |formula: &str, with_new: bool| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let file_id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
        let id = o.index.next_id(0).unwrap();
        let formula = formula.replace("FILE", &file_id.to_string()).replace("NEW", &id.to_string());
        let mut shape = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Ref"), Cell::read(Some(&formula), Some("1"), None, None, &mut 1_000));
        push_shape(&mut o.doc, shape);
        let _ = with_new;
        real(&o.doc, &o.bytes, &o.index)
    };
    match save("Sheet.FILE!Width", false) {
        Err(WriteError::Unsupported(words)) => assert!(words.contains("names a shape of the file"), "{words}"),
        other => panic!("a formula that names a shape of the file: {:?}", other.map(|w| w.edited)),
    }
    assert!(save("Sheet.NEW!Width", true).is_ok(), "a formula that names the new shape itself");
    assert!(save("Sheet.999!Width", false).is_ok(), "a formula that names a shape nowhere");
    assert!(save("Width*2", false).is_ok(), "a formula that names none");
}

/// A plain new line and a plain new connector that hold the four connection points the reader gives them carry no tier 2, so the file made is not read back to check it; with a user
/// cell each carries it, and is, and so is each that holds no connection points, which carries that it has none (the reader gives a segment the four too).
#[test]
fn a_new_line_and_a_new_connector_that_carry_tier_2_are_read_back_and_those_that_do_not_are_not() {
    let read_back = |kind: u8, user_cell: bool, points: bool| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let id = o.index.next_id(0).unwrap();
        let mut shape = connector(id, (2.0, PAGE_HEIGHT_CM - 4.0, 6.0, PAGE_HEIGHT_CM - 4.0));
        if kind == 1 {
            // A line as the reader gives one: its outline is a move to the origin and a line along the frame, and it has no route.
            let sheet = Arc::make_mut(&mut shape.sheet);
            sheet.cells.remove("ShapeRouteStyle");
            sheet.cells.remove("ConLineRouteExt");
            sheet.sections.clear();
            let mut geometry = Section::new("Geometry", Some(0));
            let mut start = Row { row_type: Some(Arc::from("MoveTo")), ix: Some(1), ..Row::default() };
            start.cells.insert(CellName::new("X"), cell(None, 0.0, Some("IN")));
            start.cells.insert(CellName::new("Y"), cell(None, 0.0, Some("IN")));
            let mut end = Row { row_type: Some(Arc::from("LineTo")), ix: Some(2), ..Row::default() };
            end.cells.insert(CellName::new("X"), cell(Some("Width*1"), 1.0, Some("IN")));
            end.cells.insert(CellName::new("Y"), cell(None, 0.0, Some("IN")));
            geometry.rows.push(Arc::new(start));
            geometry.rows.push(Arc::new(end));
            sheet.sections.push(Arc::new(geometry));
        }
        if user_cell {
            Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Comment"), Cell::constant(diacraft_doc::Value::from_text("mine")));
        }
        if points {
            shape = with_side_middles(shape);
        }
        push_shape(&mut o.doc, shape);
        real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("kind {kind}: {e}")).work.read_back_bytes
    };
    for (kind, name) in [(1u8, "a line"), (2, "a connector")] {
        assert_eq!(read_back(kind, false, true), 0, "{name} with nothing the markup cannot say is not read back");
        assert!(read_back(kind, true, true) > 0, "{name} with a user cell is read back");
        assert!(read_back(kind, false, false) > 0, "{name} with no connection points carries that it has none, and is read back");
    }
}

/// The ends of a new connector are the reader's, from its glue, where they are glued, and the connector's own where they are not: a formula in a glued end is
/// the glue's (not carried, though it names a shape of the file), and a formula in an end that is not glued is carried.
#[test]
fn the_ends_of_a_new_connector_are_rebuilt_from_the_glue_only_where_they_are_glued() {
    let save = |end_formula: Option<&str>| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let file_id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
        let id = o.index.next_id(0).unwrap();
        let mut c = connector(id, (2.0, PAGE_HEIGHT_CM - 4.0, 6.0, PAGE_HEIGHT_CM - 4.0));
        let glue = format!("PAR(PNT(Sheet.{file_id}!Connections.X1,Sheet.{file_id}!Connections.Y1))");
        let sheet = Arc::make_mut(&mut c.sheet);
        sheet.cells.insert(CellName::new("BeginX"), cell(Some(&glue), cm(2.0), Some("IN")));
        sheet.cells.insert(CellName::new("BeginY"), cell(Some(&glue), cm(PAGE_HEIGHT_CM - 4.0), Some("IN")));
        if let Some(f) = end_formula {
            sheet.cells.insert(CellName::new("EndX"), cell(Some(f), cm(6.0), Some("IN")));
        }
        push_shape(&mut o.doc, c);
        glue_to(&mut o.doc, id, file_id);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save of a glued connector: {e}"));
        part(&saved.bytes, "content.xml")
    };
    let glued = save(None);
    assert!(glued.contains("draw:start-shape=") && !glued.contains("diacraft:cells") && !glued.contains("BeginX"), "{glued}");
    let free = save(Some("BeginX+4*2.54/2.54"));
    assert!(free.contains("diacraft:cells") && free.contains("EndX"), "an end that is not glued keeps its formula: {free}");
    assert!(!free.contains("BeginX&quot;"), "and the glued one is still the glue's: {free}");
}

fn glue_to(doc: &mut Document, from: u32, to: u32) {
    glue(doc, from, true, to, 0);
}

/// A new line that holds no `Connection` section has it gone, since the reader gives a line the four standard connection points as it gives every shape (the line came back with four);
/// one that holds those four carries nothing of them; one that holds another `Connection` section carries it.
#[test]
fn a_new_line_carries_a_connection_section_it_holds_and_has_it_gone_when_it_holds_none() {
    let style_of_new = |change: &dyn Fn(&mut Shape)| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let id = o.index.next_id(0).unwrap();
        let mut line = a_line(id);
        change(&mut line);
        push_shape(&mut o.doc, line);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save of a line: {e}"));
        part(&saved.bytes, "content.xml")
    };
    let plain = style_of_new(&|_| {});
    assert!(
        plain.contains("diacraft:gone=\"s.Connection._\""),
        "the reader gives a line four connection points, so a line with none has them gone: {plain}"
    );
    let side_middles = style_of_new(&|s| *s = with_side_middles(s.clone()));
    assert!(
        !side_middles.contains("diacraft:gone") && !side_middles.contains("Connection"),
        "the four the reader gives are not carried: {side_middles}"
    );
    let with = style_of_new(&|s| {
        let mut section = Section::new("Connection", None);
        section.rows.push(row("RowType", 0, &[("X", None, 0.0), ("Y", None, 0.0)]));
        Arc::make_mut(&mut s.sheet).sections.push(Arc::new(section));
    });
    assert!(with.contains("diacraft:s-Connection"), "a connection section the line holds is carried: {with}");
}

/// Closed for the VSDX side: a new shape that is a copy of one the VSDX reader read holds attributes and elements of the file's own, shape data and foreign data, none of
/// which a new shape of this writer can write as markup. They ride in the shape's own style, the save says nothing is left out, and a read gives them back.
#[test]
fn a_new_shape_that_holds_what_a_new_shape_cannot_write_carries_it_and_says_nothing_left_out() {
    use diacraft_doc::{ForeignData, Passthrough, Placed, RawAttrs, RawElement, RawNode};
    let element = |name: &str| RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::new(), children: Vec::new() });
    let held = |attrs: &[(&str, &str)], nodes: Vec<RawNode>| {
        let mut shape = rectangle(1, "R", (2.0, 3.0, 4.0, 2.0));
        shape.raw = RawAttrs::from_pairs(attrs.iter().copied());
        shape.passthrough = Passthrough::from_nodes(nodes.into_iter().map(|node| Placed { after: 0, node }).collect());
        shape
    };
    let save_of = |shape: Shape| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let id = o.index.next_id(0).unwrap();
        push_shape(&mut o.doc, Shape { id, ..shape });
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save of a copy was refused: {e}"));
        (o, saved, id)
    };
    let mut shape =
        held(&[("ext:note", "kept"), ("ext:more", "x")], vec![element("ext:data"), RawNode::Text(Arc::from("\n  ")), element("ext:other")]);
    shape.data2 = Some(Arc::from("kept"));
    shape.foreign = Some(Arc::new(ForeignData::default()));
    let (o, saved, id) = save_of(shape);
    assert!(saved.notes.iter().all(|n| n.class() != "new_shape_unwritten"), "{:?}", saved.notes);
    let (back, _) = diacraft_odg::read(&saved.bytes, &diacraft_odg::Limits::default()).unwrap();
    let wanted = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == id).unwrap();
    let got = back.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some("R")).unwrap();
    assert_eq!(got.raw, wanted.raw);
    assert_eq!(got.passthrough, wanted.passthrough);
    assert_eq!(got.data2, wanted.data2);
    assert_eq!(got.foreign.as_deref().map(|f| (f.attrs.len(), f.picture.is_none())), Some((0, true)));
    // Saved again through the index it gave, the same model is no edit and says nothing again.
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.notes.is_empty() && again.bytes == saved.bytes, "{:?}", again.notes);
    // Each kind of thing rides by itself, with no note.
    let with_data = |f: &dyn Fn(&mut Shape)| {
        let mut s = held(&[], vec![]);
        f(&mut s);
        s
    };
    let cases: Vec<(&str, Shape)> = vec![
        ("an attribute", held(&[("ext:note", "x")], vec![])),
        ("an element", held(&[], vec![element("ext:data")])),
        ("data", with_data(&|s| s.data1 = Some(Arc::from("d")))),
        ("foreign data", with_data(&|s| s.foreign = Some(Arc::new(ForeignData::default())))),
        ("an attribute of the sheet", with_data(&|s| Arc::make_mut(&mut s.sheet).raw = RawAttrs::from_pairs([("ext:u", "1")]))),
        (
            "an element of the sheet",
            with_data(&|s| Arc::make_mut(&mut s.sheet).passthrough = Passthrough::from_nodes(vec![Placed { after: 0, node: element("ext:t") }])),
        ),
    ];
    for (case, shape) in cases {
        let (_, saved, _) = save_of(shape);
        assert!(saved.notes.is_empty(), "{case}: {:?}", saved.notes);
    }
    // A comment between the elements rides too.
    assert!(save_of(held(&[], vec![RawNode::Comment(Arc::from("c"))])).1.notes.is_empty());
}

// ----: a part whose `office:automatic-styles` is an empty element (found beside the review) -------------------------------------------

/// `content.xml`, as a package, with its (empty) `office:automatic-styles` written as `tag`.
fn with_empty_auto_styles(tag: &str) -> Vec<u8> {
    let d = Drawing::a4().page(&custom("s1", "1cm"));
    let files: Vec<(String, Vec<u8>)> = d
        .files()
        .into_iter()
        .map(|(name, bytes)| {
            if name == "content.xml" {
                let text = String::from_utf8(bytes).unwrap();
                assert!(text.contains("<office:automatic-styles></office:automatic-styles>"), "the builder's empty container");
                (name, text.replace("<office:automatic-styles></office:automatic-styles>", tag).into_bytes())
            } else {
                (name, bytes)
            }
        })
        .collect();
    crate::build::package(crate::build::DRAWING, files)
}

/// A new shape writes two styles (the graphic style and the paragraph style) and so puts two into the container: into an empty `<office:automatic-styles/>` that
/// was two edits of the same two bytes, refused as "two edits change the same bytes" (a wrong word for a file that is nothing wrong). The container is opened
/// once, holds both in the order they were made, and is closed once, whichever way the empty element is written.
#[test]
fn a_new_shape_in_a_part_whose_automatic_styles_is_an_empty_element_is_written() {
    for tag in ["<office:automatic-styles/>", "<office:automatic-styles />", "<office:automatic-styles  />"] {
        let mut o = open_bytes(with_empty_auto_styles(tag));
        let id = o.index.next_id(0).unwrap();
        push_shape(&mut o.doc, rectangle(id, "Rectangle.1", (2.0, 3.0, 4.0, 2.0)));
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{tag}: {e}"));
        let after = part(&saved.bytes, "content.xml");
        assert_eq!(after.matches("<office:automatic-styles").count(), 1, "{tag}: one container: {after}");
        // The container's tag keeps whatever stood before its `/` (`<office:automatic-styles >` for `<office:automatic-styles />`): the one edit is the `/`.
        let (open, close) = (after.find("<office:automatic-styles").unwrap(), after.find("</office:automatic-styles>").unwrap());
        let container = &after[open..close];
        let inside = &container[container.find('>').unwrap() + 1..];
        assert!(
            inside.starts_with("<style:style style:name=\"gr1\"") && inside.ends_with("<style:style style:name=\"P1\" style:family=\"paragraph\"/>"),
            "{tag}: the graphic style and then the paragraph style, nothing else: {container}"
        );
        assert!(after.contains(&custom("s1", "1cm")), "{tag}: the shape that was there is as it was");
        // Read back, the new shape is as the model has it, and a second save is no edit.
        let back = open_bytes(saved.bytes.clone());
        let fill = back
            .doc
            .pages
            .get(0)
            .unwrap()
            .shapes
            .get(1)
            .unwrap()
            .sheet
            .cell("FillForegnd")
            .and_then(|c| c.value.as_ref())
            .map(|v| v.text().to_owned());
        assert_eq!(fill.as_deref(), Some("#336699"), "{tag}");
        let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
        assert!(!again.edited && again.bytes == saved.bytes, "{tag}: a second save is no edit");
    }
}

/// **One** style into the empty container (a shape that had none is given a fill, which is one new graphic style): the one edit that opens the container is made
/// by the first style and not by the second, which a single style never reaches.
#[test]
fn one_new_style_in_an_empty_automatic_styles_element_opens_and_closes_it() {
    let mut o = open_bytes(with_empty_auto_styles("<office:automatic-styles/>"));
    let shape = crate::write_core::shape_mut(&mut o.doc, 0, 1);
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("FillForegnd"), Cell::constant(diacraft_doc::Value::from_text("#336699")));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let (open, close) = (after.find("<office:automatic-styles").unwrap(), after.find("</office:automatic-styles>").unwrap());
    let container = &after[open..close];
    assert!(container.starts_with("<office:automatic-styles><style:style style:name=\"gr1\""), "opened: {container}");
    assert_eq!(container.matches("<style:style ").count(), 1, "one style: {container}");
    assert_eq!(after.matches("<office:automatic-styles").count(), 1);
    assert_eq!(open_bytes(saved.bytes).doc.pages.get(0).unwrap().shapes.len(), 1);
}

/// The same in a flat drawing, whose one container is written by the same editor.
#[test]
fn a_new_shape_in_a_flat_drawing_whose_automatic_styles_is_an_empty_element_is_written() {
    // The one container is the whole element: written empty it holds no page layout either (the reader says so and uses its own size).
    let mut flat = crate::flat::Flat::of(&Drawing::a4().page(&custom("s1", "1cm")));
    flat.auto = "<office:automatic-styles/>".to_owned();
    let bytes = flat.bytes();
    assert!(String::from_utf8_lossy(&bytes).contains("<office:automatic-styles/>"));
    let mut o = open_bytes(bytes);
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, rectangle(id, "Rectangle.1", (2.0, 3.0, 4.0, 2.0)));
    let saved = diacraft_odg::write_with(
        &o.doc,
        Some(&diacraft_odg::OriginalPackage { bytes: &o.bytes, index: &o.index }),
        &diacraft_odg::WriteOptions::default(),
    )
    .unwrap();
    let after = String::from_utf8(saved.bytes.clone()).unwrap();
    assert_eq!(after.matches("<office:automatic-styles").count(), 1, "one container: {after}");
    assert!(
        after.contains("<office:automatic-styles><style:style style:name=\"gr1\"")
            && after.contains("<style:style style:name=\"P1\" style:family=\"paragraph\"/></office:automatic-styles>"),
        "{after}"
    );
    let again = diacraft_odg::write_with(
        &o.doc,
        Some(&diacraft_odg::OriginalPackage { bytes: &saved.bytes, index: &saved.index }),
        &diacraft_odg::WriteOptions::default(),
    )
    .unwrap();
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}
