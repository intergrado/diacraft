//! The snapping of as it was before the shared library: kept, **test-only**, as the oracle that the adapter (`adapter.rs`) is
//! held equal to on random pages (`equal.rs`). Nothing in a build of the engine calls it.

use diacraft_doc::Document;

use super::geometry::outline;
use super::index::{Gathered, Item};
use super::{
    ALIGN_REACH, Anchor, Axis, DEFAULT_ZOOM, Glue, Hit, Hold, MAX_CANDIDATES, MAX_CONNECTION_ROWS, MAX_CONNECTION_ROWS_PER_QUERY, MAX_OUTLINE_SHAPES,
    MAX_OWN_POINTS, MAX_PATH_PAIRS, MAX_TARGET_PIECES, MAX_TARGET_POINTS, P, Query, Rect, SPACING_REACH, SnapPage, SnapSettings, Snapped, Strength,
    Subject, Target, Use, geometry, osnap,
};
use crate::draw::Reader;
use kurbo::{Point, Rect as KRect};

/// A target found for the subject: how strongly it ranks, how far it is, and what it moves the subject by.
#[derive(Clone, Copy, Debug)]
struct Found {
    rank: u8,
    dist: f64,
    delta: (f64, f64),
    hit: Hit,
    glue: Option<Glue>,
}

// The order kinds of target win in, strongest first.
const RANK_CONNECTION_POINT: u8 = 0;
const RANK_VERTEX: u8 = 1;
const RANK_HANDLE: u8 = 2;
const RANK_INTERSECTION: u8 = 3;
const RANK_GUIDE: u8 = 4;
const RANK_OUTLINE: u8 = 5;
const RANK_ALIGNMENT: u8 = 6;
const RANK_GRID: u8 = 7;
const RANK_RULER: u8 = 8;

/// Whether `a` beats `b`: a stronger kind, or the same kind and nearer.
fn better(a: &Found, b: &Found) -> bool {
    a.rank < b.rank || (a.rank == b.rank && a.dist < b.dist)
}

/// Keeps the best of what it is given, and, for the harness, every offer made (`log`): a tie is a result the library may choose in place of this one.
#[derive(Default)]
struct Best {
    best: Option<Found>,
    log: Vec<Found>,
}

impl Best {
    fn offer(&mut self, found: Option<Found>) {
        if let Some(found) = found {
            self.log.push(found);
            if self.best.as_ref().is_none_or(|b| better(&found, b)) {
                self.best = Some(found);
            }
        }
    }
}

/// What the last [`snap`] of this thread was offered, for `equal.rs`: the places, and the lines of each axis.
#[derive(Default)]
pub(super) struct Trace {
    pub place: Vec<Offer>,
    pub axes: [Vec<Offer>; 2],
}

/// One thing offered to the best: its rank, its distance, what it moves the subject by, and the hit.
pub(super) type Offer = (u8, f64, (f64, f64), Hit);

thread_local! {
    pub(super) static TRACE: std::cell::RefCell<Trace> = std::cell::RefCell::new(Trace::default());
}

fn logged(log: &[Found]) -> Vec<Offer> {
    log.iter().map(|f| (f.rank, f.dist, f.delta, f.hit)).collect()
}

/// The anchors of the subject on one axis: which part and where.
fn anchors(subject: &Subject, axis: Axis) -> Vec<(Anchor, f64)> {
    match (subject, axis) {
        (Subject::Point(p), Axis::X) => vec![(Anchor::Point, p.0)],
        (Subject::Point(p), Axis::Y) => vec![(Anchor::Point, p.1)],
        (Subject::Box(r), Axis::X) => vec![(Anchor::Low, r.x0), (Anchor::Middle, (r.x0 + r.x1) / 2.0), (Anchor::High, r.x1)],
        (Subject::Box(r), Axis::Y) => vec![(Anchor::Low, r.y0), (Anchor::Middle, (r.y0 + r.y1) / 2.0), (Anchor::High, r.y1)],
        (Subject::Edges(r, e), Axis::X) => {
            [(e.left, Anchor::Low, r.x0), (e.right, Anchor::High, r.x1)].into_iter().filter_map(|(on, anchor, v)| on.then_some((anchor, v))).collect()
        }
        (Subject::Edges(r, e), Axis::Y) => {
            [(e.bottom, Anchor::Low, r.y0), (e.top, Anchor::High, r.y1)].into_iter().filter_map(|(on, anchor, v)| on.then_some((anchor, v))).collect()
        }
    }
}

/// A distance along one axis, as a pair.
fn along(axis: Axis, d: f64) -> (f64, f64) {
    match axis {
        Axis::X => (d, 0.0),
        Axis::Y => (0.0, d),
    }
}

fn other(axis: Axis) -> Axis {
    match axis {
        Axis::X => Axis::Y,
        Axis::Y => Axis::X,
    }
}

fn extent(r: &Rect, axis: Axis) -> (f64, f64) {
    match axis {
        Axis::X => (r.x0, r.x1),
        Axis::Y => (r.y0, r.y1),
    }
}

fn subject_extent(subject: &Subject, axis: Axis) -> (f64, f64) {
    match subject {
        Subject::Point(p) => {
            let v = if axis == Axis::X { p.0 } else { p.1 };
            (v, v)
        }
        Subject::Box(r) | Subject::Edges(r, _) => extent(r, axis),
    }
}

/// The rectangle round the subject.
fn subject_rect(subject: &Subject) -> Rect {
    match subject {
        Subject::Point(p) => Rect { x0: p.0, y0: p.1, x1: p.0, y1: p.1 },
        Subject::Box(r) | Subject::Edges(r, _) => *r,
    }
}

/// Everything a snap works from, so that the helpers below take one thing.
struct Work<'a> {
    page: &'a SnapPage,
    settings: &'a SnapSettings,
    strength: &'a Strength,
    query: &'a Query<'a>,
}

impl Work<'_> {
    /// A pull of `points` screen points, in paper inches.
    fn radius(&self, points: f64) -> f64 {
        points / self.query.zoom
    }

    fn moving(&self, id: u32) -> bool {
        self.query.moving.contains(&id)
    }
}

/// Which of the targets that are places a snap looks for.
#[derive(Clone, Copy, Default)]
struct Kinds {
    connection_points: bool,
    handles: bool,
    vertices: bool,
    outline: bool,
    crossings: bool,
}

impl Kinds {
    fn any(&self) -> bool {
        self.connection_points || self.handles || self.outlines()
    }

    /// Whether the outlines of the shapes near are read.
    fn outlines(&self) -> bool {
        self.vertices || self.outline || self.crossings
    }
}

/// Snaps the subject of `query` on `page` of `doc`, by `settings` (and its strengths).
pub(super) fn snap(doc: &Document, page: &SnapPage, settings: &SnapSettings, query: &Query<'_>) -> Snapped {
    let strength = &settings.strength;
    let zoom = if super::takes_zoom(query.zoom) { query.zoom } else { DEFAULT_ZOOM };
    let query = Query { zoom, ..*query };
    let work = Work { page, settings, strength, query: &query };
    TRACE.with(|t| *t.borrow_mut() = Trace::default());
    let mut out = Snapped::default();
    let region = subject_rect(&query.subject);
    let finite = region.x0.is_finite() && region.y0.is_finite() && region.x1.is_finite() && region.y1.is_finite();
    if !finite {
        return out;
    }
    let snapping = settings.snap;
    let gluing = settings.glue && query.using == Use::Connect && matches!(query.subject, Subject::Point(_));
    if !snapping && !gluing {
        return out;
    }
    // The shapes of the page are targets of the Snap switch (and are all off under `InhibitSnap`), and, for the end of a connector, of the Glue switch.
    // A shape being placed is pulled by the options of SnapSettings (snap to handles, vertices, connection points and outlines), and a connector's end by
    // those of GlueSettings: the shape's box and its connection points are what is pulled (ours).
    let shape_snap = snapping && !page.inhibit;
    let placing = shape_snap && query.using == Use::Place;
    let kinds = if matches!(query.subject, Subject::Edges(..)) {
        Kinds::default()
    } else {
        Kinds {
            // A connector's end glues to what GlueSettings names, and a shape that is placed snaps to what SnapSettings names.
            connection_points: (gluing && settings.glue_connection_points) || (placing && settings.snap_connection_points),
            handles: (gluing && settings.glue_handles) || (placing && settings.snap_handles),
            vertices: (gluing && settings.glue_vertices) || (placing && settings.snap_vertices),
            outline: (gluing && settings.glue_geometry) || (placing && settings.snap_geometry),
            crossings: settings.intersections && shape_snap,
        }
    };
    let lines_up = shape_snap && (settings.alignment_box || settings.dynamic_grid);
    // The shapes near the subject, read once for everything below that needs them.
    let near: Option<Gathered<'_>> = (lines_up || kinds.any()).then(|| {
        let near_by = work.radius(f64::from(strength.points.max(strength.geometry).max(strength.guides)));
        let reach = if lines_up { near_by.max(ALIGN_REACH) } else { near_by };
        page.shapes(doc).gather(&places_region(&query).grown(reach), MAX_CANDIDATES, &|id| work.moving(id))
    });
    if let Some(found) = &near {
        out.examined = found.examined;
        out.visited = found.visited;
        out.candidates = found.items.len();
        out.truncated = page.shapes(doc).truncated || found.cut;
    }
    let items: &[&Item] = near.as_ref().map_or(&[], |g| g.items.as_slice());

    // A place pulls both axes at once, and a place is stronger than any line.
    if kinds.any() && !items.is_empty() {
        let found = place_targets(&work, doc, items, kinds, gluing, &mut out);
        if let Some(best) = found {
            out.dx = best.delta.0;
            out.dy = best.delta.1;
            out.hits.push(best.hit);
            out.glue = best.glue;
            return out;
        }
    }

    for axis in [Axis::X, Axis::Y] {
        let mut best = Best::default();
        if snapping {
            for found in line_targets(&work, items, axis) {
                best.offer(Some(found));
            }
            if settings.dynamic_grid && matches!(query.subject, Subject::Box(_)) && !page.inhibit {
                for found in spacing_targets(&work, doc, axis, &mut out) {
                    best.offer(Some(found));
                }
            }
        }
        if gluing {
            for found in guide_glue(&work, axis) {
                best.offer(Some(found));
            }
        }
        TRACE.with(|t| t.borrow_mut().axes[usize::from(axis == Axis::Y)] = logged(&best.log));
        if let Some(found) = best.best {
            match axis {
                Axis::X => out.dx = found.delta.0,
                Axis::Y => out.dy = found.delta.1,
            }
            out.hits.push(found.hit);
        }
    }
    // A box placed onto a guide is held to it when the Glue switch and glue to guides are on (a guide pulls a placing only when the page
    // does not inhibit snapping, `line_targets`).
    if query.using == Use::Place && !matches!(query.subject, Subject::Point(_)) && settings.glue && settings.glue_guides && !page.inhibit {
        out.holds = out
            .hits
            .iter()
            .filter_map(|h| match (h.target, h.axis, h.shape) {
                (Target::Guide, Some(axis), Some(guide)) => Some(Hold { guide, axis, anchor: h.anchor, line: h.line }),
                _ => None,
            })
            .collect();
    }
    out
}

/// The line `line` as a target for the anchors of the subject on `axis`: the anchor it pulls is the nearest, and it pulls if that is within `radius`
/// inches (`None`: from any distance).
#[allow(clippy::too_many_arguments)]
fn pull(
    anchors: &[(Anchor, f64)],
    axis: Axis,
    line: f64,
    radius: Option<f64>,
    rank: u8,
    target: Target,
    shape: Option<u32>,
    span: Option<(f64, f64)>,
) -> Option<Found> {
    if !line.is_finite() {
        return None;
    }
    let mut best = Best::default();
    for (anchor, value) in anchors {
        let d = line - value;
        let dist = d.abs();
        if !dist.is_finite() || radius.is_some_and(|r| dist > r) {
            continue;
        }
        let hit = Hit { target, axis: Some(axis), normal: None, line, at: None, anchor: *anchor, shape, point: None, span, gap: None };
        best.offer(Some(Found { rank, dist, delta: along(axis, d), hit, glue: None }));
    }
    best.best
}

/// Every line target on one axis: the grid, the ruler, guides, the alignment boxes of the shapes near, and the page margins.
fn line_targets(work: &Work<'_>, items: &[&Item], axis: Axis) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    let (page, settings, strength) = (work.page, work.settings, work.strength);
    let anchors = anchors(&work.query.subject, axis);
    let (k, zoom) = (page.k, work.query.zoom);
    if settings.grid
        && let Some(marks) = page.grid.grid(axis, k, zoom)
    {
        for (_, value) in &anchors {
            if let Some(line) = marks.nearest(*value) {
                out.extend(pull(&anchors, axis, line, Some(work.radius(f64::from(strength.grid))), RANK_GRID, Target::Grid, None, None));
            }
        }
    }
    if settings.ruler
        && let Some(marks) = page.grid.ruler(axis, k, zoom)
    {
        for (_, value) in &anchors {
            if let Some(line) = marks.nearest(*value) {
                out.extend(pull(&anchors, axis, line, Some(work.radius(f64::from(strength.ruler))), RANK_RULER, Target::Ruler, None, None));
            }
        }
    }
    if page.inhibit {
        return out;
    }
    if settings.guides {
        for guide in page.guides.iter().filter(|g| g.axis == axis) {
            out.extend(pull(
                &anchors,
                axis,
                guide.at,
                Some(work.radius(f64::from(strength.guides))),
                RANK_GUIDE,
                Target::Guide,
                Some(guide.id),
                None,
            ));
        }
    }
    if settings.alignment_box || settings.dynamic_grid {
        let radius = Some(work.radius(f64::from(strength.extensions)));
        let mine = subject_extent(&work.query.subject, other(axis));
        for item in items {
            let (lo, hi) = extent(&item.bbox, axis);
            let theirs = extent(&item.bbox, other(axis));
            let span = Some((mine.0.min(theirs.0), mine.1.max(theirs.1)));
            for line in [lo, (lo + hi) / 2.0, hi] {
                out.extend(pull(&anchors, axis, line, radius, RANK_ALIGNMENT, Target::Alignment, Some(item.id), span));
            }
        }
    }
    if settings.dynamic_grid && page.size.0 > 0.0 && page.size.1 > 0.0 {
        let radius = Some(work.radius(f64::from(strength.extensions)));
        let lines = match axis {
            Axis::X => [page.margins.left, page.size.0 - page.margins.right],
            Axis::Y => [page.margins.bottom, page.size.1 - page.margins.top],
        };
        for line in lines {
            out.extend(pull(&anchors, axis, line, radius, RANK_ALIGNMENT, Target::PageMargin, None, None));
        }
    }
    out
}

/// The guides as glue for the end of a connector: the end comes onto the guide.
fn guide_glue(work: &Work<'_>, axis: Axis) -> Vec<Found> {
    if !work.settings.glue_guides || work.page.inhibit {
        return Vec::new();
    }
    let anchors = anchors(&work.query.subject, axis);
    work.page
        .guides
        .iter()
        .filter(|g| g.axis == axis)
        .filter_map(|g| pull(&anchors, axis, g.at, Some(work.radius(f64::from(work.strength.guides))), RANK_GUIDE, Target::Guide, Some(g.id), None))
        .collect()
}

/// A hit on a place.
fn on_place(target: Target, at: P, shape: Option<u32>, point: Option<u32>) -> Hit {
    Hit { target, axis: None, normal: None, line: at.0, at: Some(at), anchor: Anchor::Point, shape, point, span: None, gap: None }
}

/// The rectangle round the subject and the places of the moving shapes that are pulled with it (a connection point may lie outside its shape's box).
fn places_region(query: &Query<'_>) -> Rect {
    let mut region = subject_rect(&query.subject);
    if matches!(query.subject, Subject::Box(_)) {
        for p in query.own_points.iter().filter(|p| p.0.is_finite() && p.1.is_finite()).take(MAX_OWN_POINTS) {
            region = Rect { x0: region.x0.min(p.0), y0: region.y0.min(p.1), x1: region.x1.max(p.0), y1: region.y1.max(p.1) };
        }
    }
    region
}

/// The places of the subject that are pulled: the point itself; the corners, the middles of the sides and the middle of a box, and the places of the
/// shapes that move. A resize pulls no place.
fn anchor_points(subject: &Subject, own: &[P]) -> Vec<P> {
    match subject {
        Subject::Point(p) => vec![*p],
        Subject::Box(r) => {
            let (cx, cy) = r.centre();
            let mut out = vec![(r.x0, r.y0), (cx, r.y0), (r.x1, r.y0), (r.x0, cy), (cx, cy), (r.x1, cy), (r.x0, r.y1), (cx, r.y1), (r.x1, r.y1)];
            out.extend(own.iter().copied().filter(|p| p.0.is_finite() && p.1.is_finite()).take(MAX_OWN_POINTS));
            out
        }
        Subject::Edges(..) => Vec::new(),
    }
}

/// What a query spends on targets that are places.
struct Spent {
    /// The rows of connection points the query may still merge.
    connection_rows: usize,
    points: usize,
    pieces: usize,
    /// The pieces the curves were estimated to make.
    flattened: usize,
    /// The query's budget for building outlines, across the shapes.
    geometry: geometry::Budget,
}

/// The targets that are places: connection points, handles, vertices, the nearest point of an outline, and the crossings of two outlines. The anchors of
/// the subject are pulled to them (a point, or a box's corners, middles and the places of its shapes); the strongest kind wins, then the nearest.
///
/// **The work is bounded in rows and in points.** A shape is looked at only when its box is within the pull of the subject's box; a `Connection` or
/// `Geometry` section of more rows than the caps (counted before any is merged) is skipped and the query is `truncated`; target points and outline pieces
/// are counted against [`MAX_TARGET_POINTS`] and [`MAX_TARGET_PIECES`].
fn place_targets(work: &Work<'_>, doc: &Document, items: &[&Item], kinds: Kinds, gluing: bool, out: &mut Snapped) -> Option<Found> {
    let (strength, page) = (work.strength, work.page);
    let reader = Reader::new(doc);
    let mut best = Best::default();
    let (points, geometry) = (work.radius(f64::from(strength.points)), work.radius(f64::from(strength.geometry)));
    let reach = points.max(geometry);
    let region = places_region(work.query);
    let anchors = anchor_points(&work.query.subject, work.query.own_points);
    // The outline is pulled to by the places of the subject that are not the moving shapes' own (a box's nine): `nearest` costs a pass over the path.
    let nearest_from: Vec<P> = anchors.iter().copied().take(9).collect();
    let mut spent = Spent { connection_rows: MAX_CONNECTION_ROWS_PER_QUERY, points: 0, pieces: 0, flattened: 0, geometry: geometry::Budget::new() };
    let mut weigh =
        |target: Target, rank: u8, at: P, shape: Option<u32>, point: Option<u32>, glue: Option<Glue>, radius: f64, from: &[P], out: &mut Snapped| {
            if spent.points >= MAX_TARGET_POINTS {
                out.truncated = true;
                return;
            }
            spent.points += 1;
            out.points += 1;
            if !(at.0.is_finite() && at.1.is_finite()) {
                return;
            }
            // The anchor of the subject nearest this place.
            let nearest = from.iter().map(|q| ((at.0 - q.0).hypot(at.1 - q.1), q)).filter(|(d, _)| d.is_finite()).min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((dist, q)) = nearest
                && dist <= radius
            {
                best.offer(Some(Found {
                    rank,
                    dist,
                    delta: (at.0 - q.0, at.1 - q.1),
                    hit: on_place(target, at, shape, point),
                    glue: if gluing { glue } else { None },
                }));
            }
        };
    // The pieces of the outlines near, flattened once, for the crossings.
    let mut crossing_sets: Vec<Vec<kurbo::Line>> = Vec::new();
    let mut outlined = 0usize;
    let probe = KRect::new(region.x0 - reach, region.y0 - reach, region.x1 + reach, region.y1 + reach);
    for item in items {
        // The box is checked before a row of the shape is read.
        if !item.bbox.grown(reach).meets(&region) {
            continue;
        }
        if kinds.connection_points {
            // Counted over the layers and charged to the query before any row is merged.
            let rows = reader.connection_rows(&item.at);
            if rows > MAX_CONNECTION_ROWS || rows > spent.connection_rows {
                out.truncated = true;
            } else if rows > 0 {
                spent.connection_rows -= rows;
                for cp in reader.connection_points_within(&item.at, rows).unwrap_or_default() {
                    let at = (cp.x / page.k, cp.y / page.k);
                    let glue = Glue::ConnectionPoint { shape: item.id, point: cp.index };
                    weigh(Target::ConnectionPoint, RANK_CONNECTION_POINT, at, Some(item.id), Some(cp.index), Some(glue), points, &anchors, out);
                }
            }
        }
        if kinds.handles {
            let c = &item.corners;
            let mids = [(c[0], c[1]), (c[1], c[2]), (c[2], c[3]), (c[3], c[0])].map(|(a, b)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0));
            for at in c.iter().copied().chain(mids) {
                weigh(Target::Handle, RANK_HANDLE, at, Some(item.id), None, Some(Glue::Handle { shape: item.id }), points, &anchors, out);
            }
        }
        if !kinds.outlines() {
            continue;
        }
        if outlined >= MAX_OUTLINE_SHAPES {
            out.truncated = true;
            continue;
        }
        outlined += 1;
        let shape = outline(&reader, &item.at, page.k, &mut spent.geometry);
        if shape.too_big > 0 {
            out.truncated = true;
        }
        for path in &shape.paths {
            let cost = path.elements().len();
            if kinds.vertices {
                for v in osnap::endpoints(path) {
                    weigh(Target::Vertex, RANK_VERTEX, (v.x, v.y), Some(item.id), None, Some(Glue::Vertex { shape: item.id }), points, &anchors, out);
                }
            }
            if kinds.outline {
                let around = path.control_box().inflate(geometry, geometry);
                for q in &nearest_from {
                    if !around.contains(Point::new(q.0, q.1)) {
                        continue;
                    }
                    if spent.pieces + cost > MAX_TARGET_PIECES {
                        out.truncated = true;
                        break;
                    }
                    spent.pieces += cost;
                    if let Some((at, _)) = osnap::nearest(path, Point::new(q.0, q.1)) {
                        weigh(
                            Target::Outline,
                            RANK_OUTLINE,
                            (at.x, at.y),
                            Some(item.id),
                            None,
                            Some(Glue::Outline { shape: item.id }),
                            geometry,
                            std::slice::from_ref(q),
                            out,
                        );
                    }
                }
            }
            if kinds.crossings && crossing_sets.len() < MAX_OUTLINE_SHAPES * geometry::MAX_SECTIONS {
                if spent.pieces + cost > MAX_TARGET_PIECES {
                    out.truncated = true;
                } else {
                    spent.pieces += cost;
                    let found = osnap::pieces_near(path, Some(&probe), MAX_TARGET_PIECES.saturating_sub(spent.pieces));
                    spent.pieces += found.charged;
                    spent.flattened += found.charged;
                    out.truncated |= found.cut;
                    if !found.lines.is_empty() {
                        crossing_sets.push(found.lines);
                    }
                }
            }
        }
    }
    // The crossings of the outlines near, pair by pair.
    let mut pairs = 0usize;
    'pairs: for (i, a) in crossing_sets.iter().enumerate() {
        for b in crossing_sets.iter().skip(i + 1) {
            if pairs >= MAX_PATH_PAIRS || spent.pieces + a.len() * b.len() > MAX_TARGET_PIECES {
                out.truncated = true;
                break 'pairs;
            }
            pairs += 1;
            spent.pieces += a.len() * b.len();
            for x in osnap::crossings(a, b, Some(probe)) {
                weigh(Target::Intersection, RANK_INTERSECTION, (x.x, x.y), None, None, None, geometry, &anchors, out);
            }
        }
    }
    out.pieces = spent.pieces;
    out.flattened = spent.flattened;
    out.outline = spent.geometry.spent();
    TRACE.with(|t| t.borrow_mut().place = logged(&best.log));
    best.best
}

/// The spacing targets of the dynamic grid on one axis, for a box: a gap to the shape beside it equal to the gap on its other side, or to the gap
/// between that shape and the one beyond.
fn spacing_targets(work: &Work<'_>, doc: &Document, axis: Axis, out: &mut Snapped) -> Vec<Found> {
    let Subject::Box(subject) = work.query.subject else { return Vec::new() };
    let (lo, hi) = extent(&subject, axis);
    let (cross_lo, cross_hi) = extent(&subject, other(axis));
    // The neighbours may be a few inches off: a window along the axis, as wide across as the subject.
    let reach = (4.0 * (hi - lo)).max(SPACING_REACH);
    let window = match axis {
        Axis::X => Rect { x0: lo - reach, y0: cross_lo, x1: hi + reach, y1: cross_hi },
        Axis::Y => Rect { x0: cross_lo, y0: lo - reach, x1: cross_hi, y1: hi + reach },
    };
    let gathered = work.page.shapes(doc).gather(&window, MAX_CANDIDATES, &|id| work.moving(id));
    out.examined = out.examined.saturating_add(gathered.examined);
    out.visited = out.visited.saturating_add(gathered.visited);
    // A gather that was cut left shapes out of the neighbours, and the answer says so.
    out.truncated |= gathered.cut;
    let overlaps = |a: (f64, f64), b: (f64, f64)| a.0 < b.1 && b.0 < a.1;
    let eps = 1.0e-9;
    let beside: Vec<&Item> = gathered.items.iter().copied().filter(|i| overlaps(extent(&i.bbox, other(axis)), (cross_lo, cross_hi))).collect();
    let hi_edge = |i: &&Item| extent(&i.bbox, axis).1;
    let lo_edge = |i: &&Item| extent(&i.bbox, axis).0;
    let low = beside.iter().filter(|i| hi_edge(i) <= lo + eps).max_by(|a, b| hi_edge(a).total_cmp(&hi_edge(b)));
    let high = beside.iter().filter(|i| lo_edge(i) >= hi - eps).min_by(|a, b| lo_edge(a).total_cmp(&lo_edge(b)));
    let radius = work.radius(f64::from(work.strength.extensions));
    let mut found: Vec<Found> = Vec::new();
    let mut push = |delta: f64, shape: u32, gap: f64, from: f64, to: f64| {
        if !(delta.is_finite() && delta.abs() <= radius && gap.is_finite() && gap >= 0.0) {
            return;
        }
        let hit = Hit {
            target: Target::Spacing,
            axis: Some(axis),
            normal: None,
            line: (lo + hi) / 2.0 + delta,
            at: None,
            anchor: Anchor::Middle,
            shape: Some(shape),
            point: None,
            span: Some((from, to)),
            gap: Some(gap),
        };
        found.push(Found { rank: RANK_ALIGNMENT, dist: delta.abs(), delta: along(axis, delta), hit, glue: None });
    };
    let gap_low = low.map(|l| lo - hi_edge(l));
    let gap_high = high.map(|h| lo_edge(h) - hi);
    if let (Some(l), Some(gl), Some(gh)) = (low, gap_low, gap_high) {
        // Equal on both sides: the box moves half the difference.
        let delta = (gh - gl) / 2.0;
        push(delta, l.id, gl + delta, hi_edge(l), lo + delta);
    }
    // The gap between the neighbour and the shape beyond it, taken for this side.
    if let (Some(l), Some(gl)) = (low, gap_low) {
        let (l_lo, l_cross) = (lo_edge(l), extent(&l.bbox, other(axis)));
        let beyond = gathered
            .items
            .iter()
            .filter(|i| i.id != l.id && overlaps(extent(&i.bbox, other(axis)), l_cross) && hi_edge(i) <= l_lo + eps)
            .max_by(|a, b| hi_edge(a).total_cmp(&hi_edge(b)));
        if let Some(b) = beyond {
            let g = l_lo - hi_edge(b);
            push(g - gl, l.id, g, hi_edge(l), lo + (g - gl));
        }
    }
    if let (Some(h), Some(gh)) = (high, gap_high) {
        let (h_hi, h_cross) = (hi_edge(h), extent(&h.bbox, other(axis)));
        let beyond = gathered
            .items
            .iter()
            .filter(|i| i.id != h.id && overlaps(extent(&i.bbox, other(axis)), h_cross) && lo_edge(i) >= h_hi - eps)
            .min_by(|a, b| lo_edge(a).total_cmp(&lo_edge(b)));
        if let Some(b) = beyond {
            let g = lo_edge(b) - h_hi;
            push(gh - g, h.id, g, hi + (gh - g), lo_edge(h));
        }
    }
    found
}
