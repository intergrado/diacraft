//! What can go wrong building a path, and what is reported when a row is skipped or drawn by a fallback rule.

/// A failure that stops a whole path: too much to draw, or a number that cannot be a size. A row that cannot be drawn is not an
/// error of the path: it is skipped, or drawn by the spec's fallback, and a [`Warning`] says so.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GeomError {
    /// A section holds more rows than `MAX_ROWS` (a hostile file may carry any number).
    #[error("a geometry section holds {rows} rows and at most {max} are drawn")]
    TooManyRows { rows: usize, max: usize },
    /// The path would hold more elements than `MAX_PATH_ELEMENTS`.
    #[error("the path would hold more than {max} elements")]
    PathTooLarge { max: usize },
    /// The splines of a section spent all the work they may (`build_row_draws`): a curve was skipped for it, and the path is shorter than the
    /// rows say, so the section is refused and not drawn short.
    #[error("the splines of the section spent all the work they may, and the path would be short of a curve")]
    SplineWorkSpent,
    /// `Width` or `Height` is not a finite number.
    #[error("the shape's width or height is not a finite number")]
    BadSize,
    /// A `POLYLINE` or `NURBS` constructor that cannot be read: a wrong count of arguments, a number that is not finite, or
    /// text that is not a call of that name (a wrong count evaluates to `#VALUE!`).
    #[error("the {name} constructor cannot be read: {problem}")]
    BadConstructor { name: &'static str, problem: &'static str },
}

/// Why a row was skipped or drawn by a fallback. Each names the spec's rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningKind {
    /// A row that needs a pen when there is none (the first row is not a move, or the row follows an `Ellipse`). Skipped.
    NoPen,
    /// A number of the row is not finite, or a point it makes overflows. Skipped. Hostile input; not in the spec.
    NonFinite,
    /// The arithmetic of an arc or an ellipse overflows (a bow of 1E200, points of 1E300): the numbers are finite and the circle or ellipse through them is not
    /// a number. What happens depends on the row: an `ArcTo` or an `EllipticalArcTo` is drawn as a straight line to its end, as the arc's other fallbacks are, and
    /// not silently; an `Ellipse` row has no chord to fall back to, and **draws nothing** (skipped, the pen as it was: an `Ellipse` leaves none). Hostile input;
    /// not in the spec.
    Overflow,
    /// A spline that is not within `CURVE_TOLERANCE` of its curve where it was checked, because the cutting into cubics stopped at its depth limit
    /// (a degree 25 curve with wild control points). Drawn all the same.
    CurveApproximate,
    /// A `PolylineTo` with no readable `A`. Drawn as a straight line to (`X`, `Y`).
    PolylineUnreadable,
    /// A ratio below 0 or above 1000 (the page calls it unpredictable). Drawn as the chord.
    EllipticalRatio,
    /// A `NURBSTo` row (or a spline run) that cannot be drawn. Skipped, and the pen goes to its end. The text says why.
    NurbsUnusable(&'static str),
    /// A spline whose first knot is repeated fewer than `degree` + 1 times (periodic). Drawn by the knot rule as the
    /// data stand, which does not start on the pen.
    PeriodicSpline,
    /// A `SplineKnot` outside a run, or a `SplineStart` with no knot after it. Drawn as a straight line to its point.
    SplineStray,
    /// The two points are not on perpendicular axes. Drawn by the conjugate half-diameter formula, which is a guess.
    ObliqueEllipse,
}

impl std::fmt::Display for WarningKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WarningKind::NoPen => f.write_str("the row needs a current point and there is none: skipped"),
            WarningKind::NonFinite => f.write_str("a number of the row is not finite, or a point it makes overflows: skipped"),
            WarningKind::Overflow => f.write_str(
                "the curve's arithmetic overflows: an arc is drawn as a straight line to its end, and an ellipse row is skipped and draws nothing",
            ),
            WarningKind::CurveApproximate => {
                f.write_str("the spline could not be cut finely enough to be within the tolerance: drawn as near as it came")
            }
            WarningKind::PolylineUnreadable => f.write_str("the polyline has no readable points: drawn as a straight line"),
            WarningKind::EllipticalRatio => f.write_str("the axis ratio is below 0 or above 1000: drawn as a straight line"),
            WarningKind::NurbsUnusable(why) => write!(f, "the spline cannot be drawn: {why}"),
            WarningKind::PeriodicSpline => f.write_str("a periodic spline: drawn from its knots, so it does not start on the current point"),
            WarningKind::SplineStray => f.write_str("a spline knot with no spline start, or a start with no knot: drawn as a straight line"),
            WarningKind::ObliqueEllipse => f.write_str("the ellipse's two points are not on perpendicular axes: drawn as conjugate half-diameters"),
        }
    }
}

/// A row that was skipped or drawn by a fallback, by its index in the rows given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Warning {
    /// Index of the row in the slice given to `build_paths`.
    pub row: usize,
    pub kind: WarningKind,
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "row {}: {}", self.row, self.kind)
    }
}
