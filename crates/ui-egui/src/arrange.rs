//! Arranging the selection from the window: rotate, flip, bring to front and send to back, align.
//!
//! The window's `arrange.*` commands name the selected shapes and the page on view and send the engine's `shape.rotate`, `shape.flip`,
//! `shape.order` or `shape.align` (`docs/editing-commands.md`). The engine has no shortcut for any of them (one command with a parameter does
//! the work of several keys), so the window maps the keys (`shortcuts.rs`): Rotate Left is a turn of 90 degrees counterclockwise and Rotate
//! Right one of -90, which is the engine's convention (a positive angle is leftward). **The first shape selected is the one the others align
//! to**, which is the order the window keeps its selection in.

use diacraft_engine::Result;
use serde_json::json;

use crate::app::DiacraftApp;

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
