//! Alt text of a shape, a page and a master: the title and the description a screen reader speaks in place of the picture.
//!
//! **What is kept in cells.** The two texts are the `Value` cells of the rows `visAltText` (the title) and `visAltDescription` (the description) of the sheet's `User` section, as the public files
//! hold them, and a read is [`diacraft_doc::alt_text::read`]: the sheet's own row, else its master shape's, else nothing. A page's and a shape's sheet are what they say; **a master's
//! texts are those of its master shape** (the Edit Master Shape window's Alt Text section), and the master's page sheet is not read.
//!
//! **Project rules** (ours; the spec leaves them open, and `docs/editing-commands.md`, "Alt text and the accessibility check", lists them):
//!
//! * A set writes **the sheet's own row and never the master's**: an own `Value` cell becomes a string constant (`U` `STR`, no formula, the cell's other attributes kept); a sheet with no
//!   own cell gets one, in a row made for it, with an empty `Prompt` beside it, the form the files hold. A text equal to the own value, as a constant, writes nothing, and **a text that clears a sheet
//!   that has no text writes nothing**: clearing is setting the empty string, **never deleting the row** (an instance whose master has a title shows none once its own is cleared).
//!   Setting both texts is one undo step and writes nothing else.
//! * A text typed in replaces a formula of the cell with the constant; **a cell a `GUARD` protects, on the sheet or by inheritance, is refused in words** and nothing changes.
//! * A text is at most [`MAX_TEXT_CHARS`] characters and holds none of the controls XML 1.0 cannot hold (a control other than tab, line feed and carriage return, U+FFFE and
//!   U+FFFF); it is kept **character for character**: no trimming, no case change, no change of a line break (the window's one-line Title box turns a pasted line break into a space before it asks).
//!   A value of a file that is longer than the cap is read whole by the doc crate, and answered cut ([`Part::length`] says how long it is).
//! * A master is named with its shape: `shape`, or, left out, **the master's one top-level shape** that is not deleted; a master with none or several is refused in words that name them.
//!
//! **Hostile input.** A call checks every text before anything is written; the rows are written in one batch (all or nothing), through the document's own checks of names and sizes.

use diacraft_doc::alt_text::{self, AltText, Where};
use diacraft_doc::{
    Cell, CellAddr, CellEdit, CellWrite, Document, FormulaSlot, Lookup, Owner, Resolver, ShapeRef, SheetRef, Value, WriteOrigin, WriteOutcome,
};

use super::{Problem, require_page, require_shape};

/// The most characters one text is given with, and the most a read answers whole.
pub const MAX_TEXT_CHARS: usize = 32_768;
/// The most shape ids a refusal of a master with several shapes names.
const MAX_NAMED: usize = 8;

/// What a call works on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A shape of a page, by its `ID`: a group, a member of a group, an instance.
    Shape { page: u32, id: u32 },
    /// A page.
    Page(u32),
    /// A master, and one of its shapes (the master's one top-level shape when `None`).
    Master { id: u32, shape: Option<u32> },
}

/// The sheet a target is, checked: the sheet and, for a sheet on a page, the page (what a recalculation after a write works on).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    pub sheet: SheetRef,
    pub page: Option<u32>,
    /// The `ID` of the shape the texts are the row of, for a shape and a master.
    pub shape: Option<u32>,
}

/// The ids of a master's top-level shapes that are not deleted, in order.
fn live_top_level(doc: &Document, master: u32) -> Vec<u32> {
    doc.master(master).map(|m| m.shapes.iter().filter(|s| !s.del).map(|s| s.id).collect()).unwrap_or_default()
}

/// The sheet `target` names, which must be there ([`Target::Master`]).
pub fn locate(doc: &Document, target: &Target) -> Result<Located, Problem> {
    match target {
        Target::Shape { page, id } => {
            let at = require_shape(doc, *page, *id)?;
            Ok(Located { sheet: SheetRef::Shape(at), page: Some(*page), shape: Some(*id) })
        }
        Target::Page(page) => {
            require_page(doc, *page)?;
            Ok(Located { sheet: SheetRef::Page(*page), page: Some(*page), shape: None })
        }
        Target::Master { id, shape } => {
            if doc.master(*id).is_none() {
                return Err(Problem::bad(format!("there is no master {id}")));
            }
            let shape_id = match shape {
                Some(shape_id) => *shape_id,
                None => match live_top_level(doc, *id).as_slice() {
                    [only] => *only,
                    [] => return Err(Problem::bad(format!("master {id} has no shape to carry alt text"))),
                    many => {
                        let named: Vec<String> = many.iter().take(MAX_NAMED).map(u32::to_string).collect();
                        let more = many.len().saturating_sub(MAX_NAMED);
                        return Err(Problem::bad(format!(
                            "master {id} has {} top-level shapes (ids {}{}): give `shape`, the id of the one that carries the alt text",
                            many.len(),
                            named.join(", "),
                            if more > 0 { format!(" and {more} more") } else { String::new() }
                        )));
                    }
                },
            };
            let at: ShapeRef =
                doc.find_shape(Owner::Master(*id), shape_id).ok_or_else(|| Problem::bad(format!("master {id} has no shape {shape_id}")))?;
            Ok(Located { sheet: SheetRef::Shape(at), page: None, shape: Some(shape_id) })
        }
    }
}

/// One text, as an answer holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// The text, cut at [`MAX_TEXT_CHARS`] characters.
    pub text: String,
    /// How many characters the whole text has.
    pub length: usize,
    pub from: Where,
    /// The formula the cell holds when it is not a string constant.
    pub formula: Option<String>,
    /// A `GUARD` protects the cell.
    pub protected: bool,
}

impl Part {
    fn of(text: &alt_text::Text<'_>) -> Part {
        let end = text.text.char_indices().nth(MAX_TEXT_CHARS).map_or(text.text.len(), |(at, _)| at);
        let whole = text.text.get(..end).unwrap_or_default();
        let length = if end == text.text.len() { whole.chars().count() } else { text.text.chars().count() };
        Part {
            text: whole.to_owned(),
            length,
            from: text.from,
            formula: text.formula.map(|f| f.chars().take(MAX_TEXT_CHARS).collect()),
            protected: text.guarded,
        }
    }
}

/// The title and the description of a sheet as an answer holds them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read {
    pub title: Part,
    pub description: Part,
    /// Whether the sheet has alt text.
    pub has_alt_text: bool,
}

/// What the sheet has now.
pub fn read(doc: &Document, at: &Located) -> Read {
    let found = alt_text::read_in(doc, &at.sheet);
    Read { title: Part::of(&found.title), description: Part::of(&found.description), has_alt_text: found.has_text() }
}

/// Whether `text` is one a call may give: at most [`MAX_TEXT_CHARS`] characters, and no character XML 1.0 cannot hold.
pub fn check_text(what: &str, text: &str) -> Result<(), Problem> {
    if text.chars().take(MAX_TEXT_CHARS + 1).count() > MAX_TEXT_CHARS {
        return Err(Problem::bad(format!("the {what} must be at most {MAX_TEXT_CHARS} characters")));
    }
    if text.chars().any(|c| (c < ' ' && !matches!(c, '\t' | '\n' | '\r')) || matches!(c, '\u{fffe}' | '\u{ffff}')) {
        return Err(Problem::bad(format!("the {what} must not hold control characters other than tab, line feed and carriage return")));
    }
    Ok(())
}

/// The cell `name` of the row `row` of the sheet's own `User` section, if the sheet has one of its own.
fn own_cell<'a>(res: &Resolver<'a>, sheet: &SheetRef, row: &str, name: &str) -> Option<&'a Cell> {
    let local = res.sheet(sheet)?;
    let addr = CellAddr::in_row(alt_text::SECTION, None, diacraft_doc::RowKey::name(row), name);
    match res.lookup(local, &addr) {
        Lookup::Found(cell) => Some(cell),
        Lookup::Missing | Lookup::Deleted => None,
    }
}

/// Whether a cell is already a string constant of exactly this text: nothing to write.
fn already(cell: &Cell, text: &str) -> bool {
    matches!(cell.formula, FormulaSlot::Absent) && cell.error.is_none() && cell.value.as_ref().is_some_and(|v| v.text() == text)
}

/// The writes that set one text of the sheet, or none when there is nothing to change, or why the text may not be set.
fn writes_for(
    res: &Resolver<'_>,
    sheet: &SheetRef,
    row: &str,
    what: &str,
    now: &alt_text::Text<'_>,
    text: &str,
    out: &mut Vec<(SheetRef, CellWrite)>,
) -> Result<(), Problem> {
    let own = own_cell(res, sheet, row, alt_text::VALUE_CELL);
    match own {
        Some(cell) if already(cell, text) => return Ok(()),
        // Clearing what is not there: no row is made to hold an empty text.
        None if text.is_empty() && now.text.is_empty() => return Ok(()),
        _ => {}
    }
    if now.guarded {
        return Err(Problem::failed(format!("the {what} is protected by GUARD and cannot be changed here")));
    }
    // The cell the sheet keeps: its own, as a constant string, with its other attributes; or a new one.
    let mut cell = own.cloned().unwrap_or_default();
    cell.formula = FormulaSlot::Absent;
    cell.value = Some(Value::from_text(text));
    cell.error = None;
    cell.unit = Some(std::sync::Arc::from(alt_text::STRING_UNIT));
    let write = |addr: CellAddr, cell: Cell| (sheet.clone(), CellWrite { addr, edit: CellEdit::Cell(cell), origin: WriteOrigin::Drawing });
    out.push(write(alt_text::value_addr(row), cell));
    // A row made for the text holds an empty prompt beside it.
    if own_cell(res, sheet, row, alt_text::PROMPT_CELL).is_none() {
        let prompt = CellAddr::in_row(alt_text::SECTION, None, diacraft_doc::RowKey::name(row), alt_text::PROMPT_CELL);
        out.push(write(prompt, Cell::constant(Value::from_text(""))));
    }
    Ok(())
}

/// Sets the title and the description given of the sheet: → whether anything changed. All or nothing: a text a call may not give, a cell a `GUARD` protects and a write the model refuses
/// change nothing.
pub fn apply(doc: &mut Document, at: &Located, title: Option<&str>, description: Option<&str>) -> Result<bool, Problem> {
    if let Some(text) = title {
        check_text("title", text)?;
    }
    if let Some(text) = description {
        check_text("description", text)?;
    }
    let mut writes: Vec<(SheetRef, CellWrite)> = Vec::new();
    {
        let res = Resolver::new(doc);
        let now: AltText<'_> = alt_text::read(&res, &at.sheet);
        if let Some(text) = title {
            writes_for(&res, &at.sheet, alt_text::TITLE_ROW, "title", &now.title, text, &mut writes)?;
        }
        if let Some(text) = description {
            writes_for(&res, &at.sheet, alt_text::DESCRIPTION_ROW, "description", &now.description, text, &mut writes)?;
        }
    }
    if writes.is_empty() {
        return Ok(false);
    }
    let touched: Vec<(SheetRef, CellAddr)> =
        writes.iter().filter(|(_, w)| w.addr.cell.as_str() == alt_text::VALUE_CELL).map(|(s, w)| (s.clone(), w.addr.clone())).collect();
    let outcomes = doc.write_batch(writes).map_err(Problem::from)?;
    if outcomes.contains(&WriteOutcome::KeptGuarded) {
        return Err(Problem::failed("the cell is protected by GUARD and cannot be changed here"));
    }
    if let Some(page) = at.page {
        super::settle::settle_cells(doc, page, touched)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn a_text_is_checked_at_the_cap_and_for_the_characters_xml_cannot_hold() {
        assert!(check_text("title", "").is_ok());
        assert!(check_text("title", &"a".repeat(MAX_TEXT_CHARS)).is_ok());
        let words = check_text("title", &"a".repeat(MAX_TEXT_CHARS + 1)).unwrap_err().message;
        assert!(words.contains("title") && words.contains("32768"), "{words}");
        // The cap counts characters, not bytes.
        assert!(check_text("title", &"\u{e9}".repeat(MAX_TEXT_CHARS)).is_ok());
        for fine in ["tab\there", "line\nbreak", "carriage\rreturn", "\u{7f}\u{85}", "\u{a0}", "emoji \u{1f600}"] {
            assert!(check_text("description", fine).is_ok(), "{fine:?}");
        }
        for bad in ["\u{0}", "\u{1}", "\u{b}", "\u{c}", "\u{1b}", "\u{1f}", "\u{fffe}", "\u{ffff}"] {
            let words = check_text("description", bad).unwrap_err().message;
            assert!(words.contains("description") && words.contains("control characters"), "{bad:?}: {words}");
        }
    }

    #[test]
    fn a_part_is_cut_at_the_cap_and_says_how_long_the_whole_text_is() {
        let long = "x".repeat(MAX_TEXT_CHARS + 10);
        let text = diacraft_doc::alt_text::Text { text: &long, from: Where::Own, formula: None, guarded: false };
        let part = Part::of(&text);
        assert_eq!((part.text.chars().count(), part.length), (MAX_TEXT_CHARS, MAX_TEXT_CHARS + 10));
        let short = diacraft_doc::alt_text::Text { text: "caf\u{e9}", from: Where::Master(3), formula: Some("Prop.Name"), guarded: true };
        let part = Part::of(&short);
        assert_eq!(
            (part.text.as_str(), part.length, part.from, part.formula.as_deref(), part.protected),
            ("caf\u{e9}", 4, Where::Master(3), Some("Prop.Name"), true)
        );
    }
}
