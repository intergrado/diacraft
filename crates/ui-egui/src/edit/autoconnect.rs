//! AutoConnect in the window: the arrows round the shape the pointer rests on, the flyout of masters, and what a click, a drag and the keys do with them.
//!
//! **What the specifications say**: with AutoConnect
//! on, pointing at a shape shows an arrow at each of its four sides; pointing at an arrow opens a strip of up to **four** masters taken from the Quick Shapes (the first of the
//! stencil the Shapes window shows) and a faint preview of the one pointed at on the page; choosing one places it on that side and glues a connector to it; **a drag from an
//! arrow onto a shape that is on the page joins the two**; a master dragged from the Shapes window onto an
//! arrow is placed and joined the same way. When AutoConnect is off the arrows do not show. **Every change is one `connector.autoconnect`** (`cmd/autoconnect.rs`): a click sends
//! `existing` (the nearest neighbour in that direction, else a copy), a master sends `master`, a drag sends `to`. **There is no circle handle and no "Glue to Shape"
//! tip**: the connectors page describes them under its Web tab, which is the reference app's web version, and the desktop app's tab and published pictures show the arrows alone (took out what had built from the other reading).
//!
//! **Where the specifications are silent, and what is chosen** (each is on the check list):
//! * **The delays.** No specification gives one. The arrows show once the pointer has rested on a shape for [`ARROWS_DELAY`], so that a pointer going across the page flashes none;
//!   the flyout opens once the pointer has rested on an arrow for [`FLYOUT_DELAY`], and stays [`FLYOUT_GRACE`] after the pointer leaves the arrow and the flyout, so that it
//!   survives the way from one to the other.
//! * **The look.** The arrows are **pale sky blue** and, under the pointer, a **deeper blue** (the theme tokens `auto_arrow` and `auto_arrow_hot`, each 3:1 or more on the page and on
//!   the canvas in both themes): the desktop's published pictures draw them so and the desktop pages name no colour in words, so the blues themselves are ours. The flyout is a
//!   strip of tiles across the arrow's direction (a column beside a left or right arrow, a row beyond an up or down one: the spec says "a strip", and the preview of the shape
//!   stands in the arrow's direction, so a strip across it leaves the preview in view), the masters' tiles drawn as the Shapes window draws them.
//! * **The keys.** None is published ("Shortcut: none published"). **Alt with an arrow key is a click on that arrow for the one shape selected**, and **Alt+Shift with an arrow
//!   key opens that arrow's flyout with the keyboard**: the arrow keys move between the masters, Enter drops the one that has the keyboard, Escape closes it.
//! * **A drag let go over bare page makes nothing**, and a drag from an arrow ignores the Glue switch (it is a command of its own, not a connector end dropped).
//! * **Which shapes have arrows**: a shape (not a group, not a connector) whose layers allow glue, with the Pointer Tool on and nothing being dragged. A shape with no connection point
//!   has them too: the engine glues a connector to the whole of such a shape, so a click on one of its arrows is no longer refused.

use std::sync::Arc;
use std::time::Duration;

use diacraft_engine::draw::route::Dir;
use egui::{Context, CornerRadius, Key, Modifiers, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value, json};

use super::geom::P;
use super::scene::{Item, Kind};
use super::{Cx, HIT_REACH, Tool};
use crate::app::DiacraftApp;
use crate::stencils::{Art, Figure, MasterInfo, StencilInfo};
use crate::theme::Tokens;

/// How long the pointer must rest on a shape for its arrows to show.
pub const ARROWS_DELAY: Duration = Duration::from_millis(250);
/// How long the pointer must rest on an arrow for the flyout to open.
pub const FLYOUT_DELAY: Duration = Duration::from_millis(400);
/// How long the flyout stays after the pointer has left both the arrow and the flyout.
pub const FLYOUT_GRACE: Duration = Duration::from_millis(250);
/// The most masters the flyout offers.
pub const MAX_FLYOUT: usize = 4;

/// The base of an arrow's triangle (across the side it is on), its height (out from the side), the room between the shape's box and its base, and the slack round it that still
/// counts as on it, in points.
const ARROW_BASE: f32 = 18.0;
const ARROW_LEN: f32 = 11.0;
const ARROW_GAP: f32 = 8.0;
/// The room between the box and an arrow's base while the shape is selected: its handles stand on the box's edge (12 across) and its rotation handle's stem runs up from the
/// top, so the arrows stand a little farther off.
const ARROW_GAP_SELECTED: f32 = 13.0;
const ARROW_SLACK: f32 = 3.0;
/// A tile of the flyout, the room between tiles and between the tiles and the edge of the flyout, in points.
const TILE: f32 = 44.0;
const TILE_GAP: f32 = 4.0;
const FLYOUT_PAD: f32 = 5.0;
/// How far a flyout overlaps the arrow it stands beside, so that the pointer has no gap to cross.
const FLYOUT_OVERLAP: f32 = 2.0;
/// The room kept between the flyout and the edge of the drawing area.
const EDGE: f32 = 4.0;

/// The four sides, as the arrows of a shape and the directions of `connector.autoconnect` are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Right,
    Left,
    Up,
    Down,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Right, Side::Left, Side::Up, Side::Down];

    /// The word the engine's commands take.
    pub const fn word(self) -> &'static str {
        match self {
            Side::Right => "right",
            Side::Left => "left",
            Side::Up => "up",
            Side::Down => "down",
        }
    }

    fn dir(self) -> Dir {
        match self {
            Side::Right => Dir::Right,
            Side::Left => Dir::Left,
            Side::Up => Dir::Up,
            Side::Down => Dir::Down,
        }
    }

    /// The side an arrow key of the drawing stands for.
    fn of_key(key: Key) -> Option<Side> {
        match key {
            Key::ArrowRight => Some(Side::Right),
            Key::ArrowLeft => Some(Side::Left),
            Key::ArrowUp => Some(Side::Up),
            Key::ArrowDown => Some(Side::Down),
            _ => None,
        }
    }
}

/// One arrow of a shape, on the screen: a click on it joins the nearest neighbour or makes a copy, a rest on it opens the flyout, a drag from it onto a shape joins it.
#[derive(Clone, Copy, Debug)]
pub struct Arrow {
    pub side: Side,
    /// The triangle.
    pub tri: [Pos2; 3],
    /// What a press takes: the triangle and a little round it, clear of the selection handles on the box's own edge.
    pub hit: Rect,
    /// What keeps the arrows showing while the pointer crosses from the shape to one of them.
    pub zone: Rect,
}

/// The four arrows of a shape whose box is `b` on the screen, `gap` off its sides.
pub fn arrows_of(b: Rect, gap: f32) -> [Arrow; 4] {
    let c = b.center();
    let half = ARROW_BASE / 2.0;
    let make = |side: Side| {
        // `out` points away from the shape, `across` along its side.
        let (out, across) = match side {
            Side::Right => (vec2(1.0, 0.0), vec2(0.0, 1.0)),
            Side::Left => (vec2(-1.0, 0.0), vec2(0.0, 1.0)),
            Side::Up => (vec2(0.0, -1.0), vec2(1.0, 0.0)),
            Side::Down => (vec2(0.0, 1.0), vec2(1.0, 0.0)),
        };
        let edge = match side {
            Side::Right => pos2(b.right(), c.y),
            Side::Left => pos2(b.left(), c.y),
            Side::Up => pos2(c.x, b.top()),
            Side::Down => pos2(c.x, b.bottom()),
        };
        let base = edge + out * gap;
        let tip = base + out * ARROW_LEN;
        let tri = [base - across * half, base + across * half, tip];
        let hit = Rect::from_two_pos(base - out * 1.0 - across * (half + ARROW_SLACK), tip + out * ARROW_SLACK + across * (half + ARROW_SLACK));
        // The zone reaches from the box's edge past the triangle, so that the pointer going from the shape to the arrow finds no gap.
        let zone = Rect::from_two_pos(edge + out * 1.0 - across * (half + ARROW_SLACK), tip + out * ARROW_SLACK + across * (half + ARROW_SLACK));
        Arrow { side, tri, hit, zone }
    };
    [make(Side::Right), make(Side::Left), make(Side::Up), make(Side::Down)]
}

/// The box of an item on the screen: the box that holds its corners.
fn screen_box(item: &Item, cx: &Cx) -> Rect {
    let mut b = Rect::NOTHING;
    for corner in item.corners {
        b.extend_with(cx.screen_at(corner));
    }
    b
}

/// Where the flyout of `count` tiles stands beside `arrow`, kept inside `area`: its rectangle and each tile's. **It is a strip across the arrow's direction** (a column beside a
/// left or right arrow, a row beyond an up or down one), so that the preview of the shape, which stands in the arrow's direction, is not under it.
fn flyout_layout(arrow: &Arrow, count: usize, area: Rect) -> (Rect, Vec<Rect>) {
    let n = count.max(1) as f32;
    let along = n * TILE + (n - 1.0) * TILE_GAP + 2.0 * FLYOUT_PAD;
    let across = TILE + 2.0 * FLYOUT_PAD;
    let column = matches!(arrow.side, Side::Right | Side::Left);
    let size = if column { vec2(across, along) } else { vec2(along, across) };
    let c = arrow.hit.center();
    let min = match arrow.side {
        Side::Right => pos2(arrow.hit.right() - FLYOUT_OVERLAP, c.y - size.y / 2.0),
        Side::Left => pos2(arrow.hit.left() + FLYOUT_OVERLAP - size.x, c.y - size.y / 2.0),
        Side::Up => pos2(c.x - size.x / 2.0, arrow.hit.top() + FLYOUT_OVERLAP - size.y),
        Side::Down => pos2(c.x - size.x / 2.0, arrow.hit.bottom() - FLYOUT_OVERLAP),
    };
    let room = area.shrink(EDGE);
    let at = pos2(min.x.min(room.right() - size.x).max(room.left()), min.y.min(room.bottom() - size.y).max(room.top()));
    let rect = Rect::from_min_size(at, size);
    let tiles = (0..count)
        .map(|i| {
            let step = i as f32 * (TILE + TILE_GAP);
            let offset = if column { vec2(FLYOUT_PAD, FLYOUT_PAD + step) } else { vec2(FLYOUT_PAD + step, FLYOUT_PAD) };
            Rect::from_min_size(at + offset, vec2(TILE, TILE))
        })
        .collect();
    (rect, tiles)
}

/// Where the keyboard goes from tile `i` of `count` in a strip, one press of an arrow key: **right and down go on, left and up go back**, and the ends stay.
pub(crate) fn tile_after(i: usize, count: usize, key: Key) -> usize {
    let target = match key {
        Key::ArrowRight | Key::ArrowDown => i.saturating_add(1),
        Key::ArrowLeft | Key::ArrowUp => i.saturating_sub(1),
        _ => i,
    };
    if target < count { target } else { i }
}

/// The preview of a master on the page: the shape's own outline fitted to `rect` where it has one, else the box, faintly filled.
fn paint_ghost(painter: &Painter, t: &Tokens, rect: Rect, outline: Option<&Arc<Figure>>) {
    let line = Stroke::new(1.5, t.edit_line);
    let Some(figure) = outline.filter(|f| f.aspect.0 > 0.0 && f.aspect.1 > 0.0) else {
        painter.rect_filled(rect, CornerRadius::same(2), t.edit_fill);
        painter.rect_stroke(rect, CornerRadius::same(2), line, StrokeKind::Inside);
        return;
    };
    let (aw, ah) = figure.aspect;
    let at = |p: &P| pos2(rect.left() + (p.0 / aw) as f32 * rect.width(), rect.bottom() - (p.1 / ah) as f32 * rect.height());
    for (points, _) in &figure.areas {
        let path: Vec<Pos2> = points.iter().map(at).collect();
        if path.len() >= 3 {
            painter.add(Shape::convex_polygon(path.clone(), t.edit_fill, Stroke::NONE));
            painter.add(Shape::closed_line(path, line));
        }
    }
    for open in &figure.lines {
        painter.add(Shape::line(open.iter().map(at).collect(), line));
    }
}

/// The preview of the master under the pointer: where the shape would be on the page and how big, in paper inches.
#[derive(Clone, Copy, Debug)]
struct Ghost {
    centre: P,
    size: (f64, f64),
}

/// What a flyout offers: the masters, the figure each tile shows and the outline each preview is drawn with.
struct Offer {
    masters: Vec<MasterInfo>,
    arts: Vec<Art>,
    outlines: Vec<Option<Arc<Figure>>>,
}

/// A shape the pointer rests on, and since when.
#[derive(Clone, Copy, Debug)]
struct Hover {
    id: u32,
    since: Duration,
    /// The arrows are showing: the rest is over.
    shown: bool,
}

/// The flyout of one arrow.
struct Flyout {
    from: u32,
    side: Side,
    /// When the pointer came to the arrow (or the keyboard asked for the flyout).
    since: Duration,
    open: bool,
    /// When the pointer left both the arrow and the flyout, while it was away.
    left: Option<Duration>,
    /// Opened by the keyboard: it does not go with the pointer.
    keyboard: bool,
    masters: Vec<MasterInfo>,
    arts: Vec<Art>,
    /// The outline of each master, for the preview on the page (`None` for one that has none to read).
    outlines: Vec<Option<Arc<Figure>>>,
    rect: Rect,
    tiles: Vec<Rect>,
    /// The master that has the pointer or the keyboard.
    current: Option<usize>,
    ghost: Option<Ghost>,
}

/// What the window remembers of AutoConnect: the shape the pointer rests on and the flyout open.
#[derive(Default)]
pub struct AutoState {
    hover: Option<Hover>,
    flyout: Option<Flyout>,
    /// Where the pointer was in the last frame, over the drawing area.
    pointer: Option<Pos2>,
}

impl AutoState {
    /// Closes the flyout, if one is open. → whether there was one.
    pub(crate) fn close_flyout(&mut self) -> bool {
        self.flyout.take().is_some()
    }

    /// Whether a flyout is open.
    pub(crate) fn has_flyout(&self) -> bool {
        self.flyout.as_ref().is_some_and(|f| f.open)
    }
}

impl DiacraftApp {
    /// Whether the View > Visual Aids > AutoConnect box shows ticked: the drawing's box, with the program-wide setting over it (off, nothing is on).
    pub(crate) fn autoconnect_box(&self) -> bool {
        self.ui.enable_autoconnect && self.session.document().is_none_or(diacraft_engine::view::autoconnect_on)
    }

    /// Whether AutoConnect works with what the window is doing: the box is on, the Pointer Tool is in use and no text box, Format Painter or other mode has the pointer.
    pub(crate) fn autoconnect_live(&self) -> bool {
        let edit = &self.ui.edit;
        self.autoconnect_box()
            && edit.tool == Tool::Pointer
            && edit.text.is_none()
            && edit.painter.is_none()
            && edit.ruler.is_none()
            && !self.is_modal_open()
    }

    /// A setting of AutoConnect changed: the arrows and the flyout that are up go.
    pub(crate) fn autoconnect_settings_changed(&mut self) {
        self.ui.edit.auto = AutoState::default();
    }

    /// What the window says of AutoConnect, for `ui.inspect`: the program-wide setting, the drawing's box, the shape whose arrows show and the flyout.
    pub(crate) fn autoconnect_state(&self) -> Value {
        let auto = &self.ui.edit.auto;
        json!({
            "program": self.ui.enable_autoconnect,
            "drawing": self.session.document().is_none_or(diacraft_engine::view::autoconnect_on),
            "arrows": auto.hover.filter(|h| h.shown).map(|h| h.id),
            "flyout": auto.flyout.as_ref().filter(|f| f.open).map(|f| json!({
                "from": f.from,
                "side": f.side.word(),
                "masters": f.masters.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
                "current": f.current,
            })),
        })
    }

    /// The side and the names of the masters of the flyout that is open, for tests and agents.
    pub fn autoconnect_flyout(&self) -> Option<(String, Vec<String>)> {
        self.ui.edit.auto.flyout.as_ref().filter(|f| f.open).map(|f| (f.side.word().to_owned(), f.masters.iter().map(|m| m.name.clone()).collect()))
    }

    /// The master of the open flyout that has the pointer or the keyboard (its place in the flyout, from 0).
    pub fn autoconnect_current(&self) -> Option<usize> {
        self.ui.edit.auto.flyout.as_ref().filter(|f| f.open).and_then(|f| f.current)
    }

    /// Where the preview of the master under the pointer is, in paper inches: the middle of the shape the arrow would make.
    pub fn autoconnect_ghost(&self) -> Option<(f64, f64)> {
        self.ui.edit.auto.flyout.as_ref().filter(|f| f.open && f.current.is_some()).and_then(|f| f.ghost).map(|g| g.centre)
    }

    /// The shape a drag from an arrow would join if the button came up now.
    pub fn autoconnect_drag_target(&self) -> Option<u32> {
        let g = self.ui.edit.gesture.as_ref().filter(|g| g.active)?;
        match (&g.mode, g.target) {
            (super::gesture::Mode::Arrow { from, .. }, super::gesture::Target::Point { id, .. } | super::gesture::Target::Shape(id))
                if id != *from =>
            {
                Some(id)
            }
            _ => None,
        }
    }

    /// The masters the flyout offers: **the Quick Shapes of the stencil the Shapes window shows** that the strip may offer, the first [`MAX_FLYOUT`] of them (the first four of the stencil that is selected, changed by what is in its Quick Shapes),
    /// the stencil's first ones when it has none
    /// And the next stencil's where it feeds the strip nothing ([`DiacraftApp::autoconnect_offer`], which `stencil.quick`'s `offer` agrees with); and the
    /// figure each is drawn with.
    fn flyout_masters(&mut self, ctx: &Context) -> Offer {
        let masters = self.autoconnect_offer();
        let arts = masters.iter().map(|m| self.art_of(ctx, m)).collect();
        let outlines = masters.iter().map(|m| self.outline_of(m)).collect();
        Offer { masters, arts, outlines }
    }

    /// The masters the flyout offers (`stencil.quick`'s `offer` agrees): those of the stencil on view ([`StencilInfo::offer`]), and where that stencil feeds the strip
    /// nothing (Arrow Shapes, the Cross-Functional Flowchart Shapes, a file of parts), **those of the first stencil of the Shapes window's list that feeds it**, the default the
    /// specification names ("the stencil that follows the Quick Shapes stencil"). Empty when no open stencil feeds it: the arrows still show, a click on one still joins or copies, and no
    /// flyout opens.
    pub fn autoconnect_offer(&mut self) -> Vec<MasterInfo> {
        let list = self.stencil_list();
        let at = self.current_stencil_index();
        let own: Vec<MasterInfo> = list.get(at).map(|s| s.offer().into_iter().take(MAX_FLYOUT).cloned().collect()).unwrap_or_default();
        if !own.is_empty() {
            return own;
        }
        list.iter()
            .filter(|s| !s.is_gathered())
            .map(StencilInfo::offer)
            .find(|offered| !offered.is_empty())
            .map(|offered| offered.into_iter().take(MAX_FLYOUT).cloned().collect())
            .unwrap_or_default()
    }

    /// The arrows of `item` on the screen: a little farther off its box while it is selected (its handles are on the box).
    fn arrows_for(&self, item: &Item, cx: &Cx) -> [Arrow; 4] {
        let gap = if self.ui.edit.selection.contains(&item.id) { ARROW_GAP_SELECTED } else { ARROW_GAP };
        arrows_of(screen_box(item, cx), gap)
    }

    /// The shape the pointer is on, for arrows: a shape (not a group or a connector) whose layers allow glue, under `at`; the shape that already has the arrows
    /// stays while the pointer is on its arrows or its flyout.
    fn auto_target(&self, cx: &Cx, at: Pos2, over_page: bool) -> Option<u32> {
        let scene = self.ui.edit.scene.as_ref().filter(|s| s.page == cx.page)?;
        let auto = &self.ui.edit.auto;
        if let Some(h) = &auto.hover
            && let Some(item) = scene.get(h.id).filter(|i| i.kind == Kind::Shape)
        {
            let arrows = self.arrows_for(item, cx);
            // A flyout that does not touch its arrow (one kept inside the drawing area may stand off it) and the room between them: the pointer crossing it keeps both.
            let open = auto.flyout.as_ref().filter(|f| f.open);
            let across = open.and_then(|f| arrows.iter().find(|a| a.side == f.side && !a.zone.intersects(f.rect)).map(|a| a.zone.union(f.rect)));
            if open.is_some_and(|f| f.rect.contains(at)) || across.is_some_and(|r| r.contains(at)) || arrows.iter().any(|a| a.zone.contains(at)) {
                return Some(h.id);
            }
        }
        if !over_page {
            return None;
        }
        // A shape whose layers allow no glue has no arrows: no connector is glued to it.
        scene.hit(cx.page_at(at), cx.reach(HIT_REACH)).filter(|i| i.kind == Kind::Shape && !scene.standing(i.id).no_glue).map(|item| item.id)
    }

    /// Whether `at` is on the open flyout: a press there is the flyout's.
    pub(crate) fn auto_in_flyout(&self, at: Pos2) -> bool {
        self.ui.edit.auto.flyout.as_ref().is_some_and(|f| f.open && f.rect.contains(at))
    }

    /// The arrow showing under `at`, and the shape it is on.
    pub(crate) fn auto_arrow_at(&self, cx: &Cx, at: Pos2) -> Option<(u32, Side)> {
        let h = self.ui.edit.auto.hover.filter(|h| h.shown)?;
        let item = self.ui.edit.scene.as_ref().filter(|s| s.page == cx.page)?.get(h.id).filter(|i| i.kind == Kind::Shape)?;
        self.arrows_for(item, cx).into_iter().find(|a| a.hit.contains(at)).map(|a| (h.id, a.side))
    }

    /// The middle of the side of shape `id`'s box that `side` is, in paper inches: where a drag from its arrow begins.
    pub(super) fn auto_anchor(&self, id: u32, side: Side) -> Option<P> {
        let item = self.ui.edit.scene.as_ref()?.get(id)?;
        let (xs, ys): (Vec<f64>, Vec<f64>) = item.corners.iter().copied().unzip();
        let (lo, hi) = (
            (xs.iter().copied().fold(f64::INFINITY, f64::min), ys.iter().copied().fold(f64::INFINITY, f64::min)),
            (xs.iter().copied().fold(f64::NEG_INFINITY, f64::max), ys.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
        );
        let (cx, cy) = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
        Some(match side {
            Side::Right => (hi.0, cy),
            Side::Left => (lo.0, cy),
            Side::Up => (cx, hi.1),
            Side::Down => (cx, lo.1),
        })
    }

    /// Runs once a frame with the drawing area: works out which shape's arrows are up and which arrow's flyout, from where the pointer is and for how long, and lays the
    /// flyout's tiles out as widgets (so that they take the pointer and have names for assistive technology). `pointer` is the latest pointer place, `over` where it is when the
    /// drawing area has it.
    pub(crate) fn auto_update(&mut self, ui: &Ui, cx: &Cx, pointer: Option<Pos2>, over: Option<Pos2>) {
        let ctx = ui.ctx().clone();
        if !self.autoconnect_live() || self.ui.edit.gesture.is_some() {
            self.ui.edit.auto = AutoState::default();
            return;
        }
        let now = self.clock.now();
        // The keyboard's flyout belongs to the shape that was selected.
        let selection = self.ui.edit.selection.as_slice();
        if self.ui.edit.auto.flyout.as_ref().is_some_and(|f| f.keyboard && selection != [f.from]) {
            self.ui.edit.auto.flyout = None;
        }
        let inside = pointer.filter(|p| cx.area.contains(*p));
        self.ui.edit.auto.pointer = inside;
        // A master from the Shapes window is being dragged: the page's arrows take it, so the pointer is over the page as far as they are concerned.
        let dragging_tile = self.ui.edit.tile.is_some();
        let target = inside.and_then(|at| self.auto_target(cx, at, over.is_some() || dragging_tile));
        // The shape the pointer is on.
        let auto = &mut self.ui.edit.auto;
        match (target, auto.hover.as_mut()) {
            (Some(id), Some(h)) if h.id == id => {}
            (Some(id), _) => {
                auto.hover = Some(Hover { id, since: now, shown: false });
                // The pointer's flyout goes with the shape it was on; the keyboard's belongs to the selection and stays.
                if !auto.flyout.as_ref().is_some_and(|f| f.keyboard) {
                    auto.flyout = None;
                }
            }
            (None, _) => {
                auto.hover = None;
                if !auto.flyout.as_ref().is_some_and(|f| f.keyboard) {
                    auto.flyout = None;
                }
            }
        }
        if let Some(h) = auto.hover.as_mut() {
            let due = now >= h.since + ARROWS_DELAY;
            h.shown = due;
            if !due {
                ctx.request_repaint_after(h.since + ARROWS_DELAY - now);
            }
        }
        let Some(hover) = auto.hover.filter(|h| h.shown) else {
            self.auto_register_flyout(ui, cx);
            return;
        };
        let all = self.ui.edit.scene.as_ref().and_then(|s| s.get(hover.id)).map(|i| self.arrows_for(i, cx));
        let Some(all) = all else {
            self.ui.edit.auto.hover = None;
            return;
        };
        for a in &all {
            self.note(format!("autoconnect.arrow.{}", a.side.word()), a.hit, true);
        }
        // The arrow the pointer is on opens its flyout after a rest, and a flyout the pointer has left goes after the grace.
        let on_arrow = inside.and_then(|at| all.iter().find(|a| a.hit.contains(at)).map(|a| a.side));
        let in_flyout = inside.is_some_and(|at| self.ui.edit.auto.flyout.as_ref().is_some_and(|f| f.open && f.rect.contains(at)));
        let auto = &mut self.ui.edit.auto;
        // What the flyout is now: its side, its shape and whether the keyboard opened it.
        let held = auto.flyout.as_ref().map(|f| (f.side, f.from, f.keyboard));
        match (on_arrow, held) {
            // The pointer is on the arrow whose flyout it is: the grace starts again.
            (Some(side), Some((was, from, _))) if was == side && from == hover.id => {
                if let Some(f) = auto.flyout.as_mut() {
                    f.left = None;
                }
            }
            // The keyboard's flyout is open: it has the person's attention, and the pointer makes none of its own until it is done (a tile it passes over is still current).
            (_, Some((_, _, true))) => {}
            (Some(side), _) => {
                auto.flyout = Some(Flyout {
                    from: hover.id,
                    side,
                    since: now,
                    open: false,
                    left: None,
                    keyboard: false,
                    masters: Vec::new(),
                    arts: Vec::new(),
                    outlines: Vec::new(),
                    rect: Rect::NOTHING,
                    tiles: Vec::new(),
                    current: None,
                    ghost: None,
                });
            }
            (None, Some((_, _, false))) => {
                let left = match auto.flyout.as_mut() {
                    Some(f) if in_flyout => {
                        f.left = None;
                        None
                    }
                    Some(f) => Some(*f.left.get_or_insert(now)),
                    None => None,
                };
                if let Some(since) = left {
                    if now >= since + FLYOUT_GRACE {
                        auto.flyout = None;
                    } else {
                        ctx.request_repaint_after(since + FLYOUT_GRACE - now);
                    }
                }
            }
            _ => {}
        }
        // The flyout opens after the rest on its arrow.
        let wants_open = auto.flyout.as_ref().filter(|f| !f.open && !f.keyboard).map(|f| f.since + FLYOUT_DELAY);
        if let Some(due) = wants_open {
            if now >= due {
                let offer = self.flyout_masters(&ctx);
                if let Some(f) = self.ui.edit.auto.flyout.as_mut() {
                    f.open = true;
                    (f.masters, f.arts, f.outlines) = (offer.masters, offer.arts, offer.outlines);
                }
                // A stencil with nothing to offer shows no flyout.
                if self.ui.edit.auto.flyout.as_ref().is_some_and(|f| f.masters.is_empty()) {
                    self.ui.edit.auto.flyout = None;
                }
            } else {
                ctx.request_repaint_after(due - now);
            }
        }
        self.auto_register_flyout(ui, cx);
    }

    /// Lays the open flyout out beside the arrow of its own shape (the pointer may be on another by now, and the view may have moved since it opened).
    fn auto_layout_flyout(&mut self, cx: &Cx) {
        let Some((from, side, count)) = self.ui.edit.auto.flyout.as_ref().filter(|f| f.open).map(|f| (f.from, f.side, f.masters.len())) else {
            return;
        };
        let arrow = self
            .ui
            .edit
            .scene
            .as_ref()
            .and_then(|s| s.get(from))
            .map(|i| self.arrows_for(i, cx))
            .and_then(|all| all.into_iter().find(|a| a.side == side));
        let Some(arrow) = arrow else { return };
        let (rect, tiles) = flyout_layout(&arrow, count, cx.area);
        if let Some(f) = self.ui.edit.auto.flyout.as_mut() {
            (f.rect, f.tiles) = (rect, tiles);
        }
    }

    /// Lays the open flyout's tiles out as widgets: the pointer on one makes it the current master, a click on one drops it.
    fn auto_register_flyout(&mut self, ui: &Ui, cx: &Cx) {
        self.auto_layout_flyout(cx);
        let (from, side, rect, tiles, names) = {
            let Some(f) = self.ui.edit.auto.flyout.as_ref().filter(|f| f.open) else { return };
            (f.from, f.side, f.rect, f.tiles.clone(), f.masters.iter().map(|m| m.name.clone()).collect::<Vec<_>>())
        };
        self.note("autoconnect.flyout", rect, true);
        let mut hovered = None;
        let mut chosen = None;
        for (i, rect) in tiles.iter().enumerate() {
            self.note(format!("autoconnect.flyout.{i}"), *rect, true);
            // Drag as well as click: a tile that only senses clicks leaves the drag under it to the drawing area, which would start a rubber band from the press.
            let response = ui.interact(*rect, ui.scope_id().with(("autoconnect_tile", i)), Sense::click_and_drag());
            let name = names.get(i).map_or("", String::as_str);
            crate::widgets::name_button(&response, true, name);
            let response = response.on_hover_text(name);
            if response.hovered() {
                hovered = Some(i);
            }
            if response.clicked() {
                chosen = Some(i);
            }
        }
        let current = {
            let Some(f) = self.ui.edit.auto.flyout.as_mut() else { return };
            match hovered {
                Some(i) => f.current = Some(i),
                None if !f.keyboard => f.current = None,
                None => {}
            }
            f.current
        };
        // Where the engine would put the shape of the master that has the pointer: its own function says, so that the preview is where the drop lands.
        let ghost = current.and_then(|i| self.auto_ghost(cx.page, from, side, i));
        if let Some(f) = self.ui.edit.auto.flyout.as_mut() {
            f.ghost = ghost;
        }
        if let Some(i) = chosen {
            self.auto_choose(cx.page, from, side, i);
        }
    }

    /// The preview of master `i` of the open flyout: where `connector.autoconnect` would put its shape, which the engine's `autoconnect_centre` says.
    fn auto_ghost(&self, page: u32, from: u32, side: Side, i: usize) -> Option<Ghost> {
        let master = self.ui.edit.auto.flyout.as_ref()?.masters.get(i)?;
        let doc = self.session.document()?;
        let per_inch = self.ui.edit.scene.as_ref().filter(|s| s.page == page)?.per_inch;
        let (w, h) = (master.width.max(0.0), master.height.max(0.0));
        let centre = diacraft_engine::draw::autoconnect_centre(doc, page, from, side.dir(), (w, h))?;
        let ghost = Ghost { centre: (centre.0 / per_inch, centre.1 / per_inch), size: (w / per_inch, h / per_inch) };
        (ghost.centre.0.is_finite() && ghost.centre.1.is_finite()).then_some(ghost)
    }

    /// Master `i` of the open flyout is chosen: it is dropped beside shape `from` on the arrow `side`, joined to it, in one `connector.autoconnect`, and the new shape is selected.
    fn auto_choose(&mut self, page: u32, from: u32, side: Side, i: usize) {
        let Some(master) = self.ui.edit.auto.flyout.as_ref().and_then(|f| f.masters.get(i)).cloned() else { return };
        self.ui.edit.auto.flyout = None;
        self.auto_make(page, from, side, Some(&master));
    }

    /// A master put on an arrow (a tile of the flyout, or one dragged from the Shapes window): `connector.autoconnect` with `master`; a click on the arrow itself is `existing`.
    fn auto_make(&mut self, page: u32, from: u32, side: Side, master: Option<&MasterInfo>) {
        let mut params = json!({"from": from, "direction": side.word(), "page": page});
        if let Some(map) = params.as_object_mut() {
            match master {
                Some(m) => {
                    map.insert("master".to_owned(), json!(m.id));
                    if let Some(stencil) = &m.stencil {
                        map.insert("stencil".to_owned(), json!(stencil));
                    }
                }
                None => {
                    map.insert("existing".to_owned(), json!(true));
                }
            }
        }
        self.auto_run(page, &params);
    }

    /// Runs one `connector.autoconnect`, and leaves the shape it made selected (a join leaves the connector).
    pub(super) fn auto_run(&mut self, page: u32, params: &Value) {
        let Some(answer) = self.run_edit("connector.autoconnect", params) else { return };
        let number = |key: &str| answer.get(key).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
        let (Some(shape), Some(connector)) = (number("id"), number("connector")) else { return };
        let created = answer.get("created").and_then(Value::as_bool).unwrap_or(true);
        self.keep_preview(page, &[shape, connector]);
        self.select(&[if created { shape } else { connector }]);
    }

    /// A master from the Shapes window let go over a shape's arrow: it is placed and joined, as a tile of the flyout is. → whether it was let go on an arrow.
    pub(crate) fn auto_drop_master(&mut self, ctx: &Context, master: &MasterInfo) -> bool {
        // A part of a structure is no shape to join: it is dropped as a drop of it is, so a Swimlane let go on an arrow of a lane's shape is inserted in the list.
        if master.part {
            return false;
        }
        let Some(page) = self.current_page() else { return false };
        let (Some(cx), Some(at)) = (self.cx_of(page), ctx.input(|i| i.pointer.latest_pos())) else { return false };
        let Some((from, side)) = self.auto_arrow_at(&cx, at) else { return false };
        self.auto_make(page, from, side, Some(master));
        true
    }

    /// The keys of AutoConnect, for the one shape selected: Alt with an arrow key is a click on that arrow, Alt+Shift with one opens its flyout, and while a flyout is open by
    /// the keyboard the arrow keys move between its masters and Enter drops the one that has the keyboard. → whether a key was taken.
    pub(crate) fn auto_keys(&mut self, ctx: &Context, page: u32) -> bool {
        if !self.autoconnect_live() {
            return false;
        }
        if let Some(f) = self.ui.edit.auto.flyout.as_mut().filter(|f| f.open && f.keyboard) {
            let count = f.masters.len();
            for key in [Key::ArrowRight, Key::ArrowLeft, Key::ArrowUp, Key::ArrowDown] {
                if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key)) {
                    f.current = Some(tile_after(f.current.unwrap_or(0), count, key));
                    return true;
                }
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                let (from, side, current) = (f.from, f.side, f.current.unwrap_or(0));
                self.auto_choose(page, from, side, current);
                return true;
            }
            return false;
        }
        let Some(from) = self.single().filter(|item| item.kind == Kind::Shape).map(|item| item.id) else { return false };
        for key in [Key::ArrowRight, Key::ArrowLeft, Key::ArrowUp, Key::ArrowDown] {
            let Some(side) = Side::of_key(key) else { continue };
            // Shift first: a key pressed with Shift also matches the plain one.
            if ctx.input_mut(|i| i.consume_key(Modifiers::ALT | Modifiers::SHIFT, key)) {
                self.auto_open_by_key(ctx, page, from, side);
                return true;
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::ALT, key)) {
                self.auto_make(page, from, side, None);
                return true;
            }
        }
        false
    }

    /// Opens the flyout of `side` of shape `from` for the keyboard: the first master has it.
    fn auto_open_by_key(&mut self, ctx: &Context, page: u32, from: u32, side: Side) {
        let Some(cx) = self.cx_of(page) else { return };
        let Some(all) = self.ui.edit.scene.as_ref().and_then(|s| s.get(from)).map(|i| self.arrows_for(i, &cx)) else { return };
        let Offer { masters, arts, outlines } = self.flyout_masters(ctx);
        if masters.is_empty() {
            return;
        }
        let Some(arrow) = all.into_iter().find(|a| a.side == side) else { return };
        let (rect, tiles) = flyout_layout(&arrow, masters.len(), cx.area);
        let now = self.clock.now();
        self.ui.edit.auto.flyout = Some(Flyout {
            from,
            side,
            since: now,
            open: true,
            left: None,
            keyboard: true,
            masters,
            arts,
            outlines,
            rect,
            tiles,
            current: Some(0),
            ghost: None,
        });
        ctx.request_repaint();
    }

    /// The pointer cursor over an arrow: the hand.
    pub(crate) fn auto_cursor(&self, cx: &Cx, at: Pos2) -> Option<egui::CursorIcon> {
        self.auto_arrow_at(cx, at).map(|_| egui::CursorIcon::PointingHand)
    }

    /// Paints the arrows, the flyout and the preview of the master under the pointer, over the page (the colours are the theme's).
    pub(crate) fn paint_auto(&self, painter: &Painter, t: &Tokens, cx: &Cx) {
        let auto = &self.ui.edit.auto;
        let Some(scene) = self.ui.edit.scene.as_ref().filter(|s| s.page == cx.page) else { return };
        let flyout = auto.flyout.as_ref().filter(|f| f.open);
        if let Some(h) = auto.hover.filter(|h| h.shown)
            && let Some(item) = scene.get(h.id)
        {
            for a in self.arrows_for(item, cx) {
                let hot = auto.pointer.is_some_and(|p| a.hit.contains(p)) || flyout.is_some_and(|f| f.from == h.id && f.side == a.side);
                let fill = if hot { t.auto_arrow_hot } else { t.auto_arrow };
                painter.add(Shape::convex_polygon(a.tri.to_vec(), fill, Stroke::new(1.0, t.handle_fill)));
            }
        }
        let Some(f) = flyout else { return };
        // The preview of the master under the pointer: a faint shape where it would be, and the connector that would join it.
        if let (Some(g), Some(item)) = (f.ghost.filter(|_| f.current.is_some()), scene.get(f.from)) {
            let size = vec2((g.size.0 * cx.view.scale()) as f32, (g.size.1 * cx.view.scale()) as f32);
            let rect = Rect::from_center_size(cx.screen_at(g.centre), size);
            if rect.min.x.is_finite() && rect.max.x.is_finite() && rect.min.y.is_finite() && rect.max.y.is_finite() {
                paint_ghost(painter, t, rect, f.current.and_then(|i| f.outlines.get(i)).and_then(Option::as_ref));
                let near = self.arrows_for(item, cx).into_iter().find(|a| a.side == f.side).map(|a| a.tri[2]);
                if let Some(from) = near {
                    painter.line_segment([from, rect.clamp(from)], Stroke::new(1.0, t.edit_line));
                }
            }
        }
        // The flyout: a card with a tile for each master.
        painter.rect(f.rect, CornerRadius::same(6), t.panel_fill, Stroke::new(1.0, t.separator), StrokeKind::Inside);
        for (i, (rect, art)) in f.tiles.iter().zip(&f.arts).enumerate() {
            let current = f.current == Some(i);
            painter.rect_filled(*rect, CornerRadius::same(4), if current { t.hover } else { t.panel_header_fill });
            // The tile that has the pointer or the keyboard is ringed in the accent (a gray ring would hardly show on the dark card).
            if current {
                painter.rect_stroke(*rect, CornerRadius::same(4), Stroke::new(1.5, t.accent), StrokeKind::Inside);
            }
            crate::shapes_window::paint_art(painter, rect.shrink(5.0), art, t, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keyboard_goes_on_with_right_and_down_back_with_left_and_up_and_stays_at_the_ends() {
        assert_eq!(tile_after(0, 4, Key::ArrowRight), 1);
        assert_eq!(tile_after(1, 4, Key::ArrowDown), 2);
        assert_eq!(tile_after(3, 4, Key::ArrowRight), 3, "the last stays");
        assert_eq!(tile_after(0, 4, Key::ArrowLeft), 0, "the first stays");
        assert_eq!(tile_after(2, 4, Key::ArrowUp), 1);
        assert_eq!(tile_after(2, 4, Key::Enter), 2, "another key goes nowhere");
        // Nothing overflows or lands past the end, whatever the numbers.
        assert_eq!(tile_after(usize::MAX, usize::MAX, Key::ArrowRight), usize::MAX);
        assert_eq!(tile_after(0, 0, Key::ArrowRight), 0);
        for count in 0..6 {
            for i in 0..count {
                for key in [Key::ArrowRight, Key::ArrowLeft, Key::ArrowUp, Key::ArrowDown] {
                    assert!(tile_after(i, count, key) < count);
                }
            }
        }
    }

    #[test]
    fn an_arrow_is_beyond_the_selection_handles_and_the_four_are_alike() {
        let b = Rect::from_min_max(pos2(100.0, 100.0), pos2(250.0, 200.0));
        for gap in [ARROW_GAP, ARROW_GAP_SELECTED] {
            let all = arrows_of(b, gap);
            let by = |side: Side| all.iter().find(|a| a.side == side).copied().unwrap_or_else(|| arrows_of(b, gap)[0]);
            // A handle is 12 across, centred on the box's edge: the press area of an arrow starts past its 6 points.
            assert!(by(Side::Right).hit.left() > b.right() + 6.0 && by(Side::Left).hit.right() < b.left() - 6.0);
            assert!(by(Side::Down).hit.top() > b.bottom() + 6.0 && by(Side::Up).hit.bottom() < b.top() - 6.0);
            // They are on the middles of the sides.
            assert!((by(Side::Right).hit.center().y - b.center().y).abs() < 0.01 && (by(Side::Up).hit.center().x - b.center().x).abs() < 0.01);
            // The zone that keeps them up reaches the box's edge, so a pointer crossing from the shape finds no gap.
            assert!(by(Side::Right).zone.left() <= b.right() + 1.5 && by(Side::Right).zone.right() >= by(Side::Right).hit.right());
            assert!(by(Side::Right).zone.contains(pos2(b.right() + 2.0, b.center().y)));
        }
    }

    #[test]
    fn a_flyout_stays_inside_the_drawing_area_whatever_the_arrow_and_the_count() {
        let area = Rect::from_min_max(pos2(0.0, 0.0), pos2(400.0, 300.0));
        // Boxes at the corners and the middle of the area, and one outside it.
        for (x, y) in [(5.0, 5.0), (395.0, 5.0), (5.0, 295.0), (395.0, 295.0), (200.0, 150.0), (-500.0, 900.0)] {
            let b = Rect::from_center_size(pos2(x, y), vec2(60.0, 40.0));
            for arrow in arrows_of(b, ARROW_GAP) {
                for count in 0..=MAX_FLYOUT {
                    let (rect, tiles) = flyout_layout(&arrow, count, area);
                    assert_eq!(tiles.len(), count);
                    if area.contains_rect(b) || count == 0 {
                        // A flyout beside a box that is in the area is in the area.
                        assert!(count == 0 || area.contains_rect(rect), "{rect:?} for {b:?} {:?}", arrow.side);
                    }
                    for tile in &tiles {
                        assert!(rect.contains_rect(*tile), "a tile is inside its card: {tile:?} {rect:?}");
                    }
                    // The tiles do not overlap one another.
                    for (i, a) in tiles.iter().enumerate() {
                        assert!(tiles.iter().skip(i + 1).all(|b| !a.intersects(*b)));
                    }
                }
            }
        }
    }
}
