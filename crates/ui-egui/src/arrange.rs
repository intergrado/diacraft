//! Arranging the selection from the window: rotate, flip, bring to front and send to back, align.
//!
//! The window's `arrange.*` commands name the selected shapes and the page on view and send the engine's `shape.rotate`, `shape.flip`,
//! `shape.order` or `shape.align` (`docs/editing-commands.md`). The engine has no shortcut for any of them (one command with a parameter does
//! the work of several keys), so the window maps the keys (`shortcuts.rs`): Rotate Left is a turn of 90 degrees counterclockwise and Rotate
//! Right one of -90, which is the engine's convention (a positive angle is leftward). **The first shape selected is the one the others align
//! to**, which is the order the window keeps its selection in.

use diacraft_engine::{EngineError, Result};
use serde_json::{Value, json};

use crate::app::DiacraftApp;

/// The most shapes one `connector.reverse_ends` call takes (its `ids`, as the other commands that take many).
const REVERSE_CALL: usize = 4_096;

/// The calls of `connector.reverse_ends` a selection goes in: its shapes in order, [`REVERSE_CALL`] to a call, each naming the page.
fn reverse_calls(ids: &[u32], page: u32) -> Vec<Value> {
    ids.chunks(REVERSE_CALL).map(|chunk| json!({"ids": chunk, "page": page})).collect()
}

impl DiacraftApp {
    /// `arrange.rotate`: turns the selection by `by` degrees counterclockwise (`shape.rotate`).
    pub(crate) fn arrange_rotate(&mut self, by: f64) -> Result<()> {
        let (page, ids) = self.selection_target("arrange.rotate")?;
        self.arrange("shape.rotate", &json!({"ids": ids, "by": by, "page": page}), page, &ids, true)
    }

    /// `arrange.flip`: flips the selection across `axis`, `horizontal` or `vertical` (`shape.flip`).
    pub(crate) fn arrange_flip(&mut self, axis: &str) -> Result<()> {
        let (page, ids) = self.selection_target("arrange.flip")?;
        self.arrange("shape.flip", &json!({"ids": ids, "axis": axis, "page": page}), page, &ids, true)
    }

    /// Whether `arrange.reverseEnds` can run: a drawing with the shapes selected, one of them a connector or a line. With no scene to ask it says yes and leaves the
    /// command to refuse.
    pub(crate) fn reverse_ends_enabled(&self) -> std::result::Result<(), String> {
        if self.session().document().is_none() {
            return Err("no document open".to_owned());
        }
        if self.ui.edit.selection().is_empty() {
            return Err("nothing is selected".to_owned());
        }
        if self.selection_kinds().0 { Ok(()) } else { Err("select a connector or a line: Reverse Ends swaps its begin and end".to_owned()) }
    }

    /// `arrange.reverseEnds`: `connector.reverse_ends` of the connectors and lines among the shapes selected. The other shapes of the selection are not sent, so that
    /// a box selected with its connectors is no refusal. **A selection of more shapes than one call takes** goes in calls of one batch, one undo step. What the engine left alone
    /// (a protected end, a path it cannot reverse) is said in the status bar, in its words.
    pub(crate) fn arrange_reverse_ends(&mut self) -> Result<()> {
        let (page, _) = self.selection_target("arrange.reverseEnds")?;
        let connectors = self.selected_connectors();
        if connectors.is_empty() {
            return Err(EngineError::BadParams {
                cmd: "arrange.reverseEnds".to_owned(),
                msg: "select a connector or a line: Reverse Ends swaps its begin and end".to_owned(),
            });
        }
        // A label being typed is sent first, as for Change Shape.
        self.commit_pending_text();
        let before = self.session.revision();
        let calls = reverse_calls(&connectors, page);
        let answers: Vec<Value> = match calls.as_slice() {
            [only] => vec![self.execute("connector.reverse_ends", only)?],
            many => {
                let steps: Vec<Value> = many.iter().map(|params| json!({"command": "connector.reverse_ends", "params": params})).collect();
                let done = self.execute("command.batch", &json!({"commands": steps, "label": "Reverse ends"}))?;
                done["results"].as_array().cloned().unwrap_or_default()
            }
        };
        if self.session.revision() != before {
            self.keep_preview(page, &connectors);
        }
        // The engine's own words, when it left something as it was.
        let left_alone = |answer: &&Value| ["locked", "skipped", "geometry_kept"].iter().any(|key| answer.get(*key).is_some());
        if let Some(says) = answers.iter().find(left_alone).and_then(|answer| answer["says"].as_str()) {
            self.set_notice(says);
        }
        Ok(())
    }

    /// `arrange.order`: moves the selection in the drawing order, `front`, `back`, `forward` or `backward` (`shape.order`).
    pub(crate) fn arrange_order(&mut self, to: &str) -> Result<()> {
        let (page, ids) = self.selection_target("arrange.order")?;
        self.arrange("shape.order", &json!({"ids": ids, "to": to, "page": page}), page, &ids, false)
    }

    /// `arrange.align`: moves the shapes selected after the first to it (`shape.align`).
    pub(crate) fn arrange_align(&mut self, how: &str) -> Result<()> {
        let (page, ids) = self.selection_target("arrange.align")?;
        self.arrange("shape.align", &json!({"ids": ids, "how": how, "page": page}), page, &ids, true)
    }

    /// `arrange.changeShape`: `shape.change` of the selection to the master `master` (of the stencil `stencil`, if one is named). The shapes keep their `ID`s, so the selection
    /// stays. A shape that could not be changed is said in the notice (put there once the command has ended, `DiacraftApp::execute`), with the reason the engine gave.
    pub(crate) fn arrange_change_shape(&mut self, master: &str, stencil: Option<&str>) -> Result<()> {
        let (page, ids) = self.selection_target("arrange.changeShape")?;
        let mut params = json!({"ids": ids, "master": master, "page": page});
        if let (Some(map), Some(stencil)) = (params.as_object_mut(), stencil) {
            map.insert("stencil".to_owned(), json!(stencil));
        }
        // A label being typed is sent first, as `format_shape` sends it.
        self.commit_pending_text();
        let answer = self.execute("shape.change", &params)?;
        if let Some(first) = answer["skipped"].as_array().and_then(|skipped| skipped.first()) {
            let count = answer["skipped"].as_array().map_or(0, Vec::len);
            let why = first["why"].as_str().unwrap_or("");
            let more = if count > 1 { format!(" ({} more shapes were left as they are)", count - 1) } else { String::new() };
            self.ui.change_note = Some(format!("{why}{more}"));
        }
        Ok(())
    }

    /// Sends one arranging command. A command that moves shapes keeps their outlines painted at the new places until the picture shows them.
    fn arrange(&mut self, command: &str, params: &serde_json::Value, page: u32, ids: &[u32], moves: bool) -> Result<()> {
        let before = self.session.revision();
        self.execute(command, params)?;
        if moves && self.session.revision() != before {
            self.keep_preview(page, ids);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_past_one_calls_ids_goes_in_calls_that_hold_every_shape_once_in_order() {
        let ids: Vec<u32> = (0..10_000).collect();
        let calls = reverse_calls(&ids, 7);
        assert_eq!(calls.len(), 3);
        let sizes: Vec<usize> = calls.iter().map(|c| c["ids"].as_array().map_or(0, Vec::len)).collect();
        assert_eq!(sizes, [4_096, 4_096, 1_808]);
        let again: Vec<u64> = calls.iter().flat_map(|c| c["ids"].as_array().cloned().unwrap_or_default()).filter_map(|v| v.as_u64()).collect();
        assert_eq!(again, (0..10_000_u64).collect::<Vec<_>>());
        assert!(calls.iter().all(|c| c["page"] == 7));
        assert_eq!(reverse_calls(&ids[..1], 1).len(), 1, "one shape is one call");
        assert!(reverse_calls(&[], 1).is_empty(), "no shape, no call");
    }
}
