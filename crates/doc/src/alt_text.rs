//! Alt text: the two texts a shape, a page and a master carry for people who cannot see the drawing.
//!
//! **Where they live.** A sheet's **title** is the value of the `Value` cell of the row `visAltText` of its `User` section, and its **description** is the same for the row
//! `visAltDescription` (the names the public files use, exactly, and no other row is alt text). There is no second source of truth: the texts are cells, so the formats,
//! undo and the ShapeSheet window see them as any `User` row.
//!
//! **How they inherit.** The text is **found, not copied**: a sheet's own row if it has one that is not deleted, else its master shape's (for an instance and for
//! a member of one), else nothing. [`read`] asks the document's resolver for the `Value` cell, which already walks the sheet and then its master shape; a row
//! deleted here (`Del`) hides the master's. **A master's page sheet is not a master shape**: an instance never inherits from it, and the resolver does not walk to it.
//! A text found in a style, or in the root style, is not alt text either: only the sheet and its master shape are looked at.
//!
//! **What a text is.** The cell's value as it is, with no trimming; a number is its text; an error value or a missing value is the empty text. **A sheet has alt text** when its
//! title or its description holds a character that is not white space (Unicode `White_Space`: [`is_blank`]).
//!
//! **What a read costs.** One resolution of two cells on a sheet, through the resolver the caller holds: a walk of a page's shapes asks for each shape with the one resolver, so the
//! maps of masters are built once. The text is borrowed from the document, never copied.

use std::sync::Arc;

use crate::cell::{Cell, FormulaSlot};
use crate::name::CellName;
use crate::resolve::{Located, Resolver, Source};
use crate::sheet::{CellAddr, Row, RowKey};
use crate::sheet_ref::{Owner, SheetRef};
use crate::value::Value;
use diacraft_formula::Expr;

/// The name of the `User` row that holds the title.
pub const TITLE_ROW: &str = "visAltText";
/// The name of the `User` row that holds the description.
pub const DESCRIPTION_ROW: &str = "visAltDescription";
/// The section the two rows are in.
pub const SECTION: &str = "User";
/// The cell of each row that holds the text.
pub const VALUE_CELL: &str = "Value";
/// The cell of each row that holds the prompt, always empty in the files that have the rows.
pub const PROMPT_CELL: &str = "Prompt";
/// The unit the files give the `Value` cell (504 of 521 title rows).
pub const STRING_UNIT: &str = "STR";

/// Where a text is found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Where {
    /// Nowhere: the sheet has no row and nothing to inherit, or only a style holds a cell of that name.
    Nowhere,
    /// In the sheet's own row.
    Own,
    /// In the row of the master shape of the master with this `ID`.
    Master(u32),
}

/// One of the two texts of a sheet, as [`read`] finds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text<'a> {
    /// The text: the value as it is. Empty when the row is missing, deleted, an error or holds no value.
    pub text: &'a str,
    /// Where the value stands.
    pub from: Where,
    /// The formula the cell holds, when it holds one that is not a string constant (the window shows the formula's text, and the exports use the result in `text`).
    pub formula: Option<&'a str>,
    /// The cell is protected by `GUARD`, on the sheet or by inheritance.
    pub guarded: bool,
}

impl Text<'_> {
    /// Whether the text holds a character that is not white space.
    pub fn has_text(&self) -> bool {
        !is_blank(self.text)
    }
}

/// The title and the description of a sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AltText<'a> {
    pub title: Text<'a>,
    pub description: Text<'a>,
}

impl AltText<'_> {
    /// Whether the sheet has alt text: its title or its description holds a character that is not white space.
    pub fn has_text(&self) -> bool {
        self.title.has_text() || self.description.has_text()
    }
}

/// Whether `text` holds no character that is not white space, by Unicode's `White_Space` (a no-break space is white space and a middle dot is not).
pub fn is_blank(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

/// The address of a row's `Value` cell.
pub fn value_addr(row: &str) -> CellAddr {
    CellAddr::in_row(SECTION, None, RowKey::name(row), VALUE_CELL)
}

/// A row of the form the files hold and a format that says the text another way reads it back as: the `Value` cell a string constant of unit `STR`, and an empty `Prompt` beside
/// it, and no other cell, attribute or data.
pub fn standard_row(name: &str, text: &str) -> Row {
    let mut row = Row { name: Some(Arc::from(name)), ..Row::default() };
    row.cells.insert(CellName::new(VALUE_CELL), Cell::constant(Value::from_text(text)).with_unit(STRING_UNIT));
    row.cells.insert(CellName::new(PROMPT_CELL), Cell::constant(Value::from_text("")));
    row
}

/// The text of a row that is exactly [`standard_row`] of the name it holds and that text, `None` for a row that is anything more or other: a formula, no unit, an error, a cell of another name,
/// another attribute. A format that holds the texts natively writes such a row as its markup and reads the markup back as this row, so only another row needs its own place in the file.
pub fn standard_text(row: &Row) -> Option<&str> {
    let name = row.name.as_deref()?;
    let text = row.cells.get(VALUE_CELL)?.value.as_ref()?.text();
    (*row == standard_row(name, text)).then_some(text)
}

fn nothing<'a>() -> Text<'a> {
    Text { text: "", from: Where::Nowhere, formula: None, guarded: false }
}

/// The formula a cell's text is shown as: none for a string constant, one written as a string literal or one that is only a marker, the formula's text for the others.
fn formula_text(cell: &Cell) -> Option<&str> {
    match &cell.formula {
        FormulaSlot::Local(f) if matches!(&f.ast, Ok(Expr::Str(_))) => None,
        FormulaSlot::Local(f) => Some(&f.source),
        FormulaSlot::Absent | FormulaSlot::Inherit | FormulaSlot::NoFormula => None,
    }
}

/// One text: the row `row` of `sheet`.
fn one<'a>(res: &Resolver<'a>, sheet: &SheetRef, row: &str) -> Text<'a> {
    let Some(found) = res.resolve(sheet, &value_addr(row)) else {
        // The sheet is not there, or the row (or the section) is deleted here: nothing is inherited past a deletion.
        return nothing();
    };
    let from = match &found.value.source {
        Source::Local => Where::Own,
        Source::Master(at) => match at.owner {
            Owner::Master(id) => Where::Master(id),
            Owner::Page(_) => Where::Nowhere,
        },
        Source::Style { .. } | Source::Default => Where::Nowhere,
    };
    let guarded = found.formula.cell.is_some_and(Cell::is_guarded);
    let formula = match &found.formula {
        Located { source: Source::Local | Source::Master(_), cell: Some(cell) } => formula_text(cell),
        Located { .. } => None,
    };
    if from == Where::Nowhere {
        return Text { guarded, formula, ..nothing() };
    }
    let text = match found.value.cell {
        // An error value counts as empty: the cell's `V` is the last good value, which is not what a reader of the sheet gets now.
        Some(cell) if cell.error.is_some() => "",
        Some(cell) => cell.value.as_ref().map_or("", |v| v.text()),
        None => "",
    };
    Text { text, from, formula, guarded }
}

/// The title and the description of `sheet`, found as R13 says: the sheet's own row, else its master shape's, else nothing. A sheet that is not in the document has none.
pub fn read<'a>(res: &Resolver<'a>, sheet: &SheetRef) -> AltText<'a> {
    AltText { title: one(res, sheet, TITLE_ROW), description: one(res, sheet, DESCRIPTION_ROW) }
}

/// [`read`] for a caller that asks about one sheet: it makes its own resolver.
pub fn read_in<'a>(doc: &'a crate::Document, sheet: &SheetRef) -> AltText<'a> {
    read(&Resolver::new(doc), sheet)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn white_space_is_unicodes_and_a_dot_above_is_not() {
        for blank in ["", " ", "\t\n", "\u{a0}", "\u{2003}\u{3000}", "\u{85}", "\u{2028}"] {
            assert!(is_blank(blank), "{blank:?}");
        }
        for text in ["a", " a ", "\u{b7}", "0", ".", "\u{200b}", "\u{feff}"] {
            assert!(!is_blank(text), "{text:?}");
        }
    }

    #[test]
    fn a_standard_row_is_the_form_of_the_files_and_only_that_reads_back_as_one() {
        let row = standard_row(TITLE_ROW, "  Pump ");
        assert_eq!(standard_text(&row), Some("  Pump "));
        assert_eq!(row.cells.get(VALUE_CELL).and_then(|c| c.unit.as_deref()), Some("STR"));
        assert_eq!(row.cells.get(PROMPT_CELL).and_then(|c| c.value.as_ref()).map(Value::text), Some(""));
        assert_eq!(standard_text(&standard_row(DESCRIPTION_ROW, "")), Some(""));
        // Anything more or other is not the standard form.
        let mut no_unit = row.clone();
        no_unit.cells.insert(CellName::new(VALUE_CELL), Cell::constant(Value::from_text("  Pump ")));
        let mut formula = row.clone();
        formula.cells.insert(CellName::new(VALUE_CELL), Cell::with_formula("Prop.Name", Some(Value::from_text("  Pump "))).with_unit("STR"));
        let mut prompt = row.clone();
        prompt.cells.insert(CellName::new(PROMPT_CELL), Cell::constant(Value::from_text("x")));
        let mut no_prompt = row.clone();
        no_prompt.cells.remove(PROMPT_CELL);
        let mut extra = row.clone();
        extra.cells.insert(CellName::new("Other"), Cell::constant(Value::from_text("1")));
        let mut error = row.clone();
        if let Some(c) = error.cells.get_mut(VALUE_CELL) {
            c.error = Some(Arc::from("#REF!"));
        }
        let mut deleted = row.clone();
        deleted.del = true;
        let mut by_index = row.clone();
        by_index.ix = Some(3);
        for (what, other) in [
            ("no unit", no_unit),
            ("a formula", formula),
            ("a prompt", prompt),
            ("no prompt", no_prompt),
            ("another cell", extra),
            ("an error", error),
            ("deleted", deleted),
            ("an index", by_index),
        ] {
            assert_eq!(standard_text(&other), None, "{what}");
        }
        assert_eq!(standard_text(&Row::default()), None, "no name, no value");
    }

    #[test]
    fn the_row_names_are_the_two_the_files_use() {
        assert_eq!((TITLE_ROW, DESCRIPTION_ROW, SECTION), ("visAltText", "visAltDescription", "User"));
        assert_ne!(TITLE_ROW, "visDescription");
    }
}
