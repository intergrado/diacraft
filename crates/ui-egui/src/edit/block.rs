//! The Text Block tool (Home, Tools; Ctrl+Shift+4) and Rotate Text: the text block of a shape moved, sized and turned apart from the shape.
//!
//! A shape's text sits in a rectangle of its own, the **text block** (`docs/editing-commands.md`, "Text boxes and the text block"). With the tool on,
//! a click selects a shape and **the block of the one shape selected is drawn** from `draw::text_block_place`: its rectangle, the eight handles of its
//! own frame and, above it, a rotation handle (the shape's own handles are not shown). **A drag is drawn at once and committed on release**, as every
//! other drag of the window is, and the release sends one command, one undo step:
//!
//! * a drag of the block's body: `text.block_move {id, dx, dy}`, the distance on the page;
//! * a drag of a handle: `text.block_resize {id, x, y, width, height}`, where (`x`, `y`) is the place of the corner at (0, 0) of the block's frame after
//!   the drag and the sides are along the block's own axes (the frame `text.block` gives the corners of);
//! * a drag of the rotation handle: `text.block_rotate {id, by}`, the change of the direction the text runs on the page, in the steps the shape's own
//!   rotation handle takes.
//!
//! The block turns about its pin, which stays where it is. **Rotate Text** on the Home tab is `text.block_rotate {by: 90}` for each shape selected
//! (`rotate_text`): the engine's rule that a quarter turn is counterclockwise on the page is ours (no desktop page gives the direction or the steps).
//!
//! Choices no specification settles are marked `Assumed` and listed in the task's report.

use egui::{CornerRadius, CursorIcon, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, vec2};
use serde_json::json;

use super::geom::{self, Basis, Handle, P};
use super::gesture::{Gesture, Mode, OnClick, ROTATION_ARM, handle_at};
use super::paint::{HANDLE, ROTATION_RADIUS};
use super::{After, Cx, HANDLE_REACH, HIT_REACH, Tool, inch};
use crate::app::DiacraftApp;
use crate::theme::Tokens;

/// Where the text block of the shape selected is, on the page in inches, as the drawing had it at one revision.
#[derive(Clone, Copy, Debug)]
pub(super) struct BlockView {
    pub page: u32,
    pub id: u32,
    pub revision: u64,
    /// The four corners in the order of the block's own frame: the one at (0, 0), then (width, 0), (width, height) and (0, height).
    pub corners: [P; 4],
    /// The point the block turns about.
    pub pin: P,
    /// The way the text runs on the page, in degrees counterclockwise (the net angle).
    pub direction: f64,
}

/// Where the text block the Text Block tool shows is, in paper inches on the page (`DiacraftApp::text_block_shown`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShownBlock {
    /// The four corners in the order of the block's own frame: (0, 0), (width, 0), (width, height), (0, height).
    pub corners: [(f64, f64); 4],
    /// The point it turns about.
    pub pin: (f64, f64),
    /// The way the text runs on the page, in degrees counterclockwise.
    pub direction: f64,
}

impl BlockView {
    /// The middle of the block.
    fn centre(&self) -> P {
        let sum = self.corners.iter().fold((0.0, 0.0), |a, c| (a.0 + c.0, a.1 + c.1));
        (sum.0 / 4.0, sum.1 / 4.0)
    }

    /// The direction "up" for the text, on the page: a quarter turn left of the way it runs.
    fn up(&self) -> P {
        let radians = self.direction.to_radians();
        (-radians.sin(), radians.cos())
    }
}

/// Where the rotation handle of a block is, and where its stem leaves the block: above the middle of the edge of the block that is at the top of
/// the text (the corners are carried by the block's turn and by the shape's flips, so the edge is found by how far each corner is along "up").
fn rotation_handle(view: &BlockView, cx: &Cx) -> (P, P) {
    let (centre, up) = (view.centre(), view.up());
    let top = view.corners.iter().map(|c| (c.0 - centre.0) * up.0 + (c.1 - centre.1) * up.1).fold(0.0_f64, f64::max);
    let edge = (centre.0 + up.0 * top, centre.1 + up.1 * top);
    let arm = cx.reach(ROTATION_ARM);
    (edge, (edge.0 + up.0 * arm, edge.1 + up.1 * arm))
}

impl DiacraftApp {
    /// Reads where the text block of the one shape selected is, if the drawing, the page or the shape is not the one it was read for. Runs in every
    /// frame the Text Block tool is on.
    pub(super) fn refresh_block(&mut self, page: u32) {
        let id = match self.ui.edit.selection.as_slice() {
            [only] if self.ui.edit.tool == Tool::TextBlock => *only,
            _ => {
                self.ui.edit.block = None;
                return;
            }
        };
        let revision = self.session.revision();
        if self.ui.edit.block.is_some_and(|b| b.page == page && b.id == id && b.revision == revision) {
            return;
        }
        self.ui.edit.block = self.read_block(page, id, revision);
    }

    fn read_block(&self, page: u32, id: u32, revision: u64) -> Option<BlockView> {
        let doc = self.session.document()?;
        let place = diacraft_engine::draw::text_block_place(doc, page, id)?;
        let per_inch = diacraft_engine::draw::units_per_inch(doc, page);
        if !(per_inch.is_finite() && per_inch > 0.0) {
            return None;
        }
        let inches = |p: (f64, f64)| (p.0 / per_inch, p.1 / per_inch);
        let direction = place.direction.to_degrees();
        let corners = place.corners.map(inches);
        let finite = corners.iter().all(|c| c.0.is_finite() && c.1.is_finite()) && direction.is_finite();
        finite.then(|| BlockView { page, id, revision, corners, pin: inches(place.pin), direction: geom::normal_degrees(direction) })
    }

    /// Where the text block the Text Block tool shows is on the page in inches: its four corners in the order of its own frame, then the pin it turns
    /// about, then the way the text runs on the page in degrees. `None` when the tool is off, or no one shape is selected. For `ui.inspect` and tests.
    pub fn text_block_shown(&self) -> Option<ShownBlock> {
        let view = self.block_shown(self.ui.page?)?;
        Some(ShownBlock { corners: view.corners, pin: view.pin, direction: view.direction })
    }

    /// The block to draw and to take hold of: the one read for the shape selected, on this page.
    fn block_shown(&self, page: u32) -> Option<BlockView> {
        self.ui.edit.block.filter(|b| b.page == page && self.ui.edit.selection == [b.id])
    }

    /// The Text Block tool's press: a rotation handle, a handle or the body of the block of the shape selected starts a drag; a shape under the pointer
    /// is selected (and so is its block shown); bare page clears the selection.
    pub(super) fn press_with_block(&mut self, cx: &Cx, p: P) -> Option<(Mode, OnClick)> {
        let reach = cx.reach(HANDLE_REACH);
        if let Some(view) = self.block_shown(cx.page) {
            let (_, grip) = rotation_handle(&view, cx);
            if geom::distance(grip, p) <= reach {
                return Some((Mode::BlockRotate { id: view.id, pin: view.pin, grab: grip, start: view.direction }, OnClick::Nothing));
            }
            if let Some(basis) = Basis::of_corners(&view.corners) {
                if let Some(handle) = handle_at(&basis, p, cx) {
                    return Some((Mode::BlockResize { id: view.id, handle, basis }, OnClick::Nothing));
                }
                if geom::inside_polygon(&view.corners, p) {
                    return Some((Mode::BlockMove { id: view.id, centre: view.centre() }, OnClick::Nothing));
                }
            }
        }
        let hit = self.ui.edit.scene.as_ref().and_then(|s| s.hit(p, cx.reach(HIT_REACH))).map(|item| item.id);
        // Through `set_selection`, which drops a guide selected: a guide and shapes are never selected together.
        self.set_selection(cx.page, hit.into_iter().collect());
        self.refresh_block(cx.page);
        None
    }

    /// The cursor over the page with the Text Block tool: a grab over the rotation handle, the resize arrows over a handle, a move over the block.
    pub(super) fn block_cursor(&self, cx: &Cx, p: P) -> CursorIcon {
        let Some(view) = self.block_shown(cx.page) else { return CursorIcon::Crosshair };
        let (_, grip) = rotation_handle(&view, cx);
        if geom::distance(grip, p) <= cx.reach(HANDLE_REACH) {
            return CursorIcon::Grab;
        }
        match Basis::of_corners(&view.corners) {
            Some(basis) => match handle_at(&basis, p, cx) {
                Some(handle) => super::gesture::resize_cursor(&basis, handle),
                None if geom::inside_polygon(&view.corners, p) => CursorIcon::Move,
                None => CursorIcon::Crosshair,
            },
            None => CursorIcon::Crosshair,
        }
    }

    /// Remembers where the block of `id` now is, so its outline stays painted until the picture of the new revision is on the screen.
    fn keep_block_preview(&mut self, page: u32, id: u32) {
        let revision = self.session.revision();
        self.ui.edit.block = self.read_block(page, id, revision);
        self.ui.edit.after = self.ui.edit.block.map(|view| After { page, revision, paths: vec![view.corners.to_vec()], since: self.clock.now() });
    }

    /// The block was dragged and let go (a move): `text.block_move`, by the distance the drag's landing gives (its middle snapped as a place:
    /// `gesture.rs`).
    pub(super) fn finish_block_move(&mut self, cx: &Cx, gesture: &Gesture, id: u32) {
        let (dx, dy) = gesture.move_delta();
        if dx.abs() < 1e-6 && dy.abs() < 1e-6 {
            return;
        }
        let params = json!({"id": id, "dx": inch(dx), "dy": inch(dy), "page": cx.page});
        if self.run_edit("text.block_move", &params).is_some() {
            self.keep_block_preview(cx.page, id);
        }
    }

    /// A handle of the block was dragged and let go: `text.block_resize` with the corner at (0, 0) of the block's frame and the sides along its axes.
    pub(super) fn finish_block_resize(&mut self, cx: &Cx, gesture: &Gesture, id: u32, handle: Handle, basis: Basis) {
        let (lo, hi) = gesture.new_frame(handle, basis);
        let (old_lo, old_hi) = basis.local_box();
        if geom::distance(lo, old_lo) < 1e-6 && geom::distance(hi, old_hi) < 1e-6 {
            return;
        }
        let corner = basis.to_page(lo);
        let params = json!({
            "id": id, "x": inch(corner.0), "y": inch(corner.1), "width": inch(hi.0 - lo.0), "height": inch(hi.1 - lo.1), "page": cx.page
        });
        if self.run_edit("text.block_resize", &params).is_some() {
            self.keep_block_preview(cx.page, id);
        }
    }

    /// The rotation handle of the block was let go: `text.block_rotate` with the change of the way the text runs.
    pub(super) fn finish_block_rotate(&mut self, cx: &Cx, gesture: &Gesture, id: u32, (pin, grab, start): (P, P, f64)) {
        let by = geom::normal_degrees(gesture.turn_to(cx, pin, grab, start) - start);
        if by.abs() < 1e-6 {
            return;
        }
        let params = json!({"id": id, "by": by, "page": cx.page});
        if self.run_edit("text.block_rotate", &params).is_some() {
            self.keep_block_preview(cx.page, id);
        }
    }

    /// `format.rotateText`: turns the text block of every shape selected by `by` degrees, counterclockwise on the page, about its pin (Rotate Text).
    /// More than one shape is one batch, so one undo step.
    pub(crate) fn rotate_text(&mut self, by: f64) -> diacraft_engine::Result<()> {
        let (page, ids) = self.selection_target("format.rotateText")?;
        let step = |id: &u32| json!({"id": id, "by": by, "page": page});
        match ids.as_slice() {
            [only] => self.execute("text.block_rotate", &step(only)).map(|_| ()),
            many => {
                let commands: Vec<serde_json::Value> = many.iter().map(|id| json!({"command": "text.block_rotate", "params": step(id)})).collect();
                self.execute("command.batch", &json!({"commands": commands, "label": "Rotate text"})).map(|_| ())
            }
        }
    }

    /// Paints the block of the shape selected with the Text Block tool: its rectangle, the pin it turns about, the eight handles and the rotation handle.
    pub(super) fn paint_block(&self, painter: &Painter, t: &Tokens, cx: &Cx) {
        let Some(view) = self.block_shown(cx.page) else { return };
        // While a drag of it is on, the preview is what shows (`paint_block_gesture`).
        if self
            .ui
            .edit
            .gesture
            .as_ref()
            .is_some_and(|g| g.active && matches!(g.mode, Mode::BlockMove { .. } | Mode::BlockResize { .. } | Mode::BlockRotate { .. }))
        {
            return;
        }
        let corners: Vec<Pos2> = view.corners.iter().map(|c| cx.screen_at(*c)).collect();
        let mut line = corners.clone();
        line.push(corners[0]);
        painter.extend(Shape::dashed_line(&line, Stroke::new(1.5, t.edit_line), 5.0, 3.0));
        let stroke = Stroke::new(1.5, t.edit_line);
        if let Some(basis) = Basis::of_corners(&view.corners) {
            let (lo, hi) = basis.local_box();
            for handle in geom::visible_handles(lo, hi, cx.view.scale(), f64::from(HANDLE)) {
                let at = cx.screen_at(basis.to_page(handle.at(lo, hi)));
                painter.rect(Rect::from_center_size(at, vec2(HANDLE, HANDLE)), CornerRadius::same(1), t.handle_fill, stroke, StrokeKind::Inside);
            }
        }
        let (edge, grip) = rotation_handle(&view, cx);
        let (from, at) = (cx.screen_at(edge), cx.screen_at(grip));
        painter.line_segment([from, at], Stroke::new(1.0, t.edit_line));
        painter.circle(at, ROTATION_RADIUS, t.handle_fill, stroke);
        let arc: Vec<Pos2> = (0..=10)
            .map(|i| {
                let angle = -0.4 + std::f32::consts::TAU * 0.78 * i as f32 / 10.0;
                at + vec2(angle.cos(), angle.sin()) * (ROTATION_RADIUS - 2.5)
            })
            .collect();
        painter.add(Shape::line(arc.clone(), Stroke::new(1.25, t.edit_line)));
        if let Some(end) = arc.last() {
            painter.line_segment([*end, *end + vec2(2.6, -1.0)], Stroke::new(1.25, t.edit_line));
            painter.line_segment([*end, *end + vec2(-0.5, -2.8)], Stroke::new(1.25, t.edit_line));
        }
        // The pin the block turns about: a small circle.
        painter.circle(cx.screen_at(view.pin), 3.5, t.handle_fill, stroke);
    }

    /// Paints the block as the drag in progress would leave it.
    pub(super) fn paint_block_gesture(&self, painter: &Painter, t: &Tokens, cx: &Cx, gesture: &Gesture) {
        let Some(view) = self.block_shown(cx.page) else { return };
        let corners: Vec<P> = match &gesture.mode {
            Mode::BlockMove { .. } => {
                let (dx, dy) = gesture.move_delta();
                view.corners.iter().map(|c| (c.0 + dx, c.1 + dy)).collect()
            }
            Mode::BlockResize { handle, basis, .. } => {
                let (lo, hi) = gesture.new_frame(*handle, *basis);
                basis.corners_of(lo, hi).to_vec()
            }
            Mode::BlockRotate { pin, grab, start, .. } => {
                let to = gesture.turn_to(cx, *pin, *grab, *start);
                view.corners.iter().map(|c| geom::turned_about(*c, *pin, to - *start)).collect()
            }
            _ => return,
        };
        let points: Vec<Pos2> = corners.iter().map(|c| cx.screen_at(*c)).collect();
        painter.add(egui::epaint::PathShape::convex_polygon(points.clone(), t.edit_fill, Stroke::NONE));
        let mut line = points.clone();
        if let Some(first) = points.first() {
            line.push(*first);
        }
        painter.extend(Shape::dashed_line(&line, Stroke::new(1.5, t.edit_line), 5.0, 3.0));
        if let Mode::BlockRotate { pin, .. } = &gesture.mode {
            let (at, pointer) = (cx.screen_at(*pin), cx.screen_at(gesture.now));
            painter.extend(Shape::dashed_line(&[at, pointer], Stroke::new(1.0, t.edit_line), 5.0, 3.0));
            painter.circle(at, 3.5, t.handle_fill, Stroke::new(1.5, t.edit_line));
        }
    }
}
