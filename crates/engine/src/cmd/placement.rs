//! Size & Position: the commands the window's fields send beyond `shape.move`, `shape.rotate` and `connector.reconnect`: the pin
//! position of shapes, one size for several shapes, a line's ends and length, and a query for what the fields show. The rules are in
//! `draw/placement.rs`, `draw/line_ends.rs` and `docs/editing-commands.md`, and
//! `shape.place` (`draw/place.rs`): the X and Y fields, an absolute place for the pins of many shapes at once.
//!
//! Lengths are paper inches on the page, as the other editing commands have them, and angles are degrees counterclockwise; the cells hold the page's
//! drawing units and radians, and the command turns them at its edge. `units_per_inch` in the query's answer is the factor between the two.

use serde_json::{Map, Value, json};

use super::params::{optional_number, optional_whole, required_ids, required_number, units_per_inch};
use super::shape::{add_snap, hold};
use super::snap_params::{self, Ask};
use super::{CommandSpec, has_document, required_text, required_u32};
use crate::Session;
use crate::draw::{self, LineEnd, PinAt, Place, ShapePlacement, Which};
use crate::error::{Result, bad, failed, shown};
use crate::snap::{MAX_OWN_POINTS, OwnFrame, Rect, Snapped, Subject, Use};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "shape.place",
            "Set Position",
            ["Home", "Arrange"],
            None,
            "ids: array of whole numbers, the shapes whose pin goes to the place (a member of a group is placed on the page, not in its group's frame), up to 4096 of them; \
             x: number, optional, inches from the page's left edge to the pin; \
             y: number, optional, inches from the page's bottom edge to the pin (give x, y or both, and an axis left out does not move); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes); \
             snap: true or {zoom, view}, optional, snap the box of the shapes where they land by the document's Snap & Glue settings (at most 512 shapes, see shape.drop), \
             and the pull is added to the place of every shape",
            has_document,
            place
        ),
        cmd!(
            "shape.pin",
            "Set Pin Position",
            ["Home", "Arrange"],
            None,
            "ids: array of whole numbers, the shapes whose pin moves; \
             at: text or object, where the pin goes in the shape's own frame, before its turn and flips: one of top-left, top-center, top-right, middle-left, center, \
             middle-right, bottom-left, bottom-center or bottom-right (held as a formula of the size, so the pin keeps its place on the box when the shape is resized), \
             or {x, y} in inches from the corner at (0, 0) of the shape's own frame (held as a value), and the shape stays where it is on the page while its pin position follows; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            pin
        ),
        cmd!(
            "shape.size",
            "Set Size",
            ["Home", "Arrange"],
            None,
            "ids: array of whole numbers, the shapes that are given the size, each keeping its pin; \
             width: number, optional, more than 0, inches along each shape's own x axis; \
             height: number, optional, more than 0, inches along each shape's own y axis (give width, height or both); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            size
        ),
        cmd!(
            "shape.set_end",
            "Set Line End",
            ["Home", "Arrange"],
            None,
            "id: whole number, the line or connector; end: text, begin or end; \
             x: number, optional, inches from the page's left edge to the place the end goes; \
             y: number, optional, inches from the page's bottom edge (give x, y or both: a coordinate left out keeps its place); \
             page: whole number, optional, the id of the page (default: the first page with the shape)",
            has_document,
            set_end
        ),
        cmd!(
            "shape.set_length",
            "Set Line Length",
            ["Home", "Arrange"],
            None,
            "id: whole number, the line or connector; length: number, more than 0, inches between its ends, which keep their direction: the end that is not glued moves; \
             page: whole number, optional, the id of the page (default: the first page with the shape)",
            has_document,
            set_length
        ),
        cmd!(
            query "shape.placement",
            "Show Size and Position",
            ["Home", "Arrange"],
            None,
            "ids: array of whole numbers, the shapes to describe; page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            placement
        ),
    ]
}

/// The page the command works on: the one named, or the first that holds every shape named (one walk of a page for all of them).
fn target_page(s: &Session, p: &Value, ids: &[u32], cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    draw::page_holding(doc, named, ids).ok_or_else(|| bad(cmd, "the document has no pages"))
}

/// The drawing units to a paper inch on a page.
fn scale(s: &Session, page: u32) -> f64 {
    s.document().map_or(1.0, |doc| units_per_inch(doc, page))
}

/// Where the pin goes: a place by its name, or `{x, y}` (inches, turned into drawing units by `k`).
fn pin_at(p: &Value, cmd: &str, k: f64) -> Result<PinAt> {
    let names = Place::ALL.iter().map(|place| place.name()).collect::<Vec<_>>().join(", ");
    match p.get("at") {
        None | Some(Value::Null) => Err(bad(cmd, "missing `at`")),
        Some(Value::String(name)) => Place::parse(name)
            .map(PinAt::Place)
            .ok_or_else(|| bad(cmd, format!("`at` is `{}`, which is not a place of the box: {names}, or {{x, y}}", shown(name)))),
        Some(Value::Object(map)) => {
            if let Some(unknown) = map.keys().find(|key| !matches!(key.as_str(), "x" | "y")) {
                return Err(bad(cmd, format!("`at` has an unknown key `{}`; it takes x and y", shown(unknown))));
            }
            let point = Value::Object(map.clone());
            Ok(PinAt::Local((required_number(&point, "x", cmd)? * k, required_number(&point, "y", cmd)? * k)))
        }
        Some(_) => Err(bad(cmd, format!("`at` must be a place of the box ({names}) or an object {{x, y}}"))),
    }
}

/// Puts the pin of each shape named at `x` and/or `y` on the page → `{page, moved, unplaced}`: how many shapes moved, and how many were
/// left because they have no place to move from or to (their pin is not a finite number, or a cell would be written past the page's reach). A shape
/// whose pin is there already, or that has nothing of its own to move, is not counted, and when none moves the call is no undo step. With `snap` the
/// answer also holds `snap`, as `shape.move`'s does.
fn place(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.place";
    let ids = required_ids(p, "ids", ID)?;
    let (x, y) = (optional_number(p, "x", ID)?, optional_number(p, "y", ID)?);
    if x.is_none() && y.is_none() {
        return Err(bad(ID, "give `x`, `y` or both"));
    }
    let page = target_page(s, p, &ids, ID)?;
    let k = scale(s, page);
    let snapped = match (snap_params::parse(p, ID, snap_params::Words::PLACE)?, s.document()) {
        (Some(ask), Some(doc)) => {
            if ids.len() > snap_params::MAX_SNAPPED_SHAPES {
                return Err(bad(ID, format!("`snap` takes at most {} shapes", snap_params::MAX_SNAPPED_SHAPES)));
            }
            // The page as the session kept it, else read here, as before.
            let read = snap_params::read_for(s, page);
            snap_place(doc, read.as_deref(), page, &ids, (x, y), ask)
        }
        _ => None,
    };
    // The pull is on the axes given: an axis left out does not move, snapped or not.
    let (x, y) = snapped.as_ref().map_or((x, y), |sn| (x.map(|v| v + sn.dx), y.map(|v| v + sn.dy)));
    let (mut out, held) = s.edit_if("Set position", |doc| {
        let placed = draw::place_shapes(doc, page, &ids, x.map(|v| v * k), y.map(|v| v * k)).map_err(|problem| problem.into_engine(ID))?;
        if placed.moved > 0 {
            draw::fit_page(doc, page, &ids).map_err(|problem| problem.into_engine(ID))?;
        }
        // The shapes whose own edge the snap put on a guide are held to it; an axis left out does not move, and is held only where the
        // shape's edge is on the guide already.
        let mut held = snapped.map(|sn| hold(doc, page, &ids, sn)).transpose().map_err(|problem| problem.into_engine(ID))?;
        let changed = placed.moved > 0 || held.as_ref().is_some_and(super::shape::Held::wrote);
        if let Some(h) = held.as_mut() {
            h.changed = changed;
        }
        Ok(((json!({ "page": page, "moved": placed.moved, "unplaced": placed.unplaced }), held), changed))
    })?;
    add_snap(&mut out, held.as_ref());
    Ok(out)
}

/// The snap of a place: the box that holds the shapes where they would land (each shape's box carried by the distance its pin goes), pulled to the
/// page's targets as `shape.move` pulls the box it moves. **A shape's box is the one `shape.move` snaps** (`snap::bounds_each`: the box round the paths of a turned shape),
/// made for all the shapes under one budget. `None` where the shapes are not all on the page (the edit then says so), or the snap has no box.
fn snap_place(
    doc: &diacraft_doc::Document,
    read: Option<&crate::snap::SnapPage>,
    page: u32,
    ids: &[u32],
    (x, y): (Option<f64>, Option<f64>),
    ask: Ask,
) -> Option<Snapped> {
    let k = units_per_inch(doc, page);
    let here = draw::placements(doc, page, ids).ok()?;
    let shapes: Vec<u32> = here.iter().map(|shape| shape.id).collect();
    let boxes = snap_params::bounds_each(doc, page, &shapes)?;
    let mut corners = Vec::with_capacity(here.len().saturating_mul(4));
    let mut pulled: Vec<(u32, crate::snap::P)> = Vec::new();
    for (n, (shape, r)) in here.iter().zip(&boxes.rects).enumerate() {
        // Where the pin goes, in paper inches (the distance is 0 on an axis that is not asked for).
        let by = (x.map_or(0.0, |x| x - shape.pin.0 / k), y.map_or(0.0, |y| y - shape.pin.1 / k));
        corners.extend([(r.x0 + by.0, r.y0 + by.1), (r.x1 + by.0, r.y1 + by.1)]);
        if n < snap_params::MAX_OWN_SHAPES {
            pulled.push((shape.id, by));
        }
    }
    // The connection points of the first shapes, each moved by its own distance, as each shape's own call gave them (found in one walk of the page).
    let mut own = snap_params::own_points_each(doc, page, &pulled);
    own.truncate(MAX_OWN_POINTS);
    let placed: Rect = Rect::round(&corners)?;
    // One turned shape lines up by its own sides as well as by the box round its paths, as `shape.move`'s does: its frame goes where its pin goes.
    let frame = match (here.as_slice(), snap_params::frame_of_shapes(doc, page, &shapes)) {
        ([shape], Some(f)) => {
            let by = (x.map_or(0.0, |x| x - shape.pin.0 / k), y.map_or(0.0, |y| y - shape.pin.1 / k));
            Some(OwnFrame { centre: (f.centre.0 + by.0, f.centre.1 + by.1), ..f })
        }
        _ => None,
    };
    let mut snapped = snap_params::run_with(doc, read, Subject::Box(placed), Use::Place, ids, &own, Ask { frame, ..ask });
    // The shapes that kept the box round their corners where the box round their paths was meant are counted in the result, as `shape.move`'s are.
    snapped.corner_boxes = boxes.by_corners;
    Some(snapped)
}

/// Puts the pin of shapes at a place → `{page, pinned}`: how many shapes changed. A pin that is at the place already is not counted, and when none is
/// the call is no undo step.
fn pin(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.pin";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let at = pin_at(p, ID, scale(s, page))?;
    // The shapes draw where they did, so the page is not asked to grow (Auto Size is for a shape put or moved past the edge).
    s.edit_if("Set pin position", |doc| {
        let pinned = draw::pin_shapes(doc, page, &ids, at).map_err(|problem| problem.into_engine(ID))?;
        Ok((json!({ "page": page, "pinned": pinned }), pinned > 0))
    })
}

/// Gives shapes one size, each keeping its pin → `{page, sized}`: how many shapes changed.
fn size(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.size";
    let ids = required_ids(p, "ids", ID)?;
    let (width, height) = (optional_number(p, "width", ID)?, optional_number(p, "height", ID)?);
    let page = target_page(s, p, &ids, ID)?;
    let k = scale(s, page);
    s.edit_if("Set size", |doc| {
        let sized = draw::size_shapes(doc, page, &ids, width.map(|w| w * k), height.map(|h| h * k)).map_err(|problem| problem.into_engine(ID))?;
        if sized > 0 {
            // A page with Auto Size on grows to take in a shape that is bigger (`draw/page_fit.rs`), in the same undo step.
            draw::fit_page(doc, page, &ids).map_err(|problem| problem.into_engine(ID))?;
        }
        Ok((json!({ "page": page, "sized": sized }), sized > 0))
    })
}

/// Puts one end of a line at a place → `{page, id, changed}`.
fn set_end(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.set_end";
    let id = required_u32(p, "id", ID)?;
    let which = match required_text(p, "end", ID)? {
        "begin" => Which::Begin,
        "end" => Which::End,
        _ => return Err(bad(ID, "`end` must be `begin` or `end`")),
    };
    let (x, y) = (optional_number(p, "x", ID)?, optional_number(p, "y", ID)?);
    let page = target_page(s, p, &[id], ID)?;
    let k = scale(s, page);
    s.edit_if("Set line end", |doc| {
        let changed = draw::set_end(doc, page, id, which, x.map(|v| v * k), y.map(|v| v * k)).map_err(|problem| problem.into_engine(ID))?;
        if changed {
            draw::fit_page(doc, page, &[id]).map_err(|problem| problem.into_engine(ID))?;
        }
        Ok((json!({ "page": page, "id": id, "changed": changed }), changed))
    })
}

/// Gives a line a length → `{page, id, changed}`.
fn set_length(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.set_length";
    let id = required_u32(p, "id", ID)?;
    let length = required_number(p, "length", ID)?;
    let page = target_page(s, p, &[id], ID)?;
    let k = scale(s, page);
    s.edit_if("Set line length", |doc| {
        let changed = draw::set_length(doc, page, id, length * k).map_err(|problem| problem.into_engine(ID))?;
        if changed {
            draw::fit_page(doc, page, &[id]).map_err(|problem| problem.into_engine(ID))?;
        }
        Ok((json!({ "page": page, "id": id, "changed": changed }), changed))
    })
}

/// An end of a line as the answer has it: `{x, y, glued}`, `glued` being `{id, point}` (`point` `null` for glue to the whole of the shape) or `null`.
fn end_json(end: &LineEnd, k: f64) -> Value {
    let glued = end.glued.map_or(Value::Null, |g| json!({ "id": g.id, "point": g.point }));
    json!({ "x": end.at.0 / k, "y": end.at.1 / k, "glued": glued })
}

fn placement_json(p: &ShapePlacement, k: f64) -> Value {
    let mut out = Map::new();
    out.insert("id".into(), json!(p.id));
    out.insert("name".into(), json!(p.name));
    out.insert("one_d".into(), json!(p.one_d));
    out.insert("group".into(), json!(p.group));
    out.insert("x".into(), json!(p.pin.0 / k));
    out.insert("y".into(), json!(p.pin.1 / k));
    out.insert("width".into(), json!(p.width / k));
    out.insert("height".into(), json!(p.height / k));
    out.insert("angle".into(), json!(p.angle));
    out.insert("pin".into(), json!({ "x": p.local_pin.0 / k, "y": p.local_pin.1 / k }));
    out.insert("pin_at".into(), if p.one_d { Value::Null } else { json!(p.place.map_or("custom", Place::name)) });
    out.insert("flip_x".into(), json!(p.flip_x));
    out.insert("flip_y".into(), json!(p.flip_y));
    if let Some(line) = &p.line {
        out.insert("begin".into(), end_json(&line.begin, k));
        out.insert("end".into(), end_json(&line.end, k));
        out.insert("length".into(), json!(line.length / k));
    }
    Value::Object(out)
}

/// What the Size & Position window shows of shapes → `{page, units_per_inch, shapes: [..]}`, one entry for each shape named, in the order named: see
/// in `docs/editing-commands.md`.
fn placement(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.placement";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let k = units_per_inch(doc, page);
    let shapes = draw::placements(doc, page, &ids).map_err(|problem| problem.into_engine(ID))?;
    Ok(json!({ "page": page, "units_per_inch": k, "shapes": shapes.iter().map(|shape| placement_json(shape, k)).collect::<Vec<_>>() }))
}
