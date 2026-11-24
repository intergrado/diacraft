//! Glue to guides: a shape pulled to a guide is held to it by its own cells, and follows the guide when the guide moves or turns.
//!
//! **What is written (a project rule of the spec's).** When a placing (a
//! move, a resize, a drop, a place) is pulled to a guide on an axis and the Glue switch and glue to guides are on ([`super::snap`] gives the [`Hold`]s),
//! each shape that moves whose own edge or middle is on that guide is held by it:
//!
//! * the **glued cell** of the axis and the edge (`AlignLeft`, `AlignCenter`, `AlignRight` for an upright guide, `AlignBottom`, `AlignMiddle`, `AlignTop` for
//!   a level one) is `INTERSECTX` (an x cell) or `INTERSECTY` (a y cell) of the guide's `PinX`, `PinY` and `Angle` named with `Sheet.<id>!`, the shape's place as
//!   two lengths in drawing units (`DL`), and the guide's `Angle` plus 90 degrees; **the other axis's middle cell** (`AlignMiddle` for an upright guide,
//!   `AlignCenter` for a level one) is the other function of the same arguments, and holds the shape's own place (on 157 of the 164 shapes held to
//!   one guide). Held at a **crossing** of an upright and a level guide, the x cell and the y cell are each the function of both guides' three cells, the
//!   upright one first, in both (the spec asks for one order and names none: ours);
//! * the **four other cells** are `_MARKER(1)` with `V` 1 and no `U`, and every guide cell has a numeric `V` and no `U`
//! * **both pins follow the two cells**: `PinX` is `LocPinX+AlignLeft`, `LocPinX+AlignCenter-Width/2` or `LocPinX+AlignRight-Width`, and `PinY`
//!   `LocPinY+AlignBottom`, `LocPinY+AlignMiddle-Height/2` or `LocPinY+AlignTop-Height` (the spellings of and the cells' own vectors), each keeping
//!   the unit the pin had;
//! * **one `Connect` row for each glued cell**: `FromSheet` the shape, `FromCell` the cell, `FromPart` 1 to 6 for left, centre, right, bottom,
//!   middle and top (`VisFromParts`), `ToSheet` the guide, `ToCell` `Angle`, `ToPart` 4; the middle cell of a hold to one guide has none.
//!
//!
//! **The place** in the formulas is **the middle of the shape's box where it came to rest**, in its parent's frame (the page's) and drawing units: a reading
//! of ("the shape's own centre or middle"), ours. "the shape's `PinX` and `PinY`"; the two are one point
//! for a shape whose pin is in its middle, and for another the pin would move the shape by the middle cell's pin formula (`LocPinY+AlignMiddle-Height/2` keeps a
//! shape still only if the cell is its middle). For an upright or a level guide the place changes no value; it is where the shape goes when the guide turns
//! (the foot of the line square to the guide through it).
//!
//! **Which shapes are held.** A top-level 2-D shape that is not turned or flipped (the forms have neither), is not a guide, and whose pins and
//! alignment cells no `GUARD` protects; its own edge or middle must lie on the guide (within a millionth of an inch) after the placing, so of several shapes
//! moved together only those whose own edge is on the line are held; and the guide's line must pass through its pin. A member of a group, a
//! line or connector, and the share form (the third, for shapes inside another sheet's frame) are read and kept, and never written.
//!
//! **What breaks a hold.** "a write breaks the hold". A guide cell is an alignment cell whose own formula names a shape by
//! `Sheet.<id>!` ([`names_a_shape`]). When an edit writes a shape's pin, turn or flips ([`verdict`]): a shape now turned (by other than a half turn) or
//! flipped is released, and so is one turned a half turn with its pin off its box's middle; a pin that no longer reads a cell of
//! its axis that is a guide cell or that a row is from releases the shape **whole**, but for the free axis of a hold to one guide and a glued
//! axis whose edge is still on its guide's line (a nudge along the guide writes the pin back where it was): those are held again at the shape's
//! new place with their rows kept. Released, its rows from alignment cells go, and every unguarded guide cell of it is written as the value it holds, with no
//! formula (the spec does not say what an unglued shape holds: we keep the value); the marker cells stay, and so does a pin's formula that
//! still reads a cell, which now holds a value. Every shape that goes releases its holders, and so does a guide grouped: their alignment cells
//! naming it are made constants, `GUARD` and all (`remap::freeze_refs_of`), so no formula is left naming a shape that is gone or a pin in another frame; every
//! copy is released so; `guide.delete` refuses a guide that a row or an alignment cell of the page names. A placing with `snap` that pulls the
//! shape to its guide again renews the hold at the new place, and a resize with `snap` keeps a hold where the part of the box held is still on its guide (ours).
//!
//! **Following.** Nothing here moves a shape when its guide moves: the guide's `PinX`, `PinY` or `Angle` is written (`guide.move`, `shape.move`, `shape.rotate`),
//! the recalculation of the edit works out the cells that name it and the pins that read those (`draw::settle`), and the shape follows; the shapes
//! it carries count as changed by the edit ([`carried_by_guides`]), so their connectors are placed and routed again. The rows are not read for following. A
//! page that grows on the left or the bottom leaves the pins of held shapes to the guides it carries ([`shift_holds`]).
//!
//! **Cost.** A placing reads the page's top-level shapes once, writes eight cells for each shape held and passes once over the page's `Connect` rows; the
//! release reads the cells the edit wrote and passes over the rows once to find them and once to take them out; a guide's move finds the shapes it carries in
//! one walk of the page; `guide.delete`'s refusal, a deletion and a group that holds a guide walk the page once, whatever number of
//! shapes go. A guide's move is the recalculation of what names it: about 205 steps for each shape held (the unit tests hold these to counts).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellEdit, Connect, Connects, Document, FormulaSlot, MAX_SHAPE_DEPTH, Owner, Shape, ShapeList, ShapeRef, ShapeType, SheetRef,
    Value,
};
use diacraft_formula::{Expr, Qualifier};

use super::page::SnapPage;
use super::{Anchor, Axis};
use crate::draw::{Problem, Reader, freeze_refs_of, page_mut, settle, shape_mut, sheet_refs, units_per_inch, write_cells};
use crate::stencil::number_text;

// The units the loops below walk, for the tests that hold them to a count: rows of `Connect` read, cells written, shapes looked at.
#[cfg(test)]
thread_local! {
    pub(crate) static ROWS_READ: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static CELLS_WRITTEN: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static SHAPES_READ: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn tally(counter: &'static std::thread::LocalKey<std::cell::Cell<usize>>, n: usize) {
    counter.with(|c| c.set(c.get().saturating_add(n)));
}

/// The six cells of the `Alignment` section, x cells first.
pub const ALIGN_CELLS: [&str; 6] = ["AlignLeft", "AlignCenter", "AlignRight", "AlignBottom", "AlignMiddle", "AlignTop"];

/// `ToCell` and `ToPart` of every row from an alignment cell to a guide (the guide's `Angle`, `VisToParts` 4, the guide's intersection).
pub const TO_CELL: &str = "Angle";
pub const TO_PART: u32 = 4;

/// How near a shape's edge must be to a guide, in paper inches, to be held to it: the snap puts it there exactly, but for the arithmetic.
const ON_LINE: f64 = 1.0e-6;

/// The most shapes one placing holds (the commands send at most 512 with `snap`, 4,096 for `shape.place`).
pub const MAX_HELD: usize = 4_096;

/// A guide a placing was pulled to, and so is held to: on which axis, which part of the box it pulled (an edge or the middle), and where it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hold {
    /// The guide's `ID`.
    pub guide: u32,
    /// `X` for an upright guide, `Y` for a level one.
    pub axis: Axis,
    pub anchor: Anchor,
    /// Where the guide is along `axis`, in paper inches.
    pub line: f64,
}

impl Hold {
    /// The alignment cell it glues; `None` for a hold of a point, which no shape has.
    pub fn cell(&self) -> Option<&'static str> {
        cell_for(self.axis, self.anchor)
    }
}

fn cell_for(axis: Axis, anchor: Anchor) -> Option<&'static str> {
    match (axis, anchor) {
        (Axis::X, Anchor::Low) => Some("AlignLeft"),
        (Axis::X, Anchor::Middle) => Some("AlignCenter"),
        (Axis::X, Anchor::High) => Some("AlignRight"),
        (Axis::Y, Anchor::Low) => Some("AlignBottom"),
        (Axis::Y, Anchor::Middle) => Some("AlignMiddle"),
        (Axis::Y, Anchor::High) => Some("AlignTop"),
        (_, Anchor::Point) => None,
    }
}

/// The axis and the part of the box an alignment cell is of: the inverse of [`cell_for`].
fn place_of(cell: &str) -> Option<(Axis, Anchor)> {
    [Axis::X, Axis::Y]
        .into_iter()
        .flat_map(|axis| [Anchor::Low, Anchor::Middle, Anchor::High].into_iter().map(move |anchor| (axis, anchor)))
        .find(|(axis, anchor)| cell_for(*axis, *anchor).is_some_and(|name| name.eq_ignore_ascii_case(cell)))
}

fn axis_of(cell: &str) -> Option<Axis> {
    place_of(cell).map(|(axis, _)| axis)
}

fn is_align(cell: &str) -> bool {
    place_of(cell).is_some()
}

/// `FromPart` of a row from the cell (`VisFromParts`): the edges left, centre, right, bottom, middle, top are 1 to 6.
fn from_part(cell: &str) -> u32 {
    match cell {
        "AlignLeft" => 1,
        "AlignCenter" => 2,
        "AlignRight" => 3,
        "AlignBottom" => 4,
        "AlignMiddle" => 5,
        _ => 6,
    }
}

/// The formula of the pin that follows the cell.
fn pin_formula(cell: &str) -> &'static str {
    match cell {
        "AlignLeft" => "LocPinX+AlignLeft",
        "AlignCenter" => "LocPinX+AlignCenter-Width/2",
        "AlignRight" => "LocPinX+AlignRight-Width",
        "AlignBottom" => "LocPinY+AlignBottom",
        "AlignMiddle" => "LocPinY+AlignMiddle-Height/2",
        _ => "LocPinY+AlignTop-Height",
    }
}

fn guide_args(guide: u32) -> String {
    format!("Sheet.{guide}!PinX,Sheet.{guide}!PinY,Sheet.{guide}!Angle")
}

/// A cell held to one guide: the line square to it through the shape's place (the second form).
fn to_one(function: &str, guide: u32, place: (f64, f64)) -> String {
    format!("{function}({},{} DL,{} DL,Sheet.{guide}!Angle+90 deg)", guide_args(guide), number_text(place.0), number_text(place.1))
}

/// A cell held to where two guides cross (the first form).
fn to_crossing(function: &str, upright: u32, level: u32) -> String {
    format!("{function}({},{})", guide_args(upright), guide_args(level))
}

fn row(from: u32, cell: &'static str, guide: u32) -> Connect {
    Connect {
        from_sheet: from,
        from_cell: Some(Arc::from(cell)),
        from_part: Some(from_part(cell)),
        to_sheet: guide,
        to_cell: Some(Arc::from(TO_CELL)),
        to_part: Some(TO_PART),
        raw: diacraft_doc::RawAttrs::new(),
        ext: Default::default(),
    }
}

/// What holding one shape writes.
struct Plan {
    id: u32,
    sheet: SheetRef,
    /// The six alignment cells.
    cells: Vec<(&'static str, Cell)>,
    /// `PinX` and `PinY`: the formula and the value they hold now.
    pins: [(&'static str, &'static str, f64); 2],
    rows: Vec<Connect>,
}

/// Whether a `GUARD` protects a pin or an alignment cell of the shape (a hold would have to write it).
fn guarded(reader: &Reader<'_>, sheet: &SheetRef) -> bool {
    ["PinX", "PinY"]
        .iter()
        .chain(ALIGN_CELLS.iter())
        .any(|name| reader.resolver().resolve(sheet, &CellAddr::direct(name)).is_some_and(|found| found.formula.cell.is_some_and(Cell::is_guarded)))
}

/// The plan for holding the top-level shape `at` (`ID` `id`) to the guides `x` and `y`, on the axes where its own edge or middle is on the guide; `None`
/// when it is not held (a line, a turned or flipped shape, a guarded cell, an edge off the guide). `k` is the page's drawing units to a paper inch.
fn plan(reader: &Reader<'_>, at: &ShapeRef, id: u32, k: f64, x: Option<Hold>, y: Option<Hold>) -> Option<Plan> {
    let sheet = SheetRef::Shape(at.clone());
    if reader.is_one_d(&sheet) {
        return None;
    }
    let pose = reader.pose(&sheet);
    if pose.is_turned_or_flipped() || guarded(reader, &sheet) {
        return None;
    }
    let (w, h) = (reader.number(&sheet, "Width"), reader.number(&sheet, "Height"));
    // The box in the parent's frame, which is the page's: the pin less the local pin, unturned.
    let (x0, y0) = (pose.pin.0 - pose.loc.0, pose.pin.1 - pose.loc.1);
    if ![w, h, x0, y0, k].iter().all(|v| v.is_finite()) || k <= 0.0 {
        return None;
    }
    let on = |hold: Option<Hold>, low: f64, size: f64| {
        hold.filter(|hold| {
            let edge = match hold.anchor {
                Anchor::Low => low,
                Anchor::Middle => low + size / 2.0,
                Anchor::High => low + size,
                Anchor::Point => return false,
            };
            (edge / k - hold.line).abs() <= ON_LINE
        })
    };
    let (hx, hy) = (on(x, x0, w), on(y, y0, h));
    let place = (x0 + w / 2.0, y0 + h / 2.0);
    let form = match (hx, hy) {
        (Some(hx), Some(hy)) => Form::Crossing { x: (hx.cell()?, hx.guide, hx.line * k), y: (hy.cell()?, hy.guide, hy.line * k) },
        (Some(h), None) | (None, Some(h)) => Form::One { glued: h.cell()?, guide: h.guide, value: h.line * k },
        (None, None) => return None,
    };
    Some(compose(id, sheet, &form, place, pose.pin))
}

/// The form of a hold: to one guide by a glued cell (with the value the cell holds), or at the crossing of an upright and a level guide by a cell of
/// each axis (the first two forms).
enum Form {
    One { glued: &'static str, guide: u32, value: f64 },
    Crossing { x: (&'static str, u32, f64), y: (&'static str, u32, f64) },
}

/// What holding a shape by `form` writes: the six alignment cells, the pins (their formulas, with the values `pin` they hold) and the rows,
/// with `place` (drawing units, the box's middle) as the place of a hold to one guide.
fn compose(id: u32, sheet: SheetRef, form: &Form, place: (f64, f64), pin: (f64, f64)) -> Plan {
    let mut guide_cells: Vec<(&'static str, String, f64)> = Vec::with_capacity(2);
    let mut rows: Vec<Connect> = Vec::with_capacity(2);
    let (x_cell, y_cell) = match form {
        Form::Crossing { x: (xc, upright, xv), y: (yc, level, yv) } => {
            guide_cells.push((*xc, to_crossing("INTERSECTX", *upright, *level), *xv));
            guide_cells.push((*yc, to_crossing("INTERSECTY", *upright, *level), *yv));
            rows.push(row(id, xc, *upright));
            rows.push(row(id, yc, *level));
            (*xc, *yc)
        }
        Form::One { glued, guide, value } if axis_of(glued) == Some(Axis::Y) => {
            guide_cells.push((*glued, to_one("INTERSECTY", *guide, place), *value));
            guide_cells.push(("AlignCenter", to_one("INTERSECTX", *guide, place), place.0));
            rows.push(row(id, glued, *guide));
            ("AlignCenter", *glued)
        }
        Form::One { glued, guide, value } => {
            guide_cells.push((*glued, to_one("INTERSECTX", *guide, place), *value));
            guide_cells.push(("AlignMiddle", to_one("INTERSECTY", *guide, place), place.1));
            rows.push(row(id, glued, *guide));
            (*glued, "AlignMiddle")
        }
    };
    let mut cells: Vec<(&'static str, Cell)> = Vec::with_capacity(ALIGN_CELLS.len());
    for name in ALIGN_CELLS {
        let cell = match guide_cells.iter().find(|(n, _, _)| *n == name) {
            Some((_, formula, value)) => Cell::with_formula(formula, Some(Value::from_text(&number_text(*value)))),
            // Each cell that holds no guide is `_MARKER(1)` with `V` 1 and no `U`.
            None => Cell::with_formula("_MARKER(1)", Some(Value::from_text("1"))),
        };
        cells.push((name, cell));
    }
    let pins = [("PinX", pin_formula(x_cell), pin.0), ("PinY", pin_formula(y_cell), pin.1)];
    Plan { id, sheet, cells, pins, rows }
}

/// The writes of a plan's cells and pins (the pins keep their unit; their values are where they are, which the formulas give).
fn plan_writes(plan: Plan, writes: &mut Vec<(SheetRef, CellAddr, CellEdit)>) -> Vec<Connect> {
    for (name, cell) in plan.cells {
        writes.push((plan.sheet.clone(), CellAddr::direct(name), CellEdit::Cell(cell)));
    }
    for (name, formula, value) in plan.pins {
        let edit = CellEdit::Formula { formula: Arc::new(diacraft_formula::parse(formula)), value: Some(Value::from_text(&number_text(value))) };
        writes.push((plan.sheet.clone(), CellAddr::direct(name), edit));
    }
    plan.rows
}

/// Takes out the rows from an alignment cell of the shapes `from`, and adds `add`: one pass over the page's rows.
fn replace_rows(doc: &mut Document, page: u32, from: &HashSet<u32>, add: Vec<Connect>) -> Result<(), Problem> {
    let p = page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?;
    #[cfg(test)]
    tally(&ROWS_READ, p.connects.len());
    let mut list: Vec<Connect> =
        p.connects.iter().filter(|c| !(from.contains(&c.from_sheet) && c.from_cell.as_deref().is_some_and(is_align))).cloned().collect();
    list.extend(add);
    p.connects = Connects::from_vec(list);
    Ok(())
}

/// What [`hold_shapes`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Holding {
    /// The `ID`s of the shapes held, in the page's order.
    pub held: Vec<u32>,
    /// Those of them whose hold was written: a shape held already as the placing would hold it is not written again.
    pub written: Vec<u32>,
}

/// A row as what it says, to compare.
fn row_key(c: &Connect) -> (Option<&str>, Option<u32>, u32, Option<&str>, Option<u32>) {
    (c.from_cell.as_deref(), c.from_part, c.to_sheet, c.to_cell.as_deref(), c.to_part)
}

/// Whether the shape holds already what `plan` would write: the formulas of its six alignment cells and its pins, and its rows from alignment cells.
fn held_already(shape: &Shape, plan: &Plan, rows: &[&Connect]) -> bool {
    let text = |name: &str| shape.sheet.cell(name).and_then(|c| c.formula.text());
    let cells = plan.cells.iter().all(|(name, cell)| text(name) == cell.formula.text());
    let pins = plan.pins.iter().all(|(name, formula, _)| text(name) == Some(*formula));
    let mut have: Vec<_> = rows.iter().map(|c| row_key(c)).collect();
    let mut want: Vec<_> = plan.rows.iter().map(row_key).collect();
    have.sort_unstable();
    want.sort_unstable();
    cells && pins && have == want
}

/// Holds the shapes `ids` of a page to the guides `holds` name (the first hold of each axis counts). The shapes are where the placing
/// put them: a shape is held on an axis only where its own edge or middle is on that guide. The cells are written, the shapes' rows from alignment cells
/// replaced, and what reads the cells worked out (`settle`), in the caller's undo step; a shape held already as it would be is not written, so a placing
/// that changes nothing stays no undo step.
pub fn hold_shapes(doc: &mut Document, page: u32, ids: &[u32], holds: &[Hold]) -> Result<Holding, Problem> {
    let x = holds.iter().find(|h| h.axis == Axis::X && h.cell().is_some()).copied();
    let y = holds.iter().find(|h| h.axis == Axis::Y && h.cell().is_some()).copied();
    if (x.is_none() && y.is_none()) || ids.is_empty() {
        return Ok(Holding::default());
    }
    let wanted: HashSet<u32> = ids.iter().copied().take(MAX_HELD).collect();
    let k = units_per_inch(doc, page);
    let p = doc.page(page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    // A guide whose line misses its pin (a file's guide whose local pin is off its line) is held to by nothing, since the formulas name the
    // pin and would put the shape off the line it snapped to.
    let (x, y) = {
        let reader = Reader::new(doc);
        let through_pin = |hold: Option<Hold>| {
            hold.filter(|hold| {
                p.shapes.iter().position(|s| s.id == hold.guide && s.kind == ShapeType::Guide).and_then(|i| u32::try_from(i).ok()).is_some_and(|i| {
                    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(page), i));
                    let pin = if hold.axis == Axis::X { reader.number(&sheet, "PinX") } else { reader.number(&sheet, "PinY") };
                    (pin / k - hold.line).abs() <= ON_LINE
                })
            })
        };
        (through_pin(x), through_pin(y))
    };
    if x.is_none() && y.is_none() {
        return Ok(Holding::default());
    }
    let mut held: Vec<u32> = Vec::new();
    let plans: Vec<Plan> = {
        let reader = Reader::new(doc);
        let mut plans: Vec<(Plan, &Shape)> = Vec::new();
        for (i, shape) in p.shapes.iter().enumerate() {
            #[cfg(test)]
            tally(&SHAPES_READ, 1);
            if !wanted.contains(&shape.id) || shape.kind == ShapeType::Guide || shape.del {
                continue;
            }
            let Ok(position) = u32::try_from(i) else { break };
            if let Some(plan) = plan(&reader, &ShapeRef::top(Owner::Page(page), position), shape.id, k, x, y) {
                plans.push((plan, shape.as_ref()));
            }
        }
        // The rows the shapes have from alignment cells: one pass, read only when a shape is to be held.
        let mut rows: HashMap<u32, Vec<&Connect>> = HashMap::new();
        if !plans.is_empty() {
            #[cfg(test)]
            tally(&ROWS_READ, p.connects.len());
            for c in p.connects.iter().filter(|c| wanted.contains(&c.from_sheet) && c.from_cell.as_deref().is_some_and(is_align)) {
                rows.entry(c.from_sheet).or_default().push(c);
            }
        }
        held.extend(plans.iter().map(|(plan, _)| plan.id));
        plans
            .into_iter()
            .filter(|(plan, shape)| !held_already(shape, plan, rows.get(&plan.id).map_or(&[][..], Vec::as_slice)))
            .map(|(plan, _)| plan)
            .collect()
    };
    if plans.is_empty() {
        return Ok(Holding { held, written: Vec::new() });
    }
    let before = doc.clone();
    let mut writes: Vec<(SheetRef, CellAddr, CellEdit)> = Vec::with_capacity(plans.len().saturating_mul(ALIGN_CELLS.len() + 2));
    let mut rows: Vec<Connect> = Vec::with_capacity(plans.len().saturating_mul(2));
    let mut written: Vec<u32> = Vec::with_capacity(plans.len());
    for plan in plans {
        written.push(plan.id);
        rows.extend(plan_writes(plan, &mut writes));
    }
    #[cfg(test)]
    tally(&CELLS_WRITTEN, writes.len());
    write_cells(doc, writes)?;
    replace_rows(doc, page, &written.iter().copied().collect(), rows)?;
    settle(doc, &before, page, &written, &[])?;
    Ok(Holding { held, written })
}

/// The holds of shape `id` that its rows say (a row from an alignment cell to a guide of the page that lies along the cell's axis), each at its guide's
/// place: what a resize keeps for the edges it does not move. A shape has six alignment cells, so no more than six of its rows are read: a file that holds
/// more is not read past them, and each row read is looked for among the page's guides once.
pub fn holds_of(doc: &Document, page: u32, id: u32) -> Vec<Hold> {
    let Some(p) = doc.page(page) else { return Vec::new() };
    let rows: Vec<(Axis, Anchor, u32)> = p
        .connects
        .iter()
        .filter(|c| c.from_sheet == id)
        .filter_map(|c| {
            let (axis, anchor) = place_of(c.from_cell.as_deref()?)?;
            Some((axis, anchor, c.to_sheet))
        })
        .take(ALIGN_CELLS.len())
        .collect();
    if rows.is_empty() {
        return Vec::new();
    }
    let Some(snap) = SnapPage::read(doc, page) else { return Vec::new() };
    rows.into_iter()
        .filter_map(|(axis, anchor, guide)| {
            let line = snap.guides.iter().find(|g| g.id == guide && g.axis == axis)?;
            Some(Hold { guide, axis, anchor, line: line.at })
        })
        .collect()
}

// ---- releasing -----------------------------------------------------------------------------------------------------------------------------

/// Whether the cell's own formula names a shape by `Sheet.<id>!`: an alignment cell that does is a **guide cell**, whatever the call round the guide's
/// (`INTERSECTX(...)` as the whole formula, as every glued cell of the samples is; `GUARD(INTERSECTX(...))` or `INTERSECTX(...)+0` too). The same
/// rule [`holders`] finds a shape held to a guide by.
fn names_a_shape(cell: &Cell) -> bool {
    matches!(&cell.formula, FormulaSlot::Local(f) if !sheet_refs(&f.source).is_empty())
}

/// Whether the formula reads the cell `name` of its own sheet: a reference with no sheet before it and one part, anywhere in the formula.
fn reads(cell: Option<&Cell>, name: &str) -> bool {
    let Some(FormulaSlot::Local(f)) = cell.map(|c| &c.formula) else { return false };
    let Ok(ast) = &f.ast else { return false };
    let mut stack: Vec<&Expr> = vec![ast];
    while let Some(e) = stack.pop() {
        match e {
            Expr::Ref(r) => {
                if r.qualifiers.is_empty() && matches!(r.path.as_slice(), [only] if only.index.is_none() && only.name.eq_ignore_ascii_case(name)) {
                    return true;
                }
                qualifier_args(&r.qualifiers, &mut stack);
            }
            Expr::Call(c) => {
                stack.extend(c.args.iter().flatten());
                qualifier_args(&c.qualifiers, &mut stack);
            }
            Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
            Expr::Binary(_, a, b) => {
                stack.push(a);
                stack.push(b);
            }
            Expr::Number(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Color { .. } | Expr::Currency { .. } | Expr::Guid(_) => {}
        }
    }
    false
}

fn qualifier_args<'a>(qualifiers: &'a [Qualifier], stack: &mut Vec<&'a Expr>) {
    for q in qualifiers {
        if let Qualifier::Call { args, .. } = q {
            stack.extend(args.iter().flatten());
        }
    }
}

/// The guide cells of a shape's own (see [`names_a_shape`]).
fn guide_cells(shape: &Shape) -> Vec<&'static str> {
    ALIGN_CELLS.iter().copied().filter(|name| shape.sheet.cell(name).is_some_and(names_a_shape)).collect()
}

/// The writes that release a shape after an edit: each of its guide cells (`cells`) becomes the value it holds, with no formula. A
/// guarded one is left as a drag leaves a guarded cell (it still names its guide, which `guide.delete` then refuses and `shape.delete` releases).
fn freeze(at: &ShapeRef, shape: &Shape, cells: &[&'static str], writes: &mut Vec<(SheetRef, CellAddr, CellEdit)>) {
    for name in cells {
        let Some(cell) = shape.sheet.cell(name) else { continue };
        if cell.is_guarded() {
            continue;
        }
        let value = cell.value.clone().unwrap_or_else(|| Value::from_text("0"));
        writes.push((SheetRef::Shape(at.clone()), CellAddr::direct(name), CellEdit::Value(value)));
    }
}

/// The cells an edit may write that a hold depends on: the pins, which follow the guide cells, and the turn and the flips, which the forms have
/// none of.
const POSE: [&str; 5] = ["PinX", "PinY", "Angle", "FlipX", "FlipY"];

/// How near a turn must be to none or to a half turn, in radians, to be one (as `Pose::is_turned_or_flipped` takes a turn).
const TURN_EPS: f64 = 1.0e-9;

/// How far a guide may lean from its axis, as the sine or cosine of its turn, and lie along it (as the snap takes a guide, `snap/page.rs`).
const TILT: f64 = 1.0e-3;

/// Whether a turn is none (a whole number of turns).
fn no_turn(angle: f64) -> bool {
    let a = angle.rem_euclid(std::f64::consts::TAU);
    a.is_finite() && (a <= TURN_EPS || (a - std::f64::consts::TAU).abs() <= TURN_EPS)
}

/// Whether a turn leaves a held shape in a form R6 gives: none, or a half turn (the half turn only with the pin in the box's middle, which
/// [`verdict`] checks).
fn square_turn(angle: f64) -> bool {
    let a = angle.rem_euclid(std::f64::consts::TAU);
    no_turn(angle) || (a.is_finite() && (a - std::f64::consts::PI).abs() <= TURN_EPS)
}

/// The rows from a shape's alignment cells, by each cell's own name (at most six, however many rows a file holds and in whatever letter case), with the
/// guide of the cell's first row.
type RowCells = BTreeMap<&'static str, u32>;

/// The rows from the alignment cells of the shapes `ids`: one pass over the page's rows.
fn rows_of(doc: &Document, page: u32, ids: &HashSet<u32>) -> HashMap<u32, RowCells> {
    let mut out: HashMap<u32, RowCells> = HashMap::new();
    if let Some(p) = doc.page(page) {
        #[cfg(test)]
        tally(&ROWS_READ, p.connects.len());
        for c in p.connects.iter() {
            if ids.contains(&c.from_sheet)
                && let Some(cell) = c.from_cell.as_deref().and_then(place_of).and_then(|(axis, anchor)| cell_for(axis, anchor))
            {
                out.entry(c.from_sheet).or_default().entry(cell).or_insert(c.to_sheet);
            }
        }
    }
    out
}

/// What becomes of a shape's hold after an edit wrote its pose.
enum Verdict {
    /// Still held, or never held.
    Keep,
    /// Held again by `Form`, where the shape is now, its rows kept: the free axis of a hold to one guide was written, or a glued pin was
    /// written back on its own line (a nudge along the guide).
    Renew(Form),
    Release,
}

/// Where a guide's line is on `axis` (drawing units, the page's frame): its pin's coordinate, when the guide lies along that axis (upright for x, level
/// for y). The guides are top-level shapes, at `guides` by their `ID`s.
fn line_of(reader: &Reader<'_>, guides: &HashMap<u32, ShapeRef>, guide: u32, axis: Axis) -> Option<f64> {
    let sheet = SheetRef::Shape(guides.get(&guide)?.clone());
    let angle = reader.number(&sheet, "Angle");
    let along = match axis {
        Axis::X => angle.cos().abs() < TILT,
        Axis::Y => angle.sin().abs() < TILT,
    };
    along.then(|| reader.number(&sheet, if axis == Axis::X { "PinX" } else { "PinY" })).filter(|v| v.is_finite())
}

/// The verdict on the shape at `sheet`, whose guide cells are `cells` and whose rows are `rows`, on a page of `k` drawing units to a paper inch whose guides
/// are at `guides`:
/// * released when it is turned (but by a half turn) or flipped, or turned by a half turn with its pin off its box's middle, where the
///   forms name the wrong edge;
/// * kept when each pin still reads a cell of its axis that names a guide or that a row is from;
/// * else held again where it is when every axis whose pin no longer reads its cell is the free axis of a hold to one guide or a glued axis
///   whose edge is still on its guide's line, within `ON_LINE` (a pin written back on its own line, as a nudge along the guide writes it);
/// * else released whole (a glued pin written off its line, and so a crossing written on either axis off its line: Q4, ours).
fn verdict(reader: &Reader<'_>, sheet: &SheetRef, cells: &[&'static str], rows: &RowCells, k: f64, guides: &HashMap<u32, ShapeRef>) -> Verdict {
    if cells.is_empty() && rows.is_empty() {
        return Verdict::Keep;
    }
    let pose = reader.pose(sheet);
    if pose.flip_x || pose.flip_y || !square_turn(pose.angle) {
        return Verdict::Release;
    }
    let (w, h) = (reader.number(sheet, "Width"), reader.number(sheet, "Height"));
    // Turned by a half turn, the box is where the forms put it only when the pin is in its middle (a size that is not a number is no middle).
    let in_middle = (pose.loc.0 - w / 2.0).abs() <= ON_LINE * k && (pose.loc.1 - h / 2.0).abs() <= ON_LINE * k;
    if !no_turn(pose.angle) && !in_middle {
        return Verdict::Release;
    }
    let broken: Vec<Axis> = [(Axis::X, "PinX"), (Axis::Y, "PinY")]
        .into_iter()
        .filter(|(axis, pin)| {
            let named: Vec<&str> = cells.iter().copied().chain(rows.keys().copied()).filter(|n| axis_of(n) == Some(*axis)).collect();
            if named.is_empty() {
                return false;
            }
            // The pin's formula where the shape finds it: its own, or its master's.
            let formula = reader.resolver().resolve(sheet, &CellAddr::direct(pin)).and_then(|found| found.formula.cell);
            !named.iter().any(|n| reads(formula, n))
        })
        .map(|(axis, _)| axis)
        .collect();
    if broken.is_empty() {
        return Verdict::Keep;
    }
    let glued_on = |axis: Axis| rows.iter().find(|(cell, _)| axis_of(cell) == Some(axis)).map(|(cell, guide)| (*cell, *guide));
    // The box, unturned, in the parent's frame (the page's), and where on it the glued cell is.
    let (x0, y0) = (pose.pin.0 - pose.loc.0, pose.pin.1 - pose.loc.1);
    let on_its_line = |cell: &'static str, guide: u32| {
        let Some((axis, anchor)) = place_of(cell) else { return false };
        let (low, size) = if axis == Axis::X { (x0, w) } else { (y0, h) };
        let edge = match anchor {
            Anchor::Low => low,
            Anchor::Middle => low + size / 2.0,
            Anchor::High => low + size,
            Anchor::Point => return false,
        };
        line_of(reader, guides, guide, axis).is_some_and(|line| (edge - line).abs() <= ON_LINE * k)
    };
    for axis in &broken {
        match glued_on(*axis) {
            Some((cell, guide)) if on_its_line(cell, guide) => {}
            Some(_) => return Verdict::Release,
            // The free axis of a hold to one guide: its middle cell has no row.
            None if rows.len() == 1 => {}
            None => return Verdict::Release,
        }
    }
    let value = |cell: &str| reader.number(sheet, cell);
    match (rows.len(), glued_on(Axis::X), glued_on(Axis::Y)) {
        (1, Some((glued, guide)), None) | (1, None, Some((glued, guide))) => Verdict::Renew(Form::One { glued, guide, value: value(glued) }),
        (2, Some((xc, upright)), Some((yc, level))) => Verdict::Renew(Form::Crossing { x: (xc, upright, value(xc)), y: (yc, level, value(yc)) }),
        _ => Verdict::Release,
    }
}

/// The writes that hold the shape at `at` (`ID` `id`) again by `form`, at the middle of its box where it is now; its rows are kept as they are. `false`
/// when its box is not a number.
fn renew(reader: &Reader<'_>, at: &ShapeRef, id: u32, form: &Form, writes: &mut Vec<(SheetRef, CellAddr, CellEdit)>) -> bool {
    let sheet = SheetRef::Shape(at.clone());
    let pose = reader.pose(&sheet);
    let (w, h) = (reader.number(&sheet, "Width"), reader.number(&sheet, "Height"));
    let (x0, y0) = (pose.pin.0 - pose.loc.0, pose.pin.1 - pose.loc.1);
    if ![w, h, x0, y0].iter().all(|v| v.is_finite()) {
        return false;
    }
    let plan = compose(id, sheet, form, (x0 + w / 2.0, y0 + h / 2.0), pose.pin);
    // The rows are the shape's already.
    let _rows = plan_writes(plan, writes);
    true
}

/// Writes the releases (and the renewals) and takes out the released shapes' rows from alignment cells. → whether anything was written.
fn apply_release(doc: &mut Document, page: u32, writes: Vec<(SheetRef, CellAddr, CellEdit)>, released: &HashSet<u32>) -> Result<bool, Problem> {
    if writes.is_empty() && released.is_empty() {
        return Ok(false);
    }
    #[cfg(test)]
    tally(&CELLS_WRITTEN, writes.len());
    if !writes.is_empty() {
        write_cells(doc, writes)?;
    }
    if !released.is_empty() {
        replace_rows(doc, page, released, Vec::new())?;
    }
    Ok(true)
}

/// The guides of a page, top-level shapes, by their `ID`s: one pass over the top-level shapes.
fn guides_of(doc: &Document, page: u32) -> HashMap<u32, ShapeRef> {
    doc.page(page)
        .map(|p| {
            p.shapes
                .iter()
                .enumerate()
                .filter(|(_, s)| s.kind == ShapeType::Guide)
                .filter_map(|(i, s)| Some((s.id, ShapeRef::top(Owner::Page(page), u32::try_from(i).ok()?))))
                .collect()
        })
        .unwrap_or_default()
}

/// The guide release rule after an edit that wrote the cells `changed` on page `page`: each shape whose pin, turn or flips the edit wrote is judged by
/// [`verdict`]: released (its guide cells hold their values and no formula, and its rows from alignment cells go), held again where it is, or kept. →
/// whether anything was written. One pass over `changed`, and when a pose cell was written one over the page's rows and, when a shape written holds a
/// row, one over the page's top-level shapes for the guides.
pub(crate) fn release_broken(doc: &mut Document, page: u32, changed: &[(SheetRef, CellAddr)]) -> Result<bool, Problem> {
    let pose_cells: Vec<CellAddr> = POSE.iter().map(|name| CellAddr::direct(name)).collect();
    let mut written: BTreeMap<&[u32], &ShapeRef> = BTreeMap::new();
    for (sheet, addr) in changed {
        if let SheetRef::Shape(at) = sheet
            && at.owner == Owner::Page(page)
            && pose_cells.contains(addr)
        {
            written.entry(at.path.as_slice()).or_insert(at);
        }
    }
    if written.is_empty() {
        return Ok(false);
    }
    let k = units_per_inch(doc, page);
    let (writes, released) = {
        let reader = Reader::new(doc);
        let shapes: Vec<(&ShapeRef, &Shape)> = written.values().filter_map(|at| doc.shape(at).map(|s| (*at, s.as_ref()))).collect();
        let ids: HashSet<u32> = shapes.iter().map(|(_, s)| s.id).collect();
        let rows = rows_of(doc, page, &ids);
        // The guides: read only when a shape written holds a row.
        let guides: HashMap<u32, ShapeRef> = if rows.is_empty() { HashMap::new() } else { guides_of(doc, page) };
        let none = RowCells::new();
        let mut writes = Vec::new();
        let mut released: HashSet<u32> = HashSet::new();
        for (at, shape) in shapes {
            let cells = guide_cells(shape);
            let sheet = SheetRef::Shape(at.clone());
            match verdict(&reader, &sheet, &cells, rows.get(&shape.id).unwrap_or(&none), k, &guides) {
                Verdict::Keep => {}
                Verdict::Renew(form) if !guarded(&reader, &sheet) && renew(&reader, at, shape.id, &form, &mut writes) => {}
                Verdict::Renew(_) | Verdict::Release => {
                    released.insert(shape.id);
                    freeze(at, shape, &cells, &mut writes);
                }
            }
        }
        (writes, released)
    };
    apply_release(doc, page, writes, &released)
}

/// Visits every shape of a page, nested ones too (as deep as the model allows), with its path, until `f` says to stop. → whether it stopped.
fn walk<'a>(doc: &'a Document, page: u32, mut f: impl FnMut(&ShapeRef, &'a Shape) -> bool) -> bool {
    let Some(p) = doc.page(page) else { return false };
    let mut stack: Vec<(&'a ShapeList, Vec<u32>)> = vec![(&p.shapes, Vec::new())];
    while let Some((list, prefix)) = stack.pop() {
        for (i, shape) in list.iter().enumerate() {
            #[cfg(test)]
            tally(&SHAPES_READ, 1);
            let Ok(i) = u32::try_from(i) else { break };
            let mut path = prefix.clone();
            path.push(i);
            if f(&ShapeRef { owner: Owner::Page(page), path: path.clone() }, shape) {
                return true;
            }
            if !shape.children.is_empty() && path.len() < MAX_SHAPE_DEPTH {
                stack.push((&shape.children, path));
            }
        }
    }
    false
}

/// Whether an alignment cell of the shape's own names one of `guides` with `Sheet.<id>!`.
fn names_one_of(shape: &Shape, guides: &HashSet<u32>) -> bool {
    ALIGN_CELLS.iter().any(|name| {
        shape.sheet.cell(name).and_then(|c| c.formula.text()).is_some_and(|source| sheet_refs(source).iter().any(|(_, _, id)| guides.contains(id)))
    })
}

/// Whether a shape of the page is held to the guide `guide`: a row names it, or an alignment cell of a shape names it (a file's shape that follows the
/// guide with no row, as ten shapes of the samples do). One pass over the rows and, failing that, one walk of the page.
pub fn held_to(doc: &Document, page: u32, guide: u32) -> bool {
    if doc.page(page).is_some_and(|p| p.connects.iter().any(|c| c.to_sheet == guide)) {
        return true;
    }
    let one: HashSet<u32> = [guide].into_iter().collect();
    walk(doc, page, |_, shape| shape.id != guide && names_one_of(shape, &one))
}

/// The shapes of a page held to one of the shapes `of`: an alignment cell of theirs names it (every glued cell names the guide its row goes to,
/// and a shape held by a row alone follows nothing; its rows to a shape that goes go with it), those of `of` left out, with their paths: one walk of the
/// page.
fn holders<'a>(doc: &'a Document, page: u32, of: &HashSet<u32>) -> Vec<(ShapeRef, &'a Shape)> {
    let mut out = Vec::new();
    walk(doc, page, |at, shape| {
        if !of.contains(&shape.id) && names_one_of(shape, of) {
            out.push((at.clone(), shape));
        }
        false
    });
    out
}

/// Freezes the alignment cells of the shape at `at` whose formulas name a shape for which `named` holds, `GUARD` and all (`remap::freeze_refs_of`, the
/// rule of [`holders`]): the shape they name is going, or the copy is to keep no hold. → how many cells.
fn freeze_named(doc: &mut Document, page: u32, at: &ShapeRef, named: &dyn Fn(u32) -> bool) -> usize {
    let Some(shape) = shape_mut(doc, page, &at.path) else { return 0 };
    let next = freeze_refs_of(&shape.sheet, &ALIGN_CELLS, named);
    let frozen = ALIGN_CELLS.iter().filter(|name| next.cell(name) != shape.sheet.cell(name)).count();
    shape.sheet = next;
    frozen
}

/// Releases the shapes `targets` whole by [`freeze_named`] and takes out their rows from alignment cells. → whether any was.
fn release_named(doc: &mut Document, page: u32, targets: &[(ShapeRef, u32)], named: &dyn Fn(u32) -> bool) -> Result<bool, Problem> {
    if targets.is_empty() {
        return Ok(false);
    }
    let mut released: HashSet<u32> = HashSet::with_capacity(targets.len());
    for (at, id) in targets {
        let _frozen = freeze_named(doc, page, at, named);
        #[cfg(test)]
        tally(&CELLS_WRITTEN, _frozen);
        released.insert(*id);
    }
    replace_rows(doc, page, &released, Vec::new())?;
    Ok(true)
}

/// Releases the shapes held to one of `of` but those of `except`: before a deletion (`of` and `except` the `ID`s
/// of every shape that goes), so that no formula is left naming a shape that is gone; and before a guide is grouped (`of` the guides; their shapes, in
/// the group or not, are released while the guide's pin is still the page's). Every alignment cell of theirs that names one of `of` is frozen, whatever
/// its formula (`GUARD(INTERSECTX(...))` and `INTERSECTX(...)+0` too). → whether any was. Nothing is walked when `of` is
/// empty.
pub(crate) fn release_holders(doc: &mut Document, page: u32, of: &HashSet<u32>, except: &HashSet<u32>) -> Result<bool, Problem> {
    if of.is_empty() {
        return Ok(false);
    }
    let targets: Vec<(ShapeRef, u32)> =
        holders(doc, page, of).into_iter().filter(|(_, shape)| !except.contains(&shape.id)).map(|(at, shape)| (at, shape.id)).collect();
    release_named(doc, page, &targets, &|id| of.contains(&id))
}

/// Releases the shapes `ids` (a paste's copies, every copy is released, one left where its original is too, as no form of R4 is two guide
/// cells with no row): each that has a guide cell or a row from an alignment cell has every alignment cell that names a shape frozen. One pass over the
/// rows and one walk of the page.
pub(crate) fn release_shapes(doc: &mut Document, page: u32, ids: &HashSet<u32>) -> Result<bool, Problem> {
    if ids.is_empty() {
        return Ok(false);
    }
    let targets: Vec<(ShapeRef, u32)> = {
        let rows = rows_of(doc, page, ids);
        let mut targets = Vec::new();
        walk(doc, page, |at, shape| {
            if ids.contains(&shape.id) && (!guide_cells(shape).is_empty() || rows.contains_key(&shape.id)) {
                targets.push((at.clone(), shape.id));
            }
            false
        });
        targets
    };
    release_named(doc, page, &targets, &|_| true)
}

/// The shapes held to a guide whose `PinX`, `PinY` or `Angle` is among the cells `changed` (they follow it by recalculation): an edit adds them
/// to the shapes it changed, so that the connectors glued to them are placed and routed again. Nothing is walked when no guide's
/// cell changed.
pub(crate) fn carried_by_guides(doc: &Document, page: u32, changed: &[(SheetRef, CellAddr)]) -> Vec<u32> {
    let lines: Vec<CellAddr> = ["PinX", "PinY", "Angle"].iter().map(|name| CellAddr::direct(name)).collect();
    let moved: HashSet<u32> = changed
        .iter()
        .filter_map(|(sheet, addr)| {
            let SheetRef::Shape(at) = sheet else { return None };
            if at.owner != Owner::Page(page) || at.path.len() != 1 || !lines.contains(addr) {
                return None;
            }
            doc.shape(at).filter(|s| s.kind == ShapeType::Guide).map(|s| s.id)
        })
        .collect();
    if moved.is_empty() {
        return Vec::new();
    }
    holders(doc, page, &moved).into_iter().map(|(_, shape)| shape.id).collect()
}

/// What [`shift_holds`] gives: the `ID`s it left out of the carrying, and the cells it wrote.
pub(crate) type Carried = (HashSet<u32>, Vec<(SheetRef, CellAddr)>);

/// **And holds**: before a page that grows on the left or the bottom carries its top-level shapes by `by` (drawing units), the shapes
/// held whole to guides are taken out of the carrying, since their guides carry them: one held at a crossing is left as it is, and one held to a guide has
/// its hold written at its place moved by `by` (the guide carries the glued axis, and the place the free one). → the `ID`s left out of the carrying, and
/// the cells written. A shape whose hold is broken is carried as any other and released by the recalculation.
pub(crate) fn shift_holds(doc: &mut Document, page: u32, by: (f64, f64)) -> Result<Carried, Problem> {
    let tops: HashSet<u32> = doc.page(page).map(|p| p.shapes.iter().map(|s| s.id).collect()).unwrap_or_default();
    let rows = rows_of(doc, page, &tops);
    if rows.is_empty() {
        return Ok((HashSet::new(), Vec::new()));
    }
    let mut kept: HashSet<u32> = HashSet::new();
    let mut writes: Vec<(SheetRef, CellAddr, CellEdit)> = Vec::new();
    let (k, guides) = (units_per_inch(doc, page), guides_of(doc, page));
    {
        let Some(p) = doc.page(page) else { return Ok((HashSet::new(), Vec::new())) };
        let reader = Reader::new(doc);
        for (i, shape) in p.shapes.iter().enumerate() {
            let (Some(own), Ok(i)) = (rows.get(&shape.id), u32::try_from(i)) else { continue };
            let at = ShapeRef::top(Owner::Page(page), i);
            let sheet = SheetRef::Shape(at.clone());
            let cells = guide_cells(shape);
            if shape.kind == ShapeType::Guide
                || !matches!(verdict(&reader, &sheet, &cells, own, k, &guides), Verdict::Keep)
                || guarded(&reader, &sheet)
            {
                continue;
            }
            let axes: BTreeSet<bool> = own.keys().map(|cell| axis_of(cell) == Some(Axis::X)).collect();
            match (own.len(), own.iter().next()) {
                (2, _) if axes.len() == 2 => {
                    kept.insert(shape.id);
                }
                (1, Some((glued, guide))) => {
                    let pose = reader.pose(&sheet);
                    let (w, h) = (reader.number(&sheet, "Width"), reader.number(&sheet, "Height"));
                    let place = (pose.pin.0 - pose.loc.0 + w / 2.0 + by.0, pose.pin.1 - pose.loc.1 + h / 2.0 + by.1);
                    if !(place.0.is_finite() && place.1.is_finite()) {
                        continue;
                    }
                    let form = Form::One { glued, guide: *guide, value: reader.number(&sheet, glued) };
                    let _row = plan_writes(compose(shape.id, sheet, &form, place, (pose.pin.0 + by.0, pose.pin.1 + by.1)), &mut writes);
                    kept.insert(shape.id);
                }
                _ => {}
            }
        }
    }
    let cells: Vec<(SheetRef, CellAddr)> = writes.iter().map(|(sheet, addr, _)| (sheet.clone(), addr.clone())).collect();
    if !writes.is_empty() {
        #[cfg(test)]
        tally(&CELLS_WRITTEN, writes.len());
        write_cells(doc, writes)?;
    }
    Ok((kept, cells))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::thread::LocalKey;

    use diacraft_doc::{CellName, RawAttrs, Sheet};

    use super::*;
    use crate::draw::{delete_shapes, move_shapes};
    use crate::snap::guides::{guide_shape, move_guide};

    fn reset() {
        for counter in [&ROWS_READ, &CELLS_WRITTEN, &SHAPES_READ] {
            counter.with(|c| c.set(0));
        }
    }

    fn read(counter: &'static LocalKey<std::cell::Cell<usize>>) -> usize {
        counter.with(std::cell::Cell::get)
    }

    fn length(n: f64) -> Cell {
        Cell::constant(Value::from_text(&number_text(n))).with_unit("IN")
    }

    /// A box 2 by 1 with its pin in its middle at (`x`, `y`), as a drop makes one (constants of its own).
    fn box_shape(id: u32, (x, y): (f64, f64)) -> Arc<Shape> {
        let mut sheet = Sheet::default();
        for (name, v) in [("PinX", x), ("PinY", y), ("Width", 2.0), ("Height", 1.0), ("LocPinX", 1.0), ("LocPinY", 0.5)] {
            sheet.cells.insert(CellName::new(name), length(v));
        }
        Arc::new(Shape { sheet: Arc::new(sheet), ..Shape::new(id) })
    }

    /// `n` rows between shapes that are not on the page, as a page of many connectors has.
    fn other_rows(n: u32) -> Vec<Connect> {
        (0..n)
            .map(|i| Connect {
                from_sheet: 1_000_000 + i,
                from_cell: Some(Arc::from("BeginX")),
                from_part: Some(9),
                to_sheet: 2_000_000 + i,
                to_cell: Some(Arc::from("PinX")),
                to_part: Some(3),
                raw: RawAttrs::new(),
                ext: Default::default(),
            })
            .collect()
    }

    fn page_with(shapes: Vec<Arc<Shape>>, rows: Vec<Connect>) -> Document {
        let mut doc = Document::new_blank();
        let p = page_mut(&mut doc, 1).unwrap();
        p.shapes = ShapeList::from_vec(shapes);
        p.connects = Connects::from_vec(rows);
        doc
    }

    /// An upright guide (`ID` 1) at x 3 and `n` boxes (`ID` 2 up) whose left edges are on it, one above another, and `extra` other rows.
    fn on_one_guide(n: u32, extra: u32) -> Document {
        let mut shapes = vec![Arc::new(guide_shape(1, Axis::X, 3.0))];
        shapes.extend((0..n).map(|i| box_shape(i + 2, (4.0, 1.0 + 2.0 * f64::from(i)))));
        page_with(shapes, other_rows(extra))
    }

    fn upright(line: f64) -> Hold {
        Hold { guide: 1, axis: Axis::X, anchor: Anchor::Low, line }
    }

    fn pin(doc: &Document, id: u32) -> (f64, f64) {
        let reader = Reader::new(doc);
        let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(1), id).unwrap());
        (reader.number(&sheet, "PinX"), reader.number(&sheet, "PinY"))
    }

    #[test]
    fn holding_shapes_looks_at_each_shape_and_reads_the_rows_once_and_writes_eight_cells_each() {
        for n in [256_u32, 512] {
            let mut doc = on_one_guide(n, n);
            let ids: Vec<u32> = (2..n + 2).collect();
            reset();
            let holding = hold_shapes(&mut doc, 1, &ids, &[upright(3.0)]).unwrap();
            let n = n as usize;
            assert_eq!((holding.held.len(), holding.written.len()), (n, n));
            assert_eq!(read(&SHAPES_READ), n + 1, "each shape of the page is looked at once");
            assert_eq!(read(&CELLS_WRITTEN), 8 * n, "six alignment cells and two pins for each");
            // The rows: one pass for the shapes' own (the `n` others), one to replace them, and one by the release of the recalculation (the others and the
            // `n` new ones), which finds every hold whole and releases nothing.
            assert_eq!(read(&ROWS_READ), 2 * n + 2 * n);
            assert_eq!(doc.page(1).unwrap().connects.len(), 2 * n);
            // Held again where they are, nothing is written: the same passes over the shapes and the rows, and no cell.
            let before = doc.clone();
            reset();
            let again = hold_shapes(&mut doc, 1, &ids, &[upright(3.0)]).unwrap();
            assert_eq!((again.held.len(), again.written.len()), (n, 0));
            assert_eq!((read(&SHAPES_READ), read(&ROWS_READ), read(&CELLS_WRITTEN)), (n + 1, 2 * n, 0));
            assert_eq!(doc, before);
        }
    }

    #[test]
    fn moving_a_guide_with_a_thousand_shapes_held_to_it_is_linear() {
        // The recalculation works out what names the guide and what reads those: four cells for each shape (its two guide cells and its pins).
        let (mut steps, mut lookups) = (Vec::new(), Vec::new());
        for n in [500_u32, 1_000] {
            let mut doc = on_one_guide(n, 0);
            let ids: Vec<u32> = (2..n + 2).collect();
            assert_eq!(hold_shapes(&mut doc, 1, &ids, &[upright(3.0)]).unwrap().held.len(), n as usize);
            reset();
            diacraft_calc::work::reset();
            diacraft_doc::work::reset();
            crate::draw::NODES_WALKED.with(|c| c.set(0));
            assert!(move_guide(&mut doc, 1, 1, 5.0).unwrap());
            // The shapes visited to find shapes by their `ID`: the searches of `find_shape` and the walks that note many shapes at once (the shapes the guide
            // carries are in the edit's `touched`, and are looked for by every step of the settling).
            lookups.push(diacraft_doc::work::finds_visited() + crate::draw::NODES_WALKED.with(std::cell::Cell::get) as u64);
            for id in [2, n / 2, n + 1] {
                assert!((pin(&doc, id).0 - 6.0).abs() < 1e-9, "{id}: {:?}", pin(&doc, id));
            }
            // The guide's pin was written: the shapes it carries are found (one walk of the page) so that their connectors are routed again, and the
            // release reads the rows once and releases nothing.
            assert_eq!(read(&ROWS_READ), n as usize);
            assert_eq!(read(&SHAPES_READ), n as usize + 1);
            assert_eq!(read(&CELLS_WRITTEN), 0);
            assert_eq!(diacraft_calc::work::commits(), 1, "one recalculation");
            steps.push(diacraft_calc::work::steps());
        }
        // Twice the shapes, twice the steps (to 10%), and at most 250 steps for each shape (205 measured: two calls of six arguments, two pins).
        assert!(steps[1] * 10 <= steps[0] * 22, "{steps:?}");
        assert!(steps[1] <= 250 * 1_000, "{steps:?}");
        // The lookups too: not one walk of the page for each shape the guide carries (the re-review: 1,003,003 visits for 1,000 shapes; the walk that finds
        // them is `SHAPES_READ` above).
        assert!(lookups[1] * 10 <= lookups[0] * 22, "{lookups:?}");
        // One walk of the page of 1,001 shapes, for the settling, and the search for the guide (1,002 measured): a shape looked up once is not looked for
        // again by a later step of the settling.
        assert!(lookups[1] <= 1_001 + 10, "{lookups:?}");
    }

    #[test]
    fn moving_one_guide_of_a_thousand_crossings_is_linear_too() {
        // Every box is held at the crossing of the upright guide 1 at x 3 and the level guide 2 at y 1, by its left and bottom edges.
        let mut steps = Vec::new();
        for n in [500_u32, 1_000] {
            let mut shapes = vec![Arc::new(guide_shape(1, Axis::X, 3.0)), Arc::new(guide_shape(2, Axis::Y, 1.0))];
            shapes.extend((0..n).map(|i| box_shape(i + 3, (4.0, 1.5))));
            let mut doc = page_with(shapes, Vec::new());
            let ids: Vec<u32> = (3..n + 3).collect();
            let level = Hold { guide: 2, axis: Axis::Y, anchor: Anchor::Low, line: 1.0 };
            assert_eq!(hold_shapes(&mut doc, 1, &ids, &[upright(3.0), level]).unwrap().held.len(), n as usize);
            assert_eq!(doc.page(1).unwrap().connects.len(), 2 * n as usize, "a row from each of the two cells");
            diacraft_calc::work::reset();
            assert!(move_guide(&mut doc, 1, 2, 2.0).unwrap());
            assert!((pin(&doc, n + 2).1 - 2.5).abs() < 1e-9 && (pin(&doc, n + 2).0 - 4.0).abs() < 1e-9, "{:?}", pin(&doc, n + 2));
            steps.push(diacraft_calc::work::steps());
        }
        assert!(steps[1] * 10 <= steps[0] * 22, "{steps:?}");
    }

    #[test]
    fn a_move_that_breaks_a_thousand_holds_reads_the_rows_twice_and_writes_two_cells_each() {
        let n = 1_000_u32;
        let mut doc = on_one_guide(n, 0);
        let ids: Vec<u32> = (2..n + 2).collect();
        hold_shapes(&mut doc, 1, &ids, &[upright(3.0)]).unwrap();
        reset();
        assert_eq!(move_shapes(&mut doc, 1, &ids, 1.0, 0.0).unwrap(), n as usize);
        let n = n as usize;
        // One pass to find the rows of the shapes whose pins were written, one to take them out.
        assert_eq!(read(&ROWS_READ), 2 * n);
        assert_eq!(read(&CELLS_WRITTEN), 2 * n, "the two guide cells of each");
        assert!(doc.page(1).unwrap().connects.is_empty());
        assert!(!held_to(&doc, 1, 1), "nothing names the guide");
    }

    #[test]
    fn the_refusal_reads_the_rows_and_walks_each_shape_once() {
        // Held by rows, the refusal is the first row it reads; held by none, it walks each shape once and finds none naming the guide.
        let n = 2_000_u32;
        let mut doc = on_one_guide(n, 0);
        hold_shapes(&mut doc, 1, &[2], &[upright(3.0)]).unwrap();
        reset();
        assert!(held_to(&doc, 1, 1));
        assert_eq!(read(&SHAPES_READ), 0);
        let doc = on_one_guide(n, n);
        reset();
        assert!(!held_to(&doc, 1, 1));
        assert_eq!(read(&SHAPES_READ), n as usize + 1);
    }

    #[test]
    fn deleting_five_hundred_guides_at_once_walks_the_page_once() {
        // 500 upright guides (`ID` 1 to 500) each holding one box: deleting every guide as shapes releases the 500 boxes in one walk of the page.
        let n = 500_u32;
        let mut shapes: Vec<Arc<Shape>> = (0..n).map(|i| Arc::new(guide_shape(i + 1, Axis::X, 3.0 * f64::from(i)))).collect();
        shapes.extend((0..n).map(|i| box_shape(n + 1 + i, (3.0 * f64::from(i) + 1.0, 2.0))));
        let mut doc = page_with(shapes, Vec::new());
        for i in 0..n {
            let hold = Hold { guide: i + 1, axis: Axis::X, anchor: Anchor::Low, line: 3.0 * f64::from(i) };
            assert_eq!(hold_shapes(&mut doc, 1, &[n + 1 + i], &[hold]).unwrap().held, vec![n + 1 + i]);
        }
        let guides: Vec<u32> = (1..=n).collect();
        reset();
        assert_eq!(delete_shapes(&mut doc, 1, &guides).unwrap(), n as usize);
        let n = n as usize;
        assert_eq!(read(&SHAPES_READ), 2 * n, "the page's shapes, once");
        assert_eq!(read(&CELLS_WRITTEN), 2 * n);
        assert!(doc.page(1).unwrap().connects.is_empty());
        for id in [501_u32, 750, 1_000] {
            let shape = doc.shape(&doc.find_shape(Owner::Page(1), id).unwrap()).unwrap().clone();
            assert!(guide_cells(&shape).is_empty(), "{id} names no guide");
        }
    }

    #[test]
    fn a_shape_is_held_only_where_its_own_edge_is_on_the_guide() {
        // The edge 0.001 inch off the line is not on it; turned, flipped, a line, a guide or a guarded pin is not held.
        let mut doc = on_one_guide(1, 0);
        assert!(hold_shapes(&mut doc, 1, &[2], &[upright(3.001)]).unwrap().held.is_empty());
        let mut sheet = Sheet::clone(&box_shape(3, (4.0, 5.0)).sheet);
        sheet.cells.insert(CellName::new("Angle"), Cell::constant(Value::from_text("0.5")));
        let turned = Arc::new(Shape { sheet: Arc::new(sheet), ..Shape::new(3) });
        let mut sheet = Sheet::clone(&box_shape(4, (4.0, 7.0)).sheet);
        sheet.cells.insert(CellName::new("PinX"), Cell::with_formula("GUARD(4)", Some(Value::from_text("4"))));
        let guarded = Arc::new(Shape { sheet: Arc::new(sheet), ..Shape::new(4) });
        let mut doc = page_with(vec![Arc::new(guide_shape(1, Axis::X, 3.0)), turned, guarded], Vec::new());
        let before = doc.clone();
        assert!(hold_shapes(&mut doc, 1, &[1, 3, 4], &[upright(3.0)]).unwrap().held.is_empty());
        assert_eq!(doc, before, "nothing written");
        // No hold, no page, no shape: nothing, and no error.
        assert!(hold_shapes(&mut doc, 1, &[3], &[]).unwrap().held.is_empty());
        assert!(hold_shapes(&mut doc, 1, &[], &[upright(3.0)]).unwrap().held.is_empty());
        assert!(hold_shapes(&mut doc, 9, &[3], &[upright(3.0)]).is_err());
    }

    #[test]
    fn a_pin_reads_a_cell_by_its_bare_name_anywhere_in_its_formula() {
        let reads_text = |formula: &str, name: &str| reads(Some(&Cell::with_formula(formula, None)), name);
        assert!(reads_text("LocPinX+AlignLeft", "AlignLeft"));
        assert!(reads_text("IF(1,(alignleft*2),0)", "AlignLeft"), "any case, inside a call");
        assert!(!reads_text("Sheet.4!AlignLeft+LocPinX", "AlignLeft"), "another sheet's");
        assert!(!reads_text("LocPinX+AlignLeftX", "AlignLeft"));
        assert!(!reads_text("\"AlignLeft\"", "AlignLeft"), "text is no reference");
        assert!(!reads_text("LocPinX+((", "AlignLeft"), "a formula that does not parse reads nothing");
        assert!(!reads(None, "AlignLeft"));
        assert!(!reads(Some(&length(3.0)), "AlignLeft"));
        // A deep formula (a chain of 400 sums is a tree 400 deep, within the parser's 512) is walked with a stack of its own.
        let deep = format!("{}AlignLeft", "1+".repeat(400));
        assert!(Cell::with_formula(&deep, None).owns_formula());
        assert!(reads_text(&deep, "AlignLeft"));
    }

    #[test]
    fn a_shape_with_a_flood_of_rows_from_its_cells_is_read_for_six_holds_at_most() {
        // A hostile file: 100,000 rows from the alignment cells of one shape to a guide, on a page of 1,000 guides. Each row looked for among the
        // guides would be 1E8 comparisons; a shape has six alignment cells, and no more than six of its rows are read.
        let mut shapes: Vec<Arc<Shape>> = (0..1_000_u32).map(|i| Arc::new(guide_shape(i + 1, Axis::X, 0.01 * f64::from(i)))).collect();
        shapes.push(box_shape(5_000, (4.0, 2.0)));
        let rows: Vec<Connect> = (0..100_000_usize)
            .map(|i| Connect {
                from_sheet: 5_000,
                from_cell: Some(Arc::from(ALIGN_CELLS[i % ALIGN_CELLS.len()])),
                from_part: Some(1),
                to_sheet: 1,
                to_cell: Some(Arc::from(TO_CELL)),
                to_part: Some(TO_PART),
                raw: RawAttrs::new(),
                ext: Default::default(),
            })
            .collect();
        let mut doc = page_with(shapes, rows);
        // The three x cells of the first six rows are held to the upright guide 1.
        assert_eq!(holds_of(&doc, 1, 5_000).len(), 3);
        // Moved, the shape is released, its names read once each however many rows hold them.
        reset();
        move_shapes(&mut doc, 1, &[5_000], 0.5, 0.0).unwrap();
        assert_eq!(read(&ROWS_READ), 2 * 100_000, "a pass to find the rows and one to take them out");
        assert!(doc.page(1).unwrap().connects.is_empty());
    }

    #[test]
    fn an_edge_within_a_millionth_of_an_inch_is_on_the_guide_and_one_a_hundred_thousandth_off_is_not() {
        // `ON_LINE`: half a millionth off (the noise of a scaled drawing's arithmetic) is on the line; a hundred-thousandth off is a shape beside it.
        for (off, held) in [(0.0, true), (5.0e-7, true), (1.0e-5, false)] {
            let mut doc = page_with(vec![Arc::new(guide_shape(1, Axis::X, 3.0)), box_shape(2, (4.0 + off, 2.0))], Vec::new());
            let holding = hold_shapes(&mut doc, 1, &[2], &[upright(3.0)]).unwrap();
            assert_eq!(holding.held.len(), usize::from(held), "{off}");
        }
    }

    #[test]
    fn a_flipped_shape_or_one_whose_alignment_cell_is_guarded_is_not_held() {
        let with = |id: u32, name: &str, cell: Cell| {
            let mut sheet = Sheet::clone(&box_shape(id, (4.0, 2.0 * f64::from(id))).sheet);
            sheet.cells.insert(CellName::new(name), cell);
            Arc::new(Shape { sheet: Arc::new(sheet), ..Shape::new(id) })
        };
        let flipped = with(2, "FlipX", Cell::constant(Value::from_text("1")));
        let flipped_up = with(3, "FlipY", Cell::constant(Value::from_text("1")));
        let guarded = with(4, "AlignLeft", Cell::with_formula("GUARD(3)", Some(Value::from_text("3"))));
        let mut doc = page_with(vec![Arc::new(guide_shape(1, Axis::X, 3.0)), flipped, flipped_up, guarded], Vec::new());
        let before = doc.clone();
        let holding = hold_shapes(&mut doc, 1, &[2, 3, 4], &[upright(3.0)]).unwrap();
        assert!(holding.held.is_empty(), "{holding:?}");
        assert_eq!(doc, before, "nothing written");
    }

    #[test]
    fn a_guide_whose_line_misses_its_pin_is_held_to_by_nothing() {
        // An upright guide with its pin at x 3 and a local pin of (0, 0.5) draws its line at x 3.5. A shape snapped to the line is not held:
        // the formulas name the pin, and would put it at 3.
        let mut off = guide_shape(1, Axis::X, 3.0);
        Arc::make_mut(&mut off.sheet).cells.insert(CellName::new("LocPinY"), length(0.5));
        let mut doc = page_with(vec![Arc::new(off), box_shape(2, (4.5, 2.0))], Vec::new());
        let snap = SnapPage::read(&doc, 1).unwrap();
        assert_eq!(snap.guides.first().map(|g| g.at), Some(3.5), "the line the snap pulls to");
        assert!(hold_shapes(&mut doc, 1, &[2], &[upright(3.5)]).unwrap().held.is_empty());
        // Through its pin, the same guide holds the shape.
        let mut doc = page_with(vec![Arc::new(guide_shape(1, Axis::X, 3.5)), box_shape(2, (4.5, 2.0))], Vec::new());
        assert_eq!(hold_shapes(&mut doc, 1, &[2], &[upright(3.5)]).unwrap().held, vec![2]);
    }
}
