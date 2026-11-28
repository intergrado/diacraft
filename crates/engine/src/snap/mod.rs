//! Snapping: where a shape, or a point, that is being dropped, moved, resized or glued comes to rest.
//!
//! [`snap`] is a pure function of the document, a [`SnapPage`] (what a snap reads of a page once), the document's [`SnapSettings`], the
//! zoom, and what is being moved: a point or a box, and the `ID`s of the shapes that move with it. It gives the distance to add, and what each
//! axis snapped to ([`Snapped`]). The commands (`shape.drop`, `shape.move`, `shape.resize`, `connector.add`, `connector.reconnect`) and the
//! window's drags call the same function, so they cannot disagree. Everything is in **paper inches**, y up, the unit of both; the page's cells
//! (in drawing units) are divided once, in `grid` and `page`.
//!
//! **The targets**, from the document's settings (`settings`; snap on or off, choice of snap targets, dynamic grid):
//!
//! * snap to the **grid** and the **ruler's subdivisions** (`grid`): lines at the origin plus multiples of a pitch that follows the zoom and the
//!   drawing's scale; **guides** (`page`): the page's guide shapes; the **alignment boxes** of other shapes (their edges and middles) and, with the
//!   **dynamic grid**, also the page margins and the spacing between shapes (a gap equal to another gap); and the **intersections** of shape geometry;
//! * glue (for the end of a connector): a **connection point**, a **vertex**, a **handle** (a corner or the middle of a side of a shape's box), a
//!   point on the **outline** (geometry), or a guide.
//!
//! **Strength** (snap strength): a target pulls from as many screen pixels as its [`Strength`] says
//! (the document's own, six numbers from 0 to 999, `settings`), and where several pull, the stronger kind wins over a nearer weaker one: connection points, vertices,
//! handles, intersections, guides, outlines, the alignment boxes and margins and spacings, the grid, the ruler (ours). **The grid and the ruler pull from
//! their own strengths** (5 and 4 pixels at first): a line farther than that from the shape leaves it where the pointer put it. (Before a change they pulled
//! from any distance, the nearest line, and the alignment, margins and spacings pulled from 6 pixels: they are the Extensions slider's, 13 at first, R81.)
//!
//! **`InhibitSnap`**: a page with it 1 snaps to its ruler and grid and to nothing else of the page (guides, other
//! shapes, spacing); glue to a connection point is not a snap and stays (ours).
//!
//! **Cost.** The grid, the ruler and the guides cost arithmetic. The targets that are other shapes use [`SnapPage::shapes`], an index of the page made
//! once (bounded: [`index::MAX_INDEXED`] shapes), and a query looks at the shapes near the subject, at most [`MAX_CANDIDATES`], whatever the page holds.
//! The counts are in the result ([`Snapped::examined`], [`Snapped::candidates`]) for the tests that hold a cost to a number.
//!
//! **Glue to guides** ([`hold`]): a box being placed that is pulled to a guide on an axis, with the Glue switch and glue to guides on, is held
//! to it: the result names each guide it is held to ([`Snapped::holds`]), and the commands write the hold ([`hold::hold_shapes`]), so a shape follows
//! its guide by recalculation. A point (a connector's end) is not held.
//!
//! **Not done**: shape extension lines and isometric angle lines (`SnapExtensions`, `SnapAngles`: P2), guide points (P2), snapping to the shapes of a
//! background page, the members of a group (a group is one box), and layers' own snap settings (`docs/gaps.md`).

mod adapter;
pub mod dropped;
#[cfg(test)]
mod equal;
mod geometry;
mod grid;
pub mod guides;
pub mod hold;
pub mod index;
pub mod label;
#[cfg(test)]
mod oracle;
#[cfg(test)]
mod turned_cost;
// The object snaps live in `craft-snap` now: this file is the oracle's (`oracle.rs`) and nothing else's.
#[cfg(test)]
mod osnap;
mod page;
pub mod reads;
pub mod settings;
pub mod subject;

use diacraft_doc::Document;

pub use dropped::{DropMaster, DropPreview, drop_preview, dropped_points, dropped_points_of_file, size_on_paper};
pub(crate) use geometry::{Budget, Purpose, outline_for};
pub use geometry::{
    MAX_BOX_MEMBERS, MAX_BUILD_ELEMENTS, MAX_BUILD_POINTS, MAX_BUILD_WORK, MAX_CONSTRUCTOR_BYTES, OutlineWork, PAGE_BUILD_BYTES, PAGE_BUILD_ELEMENTS,
    PAGE_BUILD_POINTS, PAGE_BUILD_WORK,
};
pub use grid::{Axis, GridCells, Marks};
pub use hold::Hold;
pub use index::{P, Rect, ShapeIndex};
pub use page::{GuideLine, MAX_GUIDES, Margins, SlantedGuide, SnapPage};
pub use reads::{Counts, Kind, MAX_KEPT_BYTES, PageReads, counts, make_ready, reset_counts};
pub use settings::{
    Home, OPTIONS, OptionDef, STRENGTH_MAX, STRENGTHS, SnapSettings, Strength, StrengthDef, Word, WordChange, blank_drawing, strength_pixels,
};
pub use subject::{
    BoxAt, EachBounds, MAX_OWN_SHAPES, MAX_SNAPPED_SHAPES, SubjectBounds, bounds_each, bounds_of, box_of, frame_of_shapes, is_square, moved_edges,
    own_points, own_points_each, pin_of, resized_by, snap_resize, snap_resize_ext, snap_resize_set, snaps_in_its_own_axes,
};

/// The most shapes near a subject that a snap looks into.
pub const MAX_CANDIDATES: usize = 64;

/// The most library queries one call of [`snap`] makes: a box or a point is asked, then, when it snapped on one axis only, the other axis alone, and, when the
/// moving shapes have places of their own, the outlines apart; a resize that moves both sides of an axis is asked side by side. The engine's budgets (points,
/// pieces, pairs, outlines, connection rows) are the call's, not each query's.
pub const MAX_LIBRARY_QUERIES: usize = 3;

/// The most times a call reads the index for the shapes near a window: the library's own bound for one query (`craft_snap::MAX_GATHERS`: five windows to find
/// the candidates and the rest to check one on the geometry it would give) times [`MAX_LIBRARY_QUERIES`]. Each is cut at [`index::MAX_VISITS`] bucket entries,
/// so a call visits at most this many times that, whatever the page holds. (It was 3 before the library: once for the region of the subject and once along each
/// axis for the spacing.)
pub const MAX_GATHERS: usize = 16 * MAX_LIBRARY_QUERIES;

/// The most straight pieces of outline whose crossings a snap works out: the pieces near the subject, from the candidate shapes.
pub const MAX_PIECES: usize = 96;

/// How far from a box or a point, in paper inches, another shape may be and still line up with it (the dynamic grid and the alignment boxes): a shape
/// across the page is not what a drag lines up with. At the most [`MAX_CANDIDATES`] of the shapes within it are looked into, the nearest.
pub const ALIGN_REACH: f64 = 2.0;

/// How far from a box, in paper inches, the shapes whose gaps it matches may be: the neighbour, and the one beyond it.
pub const SPACING_REACH: f64 = 4.0;

/// The screen points per paper inch when none is given: 100% zoom (`ui-egui` `POINTS_PER_INCH`).
pub const DEFAULT_ZOOM: f64 = 96.0;

/// Whether a snap takes `zoom`, the screen points to a paper inch: a finite number above 0 **at which the strongest pull ([`STRENGTH_MAX`] points) is a distance on the page**. Below about
/// 5.6E-306 points to an inch the pull in inches is past the largest number, and an alignment, whose reach is at least a pull ([`alignment_reach`]), reached nothing: the
/// commands refuse such a zoom in words, and a query given one takes [`DEFAULT_ZOOM`], as for a zoom that is no number.
pub fn takes_zoom(zoom: f64) -> bool {
    zoom.is_finite() && zoom > 0.0 && (f64::from(STRENGTH_MAX) / zoom).is_finite()
}

/// How far from a subject, in paper inches, the shapes it lines up with may be at `zoom`: [`ALIGN_REACH`], or the strongest of the pulls of the points, the outlines and the guides when
/// that is farther, and the `overhang` of the places of the shapes that move with the subject beyond its box.
pub(crate) fn alignment_reach(strength: &Strength, zoom: f64, overhang: f64) -> f64 {
    let pull = f64::from(strength.points.max(strength.geometry).max(strength.guides)) / zoom;
    ALIGN_REACH.max(pull) + overhang
}

/// What is being moved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Subject {
    /// A place: a drawing tool's point, the end of a connector.
    Point(P),
    /// A box: the alignment box of the shapes that are being dropped or moved. Its edges and its middle (three anchors on each axis) are what snap.
    Box(Rect),
    /// A box whose edges are being dragged (a resize): only the edges named snap, and the middle does not.
    Edges(Rect, Edges),
}

/// The edges of a box that a resize drags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edges {
    pub left: bool,
    pub bottom: bool,
    pub right: bool,
    pub top: bool,
}

/// Whether the subject is being placed or being connected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    /// A shape or a point put down: the snap targets (grid, ruler, guides, alignment boxes, dynamic grid, intersections).
    Place,
    /// The end of a connector: the glue targets as well (connection points, vertices, handles, outlines, guides).
    Connect,
}

/// What a snap is asked.
#[derive(Clone, Copy, Debug)]
pub struct Query<'a> {
    pub subject: Subject,
    pub using: Use,
    /// The `ID`s of the shapes that are moving, which are never a target.
    pub moving: &'a [u32],
    /// The screen points to a paper inch: the zoom (it sets a variable grid's pitch, and how far a target pulls in inches). [`DEFAULT_ZOOM`] if it is
    /// not a positive number.
    pub zoom: f64,
    /// For a box: places of the shapes that move (their connection points, at most [`MAX_OWN_POINTS`] are used) that may be pulled to the connection points,
    /// vertices, outlines and intersections of other shapes, as the corners and middles of the box are. Empty for a point.
    pub own_points: &'a [P],
}

/// What a hit snapped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Ruler,
    Grid,
    Guide,
    /// The edge or the middle of another shape's alignment box.
    Alignment,
    /// A margin of the page (the dynamic grid).
    PageMargin,
    /// A gap equal to another (the dynamic grid): the subject sits as far from a shape as that shape does from another.
    Spacing,
    ConnectionPoint,
    Vertex,
    Handle,
    Outline,
    Intersection,
    /// A width or height equal to another shape's (a resize, row 6): the box was pulled to a length another shape has.
    Size,
    /// An angle equal to another shape's (a turn, row 7).
    Angle,
    /// A whole number of the rotation handle's steps (a turn).
    AngleStep,
}

impl Target {
    /// The word a command gives it (`snap.at`, the `snap` result of the editing commands).
    pub fn name(self) -> &'static str {
        match self {
            Target::Ruler => "ruler",
            Target::Grid => "grid",
            Target::Guide => "guide",
            Target::Alignment => "alignment",
            Target::PageMargin => "page_margin",
            Target::Spacing => "spacing",
            Target::ConnectionPoint => "connection_point",
            Target::Vertex => "vertex",
            Target::Handle => "handle",
            Target::Outline => "outline",
            Target::Intersection => "intersection",
            Target::Size => "size",
            Target::Angle => "angle",
            Target::AngleStep => "angle_step",
        }
    }
}

/// Which part of the subject was pulled: a point, or on a box the low edge (left, bottom), the middle, or the high edge (right, top).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Point,
    Low,
    Middle,
    High,
}

impl Anchor {
    /// The word a command gives it.
    pub fn name(self) -> &'static str {
        match self {
            Anchor::Point => "point",
            Anchor::Low => "low",
            Anchor::Middle => "middle",
            Anchor::High => "high",
        }
    }
}

/// A shape's own box on the page, turned about its middle: what its own sides, corners and proportions are made of. A call that is told the
/// subject's frame ([`Extras::frame`]) aligns its own sides with the like sides of shapes turned the same way, and with slanted guides parallel to them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OwnFrame {
    /// The middle of the box, in paper inches.
    pub centre: P,
    /// The box's size along its own x axis, which is the direction `angle` points, and along its own y axis, a quarter turn from it (counterclockwise).
    pub width: f64,
    pub height: f64,
    /// The direction of the box's own x axis, in radians (`atan2` of its x edge: a flipped shape's is turned a half turn).
    pub angle: f64,
}

/// One thing the subject snapped to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub target: Target,
    /// The axis the subject was pulled along: `None` for a hit on a place, which pulled both, and for a hit on a line that is not along an axis (see `turn`).
    pub axis: Option<Axis>,
    /// The line a hit is on when it is not along a page axis, a side of a turned shape or a slanted guide: the angle of its normal `n`, in radians within `[0, π)`.
    /// Then `axis` and `at` are `None` and `line` is the line's offset along that normal (`n · p` for the points `p` of the line, `n = (cos normal, sin normal)`). A side of a
    /// shape turned 30 degrees along its own y axis has a normal of 120 degrees and runs along 30. `None` for a line along an axis and for a place.
    pub normal: Option<f64>,
    /// Where the line is (an axis hit), in paper inches along `axis`; for a hit on a place, its x.
    pub line: f64,
    /// For a hit on a place: where it is.
    pub at: Option<P>,
    pub anchor: Anchor,
    /// The shape it snapped to (or the guide).
    pub shape: Option<u32>,
    /// For a connection point: its number from 0.
    pub point: Option<u32>,
    /// For an alignment, how far along the line the aid reaches: from where to where, on the other axis, in paper inches. For a spacing, the gap
    /// between the shape and the subject along the axis, once snapped.
    pub span: Option<(f64, f64)>,
    /// For a spacing, the size of the equal gap.
    pub gap: Option<f64>,
}

impl Hit {
    /// Whether the hit is on a place, which pulled both axes at once: it has no axis and no line along a direction of its own.
    pub fn is_place(&self) -> bool {
        self.axis.is_none() && self.normal.is_none()
    }
}

/// Something to draw about a snap, derived from where the box lands: it holds on the geometry the snap returns (the library never gives a mark that does not).
#[derive(Clone, Debug, PartialEq)]
pub enum SnapMark {
    /// A line the landed box lies on: upright at x = `at` (`Axis::X`) or level at y = `at` (`Axis::Y`), drawn from `from` to `to`. `targets` are the kinds of
    /// target on it and `count` how many shapes (or guides) it stands for; `shape` the first. The segment runs from the box to the nearest shape on each side of
    /// it (the adoption's `line_span`), however many more lie beyond. **`normal`**: a line along one of a turned shape's own sides or a slanted guide is drawn
    /// from `from` to `to` all the same, and `normal` is the angle of its normal in radians within `[0, π)` (`at` is then its offset along that normal, and `axis` the page
    /// axis the normal is nearer to); `None` for a line along an axis.
    Line { axis: Axis, normal: Option<f64>, at: f64, from: P, to: P, targets: Vec<Target>, shape: Option<u32>, count: u32, hit: bool },
    /// A gap equal to another, drawn from `from` to `to` across the middle of the two shapes' overlap, with its length `value` and its `label` (the
    /// page's drawing units, at most three places). Gaps that run on from one another share a `chain` number; `own` says it is one of the box's own gaps. **`normal`**:
    /// a gap of a row of shapes turned alike runs along their own x or y axis, and `normal` is the angle of the direction the row runs in, in radians within `[0, π)`
    /// (the row's normal is the one the gap's own sides lie along; `axis` is the page axis nearer to the row); `None` for a gap along an axis.
    Gap { axis: Axis, normal: Option<f64>, from: P, to: P, value: f64, label: String, chain: u16, own: bool, hit: bool },
    /// A place of the box that lies on a target place.
    Place { at: P, target: Target, shape: Option<u32>, point: Option<u32>, hit: bool },
    /// A width or height of the resized box that another shape has too: `side` is the box's side, `matched` the other shape (its `ID` and side) that is nearest
    /// among those with that length, and `count` how many have it. `value` is the length and `label` its text.
    Size { value: f64, label: String, side: (P, P), matched: Option<(u32, (P, P))>, count: u32, hit: bool },
    /// The angle a turn comes to equals another shape's (`matched`) or is a whole number of steps (`step`, in degrees); `label` is its text.
    Angle { pivot: P, degrees: f64, label: String, matched: Option<u32>, step: Option<f64>, hit: bool },
}

/// What a connector's end landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glue {
    ConnectionPoint { shape: u32, point: u32 },
    Vertex { shape: u32 },
    Handle { shape: u32 },
    Outline { shape: u32 },
}

impl Glue {
    /// The shape the end landed on.
    pub fn shape(self) -> u32 {
        match self {
            Glue::ConnectionPoint { shape, .. } | Glue::Vertex { shape } | Glue::Handle { shape } | Glue::Outline { shape } => shape,
        }
    }
}

/// What a snap gave.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapped {
    /// What to add to the subject, in paper inches.
    pub dx: f64,
    pub dy: f64,
    /// What it snapped to: one hit for each axis that moved, or one for a place.
    pub hits: Vec<Hit>,
    /// What a connector's end landed on, for [`Use::Connect`].
    pub glue: Option<Glue>,
    /// The guides a box being placed is held to: a hit on a guide, for [`Use::Place`] of a box, with the Glue switch and glue to
    /// guides on. At most one for each axis; the commands write them with [`hold::hold_shapes`].
    pub holds: Vec<Hold>,
    /// What to draw, from where the box lands (rows 5 and 8 of the design): every line it lies on, every equal gap, the places it meets, each with whether it was
    /// snapped to (`hit`). The window and `snap.marks` of the commands read these.
    pub marks: Vec<SnapMark>,
    /// For a resize that keeps the proportions: the box the dragged one comes to, which the snap and the ratio together decide (the edges' own changes are not
    /// enough, since the other axis follows).
    pub landed: Option<Rect>,
    /// For a resize of a turned or flipped shape (a call that was given [`Extras::frame`] and `Subject::Edges`): the box it comes to, in the shape's own axes. `dx` and `dy` are
    /// 0 for it, since no page axis is what its sides move along; [`snap_resize_ext`] works the box the command writes out of this.
    pub frame: Option<OwnFrame>,
    /// For a turn: the angle the shape comes to, in radians.
    pub turned: Option<f64>,
    /// Whether something that could have been a snap target was left out or measured by less than it is, for what it costs: the page holds more shapes than the index reads
    /// A query met a cap (more shapes near than it looks into, outlines, pieces, rows), or a turned shape that is a target is lined up by the box round its corners
    /// because the page's budget could not pay for the box round its paths (a shape that draws no path is not that).
    pub truncated: bool,
    /// How many of the moving shapes were aligned by the box round their corners where the box round their paths was meant: a turned shape whose paths the
    /// subject's budget could not pay for, one that draws no path, or a group of too many members. Set by the command that made the subject's box (`bounds_of`).
    pub corner_boxes: usize,
    /// The shapes the query looked at, and the shapes it took as candidates: the work it did, whatever the page holds.
    pub examined: usize,
    pub candidates: usize,
    /// The bucket entries the queries visited (a shape in several buckets once for each) and the target points they weighed: the rest of the work, with the
    /// rows merged that `diacraft_doc::work::rows_merged` counts. Each is bounded by a cap whatever the page holds, and a query that hits a cap is `truncated`.
    pub visited: usize,
    pub points: usize,
    /// The straight pieces of outline weighed (the elements of the paths read, the pieces the curves were estimated to make, the pairs of pieces crossed).
    pub pieces: usize,
    /// The straight pieces the curves of the outlines were estimated to flatten into (`osnap::Pieces::charged`), worked out before any curve was flattened:
    /// a curve too big or too far from the pointer is not charged and is not flattened.
    pub flattened: usize,
    /// What building the outlines the queries read cost: constructor bytes and points, path elements, spline work, and the sections left out for their cost. The box of the
    /// moving shapes is paid for by another budget, and its cost is [`SubjectBounds::cost`].
    pub outline: OutlineWork,
}

impl Snapped {
    /// The place `p` after the snap.
    pub fn apply(&self, p: P) -> P {
        (p.0 + self.dx, p.1 + self.dy)
    }
}

/// The most target points one query weighs, the most straight pieces of outline it tests, the shapes whose outlines it reads, the rows of a `Connection`
/// section it reads (a section with more is skipped whole), the places of the moving shapes it pulls, and the pairs of outlines whose crossings it works out.
/// Each is a cap on work: a query that meets one is `truncated`.
pub const MAX_TARGET_POINTS: usize = 4_096;
pub const MAX_TARGET_PIECES: usize = 16_384;
pub const MAX_OUTLINE_SHAPES: usize = 16;
pub const MAX_CONNECTION_ROWS: usize = 256;

/// The most rows of `Connection` sections that a query merges, over all the shapes it reads connection points of (the rows are counted over the shape, its
/// master and the styles and charged before any is merged; a shape whose section the query cannot pay for is read by its box only, and the query is `truncated`).
pub const MAX_CONNECTION_ROWS_PER_QUERY: usize = 1_024;
pub const MAX_OWN_POINTS: usize = 32;
pub const MAX_PATH_PAIRS: usize = 64;

/// What a call may be told besides the query: the commands' new `snap` words (the window sends the same).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Extras {
    /// A move that keeps to one page axis (a Shift-drag): the other axis moves not at all, to the bit, and the free axis snaps to everything a free
    /// move snaps to. A pull on the locked axis is ignored. Places (connection points, vertices, outlines) do not pull a locked move: the library does not
    /// offer a place to a move on a line (`docs/gaps.md`, F16).
    pub lock: Option<Axis>,
    /// The window's visible part of the page, `[x0, y0, x1, y1]` in paper inches: a size match looks only at shapes whose bounds meet it.
    /// `None`: every indexed shape.
    pub view: Option<[f64; 4]>,
    /// A resize that keeps the shape's own proportions (`keep: "proportions"`): from a corner the opposite corner stays, from a side the
    /// opposite side stays and the other axis grows about its middle, and the snap works inside the proportions.
    pub keep: bool,
    /// The width over the height a resize keeps: filled by [`snap_resize_ext`] from the shape's box when `keep` is asked; a call of [`snap_ext`] that has `keep`
    /// but no ratio keeps nothing.
    pub ratio: Option<f64>,
    /// An arrow key (`nudge: {tick}`): the box goes to the next place it can snap to along the axis, however far, and with none by `tick`.
    pub nudge: Option<Nudge>,
    /// The subject's own frame, when it is one shape turned by other than whole quarter turns ([`subject::frame_of_shapes`] makes it for a move and
    /// a resize makes its own). With a box (a move, a nudge) the query's rect is the box the shape aligns by on the page (round its paths) and the frame is where its own box
    /// is: the shape's own sides line up with the like sides of other shapes turned the same way and with slanted guides parallel to them, beside the rect's page lines. With
    /// `Subject::Edges` the frame is the box the resize drags to and the flags of `Edges` are its own sides, `left` and `right` the low and high side along its x axis, `bottom`
    /// and `top` along its y axis (a handle is mapped to its side through the shape's corners by [`snap_resize_ext`]); the answer's box is [`Snapped::frame`].
    pub frame: Option<OwnFrame>,
}

/// One press of an arrow key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nudge {
    /// The page axis the key moves along.
    pub axis: Axis,
    /// Whether it moves toward larger x or y (right, up).
    pub forward: bool,
    /// How far it moves when nothing is ahead: one ruler tick, in paper inches.
    pub tick: f64,
}

/// A turn by the rotation handle: the pointer's angle, and how far it is from the pin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turn {
    /// The pin the shape turns about, in paper inches: where the angle mark is drawn.
    pub pivot: P,
    /// The angle the pointer gives, in radians.
    pub angle: f64,
    /// How far the pointer is from the pin, in paper inches.
    pub radius: f64,
    /// The screen points to a paper inch.
    pub zoom: f64,
}

/// The step the rotation handle snaps to, in degrees, by how far the pointer is from the pin in screen points (ours): the inventory says
/// only that the further the pointer from the shape, the finer the steps, so they are 15 degrees near the shape, 5 further out, 1 further, and a tenth past that.
pub fn rotation_step(points_from_pin: f64) -> f64 {
    if points_from_pin < 110.0 {
        15.0
    } else if points_from_pin < 190.0 {
        5.0
    } else if points_from_pin < 300.0 {
        1.0
    } else {
        0.1
    }
}

/// Snaps a turn by the rotation handle (spec): the angle comes to another shape's angle when within the pull of 6 screen points at the
/// pointer (the angle whose tangent is 6 over the distance in points), which beats the step; else to the step of [`rotation_step`]. The match is on a full turn,
/// and needs the Snap switch; the step does not.
pub fn snap_turn(doc: &Document, page: &SnapPage, settings: &SnapSettings, moving: &[u32], turn: Turn) -> Snapped {
    adapter::snap_turn(doc, page, settings, moving, turn)
}

/// [`snap`] with the `Extras` of a command.
pub fn snap_ext(doc: &Document, page: &SnapPage, settings: &SnapSettings, query: &Query<'_>, extras: Extras) -> Snapped {
    adapter::snap_ext(doc, page, settings, query, extras)
}

/// Snaps the subject of `query` on `page` of `doc`, by `settings` (the options, and the [`Strength`] of each kind of target): the page's targets as `craft-snap` holds them
/// (`adapter.rs`), and what it gives told in this module's words. The commands and the window's drags call this one function, so they cannot disagree.
pub fn snap(doc: &Document, page: &SnapPage, settings: &SnapSettings, query: &Query<'_>) -> Snapped {
    adapter::snap(doc, page, settings, query)
}

#[cfg(test)]
mod zoom_tests {
    use super::*;

    /// Below the least zoom a snap takes, the reach of an alignment was no number (an infinity, which `craft_snap::Reach::Document` reads as no reach), so nothing
    /// lined up; from it up the reach is a distance for the strongest pulls. Such a zoom is now refused by the commands and stands for the default in a query.
    #[test]
    fn a_zoom_is_taken_exactly_where_the_strongest_pull_is_a_distance_on_the_page() {
        let strongest = Strength { points: STRENGTH_MAX, geometry: STRENGTH_MAX, guides: STRENGTH_MAX, ..Strength::default() };
        let reach = alignment_reach(&strongest, 1e-306, 0.0);
        assert!(
            reach.is_infinite() && !craft_snap::Reach::Document(reach).within(ALIGN_REACH, 1e-306),
            "at 1E-306 the reach is no number and reaches nothing"
        );
        for refused in [1e-306, 5.5e-306, 0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(!takes_zoom(refused), "{refused:e}");
        }
        for taken in [5.6e-306, 1e-305, 1e-300, 3.0, DEFAULT_ZOOM, 1e300, f64::MAX] {
            assert!(takes_zoom(taken), "{taken:e}");
            let reach = alignment_reach(&strongest, taken, 0.0);
            assert!(reach.is_finite() && reach >= ALIGN_REACH && craft_snap::Reach::Document(reach).within(ALIGN_REACH, taken), "{taken:e}");
        }
    }
}
