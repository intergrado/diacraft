//! The drawing the model writer saved, saved again into the file that save made with the index it gave, as a session saves it after a Save As.
//!
//! The engine keeps its own drawing after a save from the model, and splices the next save into the file the model writer made, with that writer's index. Two things the first save did
//! not write made that drawing differ from the file: the version of the program a VSDX file names (`DocProps::app_version`, which no `.odg` holds), refused as "a change of the document's
//! own fields (a property)", and the index's pages saved, which were the model writer's copy with what DiaCraft's document file carries taken out (a VSDX page's relationship, part name and
//! attributes), so that every page of a VSDX drawing was "not that page" and written again as a new one. Every model here is built in code.

use std::sync::Arc;

use diacraft_doc::Document;
use diacraft_odg::{OriginalPackage, WriteError, WriteOptions, Written, write_with};

use crate::carry_model::vsdx_model;
use crate::model_write::blank;

/// The first save, from the model alone.
fn first(doc: &Document) -> Written {
    write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the first save was refused: {e} ({})", e.class()))
}

/// The save of `doc` into the file the first save made, with the index it gave.
fn again(doc: &Document, first: &Written) -> Result<Written, WriteError> {
    write_with(doc, Some(&OriginalPackage { bytes: &first.bytes, index: &first.index }), &WriteOptions::default())
}

/// The drawing with the version of the program that wrote its file, as the VSDX reader gives `AppVersion`.
fn with_app_version(mut doc: Document, version: &str) -> Document {
    Arc::make_mut(&mut doc.props).app_version = Some(Arc::from(version));
    doc
}

/// **The version of the program a VSDX file names is no field of an `.odg`**: the model writer leaves it out and the reader of the file gives none, so the drawing that still names it is the
/// file's, and its save into the file is no edit.
#[test]
fn a_drawing_that_names_the_version_of_its_program_is_saved_again_as_no_edit() {
    let doc = with_app_version(blank(2), "16.0000");
    let made = first(&doc);
    let saved = again(&doc, &made).unwrap_or_else(|e| panic!("the second save was refused: {e}"));
    assert!(!saved.edited, "the second save is no edit");
    assert_eq!(saved.bytes, made.bytes, "the second save is the file the first made");
    // Nor is another version, or none: no save writes it, and no edit sets it.
    for other in [with_app_version(blank(2), "15.0000"), blank(2)] {
        let saved = again(&other, &made).unwrap_or_else(|e| panic!("the second save was refused: {e}"));
        assert!(!saved.edited && saved.bytes == made.bytes);
    }
}

/// **Every other property is compared**: one the file says otherwise is still refused, in the words that name a property, whether the version is named or not.
#[test]
fn a_property_that_is_not_the_files_is_still_refused_in_words() {
    let doc = with_app_version(blank(2), "16.0000");
    let made = first(&doc);
    for field in 0..4 {
        let mut changed = doc.clone();
        let props = Arc::make_mut(&mut changed.props);
        match field {
            0 => props.subject = Some(Arc::from("another subject")),
            1 => props.creator = Some(Arc::from("someone else")),
            2 => props.created = Some(Arc::from("2031-01-02T03:04:05Z")),
            _ => props.category = Some(Arc::from("a category")),
        }
        match again(&changed, &made) {
            Err(WriteError::Unsupported(words)) => assert_eq!(words, "a change of the document's own fields (a property)", "field {field}"),
            other => panic!("field {field}: expected the refusal of a property, got {:?}", other.map(|w| w.edited)),
        }
    }
}

/// **The pages the index holds are the caller's**: a drawing of a VSDX file (a page with its relationship, its part, the attributes and elements of its element, and the rest of what DiaCraft's
/// document file carries of every kind) saved from the model, then saved again into the file, is no edit and the same bytes; before, every page was written again as a new page.
#[test]
fn a_vsdx_drawing_saved_from_the_model_and_saved_again_into_its_file_is_no_edit() {
    let doc = vsdx_model();
    let made = first(&doc);
    let saved = again(&doc, &made).unwrap_or_else(|e| panic!("the second save was refused: {e}"));
    assert!(!saved.edited, "the second save is no edit");
    assert_eq!(saved.bytes, made.bytes, "the second save is the file the first made");
    // The index the first save gave holds the caller's pages as the pages saved, and the save after holds them still.
    let held = made.index.page(0).and_then(|p| p.page_id());
    assert_eq!(held, doc.pages.get(0).map(|p| p.id), "the entry is the page's");
}

/// **An edit after the first save is still written**: a shape moved on the VSDX drawing's page is an edited save, and the file read back has it where the drawing has it, and the rest of the page as
/// it was (its relationship and attributes, which DiaCraft's document file carries).
#[test]
fn a_move_after_a_model_save_is_written_into_its_file() {
    let doc = vsdx_model();
    let made = first(&doc);
    let mut moved = doc.clone();
    {
        let page = Arc::make_mut(moved.pages.get_mut(0).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let sheet = Arc::make_mut(&mut shape.sheet);
        let pin = sheet.cells.get_mut("PinX").unwrap();
        *pin = diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(4.5).unwrap()).with_unit("IN");
    }
    let saved = again(&moved, &made).unwrap_or_else(|e| panic!("the edit was refused: {e}"));
    assert!(saved.edited, "a move is an edit");
    assert_ne!(saved.bytes, made.bytes);
    let (back, _) = diacraft_odg::read(&saved.bytes, &diacraft_odg::Limits::default()).unwrap();
    let page = back.pages.get(0).unwrap();
    let x = page.shapes.iter().next().unwrap().sheet.cell("PinX").and_then(|c| c.value.as_ref()).and_then(diacraft_doc::Value::number).unwrap();
    assert!((x - 4.5).abs() < 1e-6, "the file has the shape at {x}");
    let was = doc.pages.get(0).unwrap();
    assert_eq!((&page.rel_id, &page.raw), (&was.rel_id, &was.raw), "the page's own data is as it was");
}
