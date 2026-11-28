//! The edit: doubling and halving the connection points of the shapes a call names (design §3 to §9).
//!
//! **Two steps, read and then write.** [`prepare`] reads every shape the call names and works out, for each, what the call would do: the points a doubling makes (their places,
//! formulas, ways out and rows), the points a halving takes and the glue it keeps; every budget is charged there, and a call that can change nothing is refused in words. Only then
//! does [`apply`] write, shape by shape, and settle once. The whole call is one edit of a copy of the document (`Session::edit_if`), so a refusal at any step leaves the document as
//! it was.
//!
//! **A shape that cannot be changed is told, not refused as a call**: a group, a line, a connector, a shape on a locked layer, a shape past a cap of its own. The call
//! is refused only when no shape is changed, with every reason.

use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use diacraft_calc::row_refs::RowName;
use diacraft_calc::{Budget, EvalResult, Value as Calc, evaluate_text};
use diacraft_doc::{Cell, CellAddr, CellEdit, Connect, Connects, Document, Row, RowKey, ShapeRef, SheetRef, Value};
use diacraft_geom::kurbo::Point;

use super::edges::{Edge, MAX_EDGES_PER_SHAPE, PointRow, SectionCost};
use super::ends::Section as GeomSection;
use super::layout::{
    Blended, Candidate, blend, close_gaps, common_kind, common_way_out, halving_picks, millimetres, new_places, newest_generation, next_generation,
    outward, successor,
};
use super::local::{self, Inherited};
use super::{
    Action, Answer, EdgeChange, EdgeSkipped, EndReglued, Loaded, MAX_FOLLOW_COST, MAX_FOLLOW_STEPS, MAX_MIN_GAP, MAX_POINTS_PER_SHAPE,
    MAX_REGLUED_ENDS, MAX_ROWS_WRITTEN, MAX_SHAPES_PER_CALL, MIN_MIN_GAP, Query, Request, ShapeChange, fits,
};
use crate::draw::conn_marker::{self, Marker, Term};
use crate::draw::conn_refs::{Fate, MAX_SCAN_BYTES, MAX_SCAN_CELLS, Place, RowEntry, RowMap, ScanWork, renumber_connects, renumber_page};
use crate::draw::connectors::TO_PART_BASE;
use crate::draw::glue::WHOLE_SHAPE;
use crate::draw::{Named, Problem, Which, page_mut, require_page, scan, settle, write_cells};
use crate::stencil::number_text;

/// How close two places must be for a point stored as a formula to stand where the edge says (inches).
const PLACE_TOLERANCE: f64 = 1.0e-6;

// ---- the request -----------------------------------------------------------------------------------------------------------------------

fn validate(request: &Request) -> Result<(), Problem> {
    if request.ids.is_empty() {
        return Err(Problem::bad("give at least one shape"));
    }
    if request.ids.len() > MAX_SHAPES_PER_CALL {
        return Err(Problem::bad(format!("at most {MAX_SHAPES_PER_CALL} shapes may be named")));
    }
    if !(MIN_MIN_GAP..=MAX_MIN_GAP).contains(&request.min_gap) {
        return Err(Problem::bad(format!("`min_gap` must be a number of paper inches from {MIN_MIN_GAP} to {MAX_MIN_GAP}")));
    }
    if request.edge.is_some() && request.at.is_some() {
        return Err(Problem::bad("give `edge` (an edge by its number) or `at` (the edge nearest a place), not both"));
    }
    if let Some(((x, y), reach)) = request.at
        && !(x.is_finite() && y.is_finite() && reach.is_finite() && reach > 0.0)
    {
        return Err(Problem::bad("`at` must be a place on the page in inches and `reach` a number of inches more than 0"));
    }
    if request.edge.is_some() && scan::distinct(&request.ids).len() > 1 {
        return Err(Problem::bad("`edge` names an edge of one shape: give one `id`"));
    }
    Ok(())
}

// ---- which edges ------------------------------------------------------------------------------------------------------------------------

/// A shape a call names, as it was read.
struct Read {
    id: u32,
    loaded: Result<Loaded, String>,
}

/// The edges of a shape a call changes: `all` when it names no edge and no place.
struct Targets {
    edges: Vec<usize>,
    all: bool,
}

fn skip_problem(skip: &super::Skip, id: u32) -> Problem {
    Problem::failed(skip.words(id))
}

/// The edge of the named shapes nearest the place `at` on the page within `reach` paper inches, as (the shape's place in the list, the edge): ties go to the earlier.
fn nearest_edge(q: &mut Query<'_>, reads: &[Read], at: (f64, f64), reach: f64) -> Result<Option<(usize, usize)>, Problem> {
    let mut best: Option<(f64, usize, usize)> = None;
    for (si, read) in reads.iter().enumerate() {
        let Ok(loaded) = &read.loaded else { continue };
        let back = loaded.map.inverse();
        if !back.as_coeffs().iter().all(|c| c.is_finite()) {
            continue;
        }
        let local = back * Point::new(at.0, at.1);
        for (ei, edge) in loaded.outline.edges.iter().enumerate() {
            let near = edge.nearest_within(local, reach * q.k, &mut q.work).map_err(|skip| skip_problem(&skip, read.id))?;
            if let Some(near) = near
                && best.is_none_or(|(d, _, _)| near.distance < d - 1.0e-12)
            {
                best = Some((near.distance, si, ei));
            }
        }
    }
    Ok(best.map(|(_, s, e)| (s, e)))
}

/// The edges each shape's call changes (§8.1): the one `edge` names, the edge nearest `at` within `reach` (every edge when there is none that near), or every edge.
fn targets_of(q: &mut Query<'_>, reads: &[Read], request: &Request) -> Result<Vec<Targets>, Problem> {
    let every = |read: &Read| match &read.loaded {
        Ok(loaded) => Targets { edges: (0..loaded.outline.edges.len()).collect(), all: true },
        Err(_) => Targets { edges: Vec::new(), all: true },
    };
    if let Some(number) = request.edge {
        let number = usize::try_from(number).unwrap_or(usize::MAX);
        return reads
            .iter()
            .map(|read| match &read.loaded {
                Ok(loaded) if number >= loaded.outline.edges.len() => Err(Problem::bad(format!(
                    "shape {} has {} edges, numbered from 0: there is no edge {number}",
                    read.id,
                    loaded.outline.edges.len()
                ))),
                Ok(_) => Ok(Targets { edges: vec![number], all: false }),
                Err(_) => Ok(Targets { edges: Vec::new(), all: false }),
            })
            .collect();
    }
    if let Some((place, reach)) = request.at
        && let Some((shape, edge)) = nearest_edge(q, reads, place, reach)?
    {
        return Ok(reads.iter().enumerate().map(|(i, _)| Targets { edges: if i == shape { vec![edge] } else { Vec::new() }, all: false }).collect());
    }
    Ok(reads.iter().map(every).collect())
}

// ---- what a shape comes to ------------------------------------------------------------------------------------------------------------

/// What one shape comes to in a call.
enum Outcome {
    /// It is changed: what to write.
    Changed(Box<Job>),
    /// A doubling found no room on any target edge (the smallest gap it would have made, in paper inches).
    NoRoom(f64),
    /// A halving found nothing to take.
    Nothing,
    /// It cannot be changed: its words.
    Skipped,
    /// It is no target of the call (another shape's edge was named).
    Untouched,
}

/// What to write for one changed shape.
struct Job {
    at: ShapeRef,
    id: u32,
    /// Every row the shape will hold, each with the key it will have.
    rows: Vec<Row>,
    inherited: Inherited,
    marker: Marker,
    halving: Option<Halving>,
    /// What the shape's path-form points are charged each time it is resized, in calc's steps; 0 for a halving.
    follow: u64,
}

/// What a halving writes besides the rows.
struct Halving {
    map: RowMap,
    /// The ends glued to a removed point that no point is left to take: glued to the whole shape.
    whole: Vec<EndChoice>,
    /// The ends glued to a removed point that go to another.
    moved: Vec<EndChoice>,
    /// The `Connect` rows the halving rewrites.
    ends: usize,
}

/// An end of a connector glued to a removed point: where it goes (the row's number after the halving, or the whole shape).
#[derive(Clone, Debug)]
struct EndChoice {
    connector: u32,
    /// Where the connector is, as the call found it.
    at: Option<ShapeRef>,
    which: Which,
    from: u32,
    to: Option<u32>,
}

fn base_change(loaded: &Loaded) -> ShapeChange {
    let total = loaded.rows.len();
    ShapeChange { id: loaded.id, edges: Vec::new(), skipped_edges: Vec::new(), points_before: total, points_after: total, skipped: None }
}

fn skipped(mut change: ShapeChange, words: String) -> (ShapeChange, Outcome) {
    change.skipped = Some(words);
    (change, Outcome::Skipped)
}

// ---- doubling --------------------------------------------------------------------------------------------------------------------------

/// A target edge a doubling has room on.
struct Doubling<'a> {
    edge: &'a Edge,
    adds: u32,
    /// The places of the edge's points along it, in their order.
    alongs: Vec<f64>,
    generations: Vec<u8>,
    ways: Vec<(f64, f64)>,
    kinds: Vec<f64>,
}

/// A point a doubling makes.
struct NewPoint {
    x: Blended,
    y: Blended,
    dir: (f64, f64),
    kind: f64,
}

/// The row keys the new points take: after the section's last row by `IX`, or the first free `Row_<n>` in a section keyed by name.
fn new_keys(loaded: &Loaded, count: usize) -> Option<Vec<RowKey>> {
    if loaded.rows.iter().any(|r| r.name.is_some()) {
        let used: HashSet<String> = loaded.rows.iter().filter_map(|r| r.name.as_ref().map(|n| n.to_ascii_lowercase())).collect();
        let free = (1_u64..).map(|n| format!("Row_{n}")).filter(|name| !used.contains(&name.to_ascii_lowercase()));
        return Some(free.take(count).map(|name| RowKey::name(&name)).collect());
    }
    let first = loaded.rows.iter().map(|r| r.index).max().map_or(Some(0), |m| m.checked_add(1))?;
    (0..count).map(|k| first.checked_add(u32::try_from(k).ok()?).map(RowKey::Ix)).collect()
}

/// How a marker names the row at `key`: by its name, else by its number (the `IX` plus one).
fn marker_row(key: &RowKey) -> Option<RowName> {
    match key {
        RowKey::Ix(ix) => ix.checked_add(1).map(RowName::Number),
        RowKey::Name(name) => Some(RowName::Name(name.to_string())),
    }
}

fn rounded(v: f64) -> f64 {
    let r = (v * 1.0e9).round() / 1.0e9;
    if r == 0.0 || !r.is_finite() { 0.0 } else { r }
}

/// A cell holding a length in inches: its formula, if it has one, and its value.
fn length_cell(b: &Blended) -> Cell {
    let value = Value::from_text(&number_text(b.value));
    match &b.formula {
        Some(formula) => Cell::with_formula(formula, Some(value)).with_unit("IN"),
        None => Cell::constant(value).with_unit("IN"),
    }
}

fn plain_cell(n: f64) -> Cell {
    Cell::constant(Value::from_text(&number_text(n)))
}

/// The row of a new point at `key`: `X`, `Y`, `DirX`, `DirY` and `Type`, and no other cell.
fn row_of(key: &RowKey, p: &NewPoint) -> Row {
    let mut row = Row::default();
    match key {
        RowKey::Ix(ix) => row.ix = Some(*ix),
        RowKey::Name(name) => row.name = Some(Arc::clone(name)),
    }
    for (name, cell) in [
        ("X", length_cell(&p.x)),
        ("Y", length_cell(&p.y)),
        ("DirX", plain_cell(p.dir.0)),
        ("DirY", plain_cell(p.dir.1)),
        ("Type", plain_cell(p.kind)),
    ] {
        row.cells.insert(diacraft_doc::CellName::new(name), cell);
    }
    row
}

/// What reading the sections of a shape costs and holds while its straight edges are worked out.
struct Ctx {
    sections: HashMap<Option<u32>, GeomSection>,
    /// The calc steps left for the dry runs of the path formulas.
    steps: u64,
}

/// The place of a new point on a path-form edge: `PNTX` and `PNTY` of `POINTALONGPATH` on the drawn row it falls in, each with calc's own value.
fn path_point(
    q: &Query<'_>,
    loaded: &Loaded,
    edge: &Edge,
    along: f64,
    key: &RowKey,
    place: Point,
    ctx: &mut Ctx,
) -> Result<(Blended, Blended), String> {
    let words = || {
        format!("shape {}: a point of its curve cannot be written as a formula that calc places where the curve is: it was left as it was", loaded.id)
    };
    let Some(section) = edge.section_ix else { return Err(words()) };
    let Some((segment, share)) = edge.segment_place(along) else { return Err(words()) };
    let call = format!("POINTALONGPATH(Geometry{}.Path,{},0,{segment})", u64::from(section) + 1, number_text(share));
    let sheet = SheetRef::Shape(loaded.at.clone());
    let mut value_of = |function: &str, cell: &str| -> Option<(String, f64)> {
        let text = format!("{function}({call})");
        let addr = CellAddr::in_row("Connection", None, key.clone(), cell);
        let done = evaluate_text(q.doc, &sheet, Some(&addr), &text, &Budget::with_steps(ctx.steps));
        ctx.steps = ctx.steps.saturating_sub(done.steps);
        match done.result {
            EvalResult::Value(Calc::Number(quantity)) if !done.budget_exhausted && quantity.stored.is_finite() => Some((text, quantity.stored)),
            _ => None,
        }
    };
    let (Some((xf, xv)), Some((yf, yv))) = (value_of("PNTX", "X"), value_of("PNTY", "Y")) else { return Err(words()) };
    if (xv - place.x).abs() > PLACE_TOLERANCE || (yv - place.y).abs() > PLACE_TOLERANCE {
        return Err(words());
    }
    Ok((Blended { formula: Some(xf), value: xv }, Blended { formula: Some(yf), value: yv }))
}

/// A new point on a straight edge: a share of the box, a plain length or a formula over the cells of its ends (§4.1), as the place of the edge allows.
fn straight_point(q: &mut Query<'_>, loaded: &Loaded, edge: &Edge, along: f64, place: Point, ctx: &mut Ctx) -> Result<(Blended, Blended), Problem> {
    let plain = |p: Point| (Blended { formula: None, value: rounded(p.x) }, Blended { formula: None, value: rounded(p.y) });
    let sheet = SheetRef::Shape(loaded.at.clone());
    let size = (q.reader.number(&sheet, "Width"), q.reader.number(&sheet, "Height"));
    let Some(section) = edge.section_ix else { return Ok(plain(place)) };
    if let Entry::Vacant(slot) = ctx.sections.entry(edge.section_ix) {
        slot.insert(GeomSection::read(&q.reader, &loaded.at, edge.section_ix, &mut q.work).map_err(|skip| skip_problem(&skip, loaded.id))?);
    }
    let Some(ends) = ctx.sections.get(&edge.section_ix).and_then(|s| s.ends(edge, section, size)) else { return Ok(plain(place)) };
    let t = if edge.length > 0.0 { along / edge.length } else { 0.0 };
    let ([xa, ya], [xb, yb]) = (&ends.0, &ends.1);
    let (x, y) = (blend(xa, xb, t, "Width", size.0), blend(ya, yb, t, "Height", size.1));
    // A cell that disagrees with the edge as it is drawn (a stale value, a row this does not read) is not trusted: the point is written as the length of its place.
    let x = if (x.value - place.x).abs() > PLACE_TOLERANCE { plain(place).0 } else { x };
    let y = if (y.value - place.y).abs() > PLACE_TOLERANCE { plain(place).1 } else { y };
    Ok((x, y))
}

/// One doubling's points on one edge: each with its formulas, its way out and its type.
fn points_on_edge(
    q: &mut Query<'_>,
    loaded: &Loaded,
    d: &Doubling<'_>,
    keys: &[RowKey],
    ctx: &mut Ctx,
) -> Result<Result<Vec<NewPoint>, String>, Problem> {
    let edge = d.edge;
    let kind = common_kind(&d.kinds);
    let way = common_way_out(&d.ways);
    let mut points = Vec::with_capacity(keys.len());
    for (along, key) in new_places(edge.length, edge.is_closed(), &d.alongs).into_iter().zip(keys) {
        let place = edge.place_at(along, &mut q.work).map_err(|skip| skip_problem(&skip, loaded.id))?;
        let (x, y) = if edge.kind == super::EdgeKind::Straight {
            straight_point(q, loaded, edge, along, place.point, ctx)?
        } else {
            match path_point(q, loaded, edge, along, key, place.point, ctx) {
                Ok(pair) => pair,
                Err(words) => return Ok(Err(words)),
            }
        };
        let normal = match (place.tangent, edge.run_area) {
            (Some(t), Some(area)) => outward((t.x, t.y), area),
            _ => (0.0, 0.0),
        };
        let dir = way.unwrap_or(normal);
        points.push(NewPoint { x, y, dir: (rounded(dir.0), rounded(dir.1)), kind });
    }
    Ok(Ok(points))
}

/// The target edges of a shape a doubling has room on, and those it has none on (with the gap they would have had).
fn doublings_of<'a>(q: &Query<'_>, loaded: &'a Loaded, targets: &Targets) -> (Vec<Doubling<'a>>, Vec<EdgeSkipped>) {
    let (mut doublings, mut skipped) = (Vec::new(), Vec::new());
    for &number in &targets.edges {
        let (Some(edge), Some(points)) = (loaded.outline.edges.get(number), loaded.on.edges.get(number)) else { continue };
        let (gap, adds) = q.room(edge, points);
        if adds == 0 {
            skipped.push(EdgeSkipped { edge: edge.number, gap, min_gap: q.min_gap });
            continue;
        }
        let row_of = |index: u32| loaded.rows.iter().find(|r| r.index == index);
        doublings.push(Doubling {
            edge,
            adds,
            alongs: points.iter().map(|p| p.along).collect(),
            generations: points.iter().map(|p| loaded.generation(p.index)).collect(),
            ways: points.iter().filter_map(|p| row_of(p.index)).map(|r| (r.dir_x, r.dir_y)).collect(),
            kinds: points.iter().filter_map(|p| row_of(p.index)).map(|r| r.kind).collect(),
        });
    }
    (doublings, skipped)
}

/// What following the curves of a doubling by formula costs: the charge in calc's steps for the shape's path-form points each time it is resized, or a refusal in words when a
/// section is too long to follow or the charge is past [`MAX_FOLLOW_COST`].
fn follow_cost(loaded: &Loaded, doublings: &[Doubling<'_>]) -> Result<u64, String> {
    let id = loaded.id;
    let curved: Vec<&Doubling<'_>> = doublings.iter().filter(|d| d.edge.kind != super::EdgeKind::Straight).collect();
    let section = |ix: Option<u32>| loaded.outline.sections.iter().find(|s| s.ix == ix);
    for d in &curved {
        match section(d.edge.section_ix) {
            Some(s) if d.edge.section_ix.is_some() => {
                if s.steps > MAX_FOLLOW_STEPS {
                    return Err(format!(
                        "the curve of shape {id} draws {} steps, past the {MAX_FOLLOW_STEPS} a point can follow by formula",
                        s.steps
                    ));
                }
            }
            _ => return Err(format!("the curve of shape {id} is in a section with no number a formula can name")),
        }
    }
    let worst = loaded.outline.sections.iter().map(SectionCost::walk).max().unwrap_or(0);
    let walk = |ix: u32| if ix == u32::MAX { worst } else { section(Some(ix)).map_or(worst, SectionCost::walk) };
    let held: u64 = loaded.rows.iter().filter_map(|r| r.path_section).map(walk).fold(0, u64::saturating_add);
    let new: u64 = curved.iter().map(|d| u64::from(d.adds).saturating_mul(d.edge.section_ix.map_or(worst, walk))).fold(0, u64::saturating_add);
    let cost = held.saturating_add(new).saturating_mul(2);
    // A doubling that adds no point on a curve costs the shape's path cells nothing more: only one that adds some is held to the cap.
    if new > 0 && cost > MAX_FOLLOW_COST {
        return Err(format!(
            "shape {id}: its points on curves would cost {cost} calc steps each time the shape is resized or its curve is edited, past the {MAX_FOLLOW_COST} allowed: it was left as it was"
        ));
    }
    Ok(cost)
}

/// Plans the doubling of the target edges of a shape (§3.2, §3.4, §4).
fn plan_double(q: &mut Query<'_>, loaded: &Loaded, targets: &Targets) -> Result<(ShapeChange, Outcome), Problem> {
    let id = loaded.id;
    let mut change = base_change(loaded);
    if loaded.outline.truncated && targets.all {
        return Ok(skipped(change, format!("shape {id} has more than {MAX_EDGES_PER_SHAPE} edges: a call on every edge skips it: name an edge")));
    }
    let (doublings, skipped_edges) = doublings_of(q, loaded, targets);
    change.skipped_edges = skipped_edges;
    if doublings.is_empty() {
        let least = change.skipped_edges.iter().map(|e| e.gap).fold(f64::INFINITY, f64::min);
        return Ok((change, Outcome::NoRoom(least)));
    }
    let total = u32::try_from(loaded.rows.len()).unwrap_or(u32::MAX);
    let adds = doublings.iter().fold(0_u32, |t, d| t.saturating_add(d.adds));
    if !fits(total, adds) {
        let after = total.saturating_add(adds);
        return Ok(skipped(
            change,
            format!("shape {id} would have {after} connection points, past the {MAX_POINTS_PER_SHAPE} a connector looks at: it was left as it was"),
        ));
    }
    let follow = match follow_cost(loaded, &doublings) {
        Ok(cost) => cost,
        Err(words) => return Ok(skipped(change, words)),
    };
    let Some(keys) = new_keys(loaded, usize::try_from(adds).unwrap_or(usize::MAX)) else {
        return Ok(skipped(change, format!("shape {id}: its connection points are numbered past what a row can hold")));
    };
    let mut ctx = Ctx { sections: HashMap::new(), steps: MAX_FOLLOW_COST };
    let (mut rows_new, mut terms, mut next) = (Vec::new(), loaded.marker.terms().to_vec(), 0_usize);
    for d in &doublings {
        let Some(generation) = next_generation(newest_generation(&d.generations).unwrap_or(0)) else {
            return Ok(skipped(change, format!("shape {id}: an edge's points are already of the newest generation a marker can hold")));
        };
        let count = usize::try_from(d.adds).unwrap_or(usize::MAX);
        let own = keys.get(next..next.saturating_add(count)).unwrap_or_default();
        next = next.saturating_add(count);
        let points = match points_on_edge(q, loaded, d, own, &mut ctx)? {
            Ok(points) => points,
            Err(words) => return Ok(skipped(change, words)),
        };
        for (key, point) in own.iter().zip(&points) {
            rows_new.push(row_of(key, point));
            if let Some(row) = marker_row(key) {
                terms.push(Term { row, generation });
            }
        }
        let before = u32::try_from(d.alongs.len()).unwrap_or(u32::MAX);
        change.edges.push(EdgeChange { edge: d.edge.number, kind: d.edge.kind, before, after: before.saturating_add(d.adds), generation });
    }
    change.points_after = loaded.rows.len() + rows_new.len();
    let marker = Marker::with_terms(terms)?;
    let mut rows = local::current_rows(&q.reader, &loaded.at, loaded.held);
    rows.extend(rows_new);
    let job = Job { at: loaded.at.clone(), id, rows, inherited: local::inherited(&q.reader, &loaded.at), marker, halving: None, follow };
    Ok((change, Outcome::Changed(Box::new(job))))
}

// ---- halving ---------------------------------------------------------------------------------------------------------------------------

/// The connectors a halving may meet, found once for the call: by `ID`, where each is.
struct Connectors {
    found: Named,
}

/// Which end a `Connect` row is of, by the cell it names.
fn which_of(from_cell: Option<&str>) -> Option<Which> {
    match from_cell {
        Some("BeginX" | "BeginY") => Some(Which::Begin),
        Some("EndX" | "EndY") => Some(Which::End),
        _ => None,
    }
}

/// A `Connect` row to a point of a shape, in the page's order.
#[derive(Clone, Copy, Debug)]
struct GluedEnd {
    connector: u32,
    which: Option<Which>,
    /// The row's number before the halving.
    row: u32,
}

/// The `Connect` rows of a page that name a point of one of the shapes a call names, found **in one pass over the rows** and kept by shape, so that a call on thousands of
/// shapes of a page of thousands of rows does not walk the rows for each.
#[derive(Default)]
struct Glue {
    /// By the shape whose point it is glued to: each end.
    to: HashMap<u32, Vec<GluedEnd>>,
    /// By the shape that holds the point: the rows of its own outward points that are glued to another shape (`FromPart` 100 or more).
    own: HashMap<u32, Vec<u32>>,
}

impl Glue {
    fn of(doc: &Document, page: u32, shapes: &HashSet<u32>) -> Glue {
        let mut glue = Glue::default();
        let Some(p) = doc.page(page) else { return glue };
        for c in p.connects.iter() {
            if shapes.contains(&c.to_sheet)
                && let Some(row) = c.to_part.and_then(|part| part.checked_sub(TO_PART_BASE))
            {
                glue.to.entry(c.to_sheet).or_default().push(GluedEnd { connector: c.from_sheet, which: which_of(c.from_cell.as_deref()), row });
            }
            if shapes.contains(&c.from_sheet)
                && let Some(row) = c.from_part.and_then(|part| part.checked_sub(TO_PART_BASE))
            {
                glue.own.entry(c.from_sheet).or_default().push(row);
            }
        }
        glue
    }

    /// The `ID`s of the connectors glued to a point of one of the shapes.
    fn connectors(&self) -> HashSet<u32> {
        self.to.values().flatten().map(|end| end.connector).collect()
    }

    fn ends(&self, shape: u32) -> &[GluedEnd] {
        self.to.get(&shape).map_or(&[], Vec::as_slice)
    }

    /// The `Connect` rows of `shape` that a halving rewrites: those glued to a point that goes or moves, on either side.
    fn rewritten(&self, shape: u32, moves: impl Fn(u32) -> bool) -> usize {
        let to = self.ends(shape).iter().filter(|end| moves(end.row)).count();
        let own = self.own.get(&shape).map_or(0, |rows| rows.iter().filter(|row| moves(**row)).count());
        to.saturating_add(own)
    }
}

/// The points of a shape that may take a removed point's connectors, among the rows that stay: those of its edge, else any that take connectors.
fn candidates_for(q: &Query<'_>, loaded: &Loaded, removed: &BTreeSet<u32>, edge: Option<usize>) -> Vec<Candidate> {
    let candidate = |row: &PointRow| {
        let page = q.reader.to_page(&loaded.at, (row.x, row.y));
        Candidate { ix: row.index, place: (row.x, row.y), page }
    };
    let on_edge: Vec<Candidate> = edge
        .and_then(|e| loaded.on.edges.get(e))
        .map(|points| {
            points
                .iter()
                .filter(|p| !removed.contains(&p.index))
                .filter_map(|p| loaded.rows.iter().find(|r| r.index == p.index))
                .map(candidate)
                .collect()
        })
        .unwrap_or_default();
    if !on_edge.is_empty() {
        return on_edge;
    }
    loaded.rows.iter().filter(|r| !removed.contains(&r.index) && (r.kind == 0.0 || r.kind == 2.0)).map(candidate).collect()
}

/// Where the other end of a connector is on the page, as its cells hold it.
fn far_end(q: &Query<'_>, connectors: &Connectors, end: &GluedEnd) -> Option<(f64, f64)> {
    let at = connectors.found.get(end.connector)?;
    let sheet = SheetRef::Shape(at.clone());
    let (x, y) = match end.which? {
        Which::Begin => ("EndX", "EndY"),
        Which::End => ("BeginX", "BeginY"),
    };
    Some(q.reader.parent_to_page(at, (q.reader.number(&sheet, x), q.reader.number(&sheet, y))))
}

/// The marker after a halving: the terms of the rows that stay, renumbered; those of removed rows dropped, never mapped. Each term is of the row the shape's
/// reading found it names (`Loaded::term_rows`: the exact spelling first, and a row that spells the name in another case only when none has it), the row `generation` took
/// the term's generation for, so a row that spells a name in another case keeps its own term and never takes another's.
fn halved_marker(loaded: &Loaded, removed: &[u32]) -> Result<Marker, Problem> {
    let mut terms = Vec::new();
    for (term, row) in loaded.marker.terms().iter().zip(&loaded.term_rows) {
        let Some(index) = *row else { continue };
        let Some(now) = close_gaps(removed, index) else { continue };
        let row = match &term.row {
            RowName::Number(_) => RowName::Number(now.saturating_add(1)),
            other => other.clone(),
        };
        terms.push(Term { row, generation: term.generation });
    }
    Marker::with_terms(terms)
}

/// The points a halving takes from the target edges (the newest doubling of each), with what it did to each edge.
fn taken_from(loaded: &Loaded, targets: &Targets) -> (BTreeSet<u32>, Vec<EdgeChange>) {
    let (mut removed, mut changes) = (BTreeSet::new(), Vec::new());
    for &number in &targets.edges {
        let (Some(edge), Some(points)) = (loaded.outline.edges.get(number), loaded.on.edges.get(number)) else { continue };
        let generations = loaded.generations(number);
        let taken: Vec<u32> = points.iter().zip(halving_picks(&generations)).filter(|(_, pick)| *pick).map(|(p, _)| p.index).collect();
        if taken.is_empty() {
            continue;
        }
        let before = u32::try_from(points.len()).unwrap_or(u32::MAX);
        let gone = u32::try_from(taken.len()).unwrap_or(u32::MAX);
        changes.push(EdgeChange {
            edge: edge.number,
            kind: edge.kind,
            before,
            after: before.saturating_sub(gone),
            generation: newest_generation(&generations).unwrap_or(0),
        });
        removed.extend(taken);
    }
    (removed, changes)
}

/// The row map of a halving, and where each end glued to a removed point goes.
fn map_of(
    q: &Query<'_>,
    loaded: &Loaded,
    removed: &BTreeSet<u32>,
    connectors: &Connectors,
    glue: &Glue,
) -> Result<(RowMap, Vec<EndChoice>), Problem> {
    let gone: Vec<u32> = removed.iter().copied().collect();
    let edge_of = |index: u32| loaded.on.edges.iter().position(|points| points.iter().any(|p| p.index == index));
    let new_ix = |ix: u32| close_gaps(&gone, ix);
    let mut entries = Vec::with_capacity(loaded.rows.len());
    // The points that may take a removed point's connectors are found once for each removed point, not once for each end glued to it.
    let mut taking: HashMap<u32, Vec<Candidate>> = HashMap::new();
    for row in &loaded.rows {
        let name = row.name.as_ref().map(|n| n.to_string());
        let fate = match new_ix(row.index) {
            Some(to) => Fate::Kept(to),
            None => {
                let candidates = taking.entry(row.index).or_insert_with(|| candidates_for(q, loaded, removed, edge_of(row.index)));
                let chosen = successor((row.x, row.y), candidates, None);
                let successor_row = chosen.and_then(|c| Some((new_ix(c.ix)?, loaded.rows.iter().find(|r| r.index == c.ix)?)));
                Fate::Removed(successor_row.map(|(index, r)| Place { index, name: r.name.as_ref().map(|n| n.to_string()) }))
            }
        };
        entries.push(RowEntry { index: row.index, name, fate });
    }
    let map = RowMap::new(entries)?;
    // Each end glued to a removed point chooses for itself: the nearer of two as near by its connector's other end.
    let mut choices = Vec::new();
    for end in glue.ends(loaded.id).iter().filter(|e| removed.contains(&e.row)) {
        let Some(which) = end.which else { continue };
        let Some(row) = loaded.rows.iter().find(|r| r.index == end.row) else { continue };
        let candidates = taking.get(&end.row).map_or(&[][..], Vec::as_slice);
        let chosen = successor((row.x, row.y), candidates, far_end(q, connectors, end));
        let at = connectors.found.get(end.connector).cloned();
        choices.push(EndChoice { connector: end.connector, at, which, from: end.row, to: chosen.and_then(|c| new_ix(c.ix)) });
    }
    Ok((map, choices))
}

/// Plans the halving of the target edges of a shape (§3.3, §3.5, §6).
fn plan_halve(q: &Query<'_>, loaded: &Loaded, targets: &Targets, connectors: &Connectors, glue: &Glue) -> Result<(ShapeChange, Outcome), Problem> {
    let id = loaded.id;
    let mut change = base_change(loaded);
    let (removed, edges) = taken_from(loaded, targets);
    if removed.is_empty() {
        change.skipped = Some(format!("shape {id}: nothing to halve: it has no points a doubling made"));
        return Ok((change, Outcome::Nothing));
    }
    if let Some(row) = loaded.rows.iter().find(|r| removed.contains(&r.index) && r.guarded) {
        return Ok(skipped(change, format!("connection point {} of shape {id} is guarded (GUARD) and was not changed", row.index)));
    }
    let gone: Vec<u32> = removed.iter().copied().collect();
    let (mut map, choices) = map_of(q, loaded, &removed, connectors, glue)?;
    for choice in choices.iter().filter(|c| c.to.is_some()) {
        if let Some(to) = choice.to {
            map.glue_end(choice.connector, choice.which, to);
        }
    }
    let ends = glue.rewritten(id, |row| removed.contains(&row) || close_gaps(&gone, row) != Some(row));
    let mut rows = Vec::new();
    for (copy, row) in local::current_rows(&q.reader, &loaded.at, loaded.held).into_iter().zip(&loaded.rows) {
        let Some(now) = close_gaps(&gone, row.index) else { continue };
        let mut copy = copy;
        if copy.name.is_none() {
            copy.ix = Some(now);
        }
        rows.push(copy);
    }
    change.edges = edges;
    change.points_after = rows.len();
    let marker = halved_marker(loaded, &gone)?;
    let (whole, moved): (Vec<EndChoice>, Vec<EndChoice>) = choices.into_iter().partition(|c| c.to.is_none());
    let job = Job {
        at: loaded.at.clone(),
        id,
        rows,
        inherited: local::inherited(&q.reader, &loaded.at),
        marker,
        halving: Some(Halving { map, whole, moved, ends }),
        follow: 0,
    };
    Ok((change, Outcome::Changed(Box::new(job))))
}

// ---- the call --------------------------------------------------------------------------------------------------------------------------

/// What [`prepare`] has worked out.
struct Prepared {
    shapes: Vec<ShapeChange>,
    jobs: Vec<Job>,
    scale_note: Option<String>,
    work: super::OutlineWork,
}

/// The words of a call that changes nothing: every reason (§8.1).
fn refusal(shapes: &[(ShapeChange, Outcome)], request: &Request) -> Problem {
    let mut gaps = Vec::new();
    let (mut nothing, mut reasons) = (0_usize, Vec::new());
    for (change, outcome) in shapes {
        match outcome {
            Outcome::NoRoom(gap) => gaps.push(*gap),
            Outcome::Nothing => nothing += 1,
            Outcome::Skipped => reasons.extend(change.skipped.clone()),
            Outcome::Changed(_) | Outcome::Untouched => {}
        }
    }
    let least = gaps.iter().copied().fold(f64::INFINITY, f64::min);
    let no_room = || {
        format!(
            "no edge has room: a doubling would put points {} mm apart, under the minimum of {} mm (Options)",
            millimetres(least),
            millimetres(request.min_gap)
        )
    };
    let none_to_take = "nothing to halve: the edge has no points a doubling made".to_owned();
    match (reasons.is_empty(), gaps.is_empty(), nothing) {
        (true, false, 0) => Problem::failed(no_room()),
        (true, true, n) if n > 0 => Problem::failed(none_to_take),
        (true, true, _) => Problem::failed("no shape was changed"),
        _ => {
            if !gaps.is_empty() {
                reasons.push(no_room());
            }
            if nothing > 0 {
                reasons.push(none_to_take);
            }
            let words = reasons.join("; ");
            if request.action == Action::Double && gaps.is_empty() { Problem::bad(words) } else { Problem::failed(words) }
        }
    }
}

/// Reads the shapes and works out what the call does to each, or refuses it. Writes nothing.
fn prepare(doc: &Document, page: u32, request: &Request) -> Result<Prepared, Problem> {
    let ids = scan::distinct(&request.ids);
    let named = Named::of(doc, page, ids.iter().copied())?;
    let (mut q, scale_note) = Query::start(doc, page, request.min_gap);
    let mut reads = Vec::with_capacity(ids.len());
    for id in &ids {
        let at = named.require(*id)?;
        reads.push(Read { id: *id, loaded: q.loaded(*id, &at)? });
    }
    let targets = targets_of(&mut q, &reads, request)?;
    // A halving reads the page's `Connect` rows once, and finds the connectors glued to the points in one walk of the page.
    let (glue, connectors) = match request.action {
        Action::Halve => {
            let glue = Glue::of(doc, page, &ids.iter().copied().collect());
            let found = Named::of(doc, page, glue.connectors())?;
            (glue, Connectors { found })
        }
        Action::Double => (Glue::default(), Connectors { found: Named::of(doc, page, std::iter::empty())? }),
    };
    let mut planned: Vec<(ShapeChange, Outcome)> = Vec::with_capacity(reads.len());
    let (mut rows, mut ends) = (0_usize, 0_usize);
    for (read, target) in reads.iter().zip(&targets) {
        let part = match &read.loaded {
            Err(words) => (
                ShapeChange {
                    id: read.id,
                    edges: Vec::new(),
                    skipped_edges: Vec::new(),
                    points_before: 0,
                    points_after: 0,
                    skipped: Some(words.clone()),
                },
                Outcome::Skipped,
            ),
            Ok(loaded) if target.edges.is_empty() => (base_change(loaded), Outcome::Untouched),
            Ok(loaded) => match request.action {
                Action::Double => plan_double(&mut q, loaded, target)?,
                Action::Halve => plan_halve(&q, loaded, target, &connectors, &glue)?,
            },
        };
        if let Outcome::Changed(job) = &part.1 {
            rows = rows.saturating_add(job.rows.len());
            ends = ends.saturating_add(job.halving.as_ref().map_or(0, |h| h.ends));
            if rows > MAX_ROWS_WRITTEN {
                return Err(Problem::failed(format!(
                    "the call would write {rows} connection point rows, past the {MAX_ROWS_WRITTEN} one call writes: ask for fewer shapes"
                )));
            }
            if ends > MAX_REGLUED_ENDS {
                return Err(Problem::failed(format!("more than {MAX_REGLUED_ENDS} connector ends are glued to the points of shape {}", read.id)));
            }
        }
        planned.push(part);
    }
    if !planned.iter().any(|(_, outcome)| matches!(outcome, Outcome::Changed(_))) {
        return Err(refusal(&planned, request));
    }
    let (mut shapes, mut jobs) = (Vec::with_capacity(planned.len()), Vec::new());
    for (change, outcome) in planned {
        shapes.push(change);
        if let Outcome::Changed(job) = outcome {
            jobs.push(*job);
        }
    }
    Ok(Prepared { shapes, jobs, scale_note, work: q.work })
}

// ---- writing ---------------------------------------------------------------------------------------------------------------------------

/// Glues the ends that no point is left to take to the whole shape (`ToPart` 3, `ToCell` `PinX`): the Connect rows, and the end's cells as the `_WALKGLUE` formulas of a whole-shape end
/// with the places they are now as their results, and the end's trigger naming the shape, which the engine then places as any whole-shape end is placed. Done before
/// the references are renumbered, so that nothing names a row that has no successor.
fn glue_to_whole(doc: &mut Document, page: u32, shape: u32, ends: &[EndChoice]) -> Result<(), Problem> {
    if ends.is_empty() {
        return Ok(());
    }
    let wanted: HashSet<(u32, bool)> = ends.iter().map(|e| (e.connector, e.which == Which::Begin)).collect();
    let rows: HashSet<u32> = ends.iter().map(|e| e.from).collect();
    let writes = {
        let reader = crate::draw::Reader::new(doc);
        let mut writes = Vec::new();
        for end in ends {
            let Some(at) = &end.at else { continue };
            let sheet = SheetRef::Shape(at.clone());
            let (x, y) = match end.which {
                Which::Begin => ("BeginX", "BeginY"),
                Which::End => ("EndX", "EndY"),
            };
            for cell in [x, y] {
                let place = crate::draw::walkglue::end_cell(end.which, reader.number(&sheet, cell));
                writes.push((sheet.clone(), CellAddr::direct(cell), CellEdit::Cell(place)));
            }
            let trigger = crate::draw::walkglue::trigger_name(end.which);
            writes.push((sheet.clone(), CellAddr::direct(trigger), CellEdit::Cell(crate::draw::walkglue::trigger_cell(shape))));
        }
        writes
    };
    write_cells(doc, writes)?;
    let p = page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?;
    let list: Vec<Connect> = p
        .connects
        .iter()
        .map(|c| {
            let end = which_of(c.from_cell.as_deref()).map(|which| (c.from_sheet, which == Which::Begin));
            let on_row = c.to_sheet == shape && c.to_part.and_then(|part| part.checked_sub(TO_PART_BASE)).is_some_and(|row| rows.contains(&row));
            if on_row && end.is_some_and(|e| wanted.contains(&e)) {
                Connect { to_part: Some(WHOLE_SHAPE), to_cell: Some(Arc::from("PinX")), ..c.clone() }
            } else {
                c.clone()
            }
        })
        .collect();
    p.connects = Connects::from_vec(list);
    Ok(())
}

/// Writes one planned shape: for a halving the ends first, then every reference, then the rows and the marker.
fn write_job(doc: &mut Document, page: u32, job: Job, scan_work: &mut ScanWork) -> Result<(usize, usize), Problem> {
    let (mut formulas, mut end_cells) = (0, 0);
    if let Some(halving) = &job.halving {
        glue_to_whole(doc, page, job.id, &halving.whole)?;
        let done = renumber_page(doc, page, job.id, &halving.map, scan_work)?;
        renumber_connects(doc, page, job.id, &halving.map, scan_work)?;
        (formulas, end_cells) = (done.formulas, done.end_cells);
    }
    local::write_own(doc, &job.at, job.rows, &job.inherited)?;
    conn_marker::write(doc, &job.at, &job.marker)?;
    Ok((formulas, end_cells))
}

/// Writes what [`prepare`] planned and settles once.
fn apply(doc: &mut Document, page: u32, request: &Request, prepared: Prepared, before: &Document) -> Result<Answer, Problem> {
    let Prepared { shapes, jobs, scale_note, work } = prepared;
    let mut scan_work = ScanWork::limited(MAX_SCAN_CELLS, MAX_SCAN_BYTES, MAX_REGLUED_ENDS);
    let (mut touched, mut extra, mut reglued, mut renumbered) = (Vec::new(), Vec::new(), Vec::new(), 0_usize);
    let follow_cost = jobs.iter().map(|job| job.follow).max().unwrap_or(0);
    for job in jobs {
        if let Some(halving) = &job.halving {
            touched.push(job.id);
            for end in halving.moved.iter().chain(&halving.whole) {
                extra.push(end.connector);
                reglued.push(EndReglued { connector: end.connector, begin: end.which == Which::Begin, shape: job.id, from: end.from, to: end.to });
            }
        }
        let (formulas, end_cells) = write_job(doc, page, job, &mut scan_work)?;
        renumbered = renumbered.saturating_add(formulas.saturating_sub(end_cells));
    }
    settle(doc, before, page, &touched, &extra)?;
    Ok(Answer {
        page,
        action: request.action,
        changed: true,
        min_gap: request.min_gap,
        scale_note,
        shapes,
        reglued,
        renumbered,
        work,
        scan: scan_work,
        follow_cost,
    })
}

/// Doubles or halves the points of the shapes the request names (see the module's account).
pub(super) fn run(doc: &mut Document, page: u32, request: &Request) -> Result<Answer, Problem> {
    validate(request)?;
    require_page(doc, page)?;
    let before = doc.clone();
    let prepared = prepare(doc, page, request)?;
    apply(doc, page, request, prepared, &before)
}
