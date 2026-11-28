//! Documents and sessions in known states.
// A fixture that cannot be built must fail its test, like an assertion; the shipped-code ban on
// panicking (AGENTS.md, Never crash) is for code that runs for users.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use diacraft_doc::{CellName, Document};
use diacraft_engine::Session;
use serde_json::json;

/// A blank document: what `file.new` makes.
pub fn blank_document() -> Document {
    Document::new_blank()
}

/// `doc` as a new drawing made it before its root style stated the text cells the renderer assumes ([`diacraft_doc::root_text`]): the whole `Paragraph` section, the text
/// block's cells beyond the vertical alignment and the margins, and the cells of the first run beyond its size and colour are taken out of the root style (`ID` 0), and nothing else is
/// touched. For the pins that are the stamp or the bytes of a model that builds before it made: with the cells out the model is the one such a pin was taken of, so the pin goes on saying
/// that the encoder is as it was, and the pin of the model as it is now says what the cells moved.
pub fn without_root_text_cells(mut doc: Document) -> Document {
    if let Some(at) = doc.styles.iter().position(|style| style.id == 0)
        && let Some(root) = doc.styles.get_mut(at)
    {
        let sheet = Arc::make_mut(&mut Arc::make_mut(root).sheet);
        // What a new drawing's root held before: the vertical alignment, and the size and colour of the first run.
        let held_before = |cell: &diacraft_doc::root_text::RootCell| {
            matches!((cell.section, cell.name), (None, "VerticalAlign") | (Some("Character"), "Size" | "Color"))
        };
        for cell in diacraft_doc::root_text::CELLS.iter().filter(|cell| !held_before(cell)) {
            sheet.remove_cell(&cell.addr());
        }
        sheet.remove_sections(|section| section.name.as_str() == "Paragraph");
    }
    doc
}

/// `doc` as a new drawing made it before the root style held `DropOnPageScale` (the ruling on F40) and before it stated the text cells (
/// [`without_root_text_cells`]): the cell is taken out of the root style (`ID` 0), and nothing else is touched. For the pins that are the stamp or the bytes of a model that builds before it
/// made: with the cell out the model is the one such a pin was taken of, so the pin goes on saying that the encoder is as it was, and the pin of the model as it is now says what the cell moved.
pub fn without_root_factor(doc: Document) -> Document {
    let mut doc = without_root_text_cells(doc);
    if let Some(at) = doc.styles.iter().position(|style| style.id == 0)
        && let Some(root) = doc.styles.get_mut(at)
    {
        Arc::make_mut(&mut Arc::make_mut(root).sheet).cells.remove(&CellName::new("DropOnPageScale"));
    }
    doc
}

/// A session whose document is blank: `file.new` has run, so it has one page, no undo steps and a
/// one-entry journal.
pub fn blank_session() -> Session {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).expect("file.new works on a new session");
    session
}
