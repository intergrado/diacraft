//! The drawing tools' side of the engine: `shape.draw` draws a shape with the Rectangle, Ellipse, Line, Freeform, Arc or Pencil tool from the points of a drag, or carries
//! a shape's path on from its end vertex, and `shape.close` closes an open path. The rules are named in `draw/pen`, `stencil/drawn.rs` and `docs/editing-commands.md`.
//!
//! Lengths are paper inches on the page, as the other editing commands have them; the cells hold the page's drawing units, and the command turns them at its edge.

use serde_json::{Value, json};

use super::params::{optional_number, optional_whole, page_for, units_per_inch};
use super::{CommandSpec, has_document, required_text, required_u32};
use crate::Session;
use crate::draw::pen::edit::{self, Ends};
use crate::draw::pen::{self, DEFAULT_SMOOTHING, DEFAULT_TOLERANCE, MAX_STROKE_POINTS, P, Settings, Tool};
use crate::draw::{self, Problem};
use crate::error::{Result, bad, failed};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "shape.draw",
            "Draw a Shape",
            ["Home", "Tools", "Drawing Tools"],
            None,
            "page: whole number, the id of the page; \
             tool: text, rectangle, ellipse, line, freeform, arc or pencil; \
             points: array of [x, y] pairs, inches from the page's left and bottom edges, at most 5000 of them: the rectangle, ellipse, line and arc tools read the first and the last (the two \
             ends of a drag: a rectangle's and an ellipse's opposite corners, a line's and an arc's ends), the pencil and the freeform tool read the whole stroke; \
             shift: true or false, optional (default false), Shift held: a square, a circle, or a straight segment from the first point held to a step of 45 degrees; \
             tolerance: number, optional, 0.0001 to 10 inches (default 0.03125), the most the lines and arcs of the pencil, or the curve of the freeform tool, may stand from the stroke; \
             smoothing: number, optional, 0 to 1 (default 0.4), how much the freeform stroke is smoothed before the curve is fitted; \
             continue: whole number, optional, the id of a shape on the page whose open path goes on from its end vertex (the line, arc, freeform and pencil tools): the first point is \
             taken to be that vertex, and the shape's box grows to the new path; \
             close: true or false, optional (default false), the last point is put on the first vertex of the path (the stroke's own with the pencil and freeform tools, the shape's with \
             continue) and the path is closed and takes the fill",
            has_document,
            draw_command
        ),
        cmd!(
            "shape.close",
            "Close Shape",
            ["Draw", "Create"],
            None,
            "id: whole number, the shape whose open path is closed with a straight segment from its end vertex to its first, so that it takes the fill; \
             page: whole number, optional, the id of the page (default: the first page with the shape)",
            has_document,
            close_command
        ),
    ]
}

/// The drawing units to a paper inch on a page.
fn scale(s: &Session, page: u32) -> f64 {
    s.document().map_or(1.0, |doc| units_per_inch(doc, page))
}

/// A true-or-false parameter: `None` where it is left out or null.
fn flag(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// The stroke `p` gives as `points`: an array of `[x, y]` pairs of numbers, at most [`MAX_STROKE_POINTS`] (counted before any is read).
fn points_of(p: &Value, cmd: &str) -> Result<Vec<P>> {
    let Some(Value::Array(items)) = p.get("points") else {
        return Err(bad(cmd, "`points` must be an array of [x, y] pairs, inches from the page's left and bottom edges"));
    };
    if items.len() > MAX_STROKE_POINTS {
        return Err(bad(cmd, format!("`points` holds at most {MAX_STROKE_POINTS} points, and this has {}", items.len())));
    }
    items
        .iter()
        .map(|item| match item {
            Value::Array(pair) => match pair.as_slice() {
                [Value::Number(x), Value::Number(y)] => match (x.as_f64(), y.as_f64()) {
                    (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Ok((x, y)),
                    _ => Err(bad(cmd, "each point of `points` must be a pair of finite numbers")),
                },
                _ => Err(bad(cmd, "each point of `points` must be a pair of numbers: [x, y]")),
            },
            _ => Err(bad(cmd, "each point of `points` must be a pair of numbers: [x, y]")),
        })
        .collect()
}

/// The first point of the stroke made `at`, and with `to` the last one.
fn tied(points: &mut [P], at: Option<P>, to: Option<P>) {
    if let (Some(at), Some(first)) = (at, points.first_mut()) {
        *first = at;
    }
    if let (Some(to), Some(last)) = (to, points.last_mut()) {
        *last = to;
    }
}

/// Draws a shape, or carries a path on → `{page, id, tool, rows, closed, continued, width, height}`: the size of the shape's box in inches.
fn draw_command(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.draw";
    let page = required_u32(p, "page", ID)?;
    let word = required_text(p, "tool", ID)?;
    let tool = Tool::from_name(word).ok_or_else(|| {
        bad(ID, format!("no tool `{}`; the tools are rectangle, ellipse, line, freeform, arc and pencil", crate::error::shown(word)))
    })?;
    let points = points_of(p, ID)?;
    let settings = Settings {
        shift: flag(p, "shift", ID)?.unwrap_or(false),
        tolerance: optional_number(p, "tolerance", ID)?.unwrap_or(DEFAULT_TOLERANCE),
        smoothing: optional_number(p, "smoothing", ID)?.unwrap_or(DEFAULT_SMOOTHING),
    };
    let close = flag(p, "close", ID)?.unwrap_or(false);
    let continuing = optional_whole(p, "continue", ID)?;
    let k = scale(s, page);
    let fit = |points: &[P]| pen::draw(tool, points, &settings).and_then(|drawn| drawn.scaled(k));
    // The id is above every one the session has given on the page and the file it saved from holds (`Session::id_floor`).
    let above = s.id_floor(page);
    let (made, continued) = match continuing {
        None => {
            let mut points = points;
            if close {
                if !matches!(tool, Tool::Pencil | Tool::Freeform) {
                    return Err(bad(
                        ID,
                        "`close` closes the stroke of the pencil or the freeform tool; the other tools close a path with `continue`",
                    ));
                }
                let first = points.first().copied();
                tied(&mut points, None, first);
            }
            let drawn = fit(&points).map_err(|problem| problem.into_engine(ID))?;
            let made = s.edit(tool.label(), |doc| {
                let made = edit::add_drawn_shape_above(doc, page, tool, &drawn, above).map_err(|problem| problem.into_engine(ID))?;
                // A page that has a theme gives it to the new shape, in the same undo step (`theme/apply.rs`).
                crate::theme::follow_page_theme(doc, page, made.id).map_err(|problem| problem.into_engine(ID))?;
                crate::draw::layers::join_active(doc, page, made.id).map_err(|problem| problem.into_engine(ID))?;
                // A page with Auto Size on grows to take the shape in (`draw/page_fit.rs`), in the same undo step.
                draw::fit_page(doc, page, &[made.id]).map_err(|problem| problem.into_engine(ID))?;
                Ok(made)
            })?;
            s.note_id(page, made.id);
            (made, false)
        }
        Some(id) => {
            if !tool.goes_on() {
                return Err(bad(
                    ID,
                    format!("`continue` goes on from the end of a path with the line, arc, freeform or pencil tool, not the {}", tool.name()),
                ));
            }
            let made = s.edit(tool.label(), |doc| {
                let made = edit::continue_shape(doc, page, id, close, |ends: Ends| {
                    // The stroke is tied to the end of the path, and with `close` to its start: both are in drawing units, the stroke in inches.
                    let inches = |q: P| (q.0 / k, q.1 / k);
                    let mut points = points;
                    tied(&mut points, Some(inches(ends.end)), close.then(|| inches(ends.start)));
                    let drawn = fit(&points)?;
                    // The rows without the move to the first point: the path is already there.
                    Ok(drawn.rows.into_iter().skip(1).collect())
                })
                .map_err(|problem| problem.into_engine(ID))?;
                draw::fit_page(doc, page, &[id]).map_err(|problem| problem.into_engine(ID))?;
                Ok(made)
            })?;
            (made, true)
        }
    };
    Ok(json!({
        "page": page,
        "id": made.id,
        "tool": tool.name(),
        "rows": made.rows,
        "closed": made.closed,
        "continued": continued,
        "width": made.width / k,
        "height": made.height / k,
    }))
}

/// Closes the open path of a shape → `{page, id, rows, closed, width, height}`.
fn close_command(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.close";
    let id = required_u32(p, "id", ID)?;
    let named = optional_whole(p, "page", ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let page = page_for(doc, named, &[id], ID)?;
    let k = scale(s, page);
    let made = s.edit("Close shape", |doc| {
        let made = edit::close_shape(doc, page, id).map_err(|problem: Problem| problem.into_engine(ID))?;
        draw::fit_page(doc, page, &[id]).map_err(|problem| problem.into_engine(ID))?;
        Ok(made)
    })?;
    Ok(json!({ "page": page, "id": id, "rows": made.rows, "closed": made.closed, "width": made.width / k, "height": made.height / k }))
}
