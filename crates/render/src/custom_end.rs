//! A line end drawn from a master (DD18).
//!
//! A cell `BeginArrow` or `EndArrow` of 254 whose formula is `USE("<name>")` names a **master pattern**, a master that is a line end by its `PatternFlags` (bit 2). The master is one
//! shape or one group, drawn **pointing right** with its **pin** (`LocPinX`, `LocPinY`) on the line's end point. DiaCraft draws it so:
//!
//! * **What is drawn** are the sections of its shape, and of each member of a group, as a shape's own are: a section that paints a fill is filled, one that paints an outline is stroked.
//!   Everything takes **the line's colour and transparency** (text and gradient fills in the master are not drawn), and an outline is **one line weight** wide, as the
//!   outline of a hollow numbered mark is. The line ends of the master's own shapes are not drawn: a pattern is geometry, and a `USE` among them is not followed.
//! * **Size.** An unscaled end is as tall as the line is heavy at Medium size: the height of the master's alignment box (its top shape's `Height`) is the weight, and the
//!   drawing is that much scaled, whatever the box is (the geometry may be larger than the box). At another size class the box is the weight times the unit `S` at that class over the
//!   unit at Medium, at the same weight ([`crate::ends::box_share`]: the project rule). A **scaled** end (bit 1024) keeps the master's own size under the page's scale, and the
//!   weight and the size class do not change it.
//! * **Orientation.** An end that turns with the line points the way the line runs at the end, and out of the line at the begin (the project rule for the begin,
//!   places a numbered mark at either end). An upright end (bit 256) keeps the page's orientation.
//! * **The shaft.** A turning end cuts the line where its drawing's back is (the geometry's back along the line, the project rule); an upright end does not trim it.
//!
//! **Caps.** The master is counted before anything is built ([`survey`]): at most [`MAX_END_SECTIONS`] sections and [`MAX_END_ROWS`] rows in all its shapes, at most [`MAX_END_SHAPES`]
//! shapes and [`MAX_END_DEPTH`] levels of groups. A master past a cap, one that is no line end, one with no geometry and one the line's sizes cannot draw are **number 4** with the note
//! The frame charges the counted rows as work before the master is built, so a page of lines that name one master is counted.

use std::sync::Arc;

use diacraft_doc::{CellAddr, Document, FormulaSlot, Master, Owner, ShapeRef, SheetRef};
use diacraft_formula::Expr;
use diacraft_geom::{combined_fill, kurbo::Size};
use kurbo::{Affine, BezPath, Shape as _};

use crate::ends::CustomEnd;
use crate::frame::local_transform;
use crate::geometry;
use crate::limits::{MAX_END_DEPTH, MAX_END_ROWS, MAX_END_SECTIONS, MAX_END_SHAPES};
use crate::read::{Cx, sheet_of};
use crate::style::{FillLook, Looks};

/// The `PatternFlags` bit that makes a master a line end.
const IS_END: u32 = 2;
/// The bit that keeps the end upright (`0x100`).
const UPRIGHT: u32 = 256;
/// The bit that makes the end scaled (`0x400`).
const SCALED: u32 = 1024;

/// How far from the pin a coordinate of a sized end may be, in paper inches: past it the end is one the line's size cannot draw.
const MOST_REACH: f64 = 1.0e7;

/// Why a master is not drawn as a line end: every one of them is number 4 with the note, except [`Refusal::Budget`], which draws nothing (the frame's work budget said no).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The cell names no master, or the master's `PatternFlags` lack 2.
    NotAnEnd,
    /// The master holds no top shape, more than one (a pattern is one shape or one group) or nothing that paints.
    NoGeometry,
    /// More sections, rows, shapes or levels than the caps.
    PastCaps,
    /// The master's box has no height, or the scale the line's weight asks for is no number a drawing can hold.
    Unusable,
    /// The frame's work budget has no room for the master's rows.
    Budget,
}

/// The name a cell's `USE` holds, when its formula is one (`USE("Name")` or `USE({GUID})`): found through inheritance, so a connector that inherits the end from its master has it.
pub(crate) fn used_name(cx: &Cx<'_, '_>, at: &SheetRef, cell: &str) -> Option<Arc<str>> {
    let found = cx.res.resolve(at, &CellAddr::direct(cell))?;
    let FormulaSlot::Local(formula) = &found.formula.cell?.formula else { return None };
    match formula.ast.as_ref().ok()? {
        Expr::Call(call) if call.is("USE") => match call.args.first() {
            Some(Some(Expr::Str(name) | Expr::Guid(name))) => Some(Arc::clone(name)),
            _ => None,
        },
        _ => None,
    }
}

/// The master a `USE` name finds: the first master of the document whose `NameU` or `UniqueID` is the name, as `calc` finds a master by name.
pub(crate) fn master_named<'d>(doc: &'d Document, name: &str) -> Option<&'d Master> {
    doc.masters.iter().find(|m| m.name_u.as_deref() == Some(name) || m.unique_id.as_deref() == Some(name)).map(|m| &**m)
}

/// What a master that is a line end holds, counted before it is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Survey {
    /// Shapes of the master, a group's members counted.
    pub shapes: usize,
    /// Rows of geometry over all of them: the work the master costs ([`crate::limits::WORK_PER_END_ROW`] each).
    pub rows: usize,
    /// The master's `PatternFlags`.
    pub flags: u32,
    /// The master's `ID`.
    pub master: u32,
}

impl Survey {
    /// Whether the end is scaled with the drawing (`PatternFlags` 1024): it keeps the master's own size and ignores the line's weight.
    pub(crate) fn scaled(&self) -> bool {
        self.flags & SCALED != 0
    }
}

/// Counts what `master` holds, and refuses it when it is no line end or is past a cap. A look at each layer of each shape, not at each row, so a master of a million rows
/// costs what a master of ten does; nothing is built.
pub(crate) fn survey(cx: &Cx<'_, '_>, master: &Master) -> Result<Survey, Refusal> {
    let flags = master.pattern_flags.unwrap_or(0);
    if flags & IS_END == 0 {
        return Err(Refusal::NotAnEnd);
    }
    let owner = Owner::Master(master.id);
    match cx.res.shapes(owner).map(|tops| tops.len()) {
        Some(1) => {}
        _ => return Err(Refusal::NoGeometry),
    }
    let (mut shapes, mut sections, mut rows) = (0usize, 0usize, 0usize);
    let mut stack = vec![(ShapeRef::top(owner, 0), 0usize)];
    while let Some((at, depth)) = stack.pop() {
        let Some(shape) = cx.res.shape(&at) else { return Err(Refusal::NoGeometry) };
        if shape.del {
            continue;
        }
        shapes = shapes.saturating_add(1);
        // The shapes counted, and the ones waiting, are held to the cap before any member is listed: a group of a million members is refused at once.
        let pending = shapes.saturating_add(stack.len()).saturating_add(shape.children.len());
        if depth >= MAX_END_DEPTH || pending > MAX_END_SHAPES {
            return Err(Refusal::PastCaps);
        }
        let sheet = sheet_of(&at);
        match cx.res.effective_sections_within(&sheet, "Geometry", MAX_END_SECTIONS.saturating_sub(sections)) {
            Some(Ok(keys)) => {
                sections = sections.saturating_add(keys.len());
                for key in &keys {
                    rows = rows.saturating_add(cx.res.rows_held(&sheet, key).unwrap_or(0));
                    if rows > MAX_END_ROWS {
                        return Err(Refusal::PastCaps);
                    }
                }
            }
            Some(Err(_)) => return Err(Refusal::PastCaps),
            None => {}
        }
        for i in (0..shape.children.len()).rev() {
            if let Ok(i) = u32::try_from(i) {
                stack.push((at.child(i), depth + 1));
            }
        }
    }
    // A master with no section in any shape has nothing to draw, and is counted as number 4 before any work is charged.
    if sections == 0 {
        return Err(Refusal::NoGeometry);
    }
    Ok(Survey { shapes, rows, flags, master: master.id })
}

/// A master's drawing in its own inches, with the pin on the origin and the x axis along the way the master points, before it is sized.
#[derive(Clone, Debug)]
pub(crate) struct Form {
    fill: BezPath,
    stroke: BezPath,
    /// The height of the master's alignment box, in inches: what an unscaled end is as tall as the weight by.
    pub box_height: f64,
    upright: bool,
    scaled: bool,
    /// The rows of geometry the build made (the frame counts them against its own).
    pub rows: usize,
}

/// A finite number, or 0.
fn finite(n: f64) -> f64 {
    if n.is_finite() { n } else { 0.0 }
}

/// Builds the master `survey` counted: the sections of each shape as a shape draws them, moved to the pin of the master's top shape. `rows_left` is what the frame may still build; the build
/// is within the counts, which were held to the caps.
pub(crate) fn build(cx: &Cx<'_, '_>, survey: &Survey, rows_left: usize) -> Result<Form, Refusal> {
    let owner = Owner::Master(survey.master);
    let who = || "a line end's master".to_owned();
    let (mut fill, mut stroke) = (BezPath::new(), BezPath::new());
    let (mut rows, mut box_height) = (0usize, 0.0);
    // The top shape's frame to the pin: the shape's own transform without the move to its pin (the pin is the origin).
    let mut stack = vec![(ShapeRef::top(owner, 0), Affine::IDENTITY, 0usize)];
    while let Some((at, parent, depth)) = stack.pop() {
        let Some(shape) = cx.res.shape(&at) else { continue };
        if shape.del || depth >= MAX_END_DEPTH {
            continue;
        }
        let sheet = sheet_of(&at);
        let local = local_transform(cx, &sheet);
        let affine = if depth == 0 {
            box_height = finite(cx.num(&sheet, "Height"));
            Affine::translate((-finite(cx.num(&sheet, "PinX")), -finite(cx.num(&sheet, "PinY")))) * local
        } else {
            parent * local
        };
        let size = Size::new(finite(cx.num(&sheet, "Width")), finite(cx.num(&sheet, "Height")));
        let Some(geom) = geometry::build(cx, &sheet, size, rows_left.saturating_sub(rows), &who) else { return Err(Refusal::PastCaps) };
        rows = rows.saturating_add(geom.rows);
        // The shape's own look says whether it fills and whether it outlines (a fill pattern or a line pattern of 0 paints none of its sections).
        let no_fill = matches!(Looks::fill(cx, &sheet, None, &who), FillLook::None);
        let outlined = Looks::line(cx, &sheet, None).is_some();
        let filled = combined_fill(geom.sections.iter().filter(|s| s.flags.paint(no_fill).fill).map(|s| (&s.fill, s.flags)));
        fill.extend((affine * filled).elements().iter().copied());
        if outlined {
            for section in geom.sections.iter().filter(|s| s.flags.paint(false).stroke) {
                stroke.extend((affine * section.stroke.clone()).elements().iter().copied());
            }
        }
        for i in (0..shape.children.len()).rev() {
            if let Ok(i) = u32::try_from(i) {
                stack.push((at.child(i), affine, depth + 1));
            }
        }
    }
    if fill.is_empty() && stroke.is_empty() {
        return Err(Refusal::NoGeometry);
    }
    if !(fill.is_finite() && stroke.is_finite()) {
        return Err(Refusal::Unusable);
    }
    Ok(Form { fill, stroke, box_height, upright: survey.flags & UPRIGHT != 0, scaled: survey.flags & SCALED != 0, rows })
}

/// How an end is sized: what the drawing is multiplied by to be in paper inches (inches of the master to inches of paper).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Sizing {
    /// The weight is 0, so the alignment box has no height: nothing is drawn, and nothing is said.
    Nothing,
    /// The drawing is multiplied by this.
    By(f64),
}

impl Form {
    /// The factor that sizes this end for a line of `weight` paper inches at size class `size` on a page of `page_scale` paper inches to a drawing inch:
    /// a scaled end keeps the master's own size under the page's scale; an unscaled one is as tall as the weight times [`crate::ends::box_share`], whatever the box is.
    /// `Err(Unusable)` for a box with no height (an unscaled end cannot be sized by it) and for a factor that is no number a drawing can hold.
    pub(crate) fn sizing(&self, size: i64, weight: f64, page_scale: f64) -> Result<Sizing, Refusal> {
        let factor = if self.scaled {
            page_scale
        } else {
            if weight <= 0.0 {
                return Ok(Sizing::Nothing);
            }
            if !(self.box_height.is_finite() && self.box_height > 0.0) {
                return Err(Refusal::Unusable);
            }
            weight * crate::ends::box_share(size, weight) / self.box_height
        };
        if factor.is_finite() && factor > 0.0 { Ok(Sizing::By(factor)) } else { Err(Refusal::Unusable) }
    }

    /// The end sized by `factor`: its drawing in paper inches, with the trim a turning end makes (where the drawing's back is behind the pin). `None` when the drawing is not one the
    /// line's size can hold (a coordinate no number, or past [`MOST_REACH`]).
    pub(crate) fn sized(&self, factor: f64) -> Option<CustomEnd> {
        let scale = Affine::scale(factor);
        let (fill, strokes) = (scale * self.fill.clone(), scale * self.stroke.clone());
        let boxes = [&fill, &strokes].into_iter().filter(|p| !p.is_empty()).map(|p| p.bounding_box()).reduce(|a, b| a.union(b))?;
        if ![boxes.x0, boxes.y0, boxes.x1, boxes.y1].iter().all(|c| c.is_finite() && c.abs() <= MOST_REACH) {
            return None;
        }
        // The shaft is cut where the drawing's back is: the farthest the geometry reaches behind the pin (the project rule), and not at all for an upright end.
        let trim = if self.upright { 0.0 } else { (-boxes.x0).max(0.0) };
        Some(CustomEnd { fill, strokes, upright: self.upright, trim })
    }
}

#[cfg(test)]
mod tests;
