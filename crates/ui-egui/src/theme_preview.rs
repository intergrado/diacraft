//! The live preview of a theme or a variant ("pointing at a tile previews it on the page").
//!
//! **A preview is not an edit.** While a tile of the Themes gallery, a tile of the Variants strip or a row of its Colors menu is under the pointer, the page shows what a press on it would
//! make, and **when the pointer goes the page is as it was**. The engine makes the preview as **a copy of the drawing** with the theme applied (`Session::preview_theme`: the code `theme.apply`
//! and `theme.variant` run, on a copy, under a budget of shapes and steps, `theme/preview.rs`), so nothing in the session changes: no undo step, no new revision, no unsaved mark, and the redo
//! stack is kept (a theme applied for real would clear it). The window draws the copy on a render worker of its own, in place of the page's picture, at the view the page has.
//!
//! **How a frame goes.** Each tile under the pointer *asks* while the ribbon is drawn ([`DiacraftApp::ask_theme_preview`]); the drawing area, drawn after it, takes the ask: the copy is made
//! when it is not the one it already holds (the same drawing, revision, page and tile), and its picture is asked of the worker. **Moving to another tile asks for another picture and stops the
//! one being drawn** (the renderer reads its cancel between shapes: `render_worker.rs`), and the picture of the tile before stays on the page until the next one arrives, so that the page does not
//! flicker between them. **A frame with no ask ends the preview**: the copy and its pictures are let go, and the page's own picture, which was never touched, is what shows.
//!
//! **A page past the budget** (`MAX_PREVIEW_SHAPES`) has no preview: the engine refuses in words, which the status bar says once for the tile, and the tile still applies the theme.

use std::sync::Arc;

use diacraft_engine::doc::Document;
use diacraft_engine::theme::PreviewRequest;
use egui::Context;
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::canvas::picture::{Pictures, Want};

/// What a preview is of: the drawing's generation and revision, the page, and the tile.
type Key = (u64, u64, u32, PreviewRequest);

/// The copy of the drawing a preview shows, and the version of it the pictures are keyed by.
struct Built {
    key: Key,
    doc: Arc<Document>,
    version: u64,
}

/// What the window holds of a preview.
pub struct ThemePreview {
    /// What a tile under the pointer asked for in this frame: set while the ribbon is drawn and taken by the drawing area.
    asked: Option<PreviewRequest>,
    built: Option<Built>,
    /// What was refused and why: not asked of the engine again, and not said again, while the same tile is under the pointer.
    refused: Option<(Key, String)>,
    /// The pictures of the copy, drawn on a worker of their own.
    pub(crate) pictures: Pictures,
    /// Counts the copies made: the version a picture is of (a picture of an older version never replaces a newer one).
    counter: u64,
    /// How many copies have been made and how many pictures of them were asked for, since the window began: for tests and `ui.inspect`.
    pub(crate) made: u64,
}

impl ThemePreview {
    pub fn new() -> ThemePreview {
        ThemePreview { asked: None, built: None, refused: None, pictures: Pictures::new(), counter: 0, made: 0 }
    }

    /// Lets go of the copy and its pictures: nothing is previewed.
    fn end(&mut self) {
        if self.built.take().is_some() {
            self.pictures.reset();
        }
        self.refused = None;
    }

    /// The tile the page shows, if a preview is on.
    pub fn showing(&self) -> Option<&PreviewRequest> {
        self.built.as_ref().map(|b| &b.key.3)
    }
}

impl Default for ThemePreview {
    fn default() -> ThemePreview {
        ThemePreview::new()
    }
}

impl DiacraftApp {
    /// A tile under the pointer asks for its theme to be shown on the page, in this frame.
    pub(crate) fn ask_theme_preview(&mut self, request: PreviewRequest) {
        self.ui.theme_preview.asked = Some(request);
    }

    /// Makes the preview of the tile asked for in this frame and draws it. → whether a picture of the preview is on the screen, in place of the page's own: the caller paints that.
    pub(crate) fn update_theme_preview(&mut self, ctx: &Context, want: &Want) -> bool {
        let Some(request) = self.ui.theme_preview.asked.take() else {
            self.ui.theme_preview.end();
            return false;
        };
        let key: Key = (self.ui.generation, self.session.revision(), want.page, request.clone());
        let held = self.ui.theme_preview.built.as_ref().is_some_and(|b| b.key == key);
        if !held {
            if self.ui.theme_preview.refused.as_ref().is_some_and(|(k, _)| *k == key) {
                return false;
            }
            match self.session.preview_theme(want.page, &request) {
                Ok(made) => {
                    let pv = &mut self.ui.theme_preview;
                    pv.counter = pv.counter.saturating_add(1);
                    pv.made = pv.made.saturating_add(1);
                    pv.refused = None;
                    pv.built = Some(Built { key, doc: Arc::new(made.document), version: pv.counter });
                }
                Err(e) => {
                    let words = e.to_string();
                    self.ui.theme_preview.built = None;
                    self.ui.theme_preview.refused = Some((key, words.clone()));
                    self.set_notice(&words);
                    return false;
                }
            }
        }
        let now = self.clock.now();
        let pv = &mut self.ui.theme_preview;
        let Some(built) = pv.built.as_ref() else { return false };
        let (doc, version) = (Arc::clone(&built.doc), built.version);
        let failure = pv.pictures.update_doc(ctx, &doc, version, want, now);
        if let Some(failure) = failure {
            // A preview that could not be drawn is no reason to stop the window: the page's own picture shows.
            self.set_notice(&format!("the preview could not be drawn: {failure}"));
        }
        self.ui.theme_preview.pictures.image(want.page).is_some()
    }

    /// What the window knows of the preview, for the window state: the tile shown, how many copies were made, and what was refused.
    pub(crate) fn theme_preview_state(&self) -> Value {
        let pv = &self.ui.theme_preview;
        json!({
            "showing": pv.showing().map(|r| format!("{r:?}")),
            "made": pv.made,
            "refused": pv.refused.as_ref().map(|(_, words)| words),
            "pictureShown": pv.built.is_some() && self.ui.page.is_some_and(|p| pv.pictures.image(p).is_some()),
        })
    }
}
