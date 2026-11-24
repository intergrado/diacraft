//! The legs of the connector selected: the handle at the middle of each leg of a right-angle route, and the drag that moves a leg.
//!
//! "Edit a connector path by hand": *dragging a midpoint to a new place reroutes it, keeping that path until reset*. The selection look's R30
//! draws a connector's bend or middle handle as a filled blue disc, which is what a handle here is ([`select::BEND`](crate::theme::select::BEND), the token
//! `bend_handle`). **A handle is at the middle of each leg of a route of right angles** that the engine can edit (`draw::connector_path`: a connector that is not in a
//! group, with level and upright legs only); a leg shorter than [`select::BEND_LEG`](crate::theme::select::BEND_LEG) points on the screen has none, so that the handle
//! never touches the marks at the ends. **A straight or a curved connector has none**: no specification says what a handle on one does (the report's Q1), and a handle
//! that did nothing would only mislead.
//!
//! **The drag** is a leg's, in the one direction across it (a level leg goes up and down, an upright one left and right): the leg's line is where it was when the press
//! was made plus how far the pointer has gone since. **Where it lands is the engine's snap** (`snapping.rs`, `snap_leg`): the middle of the leg is the place the snap pulls,
//! along the one axis the leg moves on, and the release is one `connector.edit_path` sent with `snap` so that the engine lands it the way the preview did (the engine's
//! `draw::leg_subject` makes the place for both). The preview is the route the engine will write (`draw::moved_route`), drawn dashed over the picture.
//! Escape drops the drag, Alt turns the snap off for it, and a drag that comes back to where it began sends nothing.
//!
//! **The keys** (the same row says "Shift with arrow keys moves it a pixel at a time"): with one connector selected whose ends are both glued (a nudge of the shape
//! would move nothing), Shift with an arrow key moves **the leg that key moves across** (Left and Right an upright leg, Up and Down a level one) **by one screen point**, with no
//! snap: one `connector.edit_path`, one undo step. **Assumed** (the specifications give no leg): of the legs with a handle that the key moves, the one nearest the middle of the
//! route. A connector with a free end is nudged as a shape, as before (`Legs::nudge_target`, `DiacraftApp::nudge_leg`).

use diacraft_engine::draw::{connector_path, connector_reroute, leg_subject, moved_route};
use egui::{Painter, Pos2, Rect, Shape, Stroke, vec2};
use serde_json::{Value, json};

use super::geom::{self, P};
use super::gesture::{Gesture, Landing, Mode, NO_CHANGE};
use super::scene::Kind;
use super::{Cx, HANDLE_REACH, Tool, inch};
use crate::app::DiacraftApp;
use crate::theme::{Tokens, select};

/// The most handles drawn and offered at once: a file's route can have thousands of legs, and a person takes hold of one of the first few that fit.
const MAX_HANDLES: usize = 256;

/// What the window reads of the route of the one connector selected, for one revision of the drawing.
pub(super) struct Legs {
    /// The drawing's generation, the revision, the page and the connector it was read for.
    key: (u64, u64, u32, u32),
    /// The corners of the route on the page in the page's drawing units, which the engine's functions take, begin first.
    units: Vec<P>,
    /// Drawing units to a paper inch.
    per_inch: f64,
}

/// A handle: which leg it is on, where it is on the page (paper inches) and whether the leg is level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LegHandle {
    pub leg: usize,
    pub at: P,
    pub level: bool,
}

/// How a leg lands while it is dragged: the line the pointer gives (as it is sent) and the line it comes to, snapped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LegLanding {
    pub raw: f64,
    pub to: f64,
}

impl Legs {
    pub(super) fn connector(&self) -> u32 {
        self.key.3
    }

    /// The corners of the route in paper inches.
    fn points(&self) -> Vec<P> {
        self.units.iter().map(|p| (p.0 / self.per_inch, p.1 / self.per_inch)).collect()
    }

    /// The handles that fit at `scale` screen points to an inch: one at the middle of each leg that is long enough on the screen.
    pub(super) fn handles(&self, scale: f64) -> Vec<LegHandle> {
        let mut out = Vec::new();
        for (leg, w) in self.units.windows(2).enumerate().take(MAX_HANDLES) {
            let (a, b) = (w[0], w[1]);
            let at = ((a.0 + b.0) / 2.0 / self.per_inch, (a.1 + b.1) / 2.0 / self.per_inch);
            let long = geom::distance((a.0 / self.per_inch, a.1 / self.per_inch), (b.0 / self.per_inch, b.1 / self.per_inch)) * scale;
            if !(long >= f64::from(select::BEND_LEG) - LEG_SLACK && at.0.is_finite() && at.1.is_finite()) {
                continue;
            }
            out.push(LegHandle { leg, at, level: (a.1 - b.1).abs() < LEVEL });
        }
        out
    }

    /// The handle of the leg a Shift-arrow key moves: of the handles at `scale` whose leg is level (`level`) or upright, the one nearest the middle of the route (the middle of its
    /// first and last corner), the first of two the same distance away. `None` when no leg of that kind has a handle.
    pub(super) fn nudge_target(&self, scale: f64, level: bool) -> Option<LegHandle> {
        let (first, last) = (self.units.first()?, self.units.last()?);
        let middle = ((first.0 + last.0) / 2.0 / self.per_inch, (first.1 + last.1) / 2.0 / self.per_inch);
        self.handles(scale)
            .into_iter()
            .filter(|h| h.level == level)
            .map(|h| (geom::distance(h.at, middle), h))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, h)| h)
    }

    /// Where the leg `leg` lies now: its height for a level leg, its place along x for an upright one, in paper inches.
    fn line_of(&self, leg: usize) -> Option<f64> {
        let (a, b) = (self.units.get(leg)?, self.units.get(leg.checked_add(1)?)?);
        Some(if (a.1 - b.1).abs() < LEVEL { a.1 } else { a.0 } / self.per_inch)
    }
}

/// How near to level two heights are for a leg to be level: the engine's own (`draw/hand.rs`).
const LEVEL: f64 = 1.0e-9;

/// How far under [`select::BEND_LEG`] a leg may come on the screen and still have its handle, in points: a leg a quarter inch long (the standoff of a route) is 24 points at 100%
/// and a hair less by the rounding of the rows it is written in, and it should not lose its handle to that.
const LEG_SLACK: f64 = 0.5;

impl DiacraftApp {
    /// Once a frame, with the drawing area: reads the route of the connector selected (when it is the one shape selected, the Pointer Tool is on and no text is being
    /// typed), once for each revision, and tells the frame's elements where the handles are.
    pub(crate) fn legs_update(&mut self, cx: &Cx) {
        let id = match self.single() {
            Some(item) if item.kind == Kind::Connector && self.ui.edit.tool == Tool::Pointer && self.ui.edit.text.is_none() => Some(item.id),
            _ => None,
        };
        let Some(id) = id.filter(|_| self.ui.edit.scene.as_ref().is_some_and(|s| s.page == cx.page)) else {
            self.ui.edit.legs = None;
            return;
        };
        let key = (self.ui.generation, self.session.revision(), cx.page, id);
        if self.ui.edit.legs.as_ref().is_none_or(|l| l.key != key) {
            let per_inch = self.ui.edit.scene.as_ref().map_or(1.0, |s| s.per_inch);
            self.ui.edit.legs = self
                .session
                .document()
                .and_then(|doc| connector_path(doc, cx.page, id))
                .filter(|path| path.editable)
                .map(|path| Legs { key, units: path.points, per_inch });
        }
        let scale = cx.view.scale();
        let hits: Vec<(usize, Rect)> = match &self.ui.edit.legs {
            Some(legs) => legs
                .handles(scale)
                .into_iter()
                .map(|h| (h.leg, Rect::from_center_size(cx.screen_at(h.at), vec2(2.0 * HANDLE_REACH, 2.0 * HANDLE_REACH))))
                .collect(),
            None => Vec::new(),
        };
        for (leg, rect) in hits {
            self.note(format!("connector.leg.{leg}"), rect, true);
        }
    }

    /// Shift with an arrow key `(x, y)` (y up) on the one connector selected: moves the leg the key moves across by one screen point, in one `connector.edit_path` with no snap. -> whether
    /// it was taken: not when the selection is not one connector with handles, when it moves by itself (an end is free: it is nudged as a shape), or when no leg has a handle the key
    /// moves.
    pub(crate) fn nudge_leg(&mut self, page: u32, (x, y): (f64, f64)) -> bool {
        let Some(item) = self.single() else { return false };
        if item.kind != Kind::Connector || item.moves_by_itself() {
            return false;
        }
        let id = item.id;
        let Some(cx) = self.cx_of(page) else { return false };
        let Some(legs) = self.ui.edit.legs.as_ref().filter(|l| l.connector() == id) else { return false };
        // A level leg goes up and down, an upright one left and right.
        let level = y != 0.0;
        let Some(handle) = legs.nudge_target(cx.view.scale(), level) else { return false };
        let Some(line) = legs.line_of(handle.leg) else { return false };
        let to = line + if level { y } else { x } * cx.reach(1.0);
        if !to.is_finite() {
            return false;
        }
        let params = json!({"id": id, "leg": handle.leg, "to": inch(to), "page": page});
        if self.run_edit("connector.edit_path", &params).is_some() {
            self.keep_preview(page, &[id]);
        }
        true
    }

    /// The screen point of the handle on leg `leg` of connector `id` as the last frame drew it: where a client takes hold. `None` for a connector with no handles, a leg that has
    /// none and a connector that is not the one selected.
    pub fn connector_leg_point(&self, id: u32, leg: usize) -> Option<Pos2> {
        let cx = self.cx_of(self.ui.page?)?;
        let legs = self.ui.edit.legs.as_ref().filter(|l| l.connector() == id)?;
        let handle = legs.handles(cx.view.scale()).into_iter().find(|h| h.leg == leg)?;
        Some(cx.screen_at(handle.at))
    }

    /// The handle at the page point `p`, the nearest within reach, if the connector `id` selected has handles.
    fn leg_at(&self, cx: &Cx, id: u32, p: P) -> Option<LegHandle> {
        let legs = self.ui.edit.legs.as_ref().filter(|l| l.connector() == id)?;
        let reach = cx.reach(HANDLE_REACH);
        legs.handles(cx.view.scale())
            .into_iter()
            .map(|h| (geom::distance(h.at, p), h))
            .filter(|(d, _)| *d <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, h)| h)
    }

    /// A press at `p` on a handle of the connector `id` selected: the drag that moves its leg.
    pub(super) fn press_on_leg(&self, cx: &Cx, id: u32, p: P) -> Option<Mode> {
        let handle = self.leg_at(cx, id, p)?;
        let legs = self.ui.edit.legs.as_ref()?;
        Some(Mode::Leg { id, leg: handle.leg, level: handle.level, line: legs.line_of(handle.leg)? })
    }

    /// The cursor over a handle of the connector selected: the two-headed arrow across the leg, the way it goes.
    pub(super) fn leg_cursor(&self, cx: &Cx, id: u32, p: P) -> Option<egui::CursorIcon> {
        let handle = self.leg_at(cx, id, p)?;
        Some(if handle.level { egui::CursorIcon::ResizeVertical } else { egui::CursorIcon::ResizeHorizontal })
    }

    /// A leg's landing (`Mode::Leg`): the line the pointer gives, and where the engine's snap puts it (`snap_leg`).
    pub(super) fn land_leg(&mut self, cx: &Cx, g: &Gesture, (id, leg, level, line): (u32, usize, bool, f64), out: &mut Landing) {
        let along = if level { g.now.1 - g.origin.1 } else { g.now.0 - g.origin.0 };
        let raw = super::snapping::sent(line + along);
        let mut to = raw;
        if out.snap {
            let subject = self.ui.edit.legs.as_ref().filter(|l| l.connector() == id).and_then(|l| leg_subject(&l.units, l.per_inch, leg, raw));
            match subject.and_then(|subject| self.snap_leg(cx.page, id, subject, cx.view.scale(), super::snapping::view_of(cx))) {
                Some((snapped, marks)) => {
                    to = snapped;
                    out.marks = marks;
                }
                None => out.snap = false,
            }
        }
        out.leg = Some(LegLanding { raw, to });
    }

    /// The button came up on a handle: leg `leg` of connector `id` goes to the line the drag landed on, in one `connector.edit_path`. With `snap` the line the pointer gave
    /// is sent and the engine snaps it as the preview did; without (Alt, or a page that was not ready) the line the preview showed. A drag that lands where the leg was sends
    /// nothing.
    pub(super) fn finish_leg(&mut self, cx: &Cx, gesture: &Gesture, (id, leg, line): (u32, usize, f64)) {
        let Some(landed) = gesture.landing.leg.filter(|_| gesture.landing.worked_out()) else { return };
        if (landed.to - line).abs() < NO_CHANGE {
            return;
        }
        // (Without `snap` the landing is the pointer's own line, so what is sent is what the preview showed.)
        let mut params = json!({"id": id, "leg": leg, "to": inch(landed.raw), "page": cx.page});
        if let (true, Some(map)) = (gesture.landing.snap, params.as_object_mut()) {
            map.insert("snap".to_owned(), Self::snap_param(cx));
        }
        if let Some(answer) = self.run_edit("connector.edit_path", &params) {
            self.noted_snap(&answer);
            self.keep_preview(cx.page, &[id]);
        }
    }

    /// The route the drag in progress would make, in paper inches, and whether the engine would take it. `None` when no leg is dragged.
    fn leg_preview(&self, gesture: &Gesture) -> Option<(Vec<P>, bool)> {
        let Mode::Leg { id, leg, .. } = &gesture.mode else { return None };
        let legs = self.ui.edit.legs.as_ref().filter(|l| l.connector() == *id)?;
        let landed = gesture.landing.leg.filter(|_| gesture.landing.worked_out())?;
        match moved_route(&legs.units, *leg, landed.to * legs.per_inch) {
            Some(units) => Some((units.iter().map(|p| (p.0 / legs.per_inch, p.1 / legs.per_inch)).collect(), true)),
            None => Some((legs.points(), false)),
        }
    }

    /// Paints the handles of the connector selected: a filled blue disc with a white halo at the middle of each leg that fits.
    pub(super) fn paint_leg_handles(&self, painter: &Painter, t: &Tokens, cx: &Cx) {
        let Some(legs) = self.ui.edit.legs.as_ref() else { return };
        for h in legs.handles(cx.view.scale()) {
            paint_bend(painter, t, cx.screen_at(h.at));
        }
    }

    /// Paints the drag of a leg: the route it would make, dashed in the edit colour (in the snap's orange where the engine would refuse the place, and then the route as it is), the
    /// handle where the leg goes, and what the snap pulled it to.
    pub(super) fn paint_leg_gesture(&self, painter: &Painter, t: &Tokens, cx: &Cx, gesture: &Gesture) {
        let Some((route, taken)) = self.leg_preview(gesture) else { return };
        let points: Vec<Pos2> = route.iter().map(|p| cx.screen_at(*p)).collect();
        if points.len() >= 2 && points.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
            let colour = if taken { t.edit_line } else { t.snap_mark };
            painter.extend(Shape::dashed_line(&points, Stroke::new(1.75, colour), super::paint::DASH, super::paint::GAP));
        }
        if let (Mode::Leg { id, leg, .. }, Some(landed)) = (&gesture.mode, gesture.landing.leg)
            && let Some(legs) = self.ui.edit.legs.as_ref().filter(|l| l.connector() == *id)
            && let Some(subject) = leg_subject(&legs.units, legs.per_inch, *leg, landed.to)
        {
            paint_bend(painter, t, cx.screen_at(subject.place));
        }
        super::paint::paint_marks(painter, t, cx, &gesture.landing.marks);
    }

    /// What `ui.inspect` tells of the legs: the connector selected and its handles, the reroute setting it has, and the drag of a leg in progress. `null` when the
    /// selection has none.
    pub(crate) fn legs_state(&self) -> Value {
        let Some(legs) = self.ui.edit.legs.as_ref() else { return Value::Null };
        let Some(cx) = self.ui.page.and_then(|page| self.cx_of(page)) else { return Value::Null };
        let handles: Vec<Value> =
            legs.handles(cx.view.scale()).iter().map(|h| json!({"leg": h.leg, "level": h.level, "at": [inch_of(h.at.0), inch_of(h.at.1)]})).collect();
        let reroute = self.session.document().and_then(|doc| connector_reroute(doc, cx.page, legs.connector())).map(|mode| mode.word());
        let dragging = self.ui.edit.gesture.as_ref().filter(|g| g.active).and_then(|g| match (&g.mode, g.landing.leg) {
            (Mode::Leg { leg, .. }, Some(landed)) => Some(json!({"leg": leg, "to": inch_of(landed.to), "raw": landed.raw, "snap": g.landing.snap})),
            _ => None,
        });
        json!({"connector": legs.connector(), "handles": handles, "reroute": reroute, "dragging": dragging})
    }

    /// The reroute setting the connectors selected agree on, as a word; `None` when they differ, or nothing selected is a connector.
    pub(crate) fn selected_reroute(&self) -> Option<&'static str> {
        let page = self.ui.edit.selection_page()?;
        let doc = self.session.document()?;
        let mut words = self.ui.edit.selection().iter().filter_map(|id| connector_reroute(doc, page, *id)).map(|mode| mode.word());
        let first = words.next()?;
        words.all(|w| w == first).then_some(first)
    }
}

/// A leg's handle: a filled blue disc with a white halo, `select::BEND` across.
fn paint_bend(painter: &Painter, t: &Tokens, at: Pos2) {
    if at.x.is_finite() && at.y.is_finite() {
        painter.circle_filled(at, select::BEND / 2.0 + select::BEND_HALO, t.handle_fill);
        painter.circle_filled(at, select::BEND / 2.0, t.bend_handle);
    }
}

/// A length in inches rounded to a millionth, as a number a client reads.
fn inch_of(v: f64) -> f64 {
    (v * 1.0e6).round() / 1.0e6
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legs(units: Vec<P>, per_inch: f64) -> Legs {
        Legs { key: (1, 1, 1, 7), units, per_inch }
    }

    /// A handle is at the middle of each leg that is at least 24 points long on the screen (less a half point of slack), and says which way the leg runs.
    #[test]
    fn a_leg_has_a_handle_at_its_middle_when_it_is_long_enough_on_the_screen() {
        // A Z of 1.25, 2 and 1.25 inches.
        let z = legs(vec![(2.75, 3.0), (4.0, 3.0), (4.0, 5.0), (5.25, 5.0)], 1.0);
        let all = z.handles(96.0);
        assert_eq!(
            all.iter().map(|h| (h.leg, h.at, h.level)).collect::<Vec<_>>(),
            [(0, (3.375, 3.0), true), (1, (4.0, 4.0), false), (2, (4.625, 5.0), true)]
        );
        // At half the zoom the end legs are 60 points and the middle one 96: all three; at a quarter, 30 and 48; at a tenth, 12 and 19: none.
        assert_eq!(z.handles(48.0).len(), 3);
        assert_eq!(z.handles(24.0).len(), 3);
        assert_eq!(z.handles(9.0).len(), 0);
        // A quarter-inch leg at 100% is 24 points, less a hair of rounding: it has one.
        let short = legs(vec![(0.0, 0.0), (0.25 - 1.0e-9, 0.0)], 1.0);
        assert_eq!(short.handles(96.0).len(), 1);
        assert_eq!(short.handles(90.0).len(), 0, "and not at 90 points to an inch");
        // The route in drawing units, two to an inch: the places are in inches.
        let scaled = legs(vec![(0.0, 0.0), (8.0, 0.0), (8.0, 4.0)], 2.0);
        let at: Vec<P> = scaled.handles(96.0).iter().map(|h| h.at).collect();
        assert_eq!(at, [(2.0, 0.0), (4.0, 1.0)]);
        assert_eq!(scaled.line_of(0), Some(0.0));
        assert_eq!(scaled.line_of(1), Some(4.0));
        assert_eq!(scaled.line_of(2), None);
        assert_eq!(scaled.line_of(usize::MAX), None);
    }

    /// The leg a Shift-arrow key moves is of the kind the key moves across, the nearest the middle of the route, and the first of two the same distance away; none when no
    /// leg of that kind has a handle at the zoom.
    #[test]
    fn the_leg_a_nudge_moves_is_the_one_across_the_key_nearest_the_middle() {
        // A staircase of five legs: level, upright, level, upright, level. The middle of the route is (3, 1).
        let stairs = legs(vec![(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (4.0, 1.0), (4.0, 2.0), (6.0, 2.0)], 1.0);
        assert_eq!(stairs.nudge_target(96.0, true).map(|h| h.leg), Some(2), "the level leg at the middle");
        assert_eq!(stairs.nudge_target(96.0, false).map(|h| h.leg), Some(1), "the two upright legs are as near: the first");
        // A Z: its upright leg is the only one, and its level ends are as near as each other.
        let z = legs(vec![(2.75, 3.0), (4.0, 3.0), (4.0, 5.0), (5.25, 5.0)], 1.0);
        assert_eq!((z.nudge_target(96.0, false).map(|h| h.leg), z.nudge_target(96.0, true).map(|h| h.leg)), (Some(1), Some(0)));
        // Too far out for a handle on any leg: none.
        assert_eq!(z.nudge_target(9.0, false), None);
        assert_eq!(legs(vec![], 1.0).nudge_target(96.0, true), None);
    }

    /// No more handles than a person could take hold of, and none at a place that is not a number.
    #[test]
    fn the_handles_are_capped_and_a_route_that_is_not_a_number_has_none() {
        let long: Vec<P> = (0..2 * MAX_HANDLES + 10).map(|i| if i % 2 == 0 { (i as f64, 0.0) } else { (i as f64 - 1.0, 5.0) }).collect();
        assert_eq!(legs(long, 1.0).handles(96.0).len(), MAX_HANDLES);
        assert!(legs(vec![(0.0, f64::NAN), (5.0, f64::NAN)], 1.0).handles(96.0).is_empty());
        assert!(legs(vec![], 1.0).handles(96.0).is_empty());
        assert!(legs(vec![(0.0, 0.0)], 1.0).handles(96.0).is_empty());
    }
}
