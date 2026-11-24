//! The drawing steps a row makes, before they are put on a path.

use kurbo::Point;

/// One segment from the pen: where it ends, and its control points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Seg {
    Line(Point),
    Quad(Point, Point),
    Cubic(Point, Point, Point),
}

impl Seg {
    /// Where the segment ends.
    pub(crate) fn end(self) -> Point {
        match self {
            Seg::Line(p) | Seg::Quad(_, p) | Seg::Cubic(_, _, p) => p,
        }
    }

    /// The same segment ending exactly at `end`: the last control point is moved with the end, so the curve keeps its shape.
    pub(crate) fn with_end(self, end: Point) -> Seg {
        match self {
            Seg::Line(_) => Seg::Line(end),
            Seg::Quad(c, _) => Seg::Quad(c, end),
            Seg::Cubic(c1, c2, old) => Seg::Cubic(c1, c2 + (end - old), end),
        }
    }

    /// True if every point of the segment is a finite number.
    pub(crate) fn is_finite(self) -> bool {
        match self {
            Seg::Line(p) => p.is_finite(),
            Seg::Quad(a, b) => a.is_finite() && b.is_finite(),
            Seg::Cubic(a, b, c) => a.is_finite() && b.is_finite() && c.is_finite(),
        }
    }
}

/// A step of a row: a segment from the pen, or a jump of the pen with no mark (a gap in the stroke, a straight edge in the fill).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Step {
    Seg(Seg),
    Jump(Point),
}

impl Step {
    pub(crate) fn is_finite(self) -> bool {
        match self {
            Step::Seg(seg) => seg.is_finite(),
            Step::Jump(p) => p.is_finite(),
        }
    }
}
