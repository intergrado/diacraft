//! A window of lines and their resolved cells (design 6).
//!
//! [`read_lines`] gives the lines `from` to `from + count` of an [`Outline`], each with the cells it shows resolved, in the order of its section's columns, and **nothing else is
//! resolved**: a scroll that brings k new lines on view resolves at most k times the columns of cells, a frame that brings none resolves none. The answer is cut when the text it
//! holds passes [`MAX_ANSWER_BYTES`](super::MAX_ANSWER_BYTES), and always gives its first line.

use std::collections::HashMap;

use diacraft_doc::{CellAddr, ix_to_row_number, section_spelling};
use diacraft_doc::{Resolver, RowKey, SectionKey, SheetRef};
use serde_json::{Value, json};

use super::cell::{self, CellView, Ctx};
use super::outline::{Counts, Outline, SectionInfo, Slot};
use super::{MAX_ANSWER_BYTES, MAX_FORMULA_CHARS};

/// What a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    /// The title of a section.
    Header,
    /// A cell on a line of its own: a cell of a display group, or one of Geometry's five.
    Cell,
    /// The titles of the columns of a section of rows.
    Titles,
    /// A row.
    Row,
    /// How many rows the outline does not list.
    Note,
}

impl LineKind {
    pub fn word(self) -> &'static str {
        match self {
            LineKind::Header => "header",
            LineKind::Cell => "cell",
            LineKind::Titles => "titles",
            LineKind::Row => "row",
            LineKind::Note => "note",
        }
    }
}

/// A row of a section, as a line shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct RowView {
    /// A named row's name, or an indexed row's number as a reference spells it.
    pub key: Value,
    /// What the Row column shows: a named row's reference name (`Prop.Cost`), an indexed row's number.
    pub label: String,
    /// The row type `T` (`LineTo`).
    pub row_type: Option<String>,
    /// `local` when the sheet holds the row, `inherited` when only a master or a style does.
    pub origin: &'static str,
}

impl RowView {
    pub fn json(&self) -> Value {
        json!({"key": self.key, "label": self.label, "type": self.row_type, "origin": self.origin})
    }
}

/// A line of a sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub line: u64,
    /// The index of its section in the outline.
    pub section: usize,
    pub kind: LineKind,
    pub row: Option<RowView>,
    pub cells: Vec<CellView>,
    /// A note line's words.
    pub note: Option<String>,
}

/// The lines read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lines {
    pub lines: Vec<Line>,
    /// The bytes of text the lines hold, as accounted.
    pub bytes: usize,
}

impl Line {
    /// The line as a JSON object, in the form of `sheet.read`'s `lines`; `section` is the name of the section it is in.
    pub fn json(&self, section: &str) -> Value {
        let mut out = json!({
            "line": self.line,
            "section": section,
            "kind": self.kind.word(),
            "row": self.row.as_ref().map(RowView::json),
            "cells": self.cells.iter().map(CellView::json).collect::<Vec<_>>(),
        });
        if let (Some(map), Some(note)) = (out.as_object_mut(), &self.note) {
            map.insert("note".into(), json!(note));
        }
        out
    }
}

/// The words of a note: "123,456 more rows are not listed here (`sheet.read` reads them)".
fn note_words(unlisted: usize) -> String {
    format!("{} more rows are not listed here (name this section in `sections` to read them with `sheet.read`)", thousands(unlisted))
}

/// A number with commas between its thousands.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// How a row's key reads in a reference: a name as it is, an index as its number.
pub(crate) fn row_key_json(section: &SectionKey, key: &RowKey) -> (Value, String) {
    match key {
        RowKey::Name(name) => {
            let label = match section_spelling(section.name.as_str()) {
                Some(spelling) => format!("{spelling}.{name}"),
                None => format!("{}.{name}", section.name),
            };
            (json!(name.as_ref()), label)
        }
        RowKey::Ix(ix) => {
            let number = ix_to_row_number(section.name.as_str() == "Geometry", *ix).unwrap_or(*ix);
            (json!(number), number.to_string())
        }
    }
}

/// The columns of a section by name, for ordering a row's cells.
fn column_places(section: &SectionInfo) -> HashMap<&str, usize> {
    section.columns.iter().enumerate().map(|(i, c)| (c.as_str(), i)).collect()
}

/// The lines `from` to `from + count` of `outline`, with their cells resolved, and the work it took. A line past the last is not given.
pub fn read_lines(res: &Resolver<'_>, sheet: &SheetRef, outline: &Outline, from: u64, count: usize) -> (Lines, Counts) {
    let mut counts = Counts::default();
    let mut out = Lines::default();
    let mut eval = super::read_eval_ctx();
    let ctx = Ctx::new(res, sheet, &mut eval);
    let local = res.sheet(sheet);
    let mut places: HashMap<usize, HashMap<&str, usize>> = HashMap::new();
    let mut line = from;
    while out.lines.len() < count && out.bytes <= MAX_ANSWER_BYTES {
        let Some((si, offset)) = outline.at(line) else { break };
        let Some(section) = outline.sections.get(si) else { break };
        let Some(slot) = section.at(offset) else { break };
        let mut item = Line { line, section: si, kind: LineKind::Header, row: None, cells: Vec::new(), note: None };
        match slot {
            Slot::Header => {}
            Slot::Titles => item.kind = LineKind::Titles,
            Slot::Note => {
                item.kind = LineKind::Note;
                item.note = Some(note_words(section.unlisted));
            }
            Slot::Flag(i) => {
                item.kind = LineKind::Cell;
                if let (Some(key), Some(name)) = (&section.key, section.flags.get(i)) {
                    let addr = CellAddr::in_section(key.name.as_str(), key.ix, name);
                    item.cells.extend(cell::view(&ctx, &addr, name, None, Some(MAX_FORMULA_CHARS), &mut counts));
                }
            }
            Slot::Cell(i) => {
                item.kind = LineKind::Cell;
                if let Some(name) = section.cells.get(i) {
                    item.cells.extend(cell::view(&ctx, &CellAddr::direct(name), name, None, Some(MAX_FORMULA_CHARS), &mut counts));
                }
            }
            Slot::Row(i) => {
                item.kind = LineKind::Row;
                if let (Some(key), Some(row)) = (&section.key, section.rows.get(i)) {
                    let columns = places.entry(si).or_insert_with(|| column_places(section));
                    let mut names = res.row_cell_names(sheet, key, row).unwrap_or_default();
                    names.sort_by_key(|n| (columns.get(n.as_str()).copied().unwrap_or(usize::MAX), n.clone()));
                    for name in &names {
                        let addr = CellAddr { section: Some(key.clone()), row: Some(row.clone()), cell: name.clone() };
                        let col = columns.get(name.as_str()).copied();
                        item.cells.extend(cell::view(&ctx, &addr, name.as_str(), col, Some(MAX_FORMULA_CHARS), &mut counts));
                    }
                    let (key_json, label) = row_key_json(key, row);
                    let origin = match local.and_then(|s| s.section(key)).and_then(|s| s.row(row)) {
                        Some(_) => "local",
                        None => "inherited",
                    };
                    item.row = Some(RowView { key: key_json, label, row_type: res.row_type(sheet, key, row).map(|t| t.to_string()), origin });
                }
            }
        }
        out.bytes = out.bytes.saturating_add(item.cells.iter().map(CellView::bytes).sum::<usize>()).saturating_add(96);
        out.lines.push(item);
        line = match line.checked_add(1) {
            Some(next) => next,
            None => break,
        };
    }
    (out, counts)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn a_count_of_rows_is_written_with_commas() {
        assert_eq!(
            (thousands(0), thousands(999), thousands(1_000), thousands(123_456), thousands(1_234_567)),
            ("0".into(), "999".into(), "1,000".into(), "123,456".into(), "1,234,567".into())
        );
    }

    #[test]
    fn a_rows_key_is_its_name_or_its_number_as_a_reference_spells_it() {
        let geometry = SectionKey::new("Geometry", Some(0));
        assert_eq!(row_key_json(&geometry, &RowKey::Ix(3)), (json!(3), "3".to_owned()), "Geometry's number is its IX");
        let scratch = SectionKey::new("Scratch", None);
        assert_eq!(row_key_json(&scratch, &RowKey::Ix(0)), (json!(1), "1".to_owned()), "any other section's is IX plus one");
        let property = SectionKey::new("Property", None);
        assert_eq!(row_key_json(&property, &RowKey::name("Cost")), (json!("Cost"), "Prop.Cost".to_owned()));
        // The largest IX has no number: it is given as it is.
        assert_eq!(row_key_json(&scratch, &RowKey::Ix(u32::MAX)).0, json!(u32::MAX));
    }
}
