//! The tools that read the two ends of a drag: Rectangle, Ellipse, Line and Arc, and what Shift does to them and to a stroke.

use kurbo::{Point, Rect};

use super::{Drawn, LEAST_SIDE, P, Tool, bounds};
use crate::draw::Problem;
use crate::draw::shapeops::CRow;

/// The step Shift holds a line, an arc's chord or a stroke to, in degrees.
const STEP_DEGREES: f64 = 45.0;

/// How far the middle of the Arc tool's arc stands from the middle of its chord, as a share of the chord: a quarter of a circle (`tan(22.5 degrees) / 2`: the sweep of an arc is
/// `4 atan(2 bow / chord)`). Ours: no specification gives the Arc tool's curve.
pub(super) const ARC_BOW: f64 = 0.207_106_781_186_547_5;

/// The corner a Shift-drag from `start` to `now` makes a square (or a circle's box) with: the side is the longer of the two the pointer went, and the corner is on the side of the
/// start the pointer is on, a pointer level with the start counting as the right and above.
pub fn square_corner(start: P, now: P) -> P {
    let (dx, dy) = (now.0 - start.0, now.1 - start.1);
    let side = dx.abs().max(dy.abs());
    (start.0 + if dx < 0.0 { -side } else { side }, start.1 + if dy < 0.0 { -side } else { side })
}

/// Where a Shift-drag from `start` to `now` ends when it is held to a step of `step` degrees: the pointer's direction turned to the nearest step, and the length the pointer went
/// along it. The start itself for a pointer on the start; `now` itself for a step that is not a number above 0.
pub fn step_end(start: P, now: P, step: f64) -> P {
    if !(step.is_finite() && step > 0.0) {
        return now;
    }
    let (dx, dy) = (now.0 - start.0, now.1 - start.1);
    let length = dx.hypot(dy);
    if length == 0.0 || !length.is_finite() {
        return start;
    }
    let step = step.to_radians();
    let angle = dy.atan2(dx);
    let held = (angle / step).round() * step;
    // The length the pointer went along the direction held to: never more than the length it went.
    let along = length * (angle - held).cos();
    (start.0 + along * held.cos(), start.1 + along * held.sin())
}

/// The path of the tools that read the two ends of a drag, `first` where the button went down and `last` where it came up.
pub(super) fn by_drag(tool: Tool, first: P, last: P, shift: bool) -> Result<Drawn, Problem> {
    match tool {
        Tool::Rectangle | Tool::Ellipse => {
            let last = if shift { square_corner(first, last) } else { last };
            let (x0, x1) = (first.0.min(last.0), first.0.max(last.0));
            let (y0, y1) = (first.1.min(last.1), first.1.max(last.1));
            if x1 - x0 < LEAST_SIDE || y1 - y0 < LEAST_SIDE {
                return Err(Problem::bad(format!("drag out a {} of at least {LEAST_SIDE} inch on each side", tool.name())));
            }
            let rect = Rect::new(x0, y0, x1, y1);
            let rows = if tool == Tool::Rectangle {
                let corner = |x: f64, y: f64| Point::new(x, y);
                vec![
                    CRow::Move(corner(x0, y0)),
                    CRow::Line(corner(x1, y0)),
                    CRow::Line(corner(x1, y1)),
                    CRow::Line(corner(x0, y1)),
                    CRow::Line(corner(x0, y0)),
                ]
            } else {
                let centre = Point::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                vec![CRow::Ellipse { centre, first: Point::new(x1, centre.y), second: Point::new(centre.x, y1) }]
            };
            Ok(Drawn { rows, closed: true, rect })
        }
        Tool::Line | Tool::Arc => {
            let last = if shift { step_end(first, last, STEP_DEGREES) } else { last };
            let length = (last.0 - first.0).hypot(last.1 - first.1);
            if length < LEAST_SIDE {
                return Err(Problem::bad(format!("drag out a {} of at least {LEAST_SIDE} inch", tool.name())));
            }
            let (from, to) = (Point::new(first.0, first.1), Point::new(last.0, last.1));
            let rows = if tool == Tool::Line {
                vec![CRow::Move(from), CRow::Line(to)]
            } else {
                // The bow is negative: the arc bulges to the left of the way the pointer went, so a drag from the left to the right arches over.
                vec![CRow::Move(from), CRow::Arc { to, bow: -ARC_BOW * length }]
            };
            let rect = bounds::bounds(&rows).ok_or_else(|| Problem::failed("the segment has no extent"))?;
            Ok(Drawn { rows, closed: false, rect })
        }
        Tool::Freeform | Tool::Pencil => Err(Problem::failed("a stroke tool is not read by its two ends")),
    }
}

/// The points a stroke tool reads: the stroke as it is, or with Shift the straight segment from where it began, held to a step of 45 degrees, to where it ended.
pub(super) fn straightened(points: &[P], shift: bool) -> Result<Vec<P>, Problem> {
    let (Some(first), Some(last)) = (points.first().copied(), points.last().copied()) else {
        return Err(Problem::bad("a stroke needs at least two points"));
    };
    if shift {
        let end = step_end(first, last, STEP_DEGREES);
        return Ok(vec![first, end]);
    }
    Ok(points.to_vec())
}
