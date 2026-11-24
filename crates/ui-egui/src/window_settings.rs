//! What a drawing says of how its window shows it: whether the grid, the rulers, the guides and the connection points show (the View tab's Ruler, Grid and Guides
//! boxes and its Visual Aids' Connection Points).
//!
//! **The settings are the drawing's**: the model holds them as a VSDX file's windows part holds them (`diacraft_doc::WindowFlags`, the
//! flags of the drawing `Window`), so a save writes them in every format, and the engine says what a drawing shows ([`diacraft_engine::view`]): a drawing with no windows part
//! shows what a new drawing shows (the grid, the rulers and the guides on, the connection points off), and one with a part shows its flags, with the grid and the rulers off,
//! the guides on and the connection points off where its window holds no value.
//!
//! **The window is thin.** It shows what [`window_settings_of`] reads of the drawing when one is opened or made, and again after any command that changed the drawing's
//! windows part (a batch, a program's `window.settings.set`: [`DiacraftApp::followed_window_settings`]); and a box turned runs the engine's `window.settings.set`, which
//! makes the drawing unsaved and **is no undo step** (the owner's ruling, 2026-11-22: "Unsaved, not undo"): Ctrl+Z goes on undoing the drawing's edits and leaves
//! the boxes as they were turned (`crate::commands`). The window's own toggles ([`crate::state::UiState`]'s `show_grid` and the
//! rest) are what it draws from, and always what the drawing shows when a command has ended.

use std::sync::Arc;

use diacraft_engine::doc::{Document, RawElement, WindowFlags};
pub use diacraft_engine::view::{NEW_DRAWING, ViewSettings};
use serde_json::json;

use crate::app::DiacraftApp;

/// What `doc` shows ([`diacraft_engine::view::view_settings`]).
pub fn window_settings_of(doc: &Document) -> ViewSettings {
    diacraft_engine::view::view_settings(doc)
}

/// The windows part a drawing holds: what the window compares, by pointer, to know that a command changed it.
pub(crate) fn windows_part(doc: Option<&Document>) -> Option<Arc<RawElement>> {
    doc.and_then(|d| d.vsdx.as_ref()).and_then(|held| held.windows.clone())
}

/// Whether two windows parts are the same allocation (or both none): a part a command did not write is the very one the drawing held.
pub(crate) fn same_part(a: Option<&Arc<RawElement>>, b: Option<&Arc<RawElement>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

impl DiacraftApp {
    /// Shows the View settings `settings`: the four boxes of the View tab, and what the drawing area draws (hidden guides are not selected or dragged).
    pub(crate) fn show_window_settings(&mut self, settings: ViewSettings) {
        self.ui.show_grid = settings.grid;
        self.ui.show_rulers = settings.rulers;
        self.set_show_guides(settings.guides);
        self.ui.show_connection_points = settings.connection_points;
    }

    /// A box of the View tab turned (`view.grid`, `view.rulers`, `view.guides`, `view.connectionPoints`), or the guides shown by a drag from a ruler: the window shows it,
    /// and **the drawing holds it** (`window.settings.set`: no undo step and no new revision, and the drawing is unsaved, so a save keeps it and closing asks first). A drawing that cannot
    /// hold it (its file's windows part is one DiaCraft does not read) still shows it, and the status bar says why it is not kept. With no drawing open the window only shows it.
    pub(crate) fn turn_view_settings(&mut self, change: WindowFlags) {
        if let Some(on) = change.grid {
            self.ui.show_grid = on;
        }
        if let Some(on) = change.rulers {
            self.ui.show_rulers = on;
        }
        if let Some(on) = change.guides {
            self.set_show_guides(on);
        }
        if let Some(on) = change.connection_points {
            self.ui.show_connection_points = on;
        }
        if self.session().document().is_none() {
            return;
        }
        let params = json!({ "grid": change.grid, "rulers": change.rulers, "guides": change.guides, "connection_points": change.connection_points });
        match self.session.execute("window.settings.set", &params) {
            // What the drawing shows now, which is what was turned.
            Ok(_) => self.showed_drawings_window_settings(),
            Err(e) => self.set_notice(&format!("{e} (the box is turned in this window only)")),
        }
    }

    /// Shows what the drawing open now shows ([`window_settings_of`]), and notes its windows part; a new drawing's settings when no drawing is open.
    pub(crate) fn showed_drawings_window_settings(&mut self) {
        let settings = self.session().document().map_or(NEW_DRAWING, window_settings_of);
        self.show_window_settings(settings);
        self.ui.windows_seen = windows_part(self.session().document());
    }

    /// After a command: when it changed the drawing's windows part (by pointer: a command that did not write it leaves the very part the drawing held), the window shows
    /// what the drawing shows now: a program's `window.settings.set` does. An undo and a redo leave the part as it is, so they leave the boxes.
    pub(crate) fn followed_window_settings(&mut self) {
        let now = windows_part(self.session().document());
        if !same_part(now.as_ref(), self.ui.windows_seen.as_ref()) {
            self.showed_drawings_window_settings();
        }
    }
}

#[cfg(test)]
mod tests {
    use diacraft_engine::doc::{RawAttrs, RawNode, VsdxHeld};

    use super::*;

    fn part() -> Arc<RawElement> {
        Arc::new(RawElement { name: Arc::from("Windows"), attrs: RawAttrs::new(), children: Vec::new() })
    }

    #[test]
    fn parts_are_the_same_by_pointer() {
        let (a, b) = (part(), part());
        assert!(same_part(Some(&a), Some(&Arc::clone(&a))));
        assert!(!same_part(Some(&a), Some(&b)), "equal parts that are two allocations are two parts");
        assert!(same_part(None, None));
        assert!(!same_part(Some(&a), None) && !same_part(None, Some(&a)));
    }

    #[test]
    fn a_drawing_with_no_part_shows_a_new_drawings_settings() {
        assert_eq!(window_settings_of(&Document::new_blank()), NEW_DRAWING);
        assert_eq!(windows_part(Some(&Document::new_blank())), None);
        let mut doc = Document::new_blank();
        let p = part();
        doc.vsdx = Some(Arc::new(VsdxHeld { windows: Some(Arc::clone(&p)), ..VsdxHeld::default() }));
        assert!(same_part(windows_part(Some(&doc)).as_ref(), Some(&p)));
        assert_eq!(window_settings_of(&doc), NEW_DRAWING, "a part with no drawing window holds no settings");
        let window =
            RawElement { name: Arc::from("Window"), attrs: RawAttrs::from_pairs([("ID", "0"), ("WindowType", "Drawing")]), children: Vec::new() };
        let p = Arc::new(RawElement { children: vec![RawNode::Element(window)], ..RawElement::clone(&p) });
        doc.vsdx = Some(Arc::new(VsdxHeld { windows: Some(p), ..VsdxHeld::default() }));
        assert_eq!(window_settings_of(&doc), diacraft_engine::view::NO_VALUE, "a drawing window that holds no value");
    }
}
