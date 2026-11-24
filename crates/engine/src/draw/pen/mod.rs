//! The drawing tools: Rectangle, Ellipse, Line, Freeform, Arc and Pencil (
//! "Drawing tools and geometry"). A tool turns the points of a drag into the `Geometry` rows of a new shape, or of a shape that is carried on from its end vertex.
//!
//! **The pure part is here, and the edit of the document is in [`edit`].** A tool takes the points the pointer went through, in **paper inches on the page**, and gives [`Drawn`]: the
//! rows of one path as [`CRow`]s (the rows `shape.combine` carries, in absolute coordinates), whether the path is closed, and the box round what it draws. [`edit`] makes a shape of
//! it (`shape.draw`), or adds it to a shape's path (`continue`), writing each row as the shares of the shape's box (`Width*0.25`), as a master's rows are.
//!
//! **What each tool makes** (the rows are the specification's):
//!
//! * **Rectangle**: `MoveTo` and four `LineTo` rows back to the first corner, closed. Shift makes a square.
//! * **Ellipse**: one `Ellipse` row in a box, closed. Shift makes a circle.
//! * **Line**: `MoveTo` and `LineTo`. Shift holds the line to a step of 45 degrees.
//! * **Arc**: `MoveTo` and one `ArcTo`, a quarter of a circle between the two points, bowed to the left of the way the pointer went. Shift holds the chord to a step of 45
//!   degrees.
//! * **Pencil** ([`pencil`]): the stroke becomes lines and arcs: `LineTo` for each straight run and `ArcTo` for each run that is part of a circle, split where the stroke turns.
//! * **Freeform** ([`freeform`]): the stroke becomes one smooth curve: a single `NURBSTo` row (degree 3, the knots of a composite Bezier), fitted by the path
//!   library the Craft apps share (`craft-pathops`' simplify, as VectorCraft's pencil and brush use it), within a tolerance and a smoothing.
//!
//! **Shift on the Line, Arc, Pencil and Freeform tools holds the stroke to a straight segment** in steps of 45 degrees, and on the Rectangle and Ellipse tools makes a square and a circle.
//! Only the Rectangle's tooltip names it; the other keys are ours and are on the check list.
//!
//! **Every length is bounded before work is done** ([`MAX_STROKE_POINTS`], [`MAX_PIECES`]), and a refusal says in words what was wrong.

mod bounds;
pub(crate) mod edit;
mod freeform;
mod pencil;
mod shapes;

#[cfg(test)]
mod tests;

use kurbo::Rect;

use super::shapeops::CRow;
use super::{MAX_COORD, Problem};

/// A point of the page, in paper inches (`x` right, `y` up).
pub type P = (f64, f64);

/// The most points one stroke may have. A window thins what the pointer gave to a point for every few screen points, so a stroke is a few hundred; a program may give more.
pub const MAX_STROKE_POINTS: usize = 5_000;

/// The most rows (lines and arcs) a Pencil stroke may be fitted with, so a scribble is refused and not cut into thousands of segments.
pub const MAX_PIECES: usize = 256;

/// The least a side of a rectangle or ellipse, and the least a line, an arc or a stroke is long, in inches: under it nothing is drawn (the press and release of a click).
pub const LEAST_SIDE: f64 = 1.0e-3;

/// The farthest a stroke may stand from the lines and arcs, or the curve, that stand for it, when the caller gives no tolerance: a thirtieth of an inch, three screen points at the
/// default zoom.
pub const DEFAULT_TOLERANCE: f64 = 1.0 / 32.0;

/// How much a Freeform stroke is smoothed when the caller gives none, from 0 (the curve fitted as it is) to 1.
pub const DEFAULT_SMOOTHING: f64 = 0.4;

/// A tool of Home, Tools, Drawing Tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Rectangle,
    Ellipse,
    Line,
    Freeform,
    Arc,
    Pencil,
}

impl Tool {
    /// The six tools in the order of the Drawing Tools menu.
    pub const ALL: [Tool; 6] = [Tool::Rectangle, Tool::Ellipse, Tool::Line, Tool::Freeform, Tool::Arc, Tool::Pencil];

    /// The word the command takes for the tool.
    pub const fn name(self) -> &'static str {
        match self {
            Tool::Rectangle => "rectangle",
            Tool::Ellipse => "ellipse",
            Tool::Line => "line",
            Tool::Freeform => "freeform",
            Tool::Arc => "arc",
            Tool::Pencil => "pencil",
        }
    }

    /// The tool a word names, in any case.
    pub fn from_name(word: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.name().eq_ignore_ascii_case(word.trim()))
    }

    /// Whether the tool makes a path that has an end vertex to go on from (a line, arc, pencil or freeform stroke), as the rectangle and the ellipse do not.
    pub const fn goes_on(self) -> bool {
        matches!(self, Tool::Line | Tool::Freeform | Tool::Arc | Tool::Pencil)
    }

    /// The label of the undo step the tool's drawing makes.
    pub const fn label(self) -> &'static str {
        match self {
            Tool::Rectangle => "Draw rectangle",
            Tool::Ellipse => "Draw ellipse",
            Tool::Line => "Draw line",
            Tool::Freeform => "Draw freeform",
            Tool::Arc => "Draw arc",
            Tool::Pencil => "Draw with pencil",
        }
    }
}

/// How a tool reads a stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Shift is held: a square or a circle, or a straight segment at a step of 45 degrees.
    pub shift: bool,
    /// The farthest the lines and arcs of the Pencil, or the curve of the Freeform, may stand from the stroke, in inches.
    pub tolerance: f64,
    /// How much the Freeform stroke is smoothed, 0 to 1.
    pub smoothing: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { shift: false, tolerance: DEFAULT_TOLERANCE, smoothing: DEFAULT_SMOOTHING }
    }
}

/// What a tool drew: the rows of one path, in paper inches on the page.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Drawn {
    /// The rows in order: a move to the start, then the segments. An ellipse is its one `Ellipse` row.
    pub rows: Vec<CRow>,
    /// Whether the path ends where it began (it takes the fill, `NoFill` 0).
    pub closed: bool,
    /// The box round what the rows draw.
    pub rect: Rect,
}

/// What a tool would draw, for the preview of a drag: the outline as lines, in paper inches.
#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
    /// One run of points, the path flattened.
    pub points: Vec<P>,
    /// Whether the path is closed.
    pub closed: bool,
    /// How many rows the shape would have (a line is two, a freeform stroke two, a pencil stroke one more for each segment).
    pub rows: usize,
}

/// The points a stroke is read from: finite, within the page's reach, not too many.
fn checked(points: &[P]) -> Result<(), Problem> {
    if points.len() > MAX_STROKE_POINTS {
        return Err(Problem::bad(format!("a stroke has at most {MAX_STROKE_POINTS} points, and this has {}", points.len())));
    }
    if points.len() < 2 {
        return Err(Problem::bad("a stroke needs at least two points: where it began and where it ended"));
    }
    if points.iter().any(|(x, y)| !(x.is_finite() && y.is_finite() && x.abs() <= MAX_COORD && y.abs() <= MAX_COORD)) {
        return Err(Problem::bad(format!("every point of a stroke must be a number within {MAX_COORD} of the page's origin")));
    }
    Ok(())
}

/// The settings a caller gave, checked.
fn checked_settings(s: &Settings) -> Result<(), Problem> {
    if !(s.tolerance.is_finite() && s.tolerance >= 1.0e-4 && s.tolerance <= 10.0) {
        return Err(Problem::bad("`tolerance` must be a number of inches from 0.0001 to 10"));
    }
    if !(s.smoothing.is_finite() && (0.0..=1.0).contains(&s.smoothing)) {
        return Err(Problem::bad("`smoothing` must be a number from 0 to 1"));
    }
    Ok(())
}

/// What `tool` draws for the stroke `points` (paper inches) under `settings`. The rectangle, ellipse, line and arc tools read the first and the last point (the two ends of a drag); the
/// pencil and the freeform tool read the whole stroke. A stroke too short, too long, or with a point that is no number, and a drag that makes nothing (a click), are refused in words.
pub(crate) fn draw(tool: Tool, points: &[P], settings: &Settings) -> Result<Drawn, Problem> {
    checked(points)?;
    checked_settings(settings)?;
    match tool {
        Tool::Rectangle | Tool::Ellipse | Tool::Line | Tool::Arc => {
            let (Some(first), Some(last)) = (points.first().copied(), points.last().copied()) else {
                return Err(Problem::bad("a stroke needs at least two points"));
            };
            shapes::by_drag(tool, first, last, settings.shift)
        }
        Tool::Pencil => {
            let points = shapes::straightened(points, settings.shift)?;
            pencil::draw(&points, settings.tolerance)
        }
        Tool::Freeform => {
            let points = shapes::straightened(points, settings.shift)?;
            freeform::draw(&points, settings.tolerance, settings.smoothing)
        }
    }
}

/// What `tool` would draw for the stroke, as an outline (the preview of a drag, which asks what the release would make); `None` when it would make nothing or refuse.
pub fn outline(tool: Tool, points: &[P], settings: &Settings) -> Option<Outline> {
    let drawn = draw(tool, points, settings).ok()?;
    Some(Outline { points: bounds::flatten(&drawn.rows), closed: drawn.closed, rows: drawn.rows.len() })
}

pub use edit::{Vertices, continuable};
/// For the Kani harnesses of the crate (`crate::kani_proofs`).
#[cfg(kani)]
pub(crate) use freeform::{MAX_CUBICS, knot};
pub use shapes::{square_corner, step_end};
