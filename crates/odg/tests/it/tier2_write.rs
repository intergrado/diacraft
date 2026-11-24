//! Tier 2 written (the proposal).
//!
//! A new shape the model holds is written natively as it looks, and what the reader would not rebuild from that markup (a user cell, a layer, a master's
//! link, a formula, a colour in capitals, a section the writer does not write) goes on the shape's own automatic style, as attributes of DiaCraft's
//! extension. Every case reads the saved bytes back and compares the shape with the model's, cell by cell.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Row, RowKey, Section, SectionKey, Shape, Sheet, Value};
use diacraft_odg::{Limits, read};

use crate::drawing::Drawing;
use crate::write_core::{open, open_bytes, part, real};
use crate::write_new::{a_line, connector, glue, push_shape, rectangle, with_side_middles};

fn cell_of(formula: Option<&str>, value: f64) -> Cell {
    Cell::read(formula, Some(&value.to_string()), None, None, &mut 1_000)
}

fn base() -> crate::write_core::Open {
    open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")))
}

/// The one style `content.xml` holds for the shape just added: the text of its graphic properties.
fn graphic_of_new(content: &str) -> &str {
    let after = content.split("<style:style style:name=\"gr1\"").nth(1).unwrap_or_else(|| panic!("no gr1 in {content}"));
    after.split("</style:style>").next().unwrap()
}

/// Whether two sheets hold the same cells, row by row and section by section, by key: a formula is the same by its text, a constant by its number
/// (within the writer's tolerance) or its text, and a unit is not compared, for it says only how a value shows.
pub fn same_cells(a: &Sheet, b: &Sheet) -> Result<(), String> {
    fn same_cell(x: &Cell, y: &Cell) -> bool {
        if x.formula.text() != y.formula.text() || x.error != y.error || x.extra != y.extra || x.raw != y.raw {
            return false;
        }
        if x.formula.text().is_some() {
            return true;
        }
        match (&x.value, &y.value) {
            (Some(p), Some(q)) => p.text() == q.text() || p.number().zip(q.number()).is_some_and(|(m, n)| (m - n).abs() <= 1e-9 * (1.0 + m.abs())),
            (None, None) => true,
            _ => false,
        }
    }
    let cells = |label: &str, p: &diacraft_doc::Cells, q: &diacraft_doc::Cells| -> Result<(), String> {
        let names: std::collections::BTreeSet<&CellName> = p.keys().chain(q.keys()).collect();
        for n in names {
            // A gradient that is not enabled is 0 by default: the reader gives the cell to a solid fill, and a model may have none.
            if n.as_str() == "FillGradientEnabled" && p.get(n).or(q.get(n)).is_some_and(|c| c.value.as_ref().and_then(Value::number) == Some(0.0)) {
                continue;
            }
            match (p.get(n), q.get(n)) {
                (Some(x), Some(y)) if same_cell(x, y) => {}
                (x, y) => return Err(format!("{label} cell {}: {x:?} against {y:?}", n.as_str())),
            }
        }
        Ok(())
    };
    cells("sheet", &a.cells, &b.cells)?;
    let keys: std::collections::BTreeSet<(String, Option<u32>)> =
        a.sections.iter().chain(b.sections.iter()).map(|s| (s.name.as_str().to_owned(), s.ix)).collect();
    for (name, ix) in keys {
        let key = SectionKey::new(&name, ix);
        let (Some(p), Some(q)) = (a.section(&key), b.section(&key)) else { return Err(format!("section {name} {ix:?} is in one sheet only")) };
        cells(&format!("section {name}"), &p.cells, &q.cells)?;
        if p.rows.len() != q.rows.len() {
            return Err(format!("section {name} has {} rows against {}", p.rows.len(), q.rows.len()));
        }
        for (x, y) in p.rows.iter().zip(q.rows.iter()) {
            // (A row type of `""` is none: the helper that builds a rectangle's connection rows writes one, and the reader writes none.)
            let kind = |r: &Row| r.row_type.as_deref().filter(|t| !t.is_empty()).map(str::to_owned);
            if x.name != y.name || x.ix != y.ix || kind(x) != kind(y) || x.local_name != y.local_name || x.del != y.del {
                return Err(format!("a row of {name} differs: {x:?} against {y:?}"));
            }
            cells(&format!("row of {name}"), &x.cells, &y.cells)?;
        }
    }
    Ok(())
}

/// Saves `doc` (the base drawing and one new shape), reads the bytes back, and gives the new shape as it was read with the bytes.
fn round_trip(shape: Shape) -> (Shape, Shape, Vec<u8>) {
    let mut o = base();
    let id = shape.id;
    let _ = id;
    push_shape(&mut o.doc, shape.clone());
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let back = open_bytes(saved.bytes.clone());
    let read = back.doc.pages.get(0).unwrap().shapes.get(1).map(|s| (**s).clone()).expect("the new shape reads back");
    (shape, read, saved.bytes)
}

#[test]
fn tier2_written_only_where_cells_differ_from_reader_defaults() {
    let o = base();
    let id = o.index.next_id(0).unwrap();
    // DiaCraft's rectangle as the engine makes it is tier 1: not one attribute of the extension is written.
    let (model, read_back, bytes) = round_trip(rectangle(id, "R", (2.0, 3.0, 4.0, 2.0)));
    let content = part(&bytes, "content.xml");
    assert!(!content.contains(" diacraft:"), "a basic shape carries no tier 2: {content}");
    same_cells(&model.sheet, &read_back.sheet).expect("the cells of a tier-1 shape read back");
    // Each cell the rebuild would not give is written, and no other: a user cell, a layer, a pin that is not the centre, a colour in capitals.
    let mut shape = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
    {
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("note")));
        sheet.cells.insert(CellName::new("LayerMember"), cell_of(None, 0.0));
        sheet.cells.insert(CellName::new("FillForegnd"), Cell::constant(Value::from_text("#FF0000")));
    }
    let (model, read_back, bytes) = round_trip(shape);
    let content = part(&bytes, "content.xml");
    let graphic = graphic_of_new(&content);
    assert!(graphic.contains("diacraft:version=\"1\"") && graphic.contains(&format!("diacraft:id=\"{id}\"")), "{graphic}");
    let cells = graphic.split("diacraft:cells=\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or_default();
    for written in ["Comment", "LayerMember", "FillForegnd"] {
        assert!(cells.contains(&format!("N=&quot;{written}&quot;")) || cells.contains(&format!("N=\\\"{written}")), "{written} in {cells}");
    }
    for rebuilt in ["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY", "FillPattern", "LinePattern"] {
        assert!(!cells.contains(&format!("&quot;{rebuilt}&quot;")), "{rebuilt} is rebuilt, and is not written: {cells}");
    }
    same_cells(&model.sheet, &read_back.sheet).expect("every cell reads back");
    assert_eq!(read_back.id, id, "a shape that carries tier 2 keeps its ID");
    assert!(!content.contains("diacraft:s-"), "no section is written: the outline and the connection points are rebuilt: {content}");
}

#[test]
fn a_section_the_writer_does_not_write_is_carried_whole_and_reads_back() {
    let o = base();
    let id = o.index.next_id(0).unwrap();
    let mut shape = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
    {
        let sheet = Arc::make_mut(&mut shape.sheet);
        let mut user = Section::new("User", None);
        let mut row = Row { name: Some(Arc::from("visVersion")), local_name: Some(Arc::from("Version")), ..Row::default() };
        row.cells.insert(CellName::new("Value"), cell_of(Some("Width*2"), 3.0));
        row.cells.insert(CellName::new("Prompt"), Cell::constant(Value::from_text("say \"hi\" <b> & more\nnext line")));
        user.rows.push(Arc::new(row));
        sheet.sections.push(Arc::new(user));
        let mut prop = Section::new("Property", None);
        let mut p = Row { name: Some(Arc::from("Cost")), ..Row::default() };
        p.cells.insert(CellName::new("Value"), Cell::constant(Value::from_text("12.5")));
        prop.rows.push(Arc::new(p));
        sheet.sections.push(Arc::new(prop));
    }
    let (model, read_back, bytes) = round_trip(shape);
    let content = part(&bytes, "content.xml");
    assert!(content.contains("diacraft:s-User=") && content.contains("diacraft:s-Property="), "{content}");
    same_cells(&model.sheet, &read_back.sheet).expect("the sections read back, the newline and the quotes too");
    let row = read_back.sheet.section(&SectionKey::new("User", None)).unwrap().row(&RowKey::name("visVersion")).unwrap();
    assert_eq!(row.cells.get("Prompt").and_then(|c| c.value.as_ref()).map(Value::text), Some("say \"hi\" <b> & more\nnext line"));
}

#[test]
fn the_links_to_a_master_the_names_and_the_text_of_a_dropped_shape_read_back() {
    let o = base();
    let id = o.index.next_id(0).unwrap();
    let mut shape = rectangle(id, "Process.4", (2.0, 3.0, 4.0, 2.0));
    shape.name = Some(Arc::from("Process"));
    shape.master = Some(3);
    shape.master_shape = Some(7);
    shape.text = Some(Arc::new(diacraft_doc::Text {
        parts: vec![
            diacraft_doc::TextPart::Cp(0),
            diacraft_doc::TextPart::Chars(Arc::from("Hello")),
            diacraft_doc::TextPart::Pp(1),
            diacraft_doc::TextPart::Chars(Arc::from("\nWorld")),
            diacraft_doc::TextPart::Fld { ix: 0, text: Arc::from("7") },
        ],
        ..Default::default()
    }));
    let (model, read_back, bytes) = round_trip(shape);
    assert_eq!((read_back.master, read_back.master_shape), (Some(3), Some(7)));
    assert_eq!((read_back.name.as_deref(), read_back.name_u.as_deref()), (Some("Process"), Some("Process.4")));
    // The parts: the `.odg` data in the text's slot is the reader's, and the model's text has none.
    assert_eq!(read_back.text.as_ref().map(|t| &t.parts), model.text.as_ref().map(|t| &t.parts), "the text with its markers");
    let content = part(&bytes, "content.xml");
    // The plain characters are the native text, as a viewer of another program shows them.
    assert!(content.contains("Hello</text:p><text:p>World7</text:p>"), "{content}");
}

/// Found by the writer's fuzz target: a carried text whose runs of characters follow each other (`xy` and a line feed, as two runs) is read back as one run, and
/// an empty run is read back as none. The text is the same text, and the check of the file made says so and does not refuse the save.
#[test]
fn runs_of_characters_that_follow_each_other_in_a_carried_text_are_one_run_when_read_and_the_save_is_made() {
    let o = base();
    let id = o.index.next_id(0).unwrap();
    let mut shape = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
    shape.text = Some(Arc::new(diacraft_doc::Text {
        parts: vec![
            diacraft_doc::TextPart::Cp(3),
            diacraft_doc::TextPart::Chars(Arc::from("xy")),
            diacraft_doc::TextPart::Chars(Arc::from("\n")),
            diacraft_doc::TextPart::Chars(Arc::from("")),
            diacraft_doc::TextPart::Pp(1),
            diacraft_doc::TextPart::Chars(Arc::from("z")),
        ],
        ..Default::default()
    }));
    let (_, read_back, _) = round_trip(shape);
    let parts = &read_back.text.as_ref().unwrap().parts;
    assert!(
        matches!(parts.as_slice(), [diacraft_doc::TextPart::Cp(3), diacraft_doc::TextPart::Chars(a), diacraft_doc::TextPart::Pp(1), diacraft_doc::TextPart::Chars(b)] if &**a == "xy\n" && &**b == "z"),
        "{parts:?}"
    );
}

#[test]
fn the_ids_of_shapes_named_by_glue_or_by_a_formula_are_kept_and_no_other_shape_gets_one() {
    let mut o = base();
    let a = o.index.next_id(0).unwrap();
    let b = a + 1;
    let c = a + 2;
    push_shape(&mut o.doc, rectangle(a, "A", (2.0, 3.0, 4.0, 2.0)));
    push_shape(&mut o.doc, rectangle(b, "B", (10.0, 3.0, 4.0, 2.0)));
    push_shape(&mut o.doc, with_side_middles(connector(c, (6.0, 25.7, 10.0, 25.7))));
    glue(&mut o.doc, c, true, a, 3);
    glue(&mut o.doc, c, false, b, 1);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let content = part(&saved.bytes, "content.xml");
    // The two glued shapes carry their ids; the connector, which nothing names and which carries no tier 2, does not.
    assert_eq!(content.matches("diacraft:id=").count(), 2, "{content}");
    assert!(content.contains(&format!("diacraft:id=\"{a}\"")) && content.contains(&format!("diacraft:id=\"{b}\"")), "{content}");
    let back = open_bytes(saved.bytes.clone());
    let page = back.doc.pages.get(0).unwrap();
    let ids: Vec<u32> = page.shapes.iter().map(|s| s.id).collect();
    assert!(ids.contains(&a) && ids.contains(&b), "{ids:?}");
    assert_eq!(page.connects.len(), 2, "the glue is rebuilt: {:?}", page.connects);
    let _ = a_line(0);
}

#[test]
fn a_shape_that_is_a_line_with_a_user_cell_carries_it() {
    let o = base();
    let id = o.index.next_id(0).unwrap();
    let mut line = a_line(id);
    Arc::make_mut(&mut line.sheet).cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("x")));
    let (model, read_back, bytes) = round_trip(line);
    assert!(part(&bytes, "content.xml").contains("diacraft:cells="));
    assert!(read_back.sheet.cell("Comment").is_some());
    let mut m = model.sheet.as_ref().clone();
    m.sections.retain(|s| s.name.as_str() != "Connection");
    // The four connection rows the reader adds to a one-dimensional shape are no tier 2 (the proposal): they are the reader's, and not the model's.
    let mut r = read_back.sheet.as_ref().clone();
    r.sections.retain(|s| s.name.as_str() != "Connection");
    same_cells(&m, &r).expect("the line's cells read back");
}

#[test]
fn what_the_model_lacks_that_the_reader_rebuilds_is_said_gone() {
    let o = base();
    let id = o.index.next_id(0).unwrap();
    let mut shape = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
    {
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.sections.retain(|s| s.name.as_str() != "Connection");
        sheet.cells.remove("LocPinX");
    }
    let (model, read_back, bytes) = round_trip(shape);
    assert!(part(&bytes, "content.xml").contains("diacraft:gone="));
    same_cells(&model.sheet, &read_back.sheet).expect("what the model lacks, the read lacks");
    assert!(read_back.sheet.cell("LocPinX").is_none() && read_back.sheet.section(&SectionKey::new("Connection", None)).is_none());
}

#[test]
fn a_second_save_of_a_saved_drawing_is_no_edit() {
    let o = base();
    let id = o.index.next_id(0).unwrap();
    let mut shape = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("note")));
    let mut o = base();
    push_shape(&mut o.doc, shape);
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    // The model as it was, the bytes and the index the save gave: saving again is no edit and gives the same bytes.
    let again = real(&o.doc, &first.bytes, &first.index).unwrap();
    assert!(!again.edited);
    assert_eq!(again.bytes, first.bytes);
    // And the file read afresh, saved with what it was read as, is no edit either.
    let fresh = open_bytes(first.bytes.clone());
    let third = real(&fresh.doc, &fresh.bytes, &fresh.index).unwrap();
    assert!(!third.edited);
    assert_eq!(third.bytes, first.bytes);
    let _ = (Document::empty(), read as fn(&[u8], &Limits) -> _);
}
