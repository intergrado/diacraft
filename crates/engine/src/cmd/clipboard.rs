//! The clipboard: copy, cut, paste and duplicate. The rules are in `draw/clipboard.rs` and `docs/editing-commands.md`.
//! The clipboard is the session's, for the document it was taken from; the system clipboard is the window's business.
//!
//! Every length is in paper inches, as in the other shape commands.

use serde_json::{Value, json};

use super::params::{optional_number, optional_whole, required_ids, units_per_inch};
use super::{CommandSpec, has_document};
use crate::Session;
use crate::draw::{self, Placement};
use crate::error::{Result, bad, failed};

/// A step of the offset a paste or a duplicate puts a copy at, in paper inches.
const STEP: f64 = 0.25;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "edit.copy",
            "Copy",
            ["Home", "Clipboard"],
            Some("Cmd+C"),
            "ids: array of whole numbers, the shapes to copy (with everything inside them); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            copy
        ),
        cmd!(
            "edit.cut",
            "Cut",
            ["Home", "Clipboard"],
            Some("Cmd+X"),
            "ids: array of whole numbers, the shapes to cut: copied, then deleted as shape.delete does, in one undo step; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            cut
        ),
        cmd!(
            "edit.paste",
            "Paste",
            ["Home", "Clipboard"],
            Some("Cmd+V"),
            "page: whole number, optional, the id of the page to paste on (default: the page the shapes were copied from); \
             x: number, optional, inches from the page's left edge where the middle of the pasted shapes goes (give x and y, or neither: \
             the shapes are then a quarter of an inch right and down of where they were, a step more for each paste of the same copy); \
             y: number, optional, inches from the page's bottom edge, see x",
            can_paste,
            paste
        ),
        cmd!(
            "shape.duplicate",
            "Duplicate",
            ["Home", "Clipboard"],
            Some("Cmd+D"),
            "ids: array of whole numbers, the shapes to duplicate (with everything inside them): copied and pasted on the same page at once, \
             the clipboard left as it is; dx: number, optional, inches to the right of the originals (default 0.25); \
             dy: number, optional, inches up from the originals (default -0.25); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            duplicate
        ),
    ]
}

/// A paste can run when the document has something on its clipboard.
fn can_paste(s: &Session) -> std::result::Result<(), String> {
    has_document(s)?;
    if s.clipboard().is_some_and(|c| !c.is_empty()) { Ok(()) } else { Err("the clipboard is empty".to_owned()) }
}

/// The page the command works on: the one named, else the first page holding all `ids`.
fn target_page(s: &Session, p: &Value, ids: &[u32], cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    draw::page_holding(doc, named, ids).ok_or_else(|| bad(cmd, "the document has no pages"))
}

/// The drawing units to a paper inch on a page.
fn page_scale(s: &Session, page: u32) -> f64 {
    s.document().map_or(1.0, |doc| units_per_inch(doc, page))
}

/// Puts the shapes on the clipboard → `{page, copied}`: how many shapes were copied, not counting their members. The document is not changed, and
/// the call is no undo step.
fn copy(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "edit.copy";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let clip = draw::copy_shapes(doc, page, &ids).map_err(|problem| problem.into_engine(ID))?;
    let copied = clip.len();
    s.set_clipboard(Some(clip));
    Ok(json!({ "page": page, "copied": copied }))
}

/// Copies the shapes and deletes them → `{page, cut}`, one undo step.
fn cut(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "edit.cut";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let clip = s.edit("Cut", |doc| draw::cut_shapes(doc, page, &ids).map_err(|problem| problem.into_engine(ID)))?;
    let cut = clip.len();
    s.set_clipboard(Some(clip));
    Ok(json!({ "page": page, "cut": cut }))
}

/// What a paste or a duplicate answers: `{page, ids, created}`, and `notes`, words that say what it left out, only when there are some (a master it put back held references to parts
/// of its file).
fn paste_answer(page: u32, pasted: &draw::Pasted) -> Value {
    let mut answer = json!({ "page": page, "ids": pasted.ids, "created": pasted.created });
    if !pasted.notes.is_empty() {
        answer["notes"] = json!(pasted.notes);
    }
    answer
}

/// Pastes the clipboard → `{page, ids, created}`, and `notes` when it left something out (see [`paste_answer`]): the `ID`s of the shapes pasted (not their members), and how many shapes were made in all.
fn paste(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "edit.paste";
    let at = (optional_number(p, "x", ID)?, optional_number(p, "y", ID)?);
    if at.0.is_some() != at.1.is_some() {
        return Err(bad(ID, "`x` and `y` go together: give both or neither"));
    }
    let clip = s.clipboard().filter(|c| !c.is_empty()).cloned().ok_or_else(|| failed(ID, "the clipboard is empty"))?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let page = match optional_whole(p, "page", ID)? {
        Some(page) => page,
        None if doc.page(clip.source_page()).is_some() => clip.source_page(),
        None => doc.pages.iter().next().map(|first| first.id).ok_or_else(|| bad(ID, "the document has no pages"))?,
    };
    let k = page_scale(s, page);
    let placement = match at {
        (Some(x), Some(y)) => Placement::CenterAt(x * k, y * k),
        _ => {
            // On the page the shapes came from the first paste is a step off; on another page it is where they were.
            let steps = f64::from(clip.pastes()) + if page == clip.source_page() { 1.0 } else { 0.0 };
            Placement::Offset(STEP * steps * k, -STEP * steps * k)
        }
    };
    let above = s.id_floor(page);
    let pasted = s.edit("Paste", |doc| {
        let pasted = draw::paste_shapes(doc, page, &clip, placement, above).map_err(|problem| problem.into_engine(ID))?;
        // A page with Auto Size on grows to take the copies in (`draw/page_fit.rs`), in the same undo step.
        draw::fit_page(doc, page, &pasted.ids).map_err(|problem| problem.into_engine(ID))?;
        Ok(pasted)
    })?;
    s.note_id(page, pasted.highest);
    s.set_clipboard(Some(clip.pasted_once_more()));
    Ok(paste_answer(page, &pasted))
}

/// Copies the shapes and pastes them on the same page at once → `{page, ids, created}` as `edit.paste`.
fn duplicate(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.duplicate";
    let ids = required_ids(p, "ids", ID)?;
    let dx = optional_number(p, "dx", ID)?.unwrap_or(STEP);
    let dy = optional_number(p, "dy", ID)?.unwrap_or(-STEP);
    let page = target_page(s, p, &ids, ID)?;
    let k = page_scale(s, page);
    let above = s.id_floor(page);
    let pasted = s.edit("Duplicate shapes", |doc| {
        let pasted = draw::duplicate_shapes(doc, page, &ids, (dx * k, dy * k), above).map_err(|problem| problem.into_engine(ID))?;
        draw::fit_page(doc, page, &pasted.ids).map_err(|problem| problem.into_engine(ID))?;
        Ok(pasted)
    })?;
    s.note_id(page, pasted.highest);
    Ok(paste_answer(page, &pasted))
}
