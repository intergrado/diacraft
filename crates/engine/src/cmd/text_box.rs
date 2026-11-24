//! The Text tool's side of the engine: `text.box` makes a box of text, and `text.block`, `text.block_move`, `text.block_resize` and
//! `text.block_rotate` look at, move, resize and turn a shape's text block on its own, as the Text Block tool does. The rules are named in
//! `draw/text_block.rs`, `stencil/text_box.rs` and `docs/editing-commands.md`.
//!
//! Lengths are paper inches on the page, as the other editing commands have them, and angles are degrees counterclockwise; the cells hold the page's
//! drawing units and radians, and the command turns them at its edge.

use serde_json::{Value, json};

use super::arrange::degrees;
use super::params::{optional_number, optional_whole, page_for, required_number, units_per_inch};
use super::{CommandSpec, has_document, optional_text, required_u32};
use crate::Session;
use crate::draw::{self, Turn};
use crate::error::{Result, bad, failed};
use crate::stencil::{TEXT_BOX_HEIGHT, TEXT_BOX_WIDTH};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "text.box",
            "Text Box",
            ["Insert", "Text"],
            None,
            "page: whole number, the id of the page; \
             x: number, where the box's centre goes, in inches from the page's left edge; \
             y: number, where the box's centre goes, in inches from the page's bottom edge; \
             width: number, optional, more than 0, the box's width in inches (default 1); \
             height: number, optional, more than 0, the box's height in inches (default 0.25); \
             text: text, optional, what the box says (default nothing)",
            has_document,
            add_box
        ),
        cmd!(
            query "text.block",
            "Show Text Block",
            ["Home", "Tools"],
            None,
            "id: whole number, the shape; page: whole number, optional, the id of the page (default: the first page with the shape)",
            has_document,
            show_block
        ),
        cmd!(
            "text.block_move",
            "Move Text Block",
            ["Home", "Tools"],
            None,
            "id: whole number, the shape; dx: number, inches to the right on the page; dy: number, inches up on the page; \
             page: whole number, optional, the id of the page (default: the first page with the shape)",
            has_document,
            move_block
        ),
        cmd!(
            "text.block_resize",
            "Resize Text Block",
            ["Home", "Tools"],
            None,
            "id: whole number, the shape; \
             x: number, inches from the page's left edge to the corner of the block that is at (0, 0) of the block's own frame (text.block lists the corners in that order, \
             the first being it); \
             y: number, inches from the page's bottom edge to that same corner; \
             width: number, more than 0, the block's size along its own x axis in inches; height: number, more than 0, along its own y axis; \
             page: whole number, optional, the id of the page (default: the first page with the shape)",
            has_document,
            resize_block
        ),
        cmd!(
            "text.block_rotate",
            "Rotate Text Block",
            ["Home", "Tools"],
            None,
            "id: whole number, the shape; \
             angle: number, optional, degrees counterclockwise, the angle TxtAngle is set to (give angle or by, not both); \
             by: number, optional, degrees counterclockwise on the page that the block turns about its pin (a shape that is mirrored on the page changes TxtAngle the other way, so the text turns as asked); \
             page: whole number, optional, the id of the page (default: the first page with the shape)",
            has_document,
            rotate_block
        ),
    ]
}

/// The drawing units to a paper inch on a page.
fn scale(s: &Session, page: u32) -> f64 {
    s.document().map_or(1.0, |doc| units_per_inch(doc, page))
}

/// The page the command works on: the one named, or the first that has the shape (see [`page_for`]).
fn target_page(s: &Session, p: &Value, id: u32, cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    page_for(doc, named, &[id], cmd)
}

/// Makes a text box → `{page, id, width, height}`, the size in inches.
fn add_box(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "text.box";
    let page = required_u32(p, "page", ID)?;
    let (x, y) = (required_number(p, "x", ID)?, required_number(p, "y", ID)?);
    let width = optional_number(p, "width", ID)?.unwrap_or(TEXT_BOX_WIDTH);
    let height = optional_number(p, "height", ID)?.unwrap_or(TEXT_BOX_HEIGHT);
    let text = optional_text(p, "text", ID)?.unwrap_or("");
    // The `ID` is above every one the session has given on the page and the file it saved from holds (`Session::id_floor`).
    let above = s.id_floor(page);
    let k = scale(s, page);
    let info = s.edit("Add text box", |doc| {
        let info =
            draw::add_text_box_above(doc, page, (x * k, y * k), (width * k, height * k), text, above).map_err(|problem| problem.into_engine(ID))?;
        // A page that has a theme gives it to the box (`theme/apply.rs`): the text takes the theme's colour and face, and the box keeps
        // the no fill and no line it is made with.
        crate::theme::follow_page_theme(doc, page, info.id).map_err(|problem| problem.into_engine(ID))?;
        crate::draw::layers::join_active(doc, page, info.id).map_err(|problem| problem.into_engine(ID))?;
        // A page with Auto Size on grows to take the box in (`draw/page_fit.rs`), in the same undo step, as a dropped shape's does.
        draw::fit_page(doc, page, &[info.id]).map_err(|problem| problem.into_engine(ID))?;
        Ok(info)
    })?;
    s.note_id(page, info.id);
    Ok(json!({ "page": page, "id": info.id, "width": info.width / k, "height": info.height / k }))
}

/// Where the text block of a shape is → `{page, id, corners, pin, width, height, angle, direction}`: the corners and the pin on the page in inches
/// (the corners in the order of the block's own frame, (0, 0) first), the size in inches, `angle` the cell `TxtAngle` and `direction` the way the text runs
/// on the page, both in degrees.
fn show_block(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "text.block";
    let id = required_u32(p, "id", ID)?;
    let page = target_page(s, p, id, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let k = units_per_inch(doc, page);
    let place = draw::text_block_place(doc, page, id).ok_or_else(|| bad(ID, format!("page {page} has no shape {id}")))?;
    let inches = |(x, y): (f64, f64)| json!([x / k, y / k]);
    Ok(json!({
        "page": page,
        "id": id,
        "corners": place.corners.map(inches),
        "pin": inches(place.pin),
        "width": place.width / k,
        "height": place.height / k,
        "angle": place.angle.to_degrees(),
        "direction": place.direction.to_degrees(),
    }))
}

/// Moves the text block of a shape by a distance on the page → `{page, id, changed}`.
fn move_block(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "text.block_move";
    let id = required_u32(p, "id", ID)?;
    let (dx, dy) = (required_number(p, "dx", ID)?, required_number(p, "dy", ID)?);
    let page = target_page(s, p, id, ID)?;
    let k = scale(s, page);
    s.edit_if("Move text block", |doc| {
        let changed = draw::move_text_block(doc, page, id, dx * k, dy * k).map_err(|problem| problem.into_engine(ID))?;
        Ok((json!({ "page": page, "id": id, "changed": changed }), changed))
    })
}

/// Gives the text block of a shape a size and a place → `{page, id, changed}`.
fn resize_block(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "text.block_resize";
    let id = required_u32(p, "id", ID)?;
    let (x, y) = (required_number(p, "x", ID)?, required_number(p, "y", ID)?);
    let (width, height) = (required_number(p, "width", ID)?, required_number(p, "height", ID)?);
    let page = target_page(s, p, id, ID)?;
    let k = scale(s, page);
    s.edit_if("Resize text block", |doc| {
        let changed = draw::resize_text_block(doc, page, id, x * k, y * k, width * k, height * k).map_err(|problem| problem.into_engine(ID))?;
        Ok((json!({ "page": page, "id": id, "changed": changed }), changed))
    })
}

/// Turns the text block of a shape about its pin → `{page, id, changed}`.
fn rotate_block(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "text.block_rotate";
    let id = required_u32(p, "id", ID)?;
    let turn = match (degrees(p, "angle", ID)?, degrees(p, "by", ID)?) {
        (Some(angle), None) => Turn::To(angle.to_radians()),
        (None, Some(by)) => Turn::By(by.to_radians()),
        (Some(_), Some(_)) => return Err(bad(ID, "give `angle` or `by`, not both")),
        (None, None) => {
            return Err(bad(ID, "give `angle` (degrees, the angle TxtAngle is set to) or `by` (degrees, the block turns by it on the page)"));
        }
    };
    let page = target_page(s, p, id, ID)?;
    s.edit_if("Rotate text block", |doc| {
        let changed = draw::rotate_text_block(doc, page, id, turn).map_err(|problem| problem.into_engine(ID))?;
        Ok((json!({ "page": page, "id": id, "changed": changed }), changed))
    })
}
