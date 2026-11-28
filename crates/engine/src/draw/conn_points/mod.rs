//! Doubling and halving connection points (the connection-point doubling design, a DiaCraft extra beyond the reference app: owner, 2026-11-26).
//!
//! **The edges of a shape and the points on them**. [`edges`] reads a shape's outline and splits it into edges (design §1), says which rows of its
//! `Connection` section belong to which edge (§2) and how much room each edge has for a doubling under the minimum distance between points (§3.8);
//! [`query_edges`] puts it into the answer of `shape.connection_edges` (§8.2), with the generation of each point from the marker of the points a doubling made
//! ([`super::conn_marker`]).
//!
//! **The edit**. [`change_points`] doubles or halves the points of the edges of the shapes it is given (§3 to §8): a doubling puts a new point in the middle, by arc
//! length, of every portion of an edge and moves nothing ([`layout`]); a halving takes away the points of the newest doubling of each edge and never an original one, closes the
//! gaps in the numbering, and brings every reference to a row along: the `Connect` rows, the formulas that name a row and the marker (`conn_refs`). A connector glued to a point a
//! halving takes away is glued to its successor on the same edge, else to the nearest point of the shape, else to the whole shape ([`edit`]). The shape takes the rows as its own
//! when it inherits them ([`local`]). The whole call is one edit of a copy of the document: a refusal leaves the document as it was.
//!
//! **Lengths.** Everything is worked out in the shape's own frame, in the page's drawing units, as the cells hold it, and told in paper inches on the page: the shape's pose
//! and its groups' carry a place to the page, and the page's drawing scale (`DrawingScale` over `PageScale`) turns drawing units into paper inches (§3.8.3). The minimum
//! distance is a paper distance, so a wall 2 metres long at 1:100 is 20 millimetres on paper.
//!
//! **What it costs.** A call is bounded in every dimension and charged by the units its loops walk ([`OutlineWork`], the caps in [`edges`], the rows a call writes, the formulas
//! its walk of the page looks at, the connector ends it renumbers, the calc steps a point on a curve costs each time its shape is resized); a shape that meets a cap of its own
//! (too many rows, sections or edges) is refused in words or listed `truncated`, and a call that spends one of its own stops there: that shape and every later one is refused
//! in words, so a list of hostile shapes costs no more than one.

mod edges;
mod edit;
mod ends;
pub mod layout;
mod local;

pub use edges::{
    CORNER_TURN, EdgeKind, MAX_ARC_CALLS, MAX_EDGES_PER_SHAPE, MAX_OUTLINE_ROWS, MAX_OUTLINE_SECTIONS, MAX_OUTLINE_STEPS, MAX_OUTLINE_WORK,
    MAX_PATH_POINTS, MIN_EDGE, ON_EDGE, OutlineWork, Spent,
};
pub use layout::{doubling_adds, fits};

use std::collections::HashMap;

use diacraft_calc::row_refs::RowName;
use diacraft_doc::{CellAddr, Document, ShapeRef, ShapeType, SheetRef};
use diacraft_geom::kurbo::{Affine, Point};

use self::edges::{Edge, OnEdge, OnEdges, Outline, PointRow, outline_of, point_rows, points_on, smallest_new_gap};
use super::conn_marker::{self, Marker, NameIndex};
use super::conn_refs::{MAX_RENUMBERED_ENDS, ScanWork};
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

/// The most steps a section may draw for a point on its curve to be written as a path formula (design §4.2).
pub const MAX_FOLLOW_STEPS: usize = 1_024;

/// The most steps of calc's budget a shape's path-form points may cost each time the shape's geometry or size changes: a tenth of calc's budget for one edit.
pub const MAX_FOLLOW_COST: u64 = 2_000_000;

/// The most `Connect` rows one call renumbers, over all its shapes. **The same number as the walk's own cap** ([`MAX_RENUMBERED_ENDS`]): the call's one counter is the walk's
/// (`ScanWork`), so the two cannot disagree about when a call has renumbered too many ends.
pub const MAX_REGLUED_ENDS: usize = MAX_RENUMBERED_ENDS;

/// The most rows one call writes, over all its shapes: each changed shape's whole own section counts (256 shapes of 256 points).
pub const MAX_ROWS_WRITTEN: usize = 65_536;

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

/// What a call does to the points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Double,
    Halve,
}

/// A call of [`change_points`].
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub ids: Vec<u32>,
    pub action: Action,
    /// One edge of the one shape named, by the number `shape.connection_edges` gives it.
    pub edge: Option<u32>,
    /// A place on the page in paper inches and how far from it an edge may be: the edge of the shapes named nearest it within that reach is the one changed.
    pub at: Option<((f64, f64), f64)>,
    /// The minimum distance between points a doubling keeps, in paper inches.
    pub min_gap: f64,
}

/// What a call did to one edge.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeChange {
    pub edge: u32,
    pub kind: EdgeKind,
    pub before: u32,
    pub after: u32,
    pub generation: u8,
}

/// An edge a doubling left as it was for want of room.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeSkipped {
    pub edge: u32,
    pub gap: f64,
    pub min_gap: f64,
}

/// What a call did to one shape.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeChange {
    pub id: u32,
    pub edges: Vec<EdgeChange>,
    pub skipped_edges: Vec<EdgeSkipped>,
    pub points_before: usize,
    pub points_after: usize,
    pub skipped: Option<String>,
}

/// An end of a connector that went to another point, or to the whole of the shape (`to` is `None`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndReglued {
    pub connector: u32,
    pub begin: bool,
    pub shape: u32,
    pub from: u32,
    pub to: Option<u32>,
}

/// What [`change_points`] did.
#[derive(Clone, Debug)]
pub struct Answer {
    pub page: u32,
    pub action: Action,
    pub changed: bool,
    pub min_gap: f64,
    pub scale_note: Option<String>,
    pub shapes: Vec<ShapeChange>,
    pub reglued: Vec<EndReglued>,
    /// The formulas of other cells that named a row that moved or went and were written again (a connector's end cells and the marker are not counted).
    pub renumbered: usize,
    /// What reading the outlines cost.
    pub work: OutlineWork,
    /// What the walk of the page for references cost.
    pub scan: ScanWork,
    /// The most any shape's path-form points are charged, in calc's steps each time the shape is resized; at most [`MAX_FOLLOW_COST`].
    pub follow_cost: u64,
}

/// Doubles or halves the connection points of the shapes the request names, on page `page`: see the module's account and `docs/editing-commands.md`. A call that changes nothing
/// is refused in words, and so is one that cannot be made whole: **the document is left as it was**.
pub fn change_points(doc: &mut Document, page: u32, request: &Request) -> Result<Answer, Problem> {
    edit::run(doc, page, request)
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
    /// The points a doubling made, each with the generation of the doubling that made it (1 for the first of its edge), in their order along the edge.
    pub added: Vec<(u32, u8)>,
    /// The newest generation among the edge's points: `None` when all are originals, so a halving of the edge is disabled.
    pub newest: Option<u8>,
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
    /// Whether some edge has a point a doubling made, which a halving would take.
    pub can_halve: bool,
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
    ShapeEdges { id, refused: Some(words), edges: Vec::new(), other: Vec::new(), points: 0, can_double: false, can_halve: false, truncated: false }
}

/// What the terms of a marker name among the rows of a shape, by the rule of a reference in a formula: for each term, in the marker's order, the number of the row it names
/// (the row's `IX`, or its place in a section keyed by name), or `None` for a term that names no row; and the generation of each row that a term stands for.
///
/// A name names the row of that spelling, and only when no row spells it so the first row of the name in any other case ([`NameIndex`]), so a row named `ROW_5` is not the row a
/// term of `Row_5` names, whichever comes first in the section. A number names the row of that `IX`, a named row too (by its place). A term stands for a row when it is spelled
/// the way the row is: a name for a row that has one, a number for a row that has none; a number that names a named row counts toward the row and gives it no generation, which
/// leaves the row an original. The terms of a marker that [`conn_marker::read`] gives name no row twice, so each row has at most one.
fn name_terms(rows: &[PointRow], marker: &Marker) -> (Vec<Option<u32>>, HashMap<u32, u8>) {
    let names = NameIndex::new(rows.iter().filter_map(|r| r.name.as_deref().map(|name| (r.index, name))));
    let has_name: HashMap<u32, bool> = rows.iter().map(|r| (r.index, r.name.is_some())).collect();
    let (mut named, mut added) = (Vec::with_capacity(marker.len()), HashMap::new());
    for term in marker.terms() {
        let (row, stands) = match &term.row {
            RowName::Number(n) => {
                let row = n.checked_sub(1).filter(|ix| has_name.contains_key(ix));
                (row, row.is_some_and(|ix| has_name.get(&ix) == Some(&false)))
            }
            RowName::Name(name) => {
                let row = names.find(name);
                (row, row.is_some())
            }
            RowName::Default => (None, false),
        };
        named.push(row);
        if let (Some(row), true) = (row, stands) {
            added.entry(row).or_insert(term.generation);
        }
    }
    (named, added)
}

/// Everything read of one shape that a query tells and an edit works from.
struct Loaded {
    id: u32,
    at: ShapeRef,
    outline: Outline,
    rows: Vec<PointRow>,
    on: OnEdges,
    marker: Marker,
    /// The row each term of `marker` names, in the marker's order ([`name_terms`]).
    term_rows: Vec<Option<u32>>,
    /// The generation of each row a term of `marker` stands for ([`name_terms`]); every other row is an original.
    added: HashMap<u32, u8>,
    /// Frame to paper inches on the page.
    map: Affine,
    /// The rows the section holds counted over the shape, its master and the styles before any was merged.
    held: usize,
}

impl Loaded {
    /// The generation of the point on row `index`: 0 for an original.
    fn generation(&self, index: u32) -> u8 {
        self.added.get(&index).copied().unwrap_or(0)
    }

    /// The generations of the points of edge `edge`, in their order along it.
    fn generations(&self, edge: usize) -> Vec<u8> {
        self.on.edges.get(edge).map_or_else(Vec::new, |points| points.iter().map(|p| self.generation(p.index)).collect())
    }
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

impl<'a> Query<'a> {
    /// A call on page `page` under the minimum distance `min_gap` (paper inches), and the note on the page's scale.
    fn start(doc: &'a Document, page: u32, min_gap: f64) -> (Query<'a>, Option<String>) {
        let reader = Reader::new(doc);
        let (k, note) = page_scale(&reader, page);
        (Query { doc, reader, page, k, min_gap, work: OutlineWork::default(), locks: layers::Locks::default() }, note)
    }

    /// The edges of the shape `id` at `at`, or why it has none.
    fn shape(&mut self, id: u32, at: &ShapeRef) -> Result<ShapeEdges, Problem> {
        match self.loaded(id, at)? {
            Ok(loaded) => Ok(self.info(&loaded).unwrap_or_else(|skip| refused(id, skip.words(id)))),
            Err(words) => Ok(refused(id, words)),
        }
    }

    /// The shape read as the call reads it, or the words that say why it cannot be: a shape that is not on the page, a group, a line, a connector, a shape on a locked layer, and
    /// one that went past a cap of its own or that the call has spent its limits on.
    fn loaded(&mut self, id: u32, at: &ShapeRef) -> Result<Result<Loaded, String>, Problem> {
        let Some(shape) = self.doc.shape(at) else { return Ok(Err(format!("shape {id} is not on the page"))) };
        if let Some(words) = refusal(&self.reader, id, &shape.kind, at) {
            return Ok(Err(words));
        }
        if let Some(layer) = self.locks.of(self.doc, self.page, shape)? {
            return Ok(Err(layers::locked_words(id, &layer)));
        }
        Ok(self.read(id, at).map_err(|skip| skip.words(id)))
    }

    fn read(&mut self, id: u32, at: &ShapeRef) -> Result<Loaded, Skip> {
        // The rows are counted before anything is merged, and a shape with too many is left as it is.
        let held = self.reader.connection_rows(at);
        let rows = point_rows(&self.reader, at, &mut self.work)?;
        let outline = outline_of(&self.reader, at, &mut self.work)?;
        if outline.edges.is_empty() {
            return Err(Skip::Shape(format!("shape {id} draws no outline: it has no edge to put connection points on")));
        }
        let on = points_on(&outline, &rows, &mut self.work)?;
        let Some(map) = to_paper(&self.reader, at, self.k) else {
            return Err(Skip::Shape(format!("shape {id}: its place on the page is not made of numbers")));
        };
        let marker = conn_marker::read(&self.reader, at);
        let (term_rows, added) = name_terms(&rows, &marker);
        Ok(Loaded { id, at: at.clone(), outline, rows, on, marker, term_rows, added, map, held })
    }

    /// What a doubling of an edge would do: its gap, and how many points it would make (0 under the minimum distance).
    fn room(&self, edge: &Edge, points: &[OnEdge]) -> (f64, u32) {
        let alongs: Vec<f64> = points.iter().map(|p| p.along).collect();
        let gap = smallest_new_gap(edge, &alongs) / self.k;
        let count = u32::try_from(points.len()).unwrap_or(u32::MAX);
        (gap, if gap + GAP_SLACK >= self.min_gap { doubling_adds(edge.is_closed(), count) } else { 0 })
    }

    fn info(&mut self, loaded: &Loaded) -> Result<ShapeEdges, Skip> {
        let total = u32::try_from(loaded.rows.len()).unwrap_or(u32::MAX);
        let mut edges = Vec::with_capacity(loaded.outline.edges.len());
        let (mut can_double, mut can_halve) = (false, false);
        for (number, (edge, points)) in loaded.outline.edges.iter().zip(&loaded.on.edges).enumerate() {
            let (gap, adds) = self.room(edge, points);
            can_double |= adds > 0 && fits(total, adds);
            let gens = loaded.generations(number);
            let newest = layout::newest_generation(&gens);
            can_halve |= newest.is_some();
            let on_page = |p: Point| {
                let q = loaded.map * p;
                (q.x, q.y)
            };
            let path = edge.sample(MAX_PATH_POINTS, &mut self.work)?.into_iter().map(on_page).collect();
            edges.push(EdgeInfo {
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
                added: points.iter().zip(&gens).filter(|(_, g)| **g > 0).map(|(p, g)| (p.index, *g)).collect(),
                newest,
                gap,
                adds,
            });
        }
        Ok(ShapeEdges {
            id: loaded.id,
            refused: None,
            edges,
            other: loaded.on.other.clone(),
            points: loaded.rows.len(),
            can_double,
            can_halve,
            truncated: loaded.outline.truncated,
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
    let (mut query, scale_note) = Query::start(doc, page, min_gap);
    let mut shapes = Vec::with_capacity(ids.len());
    for id in ids {
        let at = named.require(id)?;
        shapes.push(query.shape(id, &at)?);
    }
    let k = query.k;
    Ok(EdgesAnswer { page, min_gap, units_per_inch: k, scale_note, shapes, work: query.work })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calls_one_cap_of_renumbered_ends_is_the_walks() {
        // The call's counter is the walk's (`ScanWork`): the two numbers are one.
        assert_eq!(MAX_REGLUED_ENDS, MAX_RENUMBERED_ENDS);
    }

    fn row(index: u32, name: Option<&str>) -> PointRow {
        let name = name.map(std::sync::Arc::<str>::from);
        PointRow { index, name, x: 0.0, y: 0.0, dir_x: 0.0, dir_y: 0.0, kind: 0.0, extended: false, guarded: false, path_section: None }
    }

    fn term(row: RowName, generation: u8) -> conn_marker::Term {
        conn_marker::Term { row, generation }
    }

    #[test]
    fn a_term_names_the_row_that_spells_it_so_and_a_row_of_another_case_only_when_none_does() {
        // A section keyed by name, `ROW_5` first: R63, whichever row comes first.
        let rows = [row(0, Some("ROW_5")), row(1, Some("Row_5")), row(2, Some("Row_6"))];
        let name = |text: &str| RowName::Name(text.into());
        let marker = Marker::with_terms(vec![
            term(name("Row_5"), 1),
            term(name("row_6"), 2),
            // A number names a named row (by its place) and gives it no generation: the row stays an original. One past the rows names none.
            term(RowName::Number(1), 3),
            term(RowName::Number(9), 3),
        ])
        .unwrap();
        let (named, added) = name_terms(&rows, &marker);
        assert_eq!(named, [Some(1), Some(2), Some(0), None], "Row_5 is the row of that spelling, row_6 the first row of the name in any case");
        assert_eq!(added, HashMap::from([(1, 1), (2, 2)]), "ROW_5 is an original");
        // A name no row spells so is the first row of the name.
        let marker = Marker::with_terms(vec![term(name("rOw_5"), 4)]).unwrap();
        let (named, added) = name_terms(&rows, &marker);
        assert_eq!((named, added), (vec![Some(0)], HashMap::from([(0, 4)])));
        // A name that no row has names none.
        let marker = Marker::with_terms(vec![term(name("Row_7"), 4)]).unwrap();
        assert_eq!(name_terms(&rows, &marker), (vec![None], HashMap::new()));
    }

    #[test]
    fn a_number_names_the_row_of_that_ix_in_a_numbered_section_and_gives_it_its_generation() {
        // `IX` 0, 1 and 5: a number is the `IX` plus one, and a row that is not there is named by no number. A name names no row of a section with none.
        let rows = [row(0, None), row(1, None), row(5, None)];
        let marker = Marker::with_terms(vec![
            term(RowName::Number(6), 2),
            term(RowName::Number(1), 1),
            term(RowName::Number(3), 3),
            term(RowName::Name("Row_1".into()), 3),
        ])
        .unwrap();
        let (named, added) = name_terms(&rows, &marker);
        assert_eq!(named, [Some(5), Some(0), None, None]);
        assert_eq!(added, HashMap::from([(5, 2), (0, 1)]));
    }
}
