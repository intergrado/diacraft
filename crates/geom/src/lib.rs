//! Geometry: the rows of a ShapeSheet `Geometry` section, built into paths on kurbo.
//!
//! [`build_paths`] takes the rows of one section as evaluated numbers (the
//! formula engine evaluates the cells; this crate does not) with the shape's `Width` and `Height`, which the relative rows need,
//! and returns the path to stroke, the ring to fill, the unbounded lines, and a warning for each row it had to skip or draw by a
//! fallback rule. The readings a render may disagree with (the sign of an arc's bow, the turn and ratio of an elliptical arc, the
//! row numbering of references) are in [`rules`], one named place each, with the spec's ids.
//!
//! Every coordinate is in the shape's own frame, x right and y up, in inches; the frame's transform to the page is not
//! applied here.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod arc;
mod build;
mod error;
mod fill;
mod infinite;
#[cfg(kani)]
mod kani_proofs;
mod nurbs;
mod row;
pub mod rules;
mod seg;

pub use arc::{ArcFallback, CURVE_TOLERANCE, CircularArc, EllipseArc, circular_arc, elliptical_arc};
pub use build::{
    Bound, BuiltPaths, DrawStep, MAX_PATH_ELEMENTS, MAX_ROWS, RowDraw, RowDraws, build_path, build_paths, build_paths_bounded, build_paths_with,
    build_row_draws,
};
pub use error::{GeomError, Warning, WarningKind};
pub use fill::{Paint, SectionFlags, combined_fill, even_odd_contains, is_closed};
pub use infinite::InfiniteLine;
pub use nurbs::{CurveProblem, NurbsCurve};
pub use row::{GeomRow, MAX_CONSTRUCTOR_POINTS, MAX_CONSTRUCTOR_TEXT, Nurbs, NurbsPoint, Polyline};
pub use rules::RunJoin;

pub use kurbo;
