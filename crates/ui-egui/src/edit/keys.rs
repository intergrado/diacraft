//! The keyboard on the drawing: Escape, Delete, the arrow keys, F2 and typing on a selected shape.
//!
//! These keys act on the selection whatever the pointer is over, so that a shape clicked can be deleted with the pointer on the ribbon. They
//! stand aside while a text box or any other text field has the keyboard, and while a menu is open or the card of a folded ribbon group is
//! (`DiacraftApp::menu_open`): Escape closes the menu or the card first, and the arrows, F2 and typing do nothing to the shapes meanwhile.

use egui::{Context, Event, Key, Modifiers};
use serde_json::json;

use super::{Cx, Tool, inch};
use crate::app::DiacraftApp;

/// The arrow keys and the way each moves a shape: (key, x, y), y up.
const ARROWS: [(Key, f64, f64); 4] =
    [(Key::ArrowLeft, -1.0, 0.0), (Key::ArrowRight, 1.0, 0.0), (Key::ArrowUp, 0.0, 1.0), (Key::ArrowDown, 0.0, -1.0)];

impl DiacraftApp {
    /// The stable identity of a gallery drag, without exposing the edit state to panels.
    pub(crate) fn tile_drag_key(&self) -> Option<String> {
        self.ui.edit.tile.as_ref().map(|drag| drag.master.key())
    }

    pub(crate) fn cancel_hidden_tile_drag(&mut self) {
        let visible = self.ui.edit.tile.as_ref().is_none_or(|drag| {
            self.ui.stencils.list.as_ref().and_then(|list| list.iter().find(|s| Some(&s.id) == self.ui.stencils.filter_stencil.as_ref())).is_some_and(
                |stencil| {
                    // The search results are not narrowed by name: the words in the box are what was searched for.
                    let query = if stencil.kind == crate::stencils::Kind::Search { String::new() } else { self.ui.stencils.filter.to_lowercase() };
                    stencil.masters.iter().any(|m| m.key() == drag.master.key() && m.name.to_lowercase().contains(&query))
                },
            )
        });
        if !visible {
            self.ui.edit.tile = None;
            self.ui.edit.tile_landing = None;
            self.ui.edit.tile_ghost = None;
            self.ui.edit.tile_shape = None;
        }
    }

    /// The drawing area's `Cx` as the last frame drew it, for a page: where things are without a frame to draw.
    pub(crate) fn cx_of(&self, page: u32) -> Option<Cx> {
        let area = self.ui.canvas_rect?;
        let view = *self.ui.views.get(&page)?;
        let settings = crate::canvas::page_drawing::page_settings(self.session.document()?, page)?;
        Some(Cx { area, view, page, settings })
    }

    /// Whether the keys below should leave the keyboard to something else.
    fn keyboard_taken(&self, ctx: &Context) -> bool {
        // A field of a pane that has the focus, or has just lost it and is not yet finished: egui takes the focus from a box on an Escape as the frame begins, and
        // the Escape is the box's (it leaves the old value), not the drawing's (which would clear the selection).
        self.ui.backstage.is_some()
            || self.ui.edit.text.is_some()
            || self.is_modal_open()
            || ctx.egui_wants_keyboard_input()
            || self.pane_keys_held()
            || self.comment_keys_held(ctx)
            || self.shape_filter_keys_held()
            || self.sheet_keys_held()
    }

    /// Runs the keys that act on the drawing, once per frame.
    pub(crate) fn run_edit_keys(&mut self, ctx: &Context) {
        if self.keyboard_taken(ctx) {
            return;
        }
        let Some(page) = self.current_page() else { return };
        // With a menu or a card open every key is the menu's or the card's: Escape closes it, and the arrows, F2 and typing leave the shapes alone.
        if self.menu_open(ctx) {
            return;
        }
        if self.wants_escape() && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.escape();
        }
        if self.ui.edit.selection.is_empty() {
            return;
        }
        self.ensure_scene(page);
        // AutoConnect's keys: Alt with an arrow key clicks that arrow of the shape selected, Alt+Shift opens its flyout (`autoconnect.rs`).
        if self.auto_keys(ctx, page) {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F2)) {
            self.edit_selected_text(None);
        }
        for (key, x, y) in ARROWS {
            // Shift first: a key pressed with Shift also matches the plain key.
            if ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, key)) {
                self.nudge(page, (x, y), true);
            } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key)) {
                self.nudge(page, (x, y), false);
            }
        }
        if let Some(typed) = typed_text(ctx) {
            self.edit_selected_text(Some(typed));
        }
    }

    /// Whether Escape has anything to do: a drag to drop, the Connector tool to leave, a selection to clear.
    fn wants_escape(&self) -> bool {
        let edit = &self.ui.edit;
        edit.gesture.is_some()
            || edit.auto.has_flyout()
            || edit.tool != Tool::Pointer
            || edit.painter.is_some()
            || !edit.selection.is_empty()
            || edit.tile.is_some()
            || edit.ruler.is_some()
            || edit.guide.is_some()
    }

    /// Escape: it undoes the innermost thing that is on. A drag (a guide out of a ruler too), then the Format Painter, then a tool that is not the Pointer Tool,
    /// then the selection (a guide's too).
    pub(crate) fn escape(&mut self) {
        // AutoConnect's flyout is the innermost thing that can be open: it goes first.
        if self.ui.edit.auto.close_flyout() {
            return;
        }
        if self.ui.edit.tile.take().is_some() || self.cancel_gesture() || self.ui.edit.ruler.take().is_some() {
            return;
        }
        // The painter is put down before the click that would use it ("Esc cancels before the click"), and the tool under it stays.
        if self.ui.edit.painter.take().is_some() {
            return;
        }
        if self.ui.edit.tool != Tool::Pointer {
            self.set_tool(Tool::Pointer);
        } else {
            self.ui.edit.selection.clear();
            self.ui.edit.guide = None;
        }
    }

    /// Moves the selection one press of an arrow key: **to the next place it can snap to in that direction, however far, and with
    /// none one ruler tick** as the ruler draws it at the zoom: `shape.move` with `snap: {zoom, view, nudge: {tick}}`, `dx` and `dy` giving the direction. The engine finds the
    /// place, so what a press does is what `snap.at` says of it. **Shift with an arrow key is a plain move by one screen point**, with no snap and no stops, as before.
    ///
    /// **The page is made ready for the press**: the reader of pages makes the index of the shapes and the targets, off this thread, on the page the session keeps
    /// for the revision, which the command finds, so that a press on a ready page builds none of them here. The press that first meets the page asks for it and goes on (the command
    /// shares the reader's work, or does it, as before); **after a press the page is asked for again** for the revision the press made, so that the next press, if it comes
    /// after the reader is done, finds it ready.
    fn nudge(&mut self, page: u32, (x, y): (f64, f64), fine: bool) {
        // Shift with an arrow key on a connector whose ends are both glued moves a leg of its route, a screen point (`legs.rs`).
        if fine && self.nudge_leg(page, (x, y)) {
            return;
        }
        let Some(cx) = self.cx_of(page) else { return };
        let ids = self.selection_that_moves();
        if ids.is_empty() {
            return;
        }
        let params = if fine {
            let one_point = cx.reach(1.0);
            json!({"ids": ids, "dx": inch(x * one_point), "dy": inch(y * one_point), "page": page})
        } else {
            // The ruler along the axis the key moves on; a press never moves less than a millionth of an inch, the least the engine takes.
            let tick = crate::canvas::grid::ruler_tick(&cx.settings, cx.view.scale(), x != 0.0).unwrap_or(0.125).max(1.0e-6);
            let mut snap = Self::snap_param(&cx);
            if let Some(words) = snap.as_object_mut() {
                words.insert("nudge".to_owned(), json!({"tick": inch(tick)}));
            }
            json!({"ids": ids, "dx": inch(x * tick), "dy": inch(y * tick), "page": page, "snap": snap})
        };
        let snapped = params.get("snap").is_some();
        if snapped {
            self.make_ready_for_move(page, &ids);
        }
        if let Some(answer) = self.run_edit("shape.move", &params) {
            self.noted_snap(&answer);
            self.keep_preview(page, &ids);
        }
        // Where the reader has a thread the next press can find the page ready; inline, the work would be done here for a press that may never come.
        if snapped && !self.ui.edit.reader.is_inline() {
            self.make_ready_for_move(page, &ids);
        }
    }

    /// The selected shapes that go when moved: not a connector whose ends are both glued.
    fn selection_that_moves(&self) -> Vec<u32> {
        let Some(scene) = &self.ui.edit.scene else { return Vec::new() };
        self.ui.edit.selection.iter().copied().filter(|id| scene.get(*id).is_some_and(super::scene::Item::moves_by_itself)).collect()
    }

    /// Delete: removes the selected shapes and the connectors glued to them (it removes the selection
    /// along with connectors attached to it). The engine's `shape.delete` takes those connectors itself, except one on a locked
    /// layer or on hidden ones only, which the pointer cannot pick and which stays with a free end.
    pub(crate) fn delete_selection(&mut self) {
        let Some(page) = self.current_page() else { return };
        // A guide selected is deleted as a guide (`guide.delete`), which refuses one that shapes are glued to.
        if let Some(guide) = self.ui.edit.guide {
            self.delete_guide(page, guide);
            return;
        }
        let ids = self.ui.edit.selection.clone();
        if ids.is_empty() {
            return;
        }
        let params = json!({"ids": ids, "page": page});
        if self.run_edit("shape.delete", &params).is_some() {
            self.ui.edit.selection.clear();
            self.ui.edit.gesture = None;
            self.ui.edit.after = None;
        }
    }

    /// Selects every shape of the page (Select All).
    pub(crate) fn select_all(&mut self) {
        let Some(page) = self.current_page() else { return };
        self.ensure_scene(page);
        let all: Vec<u32> = self.ui.edit.scene.as_ref().map(|s| s.selectable_ids()).unwrap_or_default();
        self.set_selection(page, all);
    }
}

/// The text typed this frame that should start editing a shape's text: printable characters that a person typed ([`crate::shortcuts::types_text`]): with no Ctrl or Alt held, **with AltGr** (€, @, ł, ś:
/// Ctrl and Alt together off a Mac) and, on a Mac, with Option (å); not the text a Ctrl or Command chord leaves behind.
fn typed_text(ctx: &Context) -> Option<String> {
    // Before the input is borrowed: `Context::os` reads the context, which `input` holds.
    let os = ctx.os();
    ctx.input(|i| {
        if !crate::shortcuts::types_text(os, &i.modifiers) {
            return None;
        }
        i.events.iter().find_map(|e| match e {
            Event::Text(text) if !text.is_empty() && !text.chars().any(char::is_control) => Some(text.clone()),
            _ => None,
        })
    })
}
