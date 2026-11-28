//! A drag with the pointer, from the press to the release: selecting, moving, resizing, turning, joining and re-joining.
//!
//! A press decides what the drag is, from what is under the pointer (a handle of the selected shape, a shape, nothing). The drag then
//! only changes what is previewed; the release sends the one command. Escape in the middle drops the drag and sends nothing.
//!
//! **A shape that is turned or flipped has its handles** (gap U9): they sit on the shape's own box where the turn and the flips carry it,
//! and a drag works in the shape's own frame ([`Basis`]), which is what `shape.resize` takes (the page place of the corner at
//! (0, 0) of the frame, and the width and height along its axes). **The rotation handle** is above a selected 2-D shape or group; dragging it turns
//! the shape about its pin to the angle the engine's `snap_turn` gives (another shape's angle within reach, else the step of `rotation_step`) and sends
//! `shape.rotate` with the angle the pointer gave and `snap: {zoom, radius}`. **A second click on
//! a selected group** selects the member under the pointer (and one on a selected member, its sibling): the scene reads the members.
//!
//! **Several shapes, and a group, are resized and turned by the handles of their box**. A group is one shape: its handles work as a shape's do (`shape.resize`
//! with `id`, `shape.rotate`), and its members follow it. **For several shapes** the box is their joint box, which the engine stretches from, and the handles stand
//! `theme::select::BOX_MARGIN` off it ([`Mode::ResizeSet`]): a corner stretches them about the opposite corner in proportion, a side stretches in one axis (in proportion with
//! Shift), and the engine snaps the joint box as it does a shape's; the release sends `shape.resize` with `ids` and the box. The rotation handle turns them about the middle of
//! the joint box ([`Mode::RotateSet`]) by the angle the pointer goes round it, in steps that get finer the farther it is, and sends `shape.rotate` with `by` and `about`.
//!
//! **Where a drag lands is the engine's snap** (`snapping.rs`): each place of the pointer is worked out once into a [`Landing`], which the
//! preview paints and the release sends, with `snap` so that the command snaps it the same way and holds a box pulled to a guide to it. A press on a
//! **guide** (`guides.rs`) selects it, and its drag is `guide.move`.

use diacraft_engine::snap::{Axis, BoxAt, Extras, OwnFrame, Rect, Use, is_square, rotation_step, snaps_in_its_own_axes};
use egui::{CursorIcon, Modifiers, Pos2};
use serde_json::{Value, json};

use super::autoconnect::Side;
use super::geom::{self, Basis, Handle, P};
use super::scene::{Item, Kind};
use super::select_look::OnBox;
use super::snapping::{Mark, MoveSubject, sent, view_of};
use super::{Cx, DRAG_START, HANDLE_REACH, HIT_REACH, LEAST_SIZE, POINT_REACH, Tool, inch};
use crate::app::DiacraftApp;

/// How little a drag may change what it moves for its release to send nothing: **a billionth** (of an inch, or of a degree for a turn), the tolerance the preview and the release are
/// held equal to. A drag that comes back to where it began changes nothing; one that comes to a place a hair from where it began does (a release
/// under a millionth was not sent, though its preview showed the change and drew its marks).
pub(super) const NO_CHANGE: f64 = 1.0e-9;

/// Which end of a connector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Which {
    Begin,
    End,
}

impl Which {
    fn word(self) -> &'static str {
        match self {
            Which::Begin => "begin",
            Which::End => "end",
        }
    }
}

/// Where a connector end would go if the button were released now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Target {
    /// Nowhere: the end stays free at this place.
    Free(P),
    /// Glued to a shape; the engine takes the point nearest the other end.
    Shape(u32),
    /// Glued to one connection point.
    Point { id: u32, index: u32, at: P },
}

impl Target {
    /// The end as `connector.add` and `connector.reconnect` take it.
    fn param(self) -> serde_json::Value {
        match self {
            Target::Free(p) => json!({"x": inch(p.0), "y": inch(p.1)}),
            Target::Shape(id) => json!({"id": id}),
            Target::Point { id, index, .. } => json!({"id": id, "point": index}),
        }
    }
}

/// What the drag does.
#[derive(Clone, Debug)]
pub(super) enum Mode {
    /// A rubber band from empty page. `base` is what was selected when it began, kept by Shift or Ctrl.
    Marquee { additive: bool, base: Vec<u32> },
    /// The Lasso Select tool's loop (`region.rs`): the places the pointer went through, in paper inches (the press first), and what was selected when it began, kept by Shift or Ctrl.
    Lasso { base: Vec<u32>, trail: Vec<P> },
    /// The selected shapes follow the pointer, and land where the engine's snap puts their box ([`Landing`]). **`copy`**: the press was made with Ctrl
    /// held, so the release leaves the shapes where they are and makes copies the drag's distance away (`shape.duplicate`).
    Move { ids: Vec<u32>, copy: bool },
    /// A handle of the one selected shape, with the shape's frame on the page as it was when the drag began.
    Resize { id: u32, handle: Handle, basis: Basis },
    /// The rotation handle of the one selected shape: the shape turns about `pin`, from the angle it had (`start`, degrees) as the pointer goes
    /// round it from where it was grabbed (`grab`).
    Rotate { id: u32, pin: P, grab: P, start: f64 },
    /// A handle of the combined box of several shapes `ids`: they are stretched as one. `basis` is their joint box laid on the page's own axes as it was when the
    /// drag began (the frame the drag works in), and `margin` how far, in inches, the drawn box and its handles stand off the joint box.
    ResizeSet { ids: Vec<u32>, handle: Handle, basis: Basis, margin: f64 },
    /// The rotation handle of the combined box of several shapes `ids`: they turn as one about `pin`, the middle of their joint box, by the angle the pointer goes round it from
    /// where it was grabbed (`grab`).
    RotateSet { ids: Vec<u32>, pin: P, grab: P },
    /// A new connector from `from`.
    Join { from: Target },
    /// A press on the AutoConnect arrow `side` of shape `from` (`autoconnect.rs`): a click joins its nearest neighbour or makes a copy, a drag onto a shape joins that shape (`anchor`
    /// is the middle of the shape's side, where the drag's line begins).
    Arrow { from: u32, side: Side, anchor: P },
    /// An end of the selected connector.
    Reshape { id: u32, end: Which },
    /// The yellow handle at the end of the leader of the selected callout `id` (`callout.rs`): the release ties the callout to the shape it is on, or leaves its end where it is let go.
    Tip { id: u32 },
    /// The handle at the middle of leg `leg` of the selected connector `id` (`legs.rs`): the leg moves across, from the line it lies on (`line`, paper inches: its height for a
    /// level leg, its place along x for an upright one) by as far as the pointer goes.
    Leg { id: u32, leg: usize, level: bool, line: f64 },
    /// The Text tool's press: on a shape (`on`), a click types into it; on bare page, a click makes a box where it is and a drag makes one of the
    /// size dragged. The release opens the text box and sends nothing (`text.box` goes with the text).
    TextBox { on: Option<u32> },
    /// The body of the text block of shape `id` follows the pointer; `centre` is the middle of the block when the drag began.
    BlockMove { id: u32, centre: P },
    /// A handle of the text block of shape `id`, with the block's frame on the page as it was when the drag began.
    BlockResize { id: u32, handle: Handle, basis: Basis },
    /// The rotation handle of the text block of shape `id`: the block turns about `pin`, from the direction the text ran in (`start`, degrees).
    BlockRotate { id: u32, pin: P, grab: P, start: f64 },
    /// A guide dragged along its axis from where it was (`at`, paper inches); `held` are the shapes glued to it, which follow it.
    Guide { id: u32, axis: Axis, at: f64, held: Vec<u32> },
    /// A drawing tool's press (`draw.rs`): the points the pointer has gone through (the press and the pointer for the tools that read the two ends of a drag), and the path it goes
    /// on from, if the press was on its end vertex. The release sends `shape.draw`.
    Draw { tool: diacraft_engine::draw::pen::Tool, stroke: Vec<P>, onto: Option<super::draw::Onto> },
}

/// What a drag's landing was worked out for: the pointer's place, Shift, Alt, the document's revision, the zoom (which sets how far a target pulls and the grid's
/// pitch: a zoom with the pointer still lands the drag elsewhere), the view (which a size match looks in) and how many answers the page's reader has given (a page
/// that has just been made ready lands the drag again).
#[derive(Clone, Copy, Debug, PartialEq)]
struct LandKey {
    now: P,
    shift: bool,
    alt: bool,
    revision: u64,
    scale: f64,
    view: [f64; 4],
    reads: u64,
}

/// A turn by the rotation handle, as the drag has it now: what is sent and what the preview shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Turning {
    /// The angle the pointer gives, in degrees, and how far it is from the pin, in inches: the numbers `shape.rotate` is sent, as it is sent them.
    pub raw: f64,
    pub radius: f64,
    /// The angle the shape comes to, in degrees kept from just over -180 up to 180: the engine's `snap_turn`, or the step alone when none is asked.
    pub to: f64,
}

/// Where a drag lands if the button comes up now, as the engine's snap gives it (`snapping.rs`): what the preview paints and what the release sends.
/// Worked out once for each place of the pointer.
#[derive(Clone, Debug, Default)]
pub(super) struct Landing {
    /// What it was worked out for.
    key: Option<LandKey>,
    /// A move: the distance the pointer gives (as sent, with no hold: Shift's `lock` is the engine's), the page axis a Shift-drag keeps to, and the distance the shapes
    /// go, held to that axis and snapped.
    pub raw: P,
    pub lock: Option<Axis>,
    pub delta: P,
    /// A resize: the place, width and height the handle's drag gives and those the shape lands at, as sent; and the box it lands in, in the shape's own frame.
    /// `keep` is whether the proportions are kept (a corner always, a side with Shift), which the release asks of the engine with `snap`.
    pub raw_sized: Option<BoxAt>,
    pub sized: Option<BoxAt>,
    pub frame: Option<(P, P)>,
    pub keep: bool,
    /// A turn.
    pub turn: Option<Turning>,
    /// A join: where the connector begins. A join or a reshape: where the end dragged goes.
    pub start: Option<Target>,
    pub end: Option<Target>,
    /// A leg dragged by hand: the line the pointer gives and the line it lands on (`legs.rs`).
    pub leg: Option<super::legs::LegLanding>,
    /// The Text tool's drag: the corners of the box.
    pub corners: Option<(P, P)>,
    /// A guide's drag: where it goes, paper inches along its axis.
    pub guide_at: Option<f64>,
    /// What the snap pulled the drag to, drawn from where the box or the place lands.
    pub marks: Vec<Mark>,
    /// Whether the release sends `snap`: not under Alt, and not while the page is not ready for the engine's snap (the first frames of a drag), where the preview shows the
    /// drag as it is and the release is what it showed.
    pub snap: bool,
}

impl Landing {
    /// Whether it has been worked out (the drag is one, and the pointer has been seen since).
    pub fn worked_out(&self) -> bool {
        self.key.is_some()
    }
}

/// A press and what has become of it.
#[derive(Clone, Debug)]
pub(super) struct Gesture {
    pub mode: Mode,
    /// Where the press was, on the screen and on the page.
    pub start: Pos2,
    pub origin: P,
    /// Where the pointer is now.
    pub now: P,
    /// The pointer has gone far enough for the press to be a drag.
    pub active: bool,
    /// What a connector end would be glued to, for `Join` and `Reshape`.
    pub target: Target,
    pub shift: bool,
    pub alt: bool,
    /// What the button coming up with no drag does to the selection (the press itself has not).
    pub on_click: OnClick,
    /// Where the drag lands, once it is one.
    pub landing: Landing,
    /// A move: what it snaps, read once for the drag (the shapes' box and their connection points), not at each move of the pointer.
    pub subject: Option<MoveSubject>,
}

/// What a press that turns out to be a click does to the selection, when the press itself left it alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OnClick {
    Nothing,
    /// **The press itself starts the move with the group** (so a drag does not pull a member out of it), and the selection goes to this member when
    /// the button comes up with no drag.
    Select(u32),
    /// A press with Ctrl held on a shape is the start of a copy-drag, so the shape is added to the selection, or dropped from it, only
    /// when the button comes up with no drag: Ctrl-click keeps its meaning.
    Toggle(u32),
}

/// What a press on a group does about its members.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Enter {
    /// Not into a group.
    No,
    /// Select this member at once: a second click inside a member's own group, or the first click on a group whose members come first.
    Now(u32),
    /// The group is selected: move it with the press, and select this member only if the button comes up without a drag.
    OnClick(u32),
}

impl Gesture {
    /// The cursor to show while the drag is on.
    pub fn cursor(&self) -> CursorIcon {
        match &self.mode {
            Mode::Move { copy: true, .. } => CursorIcon::Copy,
            Mode::Move { .. } => CursorIcon::Move,
            Mode::Resize { handle, basis, .. } => resize_cursor(basis, *handle),
            Mode::Rotate { .. } | Mode::RotateSet { .. } => CursorIcon::Grabbing,
            Mode::ResizeSet { handle, .. } => page_cursor(*handle),
            Mode::Marquee { .. } | Mode::Lasso { .. } => CursorIcon::Crosshair,
            Mode::Join { .. } | Mode::Reshape { .. } | Mode::TextBox { .. } | Mode::Arrow { .. } | Mode::Draw { .. } | Mode::Tip { .. } => {
                CursorIcon::Crosshair
            }
            Mode::Leg { level: true, .. } => CursorIcon::ResizeVertical,
            Mode::Leg { level: false, .. } => CursorIcon::ResizeHorizontal,
            Mode::BlockMove { .. } => CursorIcon::Move,
            Mode::BlockResize { handle, basis, .. } => resize_cursor(basis, *handle),
            Mode::BlockRotate { .. } => CursorIcon::Grabbing,
            Mode::Guide { axis: Axis::X, .. } => CursorIcon::ResizeHorizontal,
            Mode::Guide { axis: Axis::Y, .. } => CursorIcon::ResizeVertical,
        }
    }

    /// For a move: the distance the pointer has gone, rounded as it is sent (no hold), and with Shift the page axis the move keeps to: the one the **whole drag** has gone
    /// farther along, a tie going to x (`lock`). The axis may change during one drag, and the release takes the axis of the last move.
    pub fn raw_move(&self) -> (P, Option<Axis>) {
        let (dx, dy) = (self.now.0 - self.origin.0, self.now.1 - self.origin.1);
        let lock = self.shift.then(|| if dx.abs() >= dy.abs() { Axis::X } else { Axis::Y });
        ((sent(dx), sent(dy)), lock)
    }

    /// For a move: the distance the pointer has gone, held to the axis Shift keeps to (the axis it holds still is given), rounded as it is sent. What goes where no snap does:
    /// the engine holds a move sent with `lock` the same way (`shape.move`).
    pub fn raw_delta(&self) -> (P, Option<Axis>) {
        let (raw, lock) = self.raw_move();
        match lock {
            Some(Axis::X) => ((raw.0, 0.0), Some(Axis::Y)),
            Some(Axis::Y) => ((0.0, raw.1), Some(Axis::X)),
            None => (raw, None),
        }
    }

    /// For a move: by how much the shapes go, where the drag lands.
    pub fn move_delta(&self) -> P {
        if self.landing.worked_out() { self.landing.delta } else { self.raw_delta().0 }
    }

    /// For a resize: the box the handle's drag makes before any snap, in the shape's own frame (lower left, upper right); with `keep` in proportion to the box now. The old
    /// box is (0, 0) to the size.
    pub fn raw_frame_keeping(&self, handle: Handle, basis: Basis, keep: bool) -> (P, P) {
        let (lo, hi) = basis.local_box();
        if keep {
            geom::resized_in_proportion(lo, hi, handle, basis.to_local(self.now), LEAST_SIZE)
        } else {
            geom::resized(lo, hi, handle, basis.to_local(self.now), false, LEAST_SIZE)
        }
    }

    /// For a resize: the box the shape lands in, in its own frame. A drag whose landing is not worked out yet is the box the handle makes, in proportion where it is kept
    /// (a corner always, a side with Shift). The text block's resize makes its own box (`geom::resized`, Shift keeping the proportions at a corner).
    pub fn new_frame(&self, handle: Handle, basis: Basis) -> (P, P) {
        self.landing
            .frame
            .filter(|_| self.landing.worked_out())
            .unwrap_or_else(|| self.raw_frame_keeping(handle, basis, handle.is_corner() || self.shift))
    }

    /// For a resize of several shapes: where the pointer puts the edges of their joint box that the handle moves. The handle is on the box that is drawn, which stands `margin`
    /// inches off the joint box on every side, so the pointer is that far outside the edge it moves.
    pub fn joint_pointer(&self, handle: Handle, margin: f64) -> P {
        let (ex, ey) = handle.edges();
        (self.now.0 - f64::from(ex) * margin, self.now.1 - f64::from(ey) * margin)
    }

    /// For a resize of several shapes: the joint box the handle's drag makes before any snap, lower left and upper right on the page; with `keep` in proportion to the box now.
    pub fn raw_joint_keeping(&self, handle: Handle, basis: Basis, margin: f64, keep: bool) -> (P, P) {
        let (lo, hi) = (basis.origin, (basis.origin.0 + basis.w, basis.origin.1 + basis.h));
        let to = self.joint_pointer(handle, margin);
        if keep { geom::resized_in_proportion(lo, hi, handle, to, LEAST_SIZE) } else { geom::resized(lo, hi, handle, to, false, LEAST_SIZE) }
    }

    /// For a resize of several shapes: the joint box they land in, on the page. A drag whose landing is not worked out yet is the box the handle makes, in proportion where it
    /// is kept (a corner always, a side with Shift).
    pub fn new_joint(&self, handle: Handle, basis: Basis, margin: f64) -> (P, P) {
        self.landing
            .frame
            .filter(|_| self.landing.worked_out())
            .unwrap_or_else(|| self.raw_joint_keeping(handle, basis, margin, handle.is_corner() || self.shift))
    }

    /// For a turn: the angle the pointer gives, in degrees: the angle the shape had when the drag began, plus how far the pointer has gone round the pin from where it was
    /// grabbed. The shape comes to this angle's step ([`Gesture::turn_to`]) or to another shape's angle (`snap_turn`).
    pub fn turn_raw(&self, pin: P, grab: P, start: f64) -> f64 {
        let (before, now) = ((grab.1 - pin.1).atan2(grab.0 - pin.0), (self.now.1 - pin.1).atan2(self.now.0 - pin.0));
        start + (now - before).to_degrees()
    }

    /// For a turn: the angle the shape is turned to, in degrees kept from just over -180 up to 180. The angle the engine's `snap_turn` gave, when the drag's landing has one;
    /// else the angle the pointer gives, snapped to the step its distance from the pin allows, which is where a drag goes under Alt and for the text block.
    pub fn turn_to(&self, cx: &Cx, pin: P, grab: P, start: f64) -> f64 {
        if let Some(turning) = self.landing.turn.filter(|_| self.landing.worked_out()) {
            return turning.to;
        }
        stepped(self.turn_raw(pin, grab, start), geom::distance(self.now, pin) * cx.view.scale())
    }
}

/// The distance `raw` held to the page axis `lock` keeps a move to: the other axis does not move at all.
fn held_to(raw: P, lock: Option<Axis>) -> P {
    match lock {
        Some(Axis::X) => (raw.0, 0.0),
        Some(Axis::Y) => (0.0, raw.1),
        None => raw,
    }
}

/// The angle `raw` degrees brought to the step of the rotation handle `points_from_pin` screen points from the pin, in degrees kept from just over -180 up to 180.
fn stepped(raw: f64, points_from_pin: f64) -> f64 {
    let step = rotation_step(points_from_pin);
    geom::normal_degrees(((raw / step).round() * step * 1.0e6).round() / 1.0e6)
}

/// The cursor for a handle of a shape laid on the page as `basis` lays it: the one of the four whose line the handle's direction from the middle
/// of the shape is nearest.
pub(super) fn resize_cursor(basis: &Basis, handle: Handle) -> CursorIcon {
    let (lo, hi) = basis.local_box();
    let (at, middle) = (basis.to_page(handle.at(lo, hi)), basis.to_page(((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0)));
    let degrees = (at.1 - middle.1).atan2(at.0 - middle.0).to_degrees().rem_euclid(180.0);
    // The page's y is up, so a direction up and to the right is the diagonal that runs from the lower left to the upper right on the screen.
    match ((degrees + 22.5) / 45.0).floor() as i32 % 4 {
        1 => CursorIcon::ResizeNeSw,
        2 => CursorIcon::ResizeVertical,
        3 => CursorIcon::ResizeNwSe,
        _ => CursorIcon::ResizeHorizontal,
    }
}

/// The cursor for a handle of a box that is square to the page (the combined box of several shapes): the two-headed arrow along the handle's own direction, diagonal at a corner
/// whatever the box's proportions (a turned shape's [`resize_cursor`] is the line its handle's direction from the middle is nearest).
pub(super) fn page_cursor(handle: Handle) -> CursorIcon {
    match handle {
        Handle::NorthWest | Handle::SouthEast => CursorIcon::ResizeNwSe,
        Handle::NorthEast | Handle::SouthWest => CursorIcon::ResizeNeSw,
        Handle::North | Handle::South => CursorIcon::ResizeVertical,
        Handle::East | Handle::West => CursorIcon::ResizeHorizontal,
    }
}

/// The handle of a 2-D shape's box that `p` is on, if any: one that is shown (`geom::visible_handles`). The box is the shape's own, where its
/// turn and its flips put it.
pub(super) fn handle_at(basis: &Basis, p: P, cx: &Cx) -> Option<Handle> {
    handle_at_sized(basis, p, cx, super::paint::HANDLE)
}

/// [`handle_at`] for handles `size` points across: the handles of a text block are small ones and a shape's are the selection look's (`theme::select::HANDLE`), and the
/// ones shown are the ones that fit.
pub(super) fn handle_at_sized(basis: &Basis, p: P, cx: &Cx, size: f32) -> Option<Handle> {
    let reach = cx.reach(HANDLE_REACH);
    let (lo, hi) = basis.local_box();
    geom::visible_handles(lo, hi, cx.view.scale(), f64::from(size))
        .into_iter()
        .map(|h| (geom::distance(basis.to_page(h.at(lo, hi)), p), h))
        .filter(|(d, _)| *d <= reach)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, h)| h)
}

/// How far, in screen points, the rotation handle of a text block is above the top edge of the block. The rotation handle of a shape is the selection look's
/// (`theme::select::ROTATE_CENTRE`).
pub(super) const ROTATION_ARM: f32 = 24.0;

/// Where the rotation handle of a shape is on the page, and where its stem leaves the shape: above the middle of the top edge as the shape
/// stands when it is turned and not flipped (the direction of the shape's angle, so the handle does not jump to the bottom when the shape is
/// flipped). `None` for a shape that has no rotation handle.
pub(super) fn rotation_handle(item: &Item, cx: &Cx) -> Option<(P, P)> {
    if !item.rotatable() {
        return None;
    }
    let basis = item.basis()?;
    let radians = item.degrees.to_radians();
    let up = (-radians.sin(), radians.cos());
    let middle = basis.to_page((basis.w / 2.0, basis.h / 2.0));
    let top = (middle.0 + up.0 * basis.h / 2.0, middle.1 + up.1 * basis.h / 2.0);
    let arm = cx.reach(crate::theme::select::ROTATE_CENTRE);
    Some((top, (top.0 + up.0 * arm, top.1 + up.1 * arm)))
}

/// The end of a connector that `p` is on, if any.
fn end_at(item: &Item, p: P, reach: f64) -> Option<Which> {
    let (begin, end) = item.ends?;
    let (db, de) = (geom::distance(begin, p), geom::distance(end, p));
    match (db <= reach, de <= reach) {
        (true, true) => Some(if db <= de { Which::Begin } else { Which::End }),
        (true, false) => Some(Which::Begin),
        (false, true) => Some(Which::End),
        (false, false) => None,
    }
}

impl DiacraftApp {
    /// The cursor for the handle of the selected shape under `p`, if there is one.
    pub(super) fn handle_cursor(&self, cx: &Cx, p: P) -> Option<CursorIcon> {
        let Some(item) = self.single() else { return self.combined_cursor(cx, p) };
        let reach = cx.reach(HANDLE_REACH);
        if let Some((_, handle)) = rotation_handle(item, cx)
            && geom::distance(handle, p) <= reach
        {
            return Some(CursorIcon::Grab);
        }
        match item.kind {
            Kind::Shape | Kind::Group if item.resizable() => {
                let basis = item.basis()?;
                handle_at_sized(&basis, p, cx, crate::theme::select::HANDLE).map(|handle| resize_cursor(&basis, handle))
            }
            Kind::Connector => end_at(item, p, reach).map(|_| CursorIcon::Crosshair).or_else(|| self.leg_cursor(cx, item.id, p)),
            _ => None,
        }
    }

    /// What a connector end dropped at `p` would be glued to: the connection point within reach, else the shape under it, else nothing.
    ///
    /// **The document's Glue settings decide, as they do for the engine's `snap`**: with the Glue switch off nothing is glued (the
    /// end is free, and is snapped as a free end is); a connection point is taken only with glue to connection points on (`glue_connection_points`, flag
    /// 8 of `GlueSettings`, the option `snap` glues a free end to a point by); and **the whole of a shape only with the same option on**, since the engine
    /// puts an end glued to the whole of a shape at that shape's connection point nearest the other end, and glues a shape that has none as a whole,
    /// at its box (ours, no specification says which option governs glue to the whole of a shape).
    pub(super) fn target_at(&self, cx: &Cx, p: P) -> Target {
        if !self.snap_settings_now().is_some_and(|s| s.glue && s.glue_connection_points) {
            return Target::Free(p);
        }
        self.target_near(cx, p, None)
    }

    /// What a connector end let go at `p` is near: the connection point within reach, else the shape under it, else nothing (`target_at` without the Glue switches). A shape
    /// named in `except` is never the answer (the shape an AutoConnect drag leaves), and nor is one whose layers allow no glue: the end is free there.
    pub(super) fn target_near(&self, cx: &Cx, p: P, except: Option<u32>) -> Target {
        let (Some(scene), Some(doc)) = (&self.ui.edit.scene, self.session.document()) else { return Target::Free(p) };
        let reach = cx.reach(POINT_REACH);
        let mut best: Option<(f64, Target)> = None;
        let takes = |i: &&Item| i.kind != Kind::Connector && Some(i.id) != except && !scene.standing(i.id).no_glue;
        for item in scene.items.iter().rev().filter(takes) {
            let (lo, hi) = item.bounds;
            if p.0 < lo.0 - reach || p.0 > hi.0 + reach || p.1 < lo.1 - reach || p.1 > hi.1 + reach {
                continue;
            }
            if let Some(point) = scene.nearest_point(doc, item.id, p, reach) {
                let d = geom::distance(point.at, p);
                if best.is_none_or(|(least, _)| d < least) {
                    best = Some((d, Target::Point { id: item.id, index: point.index, at: point.at }));
                }
            }
        }
        if let Some((_, target)) = best {
            return target;
        }
        let under = scene.hit_where(p, cx.reach(HIT_REACH), |i| takes(&i));
        under.map_or(Target::Free(p), |item| Target::Shape(item.id))
    }

    /// A press on the drawing area: it starts the drag that fits what is under the pointer.
    pub(super) fn begin_gesture(&mut self, cx: &Cx, at: Pos2, mods: Modifiers) {
        let p = cx.page_at(at);
        // A press on AutoConnect's flyout is the flyout's (its tiles take the click), and starts no gesture.
        if self.ui.edit.tool == Tool::Pointer && self.auto_in_flyout(at) {
            return;
        }
        // The Format Painter takes the press: it paints the shape under it, and starts no gesture (`painter.rs`).
        if self.ui.edit.painter.is_some() {
            self.paint_press(cx, p);
            return;
        }
        let additive = mods.shift || mods.command || mods.ctrl;
        // Ctrl is looked at **once, at the press**: a drag that began with it held copies, one that began without it moves, and the key
        // pressed or let go in the middle changes nothing.
        let copying = mods.command || mods.ctrl;
        let pressed = match self.ui.edit.tool {
            Tool::Connector => Some((self.press_with_connector(cx, p), OnClick::Nothing)),
            Tool::Text => Some((self.press_with_text(cx, p), OnClick::Nothing)),
            Tool::TextBlock => self.press_with_block(cx, p),
            Tool::Draw(tool) => Some((self.press_with_draw(cx, p, tool), OnClick::Nothing)),
            Tool::AreaSelect => Some((self.press_with_select(additive, false, p), OnClick::Nothing)),
            Tool::Lasso => Some((self.press_with_select(additive, true, p), OnClick::Nothing)),
            // An AutoConnect arrow that is showing takes the press before the shape and its handles do (its hit area is clear of the handles).
            Tool::Pointer => match self.press_on_arrow(cx, at) {
                Some(mode) => Some((mode, OnClick::Nothing)),
                None => self.press_with_pointer(cx, p, additive, copying),
            },
        };
        if let Some((mode, on_click)) = pressed {
            let target = match &mode {
                Mode::Join { from } => *from,
                _ => Target::Free(p),
            };
            self.ui.edit.gesture = Some(Gesture {
                mode,
                start: at,
                origin: p,
                now: p,
                active: false,
                target,
                shift: mods.shift,
                alt: mods.alt,
                on_click,
                landing: Landing::default(),
                subject: None,
            });
        }
    }

    /// A press on an AutoConnect arrow that shows: the drag it starts.
    fn press_on_arrow(&self, cx: &Cx, at: Pos2) -> Option<Mode> {
        let (from, side) = self.auto_arrow_at(cx, at)?;
        Some(Mode::Arrow { from, side, anchor: self.auto_anchor(from, side)? })
    }

    fn press_with_connector(&self, cx: &Cx, p: P) -> Mode {
        Mode::Join { from: self.target_at(cx, p) }
    }

    /// The Text tool's press: the shape under the pointer, if there is one.
    fn press_with_text(&self, cx: &Cx, p: P) -> Mode {
        Mode::TextBox { on: self.ui.edit.scene.as_ref().and_then(|s| s.hit(p, cx.reach(HIT_REACH))).map(|item| item.id) }
    }

    /// The Pointer Tool's press, and the member it would select if it is only a click. `None`: the press changed the selection and starts no drag
    /// (Shift or Ctrl on a selected shape).
    fn press_with_pointer(&mut self, cx: &Cx, p: P, additive: bool, copying: bool) -> Option<(Mode, OnClick)> {
        let reach = cx.reach(HANDLE_REACH);
        if let Some(item) = self.single() {
            // The yellow handle at the end of a callout's leader (`callout.rs`).
            if let Some(mode) = self.press_on_tip(cx, item.id, p) {
                return Some((mode, OnClick::Nothing));
            }
            match item.kind {
                Kind::Shape | Kind::Group if item.resizable() => {
                    if let Some(basis) = item.basis()
                        && let Some(handle) = handle_at_sized(&basis, p, cx, crate::theme::select::HANDLE)
                    {
                        return Some((Mode::Resize { id: item.id, handle, basis }, OnClick::Nothing));
                    }
                }
                Kind::Connector => {
                    if let Some(end) = end_at(item, p, reach) {
                        return Some((Mode::Reshape { id: item.id, end }, OnClick::Nothing));
                    }
                    // The handle at the middle of a leg (`legs.rs`).
                    if let Some(mode) = self.press_on_leg(cx, item.id, p) {
                        return Some((mode, OnClick::Nothing));
                    }
                }
                _ => {}
            }
            if let Some((_, handle)) = rotation_handle(item, cx)
                && geom::distance(handle, p) <= reach
            {
                return Some((
                    Mode::Rotate { id: item.id, pin: item.pin, grab: handle, start: geom::normal_degrees(item.degrees) },
                    OnClick::Nothing,
                ));
            }
        }
        // The handles round several shapes: a handle stretches them as one, the rotation handle turns them as one. A press on one that has nothing to start keeps
        // the selection and starts nothing.
        match self.press_combined(cx, p) {
            OnBox::Start(mode) => return Some((mode, OnClick::Nothing)),
            OnBox::Dead => return None,
            OnBox::Off => {}
        }
        let tolerance = cx.reach(HIT_REACH);
        let entering = if additive { Enter::No } else { self.member_under(p, tolerance) };
        let top = self.ui.edit.scene.as_ref().and_then(|s| s.hit(p, tolerance)).map(|item| item.id);
        // The shape the press is on: the member to select at once, or the group that is selected and moves (a member is chosen only by a click),
        // or what is under the pointer.
        let (pressed_on, on_click) = match entering {
            Enter::Now(member) => (Some(member), OnClick::Nothing),
            Enter::OnClick(member) => (self.single().map(|item| item.id), OnClick::Select(member)),
            Enter::No => (top, OnClick::Nothing),
        };
        let hit = pressed_on.and_then(|id| self.ui.edit.scene.as_ref().and_then(|s| s.get(id))).map(|item| item.id);
        match hit {
            Some(id) => {
                self.ui.edit.guide = None;
                let selected = self.ui.edit.selection.contains(&id);
                if copying {
                    // Ctrl on a shape: a drag copies what is selected (or this shape, when it is not), a click toggles it in the selection.
                    let ids = if selected { self.ui.edit.selection.clone() } else { vec![id] };
                    return Some((Mode::Move { ids, copy: true }, OnClick::Toggle(id)));
                }
                if additive && selected {
                    self.ui.edit.selection.retain(|s| *s != id);
                    return None;
                }
                if additive {
                    self.ui.edit.selection.push(id);
                } else if !selected {
                    self.ui.edit.selection = vec![id];
                }
                self.ui.edit.selection_page = Some(cx.page);
                Some((Mode::Move { ids: self.ui.edit.selection.clone(), copy: false }, on_click))
            }
            None => {
                // A guide under the pointer, where no shape is: it is selected, and a drag moves it.
                if !additive
                    && self.ensure_snap_page(cx.page)
                    && let Some(guide) = self.guide_hit(cx, p)
                {
                    let held = self.session.document().map(|doc| super::guides::held_by(doc, cx.page, guide.id)).unwrap_or_default();
                    self.select_guide(cx.page, guide.id);
                    return Some((Mode::Guide { id: guide.id, axis: guide.axis, at: guide.at, held }, OnClick::Nothing));
                }
                self.ui.edit.guide = None;
                let base = if additive { self.ui.edit.selection.clone() } else { Vec::new() };
                if !additive {
                    self.ui.edit.selection.clear();
                }
                Some((Mode::Marquee { additive, base }, OnClick::Nothing))
            }
        }
    }

    /// What a press at `p` does about the members of the group under it (gap U9; "first click
    /// takes the group and second click the member, with Behavior able to set group only, group first or members first").
    ///
    /// * **A second click**: the one shape selected is the group under the pointer, or a member of it, and the pointer is on a member of that group,
    ///   one level down. For a selected **group** the press starts the move of the group and the member is chosen by the click ([`Enter::OnClick`]);
    ///   for a selected **member** the sibling is selected at once, as the person is working inside the group.
    /// * **Members first** (`SelectMode` 2): the first click on such a group selects the member under the pointer, and goes down through the groups
    ///   in it that are of the same mode.
    /// * **Group only** (`SelectMode` 0, here or in any group above): the scene has no members, so there is nothing to enter.
    fn member_under(&self, p: P, tolerance: f64) -> Enter {
        let Some(scene) = self.ui.edit.scene.as_ref() else { return Enter::No };
        let Some(top) = scene.hit(p, tolerance).filter(|top| top.kind == Kind::Group) else { return Enter::No };
        if let Some(selected) = self.single() {
            let group = if selected.kind == Kind::Group { Some(selected.id) } else { selected.parent };
            if let Some(group) = group.filter(|group| scene.root_of(*group) == Some(top.id)) {
                return match scene.hit_child(group, p, tolerance) {
                    Some(member) if selected.kind == Kind::Group => Enter::OnClick(member.id),
                    Some(member) => Enter::Now(member.id),
                    None => Enter::No,
                };
            }
        }
        // Members first.
        let (mut found, mut at) = (None, top);
        for _ in 0..=super::scene::MAX_GROUP_DEPTH {
            if at.select_mode != 2 {
                break;
            }
            match scene.hit_child(at.id, p, tolerance) {
                Some(member) => {
                    found = Some(member.id);
                    at = member;
                }
                None => break,
            }
        }
        found.map_or(Enter::No, Enter::Now)
    }

    /// The pointer moved with the button down.
    pub(super) fn update_gesture(&mut self, cx: &Cx, at: Pos2, mods: Modifiers) {
        let now = cx.page_at(at);
        let target = match self.ui.edit.gesture.as_ref().map(|g| &g.mode) {
            Some(Mode::Join { .. } | Mode::Reshape { .. }) => Some(self.target_at(cx, now)),
            // A drag from an arrow joins what it is let go on, whatever the Glue switch says: it is a command of its own.
            Some(Mode::Arrow { from, .. }) => Some(self.target_near(cx, now, Some(*from))),
            // The end of a callout's leader is dropped on the shape it is let go on, the callout itself never.
            Some(Mode::Tip { id }) => Some(self.target_near(cx, now, Some(*id))),
            _ => None,
        };
        let Some(gesture) = self.ui.edit.gesture.as_mut() else { return };
        gesture.now = now;
        gesture.shift = mods.shift;
        gesture.alt = mods.alt;
        if !gesture.active && (at - gesture.start).length() >= DRAG_START {
            gesture.active = true;
        }
        if let Some(target) = target {
            gesture.target = target;
        }
        self.lasso_follow(cx, now);
        self.draw_follow(cx, now);
        self.land(cx);
    }

    /// Works out where the drag lands, if the pointer, the keys or the document changed since it last was.
    fn land(&mut self, cx: &Cx) {
        let Some(mut gesture) = self.ui.edit.gesture.take() else { return };
        let key = LandKey {
            now: gesture.now,
            shift: gesture.shift,
            alt: gesture.alt,
            revision: self.session.revision(),
            scale: cx.view.scale(),
            view: view_of(cx),
            reads: self.ui.edit.reads_done,
        };
        if gesture.active && gesture.landing.key != Some(key) {
            gesture.landing = self.landing_of(cx, &mut gesture);
            gesture.landing.key = Some(key);
        }
        self.ui.edit.gesture = Some(gesture);
    }

    /// The shapes of `ids` that a move moves: not a connector with both ends glued, which stays with its shapes.
    fn moving_of(&self, ids: &[u32]) -> Vec<u32> {
        match &self.ui.edit.scene {
            Some(scene) => ids.iter().copied().filter(|id| scene.get(*id).is_some_and(Item::moves_by_itself)).collect(),
            None => Vec::new(),
        }
    }

    /// Where the drag `g` lands (`snapping.rs`).
    fn landing_of(&mut self, cx: &Cx, g: &mut Gesture) -> Landing {
        let mut out = Landing { snap: !g.alt, ..Landing::default() };
        if let Mode::Move { ids, copy } = &g.mode {
            let (ids, copy) = (ids.clone(), *copy);
            self.land_move(cx, g, &ids, copy, &mut out);
            return out;
        }
        let g: &Gesture = g;
        match &g.mode {
            Mode::Move { .. } => {}
            Mode::Resize { id, handle, basis } => self.land_resize(cx, g, (*id, *handle, *basis), &mut out),
            Mode::Rotate { id, pin, grab, start } => self.land_rotate(cx, g, *id, (*pin, *grab, *start), &mut out),
            Mode::ResizeSet { ids, handle, basis, margin } => self.land_resize_set(cx, g, (ids, *handle, *basis, *margin), &mut out),
            Mode::RotateSet { pin, grab, .. } => Self::land_rotate_set(cx, g, (*pin, *grab), &mut out),
            Mode::Join { from } => {
                let mut snap = out.snap;
                out.start = Some(self.land_end(cx, *from, &[], g.alt, &mut Vec::new(), &mut snap));
                let mut marks = Vec::new();
                out.end = Some(self.land_end(cx, g.target, &[], g.alt, &mut marks, &mut snap));
                (out.marks, out.snap) = (marks, snap);
            }
            // The end is where the pointer is, joined to what is under it: the command takes no `snap`.
            Mode::Arrow { .. } | Mode::Tip { .. } => {
                out.end = Some(g.target);
                out.snap = false;
            }
            Mode::Reshape { id, .. } => {
                let (mut marks, mut snap) = (Vec::new(), out.snap);
                out.end = Some(self.land_end(cx, g.target, &[*id], g.alt, &mut marks, &mut snap));
                (out.marks, out.snap) = (marks, snap);
            }
            Mode::Leg { id, leg, level, line } => self.land_leg(cx, g, (*id, *leg, *level, *line), &mut out),
            // The box is dragged out from where the press landed (a place), and its moved sides snap as a resize's do, to the size of a shape in the view too.
            Mode::TextBox { on: None } => {
                let first = self.land_place(cx, g.origin, g.alt, &[], &mut Vec::new());
                let now = (sent(g.now.0), sent(g.now.1));
                let landed = if g.alt { None } else { self.text_box_snap(cx.page, first, now, cx.view.scale(), view_of(cx)) };
                let (second, marks) = landed.unwrap_or((g.now, Vec::new()));
                out.corners = Some((first, second));
                out.marks = marks;
            }
            Mode::Guide { axis, at, .. } => {
                let along = |p: P| if *axis == Axis::X { p.0 } else { p.1 };
                out.guide_at = Some(sent(at + along(g.now) - along(g.origin)));
                out.snap = false;
            }
            // The two ends of a drag with the Rectangle, Ellipse, Line or Arc tool: the start where the engine's snap puts a place (the vertex a path goes on from is not moved), and
            // the end too unless Shift holds it to a square, a circle or a step of 45 degrees, which the grid would undo. A stroke tool reads the stroke, not a landing.
            Mode::Draw { tool, stroke, onto }
                if !matches!(tool, diacraft_engine::draw::pen::Tool::Freeform | diacraft_engine::draw::pen::Tool::Pencil) =>
            {
                let mut marks = Vec::new();
                let first = stroke.first().copied().unwrap_or(g.origin);
                let first = if onto.is_some() { first } else { self.land_place(cx, first, g.alt, &[], &mut marks) };
                let second = if g.shift { g.now } else { self.land_place(cx, g.now, g.alt, &[], &mut marks) };
                out.corners = Some((first, second));
                out.marks = marks;
                out.snap = false;
            }
            // The text block's commands take no `snap`: its middle (a move) or the handle (a resize) is snapped as a place, its own shape no target, and
            // the result is sent.
            Mode::BlockMove { id, centre } => {
                let ((dx, dy), held) = g.raw_delta();
                let mut marks = Vec::new();
                let landed = self.land_place(cx, (centre.0 + dx, centre.1 + dy), g.alt, &[*id], &mut marks);
                // An axis Shift holds still stays still.
                out.delta = (
                    if held == Some(Axis::X) { 0.0 } else { sent(landed.0 - centre.0) },
                    if held == Some(Axis::Y) { 0.0 } else { sent(landed.1 - centre.1) },
                );
                out.marks = marks;
                out.snap = false;
            }
            Mode::BlockResize { id, handle, basis } => {
                let mut marks = Vec::new();
                let to = self.land_place(cx, g.now, g.alt, &[*id], &mut marks);
                let (lo, hi) = basis.local_box();
                out.frame = Some(geom::resized(lo, hi, *handle, basis.to_local(to), g.shift, LEAST_SIZE));
                out.marks = marks;
                out.snap = false;
            }
            _ => {}
        }
        out
    }

    /// A move's landing: the box of the shapes that move, where the drag puts them, with their own connection points and, for one turned shape, its own frame, as
    /// `shape.move` snaps them. The box, the points and the frame are read once for the drag ([`MoveSubject`], again only if the document changed), and not under Alt or
    /// for more shapes than `snap` takes. **Shift is the engine's `lock`**: the move keeps to the page axis the whole drag has gone farther along, and
    /// the free axis snaps. While the page is not ready the drag goes as the pointer made it (held to its axis), and the release is sent without `snap`.
    fn land_move(&mut self, cx: &Cx, g: &mut Gesture, ids: &[u32], copy: bool, out: &mut Landing) {
        let (raw, lock) = g.raw_move();
        let held = held_to(raw, lock);
        (out.raw, out.lock, out.delta) = (raw, lock, held);
        if !out.snap {
            return;
        }
        let key = (self.ui.generation, self.session.revision());
        if g.subject.as_ref().is_none_or(|s| s.key != key) {
            let moving = if copy { ids.to_vec() } else { self.moving_of(ids) };
            g.subject = Some(self.move_subject(cx.page, moving, copy));
        }
        let Some(subject) = g.subject.as_ref() else { return };
        if subject.ids.len() > diacraft_engine::snap::MAX_SNAPPED_SHAPES {
            out.snap = false;
            return;
        }
        let Some(now) = subject.now else { return };
        // The box where the held distance puts it, as `shape.move` makes it from the distance it is sent.
        let moved = Rect { x0: now.x0 + held.0, y0: now.y0 + held.1, x1: now.x1 + held.0, y1: now.y1 + held.1 };
        // The shapes' own connection points and the shape's own frame where the move puts them: the same shift, to the last bit.
        let own: Vec<P> = subject.own.iter().map(|p| (p.0 + held.0, p.1 + held.1)).collect();
        let frame = subject.frame.map(|f| OwnFrame { centre: (f.centre.0 + held.0, f.centre.1 + held.1), ..f });
        // A copy's originals stay where they are, and are what it lines up with: they are targets, so no shapes are named as moving.
        let query_ids: Vec<u32> = if copy { Vec::new() } else { subject.ids.clone() };
        let extras = Extras { lock, view: Some(view_of(cx)), frame, ..Extras::default() };
        let Some(snapped) = self.snap_box(cx.page, moved, &query_ids, &own, cx.view.scale(), extras) else {
            out.snap = false;
            return;
        };
        out.delta = (held.0 + snapped.dx, held.1 + snapped.dy);
        out.marks = snapped.marks;
    }

    /// A resize's landing: the box the handle makes, its moved edges snapped as `shape.resize` snaps them (`snap_resize_ext`: a shape turned or flipped along its own
    /// axes, a width or height stopping at that of a shape in the view). **A corner keeps the proportions with no key and a side with Shift**: the
    /// engine works the kept box out, from the box the handle made, and snaps inside the proportions; where no snap is sent (Alt, the Snap switch off, a shape the engine
    /// does not snap, a page not ready yet) the window keeps them itself and sends the box it made.
    fn land_resize(&mut self, cx: &Cx, g: &Gesture, (id, handle, basis): (u32, Handle, Basis), out: &mut Landing) {
        let keep = handle.is_corner() || g.shift;
        out.keep = keep;
        let snappable = self.session.document().is_some_and(|doc| is_square(doc, cx.page, id) || snaps_in_its_own_axes(doc, cx.page, id));
        let engine = out.snap && snappable && self.snap_settings_now().is_some_and(|s| s.snap);
        out.snap = engine;
        let sized_of = |(lo, hi): (P, P)| {
            let corner = basis.to_page(lo);
            (sent(corner.0), sent(corner.1), sent(hi.0 - lo.0), sent(hi.1 - lo.1))
        };
        // The window's own box is in proportion where they are kept; the engine is sent the box as the handle made it and keeps them itself.
        let own = g.raw_frame_keeping(handle, basis, keep);
        let asked = if engine { g.raw_frame_keeping(handle, basis, false) } else { own };
        let raw = sized_of(asked);
        (out.raw_sized, out.sized, out.frame) = (Some(raw), Some(raw), Some(asked));
        if !engine {
            return;
        }
        let extras = Extras { view: Some(view_of(cx)), keep, ..Extras::default() };
        match self.resize_snap(cx.page, id, raw, cx.view.scale(), extras) {
            // Not ready: the window keeps the proportions itself, and the release is what it shows.
            None => {
                out.snap = false;
                let sized = sized_of(own);
                (out.raw_sized, out.sized, out.frame) = (Some(sized), Some(sized), Some(own));
            }
            // No edge moves, or the snap would collapse the box: as dragged, which is what the command does too.
            Some(None) => {}
            Some(Some((snapped, sized))) => {
                out.sized = Some(sized);
                let low = basis.to_local((sized.0, sized.1));
                out.frame = Some((low, (low.0 + sized.2, low.1 + sized.3)));
                out.marks = snapped.marks;
            }
        }
    }

    /// The landing of a resize of several shapes by a handle of their combined box: the joint box the handle makes, its moved edges snapped as `shape.resize` with `ids` snaps
    /// them (`snap_resize_set`: the box of a selection is square to the page, whatever its shapes are). **A corner keeps the proportions of the joint box with no key and a side
    /// with Shift**, as a shape's handles do: the engine works the kept box out of the box the handle made; where no snap is sent (Alt, the Snap switch off, a page not ready
    /// yet) the window keeps them itself and sends the box it made.
    fn land_resize_set(&mut self, cx: &Cx, g: &Gesture, (ids, handle, basis, margin): (&[u32], Handle, Basis, f64), out: &mut Landing) {
        let keep = handle.is_corner() || g.shift;
        out.keep = keep;
        let engine = out.snap && ids.len() <= diacraft_engine::snap::MAX_SNAPPED_SHAPES && self.snap_settings_now().is_some_and(|s| s.snap);
        out.snap = engine;
        let sized_of = |(lo, hi): (P, P)| (sent(lo.0), sent(lo.1), sent(hi.0 - lo.0), sent(hi.1 - lo.1));
        // The window's own box is in proportion where they are kept; the engine is sent the box as the handle made it and keeps them itself.
        let own = g.raw_joint_keeping(handle, basis, margin, keep);
        let asked = if engine { g.raw_joint_keeping(handle, basis, margin, false) } else { own };
        let raw = sized_of(asked);
        (out.raw_sized, out.sized, out.frame) = (Some(raw), Some(raw), Some(asked));
        if !engine {
            return;
        }
        let extras = Extras { view: Some(view_of(cx)), keep, ..Extras::default() };
        match self.resize_snap_set(cx.page, ids, raw, cx.view.scale(), extras) {
            // Not ready: the window keeps the proportions itself, and the release is what it shows.
            None => {
                out.snap = false;
                let sized = sized_of(own);
                (out.raw_sized, out.sized, out.frame) = (Some(sized), Some(sized), Some(own));
            }
            // No edge moves, or the snap would collapse the box: as dragged, which is what the command does too.
            Some(None) => {}
            Some(Some((snapped, sized))) => {
                out.sized = Some(sized);
                out.frame = Some(((sized.0, sized.1), (sized.0 + sized.2, sized.1 + sized.3)));
                out.marks = snapped.marks;
            }
        }
    }

    /// The landing of a turn of several shapes by the rotation handle: how far the pointer has gone round the middle of their joint box (`raw`, degrees, as sent) and the
    /// turn it comes to, in the step the distance from the pin gives, kept from just over -180 up to 180. **The engine's `snap_turn` is for the angle of one shape,
    /// so the release is sent with no `snap`**: the turn is the step the preview showed, and the shapes keep the angles they differ by.
    fn land_rotate_set(cx: &Cx, g: &Gesture, (pin, grab): (P, P), out: &mut Landing) {
        out.snap = false;
        let raw = sent(g.turn_raw(pin, grab, 0.0));
        let radius = sent(geom::distance(g.now, pin));
        out.turn = Some(Turning { raw, radius, to: stepped(raw, radius * cx.view.scale()) });
    }

    /// A turn's landing: the angle the pointer gives (as it is sent) and the angle the shape comes to, which is `snap_turn`'s: another shape's angle within reach, else the
    /// step the distance from the pin gives. Under Alt, and while the page is not ready, the step alone, and the release is sent without `snap`.
    fn land_rotate(&mut self, cx: &Cx, g: &Gesture, id: u32, (pin, grab, start): (P, P, f64), out: &mut Landing) {
        let raw = sent(g.turn_raw(pin, grab, start));
        let radius = sent(geom::distance(g.now, pin));
        let zoom = cx.view.scale();
        let mut to = stepped(raw, radius * zoom);
        if out.snap {
            match self.turn_snap(cx.page, id, raw, radius, zoom) {
                Some(snapped) => {
                    to = geom::normal_degrees(snapped.turned.map_or(raw, f64::to_degrees));
                    out.marks = snapped.marks;
                }
                None => out.snap = false,
            }
        }
        out.turn = Some(Turning { raw, radius, to });
    }

    /// Where a connector's end `target` lands: a free end is snapped as the end of a connector (`Use::Connect`, the shapes `ids` never targets),
    /// and becomes the glue to the connection point it lands on; an end glued to a shape stays so. Alt held leaves it where it is. `snap` is made false when the page
    /// is not ready to snap it yet: the end is then where the pointer is, and the release is sent without `snap`.
    fn land_end(&mut self, cx: &Cx, target: Target, ids: &[u32], alt: bool, marks: &mut Vec<Mark>, snap: &mut bool) -> Target {
        let Target::Free(p) = target else { return target };
        if alt {
            return target;
        }
        match self.snap_place(cx.page, (sent(p.0), sent(p.1)), Use::Connect, ids, cx.view.scale(), view_of(cx)) {
            Some(snapped) => {
                marks.extend(snapped.marks);
                match snapped.glued {
                    Some((id, index)) => Target::Point { id, index, at: snapped.at },
                    None => Target::Free(snapped.at),
                }
            }
            None => {
                *snap = false;
                target
            }
        }
    }

    /// Where a place put down lands (`Use::Place`, as a drawing tool's point; the shapes `ids` are no targets); Alt held leaves it where it is, and so does a page that is
    /// not ready yet.
    pub(super) fn land_place(&mut self, cx: &Cx, p: P, alt: bool, ids: &[u32], marks: &mut Vec<Mark>) -> P {
        if alt {
            return p;
        }
        match self.snap_place(cx.page, (sent(p.0), sent(p.1)), Use::Place, ids, cx.view.scale(), view_of(cx)) {
            Some(snapped) => {
                marks.extend(snapped.marks);
                snapped.at
            }
            None => p,
        }
    }

    /// The `snap` a command of the drag is sent with: the view's zoom and the visible part of the page, so that the engine snaps as the preview did.
    pub(super) fn snap_param(cx: &Cx) -> Value {
        json!({"zoom": cx.view.scale(), "view": view_of(cx)})
    }

    /// Keeps the `snap` of a command's answer, for `ui.inspect` and the tests: what the release snapped to and the shapes it held to guides.
    pub(super) fn noted_snap(&mut self, answer: &Value) {
        self.ui.edit.last_snap = answer.get("snap").cloned();
    }

    /// The button is coming up on the second click of a double click: **it does not go in to the member under the pointer** (`OnClick::Select`). A press on a
    /// selected group chooses the member only when the button comes up with no drag, so the double click that follows would find the member selected and edit it
    /// in place of the group that was double clicked. With the group still selected, the double click sends the text where the group's
    /// `IsTextEditTarget` says, while a member that was already selected by an earlier click is edited as itself (`edit_text_at`).
    pub(super) fn keep_group_for_double_click(&mut self) {
        if let Some(gesture) = self.ui.edit.gesture.as_mut()
            && matches!(gesture.on_click, OnClick::Select(_))
        {
            gesture.on_click = OnClick::Nothing;
        }
    }

    /// The button came up: the drag ends, and the one command it makes goes.
    pub(super) fn finish_gesture(&mut self, cx: &Cx, at: Pos2, mods: Modifiers) {
        self.update_gesture(cx, at, mods);
        let Some(gesture) = self.ui.edit.gesture.take() else { return };
        // The Text tool's press ends in a text box that is open, whether it was a click or a drag.
        if let Mode::TextBox { on } = gesture.mode {
            self.finish_text_box(cx, &gesture, on);
            return;
        }
        // A press on an AutoConnect arrow ends in one `connector.autoconnect`, a click or a drag.
        if let Mode::Arrow { from, side, .. } = gesture.mode {
            self.finish_arrow(cx, &gesture, from, side);
            return;
        }
        if !gesture.active {
            match gesture.on_click {
                // A click on a member of the selected group: the member is selected now (a drag would have moved the group).
                OnClick::Select(member) => self.set_selection(cx.page, vec![member]),
                // A Ctrl-click: with no shape selected, on a shape that has a hyperlink, it follows the link; else the shape joins the selection, or leaves it.
                OnClick::Toggle(id) => {
                    if self.ui.edit.selection.is_empty() && self.follow_link_of(cx.page, id) {
                        return;
                    }
                    let mut now = self.ui.edit.selection.clone();
                    if let Some(at) = now.iter().position(|s| *s == id) {
                        now.remove(at);
                    } else {
                        now.push(id);
                    }
                    self.set_selection(cx.page, now);
                }
                OnClick::Nothing => {}
            }
            return;
        }
        match gesture.mode.clone() {
            Mode::Marquee { additive, base } => self.finish_marquee(cx, &gesture, additive, base),
            Mode::Lasso { base, trail } => self.finish_lasso(cx, base, trail),
            Mode::Move { ids, copy, .. } => self.finish_move(cx, &gesture, &ids, copy),
            Mode::Resize { id, basis, .. } => self.finish_resize(cx, &gesture, id, basis),
            Mode::Rotate { id, pin, grab, start } => self.finish_rotate(cx, &gesture, id, (pin, grab, start)),
            Mode::ResizeSet { ids, basis, .. } => self.finish_resize_set(cx, &gesture, &ids, basis),
            Mode::RotateSet { ids, pin, .. } => self.finish_rotate_set(cx, &gesture, &ids, pin),
            Mode::Join { from } => self.finish_join(cx, &gesture, from),
            Mode::Arrow { .. } => {}
            Mode::Reshape { id, end } => self.finish_reshape(cx, &gesture, id, end),
            Mode::Tip { id } => self.finish_tip(cx, &gesture, id),
            Mode::Leg { id, leg, line, .. } => self.finish_leg(cx, &gesture, (id, leg, line)),
            Mode::BlockMove { id, .. } => self.finish_block_move(cx, &gesture, id),
            Mode::BlockResize { id, handle, basis } => self.finish_block_resize(cx, &gesture, id, handle, basis),
            Mode::BlockRotate { id, pin, grab, start } => self.finish_block_rotate(cx, &gesture, id, (pin, grab, start)),
            Mode::Guide { id, at, held, .. } => {
                if let Some(to) = gesture.landing.guide_at {
                    self.finish_guide_move(cx.page, (id, at, to), &held);
                }
            }
            Mode::Draw { .. } => self.finish_draw(cx, &gesture),
            Mode::TextBox { .. } => {}
        }
    }

    /// Escape in the middle of a drag.
    pub(super) fn cancel_gesture(&mut self) -> bool {
        self.ui.edit.gesture.take().is_some()
    }

    /// The button came up on a net: the shapes the engine says it takes (`select.by_region`, `region.rs`) are selected, after those kept by Shift or Ctrl.
    fn finish_marquee(&mut self, cx: &Cx, gesture: &Gesture, additive: bool, base: Vec<u32>) {
        let (lo, hi) = geom::ordered(gesture.origin, gesture.now);
        let base = if additive { base } else { Vec::new() };
        self.select_region(cx.page, &[lo, (hi.0, lo.1), hi, (lo.0, hi.1)], base);
        self.use_up_select_tool();
    }

    fn finish_move(&mut self, cx: &Cx, gesture: &Gesture, ids: &[u32], copy: bool) {
        let landing = &gesture.landing;
        let (dx, dy) = gesture.move_delta();
        if copy {
            // A copy-drag: the originals stay and the copies are where the drag landed (`shape.duplicate` takes no `snap`, so the
            // distance sent is the one the snap gave); the copies are selected. A drag that ends where it began copies nothing.
            if dx.abs() < NO_CHANGE && dy.abs() < NO_CHANGE {
                return;
            }
            let params = json!({"ids": ids, "dx": inch(dx), "dy": inch(dy), "page": cx.page});
            if let Some(answer) = self.run_edit("shape.duplicate", &params) {
                let made: Vec<u32> = answer
                    .get("ids")
                    .and_then(serde_json::Value::as_array)
                    .map(|l| l.iter().filter_map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok())).collect())
                    .unwrap_or_default();
                self.select(&made);
                self.keep_preview(cx.page, &made);
            }
            return;
        }
        // A connector with both ends glued stays with its shapes; sending it would change nothing.
        if self.ui.edit.scene.is_none() {
            return;
        }
        let moving = self.moving_of(ids);
        if moving.is_empty() || (dx.abs() < NO_CHANGE && dy.abs() < NO_CHANGE) {
            return;
        }
        // With `snap` the distance the pointer gave is sent, with the axis Shift keeps it to (`lock`), and the engine holds and snaps it as the preview did, and holds the
        // shapes to a guide it pulled them to; without, the distance the preview showed (held to its axis by the window).
        let params = if landing.snap && landing.worked_out() {
            let mut snap = Self::snap_param(cx);
            if let (Some(axis), Some(words)) = (landing.lock, snap.as_object_mut()) {
                words.insert("lock".to_owned(), json!(axis.name()));
            }
            json!({"ids": moving, "dx": inch(landing.raw.0), "dy": inch(landing.raw.1), "page": cx.page, "snap": snap})
        } else {
            json!({"ids": moving, "dx": inch(dx), "dy": inch(dy), "page": cx.page})
        };
        if let Some(answer) = self.run_edit("shape.move", &params) {
            self.noted_snap(&answer);
            self.keep_preview(cx.page, &moving);
        }
    }

    fn finish_resize(&mut self, cx: &Cx, gesture: &Gesture, id: u32, basis: Basis) {
        let landing = &gesture.landing;
        let (Some(raw), Some(sized)) = (landing.raw_sized, landing.sized) else { return };
        // A resize that lands where the shape is changes nothing and is not sent.
        let (old_lo, old_hi) = basis.local_box();
        let low = basis.to_local((sized.0, sized.1));
        let same_size = (sized.2 - (old_hi.0 - old_lo.0)).abs() < NO_CHANGE && (sized.3 - (old_hi.1 - old_lo.1)).abs() < NO_CHANGE;
        if geom::distance(low, old_lo) < NO_CHANGE && same_size {
            return;
        }
        // `x` and `y` are where the corner at (0, 0) of the shape's own frame goes on the page. With `snap` the box the handle made is sent
        // and the engine snaps its moved edges as the preview did; without, the box the preview showed.
        let (x, y, width, height) = if landing.snap { raw } else { sized };
        let mut params = json!({"id": id, "x": inch(x), "y": inch(y), "width": inch(width), "height": inch(height), "page": cx.page});
        if let (true, Some(map)) = (landing.snap, params.as_object_mut()) {
            // `keep` is the proportions a corner always keeps and a side with Shift: the engine works the kept box out of the box the handle made.
            let mut snap = Self::snap_param(cx);
            if let (true, Some(words)) = (landing.keep, snap.as_object_mut()) {
                words.insert("keep".to_owned(), json!("proportions"));
            }
            map.insert("snap".to_owned(), snap);
        }
        if let Some(answer) = self.run_edit("shape.resize", &params) {
            self.noted_snap(&answer);
            self.keep_preview(cx.page, &[id]);
        }
    }

    /// The rotation handle was let go: the shape is turned to the angle the drag made. With `snap` the angle the pointer gave is sent, with how far the pointer was from
    /// the pin, and the engine turns it to another shape's angle or to the step as the preview did; without (Alt, or a page that was not ready) the angle the preview showed.
    fn finish_rotate(&mut self, cx: &Cx, gesture: &Gesture, id: u32, (pin, grab, start): (P, P, f64)) {
        let to = gesture.turn_to(cx, pin, grab, start);
        if (to - start).abs() < NO_CHANGE {
            return;
        }
        let params = match gesture.landing.turn.filter(|_| gesture.landing.snap && gesture.landing.worked_out()) {
            Some(turning) => {
                let mut snap = Self::snap_param(cx);
                if let Some(words) = snap.as_object_mut() {
                    words.insert("radius".to_owned(), json!(turning.radius));
                }
                json!({"ids": [id], "angle": inch(turning.raw), "page": cx.page, "snap": snap})
            }
            None => json!({"ids": [id], "angle": to, "page": cx.page}),
        };
        if let Some(answer) = self.run_edit("shape.rotate", &params) {
            self.noted_snap(&answer);
            self.keep_preview(cx.page, &[id]);
        }
    }

    /// The handle of the combined box of several shapes was let go: the shapes are stretched to the joint box the drag made (`shape.resize` with `ids`). With `snap` the box the
    /// handle made is sent and the engine snaps its moved edges as the preview did, keeping the proportions where a corner (or Shift on a side) asks; without, the box the
    /// preview showed. A drag that lands on the box the shapes have is not sent.
    fn finish_resize_set(&mut self, cx: &Cx, gesture: &Gesture, ids: &[u32], basis: Basis) {
        let landing = &gesture.landing;
        let (Some(raw), Some(sized)) = (landing.raw_sized, landing.sized) else { return };
        let same_size = (sized.2 - basis.w).abs() < NO_CHANGE && (sized.3 - basis.h).abs() < NO_CHANGE;
        if geom::distance((sized.0, sized.1), basis.origin) < NO_CHANGE && same_size {
            return;
        }
        let (x, y, width, height) = if landing.snap { raw } else { sized };
        let mut params = json!({"ids": ids, "x": inch(x), "y": inch(y), "width": inch(width), "height": inch(height), "page": cx.page});
        if let (true, Some(map)) = (landing.snap, params.as_object_mut()) {
            let mut snap = Self::snap_param(cx);
            if let (true, Some(words)) = (landing.keep, snap.as_object_mut()) {
                words.insert("keep".to_owned(), json!("proportions"));
            }
            map.insert("snap".to_owned(), snap);
        }
        if let Some(answer) = self.run_edit("shape.resize", &params) {
            self.noted_snap(&answer);
            self.keep_preview(cx.page, ids);
        }
    }

    /// The rotation handle of the combined box of several shapes was let go: they are turned by the step the preview showed, about the point it turned them about
    /// (`shape.rotate` with `by` and `about`).
    fn finish_rotate_set(&mut self, cx: &Cx, gesture: &Gesture, ids: &[u32], pin: P) {
        let Some(turning) = gesture.landing.turn.filter(|_| gesture.landing.worked_out()) else { return };
        if turning.to.abs() < NO_CHANGE {
            return;
        }
        let params = json!({"ids": ids, "by": inch(turning.to), "about": {"x": inch(pin.0), "y": inch(pin.1)}, "page": cx.page});
        if self.run_edit("shape.rotate", &params).is_some() {
            self.keep_preview(cx.page, ids);
        }
    }

    fn finish_join(&mut self, cx: &Cx, gesture: &Gesture, from: Target) {
        let to = gesture.target;
        // A short drag, or one that comes back to where it began, joins nothing.
        let travelled = geom::distance(gesture.origin, gesture.now) * cx.view.scale();
        if travelled < f64::from(POINT_REACH) || from == to {
            return;
        }
        // A free end is sent where the pointer was, with `snap`: the engine snaps it as the preview did, onto a connection point as glue.
        let mut params = json!({"from": from.param(), "to": to.param(), "page": cx.page});
        if let (true, Some(map)) = (gesture.landing.snap, params.as_object_mut()) {
            map.insert("snap".to_owned(), Self::snap_param(cx));
        }
        if let Some(answer) = self.run_edit("connector.add", &params)
            && let Some(id) = answer.get("id").and_then(serde_json::Value::as_u64).and_then(|id| u32::try_from(id).ok())
        {
            self.noted_snap(&answer);
            self.keep_preview(cx.page, &[id]);
            self.select(&[id]);
        }
    }

    /// The button came up on a press that began on an AutoConnect arrow. A click joins the nearest neighbour in the arrow's direction, or makes a copy there; a drag onto a shape
    /// joins it, at the connection point it was let go on if it was on one. A drag let go over bare page (or the shape it began on) makes nothing.
    fn finish_arrow(&mut self, cx: &Cx, gesture: &Gesture, from: u32, side: Side) {
        let mut params = json!({"from": from, "direction": side.word(), "page": cx.page});
        let Some(map) = params.as_object_mut() else { return };
        if !gesture.active {
            map.insert("existing".to_owned(), json!(true));
        } else {
            match gesture.target {
                Target::Point { id, index, .. } if id != from => {
                    map.insert("to".to_owned(), json!(id));
                    map.insert("to_point".to_owned(), json!(index));
                }
                Target::Shape(id) if id != from => {
                    map.insert("to".to_owned(), json!(id));
                }
                _ => return,
            }
        }
        self.auto_run(cx.page, &params);
    }

    fn finish_reshape(&mut self, cx: &Cx, gesture: &Gesture, id: u32, end: Which) {
        let mut params = json!({"id": id, "end": end.word(), "target": gesture.target.param(), "page": cx.page});
        if let (true, Some(map)) = (gesture.landing.snap, params.as_object_mut()) {
            map.insert("snap".to_owned(), Self::snap_param(cx));
        }
        if let Some(answer) = self.run_edit("connector.reconnect", &params) {
            self.noted_snap(&answer);
            self.keep_preview(cx.page, &[id]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_is_found_within_reach_and_the_nearest_wins() {
        let cx = Cx {
            area: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0)),
            view: crate::canvas::view::View { zoom: 1.0, center: (2.0, 1.5), fitted: false },
            page: 1,
            settings: crate::canvas::page_drawing::PageSettings::default(),
        };
        let basis = Basis { origin: (1.0, 1.0), ex: (1.0, 0.0), ey: (0.0, 1.0), w: 2.0, h: 1.0 };
        // 96 points to an inch: a reach of 8 points is about 0.083 inch.
        assert_eq!(handle_at(&basis, (3.02, 2.01), &cx), Some(Handle::NorthEast));
        assert_eq!(handle_at(&basis, (2.0, 1.0), &cx), Some(Handle::South));
        assert_eq!(handle_at(&basis, (2.0, 1.5), &cx), None);
        // A box too small for its side handles has none to take hold of.
        let small = Basis { origin: (1.0, 1.0), ex: (1.0, 0.0), ey: (0.0, 1.0), w: 0.2, h: 0.1 };
        assert_eq!(handle_at(&small, (1.1, 1.0), &cx), None);
        // A shape turned a quarter to the left has its own x axis going up the page: its east handle (the middle of the end of the x axis) is at
        // the top of the page's picture of it, and its south handle (the middle of the bottom edge of its own box) is on its right-hand side.
        let turned = Basis { origin: (3.0, 1.0), ex: (0.0, 1.0), ey: (-1.0, 0.0), w: 2.0, h: 1.0 };
        assert_eq!(handle_at(&turned, (2.0, 3.0), &cx), Some(Handle::NorthEast));
        assert_eq!(handle_at(&turned, (2.5, 3.0), &cx), Some(Handle::East));
        assert_eq!(handle_at(&turned, (3.0, 2.0), &cx), Some(Handle::South));
        assert_eq!(handle_at(&turned, (2.5, 2.0), &cx), None);
    }

    #[test]
    fn the_end_of_a_connector_is_found_by_the_pointer() {
        let item = Item {
            id: 1,
            kind: Kind::Connector,
            corners: [(0.0, 0.0); 4],
            frame: ((0.0, 0.0), (4.0, 0.0)),
            turned: false,
            in_turned_group: false,
            select_mode: 1,
            pin: (2.0, 0.0),
            parent: None,
            degrees: 0.0,
            outline: vec![vec![(0.0, 0.0), (4.0, 0.0)]],
            painted: vec![true],
            hidden: false,
            text_blocks: Vec::new(),
            ends: Some(((0.0, 0.0), (4.0, 0.0))),
            glue: [None, None],
            bounds: ((0.0, 0.0), (4.0, 0.0)),
        };
        assert_eq!(end_at(&item, (0.05, 0.0), 0.1), Some(Which::Begin));
        assert_eq!(end_at(&item, (3.95, 0.02), 0.1), Some(Which::End));
        assert_eq!(end_at(&item, (2.0, 0.0), 0.1), None);
    }

    #[test]
    fn a_target_is_sent_as_the_engine_takes_it() {
        assert_eq!(Target::Shape(7).param(), json!({"id": 7}));
        assert_eq!(Target::Point { id: 7, index: 2, at: (0.0, 0.0) }.param(), json!({"id": 7, "point": 2}));
        assert_eq!(Target::Free((1.5, 2.25)).param(), json!({"x": 1.5, "y": 2.25}));
    }
}
