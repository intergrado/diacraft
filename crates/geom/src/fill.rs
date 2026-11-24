//! What is filled and what is stroked of a section's path.

use kurbo::{BezPath, Point, Shape};

use crate::rules;

/// The flags of a `Geometry` section: cells of the section and not of a row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SectionFlags {
    /// `NoFill` 1: the path is kept out of the shape's fill.
    pub no_fill: bool,
    /// `NoLine` 1: no stroke along the path.
    pub no_line: bool,
    /// `NoShow` 1: neither stroke nor fill.
    pub no_show: bool,
}

/// What is drawn of a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paint {
    pub stroke: bool,
    pub fill: bool,
}

impl SectionFlags {
    /// What is drawn of this section, in the order gives: nothing at all for `NoShow` 1; then `NoLine` and `NoFill` decide stroke and fill
    /// apart; and a `FillPattern` of 0 on the shape (`fill_pattern_is_none`) leaves every path unfilled, whatever `NoFill` says. The shape's layer being
    /// invisible, the first test, is the caller's.
    pub fn paint(self, fill_pattern_is_none: bool) -> Paint {
        if self.no_show {
            return Paint { stroke: false, fill: false };
        }
        Paint { stroke: !self.no_line, fill: !self.no_fill && !fill_pattern_is_none }
    }

    /// True if the section's ring takes part in the shape's fill: `NoFill` 0 and `NoShow` not 1 (the project rule, which
    /// tutorial needs: an unfilled section does not make holes in a filled one).
    fn counts_for_fill(self) -> bool {
        !self.no_fill && !self.no_show
    }
}

/// True if a run's start and end coincide: within `rules::CLOSED_TOLERANCE` in the shape's frame. A point that is not a number is
/// not closed.
pub fn is_closed(start: Point, end: Point) -> bool {
    let distance = start.distance(end);
    distance.is_finite() && distance <= rules::CLOSED_TOLERANCE
}

/// The one path a shape's sections are filled by: the rings of the sections that count (`NoFill` 0 and `NoShow` not 1), together. Fill it
/// with the odd-crossings (even-odd) rule, so that overlaps between the sections become holes.
pub fn combined_fill<'a>(sections: impl IntoIterator<Item = (&'a BezPath, SectionFlags)>) -> BezPath {
    let mut out = BezPath::new();
    for (ring, flags) in sections {
        if flags.counts_for_fill() {
            for element in ring.elements() {
                out.push(*element);
            }
        }
    }
    out
}

/// True if a ray from `point` meets `path` an odd number of times, so the point is filled by the odd-crossings rule. The path's
/// subpaths are closed, as the rings of this crate are. A point that is not a number is in no path, and a path that holds a point that is not a number fills
/// nothing: kurbo's `winding` asserts that its input is a number (a panic in a debug build), and WebAssembly has no guard to catch one.
pub fn even_odd_contains(path: &BezPath, point: Point) -> bool {
    point.is_finite() && path.is_finite() && path.winding(point) % 2 != 0
}
