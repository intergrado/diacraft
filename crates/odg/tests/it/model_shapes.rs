//! The shapes of a model written from nothing (`write_with(doc, None, ..)`): each refusal of the new-shape path that left, closed, and read back through the real reader.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Row, Section, Shape, Value};
use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::model_write::blank;
use crate::write_new::{connector, glue, push_shape};

/// The drawing `doc` written from the model alone and read again, with the bytes and the classes of the notes.
fn round(doc: &Document) -> (Document, Vec<u8>, Vec<&'static str>) {
    let written = write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the save is refused: {e}"));
    let (back, _) = read(&written.bytes, &Limits::default()).unwrap_or_else(|e| panic!("the file does not read: {e}"));
    let notes = written.notes.iter().map(diacraft_odg::SaveNote::class).collect();
    (back, written.bytes, notes)
}

fn content(bytes: &[u8]) -> String {
    let package = diacraft_odg::package::Package::read(bytes, &Limits::default()).unwrap();
    String::from_utf8(package.part("content.xml").unwrap().to_vec()).unwrap()
}

fn number(shape: &Shape, cell: &str) -> f64 {
    shape.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN)
}

/// A new line between two rectangles that `connector.add` made: the ends glued, and neither of the two route cells a connector of a file holds.
fn glued_line() -> Document {
    let mut doc = blank(2);
    let mut line = connector(3, (4.0, 4.0, 8.0, 4.0));
    let sheet = Arc::make_mut(&mut line.sheet);
    sheet.cells.remove("ShapeRouteStyle");
    sheet.cells.remove("ConLineRouteExt");
    sheet.sections.retain(|s| s.name.as_str() != "User");
    push_shape(&mut doc, line);
    glue(&mut doc, 3, true, 1, 3);
    glue(&mut doc, 3, false, 2, 1);
    doc
}

#[test]
fn a_new_line_glued_to_a_shape_is_written_as_a_connector_glued_to_it() {
    let (back, bytes, _) = round(&glued_line());
    let xml = content(&bytes);
    assert!(xml.contains("<draw:connector "), "a line that is glued is a connector: {xml}");
    assert!(!xml.contains("<draw:line "), "{xml}");
    let page = back.pages.get(0).unwrap();
    let from_line: Vec<_> = page.connects.iter().collect();
    assert_eq!(from_line.len(), 2, "both ends are glued when the file is read again");
    let (begin, end) = (xml.contains("draw:start-shape="), xml.contains("draw:end-shape="));
    assert!(begin && end, "{xml}");
}

/// A curved connector as the engine makes one: a `MoveTo` and one quadratic `NURBSTo` through a control point, in the connector's own frame.
fn curved() -> Document {
    let mut doc = blank(2);
    let mut curve = connector(3, (4.0, 4.0, 8.0, 6.0));
    let sheet = Arc::make_mut(&mut curve.sheet);
    sheet.cells.insert(CellName::new("ConLineRouteExt"), Cell::constant(Value::from_number(2.0).unwrap()));
    sheet.cells.insert(CellName::new("ShapeRouteStyle"), Cell::constant(Value::from_number(1.0).unwrap()));
    let length = number_of(sheet, "Width");
    let mut geometry = Section::new("Geometry", Some(0));
    let mut put = |kind: &str, ix: u32, cells: &[(&str, &str, Option<&str>)]| {
        let mut row = Row { row_type: Some(Arc::from(kind)), ix: Some(ix), ..Row::default() };
        for (name, text, unit) in cells {
            let mut cell = Cell::constant(Value::from_text(text));
            if let Some(unit) = unit {
                cell = cell.with_unit(unit);
            }
            row.cells.insert(CellName::new(name), cell);
        }
        geometry.rows.push(Arc::new(row));
    };
    put("MoveTo", 1, &[("X", "0", Some("IN")), ("Y", "0", Some("IN"))]);
    let end_x = format!("{length}");
    let via_x = format!("{}", length / 2.0);
    put(
        "NURBSTo",
        2,
        &[
            ("X", &end_x, Some("IN")),
            ("Y", "0", Some("IN")),
            ("A", "1", None),
            ("B", "1", None),
            ("C", "0", None),
            ("D", "1", None),
            ("E", &format!("NURBS(1,2,1,1,{via_x},0.5,0,1)"), Some("NURBS")),
        ],
    );
    sheet.sections.retain(|s| s.name.as_str() != "Geometry");
    sheet.sections.push(Arc::new(geometry));
    push_shape(&mut doc, curve);
    glue(&mut doc, 3, true, 1, 3);
    glue(&mut doc, 3, false, 2, 3);
    doc
}

fn number_of(sheet: &diacraft_doc::Sheet, name: &str) -> f64 {
    sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN)
}

#[test]
fn a_new_connector_whose_route_is_a_curve_is_written_as_a_path_and_its_own_rows_come_back() {
    let doc = curved();
    let (back, bytes, _) = round(&doc);
    let xml = content(&bytes);
    let at = xml.find("<draw:connector ").expect("a connector");
    let tag = &xml[at..xml[at..].find('>').map(|n| at + n).unwrap()];
    let d = tag.split("svg:d=\"").nth(1).and_then(|r| r.split('"').next()).expect("the route is a path");
    assert!(d.contains('c'), "a curve is written as cubics: {d}");
    // The rows the session has come back (tier 2), whatever the path says.
    let was: &Shape = doc.pages.get(0).unwrap().shapes.get(2).unwrap();
    let now: &Shape = back.pages.get(0).unwrap().shapes.get(2).unwrap();
    let rows = |s: &Shape| {
        s.sheet
            .sections
            .iter()
            .find(|g| g.name.as_str() == "Geometry" && g.ix == Some(0))
            .map(|g| g.rows.iter().map(|r| r.row_type.clone()).collect::<Vec<_>>())
    };
    assert_eq!(rows(was), rows(now));
    assert!((number(was, "EndX") - number(now, "EndX")).abs() < 1e-3);
}
