//! Doubling and halving connection points (the connection-point doubling design, a DiaCraft extra beyond the reference app: owner, 2026-11-26).
//!
//! **What is built here: the edges of a shape and the points on them.** [`edges`] reads a shape's outline and splits it into edges (design
//! §1), says which rows of its `Connection` section belong to which edge (§2) and how much room each edge has for a doubling under the minimum distance
//! between points (§3.8); [`query_edges`] puts it into the answer of `shape.connection_edges` (§8.2). The edit (`shape.connection_points`, the portions,
//! the generations, the formulas, the rows and the glue) is, and the marker of the points a doubling made is: the answer here carries neither
//! `added`, `newest` nor `can_halve`, which need the marker.
//!
//! **Lengths.** Everything is worked out in the shape's own frame, in the page's drawing units, as the cells hold it, and told in paper inches on the page: the shape's pose
//! and its groups' carry a place to the page, and the page's drawing scale (`DrawingScale` over `PageScale`) turns drawing units into paper inches (§3.8.3). The minimum
//! distance is a paper distance, so a wall 2 metres long at 1:100 is 20 millimetres on paper.
//!
//! **What it costs.** A call is bounded in every dimension and charged by the units its loops walk ([`OutlineWork`], the caps in [`edges`]); a shape that meets a cap of its
//! own (too many rows, sections or edges) is refused in words or listed `truncated`, and a call that spends one of its own stops there: that shape and every later one is
//! refused in words, so a list of hostile shapes costs no more than one.

mod edges;

pub use edges::{
    CORNER_TURN, EdgeKind, MAX_ARC_CALLS, MAX_EDGES_PER_SHAPE, MAX_OUTLINE_ROWS, MAX_OUTLINE_SECTIONS, MAX_OUTLINE_STEPS, MAX_OUTLINE_WORK,
    MAX_PATH_POINTS, MIN_EDGE, ON_EDGE, OutlineWork, Spent,
};

use diacraft_doc::{CellAddr, Document, ShapeRef, ShapeType, SheetRef};
use diacraft_geom::kurbo::{Affine, Point};

use self::edges::{Edge, OnEdge, outline_of, point_rows, points_on, smallest_new_gap};
use super::{Problem, Reader, layers, require_page, scan};

/// The most connection points a shape may hold after a doubling: what a snap reads of one shape and what a connector end looks at.
pub const MAX_POINTS_PER_SHAPE: u32 = 256;

/// The most `Connection` rows of one shape that are read, counted over the shape, its master and the styles before any is merged.
pub const MAX_CONNECTION_ROWS_READ: usize = 256;

/// The most shapes one call names.
pub const MAX_SHAPES_PER_CALL: usize = 4_096;

/// The least and the most a call may ask the minimum distance between points to be, in paper inches (0.01 in to 2 in).
pub const MIN_MIN_GAP: f64 = 0.01;
pub const MAX_MIN_GAP: f64 = 2.0;

/// The minimum distance between points a doubling keeps when a call gives none, in paper inches: 3 mm.
pub const DEFAULT_MIN_GAP: f64 = 3.0 / 25.4;

/// The slack a gap is compared with the minimum under, in paper inches: a gap of exactly the minimum, worked out in floating point, is room.
const GAP_SLACK: f64 = 1.0e-9;

/// Why a shape could not be read: its own fault, or the call's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Skip {
    /// The shape went past a cap of its own (rows, sections, a number too big to add), in words that name it.
    Shape(String),
    /// The call spent one of its limits on this shape or on one before it.
    Call(Spent),
}

impl Skip {
    /// The words the answer tells for the shape `id`.
    pub fn words(&self, id: u32) -> String {
        match self {
            Skip::Shape(words) => words.clone(),
            Skip::Call(spent) => format!("shape {id}: {}", spent.words()),
        }
    }
}

/// How many points a doubling of an edge with `points` of them makes (§3.2): the middle of every portion, so an open edge's N become 2N + 1 (it gains N + 1), a closed
/// edge's N become 2N (it gains N), and a closed edge with none gets two, its start and halfway round.
pub fn doubling_adds(closed: bool, points: u32) -> u32 {
    match (closed, points) {
        (false, n) => n.saturating_add(1),
        (true, 0) => 2,
        (true, n) => n,
    }
}

/// Whether a shape with `points` points can take `adds` more without passing [`MAX_POINTS_PER_SHAPE`].
pub fn fits(points: u32, adds: u32) -> bool {
    points.checked_add(adds).is_some_and(|total| total <= MAX_POINTS_PER_SHAPE)
}

/// One edge of a shape, as the query tells it. Lengths and places are paper inches on the page.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeInfo {
    /// Its number, from 0.
    pub edge: u32,
    pub kind: EdgeKind,
    /// The `IX` of its `Geometry` section (`Geometry<IX + 1>` is the reference's spelling), if the section has one.
    pub section: Option<u32>,
    /// The first and last row that drew it.
    pub rows: (u32, u32),
    /// How many steps it draws.
    pub steps: usize,
    pub length: f64,
    pub from: (f64, f64),
    pub to: (f64, f64),
    /// At most [`MAX_PATH_POINTS`] places along it at equal lengths; a closed edge's are joined last to first.
    pub path: Vec<(f64, f64)>,
    /// The `IX` of its points, in their order along it.
    pub points: Vec<u32>,
    /// The smallest gap a doubling would make, in paper inches.
    pub gap: f64,
    /// How many points a doubling would make: 0 when it has no room under the minimum distance.
    pub adds: u32,
}

/// What the query tells of one shape.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeEdges {
    pub id: u32,
    /// Why the shape has no edge to read, in words, or `None`.
    pub refused: Option<String>,
    pub edges: Vec<EdgeInfo>,
    /// The rows on no edge: kept as they are.
    pub other: Vec<u32>,
    /// How many connection points the shape holds in all.
    pub points: usize,
    /// Whether some edge has room under the minimum distance and its doubling fits [`MAX_POINTS_PER_SHAPE`].
    pub can_double: bool,
    /// Whether the shape has more edges than are listed.
    pub truncated: bool,
}

/// The query's answer.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgesAnswer {
    pub page: u32,
    /// The minimum distance the call was worked out under, in paper inches.
    pub min_gap: f64,
    /// Drawing units to a paper inch on the page.
    pub units_per_inch: f64,
    /// Said once when the page's drawing scale is no scale and was read as 1:1.
    pub scale_note: Option<String>,
    pub shapes: Vec<ShapeEdges>,
    /// What the call spent.
    pub work: OutlineWork,
}

/// Drawing units to a paper inch on a page, and a note when the page's scale is set and cannot be used (§3.8.3): zero, negative or not a number is read as 1:1. A page
/// with no scale cells is plainly 1:1.
fn page_scale(reader: &Reader<'_>, page: u32) -> (f64, Option<String>) {
    let sheet = SheetRef::Page(page);
    let (paper, drawing) = (reader.number(&sheet, "PageScale"), reader.number(&sheet, "DrawingScale"));
    let k = drawing / paper;
    if paper > 0.0 && drawing > 0.0 && k.is_finite() && k > 0.0 {
        return (k, None);
    }
    let set = ["PageScale", "DrawingScale"].iter().any(|name| reader.is_set(&sheet, &CellAddr::direct(name)));
    let note = set.then(|| "the page's drawing scale is not a usable number: lengths are compared as drawn, one to one (1:1)".to_owned());
    (1.0, note)
}

/// The map from a shape's own frame (drawing units) to the page in paper inches: its pose and its groups' (three points carried through `Reader::to_page` fix an affine
/// map), and the scale of the drawing. `None` when the pose is not made of numbers.
fn to_paper(reader: &Reader<'_>, at: &ShapeRef, k: f64) -> Option<Affine> {
    let o = reader.to_page(at, (0.0, 0.0));
    let ex = reader.to_page(at, (1.0, 0.0));
    let ey = reader.to_page(at, (0.0, 1.0));
    let map = Affine::scale(1.0 / k) * Affine::new([ex.0 - o.0, ex.1 - o.1, ey.0 - o.0, ey.1 - o.1, o.0, o.1]);
    map.as_coeffs().iter().all(|c| c.is_finite()).then_some(map)
}

/// Why a shape has no edge by what it is (§1.3): a group, or a line, connector or guide. Its outline and its layer are asked after.
fn refusal(reader: &Reader<'_>, id: u32, kind: &ShapeType, at: &ShapeRef) -> Option<String> {
    if *kind == ShapeType::Group {
        return Some(format!("shape {id} is a group: select a shape inside it to change its connection points"));
    }
    if *kind == ShapeType::Guide || reader.is_one_d(&SheetRef::Shape(at.clone())) {
        return Some(format!("shape {id} is a line or a connector: connection points are put on two-dimensional shapes"));
    }
    None
}

fn refused(id: u32, words: String) -> ShapeEdges {
    ShapeEdges { id, refused: Some(words), edges: Vec::new(), other: Vec::new(), points: 0, can_double: false, truncated: false }
}

/// A call under way: the document, the page's scale and what the call has spent.
struct Query<'a> {
    doc: &'a Document,
    reader: Reader<'a>,
    page: u32,
    k: f64,
    min_gap: f64,
    work: OutlineWork,
    locks: layers::Locks,
}

impl Query<'_> {
    /// The edges of the shape `id` at `at`, or why it has none.
    fn shape(&mut self, id: u32, at: &ShapeRef) -> Result<ShapeEdges, Problem> {
        let Some(shape) = self.doc.shape(at) else { return Ok(refused(id, format!("shape {id} is not on the page"))) };
        if let Some(words) = refusal(&self.reader, id, &shape.kind, at) {
            return Ok(refused(id, words));
        }
        if let Some(layer) = self.locks.of(self.doc, self.page, shape)? {
            return Ok(refused(id, layers::locked_words(id, &layer)));
        }
        Ok(self.read(id, at).unwrap_or_else(|skip| refused(id, skip.words(id))))
    }

    fn read(&mut self, id: u32, at: &ShapeRef) -> Result<ShapeEdges, Skip> {
        // The rows are counted before anything is merged, and a shape with too many is left as it is.
        let rows = point_rows(&self.reader, at, &mut self.work)?;
        let outline = outline_of(&self.reader, at, &mut self.work)?;
        if outline.edges.is_empty() {
            return Err(Skip::Shape(format!("shape {id} draws no outline: it has no edge to put connection points on")));
        }
        let on = points_on(&outline, &rows, &mut self.work)?;
        let Some(map) = to_paper(&self.reader, at, self.k) else {
            return Err(Skip::Shape(format!("shape {id}: its place on the page is not made of numbers")));
        };
        let total = u32::try_from(rows.len()).unwrap_or(u32::MAX);
        let mut edges = Vec::with_capacity(outline.edges.len());
        let mut can_double = false;
        for (edge, points) in outline.edges.iter().zip(&on.edges) {
            let info = self.edge_info(edge, points, &map)?;
            can_double |= info.adds > 0 && fits(total, info.adds);
            edges.push(info);
        }
        Ok(ShapeEdges { id, refused: None, edges, other: on.other, points: rows.len(), can_double, truncated: outline.truncated })
    }

    fn edge_info(&mut self, edge: &Edge, points: &[OnEdge], map: &Affine) -> Result<EdgeInfo, Skip> {
        let alongs: Vec<f64> = points.iter().map(|p| p.along).collect();
        let gap = smallest_new_gap(edge, &alongs) / self.k;
        let count = u32::try_from(points.len()).unwrap_or(u32::MAX);
        let adds = if gap + GAP_SLACK >= self.min_gap { doubling_adds(edge.is_closed(), count) } else { 0 };
        let on_page = |p: Point| {
            let q = *map * p;
            (q.x, q.y)
        };
        let path = edge.sample(MAX_PATH_POINTS, &mut self.work)?.into_iter().map(on_page).collect();
        Ok(EdgeInfo {
            edge: edge.number,
            kind: edge.kind,
            section: edge.section_ix,
            rows: edge.rows,
            steps: edge.steps,
            length: edge.length / self.k,
            from: on_page(edge.start()),
            to: on_page(edge.end()),
            path,
            points: points.iter().map(|p| p.index).collect(),
            gap,
            adds,
        })
    }
}

/// The edges of the shapes `ids` of a page and the connection points on each, with how many points a doubling of each edge would make under the minimum distance
/// `min_gap` (paper inches). Each shape is told once, in the order first named; one that cannot be read is `refused` in words and the rest are still told. A shape that is
/// not on the page, a page that is not there and more than [`MAX_SHAPES_PER_CALL`] names are errors. Nothing is changed.
pub fn query_edges(doc: &Document, page: u32, ids: &[u32], min_gap: f64) -> Result<EdgesAnswer, Problem> {
    require_page(doc, page)?;
    if ids.len() > MAX_SHAPES_PER_CALL {
        return Err(Problem::bad(format!("at most {MAX_SHAPES_PER_CALL} shapes may be named")));
    }
    let ids = scan::distinct(ids);
    let named = scan::Named::of(doc, page, ids.iter().copied())?;
    let reader = Reader::new(doc);
    let (k, scale_note) = page_scale(&reader, page);
    let mut query = Query { doc, reader, page, k, min_gap, work: OutlineWork::default(), locks: layers::Locks::default() };
    let mut shapes = Vec::with_capacity(ids.len());
    for id in ids {
        let at = named.require(id)?;
        shapes.push(query.shape(id, &at)?);
    }
    Ok(EdgesAnswer { page, min_gap, units_per_inch: k, scale_note, shapes, work: query.work })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn a_doubling_makes_the_middle_of_every_portion() {
        // Open: N points make 2N + 1, so N + 1 are added: 0 -> 1 -> 3 -> 7.
        assert_eq!([0, 1, 3, 7].map(|n| doubling_adds(false, n)), [1, 2, 4, 8]);
        // Closed: N points have N gaps, so N are added; an empty closed edge gets two.
        assert_eq!([0, 1, 2, 4].map(|n| doubling_adds(true, n)), [2, 1, 2, 4]);
        assert_eq!(doubling_adds(false, u32::MAX), u32::MAX, "no wrap");
    }

    #[test]
    fn the_cap_is_256_points_and_nothing_wraps() {
        assert!(fits(255, 1) && fits(0, 256) && fits(127, 129));
        assert!(!fits(128, 129) && !fits(256, 1) && !fits(u32::MAX, 1) && !fits(1, u32::MAX));
    }
}
