//! Arrange: rotate, flip, drawing order and align. The rules are in `draw/arrange.rs` and `docs/editing-commands.md`.
//!
//! Angles are degrees, counterclockwise, as the Size & Position window shows them; the cells hold radians and the command turns them at its edge.

use serde_json::{Value, json};

use super::params::{optional_whole, page_for, required_ids, required_number, units_per_inch};
use super::snap_params;
use super::{CommandSpec, has_document, required_text};
use crate::Session;
use crate::draw::{self, Align, FlipAxis, Stack, Turn};
use crate::error::{Result, bad, failed};

/// The most degrees a turn may be given as: past any angle a person types, short of an arithmetic that has lost its digits.
const MAX_DEGREES: f64 = 1.0e6;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "shape.rotate",
            "Rotate Shapes",
            ["Home", "Arrange"],
            None,
            "ids: array of whole numbers, the shapes to turn; \
             angle: number, optional, degrees counterclockwise, the angle each shape is turned to about its own pin (give angle or by, not both); \
             by: number, optional, degrees counterclockwise added to the angle: one shape turns about its pin, several turn as one about the middle of their joint box; \
             about: {x, y}, optional, with by: the point of the page in inches that each shape's pin turns about, instead of the pin or the middle of the joint box, so that a set \
             (a selection, or a group's box) turns about the point the caller names. \
             A shape with LockRotate set is left alone and listed in the result's locked, and the turn is refused when every shape named is locked; \
             snap: {zoom, radius, view}, optional, with angle and one shape: the angle is the pointer's, radius being how far the pointer is from the pin in inches (it is needed), \
             and it comes to the angle of another shape within a pull of 6 points at the pointer, else to the step of the rotation handle for radius times zoom points \
             (15, 5, 1 and 0.1 degrees, finer the farther out), the result's snap.angle being where it came to, and view [x0, y0, x1, y1] the visible part of the page in inches, which a turn has no use for; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            rotate
        ),
        cmd!(
            "shape.flip",
            "Flip Shapes",
            ["Home", "Arrange"],
            None,
            "ids: array of whole numbers, the shapes to flip; axis: text, horizontal (left to right) or vertical (top to bottom): the picture is mirrored, \
             several shapes as one about the middle of their joint box; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            flip
        ),
        cmd!(
            "shape.order",
            "Change Drawing Order",
            ["Home", "Arrange"],
            None,
            "ids: array of whole numbers, the shapes to move in the drawing order; \
             to: text, front or back (the end of the shape's band, as Bring to Front and Send to Back), forward or backward (one place); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            order
        ),
        cmd!(
            "shape.align",
            "Align Shapes",
            ["Home", "Arrange"],
            None,
            "ids: array of whole numbers, the shapes to align: the first one named stays and the others move to it; \
             how: text, left, center, right, top, middle or bottom; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            align
        ),
    ]
}

/// The page the command works on (see [`page_for`]).
fn target_page(s: &Session, p: &Value, ids: &[u32], cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    page_for(doc, named, ids, cmd)
}

/// The degrees `p` gives for `key`, if it gives them: a number within [`MAX_DEGREES`].
pub(super) fn degrees(p: &Value, key: &str, cmd: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_f64() {
            Some(n) if n.is_finite() && n.abs() <= MAX_DEGREES => Ok(Some(n)),
            _ => Err(bad(cmd, format!("`{key}` must be a number of degrees, at most {MAX_DEGREES} either way"))),
        },
    }
}

/// The point `about` names in `p`, in inches: `{x, y}`, finite numbers. `None` when it is left out.
fn about_of(p: &Value, cmd: &str) -> Result<Option<(f64, f64)>> {
    match p.get("about") {
        None | Some(Value::Null) => Ok(None),
        Some(about @ Value::Object(map)) => {
            if let Some(unknown) = map.keys().find(|name| !matches!(name.as_str(), "x" | "y")) {
                return Err(bad(cmd, format!("`about` has an unknown key `{}`; it takes x and y", crate::error::shown(unknown))));
            }
            Ok(Some((required_number(about, "x", cmd)?, required_number(about, "y", cmd)?)))
        }
        Some(_) => Err(bad(cmd, "`about` must be an object {x, y}: a point of the page in inches")),
    }
}

/// Turns shapes → `{page, rotated, locked?}`: how many were turned, and the shapes a `LockRotate` left alone. A turn to the angle a shape has, and a connector with both
/// ends glued, turn nothing and are no undo step.
fn rotate(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.rotate";
    let ids = required_ids(p, "ids", ID)?;
    let turn = match (degrees(p, "angle", ID)?, degrees(p, "by", ID)?) {
        (Some(angle), None) => Turn::To(angle.to_radians()),
        (None, Some(by)) => Turn::By(by.to_radians()),
        (Some(_), Some(_)) => return Err(bad(ID, "give `angle` or `by`, not both")),
        (None, None) => return Err(bad(ID, "give `angle` (degrees, the angle to turn to) or `by` (degrees, added to the angle)")),
    };
    let page = target_page(s, p, &ids, ID)?;
    // The angle the pointer gives comes to another shape's angle, or to the handle's step. The result says where it came to.
    let mut told = Value::Null;
    let turn = match (turn, snap_params::parse(p, ID, snap_params::Words::ROTATE)?, s.document()) {
        (Turn::To(raw), Some(ask), Some(doc)) => {
            let [id] = ids.as_slice() else { return Err(bad(ID, "`snap` turns one shape")) };
            let Some(radius) = ask.radius else { return Err(bad(ID, "`snap` needs `radius`: how far the pointer is from the pin, in inches")) };
            let read = snap_params::read_for(s, page);
            let snapped = snap_params::run_turn(doc, read.as_deref(), page, *id, raw, radius, ask);
            told = snap_params::snapped_json(&snapped);
            Turn::To(snapped.turned.unwrap_or(raw))
        }
        (_, Some(_), _) => return Err(bad(ID, "`snap` turns to an `angle`, not by one")),
        (turn, None, _) => turn,
    };
    let about = about_of(p, ID)?;
    let mut out = s.edit_if("Rotate shapes", |doc| {
        let k = units_per_inch(doc, page);
        let turned = draw::turn_shapes(doc, page, &ids, turn, about.map(|a| (a.0 * k, a.1 * k))).map_err(|problem| problem.into_engine(ID))?;
        if turned.count > 0 {
            // A page with Auto Size on grows to take a turned shape in (`draw/page_fit.rs`), in the same undo step.
            draw::fit_page(doc, page, &ids).map_err(|problem| problem.into_engine(ID))?;
        }
        let mut out = json!({ "page": page, "rotated": turned.count });
        if let (Some(map), false) = (out.as_object_mut(), turned.locked.is_empty()) {
            map.insert("locked".into(), json!(turned.locked));
        }
        Ok((out, turned.count > 0))
    })?;
    if let (Some(map), false) = (out.as_object_mut(), told.is_null()) {
        map.insert("snap".into(), told);
    }
    Ok(out)
}

/// Flips shapes → `{page, flipped}`.
fn flip(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.flip";
    let ids = required_ids(p, "ids", ID)?;
    let axis = match required_text(p, "axis", ID)? {
        "horizontal" => FlipAxis::Horizontal,
        "vertical" => FlipAxis::Vertical,
        _ => return Err(bad(ID, "`axis` must be `horizontal` or `vertical`")),
    };
    let page = target_page(s, p, &ids, ID)?;
    s.edit_if("Flip shapes", |doc| {
        let flipped = draw::flip_shapes(doc, page, &ids, axis).map_err(|problem| problem.into_engine(ID))?;
        if flipped > 0 {
            draw::fit_page(doc, page, &ids).map_err(|problem| problem.into_engine(ID))?;
        }
        Ok((json!({ "page": page, "flipped": flipped }), flipped > 0))
    })
}

/// Changes the drawing order → `{page, moved}`: how many of the shapes named changed place. An order that does not change is no undo step.
fn order(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.order";
    let ids = required_ids(p, "ids", ID)?;
    let (to, label) = match required_text(p, "to", ID)? {
        "front" => (Stack::Front, "Bring to front"),
        "back" => (Stack::Back, "Send to back"),
        "forward" => (Stack::Forward, "Bring forward"),
        "backward" => (Stack::Backward, "Send backward"),
        _ => return Err(bad(ID, "`to` must be `front`, `back`, `forward` or `backward`")),
    };
    let page = target_page(s, p, &ids, ID)?;
    s.edit_if(label, |doc| {
        let moved = draw::order_shapes(doc, page, &ids, to).map_err(|problem| problem.into_engine(ID))?;
        Ok((json!({ "page": page, "moved": moved }), moved > 0))
    })
}

/// Aligns shapes to the first one named → `{page, moved}`: how many moved.
fn align(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.align";
    let ids = required_ids(p, "ids", ID)?;
    let how = match required_text(p, "how", ID)? {
        "left" => Align::Left,
        "center" => Align::Center,
        "right" => Align::Right,
        "top" => Align::Top,
        "middle" => Align::Middle,
        "bottom" => Align::Bottom,
        _ => return Err(bad(ID, "`how` must be `left`, `center`, `right`, `top`, `middle` or `bottom`")),
    };
    let page = target_page(s, p, &ids, ID)?;
    s.edit_if("Align shapes", |doc| {
        let moved = draw::align_shapes(doc, page, &ids, how).map_err(|problem| problem.into_engine(ID))?;
        if moved > 0 {
            draw::fit_page(doc, page, &ids).map_err(|problem| problem.into_engine(ID))?;
        }
        Ok((json!({ "page": page, "moved": moved }), moved > 0))
    })
}
