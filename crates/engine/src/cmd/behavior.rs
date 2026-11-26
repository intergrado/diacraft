//! The Behavior dialog's Placement tab: `shape.behavior` says how a shape is laid out and whether a route goes round it, and `shape.set_behavior` sets the tab's boxes.
//! The rules are in `draw/behavior.rs` and `draw/obstacle.rs`.

use serde_json::{Value, json};

use super::params::{optional_whole, page_for, required_ids};
use super::{CommandSpec, has_document, optional_text};
use crate::Session;
use crate::draw::{self, Behavior, BehaviorChange, PlacementBehavior, Plow};
use crate::error::{Result, bad, failed, shown};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "shape.behavior",
            "Show Shape Behavior",
            ["Developer", "Shape Design"],
            None,
            "ids: array of whole numbers, the shapes to describe, up to 4096; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            behavior
        ),
        cmd!(
            "shape.set_behavior",
            "Behavior",
            ["Developer", "Shape Design"],
            None,
            "ids: array of whole numbers, the two-dimensional shapes to set, up to 4096 (a connector, a line or a guide is refused); \
             placement: text, optional, auto (let the application decide: placeable once a connector is glued to the shape), route_around (always placeable) or none (neither: a route does not go round it); \
             route_through_x: true or false, optional, a level leg of a route may go through the shape; \
             route_through_y: true or false, optional, an upright leg of a route may go through the shape; \
             do_not_move: true or false, optional, a layout leaves the shape where it is; \
             allow_on_top: true or false, optional, a layout may put placeable shapes on top of the shape; \
             plow: text, optional, as_page, never or always: whether dropping the shape moves other shapes away; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            set_behavior
        ),
    ]
}

fn target_page(s: &Session, p: &Value, ids: &[u32], cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    page_for(doc, named, ids, cmd)
}

fn optional_flag(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

fn behavior_json(b: &Behavior) -> Value {
    json!({
        "id": b.id,
        "placement": b.placement.word(),
        "obj_type": b.obj_type,
        "route_through_x": b.route_through_x,
        "route_through_y": b.route_through_y,
        "do_not_move": b.do_not_move,
        "allow_on_top": b.allow_on_top,
        "plow": b.plow.word(),
        "obstacle": b.why_not.is_none(),
        "why_not": b.why_not,
    })
}

/// Describes shapes → `{page, shapes: [{id, placement, obj_type, route_through_x, route_through_y, do_not_move, allow_on_top, plow, obstacle, why_not}]}`: the Placement tab's values,
/// whether a route goes round the shape now and, when it does not, why (`why_not`, else null).
fn behavior(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.behavior";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let shapes = draw::shape_behavior(doc, page, &ids).map_err(|e| e.into_engine(ID))?;
    Ok(json!({ "page": page, "shapes": shapes.iter().map(behavior_json).collect::<Vec<_>>() }))
}

/// Sets the Placement tab's boxes → `{page, changed}`: how many shapes were written. It routes nothing.
fn set_behavior(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.set_behavior";
    let ids = required_ids(p, "ids", ID)?;
    let placement = match optional_text(p, "placement", ID)? {
        None => None,
        Some(word) => Some(
            PlacementBehavior::from_word(word)
                .ok_or_else(|| bad(ID, format!("`placement` must be auto, route_around or none, not `{}`", shown(word))))?,
        ),
    };
    let plow = match optional_text(p, "plow", ID)? {
        None => None,
        Some(word) => Some(Plow::from_word(word).ok_or_else(|| bad(ID, format!("`plow` must be as_page, never or always, not `{}`", shown(word))))?),
    };
    let change = BehaviorChange {
        placement,
        route_through_x: optional_flag(p, "route_through_x", ID)?,
        route_through_y: optional_flag(p, "route_through_y", ID)?,
        do_not_move: optional_flag(p, "do_not_move", ID)?,
        allow_on_top: optional_flag(p, "allow_on_top", ID)?,
        plow,
    };
    if change.is_empty() {
        return Err(bad(ID, "give at least one of placement, route_through_x, route_through_y, do_not_move, allow_on_top and plow"));
    }
    let page = target_page(s, p, &ids, ID)?;
    s.edit_if("Shape behavior", |doc| {
        let changed = draw::set_behavior(doc, page, &ids, &change).map_err(|e| e.into_engine(ID))?;
        Ok((json!({ "page": page, "changed": changed }), changed > 0))
    })
}
