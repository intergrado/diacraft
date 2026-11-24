//! The drawing tools in the window: Rectangle, Ellipse, Line, Freeform, Arc and Pencil (Home, Tools, Drawing Tools).
//!
//! **A tool is a mode**: chosen from the menu or by its key (Ctrl+8 Rectangle, Ctrl+9 Ellipse, Ctrl+6 Line, Ctrl+5 Freeform, Ctrl+7 Arc, Ctrl+4 Pencil), it stays on until the Pointer Tool
//! or Escape. **One press and release is one command**, `shape.draw` (`docs/editing-commands.md`, "The drawing tools"): the pointer's points go to the engine, which makes the shape or
//! carries a path on, as one undo step, and the new shape is selected. Nothing is sent while the button is down; Escape in the middle drops the stroke.
//!
//! **The preview is the engine's.** While the button is down the window asks `diacraft_engine::draw::pen::outline` what the release would make, from the same points and settings it
//! will send, so the outline painted is the shape drawn (a stroke too long to fit at every frame is shown as the stroke itself, and fitted on release). **What a drag reads:**
//!
//! * Rectangle, Ellipse, Line and Arc read the press and the pointer: the start is on the grid and the guides as the engine's snap says (`land_place`, as the Text tool's box does), and
//!   so is the end unless Shift is held (Shift makes a square, a circle or a segment at a step of 45 degrees, which the grid would undo).
//! * Pencil and Freeform read the whole stroke: every place the pointer went through in the frame, at least two screen points from the one before it, at most
//!   [`MOST_POINTS`] of them. The tolerance of the fit is [`TOLERANCE_POINTS`] screen points, and the Freeform's smoothing is the window's setting (`window.freeformSmoothing`).
//!
//! **A path goes on** ("drawing the next segment from that vertex joins them"): with Line, Arc, Freeform or Pencil on and **one shape selected whose open path the engine can
//! carry on** (`pen::continuable`), a press within reach of its end vertex goes on from it, and a stroke let go within reach of its first vertex closes it and it takes the fill. A
//! new shape is selected when it is drawn, so the next stroke from its end joins it. The two ends of such a path are marked while one of these tools is on.

use diacraft_engine::draw::pen::{self, Settings, Tool as PenTool, Vertices};
use egui::epaint::PathShape;
use egui::{Painter, Pos2, Shape, Stroke};
use serde_json::{Value, json};

use super::geom::{self, P};
use super::gesture::{Gesture, Mode};
use super::{Cx, HANDLE_REACH, Tool, inch};
use crate::app::DiacraftApp;
use crate::theme::Tokens;

/// The least a point of a stroke is from the one before it, in screen points.
const POINT_GAP: f32 = 2.0;

/// How far the curve or the lines may stand from the stroke, in screen points.
const TOLERANCE_POINTS: f64 = 3.0;

/// The most points a stroke keeps: what `shape.draw` takes, less the one the release adds.
pub const MOST_POINTS: usize = pen::MAX_STROKE_POINTS - 1;

/// The longest stroke fitted at every frame of the drag; a longer one is painted as it is and fitted when the button comes up.
const PREVIEW_FIT_POINTS: usize = 1_500;

/// The radius of the mark at a vertex, in screen points.
const VERTEX_RADIUS: f32 = 4.0;

/// What a press that goes on from a path carries on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Onto {
    /// The shape whose path goes on.
    pub id: u32,
    /// Where its path ends and began, in paper inches.
    pub vertices: Vertices,
}

/// The tool a window command names (`tool.rectangle` and the others), if it names one.
pub(crate) fn tool_of_command(id: &str) -> Option<PenTool> {
    PenTool::from_name(id.strip_prefix("tool.")?)
}

/// Whether the tool reads the whole stroke the pointer made, and not only the press and the pointer.
fn reads_the_stroke(tool: PenTool) -> bool {
    matches!(tool, PenTool::Freeform | PenTool::Pencil)
}

/// Adds `p` to the stroke when it is at least `gap` from the last point (the pointer is not at the last place twice).
fn push_point(stroke: &mut Vec<P>, p: P, gap: f64) {
    if stroke.len() < MOST_POINTS && stroke.last().is_none_or(|last| geom::distance(*last, p) >= gap) {
        stroke.push(p);
    }
}

/// What a drag will send: the points, the settings and whether the stroke closes the path.
struct Plan {
    points: Vec<P>,
    settings: Settings,
    close: bool,
}

impl DiacraftApp {
    /// The drawing tool used last, which the Drawing Tools button shows (`None` before one has been).
    pub fn last_drawing_tool(&self) -> Option<PenTool> {
        self.ui.edit.last_pen
    }

    /// The window's setting of how much a Freeform stroke is smoothed, 0 to 1.
    pub fn freeform_smoothing(&self) -> f64 {
        self.ui.freeform_smoothing
    }

    /// The one selected shape a stroke with `tool` goes on from when it is pressed at `p`: its path is open and the press is on its end vertex.
    fn onto_at(&self, cx: &Cx, p: P, tool: PenTool) -> Option<Onto> {
        if !tool.goes_on() || self.ui.edit.selection_page != Some(cx.page) {
            return None;
        }
        let [id] = self.ui.edit.selection.as_slice() else { return None };
        let vertices = pen::continuable(self.session.document()?, cx.page, *id)?;
        (geom::distance(vertices.end, p) <= cx.reach(HANDLE_REACH)).then_some(Onto { id: *id, vertices })
    }

    /// The press of a drawing tool: the stroke begins here, or on the end vertex of the path it goes on from.
    pub(super) fn press_with_draw(&self, cx: &Cx, p: P, tool: PenTool) -> Mode {
        let onto = self.onto_at(cx, p, tool);
        let first = onto.map_or(p, |o| o.vertices.end);
        Mode::Draw { tool, stroke: vec![first], onto }
    }

    /// The pointer moved with a drawing tool down: the places it went through since the last frame, and where it is now, join the stroke.
    pub(super) fn draw_follow(&mut self, cx: &Cx, now: P) {
        let trail = std::mem::take(&mut self.ui.edit.trail);
        let gap = cx.reach(POINT_GAP);
        let Some(Mode::Draw { tool, stroke, .. }) = self.ui.edit.gesture.as_mut().map(|g| &mut g.mode) else { return };
        if reads_the_stroke(*tool) {
            for at in trail {
                push_point(stroke, cx.page_at(at), gap);
            }
            // The pointer ends the stroke where it is: a place a hair from the last is the last place moved.
            let held = stroke.len() > 1 && stroke.last().is_some_and(|last| geom::distance(*last, now) < gap);
            match stroke.last_mut() {
                Some(last) if held => *last = now,
                _ => push_point(stroke, now, 0.0),
            }
        } else {
            stroke.truncate(1);
            stroke.push(now);
        }
    }

    /// What the drag in progress will send, from the stroke, the landing, the keys and the zoom.
    fn plan_of(&self, cx: &Cx, g: &Gesture) -> Option<Plan> {
        let Mode::Draw { tool, stroke, onto } = &g.mode else { return None };
        let reach = cx.reach(HANDLE_REACH);
        let points: Vec<P> = if reads_the_stroke(*tool) {
            stroke.clone()
        } else {
            // The two ends of the drag: the start and the end as the snap landed them, or as the pointer made them while the page is not ready.
            let (first, second) = g.landing.corners.filter(|_| g.landing.worked_out()).unwrap_or((g.origin, g.now));
            vec![onto.map_or(first, |o| o.vertices.end), second]
        };
        let (first, last) = (points.first().copied()?, points.last().copied()?);
        // A stroke let go over the first vertex of the path closes it: the vertex of the path it goes on from, or the start of a stroke that has gone out and come back.
        let went = stroke.windows(2).map(|w| geom::distance(w[0], w[1])).sum::<f64>();
        let close = match onto {
            Some(o) => geom::distance(last, o.vertices.start) <= reach,
            None => matches!(tool, PenTool::Pencil | PenTool::Freeform) && went > 3.0 * reach && geom::distance(last, first) <= reach,
        };
        let tolerance = (TOLERANCE_POINTS / cx.view.scale()).clamp(0.002, 0.5);
        let settings = Settings { shift: g.shift, tolerance, smoothing: self.ui.freeform_smoothing };
        Some(Plan { points, settings, close })
    }

    /// The button came up on a drawing tool's press: one `shape.draw`, and the shape it drew is selected. A press and release that did not move draws nothing.
    pub(super) fn finish_draw(&mut self, cx: &Cx, gesture: &Gesture) {
        let Mode::Draw { tool, onto, .. } = &gesture.mode else { return };
        let Some(plan) = self.plan_of(cx, gesture) else { return };
        let points: Vec<Value> = plan.points.iter().map(|p| json!([inch(p.0), inch(p.1)])).collect();
        let mut params = json!({
            "page": cx.page,
            "tool": tool.name(),
            "points": points,
            "shift": plan.settings.shift,
            "tolerance": plan.settings.tolerance,
            "smoothing": plan.settings.smoothing,
        });
        if let Some(map) = params.as_object_mut() {
            if let Some(onto) = onto {
                map.insert("continue".to_owned(), json!(onto.id));
            }
            if plan.close {
                map.insert("close".to_owned(), json!(true));
            }
        }
        if let Some(answer) = self.run_edit("shape.draw", &params)
            && let Some(id) = answer.get("id").and_then(Value::as_u64).and_then(|id| u32::try_from(id).ok())
        {
            self.select(&[id]);
            self.keep_preview(cx.page, &[id]);
        }
    }

    /// The window state's `drawing`: the tool and the stroke while a drag with a drawing tool is on (`null` otherwise). For tests and agents.
    pub(crate) fn drawing_state(&self) -> Value {
        let Some(g) = self.ui.edit.gesture.as_ref().filter(|g| g.active) else { return Value::Null };
        let Mode::Draw { tool, stroke, onto } = &g.mode else { return Value::Null };
        let cx = self.ui.page.and_then(|page| self.cx_of(page));
        let closing = cx.and_then(|cx| self.plan_of(&cx, g)).is_some_and(|plan| plan.close);
        json!({"tool": tool.name(), "points": stroke.len(), "continuing": onto.map(|o| o.id), "closing": closing})
    }

    /// Paints what the drag with a drawing tool would make: the outline the engine says (filled lightly when it closes), the snap's marks, and the vertex it is carried on from.
    pub(super) fn paint_draw(&self, painter: &Painter, t: &Tokens, cx: &Cx, gesture: &Gesture) {
        if !gesture.active {
            return;
        }
        let Mode::Draw { tool, stroke, onto } = &gesture.mode else { return };
        let Some(plan) = self.plan_of(cx, gesture) else { return };
        // The engine's outline; a stroke too long to fit at every frame is shown as the pointer made it.
        let outline =
            if reads_the_stroke(*tool) && plan.points.len() > PREVIEW_FIT_POINTS { None } else { pen::outline(*tool, &plan.points, &plan.settings) };
        let (points, closed): (Vec<P>, bool) = match outline {
            Some(o) => (o.points, o.closed),
            None => (stroke.clone(), false),
        };
        let line: Vec<Pos2> = points.iter().map(|p| cx.screen_at(*p)).collect();
        if closed && line.len() >= 3 {
            painter.add(PathShape::convex_polygon(line.clone(), t.edit_fill, Stroke::NONE));
        }
        if line.len() >= 2 {
            painter.add(Shape::line(line, Stroke::new(1.75, t.edit_line)));
        }
        super::paint::paint_marks(painter, t, cx, &gesture.landing.marks);
        if let Some(onto) = onto {
            self.paint_vertices(painter, t, cx, onto.vertices, plan.close);
        }
    }

    /// The two ends of an open path: a filled mark at the end the next stroke goes on from, and a ring at the start, lit while a stroke would close the path.
    pub(super) fn paint_vertices(&self, painter: &Painter, t: &Tokens, cx: &Cx, v: Vertices, closing: bool) {
        let (end, start) = (cx.screen_at(v.end), cx.screen_at(v.start));
        painter.circle(end, VERTEX_RADIUS, t.edit_line, Stroke::new(1.5, t.handle_fill));
        let (fill, ring) = if closing { (t.glue_halo, t.glue_on) } else { (t.handle_fill, t.edit_line) };
        painter.circle(start, VERTEX_RADIUS + 0.5, fill, Stroke::new(1.5, ring));
    }

    /// The vertices of the one selected shape while a tool that goes on from a path is on and no stroke is down: they say where a press goes on, as the reference app's "each segment's
    /// end shows a vertex".
    pub(super) fn paint_selected_vertices(&self, painter: &Painter, t: &Tokens, cx: &Cx) {
        let Tool::Draw(tool) = self.ui.edit.tool else { return };
        if !tool.goes_on() || self.ui.edit.gesture.is_some() || self.ui.edit.selection_page != Some(cx.page) {
            return;
        }
        let [id] = self.ui.edit.selection.as_slice() else { return };
        let Some(doc) = self.session.document() else { return };
        if let Some(v) = pen::continuable(doc, cx.page, *id) {
            self.paint_vertices(painter, t, cx, v, false);
        }
    }

    /// `draw.closeShape` (Draw, Create, Close Shape): the open path of each shape selected is closed with a straight segment and takes the fill (`shape.close`; one undo step for all).
    pub(crate) fn close_selected_shapes(&mut self) -> diacraft_engine::Result<()> {
        let (page, ids) = self.selection_target("draw.closeShape")?;
        let steps: Vec<Value> = ids.iter().map(|id| json!({"command": "shape.close", "params": {"id": id, "page": page}})).collect();
        let before = self.session.revision();
        self.execute("command.batch", &json!({"commands": steps, "label": "Close shape"}))?;
        if self.session.revision() != before {
            self.keep_preview(page, &ids);
        }
        Ok(())
    }
}
