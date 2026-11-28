//! The text block of a shape on its own: where it is, and moving, resizing and turning it (
//! and "Rotate text").
//!
//! A shape's text is laid out in a rectangle of its own, the **text block**, that sits in the shape's frame by seven cells (`TxtPinX`, `TxtPinY`,
//! `TxtLocPinX`, `TxtLocPinY`, `TxtWidth`, `TxtHeight`, `TxtAngle`). The Text Block tool "moves, resizes and rotates the text area apart from the
//! shape, which then keeps that offset"; these functions write those cells and nothing of the shape.
//!
//! * **The block's frame** is: a point of the block goes to the shape's by subtracting the local pin, negating x under `FlipX` and y under
//!   `FlipY`, turning by `TxtAngle` and adding the pin. That is [`Pose::up`] with the block's cells and the shape's flips, so a point of the block is
//!   carried to the page by that and then by the poses of the shape and its groups. The flips of the groups the shape is in are taken as well, as
//!   the renderer takes them (no flip above a shape mirrors its text), two of one axis cancelling. A cell found nowhere has
//!   the value (the block is the shape).
//! * Our rule (the seven travel as a set). A shape that has none of the seven cells, on itself, its master or a style, is
//!   given all seven before the first is changed, with the formulas a public page prints as the ones written on creation (`Width*1` and so on), so a
//!   file that is saved holds the set a shape that shows text holds. A shape that has any keeps what it has.
//! * Our rule. The block is moved by a distance on the page. The distance is carried into the shape's frame (down through the
//!   groups the shape is in and then its own turn and flips), and `TxtPinX` and `TxtPinY` change by it, each written as a value of its own (a write
//!   replaces a formula; a `BOUND` or `SETATREF` formula keeps itself). So the block "keeps that offset": a later move
//!   of the shape carries it, and a later resize of the shape leaves a pin that is a value where it was. A cell a `GUARD` protects is an error.
//! * Our rule, after a change. The block is given a size along its own axes with the corner at (0, 0) of its own frame at a point
//!   of the page. `TxtWidth` and `TxtHeight` are written; the local pin is worked out again for the new size (a pin at the middle stays at the
//!   middle: `TxtWidth*0.5` is a formula still), and the pin is the corner carried by the block's turn and flips, so the corner lands there. The shape,
//!   `TxtAngle` and the local pin's cells are not written.
//! * Our rule (gives the angle). The block turns about its pin, which stays where it is: only `TxtAngle` is written. `by`
//!   is a turn on the page, counterclockwise: `TxtAngle` changes by it, or by its negative when the picture of the shape on the page is a mirror
//!   image (an odd number of flips among the shape and its groups: the net angle is `Angle` − `TxtAngle` under one flip), so the text turns
//!   the way it was asked and never mirrors. `to` sets the cell, in the range the files hold (just over -pi to pi, as the rotate rule keeps
//!   `Angle`). A formula in `TxtAngle` that keeps the text upright (`-Angle`) is replaced by the value, as any write replaces a formula.
//! * **No route and no label is placed again.** The cells are recalculated with what reads them (`settle_kept`) and nothing else: a connector's
//!   label that is moved here stays where it was put until the connector is routed again, when the label rule places it on the middle leg.

use diacraft_calc::Changes;
use diacraft_doc::{CellAddr, CellEdit, Document, Resolver, SheetRef};

use super::arrange::{MAX_TURN, SAME_PLACE, Turn, same_angle, tidy_angle, write_angle_cell};
use super::read::{Pose, Reader};
use super::route::P;
use super::settle::settle_kept;
use super::shapes::evaluate_number;
use super::worked::Worked;
use super::{Problem, checked, require_shape, write_cells, write_length};
use crate::stencil::text_transform_cells;

/// The seven cells of the text block.
const BLOCK_CELLS: [&str; 7] = ["TxtPinX", "TxtPinY", "TxtLocPinX", "TxtLocPinY", "TxtWidth", "TxtHeight", "TxtAngle"];

/// Where a shape's text block is, on the page, in the page's drawing units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockPlace {
    /// The four corners of the block on the page, in the order of its own frame: the corner at (0, 0), then (`width`, 0), (`width`, `height`) and
    /// (0, `height`). `text_block_resize` takes the first.
    pub corners: [P; 4],
    /// The block's pin on the page: the point it turns about.
    pub pin: P,
    /// The block's size along its own axes (`TxtWidth`, `TxtHeight`).
    pub width: f64,
    pub height: f64,
    /// `TxtAngle`, in radians, as the cell holds it.
    pub angle: f64,
    /// The way the block's x axis points on the page, in radians: the direction the text runs (the net angle).
    pub direction: f64,
}

/// The text block's cells in the shape's drawing units, with the values for the cells found nowhere.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Block {
    width: f64,
    height: f64,
    pin: P,
    loc: P,
    angle: f64,
}

impl Block {
    /// The block's frame in the shape's, with the flips of the shape whose pose is `shape` and those of the groups it is in. The
    /// groups' flips are the renderer's too (no flip above a shape mirrors its text, so the block takes them as it takes the shape's,
    /// about its local pin, and the block is drawn where this says), and with them the frame is right-handed on the page.
    fn pose(&self, shape: &Pose, groups: (bool, bool)) -> Pose {
        Pose { pin: self.pin, loc: self.loc, angle: self.angle, flip_x: shape.flip_x != groups.0, flip_y: shape.flip_y != groups.1 }
    }
}

/// What a text-block cell is: the number it holds on the shape, its master or a style (a cell that holds a formula and no saved value is what the formula works out to),
/// else the printed formula (a cell and a factor, `Width*0.5`, which names another such cell, three deep at most), as the renderer reads it; 0 when there is none.
fn number_of(res: &Resolver<'_>, worked: &Worked<'_>, sheet: &SheetRef, name: &str, depth: u8) -> f64 {
    let addr = CellAddr::direct(name);
    let Some(found) = res.resolve(sheet, &addr) else { return 0.0 };
    if let Some(n) = worked.number_of(sheet, &addr, &found) {
        return n;
    }
    let Some(formula) = found.default.formula.filter(|_| depth < 4) else { return 0.0 };
    let Some((base, factor)) = formula.split_once('*') else { return 0.0 };
    let Ok(factor) = factor.trim().parse::<f64>() else { return 0.0 };
    let value = number_of(res, worked, sheet, base.trim(), depth + 1) * factor;
    if value.is_finite() { value } else { 0.0 }
}

fn read_block(reader: &Reader<'_>, sheet: &SheetRef) -> Block {
    read_block_with(reader.resolver(), reader.worked(), sheet)
}

fn read_block_with(res: &Resolver<'_>, worked: &Worked<'_>, sheet: &SheetRef) -> Block {
    let n = |name: &str| number_of(res, worked, sheet, name, 0);
    Block {
        width: n("TxtWidth"),
        height: n("TxtHeight"),
        pin: (n("TxtPinX"), n("TxtPinY")),
        loc: (n("TxtLocPinX"), n("TxtLocPinY")),
        angle: n("TxtAngle"),
    }
}

/// The four corners of the text block of the shape `sheet` **in the shape's own frame**, in the drawing units of its cells, in the order of the block's own frame: (0, 0), (`width`, 0),
/// (`width`, `height`), (0, `height`). `shape` is the shape's pose and `groups` the flips of the groups it is in (`Reader::group_flips`: the block takes them as the
/// renderer's text does). The window's pointer reads a text block with this, through the resolver and the reader of unsaved formulas it already holds, and [`text_block_place`] carries the
/// same corners to the page: one rule for where a block is.
pub fn text_block_in_frame(res: &Resolver<'_>, worked: &Worked<'_>, sheet: &SheetRef, shape: &Pose, groups: (bool, bool)) -> [P; 4] {
    let block = read_block_with(res, worked, sheet);
    let pose = block.pose(shape, groups);
    [(0.0, 0.0), (block.width, 0.0), (block.width, block.height), (0.0, block.height)].map(|p| pose.up(p))
}

/// Where the text block of shape `id` on a page is: `None` for a page or shape that is not there.
pub fn text_block_place(doc: &Document, page: u32, id: u32) -> Option<BlockPlace> {
    let reader = Reader::new(doc);
    let at = reader.find(page, id)?;
    let sheet = SheetRef::Shape(at.clone());
    let block = read_block(&reader, &sheet);
    let pose = block.pose(&reader.pose(&sheet), reader.group_flips(&at));
    let corners = text_block_in_frame(reader.resolver(), reader.worked(), &sheet, &reader.pose(&sheet), reader.group_flips(&at))
        .map(|p| reader.to_page(&at, p));
    let along = reader.vector_to_page(&at, pose.up_vector((1.0, 0.0)));
    Some(BlockPlace {
        corners,
        pin: reader.to_page(&at, block.pin),
        width: block.width,
        height: block.height,
        angle: block.angle,
        direction: along.1.atan2(along.0),
    })
}

/// Gives a shape that has none of the seven cells all seven. → the names written, for the recalculation.
fn ensure_set(doc: &mut Document, sheet: &SheetRef) -> Result<Vec<&'static str>, Problem> {
    let (has_some, size) = {
        let reader = Reader::new(doc);
        let some = BLOCK_CELLS.iter().any(|name| reader.is_set(sheet, &CellAddr::direct(name)));
        (some, (reader.number(sheet, "Width"), reader.number(sheet, "Height")))
    };
    if has_some {
        return Ok(Vec::new());
    }
    let cells = text_transform_cells(size.0, size.1);
    let names = cells.iter().map(|(name, _)| *name).collect();
    write_cells(doc, cells.into_iter().map(|(name, cell)| (sheet.clone(), CellAddr::direct(name), CellEdit::Cell(cell))).collect())?;
    Ok(names)
}

/// Works out again what reads the cells written, and nothing else.
fn settle_cells(doc: &mut Document, page: u32, sheet: &SheetRef, names: &[&str]) -> Result<(), Problem> {
    let changes = Changes { cells: names.iter().map(|name| (sheet.clone(), CellAddr::direct(name))).collect(), ..Changes::default() };
    settle_kept(doc, page, &changes)
}

/// Moves the text block of shape `id` by (`dx`, `dy`), a distance on the page in drawing units. → whether anything changed: a
/// distance of nothing writes nothing.
pub fn move_text_block(doc: &mut Document, page: u32, id: u32, dx: f64, dy: f64) -> Result<bool, Problem> {
    let (dx, dy) = (checked("dx", dx)?, checked("dy", dy)?);
    let at = require_shape(doc, page, id)?;
    let sheet = SheetRef::Shape(at.clone());
    let (block, shift) = {
        let reader = Reader::new(doc);
        let in_parent = reader.vector_from_page(&at, (dx, dy));
        (read_block(&reader, &sheet), reader.pose(&sheet).down_vector(in_parent))
    };
    let planned: Vec<(&'static str, f64)> = [("TxtPinX", block.pin.0, shift.0), ("TxtPinY", block.pin.1, shift.1)]
        .into_iter()
        .filter(|(_, _, by)| by.abs() > SAME_PLACE)
        .map(|(name, was, by)| (name, was + by))
        .collect();
    if planned.is_empty() {
        return Ok(false);
    }
    let mut written = ensure_set(doc, &sheet)?;
    for (name, value) in planned {
        write_length(doc, &sheet, name, checked(name, value)?)?;
        written.push(name);
    }
    settle_cells(doc, page, &sheet, &written)?;
    Ok(true)
}

/// Gives the text block of shape `id` the size `width` by `height` along its own axes, its corner at (0, 0) of its own frame at (`x`, `y`) on the
/// page: all in drawing units. → whether anything changed: a block that has the size and the place already is left alone.
pub fn resize_text_block(doc: &mut Document, page: u32, id: u32, x: f64, y: f64, width: f64, height: f64) -> Result<bool, Problem> {
    let (x, y, width, height) = (checked("x", x)?, checked("y", y)?, checked("width", width)?, checked("height", height)?);
    if width <= 0.0 || height <= 0.0 {
        return Err(Problem::bad("`width` and `height` must be more than 0"));
    }
    let at = require_shape(doc, page, id)?;
    let sheet = SheetRef::Shape(at.clone());
    let (block, shape, groups, corner) = {
        let reader = Reader::new(doc);
        let shape = reader.pose(&sheet);
        // The corner as the shape's own frame has it: the page's point carried down through the groups, and then down the shape's pose.
        (read_block(&reader, &sheet), shape, reader.group_flips(&at), shape.down(reader.point_from_page(&at, (x, y))))
    };
    let old_corner = block.pose(&shape, groups).up((0.0, 0.0));
    if (block.width - width).abs() <= SAME_PLACE
        && (block.height - height).abs() <= SAME_PLACE
        && (old_corner.0 - corner.0).abs() <= SAME_PLACE
        && (old_corner.1 - corner.1).abs() <= SAME_PLACE
    {
        return Ok(false);
    }
    let mut written = ensure_set(doc, &sheet)?;
    write_length(doc, &sheet, "TxtWidth", width)?;
    write_length(doc, &sheet, "TxtHeight", height)?;
    written.extend(["TxtWidth", "TxtHeight"]);
    // The local pin for the new size, worked out on its own (the pin is placed with it, and the page is recalculated once).
    let loc = (evaluate_number(doc, &sheet, "TxtLocPinX").unwrap_or(block.loc.0), evaluate_number(doc, &sheet, "TxtLocPinY").unwrap_or(block.loc.1));
    // The pin is the local pin carried from the corner by the block's turn and the flips of the shape and of its groups.
    let to_pin = Pose { pin: (0.0, 0.0), loc: (0.0, 0.0), angle: block.angle, flip_x: shape.flip_x != groups.0, flip_y: shape.flip_y != groups.1 }
        .up_vector(loc);
    for (name, was, now) in [("TxtPinX", block.pin.0, corner.0 + to_pin.0), ("TxtPinY", block.pin.1, corner.1 + to_pin.1)] {
        if (now - was).abs() > SAME_PLACE {
            write_length(doc, &sheet, name, checked(name, now)?)?;
            written.push(name);
        }
    }
    settle_cells(doc, page, &sheet, &written)?;
    Ok(true)
}

/// Turns the text block of shape `id` about its pin: `Turn::By` a turn on the page, counterclockwise, and `Turn::To` an
/// angle for `TxtAngle`, both in radians. → whether anything changed: an angle the cell has already writes nothing.
pub fn rotate_text_block(doc: &mut Document, page: u32, id: u32, turn: Turn) -> Result<bool, Problem> {
    let (Turn::To(value) | Turn::By(value)) = turn;
    if !value.is_finite() || value.abs() > MAX_TURN {
        return Err(Problem::bad("the angle must be a number of a few turns at most"));
    }
    let at = require_shape(doc, page, id)?;
    let sheet = SheetRef::Shape(at.clone());
    let (block, mirrored) = {
        let reader = Reader::new(doc);
        (read_block(&reader, &sheet), reader.is_mirrored(&at))
    };
    let wanted = tidy_angle(match turn {
        Turn::To(angle) => angle,
        Turn::By(by) => block.angle + if mirrored { -by } else { by },
    });
    if same_angle(wanted, block.angle) {
        return Ok(false);
    }
    let mut written = ensure_set(doc, &sheet)?;
    write_angle_cell(doc, &sheet, "TxtAngle", wanted)?;
    written.push("TxtAngle");
    settle_cells(doc, page, &sheet, &written)?;
    Ok(true)
}
