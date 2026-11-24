//! The shape operations: Union, Combine, Fragment, Intersect and Subtract. The rules are in `draw/shapeops/` and `docs/editing-commands.md`
//! ("Shape operations").
//!
//! Each takes `ids`, the shapes to work on, the first named being the first shape (SHAPEOPS-PRIMARY), and `page`. Each is one undo step, and one that is refused
//! changes nothing and adds no step.

use serde_json::{Value, json};

use super::params::{optional_whole, required_ids};
use super::{CommandSpec, has_document};
use crate::Session;
use crate::draw::{self, Op};
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
    ]
}

/// Runs `op` → `{page, ids, deleted, kept}`: the `ID`s of the shapes made (the last in front), of the shapes deleted, and the shape that stayed (Subtract's first
/// shape, or `null`). A refused command is no undo step.
fn operate(s: &mut Session, p: &Value, op: Op) -> Result<Value> {
    let id = op.id();
    let ids = required_ids(p, "ids", id)?;
    let named = optional_whole(p, "page", id)?;
    let doc = s.document().ok_or_else(|| failed(id, "no document open"))?;
    let page = draw::page_holding(doc, named, &ids).ok_or_else(|| bad(id, "the document has no pages"))?;
    // The `ID`s are above every one the session has given on the page and the file it saved from holds (`Session::id_floor`).
    let above = s.id_floor(page);
    let made = s.edit(op.name(), |doc| draw::shape_ops(doc, page, op, &ids, above).map_err(|problem| problem.into_engine(id)))?;
    if let Some(highest) = made.produced.iter().max() {
        s.note_id(page, *highest);
    }
    Ok(json!({ "page": page, "ids": made.produced, "deleted": made.deleted, "kept": made.kept }))
}

fn union(s: &mut Session, p: &Value) -> Result<Value> {
    operate(s, p, Op::Union)
}

fn combine(s: &mut Session, p: &Value) -> Result<Value> {
    operate(s, p, Op::Combine)
}

fn fragment(s: &mut Session, p: &Value) -> Result<Value> {
    operate(s, p, Op::Fragment)
}

fn intersect(s: &mut Session, p: &Value) -> Result<Value> {
    operate(s, p, Op::Intersect)
}

fn subtract(s: &mut Session, p: &Value) -> Result<Value> {
    operate(s, p, Op::Subtract)
}
