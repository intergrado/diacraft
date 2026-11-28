//! The shapes a command works on: whether each may take part (SHAPEOPS-NEEDS, SHAPEOPS-LOCKS), and what its geometry is in the container's
//! coordinates.
//!
//! **Validating** ([`validate`]) finds the shapes in one walk of the page, checks they share one parent and meets the needs of R12 that can be told from the shape
//! alone, and the locks of R13, and builds nothing. **Reading** ([`read`]) turns each operand's shown `Geometry` sections into paths:
//!
//! * The rows are the shape's effective ones (its master's put together with its own), read by the renderer's own reader of a row's cells
//!   and built by `diacraft_geom::build_paths_bounded`, so what is worked on is the curve that is drawn: an ellipse is an ellipse and an arc is an arc.
//! * The paths are carried **through the shape's own pose** into its container's coordinates: the page's, for a shape on the page, and the group's own frame, for a
//!   member. The groups round a member are not part of it: a result inside a turned group is as turned as the members beside it.
//! * A section with `NoShow` 1 is not read. A section with `NoFill` 0 gives its fill ring, which closes an open path by a straight edge, and has a
//!   fill area when that ring is thicker than [`MERGE`] in some direction ([`Sect::takes_fill`]: a line there and back has none). That is judged from the ring, by no
//!   sweep of the path library, which costs what the crossings of its edges do and no count here bounds. A section with `NoFill` 1 or an open path is a cutting line for
//!   Fragment.
//!
//! **The work is bounded before it is done.** Every count is charged to a [`Budget`] from what the sheets hold, before a section is merged or built; a shape that
//! is over what is left is refused in words. (The one cost these counts do not bound is the library's own sweep of edges that cross in very many places: that is the
//! library's limit to set, and a command runs it once.)

use std::collections::HashSet;

use diacraft_doc::{CellAddr, Document, RowKey, Shape, ShapeRef, ShapeType, SheetRef};
use diacraft_geom::{Bound, GeomRow, SectionFlags, rules};
use kurbo::{Affine, BezPath, PathEl, Point, Size, Vec2};

use super::{Budget, MAX_SECTIONS_PER_SHAPE, MERGE, Op, ops};
use crate::draw::layers::shape_layers;
use crate::draw::scan::{self, Scan};
use crate::draw::{Pose, Problem, Reader};

// How many rows the scan for a shown path has looked at on this thread, for the test that holds the check of Offset and Fit Curve to the first rows of each shape. Not kept
// outside tests.
#[cfg(test)]
thread_local! {
    pub(super) static ROWS_SCANNED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The most rows of the page's `Layer` section that are read, for the locks.
const MAX_LAYERS: usize = 4_096;

/// The most shapes one command takes (the commands' own limit on ids, `cmd::params::MAX_IDS`).
pub const MAX_OPERANDS: usize = 4_096;

/// The shapes a command was given, found and checked.
#[derive(Debug)]
pub(super) struct Picked {
    pub scan: Scan,
    /// The shapes in the order the command named them, the first shape first (SHAPEOPS-PRIMARY).
    pub shapes: Vec<(u32, ShapeRef)>,
    /// The path of the container they are in: empty for the page.
    pub parent: Vec<u32>,
}

/// One shown `Geometry` section of an operand, in the container's coordinates.
#[derive(Debug)]
pub(super) struct Sect {
    pub ix: Option<u32>,
    pub flags: SectionFlags,
    pub no_snap: bool,
    pub no_quick_drag: bool,
    /// The rows as numbers, in the shape's own frame: kept for Combine, which carries them, and empty for the others.
    pub rows: Vec<GeomRow>,
    /// How many rows the section holds, kept or not: what Fit Curve compares a fit's rows with.
    pub row_count: usize,
    /// The path as drawn, with a gap at each move.
    pub stroke: BezPath,
    /// The ring that is filled: every move a straight edge, closed once.
    pub fill: BezPath,
    /// Whether the ring may cover an area (`ops::may_cover_area`): a line there and back, a point and a run along one line cover none.
    pub covers: bool,
    /// Whether some run of the path does not come back to where it started.
    pub open: bool,
}

impl Sect {
    /// Whether the section has a fill area: it takes the fill (`NoFill` 0) and its ring may cover an area. An open path along one line with `NoFill` 0 takes the fill and has
    /// none, so it is an open section, which Subtract keeps.
    pub(super) fn takes_fill(&self) -> bool {
        !self.flags.no_fill && self.covers
    }
}

/// One shape taking part.
#[derive(Debug)]
pub(super) struct Operand {
    pub id: u32,
    pub at: ShapeRef,
    /// `Width` and `Height`, which the relative rows are shares of.
    pub size: (f64, f64),
    /// The shape's own frame to its container's.
    pub map: Affine,
    pub sections: Vec<Sect>,
}

impl Operand {
    /// The fill area: the rings of the sections that have one, together, to be filled by the odd-crossings rule.
    pub(super) fn fill(&self) -> BezPath {
        let mut all = BezPath::new();
        for section in self.sections.iter().filter(|s| s.takes_fill()) {
            for el in section.fill.elements() {
                all.push(*el);
            }
        }
        all
    }

    /// Whether some section has a fill area, judged without a sweep.
    pub(super) fn has_fill(&self) -> bool {
        self.sections.iter().any(Sect::takes_fill)
    }

    /// The cutting lines: the paths of the sections that are open or have `NoFill` 1.
    pub(super) fn cuts(&self) -> Vec<BezPath> {
        self.sections.iter().filter(|s| s.flags.no_fill || s.open).filter(|s| has_segment(&s.stroke)).map(|s| s.stroke.clone()).collect()
    }

    /// Whether the shape has some path to draw.
    pub(super) fn has_path(&self) -> bool {
        self.sections.iter().any(|s| has_segment(&s.stroke))
    }
}

fn has_segment(path: &BezPath) -> bool {
    path.elements().iter().any(|el| !matches!(el, PathEl::MoveTo(_) | PathEl::ClosePath))
}

/// A shape's own frame to its container's: the steps of `Pose::up` as one map (subtract the local pin, flip, turn, add the pin).
pub(super) fn pose_map(pose: &Pose) -> Affine {
    let flip = Affine::scale_non_uniform(if pose.flip_x { -1.0 } else { 1.0 }, if pose.flip_y { -1.0 } else { 1.0 });
    Affine::translate(Vec2::new(pose.pin.0, pose.pin.1)) * Affine::rotate(pose.angle) * flip * Affine::translate(Vec2::new(-pose.loc.0, -pose.loc.1))
}

/// The ids of the shapes on `page` that have a connector's end glued to something: a `Connect` row from them, by an end cell.
fn glued_connectors(doc: &Document, page: u32) -> HashSet<u32> {
    let Some(p) = doc.page(page) else { return HashSet::new() };
    p.connects.iter().filter(|c| matches!(c.from_cell.as_deref(), Some("BeginX" | "BeginY" | "EndX" | "EndY"))).map(|c| c.from_sheet).collect()
}

/// The layers of the page that are locked: the `IX` of each row of its `Layer` section with a `Lock` that is not 0.
fn locked_layers(reader: &Reader<'_>, page: u32) -> Result<HashSet<u32>, Problem> {
    let sheet = SheetRef::Page(page);
    let key = diacraft_doc::SectionKey::new("Layer", None);
    let rows = match reader.resolver().effective_rows_within(&sheet, &key, MAX_LAYERS) {
        None => return Ok(HashSet::new()),
        Some(Ok(rows)) => rows,
        Some(Err(n)) => return Err(Problem::failed(format!("the page has {n} layers, and a shape operation reads at most {MAX_LAYERS}"))),
    };
    Ok(rows
        .into_iter()
        .filter_map(|row| match row.key {
            RowKey::Ix(ix) => (reader.addr_number(&sheet, &CellAddr::in_row("Layer", None, row.key.clone(), "Lock")) != 0.0).then_some(ix),
            RowKey::Name(_) => None,
        })
        .collect())
}

/// The text of a cell directly under a sheet, as found by inheritance: a cell that holds a formula and no saved value is what the formula works out to, as the
/// picture reads it.
fn text_of(reader: &Reader<'_>, sheet: &SheetRef, addr: &CellAddr) -> Option<String> {
    let found = reader.resolver().resolve(sheet, addr)?;
    reader.worked().text_of(sheet, addr, &found)
}

/// Whether the shape is a container or a list: `User.msvStructureType` says so.
fn is_container_or_list(reader: &Reader<'_>, sheet: &SheetRef) -> bool {
    let addr = CellAddr::in_row("User", None, RowKey::name("msvStructureType"), "Value");
    text_of(reader, sheet, &addr).is_some_and(|t| matches!(t.trim(), "Container" | "List"))
}

/// Whether a layer the shape is on is locked: some layer its **own** `LayerMember` names (a list an instance would take from its master is not read), of the page's
/// table, has `Lock` set. The list is read as every reader of one is, bounded (`layers::shape_layers`).
fn on_locked_layer(shape: &Shape, locked: &HashSet<u32>) -> bool {
    !locked.is_empty() && shape_layers(shape).iter().any(|ix| locked.contains(ix))
}

/// Why the shape may not take part, if it may not.
fn refusal(reader: &Reader<'_>, doc: &Document, id: u32, at: &ShapeRef, glued: &HashSet<u32>, locked: &HashSet<u32>) -> Option<String> {
    let shape = doc.shape(at)?;
    let sheet = SheetRef::Shape(at.clone());
    let what = |why: &str| Some(format!("shape {id} {why}"));
    if shape.kind == ShapeType::Group || !shape.children.is_empty() {
        return what("is a group: select inside it, and its members are worked on");
    }
    match shape.kind {
        ShapeType::Guide => return what("is a guide"),
        ShapeType::Foreign => return what("is a picture or an embedded object, which has no geometry of its own to work on"),
        _ => {}
    }
    if is_container_or_list(reader, &sheet) || super::ties::holds_members(reader, &sheet) {
        return what("is a container or a list");
    }
    if (reader.is_one_d(&sheet) && glued.contains(&id)) || reader.is_routed(&sheet) {
        return what("is a connector");
    }
    if on_locked_layer(shape, locked) {
        return what("is on a locked layer");
    }
    None
}

/// Finds the shapes named, checks them against what the command needs, and says why not in words: the document is not changed and nothing
/// is built. The first of `ids` is the first shape.
pub(super) fn validate(doc: &Document, page: u32, op: Op, ids: &[u32]) -> Result<Picked, Problem> {
    let ids = scan::distinct(ids);
    if ids.len() > MAX_OPERANDS {
        return Err(Problem::bad(format!("{} takes at most {MAX_OPERANDS} shapes", op.name())));
    }
    if ids.len() < op.fewest() {
        let need = if op.fewest() == 1 { "a shape" } else { "two shapes at least" };
        return Err(Problem::bad(format!("{} needs {need} (a shape named twice counts once)", op.name())));
    }
    let (found, document_order) = scan::named(doc, page, &ids)?;
    let by_id: std::collections::HashMap<u32, ShapeRef> = document_order.into_iter().collect();
    let shapes: Vec<(u32, ShapeRef)> = ids.iter().filter_map(|id| by_id.get(id).map(|at| (*id, at.clone()))).collect();
    let parent: Vec<u32> = shapes.first().and_then(|(_, at)| at.path.split_last().map(|(_, up)| up.to_vec())).unwrap_or_default();
    if shapes.iter().any(|(_, at)| at.path.split_last().map(|(_, up)| up) != Some(parent.as_slice())) {
        return Err(Problem::bad("the shapes are not all on the page or all members of one group: select shapes that share one container"));
    }
    let reader = Reader::new(doc);
    let (glued, locked) = (glued_connectors(doc, page), locked_layers(&reader, page)?);
    for (id, at) in &shapes {
        if let Some(why) = refusal(&reader, doc, *id, at, &glued, &locked) {
            return Err(Problem::bad(format!("{} cannot work on it: {why}", op.name())));
        }
    }
    // The locks: a command that deletes an operand is refused when the operand cannot be deleted; a command that changes an operand's vertices (Subtract's first
    // shape, every shape of Fit Curve) is refused when they are locked. Offset deletes and changes nothing, so no lock refuses it.
    for (k, (id, at)) in shapes.iter().enumerate() {
        let sheet = SheetRef::Shape(at.clone());
        if op.deletes(k) && reader.number(&sheet, "LockDelete") != 0.0 {
            return Err(Problem::bad(format!("shape {id} cannot be deleted (`LockDelete`), so {} is refused", op.name())));
        }
        if op.edits_vertices(k) && reader.number(&sheet, "LockVtxEdit") != 0.0 {
            return Err(Problem::bad(format!("shape {id} has its vertices locked (`LockVtxEdit`), so {} is refused", op.name())));
        }
    }
    Ok(Picked { scan: found, shapes, parent })
}

/// What it costs to read the constructors of a section's rows (`PolylineTo`'s `A`, `NURBSTo`'s `E`): their bytes, known from their lengths alone.
fn constructor_bytes(rows: &[diacraft_doc::EffectiveRow<'_>]) -> usize {
    rows.iter()
        .filter_map(|row| {
            let name = match row.row_type.as_deref()? {
                "PolylineTo" => "A",
                "NURBSTo" => "E",
                _ => return None,
            };
            row.cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(_, r)| r.value.cell).and_then(|c| c.value.as_ref()).map(|v| v.text().len())
        })
        .fold(0_usize, usize::saturating_add)
}

/// Whether a path has a run that does not come back to its start.
fn has_open_run(path: &BezPath) -> bool {
    let (mut start, mut pen, mut closed_by_close, mut any) = (None::<Point>, None::<Point>, false, false);
    let mut open = false;
    let end_run = |start: Option<Point>, pen: Option<Point>, closed: bool, any: bool, open: &mut bool| {
        if let (Some(s), Some(p)) = (start, pen)
            && any
            && !closed
            && s.distance(p) > MERGE
        {
            *open = true;
        }
    };
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                end_run(start, pen, closed_by_close, any, &mut open);
                (start, pen, closed_by_close, any) = (Some(p), Some(p), false, false);
            }
            PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => {
                pen = Some(p);
                any = true;
            }
            PathEl::ClosePath => closed_by_close = true,
        }
    }
    end_run(start, pen, closed_by_close, any, &mut open);
    open
}

fn flag(reader: &Reader<'_>, sheet: &SheetRef, ix: Option<u32>, name: &str) -> bool {
    reader.addr_number(sheet, &CellAddr::in_section("Geometry", ix, name)) != 0.0
}

/// The shown sections of the shape at `at`, built and carried to the container, charged to `budget`.
fn sections(
    reader: &Reader<'_>,
    id: u32,
    at: &ShapeRef,
    size: (f64, f64),
    map: Affine,
    budget: &mut Budget,
    keep_rows: bool,
) -> Result<Vec<Sect>, Problem> {
    let sheet = SheetRef::Shape(at.clone());
    let res = reader.resolver();
    let too_much = |what: &str| Problem::failed(format!("shape {id} has more geometry than a shape operation reads: {what}"));
    let keys = match res.effective_sections_within(&sheet, "Geometry", MAX_SECTIONS_PER_SHAPE) {
        Some(Ok(keys)) => keys,
        Some(Err(n)) => return Err(too_much(&format!("{n} sections, at most {MAX_SECTIONS_PER_SHAPE}"))),
        None => return Ok(Vec::new()),
    };
    let mut out = Vec::new();
    for key in keys {
        // Every section looked at is charged, a hidden one and an empty one too: each costs a lookup, and 4,096 shapes of 1,024 hidden sections are four million.
        budget.sections = budget.sections.checked_sub(1).ok_or_else(|| too_much("too many sections in all the shapes"))?;
        if flag(reader, &sheet, key.ix, "NoShow") {
            continue;
        }
        let Some(raw) = res.rows_held(&sheet, &key).filter(|n| *n > 0) else { continue };
        budget.rows = budget.rows.checked_sub(raw).ok_or_else(|| too_much("too many rows in all the shapes"))?;
        let Some(Ok(rows)) = res.effective_rows_within(&sheet, &key, raw) else { continue };
        budget.bytes = budget.bytes.checked_sub(constructor_bytes(&rows)).ok_or_else(|| too_much("too much constructor text in all the shapes"))?;
        let (geom, _) = reader.worked().geom_rows(&sheet, &key, &rows);
        let bound = Bound { curve_work: budget.work, path_elements: budget.elements };
        let built = diacraft_geom::build_paths_bounded(&geom, Size::new(size.0, size.1), rules::FILL_RUN_JOIN, bound)
            .map_err(|e| too_much(&format!("a section cannot be built ({e})")))?;
        budget.work = budget.work.saturating_sub(built.curve_work);
        let (mut stroke, mut fill) = (built.stroke, built.fill);
        budget.elements = budget.elements.saturating_sub(stroke.elements().len().saturating_add(fill.elements().len()));
        stroke.apply_affine(map);
        fill.apply_affine(map);
        if !(path_is_finite(&stroke) && path_is_finite(&fill)) {
            return Err(Problem::failed(format!("shape {id} has geometry with a number that is not finite")));
        }
        let flags = SectionFlags { no_fill: flag(reader, &sheet, key.ix, "NoFill"), no_line: flag(reader, &sheet, key.ix, "NoLine"), no_show: false };
        out.push(Sect {
            ix: key.ix,
            flags,
            no_snap: flag(reader, &sheet, key.ix, "NoSnap"),
            no_quick_drag: flag(reader, &sheet, key.ix, "NoQuickDrag"),
            row_count: geom.len(),
            rows: if keep_rows { geom } else { Vec::new() },
            covers: ops::may_cover_area(&fill),
            open: has_open_run(&stroke),
            stroke,
            fill,
        });
    }
    Ok(out)
}

fn path_is_finite(path: &BezPath) -> bool {
    path.elements().iter().all(|el| match *el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => p.is_finite(),
        PathEl::QuadTo(a, p) => a.is_finite() && p.is_finite(),
        PathEl::CurveTo(a, b, p) => a.is_finite() && b.is_finite() && p.is_finite(),
        PathEl::ClosePath => true,
    })
}

/// Reads the operands of a command, in the order they were named, charging `budget`. Refuses what the command needs and a shape lacks: some shown geometry, and
/// for Union, Intersect and Subtract (and for one operand of Fragment) a fill area.
pub(super) fn read(doc: &Document, op: Op, picked: &Picked, budget: &mut Budget) -> Result<Vec<Operand>, Problem> {
    let reader = Reader::new(doc);
    let mut operands = Vec::with_capacity(picked.shapes.len());
    for (id, at) in &picked.shapes {
        let sheet = SheetRef::Shape(at.clone());
        let pose = reader.pose(&sheet);
        let map = pose_map(&pose);
        let size = (reader.number(&sheet, "Width"), reader.number(&sheet, "Height"));
        if !(map.as_coeffs().iter().all(|c| c.is_finite()) && size.0.is_finite() && size.1.is_finite()) {
            return Err(Problem::failed(format!("shape {id} has a position or a size that is not a number")));
        }
        // Fit Curve changes a shape in place, in the shape's own frame: its sections are read there, which is where the rows are written. The others work in the container's.
        let into = if op == Op::FitCurve { Affine::IDENTITY } else { map };
        let sections = sections(&reader, *id, at, size, into, budget, op == Op::Combine)?;
        operands.push(Operand { id: *id, at: at.clone(), size, map, sections });
    }
    needs(op, &operands)?;
    Ok(operands)
}

/// Whether the shape at `at` shows some path, judged from the types of the rows alone: some shown `Geometry` section (not `NoShow`) with a row that is not a `MoveTo`, in the
/// section as the shape holds it or as its master does. Nothing is built and no cell is read but the section's `NoShow`, and the rows are looked at until one is found.
/// Sections and rows are charged to `budget` as [`sections`] charges them, so that what is scanned is bounded by what a command reads. (A row of the shape that gives a type of
/// its own to one of the master's is not told from the master's: a shape that this finds a path in and the reading does not is refused by the command, with the same words.)
fn shows_a_path(reader: &Reader<'_>, id: u32, at: &ShapeRef, budget: &mut Budget) -> Result<bool, Problem> {
    let sheet = SheetRef::Shape(at.clone());
    let res = reader.resolver();
    let too_much = |what: &str| Problem::failed(format!("shape {id} has more geometry than a shape operation reads: {what}"));
    let keys = match res.effective_sections_within(&sheet, "Geometry", MAX_SECTIONS_PER_SHAPE) {
        Some(Ok(keys)) => keys,
        Some(Err(n)) => return Err(too_much(&format!("{n} sections, at most {MAX_SECTIONS_PER_SHAPE}"))),
        None => return Ok(false),
    };
    for key in keys {
        budget.sections = budget.sections.checked_sub(1).ok_or_else(|| too_much("too many sections in all the shapes"))?;
        if flag(reader, &sheet, key.ix, "NoShow") {
            continue;
        }
        let Some(raw) = res.rows_held(&sheet, &key).filter(|n| *n > 0) else { continue };
        budget.rows = budget.rows.checked_sub(raw).ok_or_else(|| too_much("too many rows in all the shapes"))?;
        let draws = res.section_layers(&sheet, &key).unwrap_or_default().iter().any(|layer| {
            layer.rows.iter().any(|row| {
                #[cfg(test)]
                ROWS_SCANNED.with(|c| c.set(c.get().saturating_add(1)));
                !row.del && row.row_type.as_deref().is_some_and(|kind| !matches!(kind, "MoveTo" | "RelMoveTo"))
            })
        });
        if draws {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether Offset or Fit Curve could run on these shapes, as far as what they need of the geometry: a shown path in each, and nothing more. It is the cheap form of
/// [`read`] for the two, which need no fill area: a scan of row types ([`shows_a_path`]), where reading builds every stroke and fill of every shape (a shape of 100,000 rows
/// took 140 ms to be asked about). The document is not changed.
pub(super) fn check_paths(doc: &Document, op: Op, picked: &Picked, budget: &mut Budget) -> Result<(), Problem> {
    let reader = Reader::new(doc);
    for (id, at) in &picked.shapes {
        if !shows_a_path(&reader, *id, at, budget)? {
            return Err(no_geometry(op, *id));
        }
    }
    Ok(())
}

fn no_geometry(op: Op, id: u32) -> Problem {
    Problem::bad(format!("{} cannot work on shape {id}: it has no geometry to draw (text only, or every section hidden)", op.name()))
}

/// What each command needs of the geometry: a path in each operand, and a fill area where the command says so. Whether a shape has a fill area is judged from
/// its rings and not by a sweep of the library (`Sect::takes_fill`): the sweep of an operand costs what its crossings do, which no count here bounds. A ring that passes and
/// covers nothing is refused by the operation's own result.
fn needs(op: Op, operands: &[Operand]) -> Result<(), Problem> {
    for operand in operands {
        if !operand.has_path() {
            return Err(no_geometry(op, operand.id));
        }
        if op.needs_fill_area() && !operand.has_fill() {
            return Err(Problem::bad(format!(
                "{} needs a fill area in each shape, and shape {} has none (only lines, or `NoFill` on every path)",
                op.name(),
                operand.id
            )));
        }
    }
    if op == Op::Fragment && !operands.iter().any(Operand::has_fill) {
        return Err(Problem::bad("Fragment needs a shape with a fill area, which the lines then cut"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn a_turned_and_flipped_pose_carries_a_point_as_the_poses_own_steps_do() {
        for (fx, fy) in [(false, false), (true, false), (false, true), (true, true)] {
            for angle in [0.0, 0.5, std::f64::consts::FRAC_PI_2, -2.0] {
                let pose = Pose { pin: (5.0, 3.0), loc: (1.0, 0.5), angle, flip_x: fx, flip_y: fy };
                let map = pose_map(&pose);
                for p in [(0.0, 0.0), (2.0, 1.0), (-3.5, 7.25)] {
                    let by_map = map * Point::new(p.0, p.1);
                    let by_pose = pose.up(p);
                    assert!((by_map.x - by_pose.0).abs() < 1e-9 && (by_map.y - by_pose.1).abs() < 1e-9, "{pose:?} {p:?}");
                }
            }
        }
    }

    #[test]
    fn a_run_that_does_not_come_back_is_open_and_one_that_closes_is_not() {
        let mut open = BezPath::new();
        open.move_to((0.0, 0.0));
        open.line_to((1.0, 1.0));
        assert!(has_open_run(&open));
        let mut ring = BezPath::new();
        ring.move_to((0.0, 0.0));
        ring.line_to((1.0, 0.0));
        ring.line_to((1.0, 1.0));
        ring.line_to((0.0, 0.0));
        assert!(!has_open_run(&ring));
        let mut closed = BezPath::new();
        closed.move_to((0.0, 0.0));
        closed.line_to((1.0, 0.0));
        closed.line_to((1.0, 1.0));
        closed.close_path();
        assert!(!has_open_run(&closed));
        let mut two = open.clone();
        two.move_to((5.0, 5.0));
        two.line_to((6.0, 5.0));
        two.line_to((6.0, 6.0));
        two.line_to((5.0, 5.0));
        assert!(has_open_run(&two), "one open run among closed ones");
    }
}
