//! What the engine's `.odg` save listed as lost on a new shape (`crates/engine/src/odgfile/flatten.rs`, the `KINDS` rows) survives a save from the model alone, through the real
//! reader. Every shape is new to the model writer, so a loss here is a loss on every drawing that goes through `.diacraft`.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Row, Section, Shape, Value};
use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::model_write::blank;

/// The drawing `doc` written from the model alone and read again, with its notes.
fn round(doc: &Document) -> (Document, Vec<&'static str>) {
    let written = write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the save is refused: {e}"));
    let (back, _) = read(&written.bytes, &Limits::default()).unwrap_or_else(|e| panic!("the file does not read: {e}"));
    (back, written.notes.iter().map(diacraft_odg::SaveNote::class).collect())
}

/// The first shape of the first page of `doc`.
fn first(doc: &Document) -> &Shape {
    doc.pages.get(0).unwrap().shapes.get(0).expect("a shape")
}

/// A drawing of one rectangle that `edit` has changed.
fn with(edit: impl FnOnce(&mut Shape)) -> Document {
    let mut doc = blank(1);
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    edit(Arc::make_mut(page.shapes.get_mut(0).unwrap()));
    doc
}

fn number(shape: &Shape, cell: &str) -> Option<f64> {
    shape.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number())
}

fn constant(shape: &mut Shape, name: &str, value: f64, unit: Option<&str>) {
    let mut cell = Cell::constant(Value::from_number(value).unwrap());
    if let Some(unit) = unit {
        cell = cell.with_unit(unit);
    }
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(name), cell);
}

fn named_row(name: &str, cells: &[(&str, f64)]) -> Arc<Row> {
    let mut row = Row { name: Some(Arc::from(name)), ..Row::default() };
    for (cell, value) in cells {
        row.cells.insert(CellName::new(cell), Cell::constant(Value::from_number(*value).unwrap()));
    }
    Arc::new(row)
}

#[test]
fn rounded_corners_survive_a_model_save() {
    let (back, _) = round(&with(|s| constant(s, "Rounding", 0.25, Some("IN"))));
    let got = number(first(&back), "Rounding").expect("the Rounding cell comes back");
    assert!((got - 0.25).abs() < 1e-9, "{got}");
}

#[test]
fn a_shadow_survives_a_model_save() {
    let (back, _) = round(&with(|s| {
        constant(s, "ShdwPattern", 1.0, None);
        constant(s, "ShdwOffsetX", 0.1, Some("IN"));
        constant(s, "ShdwOffsetY", -0.1, Some("IN"));
        Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("ShdwForegnd"), Cell::constant(Value::from_text("#808080")));
    }));
    let shape = first(&back);
    assert_eq!(number(shape, "ShdwPattern"), Some(1.0));
    assert!((number(shape, "ShdwOffsetX").unwrap() - 0.1).abs() < 1e-9);
    assert!((number(shape, "ShdwOffsetY").unwrap() + 0.1).abs() < 1e-9);
    assert_eq!(shape.sheet.cell("ShdwForegnd").and_then(|c| c.value.clone()), Some(Value::from_text("#808080")));
}

#[test]
fn a_text_block_that_is_not_the_whole_shape_survives_a_model_save() {
    let (back, _) = round(&with(|s| {
        constant(s, "TxtWidth", 0.5, Some("IN"));
        constant(s, "TxtHeight", 0.25, Some("IN"));
        constant(s, "TxtPinX", 0.5, Some("IN"));
        constant(s, "TxtPinY", 0.5, Some("IN"));
        constant(s, "TxtLocPinX", 0.25, Some("IN"));
        constant(s, "TxtLocPinY", 0.125, Some("IN"));
    }));
    let shape = first(&back);
    for (cell, want) in [("TxtWidth", 0.5), ("TxtHeight", 0.25), ("TxtPinX", 0.5), ("TxtPinY", 0.5), ("TxtLocPinX", 0.25), ("TxtLocPinY", 0.125)] {
        assert!((number(shape, cell).unwrap_or(f64::NAN) - want).abs() < 1e-9, "{cell}");
    }
}

#[test]
fn shape_data_survives_a_model_save() {
    let (back, _) = round(&with(|s| {
        let sheet = Arc::make_mut(&mut s.sheet);
        let mut property = Section::new("Property", None);
        property.rows.push(named_row("Cost", &[("Value", 12.5)]));
        sheet.sections.push(Arc::new(property));
        let mut user = Section::new("User", None);
        user.rows.push(named_row("Owner", &[("Value", 7.0)]));
        sheet.sections.push(Arc::new(user));
    }));
    let shape = first(&back);
    let value = |section: &str, row: &str| {
        shape
            .sheet
            .sections
            .iter()
            .find(|s| s.name.as_str() == section)
            .and_then(|s| s.rows.iter().find(|r| r.name.as_deref() == Some(row)))
            .and_then(|r| r.cells.get("Value")?.value.as_ref()?.number())
    };
    assert_eq!(value("Property", "Cost"), Some(12.5));
    assert_eq!(value("User", "Owner"), Some(7.0));
}

#[test]
fn another_cell_survives_a_model_save() {
    let (back, _) = round(&with(|s| constant(s, "ObjType", 3.0, None)));
    assert_eq!(number(first(&back), "ObjType"), Some(3.0));
}

#[test]
fn a_page_cell_no_attribute_of_the_file_holds_survives_a_model_save() {
    // `DrawingSizeType` is the engine's own mark of how a page was sized (1 a preset size, 3 a custom one): no attribute of an .odg holds it, and the engine's save used to take it out
    // of the copy it handed the writer, without a word.
    let mut doc = blank(1);
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    Arc::make_mut(&mut page.sheet).cells.insert(CellName::new("DrawingSizeType"), Cell::constant(Value::from_number(3.0).unwrap()));
    let (back, _) = round(&doc);
    let got = back.pages.get(0).unwrap().sheet.cell("DrawingSizeType").and_then(|c| c.value.as_ref()?.number());
    assert_eq!(got, Some(3.0));
}

#[test]
fn the_label_cells_of_a_connector_with_no_label_survive_a_model_save() {
    // The place of a label on a connector (the engine writes it when it routes one): no attribute of an .odg holds it.
    let mut doc = blank(0);
    let mut line = crate::write_new::connector(1, (4.0, 4.0, 8.0, 4.0));
    for (name, v) in [("TxtPinX", 2.0), ("TxtPinY", 0.0), ("TxtLocPinX", 0.5), ("TxtLocPinY", 0.25), ("TxtWidth", 1.0), ("TxtHeight", 0.5)] {
        constant(&mut line, name, v, Some("IN"));
    }
    crate::write_new::push_shape(&mut doc, line);
    let (back, _) = round(&doc);
    let shape = first(&back);
    for (name, v) in [("TxtPinX", 2.0), ("TxtWidth", 1.0), ("TxtHeight", 0.5)] {
        assert!((number(shape, name).unwrap_or(f64::NAN) - v).abs() < 1e-9, "{name}");
    }
}

#[test]
fn the_layer_of_a_shape_survives_a_model_save() {
    // `LayerMember` names the row of the page's layer section the shape is on.
    let (back, _) = round(&with(|s| constant(s, "LayerMember", 2.0, None)));
    assert_eq!(number(first(&back), "LayerMember"), Some(2.0));
}

#[test]
fn text_formatting_survives_a_model_save() {
    // A run's row and the text's background: what the engine's save used to replace with the text's characters.
    let (back, _) = round(&with(|s| {
        let sheet = Arc::make_mut(&mut s.sheet);
        let mut character = Section::new("Character", None);
        let mut row = Row { ix: Some(0), ..Row::default() };
        row.cells.insert(CellName::new("Size"), Cell::constant(Value::from_number(0.5).unwrap()).with_unit("IN"));
        character.rows.push(Arc::new(row));
        sheet.sections.push(Arc::new(character));
        sheet.cells.insert(CellName::new("TextBkgnd"), Cell::constant(Value::from_text("#ffff00")));
        s.text = Some(Arc::new(diacraft_doc::Text {
            parts: vec![diacraft_doc::TextPart::Cp(0), diacraft_doc::TextPart::Chars(Arc::from("Hello")), diacraft_doc::TextPart::Cp(1)],
            ..Default::default()
        }));
    }));
    let shape = first(&back);
    let size = shape
        .sheet
        .sections
        .iter()
        .find(|g| g.name.as_str() == "Character")
        .and_then(|g| g.rows.first()?.cells.get("Size")?.value.as_ref()?.number());
    assert_eq!(size, Some(0.5), "the run's row comes back");
    assert_eq!(shape.sheet.cell("TextBkgnd").and_then(|c| c.value.clone()), Some(Value::from_text("#ffff00")));
    assert!(
        shape.text.as_ref().is_some_and(|t| t.parts.iter().any(|p| matches!(p, diacraft_doc::TextPart::Cp(0)))),
        "and the run's marker in the text"
    );
}
