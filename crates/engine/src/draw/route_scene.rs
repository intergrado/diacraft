//! The page's routing scene, kept by the session and put right by pointer (the connector routing design section 3).
//!
//! **What it is.** For one page, what the router needs and the cells would otherwise be read for at every edit: each shape a route may go round, as the router's
//! [`Obstacles`], and the legs of the page's connectors, as its [`Wires`], with the glue of their ends, their setting and their style; and the page's routing cells.
//! Nothing here routes. [`Scene::put_right`] makes the scene the drawing's, [`Scene::obstacles`] and [`Scene::wires`] are what a search is given, [`Scene::set_legs`] lets a
//! route made in an edit be a wire to the next, and [`Scene::settings`] gives the router the page's clearances and spacings, its costs in drawing units and the one paper
//! inch its first area reaches past the ends (`diacraft_route::Settings::margin`).
//!
//! **How it stays right: nothing depends on it being right** (as `calc_index` does for the readers of the page). Every time it is put right, the page's top-level shapes are
//! walked and **each is compared by pointer** with the allocation the scene read: a shape whose pointer is the one the scene holds is not read again; one whose pointer differs, or
//! that is new, is read again; one that is gone is taken out. A pointer is kept as a weak reference ([`Mark`]): it keeps the allocation's address from being given to another shape,
//! holds nothing of the shape's contents, and a shape that is written, however it is written, is another allocation. So is **a shape whose cell reads another shape's** (`Sheet.2!PinX`), with no
//! list of what reads what: the scene reads the values cells hold, and the edit that moves the shape read works the one that reads it out and writes the values it gets into it, which is
//! another allocation, so the compare reads it again (the scene is put right after the page is recalculated, the connector routing design's section 3.4; `tests/links.rs` and the property
//! test hold it). So an undo, a redo, a command that failed and was put back, a file
//! opened over the drawing, or another session's document costs a walk of the page's top-level pointers and the reads of what differs, **and never a wrong scene**. The walk reads no
//! cell. **What differs reads the page whole**: the page's sheet (the layers, the routing cells and the style the page sets), the document's styles, masters and sheet, from which
//! every shape's cells are inherited; a first read; a repeated shape `ID` (the first shape holds it, and the page is read whole at every put right while the repeat lasts: always right,
//! and bounded by the caps); and work that ran out in the middle of a put right. What a `Connect` row changes reads **no shape**: the glue is read again in one pass over the rows (a
//! shape is an obstacle or not by what its facts and the glue say, [`ObstacleFacts::check`], so a glue makes one with no read of it), and a connector that was no wire because it was glued to
//! nothing and is glued now is read. How a put right goes, step by step, is in `put.rs`.
//!
//! **What an obstacle is, and what a wire.** The shapes are the (`obstacle.rs`: [`candidates_below`] keeps every two-dimensional shape of a top-level shape and of the
//! members of a group that holds 8 and not 1, **whether or not it is an obstacle now**, so that a connector glued to a shape, or a layer shown, changes what is an obstacle with no
//! read of the shape). A wire is a top-level one-dimensional shape that DiaCraft made (`User.DiaCraftRoute`) or that is glued at an end, with the legs of its route as
//! `hand::base_route_within` reads them; a plain line is none, and a connector inside a group is none (the scene is the page's top level). A leg that is neither level nor upright
//! is the router's to leave out.
//!
//! **What it reports** ([`Changed`], for cause C): the obstacles that **came, moved, changed their box, were shown or hidden, or went**, each with the
//! box the scene last settled on and the box it has now. **A setting is no change:** a shape made placeable or not by its `ObjType` (`shape.set_behavior`), or by a glue, keeps
//! the box it was settled on, and is reported with the next change of its box (item 3: "cause C asks the next time the shape changes"). The scene being put right
//! in between, by any edit, does not settle it: the box a shape is settled on moves only when its own box does. An undo, a redo and a command that failed put the scene right with the
//! document they bring and report nothing of it (`kept.rs`: [`KeptScenes::sync`]). **A put right that stops settles nothing:** when its work runs out, or a repeated `ID` makes it begin
//! again, the candidates are put back as the last put right that was complete left them (`put.rs`: `restore_baseline`), so that the next one reports from those boxes however many stops came
//! between, and the retry reports the moves the pass that met the repeat had read.
//!
//! **What it costs.** A put right walks the page's top-level pointers (at most [`Caps::shapes`] plus [`Caps::lines`]) and reads the shapes that differ; the first edit of a page reads every
//! shape (10 microseconds a shape, 100 to 150 ms for 10,000) and each edit after it pays the walk and what changed. Every pointer walked, shape looked at, shape whose kind was told by its
//! cells, `Connect` row passed and route row read is one step of the [`Work`] it is given (the edit's), **charged before the read** (a group's members one by one: a put right with one step
//! left for them reads one, however many the group holds), and a put right that is given no more work stops and says so ([`Changed::incomplete`]). **What is read is capped** ([`Caps`]), each in page order: [`MAX_SCENE_SHAPES`] two-dimensional top-level shapes and group members;
//! [`MAX_SCENE_LINES`] one-dimensional ones (a number the design did not give); [`MAX_SCENE_ROUTE_ROWS`] rows of routes, none of a route of more than [`MAX_SCENE_ROWS_EACH`]; and
//! [`MAX_SCENE_CONNECT_ROWS`] `Connect` rows (another number the design did not give). A shape past its cap is held with its pointer and read no more (**skipped**), so one that did not change
//! costs a compare however far past the cap the page goes. A page past a cap is routed with what was read, and [`Scene::take_notice`] says so once.
//!
//! **What it holds** is counted by [`Scene::held_bytes`] (the counts of what it keeps at the prices of `route_scene/price.rs`, pinned by tests and measured by an ignored one), and
//! is charged to the session's hold on the process's ledger (`kept.rs`).

#![allow(
    dead_code,
    reason = "used by the routing of connectors and by the trigger that routes again, which are not built yet: until then the scene is kept and counted by the session and read by its tests"
)]

mod kept;
mod price;
mod put;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};

use diacraft_doc::{Connect, Document, Master, Page, Shape, Sheet, Style};
use diacraft_route::{Obstacle, Obstacles, Overlap, Point, Rect, Separate, Settings, Wire, Wires, Work};

use super::obstacle::ObstacleFacts;
use super::reroute::Reroute;
use super::style::PathStyle;

#[allow(unused_imports, reason = "with_scene is the call the routing of connectors and its trigger make; until then the tests make it")]
pub(crate) use kept::{InUse, KeptScenes, with_scene};

/// The most two-dimensional top-level shapes and group members the scene reads as obstacle candidates, in page order (the snap index's `MAX_INDEXED`). A shape past it is no
/// obstacle, and the first edit that meets the cut says so once.
pub const MAX_SCENE_SHAPES: usize = 10_000;
/// The most one-dimensional top-level shapes the scene looks at, in page order: a number the design did not give, ten times the connectors an edit routes ([`super::MAX_ROUTED_PER_EDIT`]), so that the
/// table of them is bounded whatever a file holds. A connector past it has no wire, and is said.
pub const MAX_SCENE_LINES: usize = 100_000;
/// The most rows of routes the scene reads for the legs of the page's connectors, counted over each connector and its master before any is merged (the jump pass's
/// [`super::MAX_JUMP_ROWS`]). A page past it is routed with the wires read, in page order.
pub const MAX_SCENE_ROUTE_ROWS: usize = 400_000;
/// The most rows one connector's route may have to be read: a connector with more is no route a person or DiaCraft draws, has no wire, and is said.
pub const MAX_SCENE_ROWS_EACH: usize = 4_096;
/// The most `Connect` rows the scene passes over to find the glue of the page's connectors: a number the design did not give, so that the table of glue is bounded whatever a file holds.
pub const MAX_SCENE_CONNECT_ROWS: usize = 400_000;

/// What a scene reads at most (the numbers above; a test gives smaller ones to see each cap met).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Caps {
    pub shapes: usize,
    pub lines: usize,
    pub route_rows: usize,
    pub rows_each: usize,
    pub connect_rows: usize,
}

impl Default for Caps {
    fn default() -> Caps {
        Caps {
            shapes: MAX_SCENE_SHAPES,
            lines: MAX_SCENE_LINES,
            route_rows: MAX_SCENE_ROUTE_ROWS,
            rows_each: MAX_SCENE_ROWS_EACH,
            connect_rows: MAX_SCENE_CONNECT_ROWS,
        }
    }
}

/// The root's US value of a spacing, in drawing units, for one that is not a number or is below 0.
const DEFAULT_SPACING: f64 = 0.125;
/// The costs of item 6, in paper inches: a bend, a crossing of a wire's leg. The near cost is per unit of length and a lean is later.
const BEND_INCHES: f64 = 0.5;
const CROSS_INCHES: f64 = 0.25;
const NEAR_PER_UNIT: f64 = 1.0;

/// A spacing as the router is given it: a number that is not one or is below 0 is the root's 0.125, and one over one paper inch is one paper inch. `per_inch` is how many
/// drawing units one paper inch is (`DrawingScale` over `PageScale`), more than 0 and finite, or 1 for one that is not.
pub(crate) fn clamp_spacing(value: f64, per_inch: f64) -> f64 {
    let cap = if per_inch.is_finite() && per_inch > 0.0 { per_inch } else { 1.0 };
    if value.is_nan() || value < 0.0 { DEFAULT_SPACING.min(cap) } else { value.min(cap) }
}

/// What a scene could not read in full, said once ([`Scene::take_notice`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Cut {
    /// A two-dimensional top-level shape or a group member past [`Caps::shapes`] was not read.
    pub shapes: bool,
    /// A one-dimensional top-level shape past [`Caps::lines`] was not read: a connector among them has no wire.
    pub lines: bool,
    /// A connector past [`Caps::route_rows`] rows of routes has no wire.
    pub route_rows: bool,
    /// A connector of more than [`Caps::rows_each`] rows has no wire.
    pub long_routes: bool,
    /// `Connect` rows past [`Caps::connect_rows`] were not read.
    pub connects: bool,
}

impl Cut {
    /// Whether anything was left unread.
    pub(crate) fn any(&self) -> bool {
        self.shapes || self.lines || self.route_rows || self.long_routes || self.connects
    }

    /// What was left unread, in words for the command's answer.
    pub(crate) fn words(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.shapes {
            parts.push("the shapes past the first 10,000 of the page");
        }
        if self.lines {
            parts.push("the connectors past the first 100,000 of the page");
        }
        if self.route_rows || self.long_routes {
            parts.push("the routes of the connectors past the rows read for routing");
        }
        if self.connects {
            parts.push("the glue past the first 400,000 rows");
        }
        if parts.is_empty() {
            return String::new();
        }
        format!("the page was read in part for routing: {} were not read, so a route may go through what they hold", parts.join(" and "))
    }
}

/// An obstacle that came, moved, changed its box, was shown or hidden, or went: the box the scene had settled on (`None` for a shape that was no obstacle) and the box it has
/// now (`None` for one that is none now).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ChangedObstacle {
    pub id: u32,
    pub old: Option<Rect>,
    pub new: Option<Rect>,
}

/// What [`Scene::put_right`] found different, by shape `ID`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Changed {
    /// The obstacles whose box or placeability changed in the way the cause C counts (see the module's "What it reports"), in the order of their `ID`s.
    pub obstacles: Vec<ChangedObstacle>,
    /// The work ran out before the scene was put right: it is not the drawing's, and the next put right reads the page whole.
    pub incomplete: bool,
}

/// What the scene has read since it was made: the counts the tests hold a cost to (a count does not move with the load on the machine, as a time does).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Reads {
    /// Top-level pointers walked.
    pub walked: u64,
    /// Shapes looked at (a top-level shape, and each member of a group read for its members).
    pub shapes: u64,
    /// Shapes past a cap whose kind was told by the four cells that say a shape is one-dimensional, and no more of them read.
    pub peeks: u64,
    /// Connectors whose route was read.
    pub lines: u64,
    /// `Connect` rows passed over.
    pub connect_rows: u64,
    /// Rows of routes read.
    pub route_rows: u64,
    /// Put rights that read the page whole.
    pub whole: u64,
}

/// How much of a connector's route the scene has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegsState {
    /// A shape that is no connector (a plain line): its route is not read.
    Unread,
    /// The legs are read.
    Read,
    /// More rows than [`Caps::rows_each`]: no legs.
    Long,
    /// Past [`Caps::route_rows`]: no legs.
    Cut,
}

/// What the scene holds of a connector (a top-level one-dimensional shape).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ConnectorFacts {
    pub id: u32,
    /// DiaCraft made its route (`User.DiaCraftRoute`).
    pub ours: bool,
    /// It is a connector: ours or glued at an end. A plain line is none.
    pub wired: bool,
    /// The rows of its first `Geometry` section, counted over the connector and its master.
    pub rows: usize,
    pub state: LegsState,
    /// `ConFixedCode` read as a setting.
    pub setting: Reroute,
    pub style: PathStyle,
    /// The shapes its begin and its end are glued to.
    pub glue: [Option<u32>; 2],
    /// The legs of its route on the page, in order.
    pub legs: Vec<(Point, Point)>,
}

/// The identity of a shared allocation as the scene read it: a weak pointer, which keeps the allocation's address from being given to another and holds none of its contents.
/// A value behind an `Arc` that is written is another allocation (`Arc::make_mut` copies a shared one and moves one that has a weak pointer), so equal pointers are the same value.
#[derive(Clone, Debug)]
struct Mark<T>(Option<Weak<T>>);

impl<T> Mark<T> {
    fn of(arc: Option<&Arc<T>>) -> Mark<T> {
        Mark(arc.map(Arc::downgrade))
    }

    fn same(&self, now: Option<&Arc<T>>) -> bool {
        match (&self.0, now) {
            (None, None) => true,
            (Some(kept), Some(now)) => std::ptr::eq(kept.as_ptr(), Arc::as_ptr(now)),
            _ => false,
        }
    }
}

/// What every shape's cells are inherited from, as the scene read it.
#[derive(Clone, Debug)]
struct Inputs {
    page_sheet: Mark<Sheet>,
    doc_sheet: Mark<Sheet>,
    styles: Mark<Vec<Arc<Vec<Arc<Style>>>>>,
    masters: Mark<Vec<Arc<Vec<Arc<Master>>>>>,
}

impl Inputs {
    fn of(doc: &Document, page: &Page) -> Inputs {
        Inputs {
            page_sheet: Mark::of(Some(&page.sheet)),
            doc_sheet: Mark::of(Some(&doc.sheet)),
            styles: Mark::of(doc.styles.top_arc()),
            masters: Mark::of(doc.masters.top_arc()),
        }
    }

    fn same(&self, doc: &Document, page: &Page) -> bool {
        self.page_sheet.same(Some(&page.sheet))
            && self.doc_sheet.same(Some(&doc.sheet))
            && self.styles.same(doc.styles.top_arc())
            && self.masters.same(doc.masters.top_arc())
    }
}

/// The page's routing cells as the router is given them.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PageCells {
    clearance: (f64, f64),
    spacing: (f64, f64),
    separate: Separate,
    overlap: Overlap,
    per_inch: f64,
}

/// Where each connector's ends are glued, from the page's `Connect` rows.
#[derive(Clone, Debug, Default)]
struct Glue {
    /// By connector: the shape its begin and its end are glued to, the first row on either cell of an end.
    from: HashMap<u32, [Option<u32>; 2]>,
    /// The shapes a connector has an end glued to (a shape glued to itself left out).
    to: HashSet<u32>,
}

/// A shape the scene read: an obstacle when its facts and the glue say so.
#[derive(Clone, Debug)]
struct Cand {
    /// The top-level shape it was read with: its own `ID`, or its group's.
    top: u32,
    facts: ObstacleFacts,
    /// Whether it is in the router's obstacles now.
    obstacle: bool,
    /// The box the scene last settled on (`None` for a shape that was no obstacle then): the old box of the next report.
    base: Option<Rect>,
}

/// What kind of shape a top-level shape is, for the budget it is counted in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    TwoD,
    OneD,
}

/// What a top-level shape gave.
#[derive(Clone, Debug)]
enum Body {
    /// No obstacle and no connector: a guide, a shape deleted from its master, one with no box.
    Nothing,
    /// A candidate with the shape's own `ID`.
    Own,
    /// The members of a group that holds 8 and not 1, by `ID`.
    Members(Vec<u32>),
    /// A one-dimensional shape.
    Line(Box<Line>),
    /// A shape past the cap of its kind: held with its pointer, so that it costs a compare and no read until the cap leaves room for it.
    Skipped(Class),
}

/// A top-level one-dimensional shape.
#[derive(Clone, Debug)]
struct Line {
    ours: bool,
    wired: bool,
    held: usize,
    state: LegsState,
    setting: Reroute,
    style: PathStyle,
    glue: [Option<u32>; 2],
    legs: Vec<(Point, Point)>,
}

/// A top-level shape as the scene read it.
#[derive(Clone, Debug)]
struct Top {
    mark: Mark<Shape>,
    /// The put right that last saw it in the page.
    epoch: u64,
    body: Body,
    /// The shapes looked at to read it (the shape, and its members).
    cost: u32,
    /// The shapes of budget a read of its members was cut with (`Some`), so that it is valid only when as many are left.
    limit: Option<u32>,
}

/// The page's routing scene.
#[derive(Debug)]
pub(crate) struct Scene {
    page: u32,
    caps: Caps,
    inputs: Option<Inputs>,
    shapes: Mark<Vec<Arc<Vec<Arc<Shape>>>>>,
    connects: Mark<Vec<Connect>>,
    /// `Connect` rows past [`Caps::connect_rows`] were left unread at the last read of the glue.
    connects_cut: bool,
    cells: PageCells,
    glue: Glue,
    tops: HashMap<u32, Top>,
    cands: HashMap<u32, Cand>,
    obstacles: Obstacles,
    wires: Wires,
    /// Connectors whose legs [`Scene::set_legs`] changed: read again at the next put right whatever their pointer says.
    dirty: HashSet<u32>,
    epoch: u64,
    cut: Cut,
    notified: Cut,
    /// A repeated `ID` was met: the first shape holds it, and every put right reads the page whole, which is always right.
    collided: bool,
    /// The work ran out in a put right: the scene is not the drawing's.
    broken: bool,
    reads: Reads,
}

impl Scene {
    /// A scene with nothing in it, for `page`.
    pub(crate) fn new(page: u32) -> Scene {
        Scene::with_caps(page, Caps::default())
    }

    /// [`Scene::new`] with the caps of a test.
    pub(crate) fn with_caps(page: u32, caps: Caps) -> Scene {
        Scene {
            page,
            caps,
            inputs: None,
            shapes: Mark(None),
            connects: Mark(None),
            connects_cut: false,
            cells: PageCells {
                clearance: (DEFAULT_SPACING, DEFAULT_SPACING),
                spacing: (DEFAULT_SPACING, DEFAULT_SPACING),
                separate: Separate::Unrelated,
                overlap: Overlap::Related,
                per_inch: 1.0,
            },
            glue: Glue::default(),
            tops: HashMap::new(),
            cands: HashMap::new(),
            obstacles: Obstacles::new(Vec::new()),
            wires: Wires::new(Vec::new()),
            dirty: HashSet::new(),
            epoch: 0,
            cut: Cut::default(),
            notified: Cut::default(),
            collided: false,
            broken: false,
            reads: Reads::default(),
        }
    }

    /// A scene of page `page` of `doc`, read whole: what a scene put right must equal.
    pub(crate) fn read(doc: &Document, page: u32, work: &mut Work) -> Scene {
        let mut scene = Scene::new(page);
        scene.put_right(doc, page, work);
        scene
    }

    /// The obstacles a route goes round, as the router takes them.
    pub(crate) fn obstacles(&self) -> &Obstacles {
        &self.obstacles
    }

    /// The legs of the page's connectors, as the router takes them.
    pub(crate) fn wires(&self) -> &Wires {
        &self.wires
    }

    /// Makes the scene the drawing's (see the module's "How it stays right"): what differs from what the scene read, by pointer, is read again, and what is gone is taken out. Every
    /// pointer walked, shape looked at, `Connect` row passed and route row read is a step of `work`. → the obstacles that changed (see "What it reports"); work that ran out says so.
    pub(crate) fn put_right(&mut self, doc: &Document, page: u32, work: &mut Work) -> Changed {
        self.put_right_in(doc, page, work)
    }

    /// Makes `legs` the legs of connector `id`, so that the connectors routed after it in the edit see them (those routed before are wires to those routed after). The
    /// connector is read again at the next put right, whatever its pointer says. → `false` for a shape the scene does not hold as a connector (one inside a group, one that is not
    /// there, a plain line): nothing is changed.
    pub(crate) fn set_legs(&mut self, id: u32, legs: Vec<(Point, Point)>) -> bool {
        self.set_legs_of(id, legs)
    }

    /// The settings the router is given for this page (item 6): the clearances and spacings clamped, the costs in drawing units (paper inches times
    /// the page's scale), the two cells that say which wires a route keeps apart from, and the first area's margin of one paper inch. No lean: that is.
    pub(crate) fn settings(&self) -> Settings {
        let k = self.cells.per_inch;
        Settings {
            clearance: self.cells.clearance,
            spacing: self.cells.spacing,
            bend: BEND_INCHES * k,
            cross: CROSS_INCHES * k,
            near: NEAR_PER_UNIT,
            lean: None,
            separate: self.cells.separate,
            overlap: self.cells.overlap,
            margin: k,
        }
    }

    /// What the scene holds of connector `id` (a one-dimensional top-level shape, a connector or a plain line).
    pub(crate) fn connector(&self, id: u32) -> Option<ConnectorFacts> {
        let Some(Top { body: Body::Line(line), .. }) = self.tops.get(&id) else { return None };
        Some(ConnectorFacts {
            id,
            ours: line.ours,
            wired: line.wired,
            rows: line.held,
            state: line.state,
            setting: line.setting,
            style: line.style,
            glue: line.glue,
            legs: line.legs.clone(),
        })
    }

    /// What could not be read in full, the first time it is asked after it arose (and again after it was put right and arose again, or grew): `None` for nothing, and for what was said.
    pub(crate) fn take_notice(&mut self) -> Option<Cut> {
        if !self.cut.any() {
            self.notified = Cut::default();
            return None;
        }
        if self.cut == self.notified {
            return None;
        }
        self.notified = self.cut;
        Some(self.cut)
    }

    /// What the scene holds, as accounted: the counts of what it keeps at the prices of `price.rs`.
    pub(crate) fn held_bytes(&self) -> u64 {
        price::price(&self.parts())
    }

    /// What the scene read of shape `id`, an obstacle or not (a candidate).
    pub(crate) fn candidate(&self, id: u32) -> Option<&ObstacleFacts> {
        self.cands.get(&id).map(|c| &c.facts)
    }

    /// What could not be read in full now.
    pub(crate) fn cut(&self) -> Cut {
        self.cut
    }

    /// Whether the scene is the drawing's as of the last put right (the work did not run out in it).
    pub(crate) fn is_complete(&self) -> bool {
        !self.broken
    }

    /// What the scene has read since it was made.
    pub(crate) fn reads(&self) -> Reads {
        self.reads
    }

    /// The page the scene is of.
    pub(crate) fn page(&self) -> u32 {
        self.page
    }
}

/// The obstacle the router is given for a candidate.
fn obstacle_of(facts: &ObstacleFacts) -> Obstacle {
    Obstacle { id: facts.id, rect: rect_of(facts), pass_level: facts.pass_x, pass_upright: facts.pass_y }
}

/// The box of a candidate, as the router has boxes.
fn rect_of(facts: &ObstacleFacts) -> Rect {
    Rect { x0: facts.bounds.x0, y0: facts.bounds.y0, x1: facts.bounds.x1, y1: facts.bounds.y1 }
}

/// The wire the router is given for a connector's legs.
fn wire_of(id: u32, legs: &[(Point, Point)], glue: [Option<u32>; 2]) -> Wire {
    Wire { id, legs: legs.to_vec(), glued: glue }
}
