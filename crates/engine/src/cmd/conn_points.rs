//! `shape.connection_edges`: the edges of a shape's outline, the connection points on each, and what a doubling of each would make under the minimum
//! distance between points. A query: it changes nothing and is no undo step. The rules are in `draw/conn_points` and
//! `docs/editing-commands.md`, "Connection points: double and halve (CPDOUBLE)".
//!
//! Lengths are paper inches on the page, as every `shape.*` command takes them; `units_per_inch` in the answer is the factor to the page's drawing units.

use serde_json::{Map, Value, json};

use super::params::{optional_number, optional_whole, page_for, required_ids};
use super::{CommandSpec, has_document, optional_text};
use crate::Session;
use crate::draw::conn_points::{
    Action, Answer, DEFAULT_MIN_GAP, EdgeChange, EdgeInfo, EdgeKind, EdgeSkipped, EdgesAnswer, EndReglued, MAX_MIN_GAP, MIN_MIN_GAP, Request,
    ShapeChange, ShapeEdges, change_points, query_edges,
};
use crate::error::{Result, bad, failed, shown};

/// The reach of an `at` that gives none: five screen points at full size, in inches (5/72).
const DEFAULT_REACH: f64 = 5.0 / 72.0;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "shape.connection_edges",
            "Show Connection Point Edges",
            [],
            None,
            "id: whole number, the shape (give id or ids, not both); \
             ids: array of whole numbers, at most 4096, the shapes to describe, an ID named twice told once; \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes); \
             min_gap: number, optional, paper inches, the least distance between points that a doubling keeps, from 0.01 to 2 (default 3 mm, 0.11811)",
            has_document,
            edges
        ),
        cmd!(
            "shape.connection_points",
            "Double or Halve Connection Points",
            ["Home", "Tools"],
            None,
            "id: whole number, the shape (give id or ids, not both); \
             ids: array of whole numbers, at most 4096, the shapes to change in one undo step, an ID named twice counted once; \
             action: text, double (a new point at the middle of every portion of the edge, from one point to the next and from each end of an open edge to the point nearest it, every old point staying) \
             or halve (takes away the points of the newest doubling of the edge, never an original point); \
             edge: whole number, optional, with one shape only, the edge as shape.connection_edges numbers it (without edge or at every edge of every shape is changed); \
             at: object {x, y}, optional, a place on the page in inches, the edge of the shapes named nearest it within reach is the one changed and when none is that near every edge is, not with edge; \
             reach: number, optional, with at, inches, more than 0 (default 5/72, five screen points at full size); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes); \
             min_gap: number, optional, paper inches, the least distance between points a doubling keeps, from 0.01 to 2 (default 3 mm, 0.11811)",
            has_document,
            points
        ),
    ]
}

/// The shapes the call names: `id`, or `ids`, and not both.
fn named(p: &Value, cmd: &str) -> Result<Vec<u32>> {
    let given = |key: &str| p.get(key).is_some_and(|v| !v.is_null());
    match (given("id"), given("ids")) {
        (true, true) => Err(bad(cmd, "give `id` (one shape) or `ids` (several), not both")),
        (false, false) => Err(bad(cmd, "give `id` or `ids`")),
        (false, true) => required_ids(p, "ids", cmd),
        (true, false) => Ok(vec![optional_whole(p, "id", cmd)?.ok_or_else(|| bad(cmd, "missing `id`"))?]),
    }
}

/// The minimum distance the call asks for, in paper inches: a number from [`MIN_MIN_GAP`] to [`MAX_MIN_GAP`], else [`DEFAULT_MIN_GAP`].
pub(super) fn min_gap_of(p: &Value, cmd: &str) -> Result<f64> {
    match optional_number(p, "min_gap", cmd)? {
        None => Ok(DEFAULT_MIN_GAP),
        Some(gap) if (MIN_MIN_GAP..=MAX_MIN_GAP).contains(&gap) => Ok(gap),
        Some(_) => Err(bad(cmd, format!("`min_gap` must be a number of paper inches from {MIN_MIN_GAP} to {MAX_MIN_GAP}"))),
    }
}

/// A number for the answer: a length that is not a number (a hostile scale) is `null`, and never a panic.
fn num(x: f64) -> Value {
    if x.is_finite() { json!(x) } else { Value::Null }
}

fn place(p: (f64, f64)) -> Value {
    json!({ "x": num(p.0), "y": num(p.1) })
}

fn kind_word(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Straight => "straight",
        EdgeKind::Curved => "curved",
        EdgeKind::Closed => "closed",
    }
}

fn edge_json(edge: &EdgeInfo) -> Value {
    json!({
        "edge": edge.edge,
        "kind": kind_word(edge.kind),
        // The reference's own spelling of the section: its `IX` plus one. A section with no `IX` has none.
        "section": edge.section.map(|ix| format!("Geometry{}", u64::from(ix) + 1)),
        "rows": [edge.rows.0, edge.rows.1],
        "steps": edge.steps,
        "length": num(edge.length),
        "from": place(edge.from),
        "to": place(edge.to),
        "path": edge.path.iter().map(|p| place(*p)).collect::<Vec<_>>(),
        "points": edge.points,
        "added": edge.added.iter().map(|(point, generation)| json!({ "point": point, "generation": generation })).collect::<Vec<_>>(),
        "newest": edge.newest,
        "gap": num(edge.gap),
        "adds": edge.adds,
    })
}

fn shape_json(shape: &ShapeEdges) -> Value {
    json!({
        "id": shape.id,
        "refused": shape.refused,
        "edges": shape.edges.iter().map(edge_json).collect::<Vec<_>>(),
        "other": shape.other,
        "points": shape.points,
        "can_double": shape.can_double,
        "can_halve": shape.can_halve,
        "truncated": shape.truncated,
    })
}

fn answer_json(answer: &EdgesAnswer) -> Value {
    let mut out = Map::new();
    out.insert("page".into(), json!(answer.page));
    out.insert("min_gap".into(), num(answer.min_gap));
    out.insert("units_per_inch".into(), num(answer.units_per_inch));
    out.insert("scale_note".into(), json!(answer.scale_note));
    out.insert("shapes".into(), Value::Array(answer.shapes.iter().map(shape_json).collect()));
    Value::Object(out)
}

/// The edges of the shapes named and the points on each → `{page, min_gap, units_per_inch, scale_note, shapes: [..]}`: see in
/// `docs/editing-commands.md`.
fn edges(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.connection_edges";
    let ids = named(p, ID)?;
    let min_gap = min_gap_of(p, ID)?;
    let named_page = optional_whole(p, "page", ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let page = page_for(doc, named_page, &ids, ID)?;
    let answer = query_edges(doc, page, &ids, min_gap).map_err(|problem| problem.into_engine(ID))?;
    Ok(answer_json(&answer))
}

/// The action the call asks for. `set_count` was dropped and is refused in its own words.
fn action_of(p: &Value, cmd: &str) -> Result<Action> {
    match optional_text(p, "action", cmd)? {
        None => Err(bad(cmd, "missing `action`: double or halve")),
        Some("double") => Ok(Action::Double),
        Some("halve") => Ok(Action::Halve),
        Some("set_count") => Err(bad(cmd, "set_count was dropped: double or halve")),
        Some(other) => Err(bad(cmd, format!("`action` must be `double` or `halve`, not `{}`", shown(other)))),
    }
}

/// A place on the page, `{x, y}` in inches, and how far from it an edge may be: `at` with its `reach`.
fn at_of(p: &Value, cmd: &str) -> Result<Option<((f64, f64), f64)>> {
    let reach = optional_number(p, "reach", cmd)?;
    let Some(at) = p.get("at").filter(|v| !v.is_null()) else {
        return match reach {
            Some(_) => Err(bad(cmd, "`reach` goes with `at`: give a place for it to be the reach of")),
            None => Ok(None),
        };
    };
    let words = || bad(cmd, "`at` must be an object {x, y} of two numbers: a place on the page in inches");
    let Value::Object(map) = at else { return Err(words()) };
    if map.keys().any(|k| k != "x" && k != "y") {
        return Err(words());
    }
    let number = |key: &str| map.get(key).and_then(Value::as_f64).filter(|n| n.is_finite()).ok_or_else(words);
    let place = (number("x")?, number("y")?);
    let reach = reach.unwrap_or(DEFAULT_REACH);
    if reach <= 0.0 {
        return Err(bad(cmd, "`reach` must be a number of inches more than 0"));
    }
    Ok(Some((place, reach)))
}

/// The call's parameters as a request, each checked in words.
fn request_of(p: &Value, cmd: &str) -> Result<Request> {
    let ids = named(p, cmd)?;
    let action = action_of(p, cmd)?;
    let edge = optional_whole(p, "edge", cmd)?;
    let at = at_of(p, cmd)?;
    if edge.is_some() && at.is_some() {
        return Err(bad(cmd, "give `edge` (an edge by its number) or `at` (the edge nearest a place), not both"));
    }
    Ok(Request { ids, action, edge, at, min_gap: min_gap_of(p, cmd)? })
}

fn change_json(c: &ShapeChange) -> Value {
    json!({
        "id": c.id,
        "edges": c.edges.iter().map(edge_change_json).collect::<Vec<_>>(),
        "skipped_edges": c.skipped_edges.iter().map(skipped_edge_json).collect::<Vec<_>>(),
        "points_before": c.points_before,
        "points_after": c.points_after,
        "skipped": c.skipped,
    })
}

fn edge_change_json(e: &EdgeChange) -> Value {
    json!({ "edge": e.edge, "kind": kind_word(e.kind), "before": e.before, "after": e.after, "generation": e.generation })
}

fn skipped_edge_json(e: &EdgeSkipped) -> Value {
    json!({ "edge": e.edge, "gap": num(e.gap), "min_gap": num(e.min_gap) })
}

fn reglued_json(r: &EndReglued) -> Value {
    json!({ "connector": r.connector, "end": if r.begin { "begin" } else { "end" }, "shape": r.shape, "from": r.from, "to": r.to })
}

fn points_json(answer: &Answer) -> Value {
    json!({
        "page": answer.page,
        "action": match answer.action { Action::Double => "double", Action::Halve => "halve" },
        "changed": answer.changed,
        "min_gap": num(answer.min_gap),
        "scale_note": answer.scale_note,
        "shapes": answer.shapes.iter().map(change_json).collect::<Vec<_>>(),
        "reglued": answer.reglued.iter().map(reglued_json).collect::<Vec<_>>(),
        "renumbered": answer.renumbered,
    })
}

/// Doubles or halves the connection points of the shapes → `{page, action, changed, min_gap, scale_note, shapes: [{id, edges: [{edge, kind, before, after, generation}],
/// skipped_edges: [{edge, gap, min_gap}], points_before, points_after, skipped}], reglued: [{connector, end, shape, from, to}], renumbered}`: see in
/// `docs/editing-commands.md`. One undo step, labelled "Double connection points" or "Halve connection points"; a call that changes nothing is refused in words.
fn points(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.connection_points";
    let request = request_of(p, ID)?;
    let named_page = optional_whole(p, "page", ID)?;
    let page = {
        let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
        page_for(doc, named_page, &request.ids, ID)?
    };
    let label = match request.action {
        Action::Double => "Double connection points",
        Action::Halve => "Halve connection points",
    };
    let answer = s.edit_if(label, |doc| {
        let answer = change_points(doc, page, &request).map_err(|problem| problem.into_engine(ID))?;
        let changed = answer.changed;
        Ok((answer, changed))
    })?;
    Ok(points_json(&answer))
}
