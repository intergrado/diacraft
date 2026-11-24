//! Copy, Cut, Paste, Duplicate, Group and Ungroup from the window, over the clipboard and group commands (`docs/editing-commands.md`).
//!
//! The window's `clipboard.*` and `arrange.group` and `arrange.ungroup` name the selection and the page on view and send one engine command each:
//! `edit.copy`, `edit.cut`, `edit.paste`, `shape.duplicate`, `shape.group`, `shape.ungroup`. They are one undo step each, and **what the person
//! has selected afterwards is what the command made** (ours): the shapes pasted or duplicated, the group made, the members of the
//! groups taken apart; after a cut, nothing.
//!
//! **The clipboard is the session's**: a paste is within one document. **The system clipboard** gets something as well (ours):
//! egui carries **text or an image, one of the two** (the last asked for wins), and an image would need the page drawn from the shapes alone, so
//! what goes there is **the text of the shapes' labels**, one shape to a line, members of a group after it, in the order of the selection. A copy
//! of shapes with no text puts nothing there and leaves what another program put. Pasting from another program is not done: a paste is the
//! session's clipboard.
//!
//! **Cut sends the shapes selected and no more** (ours): a connector glued to a shape that is cut stays, its end a plain point
//! Where the Delete key takes such connectors with the shapes.

use diacraft_engine::doc::{Document, Owner, Shape};
use diacraft_engine::{EngineError, Result};
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::edit::shape_text;

/// The most shapes whose labels go to the system clipboard, and the most characters: a selection of thousands is not a text.
const MAX_LABEL_SHAPES: usize = 2_000;
const MAX_LABEL_CHARS: usize = 100_000;

/// The text of the labels of the shapes `ids` of `page` and, after each, of the shapes inside it: one non-empty label to a line.
pub fn labels_of(doc: &Document, page: u32, ids: &[u32]) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut seen = 0_usize;
    for id in ids {
        let Some(at) = doc.find_shape(Owner::Page(page), *id) else { continue };
        let Some(top) = doc.shape(&at) else { continue };
        // Depth first, a shape and then its members in order; a stack that is bounded by what is looked at.
        let mut stack: Vec<&Shape> = vec![top];
        while let Some(shape) = stack.pop() {
            seen += 1;
            if seen > MAX_LABEL_SHAPES {
                break;
            }
            let text = shape_text(shape);
            if !text.trim().is_empty() {
                lines.push(text.trim().to_owned());
            }
            stack.extend(shape.children.iter().rev().map(|child| &**child));
        }
    }
    let mut out = lines.join("\n");
    if out.chars().count() > MAX_LABEL_CHARS {
        out = out.chars().take(MAX_LABEL_CHARS).collect();
    }
    out
}

/// The `ID`s an answer holds under `key`, as numbers.
fn ids_of(answer: &Value, key: &str) -> Vec<u32> {
    answer
        .get(key)
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok())).collect())
        .unwrap_or_default()
}

impl DiacraftApp {
    /// `clipboard.copy`: `edit.copy` of the selection, and the text of its labels for the system clipboard.
    pub(crate) fn clipboard_copy(&mut self) -> Result<()> {
        let (page, ids) = self.selection_target("clipboard.copy")?;
        let labels = self.session.document().map(|doc| labels_of(doc, page, &ids)).unwrap_or_default();
        self.execute("edit.copy", &json!({"ids": ids, "page": page}))?;
        self.ui.system_copy = (!labels.is_empty()).then_some(labels);
        Ok(())
    }

    /// `clipboard.cut`: `edit.cut` of the selection (one undo step), the text of its labels for the system clipboard, and nothing selected.
    pub(crate) fn clipboard_cut(&mut self) -> Result<()> {
        let (page, ids) = self.selection_target("clipboard.cut")?;
        let labels = self.session.document().map(|doc| labels_of(doc, page, &ids)).unwrap_or_default();
        self.execute("edit.cut", &json!({"ids": ids, "page": page}))?;
        self.ui.system_copy = (!labels.is_empty()).then_some(labels);
        self.ui.edit.clear_selection_after_cut();
        Ok(())
    }

    /// `clipboard.paste`: `edit.paste` on the page on view, at `x` and `y` if both are given; the shapes pasted are selected.
    pub(crate) fn clipboard_paste(&mut self, x: Option<f64>, y: Option<f64>) -> Result<()> {
        let page =
            self.current_page().ok_or_else(|| EngineError::BadParams { cmd: "clipboard.paste".to_owned(), msg: "no document open".to_owned() })?;
        let mut params = json!({"page": page});
        if let (Some(x), Some(y), Some(map)) = (x, y, params.as_object_mut()) {
            map.insert("x".to_owned(), json!(x));
            map.insert("y".to_owned(), json!(y));
        } else if x.is_some() != y.is_some() {
            return Err(EngineError::BadParams {
                cmd: "clipboard.paste".to_owned(),
                msg: "`x` and `y` go together: give both or neither".to_owned(),
            });
        }
        let answer = self.execute("edit.paste", &params)?;
        self.select(&ids_of(&answer, "ids"));
        Ok(())
    }

    /// `clipboard.duplicate`: `shape.duplicate` of the selection by `dx` and `dy` (a quarter inch right and down by default); the copies are selected.
    pub(crate) fn clipboard_duplicate(&mut self, dx: Option<f64>, dy: Option<f64>) -> Result<()> {
        let (page, ids) = self.selection_target("clipboard.duplicate")?;
        let mut params = json!({"ids": ids, "page": page});
        if let Some(map) = params.as_object_mut() {
            if let Some(dx) = dx {
                map.insert("dx".to_owned(), json!(dx));
            }
            if let Some(dy) = dy {
                map.insert("dy".to_owned(), json!(dy));
            }
        }
        let answer = self.execute("shape.duplicate", &params)?;
        let made = ids_of(&answer, "ids");
        self.select(&made);
        self.keep_preview(page, &made);
        Ok(())
    }

    /// `arrange.group`: `shape.group` of the selection; the group is selected.
    pub(crate) fn arrange_group(&mut self) -> Result<()> {
        let (page, ids) = self.selection_target("arrange.group")?;
        let answer = self.execute("shape.group", &json!({"ids": ids, "page": page}))?;
        if let Some(id) = answer.get("id").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()) {
            self.select(&[id]);
        }
        Ok(())
    }

    /// `arrange.ungroup`: `shape.ungroup` of the selection; the members are selected.
    pub(crate) fn arrange_ungroup(&mut self) -> Result<()> {
        let (page, ids) = self.selection_target("arrange.ungroup")?;
        let answer = self.execute("shape.ungroup", &json!({"ids": ids, "page": page}))?;
        self.select(&ids_of(&answer, "ids"));
        Ok(())
    }

    /// The text of the labels the last copy or cut asked to put on the system clipboard, if it is still waiting: taken by the frame, which has the
    /// context egui's clipboard is reached through.
    pub(crate) fn take_system_copy(&mut self) -> Option<String> {
        self.ui.system_copy.take()
    }
}
