//! The writer's edits of an outline: a path edited
//! from 4 to 5 points, a custom shape's handle moved, a polygon's point moved. Each is checked with `diff_packages`: only the attribute that holds the
//! outline changes, on the element that holds it.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Row, SectionKey, Value};
use diacraft_odg::WriteError;

use crate::drawing::Drawing;
use crate::write_core::{Saver, compare, content_changes, open, open_bytes, part, real, shape_mut};

const FRAME: &str = r#"svg:x="1cm" svg:y="2cm" svg:width="4cm" svg:height="4cm""#;

fn id_of(doc: &Document, name: &str) -> u32 {
    doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some(name)).unwrap().id
}

/// The first `Geometry` section's rows of a shape on the first page.
fn rows(doc: &Document, id: u32) -> Vec<Arc<Row>> {
    let shape = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == id).unwrap();
    shape.sheet.section(&SectionKey::new("Geometry", Some(0))).unwrap().rows.iter().cloned().collect()
}

/// Sets a row's cell to a number.
fn set(row: &mut Arc<Row>, cell: &str, v: f64) {
    Arc::make_mut(row).cells.insert(CellName::new(cell), Cell::constant(Value::from_number(v).unwrap()));
}

/// Replaces the first `Geometry` section's rows of the shape.
fn replace_rows(doc: &mut Document, id: u32, rows: Vec<Arc<Row>>) {
    let shape = shape_mut(doc, 0, id);
    let sheet = Arc::make_mut(&mut shape.sheet);
    let section = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Geometry").unwrap();
    let section = Arc::make_mut(section);
    section.rows.clear();
    for r in rows {
        section.rows.push(r);
    }
}

/// A path of 4 points edited to 5: its `svg:d` is regenerated and nothing else of the file changes but the bookkeeping.
fn a_path_edited_from_4_to_5_points(save: &Saver) {
    let body = format!(
        r#"<draw:path draw:name="p" {FRAME} svg:viewBox="0 0 1000 1000" svg:d="M 0 0 L 1000 0 L 1000 1000 L 0 1000 Z"><text:p/></draw:path>"#
    );
    let mut o = open(&Drawing::a4().page(&body));
    let p = id_of(&o.doc, "p");
    let mut rs = rows(&o.doc, p);
    assert_eq!(rs.len(), 5, "a move and three lines and the line back to the start");
    // A fifth point after the third, half way across and below the shape's bottom.
    let mut new = rs[3].clone();
    set(&mut new, "X", 0.5);
    set(&mut new, "Y", -0.25);
    rs.insert(3, new);
    for (i, r) in rs.iter_mut().enumerate() {
        Arc::make_mut(r).ix = Some(i as u32 + 1);
    }
    replace_rows(&mut o.doc, p, rs);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    let expected =
        before.replacen(r#"svg:d="M 0 0 L 1000 0 L 1000 1000 L 0 1000 Z""#, r#"svg:d="M 0 0 L 1000 0 L 1000 1000 L 500 1250 L 0 1000 Z""#, 1);
    assert_ne!(expected, before);
    assert_eq!(after, expected, "the attribute, and nothing else of the part");
    let diff = compare(&o.bytes, &saved.bytes);
    let (changed, added, removed) = content_changes(&diff);
    assert_eq!((changed.len(), added, removed), (1, 0, 0), "{diff:?}");
    assert_eq!(changed[0].1, ["svg:d"], "the one attribute that holds the outline");
    // Read back: 6 rows, the new point in its place.
    let back = open_bytes(saved.bytes);
    let back_rows = rows(&back.doc, id_of(&back.doc, "p"));
    assert_eq!(back_rows.len(), 6);
    assert_eq!(back_rows[3].cells.get("X").and_then(|c| c.value.as_ref()?.number()), Some(0.5));
}

/// A polygon's point moved: `draw:points` is regenerated as integers in the view box.
fn a_polygon_point_moved(save: &Saver) {
    let body =
        format!(r#"<draw:polygon draw:name="g" {FRAME} svg:viewBox="0 0 1000 1000" draw:points="0,0 1000,0 500,1000"><text:p/></draw:polygon>"#);
    let mut o = open(&Drawing::a4().page(&body));
    let g = id_of(&o.doc, "g");
    let mut rs = rows(&o.doc, g);
    assert_eq!(rs.len(), 4, "three points and the line back to the first");
    // The third point to a quarter across and a quarter up.
    set(&mut rs[2], "X", 0.25);
    set(&mut rs[2], "Y", 0.25);
    replace_rows(&mut o.doc, g, rs);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    assert_eq!(after, before.replacen(r#"draw:points="0,0 1000,0 500,1000""#, r#"draw:points="0,0 1000,0 250,750""#, 1));
    let diff = compare(&o.bytes, &saved.bytes);
    let (changed, added, removed) = content_changes(&diff);
    assert_eq!((changed.len(), added, removed), (1, 0, 0), "{diff:?}");
    assert_eq!(changed[0].1, ["draw:points"]);
    let back = open_bytes(saved.bytes);
    assert_eq!(rows(&back.doc, id_of(&back.doc, "g"))[2].cells.get("X").and_then(|c| c.value.as_ref()?.number()), Some(0.25));
}

/// A custom shape's handle moved. The handle drives the modifier through the formula `Controls.Row_1.X/Width*21600+0`; its
/// drag changes the modifier's cached value and the handle's place, and the file's `draw:modifiers` is the one attribute that changes: the handle
/// element is kept, as the table says (a drag changes the modifiers, not the handle).
fn a_handle_moved(save: &Saver) {
    let body = format!(
        r#"<draw:custom-shape draw:name="h" {FRAME}><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:modifiers="5400 7" draw:enhanced-path="M 0 0 L $0 0 21600 21600 Z N"><draw:handle draw:handle-position-x="$0" draw:handle-position-y="top"/></draw:enhanced-geometry></draw:custom-shape>"#
    );
    let mut o = open(&Drawing::a4().page(&body));
    let h = id_of(&o.doc, "h");
    let value = |doc: &Document, section: &str, row: &str, cell: &str| -> Option<f64> {
        let s = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == h)?;
        let sec = s.sheet.sections.iter().find(|x| x.name.as_str() == section)?;
        sec.rows
            .iter()
            .find(|r| r.name.as_deref() == Some(row) || r.ix.map(|i| format!("Row_{i}")).as_deref() == Some(row))?
            .cells
            .get(cell)?
            .value
            .as_ref()?
            .number()
    };
    assert_eq!(value(&o.doc, "User", "ODFMod0", "Value"), Some(5400.0));
    // The drag, as a recalculation leaves the sheet: the handle's place and the modifier's cached value, formula unchanged.
    let width = {
        let s = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == h).unwrap();
        s.sheet.cell("Width").and_then(|c| c.value.as_ref()?.number()).unwrap()
    };
    // SECNAME: the section of the handles is `Control` (the formula spells it `Controls.`); a loop that matched another name would touch nothing and the case would pass
    // on the writer's carry of the whole section, so the rows it touched are counted.
    let (mut modifiers, mut handles) = (0, 0);
    {
        let shape = shape_mut(&mut o.doc, 0, h);
        let sheet = Arc::make_mut(&mut shape.sheet);
        for section in sheet.sections.iter_mut() {
            let section = Arc::make_mut(section);
            let name = section.name.as_str().to_owned();
            for row in section.rows.iter_mut() {
                let row = Arc::make_mut(row);
                if name == "User" && row.name.as_deref() == Some("ODFMod0") {
                    let value = row.cells.get_mut("Value").unwrap();
                    value.value = Value::from_number(2700.0);
                    modifiers += 1;
                }
                if name == "Control" {
                    row.cells.get_mut("X").unwrap().value = Value::from_number(width * 2700.0 / 21600.0);
                    handles += 1;
                }
            }
        }
    }
    assert_eq!((modifiers, handles), (1, 1), "the drag touched the modifier's row and the handle's row");
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    assert_eq!(after, before.replacen(r#"draw:modifiers="5400 7""#, r#"draw:modifiers="2700 7""#, 1), "the one modifier moved, the other as written");
    let diff = compare(&o.bytes, &saved.bytes);
    let (changed, added, removed) = content_changes(&diff);
    assert_eq!((changed.len(), added, removed), (1, 0, 0), "{diff:?}");
    assert_eq!(changed[0].1, ["draw:modifiers"]);
    assert!(changed[0].0.ends_with("draw:enhanced-geometry"), "{changed:?}");
}

type Case = (&'static str, fn(&Saver));

const CASES: [Case; 3] = [
    ("a_path_edited_from_4_to_5_points", a_path_edited_from_4_to_5_points),
    ("a_polygon_point_moved", a_polygon_point_moved),
    ("a_handle_moved", a_handle_moved),
];

#[test]
fn a_path_edited_from_4_to_5_points_test() {
    a_path_edited_from_4_to_5_points(&real);
}

#[test]
fn a_polygon_point_moved_test() {
    a_polygon_point_moved(&real);
}

#[test]
fn a_custom_shapes_handle_moved_test() {
    a_handle_moved(&real);
}

#[test]
fn echo_control_fails_every_outline_case() {
    crate::write_core::echo_control(&CASES);
}

#[test]
fn an_outline_the_writer_does_not_write_is_refused_in_words_and_nothing_is_written() {
    let refused = |doc: &Document, o: &crate::write_core::Open, words: &str| match real(doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(what)) => assert!(what.contains(words), "{what}"),
        other => panic!("expected a refusal of {words}: {other:?}"),
    };
    // A polygon point off the integer grid of the view box.
    let body =
        format!(r#"<draw:polygon draw:name="g" {FRAME} svg:viewBox="0 0 1000 1000" draw:points="0,0 1000,0 500,1000"><text:p/></draw:polygon>"#);
    let o = open(&Drawing::a4().page(&body));
    let g = id_of(&o.doc, "g");
    let mut doc = o.doc.clone();
    let mut rs = rows(&doc, g);
    set(&mut rs[2], "X", 0.3333333);
    replace_rows(&mut doc, g, rs);
    refused(&doc, &o, "integer grid");
    // A row that is not a plain line.
    let mut doc = o.doc.clone();
    let mut rs = rows(&doc, g);
    Arc::make_mut(&mut rs[1]).row_type = Some(Arc::from("RelEllipticalArcTo"));
    replace_rows(&mut doc, g, rs);
    refused(&doc, &o, "plain lines");
    // An outline of a rectangle (its corner radius is another attribute, and its form is its frame): refused, and not dropped.
    let body = format!(r#"<draw:rect draw:name="r" {FRAME}><text:p/></draw:rect>"#);
    let o_rect = open(&Drawing::a4().page(&body));
    let r = id_of(&o_rect.doc, "r");
    let mut doc = o_rect.doc.clone();
    let mut rs = rows(&doc, r);
    set(&mut rs[2], "X", 0.5);
    replace_rows(&mut doc, r, rs);
    refused(&doc, &o_rect, "outline of this kind of shape element");
    // A modifier of the file that is no number, beside one that a drag changes: refused in words, as the text the file kept would not read back (the fuzz target found the
    // refusal of the check of the bytes, which is for the writer's own faults).
    let body = format!(
        r#"<draw:custom-shape draw:name="m" {FRAME}><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:modifiers="5400 16200 x" draw:enhanced-path="M 0 0 L $0 $1 $2 0 Z N"><draw:handle draw:handle-position-x="$0" draw:handle-position-y="top"/></draw:enhanced-geometry></draw:custom-shape>"#
    );
    let o = open(&Drawing::a4().page(&body));
    let m = id_of(&o.doc, "m");
    let mut doc = o.doc.clone();
    {
        let shape = shape_mut(&mut doc, 0, m);
        let sheet = Arc::make_mut(&mut shape.sheet);
        let mut touched = 0;
        for section in sheet.sections.iter_mut().filter(|s| s.name.as_str() == "User") {
            for row in Arc::make_mut(section).rows.iter_mut().filter(|r| r.name.as_deref() == Some("ODFMod0")) {
                Arc::make_mut(row).cells.get_mut("Value").unwrap().value = Value::from_number(100.0);
                touched += 1;
            }
        }
        assert_eq!(touched, 1);
    }
    refused(&doc, &o, "of the file that is not a number is kept as it is written only when no other modifier is written");
    // A handle whose place changed with no modifier following it.
    let body = format!(
        r#"<draw:custom-shape draw:name="h" {FRAME}><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:modifiers="5400" draw:enhanced-path="M 0 0 L $0 0 21600 21600 Z N"><draw:handle draw:handle-position-x="$0" draw:handle-position-y="top"/></draw:enhanced-geometry></draw:custom-shape>"#
    );
    let o = open(&Drawing::a4().page(&body));
    let h = id_of(&o.doc, "h");
    let mut doc = o.doc.clone();
    {
        let shape = shape_mut(&mut doc, 0, h);
        let sheet = Arc::make_mut(&mut shape.sheet);
        let mut touched = 0;
        for section in sheet.sections.iter_mut().filter(|s| s.name.as_str() == "Control") {
            for row in Arc::make_mut(section).rows.iter_mut() {
                // The handle written as a constant (a drag of the handle alone, which a handle's cells as shares of the frame take the formula from); a value that only
                // moved under the formula is a recalculation's, and carries nothing.
                Arc::make_mut(row).cells.insert(CellName::new("X"), Cell::constant(Value::from_number(0.123).unwrap()));
                touched += 1;
            }
        }
        assert_eq!(touched, 1, "the reader's handle section is `Control`");
    }
    // It is carried by DiaCraft's extension: the handle's section is written whole, and reads back as the model has it.
    let saved = real(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("a handle moved with no modifier was refused: {e}"));
    let back = crate::write_core::open_bytes(saved.bytes);
    let shape = back.doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    let controls = shape.sheet.section(&diacraft_doc::SectionKey::new("Control", None)).unwrap();
    assert!(controls.rows.iter().all(|r| r.cells.get("X").and_then(|c| c.value.as_ref()?.number()) == Some(0.123)));
}
