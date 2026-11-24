//! The yellow handle of a callout, and the drag that ties the callout to a shape.
//!
//! *"A callout shows a yellow handle at the end of its leader line; drag it onto a shape to associate it, then move the text where wanted."* A callout the engine made (`callout.list` says its `style` and
//! where its `tip` is) that is **the one shape selected**, with the Pointer Tool on and no text typed, shows a yellow diamond at the end of its leader. A press on it starts a drag of the end; the
//! release sends one command:
//!
//! * on a shape (the connection point or the shape the pointer is within reach of, as a connector's end is: [`DiacraftApp::target_near`], the callout itself never): `callout.attach`, which ties the
//!   callout to it, so that it moves with the shape and the end of the leader sits at the shape's edge;
//! * on bare page: `callout.detach` with the place, which ties the callout to nothing and puts the end where it was let go.
//!
//! A drag that ends where it began sends nothing. The drag is drawn as the line from the callout to the pointer, green over a shape it would tie to, with the handle at the pointer; Escape drops it.
//! The handle's colour is the theme's token `callout_handle`.

use diacraft_engine::draw::callouts;
use egui::{Painter, Pos2, Shape, Stroke, vec2};

use super::geom::{self, P};
use super::gesture::{Gesture, Mode, Target};
use super::scene::Kind;
use super::{Cx, HANDLE_REACH, Tool, inch};
use crate::app::DiacraftApp;
use crate::theme::Tokens;

/// Half the diagonal of the handle's diamond, in points.
const HANDLE_HALF: f32 = 6.5;

/// What the window reads of the callout selected, for one revision of the drawing: where the end of its leader is, in paper inches.
#[derive(Clone, Copy, Debug)]
pub(super) struct Tip {
    pub id: u32,
    pub at: P,
}

impl DiacraftApp {
    /// Once a frame, with the drawing area: reads the end of the leader of the callout selected (when it is the one shape selected, the Pointer Tool is on and no text is being typed), once for each
    /// revision, and tells the frame's elements where the handle is.
    pub(crate) fn tip_update(&mut self, cx: &Cx) {
        let id = match self.single() {
            Some(item) if item.kind == Kind::Shape && self.ui.edit.tool == Tool::Pointer && self.ui.edit.text.is_none() => Some(item.id),
            _ => None,
        };
        let Some(id) = id.filter(|_| self.ui.edit.scene.as_ref().is_some_and(|s| s.page == cx.page)) else {
            self.ui.edit.tip = None;
            self.ui.edit.tip_asked = None;
            return;
        };
        let key = (self.ui.generation, self.session.revision(), cx.page, id);
        // Asked once for each revision, page and shape: the engine finds the shape in a walk of the page, and a shape that is no callout answers nothing every time.
        if self.ui.edit.tip_asked != Some(key) {
            self.ui.edit.tip_asked = Some(key);
            let per_inch = self.ui.edit.scene.as_ref().map_or(1.0, |s| s.per_inch);
            self.ui.edit.tip = self
                .session
                .document()
                .and_then(|doc| callouts::info(doc, cx.page, id).ok())
                .and_then(|info| info.tip)
                .filter(|(x, y)| x.is_finite() && y.is_finite() && per_inch > 0.0)
                .map(|(x, y)| Tip { id, at: (x / per_inch, y / per_inch) });
        }
        if let Some(tip) = self.ui.edit.tip {
            let rect = egui::Rect::from_center_size(cx.screen_at(tip.at), vec2(2.0 * HANDLE_REACH, 2.0 * HANDLE_REACH));
            self.note("callout.tip", rect, true);
        }
    }

    /// The screen point of the yellow handle of callout `id` as the last frame drew it: where a client takes hold. `None` for a shape that is not the callout selected.
    pub fn callout_tip_point(&self, id: u32) -> Option<Pos2> {
        let cx = self.cx_of(self.ui.page?)?;
        let tip = self.ui.edit.tip.filter(|t| t.id == id)?;
        Some(cx.screen_at(tip.at))
    }

    /// A press on the handle of callout `id` at the page point `p`: the drag of its end.
    pub(super) fn press_on_tip(&self, cx: &Cx, id: u32, p: P) -> Option<Mode> {
        let tip = self.ui.edit.tip.filter(|t| t.id == id)?;
        (geom::distance(tip.at, p) <= cx.reach(HANDLE_REACH)).then_some(Mode::Tip { id })
    }

    /// Paints the yellow handle of the callout selected.
    pub(super) fn paint_tip_handle(&self, painter: &Painter, t: &Tokens, cx: &Cx) {
        if let Some(tip) = self.ui.edit.tip {
            paint_diamond(painter, t, cx.screen_at(tip.at));
        }
    }

    /// Paints the drag of the end: the line from the callout to where the pointer is (green over a shape it would tie to), and the handle at the pointer.
    pub(super) fn paint_tip_gesture(&self, painter: &Painter, t: &Tokens, cx: &Cx, gesture: &Gesture) {
        let Mode::Tip { id } = &gesture.mode else { return };
        let Some(scene) = self.ui.edit.scene.as_ref() else { return };
        let from = scene.get(*id).map_or(gesture.origin, |item| item.centre());
        let over_shape = matches!(gesture.target, Target::Shape(_) | Target::Point { .. });
        let colour = if over_shape { t.glue_on } else { t.edit_line };
        let (a, b) = (cx.screen_at(from), cx.screen_at(gesture.now));
        painter.extend(Shape::dashed_line(&[a, b], Stroke::new(1.75, colour), super::paint::DASH, super::paint::GAP));
        paint_diamond(painter, t, b);
    }

    /// The button came up on the handle of callout `id`: `callout.attach` when the pointer is on a shape, else `callout.detach` with the place; nothing for a drag that came back to where it began.
    pub(super) fn finish_tip(&mut self, cx: &Cx, gesture: &Gesture, id: u32) {
        let moved = geom::distance(gesture.origin, gesture.now) > super::gesture::NO_CHANGE;
        if !moved {
            return;
        }
        let params = match gesture.target {
            Target::Shape(target) | Target::Point { id: target, .. } => serde_json::json!({"id": id, "target": target, "page": cx.page}),
            Target::Free(_) => serde_json::json!({"id": id, "x": inch(gesture.now.0), "y": inch(gesture.now.1), "page": cx.page}),
        };
        let command = if params.get("target").is_some() { "callout.attach" } else { "callout.detach" };
        if self.run_edit(command, &params).is_some() {
            self.keep_preview(cx.page, &[id]);
        }
    }
}

/// The handle: a diamond the token's yellow, with a ring of the handle's white round it.
fn paint_diamond(painter: &Painter, t: &Tokens, at: Pos2) {
    if !(at.x.is_finite() && at.y.is_finite()) {
        return;
    }
    let corners = |half: f32| vec![at + vec2(0.0, -half), at + vec2(half, 0.0), at + vec2(0.0, half), at + vec2(-half, 0.0)];
    painter.add(Shape::convex_polygon(corners(HANDLE_HALF + 1.5), t.handle_fill, Stroke::NONE));
    painter.add(Shape::convex_polygon(corners(HANDLE_HALF), t.callout_handle, Stroke::new(1.0, t.edit_line)));
}
