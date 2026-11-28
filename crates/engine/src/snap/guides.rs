//! Guides: the shapes a person drags out of a ruler to lay a drawing out along.
//!
//! **What the specifications give.** A guide is a page shape with `Type="Guide"` (102 files hold one on a page). Its geometry is one
//! `InfiniteLine` row, `IX` 1 of its section, from (`X`, `Y`) to (`A`, `B`) in the shape's own frame, always horizontal there with `X`, `Y` and `B` 0 and
//! `A` above 0, so a vertical guide is a turned shape. It reads 1 for `NonPrinting` and the
//! three `ShapePermeable` cells through the guide style, and so is not printed and does not stop a connector, and a
//! page's extents leave it out (`Shape.BoundingBox`). Learn names the cells a shape's edge is held to a guide by (`Alignment`:
//! `AlignLeft`, `AlignCenter`, `AlignRight`, `AlignBottom`, `AlignMiddle`, `AlignTop`) and the part numbers of the `Connect` rows that join them.
//!
//! **Glue to a guide** is [`super::hold`]: a shape pulled to a guide is held by its `Alignment` cells, which call `INTERSECTX` and
//! `INTERSECTY` of the guide's `PinX`, `PinY` and `Angle` (and `_MARKER(1)` in the cells that hold no guide), its pins' formulas and a `Connect` row
//! And follows the guide by recalculation when [`move_guide`] writes its pin. A guide that
//! shapes are held to cannot be deleted here, since their formulas would be left naming it.
//!
//! **Our rule: no specification gives the cells of a guide a person makes.** The cells a guide made here holds: `PinX`, `PinY`, `Width` and `Height` (the
//! last two 0: it covers no area), `LocPinX` and `LocPinY` 0 (the pin is the geometry's origin), `Angle` (0, or a quarter turn for a vertical guide), the flips
//! 0, one `Geometry` section with `NoFill` 1, `NoSnap` 1 (nothing snaps to the guide's own path: it is a guide, snapped to as one) and the `InfiniteLine` row, and
//! `NonPrinting`, `ShapePermeableX`, `ShapePermeableY` and `ShapePermeablePlace` of 1, written on the shape itself because a new drawing has no guide style
//! to give them. Its style links are the root style's. A horizontal guide's pin is at the page's left edge, and a vertical guide's at its bottom edge.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Owner, Row, Section, Shape, ShapeType, Sheet, SheetRef, Value};

use super::grid::Axis;
use super::page::{GuideLine, MAX_GUIDES, SnapPage};
use crate::draw::{Problem, Reader, angle_text, checked, page_mut, require_page, settle, units_per_inch, write_length};
use crate::stencil::number_text;

/// A guide, for the commands: its `ID`, its axis and its place in paper inches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuideInfo {
    pub id: u32,
    pub axis: Axis,
    pub at: f64,
}

impl From<GuideLine> for GuideInfo {
    fn from(g: GuideLine) -> GuideInfo {
        GuideInfo { id: g.id, axis: g.axis, at: g.at }
    }
}

fn length(n: f64) -> Cell {
    Cell::constant(Value::from_text(&number_text(n))).with_unit("IN")
}

fn plain(n: f64) -> Cell {
    Cell::constant(Value::from_text(&number_text(n)))
}

fn put(cells: &mut std::collections::BTreeMap<CellName, Cell>, name: &str, cell: Cell) {
    cells.insert(CellName::new(name), cell);
}

/// The guide shape `id` along `axis` at `at` drawing units (a vertical guide is at x = `at`, a level one at y = `at`).
pub(crate) fn guide_shape(id: u32, axis: Axis, at: f64) -> Shape {
    let mut sheet = Sheet { links: diacraft_doc::Links { line: Some(0), fill: Some(0), text: Some(0) }, ..Sheet::default() };
    let cells = &mut sheet.cells;
    let (pin, angle) = match axis {
        Axis::X => ((at, 0.0), std::f64::consts::FRAC_PI_2),
        Axis::Y => ((0.0, at), 0.0),
    };
    put(cells, "PinX", length(pin.0));
    put(cells, "PinY", length(pin.1));
    put(cells, "Width", length(0.0));
    put(cells, "Height", length(0.0));
    put(cells, "LocPinX", length(0.0));
    put(cells, "LocPinY", length(0.0));
    // Thirteen places, as a file holds an angle: at nine, a quarter turn tilts the line by 3E-10, which a shape held to it far up the page follows
    // (`INTERSECTX` works with the angle as written: 1E-7 inch at 500 inches).
    put(cells, "Angle", Cell::constant(Value::from_text(&angle_text(angle))).with_unit("RAD"));
    for flat in ["FlipX", "FlipY"] {
        put(cells, flat, plain(0.0));
    }
    for flag in ["NonPrinting", "ShapePermeableX", "ShapePermeableY", "ShapePermeablePlace"] {
        put(cells, flag, plain(1.0));
    }
    let mut section = Section::new("Geometry", Some(0));
    for (flag, value) in [("NoFill", 1.0), ("NoLine", 0.0), ("NoShow", 0.0), ("NoSnap", 1.0), ("NoQuickDrag", 0.0)] {
        put(&mut section.cells, flag, plain(value));
    }
    let mut row = Row { ix: Some(1), row_type: Some(Arc::from("InfiniteLine")), ..Row::default() };
    for (name, n) in [("X", 0.0), ("Y", 0.0), ("A", 1.0), ("B", 0.0)] {
        put(&mut row.cells, name, length(n));
    }
    section.rows.push(Arc::new(row));
    sheet.sections.push(Arc::new(section));
    Shape { kind: ShapeType::Guide, sheet: Arc::new(sheet), ..Shape::new(id) }
}

/// Adds a guide along `axis` at `at` paper inches to a page; its `ID` is above every `ID` on the page and above `above`. → the guide.
pub fn add_guide(doc: &mut Document, page: u32, axis: Axis, at: f64, above: u32) -> Result<GuideInfo, Problem> {
    require_page(doc, page)?;
    let k = units_per_inch(doc, page);
    let at = checked("at", at)?;
    let drawing = checked("at", at * k)?;
    let existing = SnapPage::read(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    if existing.guides.len() + existing.guides_left_out >= MAX_GUIDES {
        return Err(Problem::failed(format!("a page holds at most {MAX_GUIDES} guides")));
    }
    let (highest, count) = crate::draw::page_stats(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    if count >= diacraft_doc::MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {} shapes", diacraft_doc::MAX_SHAPES_PER_PAGE)));
    }
    let id = highest.max(above).checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
    page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?.push_shape(Arc::new(guide_shape(id, axis, drawing)));
    Ok(GuideInfo { id, axis, at })
}

/// The top-level guide `id` of a page: its path, and its line.
fn find_guide(doc: &Document, page: u32, id: u32) -> Result<(diacraft_doc::ShapeRef, GuideLine), Problem> {
    require_page(doc, page)?;
    let snap = SnapPage::read(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    let line = snap.guides.iter().find(|g| g.id == id).copied().ok_or_else(|| Problem::bad(format!("page {page} has no guide {id}")))?;
    let at = doc
        .find_shape(Owner::Page(page), id)
        .filter(|at| {
            doc.page(page)
                .and_then(|p| at.path.first().and_then(|i| p.shapes.get(usize::try_from(*i).ok()?)).map(|s| s.kind == ShapeType::Guide))
                .unwrap_or(false)
        })
        .ok_or_else(|| Problem::bad(format!("page {page} has no guide {id}")))?;
    Ok((at, line))
}

/// Moves a guide to `at` paper inches along its axis: its pin moves by the difference, so that a guide that came from a file moves as it is. → whether it
/// moved (a guide already there is not touched).
pub fn move_guide(doc: &mut Document, page: u32, id: u32, at: f64) -> Result<bool, Problem> {
    let at = checked("at", at)?;
    let (path, line) = find_guide(doc, page, id)?;
    let k = units_per_inch(doc, page);
    let delta = checked("at", (at - line.at) * k)?;
    if delta == 0.0 {
        return Ok(false);
    }
    let before = doc.clone();
    let sheet = SheetRef::Shape(path);
    let cell = if line.axis == Axis::X { "PinX" } else { "PinY" };
    let now = Reader::new(doc).number(&sheet, cell);
    write_length(doc, &sheet, cell, checked(cell, now + delta)?)?;
    settle(doc, &before, page, &[id], &[])?;
    Ok(true)
}

/// Deletes a guide. A guide that shapes are glued to (a `Connect` row names it, or an alignment cell of a shape does: `hold::held_to`) is refused: the
/// glue's formulas would be left naming a shape that is gone. `shape.delete` of the guide releases them instead (`hold::release_holders`).
pub fn delete_guide(doc: &mut Document, page: u32, id: u32) -> Result<(), Problem> {
    let (path, _) = find_guide(doc, page, id)?;
    if super::hold::held_to(doc, page, id) {
        return Err(Problem::failed(
            "shapes are glued to this guide; move them off it first, or delete it as a shape (shape.delete), which leaves them where they are",
        ));
    }
    let Some(index) = path.path.first().and_then(|i| usize::try_from(*i).ok()) else { return Err(Problem::failed("the guide went away")) };
    let removed = page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?.remove_shape(index);
    removed.map(|_| ()).ok_or_else(|| Problem::failed("the guide went away"))
}

/// The guides of a page, in paper inches, with how many were left out (not along an axis).
pub fn list_guides(doc: &Document, page: u32) -> Option<(Vec<GuideInfo>, usize)> {
    let snap = SnapPage::read(doc, page)?;
    Some((snap.guides.iter().copied().map(GuideInfo::from).collect(), snap.guides_left_out))
}
