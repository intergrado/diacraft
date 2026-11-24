//! The concern 1: the style sheets of a drawing that came from a VSDX file are written by the model writer, or carried, and never refused for what the package it makes
//! holds. A VSDX file's root style holds cells of its own where the package's root (made by the reader of its defaults) holds the file's, and its style sheets may name three different
//! style sheets where an OpenDocument style names one parent: the splice path's rule ("a change of a style sheet the file holds") was applied to a package that holds none.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Links, RawAttrs, Sheet, Style, Value};
use diacraft_odg::{Limits, WriteError, WriteOptions, read, write_with};

use crate::model_write::blank;

/// A style sheet with two cells, as a VSDX file's holds them, linked to `links`.
fn sheet_style(id: u32, name: &str, links: (Option<u32>, Option<u32>, Option<u32>), weight: &str) -> Style {
    let mut sheet = Sheet { links: Links { line: links.0, fill: links.1, text: links.2 }, ..Sheet::default() };
    sheet.cells.insert(CellName::new("LineWeight"), Cell::constant(Value::from_text(weight)).with_unit("IN"));
    sheet.cells.insert(CellName::new("EnableFillProps"), Cell::constant(Value::from_text("1")));
    Style { id, name: Some(Arc::from(name)), name_u: Some(Arc::from(name)), sheet: Arc::new(sheet), ..Style::default() }
}

/// The model of a VSDX file's drawing, as far as its style sheets go: a root with cells of its own and the attribute the VSDX reader keeps on it, and `more` style sheets after it.
fn with_styles(more: Vec<Style>) -> Document {
    let mut doc = blank(1);
    let mut root = sheet_style(0, "No Style", (None, None, None), "0.01");
    root.raw = RawAttrs::from_pairs([("IsCustomNameU", "1")]);
    *doc.styles.get_mut(0).unwrap() = Arc::new(root);
    for style in more {
        doc.styles.push(Arc::new(style));
    }
    doc
}

fn written_and_read(doc: &Document) -> Document {
    let written = write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    read(&written.bytes, &Limits::default()).unwrap().0
}

/// The identity and the cells of each style sheet: what a read of the file must give back.
/// What a style sheet shows: its `ID`, names, links and the values of its cells.
type Shown = (u32, Option<String>, Option<String>, Links, Vec<(String, String)>);

fn shown(doc: &Document) -> Vec<Shown> {
    doc.styles
        .iter()
        .map(|s| {
            let cells = s
                .sheet
                .cells
                .iter()
                .map(|(n, c)| (n.as_str().to_owned(), c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default()))
                .collect();
            (s.id, s.name.as_deref().map(str::to_owned), s.name_u.as_deref().map(str::to_owned), s.sheet.links, cells)
        })
        .collect()
}

/// The root of a drawing a VSDX file gave is carried: its cells replace the ones the package's root holds, and what its element held that no field reads comes back too. It was refused
/// ("a change of a style sheet the file holds, or a style sheet taken from it") for every drawing of a VSDX file.
#[test]
fn a_root_style_sheet_with_cells_of_its_own_is_written_and_read_back_whole() {
    let doc = with_styles(Vec::new());
    let back = written_and_read(&doc);
    assert_eq!(shown(&back).first(), shown(&doc).first(), "the root's identity and cells");
    assert_eq!(back.styles.get(0).unwrap().raw, doc.styles.get(0).unwrap().raw, "the attribute of the root's element rides in an item");
    // A drawing whose root is the package's own writes no entry for it, and reads the same.
    let plain = blank(1);
    let again = written_and_read(&plain);
    assert_eq!(again.styles.get(0).unwrap().sheet, plain.styles.get(0).unwrap().sheet);
}

/// A style sheet whose three links name different style sheets, one with no name, one with the name of another, and one named as the root is: each is written as an empty common style under a name
/// of its own, and the document file carries the truth, so that the sheet comes back as it was.
#[test]
fn style_sheets_whose_links_and_names_a_style_cannot_say_are_carried() {
    let mut nameless = sheet_style(5, "", (Some(1), Some(1), Some(1)), "0.05");
    nameless.name = Some(Arc::from(""));
    let doc = with_styles(vec![
        sheet_style(1, "Normal", (Some(0), Some(0), Some(0)), "0.02"),
        // Three links that differ, one of them a style that comes later.
        sheet_style(2, "Box", (Some(1), Some(0), Some(4)), "0.03"),
        // The name of an earlier style, and a link that names no style sheet that is before it.
        sheet_style(3, "Normal", (Some(2), Some(2), Some(2)), "0.04"),
        sheet_style(4, "Later", (None, None, None), "0.045"),
        nameless,
    ]);
    // (The sixth style, with an ID of 5, is at place 5: the numbering has no gap.)
    let back = written_and_read(&doc);
    assert_eq!(shown(&back), shown(&doc));
}

/// The numbering of the style sheets is no place: a style sheet whose ID is not its place is written (the file numbers its styles by their places and DiaCraft's document
/// file carries the ID) and read back with the ID it had; it was refused, in words, and nothing was written. Two style sheets of one ID are what is refused now (`style_ids.rs` has the rest).
#[test]
fn a_style_sheet_whose_id_is_not_its_place_is_written_and_two_of_one_id_are_refused_in_words() {
    let doc =
        with_styles(vec![sheet_style(1, "Normal", (Some(0), Some(0), Some(0)), "0.02"), sheet_style(7, "Gap", (Some(0), Some(0), Some(0)), "0.02")]);
    let back = written_and_read(&doc);
    assert_eq!(shown(&back), shown(&doc), "the gap in the numbering comes back");
    let twice =
        with_styles(vec![sheet_style(7, "Normal", (Some(0), Some(0), Some(0)), "0.02"), sheet_style(7, "Gap", (Some(0), Some(0), Some(0)), "0.02")]);
    let Err(WriteError::Unsupported(words)) = write_with(&twice, None, &WriteOptions::default()) else { panic!("an ID held twice is refused") };
    assert!(words.contains("same ID"), "{words}");
}
