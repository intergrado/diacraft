//! Insert, Diagram Parts, Callout: a press on a tile of the gallery adds a callout (`window.callout {style?}`, which sends the engine's `callout.insert`).
//!
//! *"With a shape selected, the callout is added associated with it ... With nothing selected it appears at the middle of the view, not associated."* So the window sends the shapes selected on the
//! page on view as `ids`, or, when none is selected, the middle of the drawing area as `x` and `y`; the new callouts become the selection, so the yellow handle at the end of the leader shows
//! (`edit/callout.rs`) and the text can be typed at once. The gallery is `ribbon/callouts.rs`.

use diacraft_engine::{EngineError, Result};
use serde_json::{Value, json};

use crate::app::DiacraftApp;

impl DiacraftApp {
    /// Whether a callout can be added now: the engine's `callout.insert` can run (a drawing is open).
    pub(crate) fn callout_enabled(&self) -> std::result::Result<(), String> {
        self.command_enabled("callout.insert")
    }

    /// `callout.insert` of the style `style` (the first tile when `None`) on the page on view, for the shapes selected on it, or at the middle of the view when none is. The callouts made are the
    /// selection. → the engine's answer; a refusal is the engine's, in words.
    pub fn insert_callout(&mut self, style: Option<&str>) -> Result<Value> {
        const ID: &str = "window.callout";
        let page = self.current_page().ok_or_else(|| EngineError::BadParams { cmd: ID.to_owned(), msg: "no page is open".to_owned() })?;
        // A text box that is open ends first: what was typed is kept.
        self.commit_pending_text();
        let mut params = json!({ "page": page });
        if let Some(map) = params.as_object_mut() {
            if let Some(style) = style {
                map.insert("style".to_owned(), json!(style));
            }
            let selected: Vec<u32> = if self.ui.edit.selection_page() == Some(page) { self.ui.edit.selection().to_vec() } else { Vec::new() };
            if selected.is_empty() {
                if let Some(cx) = self.cx_of(page) {
                    let (x, y) = cx.page_at(cx.area.center());
                    if x.is_finite() && y.is_finite() {
                        map.insert("x".to_owned(), json!(x));
                        map.insert("y".to_owned(), json!(y));
                    }
                }
            } else {
                map.insert("ids".to_owned(), json!(selected));
            }
        }
        let answer = self.execute("callout.insert", &params)?;
        let made: Vec<u32> = answer
            .get("ids")
            .and_then(Value::as_array)
            .map(|l| l.iter().filter_map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok())).collect())
            .unwrap_or_default();
        self.select(&made);
        Ok(answer)
    }
}
