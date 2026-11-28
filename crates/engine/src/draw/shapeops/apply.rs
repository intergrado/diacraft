//! A command's work on the document: the originals go by the engine's own delete (Offset keeps them), the produced shapes are put
//! at the front of the container, what reads them is calculated, and for Subtract and Fit Curve the geometry and the frame of the shapes are changed in place.
//!
//! Nothing here keeps its own copy of the rules for glue, guides or jumps: [`delete_originals`] (the engine's `delete_paths`) settles them (a connector glued to a shape that goes
//! keeps that end where it is, unglued, and its `Connect` row goes), which is R14. The caller (`Session::edit`) works on a copy of the document, so a refusal at
//! any step leaves it as it was, and a success is one undo step.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellName, Document, MAX_SECTIONS_PER_SHEET, MAX_SHAPES_PER_PAGE, Owner, Section, Shape, ShapeRef, ShapeType, Sheet, SheetRef,
};
use kurbo::{Affine, BezPath, Point, Rect, Shape as _, Vec2};

use super::operand::{self, Operand, Picked, pose_map};
use super::rows::{self, Flags, Frame};
use super::runs::{Run, runs_of};
use super::{Args, Budget, MAX_LOOK_UNITS, MAX_PRODUCED, MAX_REACH, MAX_WORK_EXTENT, MERGE, Op, carry, fit, join, look, offset, ops, trim};
use crate::draw::shapes::delete_paths;
use crate::draw::{Pose, Problem, Reader, page_mut, require_page, settle, shape_mut, write_length};
use crate::stencil::text_transform_cells;

/// What a command did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Made {
    /// The `ID`s of the shapes it made, in the order of the drawing (the last frontmost, SHAPEOPS-ORDER).
    pub produced: Vec<u32>,
    /// The `ID`s of the shapes it deleted.
    pub deleted: Vec<u32>,
    /// The shape that stayed and was changed: Subtract's first shape.
    pub kept: Option<u32>,
    /// The `ID`s of the shapes it changed in place, besides Subtract's first shape: Fit Curve's, those whose geometry the fit made smaller.
    pub changed: Vec<u32>,
    /// The `ID`s of the shapes Fit Curve **left as they were** though they hold something to fit (a section of more than two rows): the tolerance was too tight for the fit to
    /// make any of them smaller, or the library gave up on them (its work for a command is bounded, and past it the rest stays as it was). Empty for the others, and for a
    /// tolerance of 0, which asks for an exact match and changes nothing by design.
    pub not_fitted: Vec<u32>,
}

/// Whether the command may run on these shapes, and if not why not in words: everything `run` checks before it builds anything, and the reading of
/// each shape's geometry that says whether it has a path and a fill area (for Offset and Fit Curve, which need only a path, a scan of the types of the rows). The document is
/// not changed. It costs what reading the shapes costs, so a window asks it for a selection and not for each frame.
pub fn check(doc: &Document, page: u32, op: Op, ids: &[u32]) -> Result<(), String> {
    require_page(doc, page).map_err(|p| p.message)?;
    let picked = operand::validate(doc, page, op, ids).map_err(|p| p.message)?;
    if matches!(op, Op::Offset | Op::FitCurve) {
        // These two need no fill area, only a shown path in each shape, which the row types tell: the shapes are not built.
        return operand::check_paths(doc, op, &picked, &mut Budget::new()).map_err(|p| p.message);
    }
    operand::read(doc, op, &picked, &mut Budget::new()).map(|_| ()).map_err(|p| p.message)
}

/// Runs the operation on the shapes `ids` of a page, the first named being the first shape, with no arguments: for the seven that take none. The new shapes' `ID`s
/// are above `above` and above every `ID` on the page. A refusal is a [`Problem`] and the document may then be half changed: the caller works on a copy.
pub fn run(doc: &mut Document, page: u32, op: Op, ids: &[u32], above: u32) -> Result<Made, Problem> {
    run_with(doc, page, op, ids, above, &Args::default())
}

/// [`run`] with the arguments of Offset (the distance) and of Fit Curve (the tolerance and the flags), which are refused in words when they are not ones the
/// command can run with ([`Args::check`]).
pub fn run_with(doc: &mut Document, page: u32, op: Op, ids: &[u32], above: u32, args: &Args) -> Result<Made, Problem> {
    require_page(doc, page)?;
    args.check(op)?;
    let picked = operand::validate(doc, page, op, ids)?;
    let operands = operand::read(doc, op, &picked, &mut Budget::new())?;
    // Fit Curve is given what the cells hold as it is; Offset works on each section near its own origin and checks its size there.
    if op == Op::FitCurve {
        check_reach(op, &operands)?;
    }
    match op {
        Op::Subtract => subtract(doc, page, &picked, &operands),
        Op::FitCurve => fit_curve(doc, page, &operands, args),
        Op::Union | Op::Combine | Op::Fragment | Op::Intersect | Op::Join | Op::Trim | Op::Offset => {
            build(doc, page, op, &picked, &operands, above, args)
        }
    }
}

/// Refuses shapes that cover more than [`MAX_REACH`] in the cells' own units, in words: a shape that large is past what the sweep, and the numbers the rows are written in,
/// take.
fn check_reach(op: Op, operands: &[Operand]) -> Result<(), Problem> {
    let all = operands.iter().flat_map(|o| o.sections.iter().map(|s| &s.stroke));
    if let Some(rect) = ops::bounds(all)
        && rect.width().max(rect.height()) > MAX_REACH
    {
        return Err(Problem::failed(format!(
            "{} cannot work on shapes that cover more than {MAX_REACH} (the paths are too large for the path library)",
            op.name()
        )));
    }
    Ok(())
}

// ---- the new shapes -------------------------------------------------------------------------------------------------------------

/// One shape to make: its sections and the box they are shares of, in the working frame.
struct Piece {
    sections: Vec<Section>,
    frame: Frame,
}

fn moved(mut path: BezPath, by: Affine) -> BezPath {
    path.apply_affine(by);
    path
}

/// The loops of a path as one piece: a section for each loop, in the box that holds them. `Ok(None)` when nothing is left of it.
fn piece_of(path: &BezPath) -> Result<Option<Piece>, Problem> {
    let loops = rows::clean(path);
    let Some(rect) = rows::bounds(&loops) else { return Ok(None) };
    if loops.len() > MAX_SECTIONS_PER_SHEET {
        return Err(Problem::failed(format!(
            "the result has {} boundaries, and a shape holds at most {MAX_SECTIONS_PER_SHEET} sections",
            loops.len()
        )));
    }
    let frame = Frame::of(rect);
    let sections = loops.iter().enumerate().map(|(i, ring)| rows::section_of(ring, &frame, u32::try_from(i).unwrap_or(u32::MAX))).collect();
    Ok(Some(Piece { sections, frame }))
}

fn one_piece(path: &BezPath, nothing: &str) -> Result<Vec<Piece>, Problem> {
    piece_of(path)?.map(|piece| vec![piece]).ok_or_else(|| Problem::failed(nothing))
}

/// Combine's piece: a section for each shown section of each operand, its rows carried exactly.
fn combined(operands: &[Operand], to_work: Affine) -> Result<Vec<Piece>, Problem> {
    let mut carried = Vec::new();
    let mut strokes = Vec::new();
    for operand in operands {
        for section in operand.sections.iter().filter(|s| s.stroke.elements().len() > 1) {
            carried.push((carry::carry(&section.rows, operand.size, to_work * operand.map), section));
            strokes.push(moved(section.stroke.clone(), to_work));
        }
    }
    let rect = ops::bounds(&strokes).ok_or_else(|| Problem::failed("the shapes have no geometry to combine"))?;
    if carried.len() > MAX_SECTIONS_PER_SHEET {
        return Err(Problem::failed(format!("the shapes have {} sections, and a shape holds at most {MAX_SECTIONS_PER_SHEET}", carried.len())));
    }
    let frame = Frame::of(rect);
    let sections =
        carried.iter().enumerate().map(|(i, (rows, from))| carry::section_of(rows, from, &frame, u32::try_from(i).unwrap_or(u32::MAX))).collect();
    Ok(vec![Piece { sections, frame }])
}

/// The paths of the shown sections of every operand, in selection order and then section order, carried to the working frame: one run for each `MoveTo`.
fn runs_of_operands(operands: &[Operand], to_work: Affine) -> Vec<Run> {
    operands.iter().flat_map(|o| o.sections.iter()).flat_map(|section| runs_of(&moved(section.stroke.clone(), to_work))).collect()
}

/// Join's piece: a section for each chain of the paths whose ends meet, in the order the chains began, the paths already closed passing through as sections
/// of their own.
fn joined(operands: &[Operand], to_work: Affine) -> Result<Vec<Piece>, Problem> {
    let runs = runs_of_operands(operands, to_work);
    if runs.is_empty() {
        return Err(Problem::failed("the shapes have no path to join"));
    }
    let mut steps = join::STEPS;
    let chains = join::chains(&runs, &mut steps)?;
    if chains.len() > MAX_SECTIONS_PER_SHEET {
        return Err(Problem::failed(format!("the shapes make {} chains, and a shape holds at most {MAX_SECTIONS_PER_SHEET} sections", chains.len())));
    }
    let paths: Vec<BezPath> = chains.iter().map(Run::path).collect();
    let rect = ops::bounds(&paths).ok_or_else(|| Problem::failed("the shapes have no geometry to join"))?;
    let frame = Frame::of(rect);
    let sections = chains
        .iter()
        .enumerate()
        .map(|(i, chain)| rows::section_of_run(chain, &frame, u32::try_from(i).unwrap_or(u32::MAX), !chain.closed()))
        .collect();
    Ok(vec![Piece { sections, frame }])
}

/// Trim's pieces: a shape for each stretch of the paths between the places where they cross or touch, each in a box of its own.
fn trimmed(operands: &[Operand], to_work: Affine) -> Result<Vec<Piece>, Problem> {
    let mut runs = runs_of_operands(operands, to_work);
    if runs.is_empty() {
        return Err(Problem::failed("the shapes have no path to trim"));
    }
    let cuts = trim::cuts(&mut runs)?;
    let mut made = Vec::with_capacity(cuts.len());
    for cut in &cuts {
        let frame = Frame::of(cut.run.path().bounding_box());
        let section = rows::section_of_run(&cut.run, &frame, 0, !cut.whole);
        made.push(Piece { sections: vec![section], frame });
    }
    Ok(made)
}

/// A piece to make with the map from the frame it is in to the container's, and the operand whose formatting it takes: the first for every command but Offset, whose copies
/// each follow their own original.
struct Planned {
    piece: Piece,
    from_work: Affine,
    look_of: usize,
}

/// Whether the rectangle is a box: its corner and its sides are numbers (a box of paths of 1E308 apart has a side that overflows).
fn is_box(rect: &Rect) -> bool {
    rect.x0.is_finite() && rect.y0.is_finite() && rect.width().is_finite() && rect.height().is_finite()
}

/// The piece that holds `runs`, a section for each, in the box that holds them: `None` when they hold nothing (a point, or paths shorter than [`MERGE`]);
/// a number that is not finite, in a run or in the box (it overflows), is refused in words. A closed run takes the fill (`NoFill` 0) and an open one does not.
fn piece_of_runs(runs: &[Run]) -> Result<Option<Piece>, Problem> {
    let runs: Vec<&Run> = runs.iter().filter(|r| !r.segs.is_empty()).collect();
    if !runs.iter().all(|r| r.is_finite()) {
        return Err(Problem::failed("a result has a number that is not finite"));
    }
    let Some(rect) = runs.iter().map(|r| r.bounds()).reduce(|a, b| a.union(b)) else { return Ok(None) };
    if !is_box(&rect) {
        return Err(Problem::failed("a result is too large for its box to be a number"));
    }
    if rect.width().hypot(rect.height()) <= MERGE {
        return Ok(None);
    }
    if runs.len() > MAX_SECTIONS_PER_SHEET {
        return Err(Problem::failed(format!("the result has {} paths, and a shape holds at most {MAX_SECTIONS_PER_SHEET} sections", runs.len())));
    }
    let frame = Frame::of(rect);
    let sections =
        runs.iter().enumerate().map(|(i, run)| rows::section_of_run(run, &frame, u32::try_from(i).unwrap_or(u32::MAX), !run.closed())).collect();
    Ok(Some(Piece { sections, frame }))
}

/// Offset's shapes: for each shown section of each operand, the shape of its outer or left side and the shape of its inner or right side, each taking its own
/// original's formatting. Each section is worked on near its own origin, so that what the library does (its sweep's tolerance is the same wherever a shape is) is the same
/// wherever the shape is, and its piece carries the map back. The reach of every section is checked before the first library call.
fn offset_pieces(operands: &[Operand], args: &Args) -> Result<Vec<Planned>, Problem> {
    let too_many = || Problem::failed(format!("Offset would make more than {MAX_PRODUCED} shapes, and a command makes at most {MAX_PRODUCED}"));
    let mut sections: Vec<(usize, Vec2, Vec<Run>)> = Vec::new();
    for (k, operand) in operands.iter().enumerate() {
        for section in &operand.sections {
            let Some(rect) = ops::bounds(std::iter::once(&section.stroke)) else { continue };
            let reach = rect.width().max(rect.height()) / 2.0 + 2.0 * args.distance;
            if reach.is_nan() || reach > MAX_WORK_EXTENT {
                return Err(Problem::failed(format!(
                    "Offset cannot work on paths that cover more than {MAX_WORK_EXTENT} once the distance is added (half the size of a path and twice the distance must come to no more: the paths are too large for the path library)"
                )));
            }
            let origin = rect.center().to_vec2();
            let runs = runs_of(&moved(section.stroke.clone(), Affine::translate(-origin)));
            if !runs.is_empty() {
                sections.push((k, origin, runs));
            }
        }
    }
    // A side of a section is one shape with a section for each of its paths: more paths than a shape holds sections are refused before any library work is done for them.
    if let Some((_, _, runs)) = sections.iter().find(|(_, _, runs)| runs.len() > MAX_SECTIONS_PER_SHEET) {
        return Err(Problem::failed(format!("the result has {} paths, and a shape holds at most {MAX_SECTIONS_PER_SHEET} sections", runs.len())));
    }
    // The library's sweep work for the command is one limit, shared among the calls it makes (`offset::Meter`).
    let meter = offset::Meter::of(sections.iter().map(|(_, _, runs)| runs.as_slice()));
    let mut planned: Vec<Planned> = Vec::new();
    for (k, origin, runs) in &sections {
        let sides = offset::sides(runs, args.distance, &meter)?;
        for side in [sides.first, sides.second] {
            if let Some(piece) = piece_of_runs(&side)? {
                if planned.len() >= MAX_PRODUCED {
                    return Err(too_many());
                }
                planned.push(Planned { piece, from_work: Affine::translate(*origin), look_of: *k });
            }
        }
    }
    if planned.is_empty() {
        return Err(Problem::failed("no copy of the shapes is left a distance away: nothing is made"));
    }
    Ok(planned)
}

/// The pieces the operation makes, in the working frame (the container's coordinates less `to_work`'s move).
fn pieces(op: Op, operands: &[Operand], to_work: Affine) -> Result<Vec<Piece>, Problem> {
    let fills = || operands.iter().map(|o| moved(o.fill(), to_work)).collect::<Vec<_>>();
    match op {
        Op::Union => one_piece(&ops::union(&fills())?, "the shapes have no area to unite"),
        Op::Intersect => one_piece(&ops::intersect(&fills())?, "the shapes have no area in common"),
        Op::Combine => combined(operands, to_work),
        Op::Fragment => {
            let cuts: Vec<BezPath> = operands.iter().flat_map(|o| o.cuts()).map(|c| moved(c, to_work)).collect();
            // A shape with no fill area (an open path along one line with `NoFill` 0, which is a line there and back) is no region, only a cutting line.
            let regions: Vec<BezPath> = operands.iter().filter(|o| o.has_fill()).map(|o| moved(o.fill(), to_work)).collect();
            let faces = ops::fragment(&regions, &cuts)?;
            // The cap is met as the pieces are built, and the work stops there: a result of a hundred thousand faces is not all built to be counted.
            let mut made = Vec::with_capacity(faces.len().min(MAX_PRODUCED));
            for face in &faces {
                if let Some(piece) = piece_of(face)? {
                    if made.len() >= MAX_PRODUCED {
                        return Err(Problem::failed(format!(
                            "Fragment would make more than {MAX_PRODUCED} shapes, and a command makes at most {MAX_PRODUCED}"
                        )));
                    }
                    made.push(piece);
                }
            }
            if made.is_empty() {
                return Err(Problem::failed("the shapes have no region to make a shape of"));
            }
            Ok(made)
        }
        Op::Join => joined(operands, to_work),
        Op::Trim => trimmed(operands, to_work),
        Op::Subtract | Op::FitCurve => Err(Problem::failed(format!("{} changes shapes and makes none", op.name()))),
        Op::Offset => Err(Problem::failed("Offset works on each section on its own: see `offset_pieces`")),
    }
}

fn put(cells: &mut std::collections::BTreeMap<CellName, Cell>, name: &str, cell: Cell) {
    cells.insert(CellName::new(name), cell);
}

/// The frame of a produced shape, as R9 says: unturned and unflipped, the pin in the middle of its box, the local pin half the size by formula, the text block the
/// whole shape. The cells that go with it.
fn frame_cells(centre: Point, width: f64, height: f64) -> Vec<(CellName, Cell)> {
    let mut cells = std::collections::BTreeMap::new();
    put(&mut cells, "PinX", rows::length(centre.x));
    put(&mut cells, "PinY", rows::length(centre.y));
    put(&mut cells, "Width", rows::length(width));
    put(&mut cells, "Height", rows::length(height));
    put(&mut cells, "LocPinX", rows::length_formula("Width*0.5", width * 0.5));
    put(&mut cells, "LocPinY", rows::length_formula("Height*0.5", height * 0.5));
    for flat in ["Angle", "FlipX", "FlipY", "ResizeMode"] {
        put(&mut cells, flat, rows::plain(0.0));
    }
    for (name, cell) in text_transform_cells(width, height) {
        put(&mut cells, name, cell);
    }
    cells.into_iter().collect()
}

/// The shape `id` for a piece: no master, no name, the frame of R9, the geometry, and the first shape's look. `shared` are the look's sections, held once for all
/// the shapes a command makes: each holds the same `Arc`s and not a copy.
fn shape_of(id: u32, piece: Piece, look: &look::Look, shared: &[Arc<Section>], from_work: Affine) -> Shape {
    let Piece { sections, frame } = piece;
    let centre = from_work * Point::new(frame.min.x + frame.width * 0.5, frame.min.y + frame.height * 0.5);
    let mut sheet = Sheet { links: look.links, ..Sheet::default() };
    for (name, cell) in frame_cells(centre, frame.width, frame.height).into_iter().chain(look.cells.iter().cloned()) {
        sheet.cells.insert(name, cell);
    }
    // The look's own sections first, the geometry after, as a file keeps them.
    sheet.sections.extend(shared.iter().map(Arc::clone));
    sheet.sections.extend(sections.into_iter().map(Arc::new));
    Shape { kind: ShapeType::Shape, sheet: Arc::new(sheet), text: look.text.clone(), ..Shape::new(id) }
}

/// The names of the direct cells a produced shape has: its frame's and the look's.
fn names_of(frame: &[(CellName, Cell)], look: &look::Look) -> BTreeSet<CellName> {
    frame.iter().map(|(n, _)| n.clone()).chain(look.cells.iter().map(|(n, _)| n.clone())).collect()
}

/// What one operand's formatting gives the shapes made of it: its look, and its sections held once (`Arc`s) for all the shapes made.
struct Looks {
    look: look::Look,
    shared: Vec<Arc<Section>>,
    /// What each shape made of it holds of the look and of its frame (the cells and the rows of the sections), as `MAX_LOOK_UNITS` counts.
    per_shape: usize,
}

/// The look of the operand `at`, ready for the shapes made of it: the text only where the command gives it, the formulas that name what a made shape lacks frozen
/// to their values, Offset's `FillPattern` 0, and for a command that makes one shape the ties of the operand's containers and lists (SHAPEOPS-CONTAINERS).
fn looks_of(reader: &Reader<'_>, op: Op, at: &ShapeRef, ties: bool) -> Result<Looks, Problem> {
    let mut look = look::capture(reader, at, op.gives_text())?;
    if op == Op::Offset {
        // An offset's copy has no fill, written here so that a style it is linked to cannot bring one back.
        look.cells.retain(|(name, _)| name.as_str() != "FillPattern");
        look.cells.push((CellName::new("FillPattern"), rows::plain(0.0)));
    }
    let frame = frame_cells(Point::ORIGIN, 0.0, 0.0);
    let have = names_of(&frame, &look);
    look::freeze_unportable(&mut look.cells, &mut look.sections, &have);
    // The ties name other shapes by `Sheet.<id>!`, which is what they are for, so they are added after the formulas that would be frozen for it.
    if ties && let Some(carried) = look.ties.take() {
        look.cells.push((CellName::new("Relationships"), carried));
    }
    let shared: Vec<Arc<Section>> = std::mem::take(&mut look.sections).into_iter().map(Arc::new).collect();
    let in_sections = shared.iter().fold(0_usize, |sum, section| sum.saturating_add(look::section_units(section)));
    let per_shape = in_sections.saturating_add(look.cells.len()).saturating_add(frame.len());
    Ok(Looks { look, shared, per_shape })
}

fn build(doc: &mut Document, page: u32, op: Op, picked: &Picked, operands: &[Operand], above: u32, args: &Args) -> Result<Made, Problem> {
    let planned: Vec<Planned> = if op == Op::Offset {
        offset_pieces(operands, args)?
    } else {
        // The working frame: the container's coordinates, moved so that the middle of everything worked on is at the origin. A small shape far out is then as easy to
        // sweep as one near the origin, and the library's tolerance does not grow with the distance.
        let all = operands.iter().flat_map(|o| o.sections.iter().flat_map(|s| [&s.stroke, &s.fill]));
        let rect = ops::bounds(all).ok_or_else(|| Problem::failed("the shapes have no geometry"))?;
        let to_work = Affine::translate(Vec2::new(-rect.center().x, -rect.center().y));
        let from_work = to_work.inverse();
        pieces(op, operands, to_work)?.into_iter().map(|piece| Planned { piece, from_work, look_of: 0 }).collect()
    };
    // A command that makes one shape puts it where the first shape was in containers and lists (SHAPEOPS-CONTAINERS); one that makes many puts none there, and Offset,
    // which keeps its originals where they are, puts its copies in none.
    let one_shape = planned.len() == 1 && op != Op::Offset;
    let reader = Reader::new(doc);
    let mut looks: BTreeMap<usize, Looks> = BTreeMap::new();
    for plan in &planned {
        if let std::collections::btree_map::Entry::Vacant(place) = looks.entry(plan.look_of) {
            let operand = operands.get(plan.look_of).ok_or_else(|| Problem::failed("there are no shapes"))?;
            place.insert(looks_of(&reader, op, &operand.at, one_shape)?);
        }
    }
    // What each shape made holds of the formatting it takes and of its frame is charged before any is built: the cells it is given (the original's own and the frame's)
    // and the rows and cells of the look's sections, which every shape shares in memory and a file written from the drawing writes for each. What it comes to times the
    // shapes, past `MAX_LOOK_UNITS`, is refused in words.
    let mut total = 0_usize;
    for (k, held) in &looks {
        let made_of_it = planned.iter().filter(|p| p.look_of == *k).count();
        total = total.saturating_add(held.per_shape.saturating_mul(made_of_it));
    }
    if total > MAX_LOOK_UNITS {
        return Err(match looks.values().next() {
            Some(held) if looks.len() == 1 => Problem::failed(format!(
                "the first shape's formatting holds {} cells and rows with the frame each shape is given, and {} shapes would each hold them: a command writes at most {MAX_LOOK_UNITS} in all",
                held.per_shape,
                planned.len()
            )),
            _ => Problem::failed(format!(
                "the formatting of the shapes holds {total} cells and rows with the frames of the {} shapes made, and a command writes at most {MAX_LOOK_UNITS} in all",
                planned.len()
            )),
        });
    }
    let gone: Vec<&(u32, ShapeRef)> = picked.shapes.iter().enumerate().filter(|(k, _)| op.deletes(*k)).map(|(_, found)| found).collect();
    let deleted: Vec<u32> = gone.iter().map(|(id, _)| *id).collect();
    if picked.scan.count.saturating_sub(deleted.len()).saturating_add(planned.len()) > MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
    }
    let mut next = picked.scan.highest.max(above);
    let mut shapes = Vec::with_capacity(planned.len());
    let mut produced = Vec::with_capacity(planned.len());
    for Planned { piece, from_work, look_of } in planned {
        let held = looks.get(&look_of).ok_or_else(|| Problem::failed("a look went missing"))?;
        next = next.checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
        produced.push(next);
        shapes.push(shape_of(next, piece, &held.look, &held.shared, from_work));
    }
    // The originals go first, by the engine's delete: a connector glued to one keeps its end where it is, unglued. Offset keeps them.
    if !gone.is_empty() {
        delete_originals(doc, page, gone.iter().copied())?;
    }
    let before = doc.clone();
    append(doc, page, &picked.parent, shapes)?;
    if let ([id], [first, ..], true) = (produced.as_slice(), operands, one_shape) {
        super::ties::retarget(doc, page, first.id, *id)?;
    }
    settle(doc, &before, page, &produced, &[])?;
    Ok(Made { produced, deleted, kept: None, changed: Vec::new(), not_fitted: Vec::new() })
}

/// Deletes the shapes `found` by the engine's delete, at the places the one walk of the page found them: a search of the page for each of up to
/// 4,096 shapes would be the product of the shapes and the page. They are not groups, so each takes nothing with it.
fn delete_originals<'a>(doc: &mut Document, page: u32, found: impl Iterator<Item = &'a (u32, ShapeRef)>) -> Result<(), Problem> {
    let (paths, gone): (Vec<Vec<u32>>, BTreeSet<u32>) = found.map(|(id, at)| (at.path.clone(), *id)).unzip();
    delete_paths(doc, page, paths, &gone)
}

/// Puts shapes at the front of their container: at the end of the page's list, or of a group's members.
fn append(doc: &mut Document, page: u32, parent: &[u32], shapes: Vec<Shape>) -> Result<(), Problem> {
    if parent.is_empty() {
        let list = &mut page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?.shapes;
        shapes.into_iter().for_each(|shape| list.push(Arc::new(shape)));
    } else {
        let list = &mut shape_mut(doc, page, parent).ok_or_else(|| Problem::failed("the group went away"))?.children;
        shapes.into_iter().for_each(|shape| list.push(Arc::new(shape)));
    }
    Ok(())
}

// ---- Subtract -------------------------------------------------------------------------------------------------------------------

/// The `IX` of the `Geometry` sections of a sheet that are not deleted.
fn geometry_ixs(sheet: &Sheet, with_deleted: bool) -> BTreeSet<u32> {
    sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry" && (with_deleted || !s.del)).filter_map(|s| s.ix).collect()
}

/// The `IX` of the sections of the shape's master (or of the master shape it stands for): an instance inherits them, and a section of the same `IX` that it
/// deletes or replaces is a local one that says so.
fn inherited_ixs(reader: &Reader<'_>, at: &ShapeRef) -> BTreeSet<u32> {
    let res = reader.resolver();
    match res.master_step(at) {
        Ok(Some(step)) => res.shape(&step.shape).map(|s| geometry_ixs(&s.sheet, false)).unwrap_or_default(),
        _ => BTreeSet::new(),
    }
}

/// The lowest `IX` from 0 that `used` lacks, taken.
fn free_ix(used: &mut BTreeSet<u32>) -> u32 {
    let ix = (0..=u32::MAX).find(|n| !used.contains(n)).unwrap_or(u32::MAX);
    used.insert(ix);
    ix
}

/// Takes the sections `replaced` out of the shape at `at` and puts `count` new ones in their place, as sections local to the shape: a section it inherits from a
/// master (`inherited`, the `IX` of the master's) is marked deleted on the shape (`Del`), and a section of its own goes. Each new section is made by `make` (its
/// place among the new ones and its `IX`), and takes the lowest `IX` not held by the shape or its master.
fn replace_geometry(
    doc: &mut Document,
    page: u32,
    at: &ShapeRef,
    replaced: &BTreeSet<u32>,
    inherited: &BTreeSet<u32>,
    count: usize,
    make: &dyn Fn(usize, u32) -> Section,
) -> Result<(), Problem> {
    let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
    let mut counted = shape.counted_sheet();
    counted.remove_sections(|s| s.name.as_str() == "Geometry" && s.ix.is_some_and(|ix| replaced.contains(&ix)));
    for ix in replaced.iter().filter(|ix| inherited.contains(ix)) {
        counted.push_section(Arc::new(Section { del: true, ..Section::new("Geometry", Some(*ix)) }));
    }
    let mut used: BTreeSet<u32> = geometry_ixs(counted.sheet, true).union(inherited).copied().collect();
    if counted.sheet.sections.len().saturating_add(count) > MAX_SECTIONS_PER_SHEET {
        return Err(Problem::failed(format!("the result has too many boundaries for a shape's {MAX_SECTIONS_PER_SHEET} sections")));
    }
    for k in 0..count {
        let ix = free_ix(&mut used);
        counted.push_section(Arc::new(make(k, ix)));
    }
    Ok(())
}

/// What a shape changed in place needs to know before its geometry is replaced: the frame the new geometry is written in, whether that is the shape's own, and the
/// shape's pose as it was.
struct Prior {
    keep_frame: bool,
    frame: Frame,
    pose: Pose,
}

/// The frame for the geometry of the shape `shape` that replaces its sections `replaced`, and its pose: a shape that keeps sections of its own,
/// or that holds its size (`LockCalcWH`), keeps its frame, so that what is kept stays where it is; otherwise the frame is the box `rect` of the new geometry.
fn prior(reader: &Reader<'_>, shape: &Operand, replaced: &BTreeSet<u32>, rect: Rect) -> Result<Prior, Problem> {
    let sheet = SheetRef::Shape(shape.at.clone());
    let effective = reader.resolver().effective_sections(&sheet, "Geometry").map_or(0, |keys| keys.len());
    let keep_frame = effective > replaced.len() || reader.number(&sheet, "LockCalcWH") != 0.0;
    let frame = if keep_frame {
        if shape.size.0 <= 0.0 || shape.size.1 <= 0.0 {
            return Err(Problem::failed(format!("shape {} has no width or no height to write the geometry in", shape.id)));
        }
        Frame { min: Point::ORIGIN, width: shape.size.0, height: shape.size.1 }
    } else {
        Frame::of(rect)
    };
    Ok(Prior { keep_frame, frame, pose: reader.pose(&sheet) })
}

/// What makes a new section of a shape changed in place: its place among the new ones, its `IX` and the frame it is written in.
type Make<'a> = Box<dyn Fn(usize, u32, &Frame) -> Section + 'a>;

/// One shape whose geometry is replaced in place: where it is now, the sections replaced and the `count` that take their place, made by `make` (its place among the new
/// ones, its `IX` and the frame they are written in), and what [`prior`] found.
struct Change<'a> {
    id: u32,
    at: ShapeRef,
    replaced: BTreeSet<u32>,
    prior: Prior,
    count: usize,
    make: Make<'a>,
}

/// Replaces the sections of each shape of `changes` by those it makes in the frame of its `prior`, and when that frame is the geometry's own (the box refitted) writes the
/// new size and moves the pin so that nothing moves on the page.
///
/// **All the shapes together, in the steps of one:** every shape's rows and size are written, then the page is recalculated once; then every pin is written, and it is
/// recalculated once more. The shapes are where the caller found them (`at`), so there is no search for one. A recalculation and a walk of the page for each of up to
/// 4,096 shapes would be the product of the shapes and the page.
fn rewrite_in_place(doc: &mut Document, page: u32, changes: &[Change<'_>]) -> Result<(), Problem> {
    if changes.is_empty() {
        return Ok(());
    }
    let before = doc.clone();
    let inherited: Vec<BTreeSet<u32>> = {
        let reader = Reader::new(doc);
        changes.iter().map(|change| inherited_ixs(&reader, &change.at)).collect()
    };
    for (change, inherited) in changes.iter().zip(&inherited) {
        replace_geometry(doc, page, &change.at, &change.replaced, inherited, change.count, &|k, ix| (change.make)(k, ix, &change.prior.frame))?;
        if !change.prior.keep_frame {
            let sheet = SheetRef::Shape(change.at.clone());
            write_length(doc, &sheet, "Width", change.prior.frame.width)?;
            write_length(doc, &sheet, "Height", change.prior.frame.height)?;
        }
    }
    let ids: Vec<u32> = changes.iter().map(|change| change.id).collect();
    settle(doc, &before, page, &ids, &[])?;
    let refitted: Vec<&Change<'_>> = changes.iter().filter(|change| !change.prior.keep_frame).collect();
    if refitted.is_empty() {
        return Ok(());
    }
    // The local pin follows the new size by its formula; the pin moves by what the box's corner and the local pin moved, so nothing moves on the page.
    let loc_after: Vec<(f64, f64)> = {
        let reader = Reader::new(doc);
        refitted
            .iter()
            .map(|change| {
                let sheet = SheetRef::Shape(change.at.clone());
                (reader.number(&sheet, "LocPinX"), reader.number(&sheet, "LocPinY"))
            })
            .collect()
    };
    let before_pin = doc.clone();
    for (change, loc_after) in refitted.iter().zip(loc_after) {
        let sheet = SheetRef::Shape(change.at.clone());
        let pose = &change.prior.pose;
        let [a, b, c, d, _, _] = pose_map(pose).as_coeffs();
        let linear = Affine::new([a, b, c, d, 0.0, 0.0]);
        let by = linear * Point::new(change.prior.frame.min.x + loc_after.0 - pose.loc.0, change.prior.frame.min.y + loc_after.1 - pose.loc.1);
        write_length(doc, &sheet, "PinX", pose.pin.0 + by.x)?;
        write_length(doc, &sheet, "PinY", pose.pin.1 + by.y)?;
    }
    let moved: Vec<u32> = refitted.iter().map(|change| change.id).collect();
    settle(doc, &before_pin, page, &moved, &[])
}

fn subtract(doc: &mut Document, page: u32, picked: &Picked, operands: &[Operand]) -> Result<Made, Problem> {
    let (first, others) = operands.split_first().ok_or_else(|| Problem::failed("there are no shapes"))?;
    // The working frame is the first shape's own: its geometry is then what it was, and the others are carried to it.
    let to_work = first.map.inverse();
    let first_fill = moved(first.fill(), to_work);
    let others_fill: Vec<BezPath> = others.iter().map(|o| moved(o.fill(), to_work)).collect();
    let loops = rows::clean(&ops::subtract(&first_fill, &others_fill)?);
    let Some(rect) = rows::bounds(&loops) else {
        return Err(Problem::failed(format!("nothing of shape {} is left once the others are taken away", first.id)));
    };
    // The sections replaced: the shown ones that have a fill area. The others (open ones along a line with `NoFill` 0, `NoFill` 1, hidden) are kept as they are.
    let replaced: BTreeSet<u32> = first.sections.iter().filter(|s| s.takes_fill()).filter_map(|s| s.ix).collect();
    let prior = prior(&Reader::new(doc), first, &replaced, rect)?;
    let others_ids: Vec<u32> = picked.shapes.iter().skip(1).map(|(id, _)| *id).collect();
    delete_originals(doc, page, picked.shapes.iter().skip(1))?;
    // The others are gone, and the places of the shapes after them in a list have moved up: the first shape is found again.
    let at = doc.find_shape(Owner::Page(page), first.id).ok_or_else(|| Problem::failed("the shape went away"))?;
    let change = Change {
        id: first.id,
        at,
        replaced,
        prior,
        count: loops.len(),
        make: Box::new(|k, ix, frame| match loops.get(k) {
            Some(ring) => rows::section_of(ring, frame, ix),
            None => Section::new("Geometry", Some(ix)),
        }),
    };
    rewrite_in_place(doc, page, &[change])?;
    Ok(Made { produced: Vec::new(), deleted: others_ids, kept: Some(first.id), changed: Vec::new(), not_fitted: Vec::new() })
}

// ---- Fit Curve ------------------------------------------------------------------------------------------------------------------

/// The flags a section's rows are written with: the section's own, as it was read.
fn flags_of(section: &operand::Sect) -> Flags {
    Flags { no_fill: section.flags.no_fill, no_line: section.flags.no_line, no_snap: section.no_snap, no_quick_drag: section.no_quick_drag }
}

/// Where the runs of one section are among the runs of the command.
struct Candidate {
    /// The operand and the place of the section among its sections.
    operand: usize,
    section: usize,
    ix: u32,
    /// The runs of the section: from `first`, `count` of them.
    first: usize,
    count: usize,
}

/// Fit Curve: each shape stays the same shape, with every cell, and the sections whose paths the fit makes smaller get new rows in their place, with the
/// flags they had. A shape whose paths the fit does not make smaller is not touched, and when none is the command changes nothing and makes no undo step.
///
/// **Smaller is rows**: a section is replaced only when the rows of the fit are fewer than the rows it holds, so that one `Ellipse` row is not made into a
/// `MoveTo` and four `NURBSTo`, nor an `ArcTo` into the cubics that draw it. Every run of every shape is fitted by **one call** ([`fit::fit`]), so that the library's work for
/// the command is bounded once; the shapes are changed together, in the steps of one ([`rewrite_in_place`]).
fn fit_curve(doc: &mut Document, page: u32, operands: &[Operand], args: &Args) -> Result<Made, Problem> {
    let options = fit::Options { tolerance: args.tolerance, flags: args.flags };
    // The sections are read in the shape's own frame, which is where the rows are written (`operand::read`).
    let mut runs: Vec<Run> = Vec::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    for (o, operand) in operands.iter().enumerate() {
        for (k, section) in operand.sections.iter().enumerate() {
            let Some(ix) = section.ix else { continue };
            let found = runs_of(&section.stroke);
            candidates.push(Candidate { operand: o, section: k, ix, first: runs.len(), count: found.len() });
            runs.extend(found);
        }
    }
    let mut steps = fit::STEPS;
    let fitted = fit::fit(&runs, &options, &mut steps)?;
    let reader = Reader::new(doc);
    let mut changes: Vec<Change<'_>> = Vec::new();
    for (o, operand) in operands.iter().enumerate() {
        let mut replaced: BTreeSet<u32> = BTreeSet::new();
        let mut made: Vec<(Vec<Run>, Flags)> = Vec::new();
        for candidate in candidates.iter().filter(|c| c.operand == o) {
            let range = candidate.first..candidate.first.saturating_add(candidate.count);
            let (Some(old), Some(new)) = (runs.get(range.clone()), fitted.get(range)) else { continue };
            if new.iter().all(Option::is_none) {
                continue;
            }
            let all: Vec<Run> = old.iter().zip(new).map(|(old, new)| new.clone().unwrap_or_else(|| old.clone())).collect();
            let Some(section) = operand.sections.get(candidate.section) else { continue };
            let rows = all.iter().fold(0_usize, |sum, run| sum.saturating_add(run.segs.len()).saturating_add(1));
            if rows >= section.row_count {
                continue;
            }
            replaced.insert(candidate.ix);
            made.push((all, flags_of(section)));
        }
        let Some(rect) = made.iter().flat_map(|(runs, _)| runs.iter().map(Run::bounds)).reduce(|a, b| a.union(b)) else { continue };
        if !is_box(&rect) {
            return Err(Problem::failed(format!("the fit of shape {} has a number that is not finite", operand.id)));
        }
        let prior = prior(&reader, operand, &replaced, rect)?;
        changes.push(Change {
            id: operand.id,
            at: operand.at.clone(),
            replaced,
            prior,
            count: made.len(),
            make: Box::new(move |k, ix, frame| match made.get(k) {
                Some((runs, flags)) => rows::section_of_runs(runs, frame, ix, *flags),
                None => Section::new("Geometry", Some(ix)),
            }),
        });
    }
    let changed: Vec<u32> = changes.iter().map(|change| change.id).collect();
    // A shape the fit left as it was and that holds something to fit is said so: with a tolerance of nothing that is the design, and with any other the tolerance is too tight
    // for the fit (or its work ran out), which a success with nothing changed would not say.
    let not_fitted: Vec<u32> = if args.tolerance > 0.0 {
        operands.iter().filter(|o| !changed.contains(&o.id) && o.sections.iter().any(|s| s.row_count > 2)).map(|o| o.id).collect()
    } else {
        Vec::new()
    };
    drop(reader);
    rewrite_in_place(doc, page, &changes)?;
    Ok(Made { produced: Vec::new(), deleted: Vec::new(), kept: None, changed, not_fitted })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::draw::shapeops::runs::Seg;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn a_piece_of_runs_is_the_box_that_holds_them_and_a_point_a_sliver_and_a_number_that_is_not_one_make_none() {
        let line = |a: (f64, f64), b: (f64, f64)| Run { start: p(a.0, a.1), segs: vec![Seg::Line(p(b.0, b.1))] };
        let piece = piece_of_runs(&[line((1.0, 1.0), (4.0, 3.0))]).unwrap().unwrap();
        assert_eq!((piece.frame.min, piece.frame.width, piece.frame.height), (p(1.0, 1.0), 3.0, 2.0));
        assert_eq!(piece.sections.len(), 1);
        // Two runs in one box, a section each.
        let two = piece_of_runs(&[line((0.0, 0.0), (1.0, 0.0)), line((5.0, 5.0), (6.0, 7.0))]).unwrap().unwrap();
        assert_eq!((two.frame.width, two.frame.height, two.sections.len()), (6.0, 7.0, 2));
        // Nothing: no run, a run of no segment, a run shorter than 1E-5, and a number that is not a number.
        assert!(piece_of_runs(&[]).unwrap().is_none());
        assert!(piece_of_runs(&[Run { start: p(1.0, 1.0), segs: vec![] }]).unwrap().is_none());
        assert!(piece_of_runs(&[line((1.0, 1.0), (1.000_001, 1.0))]).unwrap().is_none(), "shorter than a hundred-thousandth");
        assert!(piece_of_runs(&[line((1.0, 1.0), (1.000_02, 1.0))]).unwrap().is_some(), "longer than it");
        for bad in [
            line((1.0, 1.0), (f64::NAN, 1.0)),
            line((1.0, 1.0), (f64::INFINITY, 1.0)),
            line((f64::NAN, 1.0), (4.0, 3.0)),
            line((-1.0e308, 0.0), (1.0e308, 1.0)),
        ] {
            let Err(error) = piece_of_runs(&[bad]) else { panic!("refused") };
            assert!(!error.bad_params && (error.message.contains("not finite") || error.message.contains("too large")), "{error:?}");
        }
    }

    #[test]
    fn a_closed_run_takes_the_fill_and_an_open_one_does_not() {
        let open = Run { start: p(0.0, 0.0), segs: vec![Seg::Line(p(2.0, 0.0)), Seg::Line(p(2.0, 1.0))] };
        let closed = Run { start: p(0.0, 0.0), segs: vec![Seg::Line(p(2.0, 0.0)), Seg::Line(p(2.0, 1.0)), Seg::Line(p(0.0, 0.0))] };
        let piece = piece_of_runs(&[open, closed]).unwrap().unwrap();
        let no_fill =
            |k: usize| piece.sections.get(k).and_then(|s| s.cells.get("NoFill")).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned());
        assert_eq!((no_fill(0).as_deref(), no_fill(1).as_deref()), (Some("1"), Some("0")), "R11");
    }

    #[test]
    fn a_box_is_a_corner_and_two_sides_that_are_numbers() {
        assert!(is_box(&Rect::new(0.0, 0.0, 3.0, 2.0)));
        assert!(is_box(&Rect::new(-1.0e300, -1.0e300, -1.0e300, 0.0)), "a flat box is a box");
        assert!(!is_box(&Rect::new(f64::NAN, 0.0, 3.0, 2.0)));
        assert!(!is_box(&Rect::new(0.0, f64::NAN, 3.0, 2.0)));
        assert!(!is_box(&Rect::new(0.0, 0.0, f64::INFINITY, 2.0)));
        assert!(!is_box(&Rect::new(0.0, 0.0, 3.0, f64::NEG_INFINITY)));
        assert!(!is_box(&Rect::new(-1.0e308, 0.0, 1.0e308, 2.0)), "a side that overflows");
        assert!(!is_box(&Rect::new(0.0, -1.0e308, 3.0, 1.0e308)));
    }

    #[test]
    fn a_piece_of_more_runs_than_a_shape_holds_sections_is_refused_in_words() {
        let runs: Vec<Run> = (0..=MAX_SECTIONS_PER_SHEET).map(|k| Run { start: p(k as f64, 0.0), segs: vec![Seg::Line(p(k as f64, 1.0))] }).collect();
        let Err(error) = piece_of_runs(&runs) else { panic!("refused") };
        assert!(!error.bad_params && error.message.contains("a shape holds at most"), "{error:?}");
        assert!(piece_of_runs(&runs[..MAX_SECTIONS_PER_SHEET]).unwrap().is_some(), "the most a shape holds is made");
    }

    /// A circle of radius 1 as 36 lines, at the page's point `at`, as a shape of constants.
    fn circle_shape(id: u32, at: (f64, f64)) -> Shape {
        use diacraft_doc::{Links, Row, Value};
        let constant = |n: f64| Cell::constant(Value::from_text(&format!("{n}")));
        let mut sheet = Sheet { links: Links { line: Some(0), fill: Some(0), text: Some(0) }, ..Sheet::default() };
        for (name, v) in [
            ("PinX", at.0),
            ("PinY", at.1),
            ("Width", 2.0),
            ("Height", 2.0),
            ("LocPinX", 1.0),
            ("LocPinY", 1.0),
            ("Angle", 0.0),
            ("FlipX", 0.0),
            ("FlipY", 0.0),
        ] {
            sheet.cells.insert(CellName::new(name), constant(v));
        }
        let mut section = Section::new("Geometry", Some(0));
        for flag in ["NoFill", "NoLine", "NoShow", "NoSnap", "NoQuickDrag"] {
            section.cells.insert(CellName::new(flag), constant(0.0));
        }
        for i in 0..=36_u32 {
            let a = f64::from(i % 36) * 10.0_f64.to_radians();
            let mut row = Row { ix: Some(i + 1), row_type: Some(Arc::from(if i == 0 { "MoveTo" } else { "LineTo" })), ..Row::default() };
            row.cells.insert(CellName::new("X"), constant(1.0 + a.cos()));
            row.cells.insert(CellName::new("Y"), constant(1.0 + a.sin()));
            section.rows.push(Arc::new(row));
        }
        sheet.sections.push(Arc::new(section));
        let mut shape = Shape::new(id);
        shape.kind = ShapeType::Shape;
        shape.sheet = Arc::new(sheet);
        shape
    }

    fn page_of_circles(n: u32) -> Document {
        let mut doc = Document::empty();
        let mut page = diacraft_doc::Page::new(1, "P");
        let shapes = (1..=n).map(|id| Arc::new(circle_shape(id, (f64::from(id % 20) * 3.0, f64::from(id / 20) * 3.0)))).collect();
        page.shapes = diacraft_doc::ShapeList::from_vec(shapes);
        doc.pages.push(Arc::new(page));
        doc
    }

    #[test]
    fn fit_curve_of_every_shape_of_a_page_walks_the_page_a_few_times_and_recalculates_twice_whatever_the_number_of_shapes() {
        // Counted, not timed: the shapes are found by the walks of the page (a visit to each shape of it, a few times over) and the page is recalculated twice (the rows and
        // the sizes, then the pins), not a walk and two recalculations for each shape, which was the product of the shapes and the page (87 s for 4,096 circles).
        for n in [10_u32, 40, 160] {
            let mut doc = page_of_circles(n);
            let ids: Vec<u32> = (1..=n).collect();
            crate::draw::NODES_WALKED.with(|c| c.set(0));
            crate::draw::SETTLES.with(|c| c.set(0));
            let args = Args { tolerance: 0.02, flags: Args::DO_CIRCLES, ..Args::default() };
            let made = run_with(&mut doc, 1, Op::FitCurve, &ids, 10_000, &args).unwrap();
            assert_eq!(made.changed.len(), n as usize, "every circle is fitted");
            let walked = crate::draw::NODES_WALKED.with(std::cell::Cell::get);
            let settles = crate::draw::SETTLES.with(std::cell::Cell::get);
            assert!(walked <= 4 * n as usize, "{walked} visits for {n} shapes on a page of {n}");
            assert_eq!(settles, 2, "the rows and sizes, then the pins");
        }
    }

    #[test]
    fn a_check_of_offset_and_fit_curve_reads_the_first_rows_of_each_shape_and_no_more() {
        // Counted, not timed: each shape here is a `MoveTo` and 36 `LineTo`s, and the scan for a shown path stops at the second row, so the rows looked at are two for each
        // shape and not thirty-seven, however many shapes there are, and the shapes are not built (the reading that the check was before looked at, and built from, every row).
        for n in [10_u32, 40, 160] {
            let doc = page_of_circles(n);
            let ids: Vec<u32> = (1..=n).collect();
            for op in [Op::Offset, Op::FitCurve] {
                super::operand::ROWS_SCANNED.with(|c| c.set(0));
                assert_eq!(check(&doc, 1, op, &ids), Ok(()), "{}", op.id());
                assert_eq!(super::operand::ROWS_SCANNED.with(std::cell::Cell::get), 2 * n as usize, "{} on {n} shapes", op.id());
            }
        }
    }
}
