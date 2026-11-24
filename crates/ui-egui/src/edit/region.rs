//! Selecting by region in the window: the net of the Pointer Tool and of Area Select, and the loop of Lasso Select (Home, Editing, Select).
//!
//! **The engine says what a region takes** (`select.by_region`, `docs/editing-commands.md`): the window sends the polygon (a net is its four corners, a loop the points the
//! pointer went through, simplified) and the option "Select shapes partially within area" (`UiState::select_partial`, File, Options, Advanced: R8, off at first), and selects the `ID`s that
//! come back, after the ones Shift or Ctrl kept. **Nothing is sent that changes the drawing**: a selection is the window's.
//!
//! **Area Select and Lasso Select are modes for the next drag** (`Tool::AreaSelect`, `Tool::Lasso`): a press anywhere, **on a shape too**, starts the net or the loop instead of a move, and one
//! drag uses the mode up, the Pointer Tool being back (assumed: the page says "the next drag"). A click that is no drag leaves the mode on, and Escape puts it down.
//!
//! **The loop** is the places the pointer went through, at least [`POINT_GAP`] screen points apart and at most [`MOST_POINTS`] of them, drawn as a thin dotted trail that is not closed
//! On the release it is simplified to within [`SIMPLIFY`] screen points and its last point joins the first. A loop of fewer than three points selects nothing.

use serde_json::{Value, json};

use super::geom::{self, P};
use super::gesture::Mode;
use super::{Cx, Tool, dedup, inch};
use crate::app::DiacraftApp;

/// The least a point of the loop is from the one before it, in screen points.
const POINT_GAP: f32 = 3.0;

/// The most points a loop keeps: what `select.by_region` takes.
pub const MOST_POINTS: usize = diacraft_engine::draw::MAX_SELECT_POINTS;

/// How far, in screen points, the simplified loop may stand from the one the pointer made.
const SIMPLIFY: f32 = 1.5;

impl DiacraftApp {
    /// The press of Area Select (`lasso` false) or of Lasso Select: the selection is dropped unless Shift or Ctrl is held, and the net or the loop begins here.
    pub(super) fn press_with_select(&mut self, additive: bool, lasso: bool, at: P) -> Mode {
        self.ui.edit.guide = None;
        let base = if additive { self.ui.edit.selection.clone() } else { Vec::new() };
        if !additive {
            self.ui.edit.selection.clear();
        }
        if lasso { Mode::Lasso { base, trail: vec![at] } } else { Mode::Marquee { additive, base } }
    }

    /// The pointer moved with the Lasso Select tool down: the places it went through since the last frame, and where it is now, join the trail.
    pub(super) fn lasso_follow(&mut self, cx: &Cx, now: P) {
        if !matches!(self.ui.edit.gesture.as_ref().map(|g| &g.mode), Some(Mode::Lasso { .. })) {
            return;
        }
        let moved = std::mem::take(&mut self.ui.edit.trail);
        let gap = cx.reach(POINT_GAP);
        let Some(Mode::Lasso { trail, .. }) = self.ui.edit.gesture.as_mut().map(|g| &mut g.mode) else { return };
        for at in moved {
            push_point(trail, cx.page_at(at), gap);
        }
        // The pointer ends the trail where it is: a place a hair from the last is the last place moved.
        let held = trail.len() > 1 && trail.last().is_some_and(|last| geom::distance(*last, now) < gap);
        match trail.last_mut() {
            Some(last) if held => *last = now,
            _ => push_point(trail, now, 0.0),
        }
    }

    /// The button came up on a loop: its shapes are selected.
    pub(super) fn finish_lasso(&mut self, cx: &Cx, base: Vec<u32>, trail: Vec<P>) {
        let polygon = geom::simplified(&trail, cx.reach(SIMPLIFY));
        if polygon.len() < 3 {
            self.set_selection(cx.page, base);
        } else {
            self.select_region(cx.page, &polygon, base);
        }
        self.use_up_select_tool();
    }

    /// One drag has been a net or a loop: the mode of Area Select or Lasso Select is used up.
    pub(super) fn use_up_select_tool(&mut self) {
        if matches!(self.ui.edit.tool, Tool::AreaSelect | Tool::Lasso) {
            self.ui.edit.tool = Tool::Pointer;
        }
    }

    /// Selects what the region takes (`select.by_region`), after `base`. A refusal reaches the person as a notice and leaves only `base` selected.
    pub(crate) fn select_region(&mut self, page: u32, polygon: &[P], base: Vec<u32>) {
        let points: Vec<Value> = polygon.iter().map(|p| json!([inch(p.0), inch(p.1)])).collect();
        let params = json!({"polygon": points});
        if let Err(e) = self.select_in_region(page, params, base.clone()) {
            self.set_notice(&e.to_string());
            self.set_selection(page, base);
        }
    }

    /// Runs `select.by_region` with `params` (`polygon` or `rect`, and `partial`, which is the window's option when it is left out) for `page` and selects the shapes it answers, after
    /// `base`. The window's commands and the pointer both come here.
    pub(crate) fn select_in_region(&mut self, page: u32, mut params: Value, base: Vec<u32>) -> diacraft_engine::Result<()> {
        if let Some(map) = params.as_object_mut() {
            map.insert("page".to_owned(), json!(page));
            if map.get("partial").is_none_or(Value::is_null) {
                map.insert("partial".to_owned(), json!(self.ui.select_partial));
            }
        }
        let answer = self.execute("select.by_region", &params)?;
        let found: Vec<u32> = answer
            .get("ids")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok())).collect())
            .unwrap_or_default();
        if let Some(unread) = answer.get("unread").and_then(Value::as_u64) {
            self.set_notice(&format!("This page has more shapes than a selection reads; {unread} were left out. The commands still reach them."));
        }
        let mut chosen = base;
        chosen.extend(found);
        self.set_selection(page, dedup(chosen));
        Ok(())
    }
}

/// Adds `p` to the trail when it is at least `gap` from the last point and the trail has room.
fn push_point(trail: &mut Vec<P>, p: P, gap: f64) {
    if trail.len() < MOST_POINTS && trail.last().is_none_or(|last| geom::distance(*last, p) >= gap) {
        trail.push(p);
    }
}
