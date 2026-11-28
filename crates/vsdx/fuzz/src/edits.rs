//! A sheet edit made on a model the way the row commands make it, and the two saves into the file that follow it: the property `vsdx_roundtrip` holds on every package that reads, and
//! the test of `crates/vsdx` that mirrors it (`tests/it/fuzz.rs`, which includes this file).
//!
//! The edit is one of four on the first section that has a row, picked by the first bytes of the input: a row **inserted** with the rows after it numbered again, a row **deleted** with the
//! rows after it numbered again, the section **taken out**, a cell of its first row **written**. It is made to a model that a reader made, which a hostile file makes of any shape (rows with no
//! number, a number twice, a section with no row, the largest number), so every step is `get`, `checked_add` and `checked_sub`.
//!
//! The property: a save into the file after the edit is a package that reads, or a refusal in words that is not the writer's own check failing and does not say it cannot write a change to unknown
//! elements (an edit of rows and sections never changes one: a row numbered again holds its own); and **a second edit and a second save into the file the first made, with the file as it reads
//! for the baseline** (what the engine keeps after a save), is the same.

use std::sync::Arc;

use diacraft_doc::{Cell, Document, Row, Section, Sheet};
use diacraft_vsdx::{Limits, OriginalBytes, WriteError, WriteOptions, read, write_with};

const NOW: i64 = 1_700_000_000;

/// The places an edit may reach: `(page, shape, section)` of every section of a page's top-level shapes that is not deleted and holds a row, at most 64, in order.
fn candidates(doc: &Document) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    for (p, page) in doc.pages.iter().enumerate() {
        for (s, shape) in page.shapes.iter().enumerate() {
            for (k, section) in shape.sheet.sections.iter().enumerate() {
                if !section.del && !section.rows.is_empty() {
                    out.push((p, s, k));
                    if out.len() >= 64 {
                        return out;
                    }
                }
            }
        }
    }
    out
}

/// A row with one cell `A` holding `value`.
fn row(ix: Option<u32>, name: Option<&str>, value: &str) -> Row {
    let mut row = Row { ix, name: name.map(Arc::from), ..Row::default() };
    row.cells.insert("A".into(), Cell::read(None, Some(value), None, None, &mut 0));
    row
}

/// An insert at the place `at` (modulo the rows and one): the new row takes the number of the row it goes before, and that row and every row after it are one number up. A section keyed by
/// name gets a row of a name of its own.
fn insert_row(section: &mut Section, at: usize) {
    let named = section.rows.iter().any(|r| r.ix.is_none() && r.name.is_some());
    let place = at % section.rows.len().saturating_add(1);
    if named {
        let name = format!("fuzz_{}", section.rows.len());
        section.rows.insert(place, Arc::new(row(None, Some(&name), "1")));
        return;
    }
    let number = section
        .rows
        .get(place)
        .and_then(|r| r.ix)
        .or_else(|| section.rows.iter().filter_map(|r| r.ix).max().and_then(|m| m.checked_add(1)))
        .unwrap_or(0);
    for held in section.rows.iter_mut() {
        if held.ix.is_some_and(|ix| ix >= number) {
            let held = Arc::make_mut(held);
            held.ix = held.ix.and_then(|ix| ix.checked_add(1));
        }
    }
    section.rows.insert(place, Arc::new(row(Some(number), None, "1")));
}

/// A delete of the row at the place `at` (modulo the rows): the rows after it close up.
fn delete_row(section: &mut Section, at: usize) {
    let place = at % section.rows.len().max(1);
    if place >= section.rows.len() {
        return;
    }
    let gone = section.rows.remove(place).ix;
    if let Some(gone) = gone {
        for held in section.rows.iter_mut() {
            if held.ix.is_some_and(|ix| ix > gone) {
                let held = Arc::make_mut(held);
                held.ix = held.ix.and_then(|ix| ix.checked_sub(1));
            }
        }
    }
}

/// The sheet at `(page, shape)`, to change.
fn sheet_at(doc: &mut Document, page: usize, shape: usize) -> Option<&mut Sheet> {
    let page = Arc::make_mut(doc.pages.get_mut(page)?);
    let shape = Arc::make_mut(page.shapes.get_mut(shape)?);
    Some(Arc::make_mut(&mut shape.sheet))
}

/// Makes the edit `data` says on `doc`. `false` when the model has no section with a row to edit.
pub fn edit_a_sheet(doc: &mut Document, data: &[u8]) -> bool {
    let all = candidates(doc);
    if all.is_empty() {
        return false;
    }
    let pick = usize::from(data.first().copied().unwrap_or(0)) % all.len();
    let Some(&(p, s, k)) = all.get(pick) else { return false };
    let op = data.get(1).copied().unwrap_or(0) % 4;
    let at = usize::from(data.get(2).copied().unwrap_or(0));
    let Some(sheet) = sheet_at(doc, p, s) else { return false };
    if op == 2 {
        if k < sheet.sections.len() {
            sheet.sections.remove(k);
        }
        return true;
    }
    let Some(section) = sheet.sections.get_mut(k).map(Arc::make_mut) else { return false };
    match op {
        0 => insert_row(section, at),
        1 => delete_row(section, at),
        _ => {
            if let Some(first) = section.rows.get_mut(0).map(Arc::make_mut) {
                first.cells.insert("A".into(), Cell::read(None, Some("7"), None, None, &mut 0));
            }
        }
    }
    true
}

/// Whether a refusal of the writer is one a save of a model that read may give: not the writer's own check failing, not a tree that is not the file's, and not the words of a change to unknown
/// nodes that an edit of rows and sections does not make. → why not.
fn refusal(e: &WriteError) -> Result<(), String> {
    let words = e.to_string();
    if matches!(e, WriteError::SelfCheck(_) | WriteError::Mismatch(_)) {
        return Err(format!("a save of a model that read was refused as the writer's own failure: {words}"));
    }
    if words.contains("unknown elements") {
        return Err(format!("an edit of rows and sections was refused as a change to unknown nodes: {words}"));
    }
    Ok(())
}

/// The edit `data` says, saved into the file `bytes` that `doc` was read from, and then a second edit saved into the file the first save made. → why not.
pub fn save_after_edit(bytes: &[u8], doc: &Document, data: &[u8]) -> Result<(), String> {
    let mut edited = doc.clone();
    if !edit_a_sheet(&mut edited, data) {
        return Ok(());
    }
    let options = WriteOptions { now: Some(NOW), ..WriteOptions::default() };
    let first = match write_with(&edited, Some(&OriginalBytes { bytes, baseline: Some(doc) }), &options) {
        Ok(first) => first,
        Err(e) => return refusal(&e),
    };
    read(&first.bytes, &Limits::default()).map_err(|e| format!("the file the first save made does not read: {e}"))?;
    // What the engine does after a save: the drawing is the model that was edited, and the baseline is the file as it reads.
    let mut again = edited.clone();
    edit_a_sheet(&mut again, data.get(3..).unwrap_or_default());
    match write_with(&again, Some(&OriginalBytes { bytes: &first.bytes, baseline: Some(&first.document) }), &options) {
        Ok(second) => read(&second.bytes, &Limits::default()).map(|_| ()).map_err(|e| format!("the file the second save made does not read: {e}")),
        Err(e) => refusal(&e),
    }
}
