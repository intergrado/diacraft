//! The shape operations: Union, Combine, Fragment, Intersect, Subtract, Join, Trim, Offset and Fit Curve. The rules are in
//! `draw/shapeops/` and `docs/editing-commands.md` ("Shape operations", "Join and Trim" and "Offset and Fit Curve").
//!
//! Each takes `ids`, the shapes to work on, the first named being the first shape (SHAPEOPS-PRIMARY), and `page`; Offset also a `distance` and Fit Curve a
//! `tolerance` and `flags`. Each is one undo step, and one that is refused changes nothing and adds no step. A Fit Curve that finds nothing to make smaller
//! is not refused and changes nothing either.

use serde_json::{Value, json};

use super::params::{optional_whole, required_ids, required_number};
use super::{CommandSpec, has_document};
use crate::Session;
use crate::draw::{self, Op, ShapeOpsArgs};
use crate::error::{Result, bad, failed};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "shape.union",
            "Union",
            ["Developer", "Shape Design", "Operations"],
            None,
            "ids: array of whole numbers, the shapes to unite, two at least, all on the page or all members of one group, the first named being the first shape: \
             one new shape with the outline of all of them, in front, with the first shape's text and formatting, and the originals gone; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            union
        ),
        cmd!(
            "shape.combine",
            "Combine",
            ["Developer", "Shape Design", "Operations"],
            None,
            "ids: array of whole numbers, the shapes to combine, two at least, all on the page or all members of one group, the first named being the first shape: \
             one new shape with a section for each section of each original, in front, so that where they overlapped there is a hole, with the first shape's \
             text and formatting, and the originals gone; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            combine
        ),
        cmd!(
            "shape.fragment",
            "Fragment",
            ["Developer", "Shape Design", "Operations"],
            None,
            "ids: array of whole numbers, the shapes to break up, one at least, all on the page or all members of one group, the first named being the first shape: \
             a new shape for each region the shapes make together, lines among them cutting the regions, in front, with the first shape's formatting and no text, \
             and the originals gone; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            fragment
        ),
        cmd!(
            "shape.intersect",
            "Intersect",
            ["Developer", "Shape Design", "Operations"],
            None,
            "ids: array of whole numbers, the shapes to intersect, two at least, all on the page or all members of one group, the first named being the first shape: \
             one new shape for the area all of them cover, in front, with the first shape's text and formatting, and the originals gone, and refused when no area is \
             common to all; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            intersect
        ),
        cmd!(
            "shape.subtract",
            "Subtract",
            ["Developer", "Shape Design", "Operations"],
            None,
            "ids: array of whole numbers, the shapes, two at least, all on the page or all members of one group: the area of every shape after the first is taken \
             away from the first, which stays the same shape with its id, text, data, glue and place, and the others are deleted, and refused when nothing of the \
             first is left; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            subtract
        ),
        cmd!(
            "shape.join",
            "Join",
            ["Developer", "Shape Design", "Operations"],
            None,
            "ids: array of whole numbers, the shapes to join, two at least, all on the page or all members of one group, the first named being the first shape: \
             one new shape, in front, with the first shape's text and formatting and one section for each chain of their paths whose ends meet, \
             and the originals gone; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            join
        ),
        cmd!(
            "shape.trim",
            "Trim",
            ["Developer", "Shape Design", "Operations"],
            None,
            "ids: array of whole numbers, the shapes to trim, one at least, all on the page or all members of one group, the first named being the first shape: \
             a new shape for each stretch of their paths between the places where the paths cross or touch, in front, with the first shape's formatting \
             and no text, and the originals gone; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            trim
        ),
        cmd!(
            "shape.offset",
            "Offset",
            ["Developer", "Shape Design", "Operations"],
            None,
            "ids: array of whole numbers, the shapes to offset, one at least, all on the page or all members of one group; \
             distance: number above 0, at least 0.0001, in the cells' own units (inches on a page of the default scale, the drawing's units on a scaled one): for each path of each shape two new shapes in front, \
             one on each side of it (outside and inside a closed path, left and right of an open one), each a path a distance from it with the line format of its own \
             original, no fill and no text, and the originals stay, and a side that vanishes (an inner path of a shape narrower than the distance) is not made; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            offset
        ),
        cmd!(
            "shape.fit_curve",
            "Fit Curve",
            ["Developer", "Shape Design", "Operations"],
            None,
            "ids: array of whole numbers, the shapes to fit, one at least, all on the page or all members of one group: each stays the same shape, and the paths that can \
             be made of fewer segments (lines, splines and with flag 2 arcs) within the tolerance get their rows replaced, its box refitted unless it keeps its size, and the answer's not_fitted lists the shapes left as they were that hold something to fit (the tolerance was too tight, or the library gave up); \
             tolerance: number of 0 or more, in the cells' own units like the distance, how far the new paths may stray from the old ones (0 asks for an exact match and changes nothing); \
             flags: whole number, optional, 0 or a sum of 1 (periodic splines: a closed path is fitted as a loop), 2 (circular arcs for what is circular) and 4 (break at \
             abrupt changes of direction); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            fit_curve
        ),
    ]
}

/// Runs `op` → `{page, ids, deleted, kept, changed, not_fitted}`: the `ID`s of the shapes made (the last in front), of the shapes deleted, the shape that stayed (Subtract's first
/// shape, or `null`), the shapes changed in place besides it (Fit Curve's) and the shapes Fit Curve left as they were though they hold something to fit. A refused command is no
/// undo step.
///
/// `args` are the distance of Offset and the tolerance and flags of Fit Curve **in the cells' own units** (inches on a page of the default scale, the drawing's units on a
/// scaled one), as the shape operations take their tolerances: no conversion by the page's drawing scale is made here. The window's dialog is what turns the page's
/// measurement units into them.
fn operate(s: &mut Session, p: &Value, op: Op, args: &ShapeOpsArgs) -> Result<Value> {
    let id = op.id();
    let ids = required_ids(p, "ids", id)?;
    let named = optional_whole(p, "page", id)?;
    let doc = s.document().ok_or_else(|| failed(id, "no document open"))?;
    let page = draw::page_holding(doc, named, &ids).ok_or_else(|| bad(id, "the document has no pages"))?;
    // The `ID`s are above every one the session has given on the page and the file it saved from holds (`Session::id_floor`).
    let above = s.id_floor(page);
    // A command that finds nothing to change (Fit Curve of paths it cannot make smaller) changes nothing: no undo step (`Session::edit_if`).
    let made = s.edit_if(op.name(), |doc| {
        let made = draw::shape_ops_with(doc, page, op, &ids, above, args).map_err(|problem| problem.into_engine(id))?;
        let changed = !(made.produced.is_empty() && made.deleted.is_empty() && made.kept.is_none() && made.changed.is_empty());
        Ok((made, changed))
    })?;
    if let Some(highest) = made.produced.iter().max() {
        s.note_id(page, *highest);
    }
    Ok(
        json!({ "page": page, "ids": made.produced, "deleted": made.deleted, "kept": made.kept, "changed": made.changed, "not_fitted": made.not_fitted }),
    )
}

fn union(s: &mut Session, p: &Value) -> Result<Value> {
    operate(s, p, Op::Union, &ShapeOpsArgs::default())
}

fn combine(s: &mut Session, p: &Value) -> Result<Value> {
    operate(s, p, Op::Combine, &ShapeOpsArgs::default())
}

fn fragment(s: &mut Session, p: &Value) -> Result<Value> {
    operate(s, p, Op::Fragment, &ShapeOpsArgs::default())
}

fn intersect(s: &mut Session, p: &Value) -> Result<Value> {
    operate(s, p, Op::Intersect, &ShapeOpsArgs::default())
}

fn subtract(s: &mut Session, p: &Value) -> Result<Value> {
    operate(s, p, Op::Subtract, &ShapeOpsArgs::default())
}

fn join(s: &mut Session, p: &Value) -> Result<Value> {
    operate(s, p, Op::Join, &ShapeOpsArgs::default())
}

fn trim(s: &mut Session, p: &Value) -> Result<Value> {
    operate(s, p, Op::Trim, &ShapeOpsArgs::default())
}

/// `shape.offset {ids, distance}`: the distance is required, in the cells' own units, and checked by the operation (above 0, finite, at least
/// [`draw::MIN_SHAPE_OPS_OFFSET`] and no more than the path library's reach allows).
fn offset(s: &mut Session, p: &Value) -> Result<Value> {
    let distance = required_number(p, "distance", Op::Offset.id())?;
    operate(s, p, Op::Offset, &ShapeOpsArgs { distance, ..ShapeOpsArgs::default() })
}

/// `shape.fit_curve {ids, tolerance, flags?}`.
fn fit_curve(s: &mut Session, p: &Value) -> Result<Value> {
    let id = Op::FitCurve.id();
    let tolerance = required_number(p, "tolerance", id)?;
    let flags = optional_whole(p, "flags", id)?.unwrap_or(0);
    operate(s, p, Op::FitCurve, &ShapeOpsArgs { tolerance, flags, ..ShapeOpsArgs::default() })
}
