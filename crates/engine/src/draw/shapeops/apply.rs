//! A command's work on the document: the originals go by the engine's own delete, the produced shapes are put at the front of the
//! container, what reads them is calculated, and for Subtract the first shape's geometry and frame are changed in place.
//!
//! Nothing here keeps its own copy of the rules for glue, guides or jumps: [`delete_originals`] (the engine's `delete_paths`) settles them (a connector glued to a shape that goes
//! keeps that end where it is, unglued, and its `Connect` row goes), which is R14. The caller (`Session::edit`) works on a copy of the document, so a refusal at
//! any step leaves it as it was, and a success is one undo step.

use std::collections::BTreeSet;
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellName, Document, MAX_SECTIONS_PER_SHEET, MAX_SHAPES_PER_PAGE, Owner, Section, Shape, ShapeRef, ShapeType, Sheet, SheetRef,
};
use kurbo::{Affine, BezPath, Point, Vec2};

use super::operand::{self, Operand, Picked, pose_map};
use super::rows::{self, Frame, Loop};
use super::{Budget, MAX_LOOK_UNITS, MAX_PRODUCED, Op, carry, look, ops};
use crate::draw::shapes::delete_paths;
use crate::draw::{Problem, Reader, page_mut, require_page, settle, shape_mut, write_length};
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
}

/// Whether the command may run on these shapes, and if not why not in words: everything `run` checks before it builds anything, and the reading of
/// each shape's geometry that says whether it has a path and a fill area. The document is not changed. It costs what reading the shapes costs, so a window asks
/// it for a selection and not for each frame.
pub fn check(doc: &Document, page: u32, op: Op, ids: &[u32]) -> Result<(), String> {
    require_page(doc, page).map_err(|p| p.message)?;
    let picked = operand::validate(doc, page, op, ids).map_err(|p| p.message)?;
    operand::read(doc, op, &picked, &mut Budget::new()).map(|_| ()).map_err(|p| p.message)
}

/// Runs the operation on the shapes `ids` of a page, the first named being the first shape. The new shapes' `ID`s are above `above` and above every `ID` on
/// the page. A refusal is a [`Problem`] and the document may then be half changed: the caller works on a copy.
pub fn run(doc: &mut Document, page: u32, op: Op, ids: &[u32], above: u32) -> Result<Made, Problem> {
    require_page(doc, page)?;
    let picked = operand::validate(doc, page, op, ids)?;
    let operands = operand::read(doc, op, &picked, &mut Budget::new())?;
    match op {
        Op::Subtract => subtract(doc, page, &picked, &operands),
        Op::Union | Op::Combine | Op::Fragment | Op::Intersect => build(doc, page, op, &picked, &operands, above),
    }
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
        Op::Subtract => Err(Problem::failed("Subtract changes the first shape and makes none")),
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

fn build(doc: &mut Document, page: u32, op: Op, picked: &Picked, operands: &[Operand], above: u32) -> Result<Made, Problem> {
    // The working frame: the container's coordinates, moved so that the middle of everything worked on is at the origin. A small shape far out is then as easy to
    // sweep as one near the origin, and the library's tolerance does not grow with the distance.
    let all = operands.iter().flat_map(|o| o.sections.iter().flat_map(|s| [&s.stroke, &s.fill]));
    let rect = ops::bounds(all).ok_or_else(|| Problem::failed("the shapes have no geometry"))?;
    let to_work = Affine::translate(Vec2::new(-rect.center().x, -rect.center().y));
    let from_work = to_work.inverse();
    let made = pieces(op, operands, to_work)?;
    let first = operands.first().ok_or_else(|| Problem::failed("there are no shapes"))?;
    let with_text = op != Op::Fragment;
    let mut look = look::capture(&Reader::new(doc), &first.at, with_text)?;
    let frame = frame_cells(Point::ORIGIN, 0.0, 0.0);
    let have = names_of(&frame, &look);
    look::freeze_unportable(&mut look.cells, &mut look.sections, &have);
    // A command that makes one shape puts it where the first shape was in containers and lists (SHAPEOPS-CONTAINERS); one that makes many puts none there. The
    // ties name other shapes by `Sheet.<id>!`, which is what they are for, so they are added after the formulas that would be frozen for it.
    if made.len() == 1
        && let Some(ties) = look.ties.take()
    {
        look.cells.push((CellName::new("Relationships"), ties));
    }
    // What each shape made holds of the first shape's formatting and of its frame is charged before any is built: the cells it is given (the first shape's own and the
    // frame's) and the rows and cells of the look's sections, which every shape shares in memory and a file written from the drawing writes for each. What it comes to
    // times the shapes, past `MAX_LOOK_UNITS`, is refused in words.
    let shared: Vec<Arc<Section>> = std::mem::take(&mut look.sections).into_iter().map(Arc::new).collect();
    let in_sections = shared.iter().fold(0_usize, |sum, section| sum.saturating_add(look::section_units(section)));
    let per_shape = in_sections.saturating_add(look.cells.len()).saturating_add(frame.len());
    if per_shape.saturating_mul(made.len()) > MAX_LOOK_UNITS {
        return Err(Problem::failed(format!(
            "the first shape's formatting holds {per_shape} cells and rows with the frame each shape is given, and {} shapes would each hold them: a command writes at most {MAX_LOOK_UNITS} in all",
            made.len()
        )));
    }
    let deleted: Vec<u32> = picked.shapes.iter().map(|(id, _)| *id).collect();
    if picked.scan.count.saturating_sub(deleted.len()).saturating_add(made.len()) > MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
    }
    let mut next = picked.scan.highest.max(above);
    let mut shapes = Vec::with_capacity(made.len());
    let mut produced = Vec::with_capacity(made.len());
    for piece in made {
        next = next.checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
        produced.push(next);
        shapes.push(shape_of(next, piece, &look, &shared, from_work));
    }
    // The originals go first, by the engine's delete: a connector glued to one keeps its end where it is, unglued.
    delete_originals(doc, page, picked.shapes.iter())?;
    let before = doc.clone();
    append(doc, page, &picked.parent, shapes)?;
    if let ([id], [first, ..]) = (produced.as_slice(), operands) {
        super::ties::retarget(doc, page, first.id, *id)?;
    }
    settle(doc, &before, page, &produced, &[])?;
    Ok(Made { produced, deleted, kept: None })
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
fn inherited_ixs(doc: &Document, at: &ShapeRef) -> BTreeSet<u32> {
    let reader = Reader::new(doc);
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

/// Takes the sections `replaced` out of the shape at `at` and puts `loops` in their place, as sections local to the shape: a section it inherits from a
/// master is marked deleted on the shape (`Del`), and a section of its own goes. The new sections take the lowest `IX` not held by the shape or its
/// master.
fn replace_geometry(doc: &mut Document, page: u32, at: &ShapeRef, replaced: &BTreeSet<u32>, loops: &[Loop], frame: &Frame) -> Result<(), Problem> {
    let inherited = inherited_ixs(doc, at);
    let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
    let sheet = Arc::make_mut(&mut shape.sheet);
    sheet.sections.retain(|s| !(s.name.as_str() == "Geometry" && s.ix.is_some_and(|ix| replaced.contains(&ix))));
    for ix in replaced.iter().filter(|ix| inherited.contains(ix)) {
        sheet.sections.push(Arc::new(Section { del: true, ..Section::new("Geometry", Some(*ix)) }));
    }
    let mut used: BTreeSet<u32> = geometry_ixs(sheet, true).union(&inherited).copied().collect();
    if sheet.sections.len().saturating_add(loops.len()) > MAX_SECTIONS_PER_SHEET {
        return Err(Problem::failed(format!("the result has too many boundaries for a shape's {MAX_SECTIONS_PER_SHEET} sections")));
    }
    for ring in loops {
        let ix = free_ix(&mut used);
        sheet.sections.push(Arc::new(rows::section_of(ring, frame, ix)));
    }
    Ok(())
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
    let (kept_any, lock_size, pose_before) = {
        let reader = Reader::new(doc);
        let sheet = SheetRef::Shape(first.at.clone());
        let effective = reader.resolver().effective_sections(&sheet, "Geometry").map_or(0, |keys| keys.len());
        (effective > replaced.len(), reader.number(&sheet, "LockCalcWH") != 0.0, reader.pose(&sheet))
    };
    // A shape that keeps sections of its own, or that holds its size (`LockCalcWH`), keeps its frame: the new geometry is written in it, so that what is kept
    // stays where it is. Otherwise the frame is refitted to the new geometry, and the pin moves so that nothing moves on the page.
    let keep_frame = kept_any || lock_size;
    let frame = if keep_frame {
        if first.size.0 <= 0.0 || first.size.1 <= 0.0 {
            return Err(Problem::failed(format!("shape {} has no width or no height to write the geometry in", first.id)));
        }
        Frame { min: Point::ORIGIN, width: first.size.0, height: first.size.1 }
    } else {
        Frame::of(rect)
    };
    let others_ids: Vec<u32> = picked.shapes.iter().skip(1).map(|(id, _)| *id).collect();
    delete_originals(doc, page, picked.shapes.iter().skip(1))?;
    let at = doc.find_shape(Owner::Page(page), first.id).ok_or_else(|| Problem::failed("the first shape went away"))?;
    let before = doc.clone();
    replace_geometry(doc, page, &at, &replaced, &loops, &frame)?;
    let sheet = SheetRef::Shape(at.clone());
    if !keep_frame {
        write_length(doc, &sheet, "Width", frame.width)?;
        write_length(doc, &sheet, "Height", frame.height)?;
    }
    settle(doc, &before, page, &[first.id], &[])?;
    if !keep_frame {
        // The local pin follows the new size by its formula; the pin moves by what the box's corner and the local pin moved, so nothing moves on the page.
        let loc_after = {
            let reader = Reader::new(doc);
            (reader.number(&sheet, "LocPinX"), reader.number(&sheet, "LocPinY"))
        };
        let [a, b, c, d, _, _] = pose_map(&pose_before).as_coeffs();
        let linear = Affine::new([a, b, c, d, 0.0, 0.0]);
        let by = linear * Point::new(frame.min.x + loc_after.0 - pose_before.loc.0, frame.min.y + loc_after.1 - pose_before.loc.1);
        let before_pin = doc.clone();
        write_length(doc, &sheet, "PinX", pose_before.pin.0 + by.x)?;
        write_length(doc, &sheet, "PinY", pose_before.pin.1 + by.y)?;
        settle(doc, &before_pin, page, &[first.id], &[])?;
    }
    Ok(Made { produced: Vec::new(), deleted: others_ids, kept: Some(first.id) })
}
