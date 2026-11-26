//! Right-angle connector routing: a route round boxes, kept off other connectors, within a work budget.
//!
//! The crate takes boxes, the legs of other connectors, two ends and settings in `f64` (x right, y up) and gives the corners of a route. It knows nothing of a
//! document: no ShapeSheet, no DiaCraft type, no threads, no clock, no dependency. The engine decides which shapes are [`Obstacle`]s and what the cells say; this crate
//! finds the route. The names say nothing of DiaCraft so that the crate can be carved into craft-libs for the other Craft apps' connectors.
//!
//! **The algorithm** is ours, from the published one: Wybrow, Marriott and Stuckey, *Orthogonal Connector Routing*, Graph Drawing 2009 (LNCS 5849). The graph is the
//! Hanan grid of the routing area: an upright line at the left and the right edge of each obstacle grown by the clearance, a level line at its bottom and its top, a
//! line of each kind through each end and the end of its stub, the area's edges and, for the wires a route keeps its spacing from, a line at the spacing on each side
//! of each leg. **The grid is never built**: its lines are two sorted lists, a vertex is a pair of indexes, and an edge is asked of the obstacle buckets when the
//! search reaches it. The search is A\* over (vertex, the way the route arrived), with the cost of a route its length, plus a bend cost for each turn (the turn onto an
//! end's stub included), a cross cost for each crossing of a wire's leg, a near cost for each unit run closer than the spacing to a wire where the rules say so, and a
//! lean cost for a first leg against the flow of a directional style. The heuristic is the Manhattan distance and the bends the ends force, a consistent lower bound, so
//! the route found is the cheapest of the grid.
//!
//! **What a route is.** Level and upright legs from the begin to the end, leaving each end along one of the ways it allows (a **stub** of the clearance across that
//! side first, shortened to what is free when another obstacle is nearer, and a turn only after it) and arriving against the end's way. It enters no obstacle grown by
//! the clearance (the grown box's edge is allowed) unless the obstacle lets it through (`pass_level`, `pass_upright`) or is exempt for this request. A search that finds
//! no route at the full clearance is made again at half and then at none; one that finds no way is made again on an area three times as wide. Then the route is centred
//! (the middle leg of a Z lies half way) and tidied. A route of more than [`MAX_ROUTE_POINTS`] points is refused.
//!
//! **Costs of wires.** A leg of another connector costs [`Settings::cross`] when the route crosses it, and [`Settings::near`] for each unit the route runs along it
//! closer than the spacing, when [`Separate`] and [`Overlap`] say that wire is one to keep apart from: [`Overlap::Never`] every wire, [`Overlap::Any`] none, and
//! [`Overlap::Related`] every wire but the related ones (those that share a shape their ends are glued to), of which [`Separate::Unrelated`] asks the unrelated,
//! [`Separate::All`] all and [`Separate::None`] none.
//!
//! **Bounds** (every input is hostile). Every number is tested before any arithmetic: a box or a leg that is not finite, or lies beyond [`MAX_COORD`], is left out and
//! counted; an end that is such a point is refused. Settings are clamped. Every loop charges a [`Work`] before it runs: states, heap pushes, bucket entries and lines.
//! A route takes at most [`MAX_ROUTE_WORK`] steps of its own and whatever is left of the work it is given; an area holds at most [`MAX_AREA_OBSTACLES`] obstacles and
//! [`MAX_AREA_WIRES`] wires and a grid at most [`MAX_GRID_LINES`] lines on an axis, and a search reaches at most [`MAX_EXPANSIONS`] states; each past its limit is
//! [`Outcome::OutOfWork`]. A query that visits more than [`MAX_QUERY_VISITS`] entries is cut, and the edge it asked about counts as blocked. The same input gives the same
//! route on every machine: lines are sorted and merged by one rule, ties are broken by the order the states were reached, no hash map's order is in any result.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod buckets;
mod centre;
mod geom;
mod grid;
mod search;
mod work;

pub use buckets::{MAX_BUCKETS_PER_BOX, MAX_QUERY_VISITS, MAX_WIRE_LEGS, MAX_WIRES_LEGS, Obstacle, Obstacles, Query, Wire, Wires, bucket_of};
pub use geom::{DIRS, Dir, DirSet, EPS, MAX_COORD, Point, Rect, turn_cost};
pub use search::{
    MAX_AREA_OBSTACLES, MAX_AREA_WIRES, MAX_EXPANSIONS, MAX_GRID_LINES, MAX_ROUTE_POINTS, MAX_ROUTE_WORK, enters_any, forced_bends, route,
    state_index,
};
pub use work::{Counts, Step, Work};

/// One end of a connector.
#[derive(Clone, Debug, PartialEq)]
pub struct End {
    /// Where the route starts or finishes: the route's first or last point, exactly.
    pub at: Point,
    /// The ways the route may leave from here: one, or any.
    pub ways: DirSet,
    /// The obstacle this end stands on (the shape it is glued to), whose clearance the stub is let through, when it is one.
    pub stands_on: Option<u32>,
}

/// What to route.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub begin: End,
    pub end: End,
    /// The obstacles that are none for this request: the shapes whose box holds an end, or the point a stub reaches.
    pub exempt: Vec<u32>,
    /// The shapes the two ends are glued to, to tell which wires are related.
    pub glued: [Option<u32>; 2],
}

/// Which wires a route keeps its spacing from (`LineAdjustFrom`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Separate {
    /// From the wires it is not related to.
    Unrelated,
    /// From every wire.
    All,
    /// From none.
    None,
}

/// Which wires a route may lie on (`LineAdjustTo`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlap {
    /// On the related ones.
    Related,
    /// On none: it keeps its spacing from every wire.
    Never,
    /// On any.
    Any,
}

/// The settings of a route, in the units of the boxes. The router clamps each: a length or a cost below 0 or not finite is 0, and a length above [`MAX_COORD`] is that.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// The gap kept from an obstacle: beside upright legs and beside level legs.
    pub clearance: (f64, f64),
    /// The gap kept from a wire: beside upright legs and beside level legs.
    pub spacing: (f64, f64),
    /// The cost of a turn.
    pub bend: f64,
    /// The cost of crossing a wire's leg.
    pub cross: f64,
    /// The cost of each unit run closer than the spacing to a wire that is kept apart from.
    pub near: f64,
    /// A flow: a first leg out of the begin against it, where the end lies along it, costs this much more.
    pub lean: Option<(Dir, f64)>,
    pub separate: Separate,
    pub overlap: Overlap,
    /// How far the first area reaches past the ends and their stubs (one paper inch in the engine).
    pub margin: f64,
}

impl Default for Settings {
    /// The values of a drawing in inches: the clearance and spacing of the root's US sheet and the costs of the engine.
    fn default() -> Settings {
        Settings {
            clearance: (0.125, 0.125),
            spacing: (0.125, 0.125),
            bend: 0.5,
            cross: 0.25,
            near: 1.0,
            lean: None,
            separate: Separate::Unrelated,
            overlap: Overlap::Related,
            margin: 1.0,
        }
    }
}

/// The clearance a route keeps from the obstacles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clearance {
    Full,
    Half,
    None,
}

/// The answer to a request.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// A route: its corners, at most [`MAX_ROUTE_POINTS`], the first the begin and the last the end. `cut` says a query was cut while it was searched, so another route
    /// may be cheaper.
    Routed { points: Vec<Point>, clearance: Clearance, cut: bool },
    /// There is no way: an end is walled in, or the boxes leave none.
    NoWay,
    /// The work (or a cap of an area) ran out first.
    OutOfWork,
    /// The request was refused, in words.
    Refused(&'static str),
}
