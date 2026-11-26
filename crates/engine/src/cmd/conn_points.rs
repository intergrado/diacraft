//! `shape.connection_edges`: the edges of a shape's outline, the connection points on each, and what a doubling of each would make under the minimum
//! distance between points. A query: it changes nothing and is no undo step. The rules are in `draw/conn_points` and
//! `docs/editing-commands.md`, "Connection points: double and halve (CPDOUBLE)".
//!
//! Lengths are paper inches on the page, as every `shape.*` command takes them; `units_per_inch` in the answer is the factor to the page's drawing units.

use serde_json::{Map, Value, json};

use super::params::{optional_number, optional_whole, page_for, required_ids};
use super::{CommandSpec, has_document};
use crate::Session;
use crate::draw::conn_points::{DEFAULT_MIN_GAP, EdgeInfo, EdgeKind, EdgesAnswer, MAX_MIN_GAP, MIN_MIN_GAP, ShapeEdges, query_edges};
use crate::error::{Result, bad, failed};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
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
    )]
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
