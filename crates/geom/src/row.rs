//! The rows of a `Geometry` section, as evaluated numbers (the table of the 15 row types).
//!
//! `geom` takes numbers already evaluated by the formula engine: `V` of each cell, in inches for a length, radians for an angle,
//! and a share for a cell of a relative row. The fields are named for what the spec says the cell means; the doc of each row
//! gives the cell letters.

use crate::error::GeomError;

/// The most points a `POLYLINE` or `NURBS` constructor may hold. The most in files are 490 pairs and 773 tuples.
pub const MAX_CONSTRUCTOR_POINTS: usize = 50_000;

/// The longest constructor text read (the `V` of an `A` or `E` cell).
pub const MAX_CONSTRUCTOR_TEXT: usize = 4 << 20;

/// One row of a `Geometry` section.
#[derive(Clone, Debug, PartialEq)]
pub enum GeomRow {
    /// `X`, `Y`: where the pen goes, with no mark.
    MoveTo { x: f64, y: f64 },
    /// `X`, `Y` as shares of `Width` and `Height`.
    RelMoveTo { x: f64, y: f64 },
    /// `X`, `Y`: a straight segment from the pen.
    LineTo { x: f64, y: f64 },
    /// `X`, `Y` as shares of `Width` and `Height`.
    RelLineTo { x: f64, y: f64 },
    /// `X`, `Y`: the end; `A`: the bow, how far the arc's middle stands from the chord's, signed.
    ArcTo { x: f64, y: f64, bow: f64 },
    /// `X`, `Y`: the end; `A`, `B`: a point the arc passes through; `C`: the angle of the axis the ratio is along, in radians;
    /// `D`: the ratio of that axis to the other.
    EllipticalArcTo { x: f64, y: f64, mid_x: f64, mid_y: f64, angle: f64, ratio: f64 },
    /// As [`GeomRow::EllipticalArcTo`] with `X`, `Y`, `A` and `B` as shares of `Width` and `Height`; `C` and `D` are not scaled.
    RelEllipticalArcTo { x: f64, y: f64, mid_x: f64, mid_y: f64, angle: f64, ratio: f64 },
    /// `X`, `Y`: the end; `A`: the `POLYLINE` constructor, `None` when it cannot be read.
    PolylineTo { x: f64, y: f64, polyline: Option<Polyline> },
    /// `X`, `Y`: the last control point; `A`: the second-to-last knot; `B`: the last weight; `C`: the first knot; `D`: the first
    /// weight; `E`: the `NURBS` constructor, `None` when it cannot be read.
    NurbsTo { x: f64, y: f64, second_last_knot: f64, last_weight: f64, first_knot: f64, first_weight: f64, nurbs: Option<Nurbs> },
    /// `X`, `Y`: the second control point; `A`: the second knot; `B`: the first knot; `C`: the last knot; `D`: the degree.
    SplineStart { x: f64, y: f64, second_knot: f64, first_knot: f64, last_knot: f64, degree: f64 },
    /// `X`, `Y`: the next control point; `A`: its knot.
    SplineKnot { x: f64, y: f64, knot: f64 },
    /// `X`, `Y`: the end; `A`, `B`: the first control point; `C`, `D`: the second; all shares.
    RelCubBezTo { x: f64, y: f64, c1_x: f64, c1_y: f64, c2_x: f64, c2_y: f64 },
    /// `X`, `Y`: the end; `A`, `B`: the control point; all shares.
    RelQuadBezTo { x: f64, y: f64, c_x: f64, c_y: f64 },
    /// `X`, `Y`: the centre; `A`, `B`: the first point; `C`, `D`: the second point. Alone in its section.
    Ellipse { x: f64, y: f64, first_x: f64, first_y: f64, second_x: f64, second_y: f64 },
    /// `X`, `Y`: the first point; `A`, `B`: the second point. Alone in its section.
    InfiniteLine { x: f64, y: f64, other_x: f64, other_y: f64 },
}

/// The `POLYLINE(xType, yType, x1, y1, ...)` constructor of a `PolylineTo` row.
#[derive(Clone, Debug, PartialEq)]
pub struct Polyline {
    /// `xType` of 0: every x is a share of `Width`. Any other value: a length in the shape's frame.
    pub x_is_share: bool,
    /// `yType` of 0: every y is a share of `Height`.
    pub y_is_share: bool,
    /// The vertices, before the last one that `X` and `Y` give.
    pub points: Vec<(f64, f64)>,
}

impl Polyline {
    const NAME: &'static str = "POLYLINE";

    /// The constructor from its arguments: two types, then pairs. An odd count gives an error (`POLYLINE(0,0,0.25)` is `#VALUE!`),
    /// and so does an argument that is not a finite number, or more points than [`MAX_CONSTRUCTOR_POINTS`].
    pub fn from_args(args: &[f64]) -> Result<Polyline, GeomError> {
        let bad = |problem| GeomError::BadConstructor { name: Self::NAME, problem };
        let Some((types, rest)) = args.split_first_chunk::<2>() else { return Err(bad("it needs two types before the pairs")) };
        let (pairs, odd) = rest.as_chunks::<2>();
        if !odd.is_empty() {
            return Err(bad("the points are not in pairs"));
        }
        if pairs.len() > MAX_CONSTRUCTOR_POINTS {
            return Err(bad("it holds too many points"));
        }
        if args.iter().any(|v| !v.is_finite()) {
            return Err(bad("an argument is not a finite number"));
        }
        let [x_type, y_type] = *types;
        let points = pairs.iter().map(|&[x, y]| (x, y)).collect();
        Ok(Polyline { x_is_share: x_type == 0.0, y_is_share: y_type == 0.0, points })
    }

    /// The constructor from the text a cell's `V` holds (`POLYLINE(0,0,0.5,0.5)`: plain numbers).
    pub fn from_text(text: &str) -> Result<Polyline, GeomError> {
        Polyline::from_args(&call_arguments(text, Self::NAME, 2 + 2 * MAX_CONSTRUCTOR_POINTS)?)
    }
}

/// One tuple of a `NURBS` constructor: a control point, its knot and its weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NurbsPoint {
    pub x: f64,
    pub y: f64,
    pub knot: f64,
    pub weight: f64,
}

/// The `NURBS(knotLast, degree, xType, yType, x1, y1, knot1, weight1, ...)` constructor of a `NURBSTo` row.
#[derive(Clone, Debug, PartialEq)]
pub struct Nurbs {
    /// The last knot of the list.
    pub last_knot: f64,
    /// The degree as the constructor holds it. It must be a whole number from 1 to 25 for the row to draw.
    pub degree: f64,
    /// `xType` of 0: every x of the tuples is a share of `Width` (`X` of the row is a length either way).
    pub x_is_share: bool,
    /// `yType` of 0: every y of the tuples is a share of `Height`.
    pub y_is_share: bool,
    /// The middle control points with their knots and weights.
    pub points: Vec<NurbsPoint>,
}

impl Nurbs {
    const NAME: &'static str = "NURBS";

    /// The constructor from its arguments: four, then groups of four. Seven arguments give an error (E-V2: `#VALUE!`), as does an
    /// argument that is not a finite number or more tuples than [`MAX_CONSTRUCTOR_POINTS`].
    pub fn from_args(args: &[f64]) -> Result<Nurbs, GeomError> {
        let bad = |problem| GeomError::BadConstructor { name: Self::NAME, problem };
        let Some((head, rest)) = args.split_first_chunk::<4>() else { return Err(bad("it needs four leading arguments")) };
        let (groups, odd) = rest.as_chunks::<4>();
        if !odd.is_empty() {
            return Err(bad("the tuples are not in groups of four"));
        }
        if groups.len() > MAX_CONSTRUCTOR_POINTS {
            return Err(bad("it holds too many points"));
        }
        if args.iter().any(|v| !v.is_finite()) {
            return Err(bad("an argument is not a finite number"));
        }
        let [last_knot, degree, x_type, y_type] = *head;
        let points = groups.iter().map(|&[x, y, knot, weight]| NurbsPoint { x, y, knot, weight }).collect();
        Ok(Nurbs { last_knot, degree, x_is_share: x_type == 0.0, y_is_share: y_type == 0.0, points })
    }

    /// The constructor from the text a cell's `V` holds (`NURBS(1,3,1,1,1,2,0,1,3,2,0,1)`: plain numbers).
    pub fn from_text(text: &str) -> Result<Nurbs, GeomError> {
        Nurbs::from_args(&call_arguments(text, Self::NAME, 4 + 4 * MAX_CONSTRUCTOR_POINTS)?)
    }
}

/// The numbers of a call `NAME(n1,n2,...)` in the text of a cell's value: the name in any case, white space anywhere between tokens,
/// and plain finite numbers (formula-language R16) as the arguments. At most `max_args` arguments are read.
fn call_arguments(text: &str, name: &'static str, max_args: usize) -> Result<Vec<f64>, GeomError> {
    let bad = |problem| GeomError::BadConstructor { name, problem };
    if text.len() > MAX_CONSTRUCTOR_TEXT {
        return Err(bad("the text is too long"));
    }
    let text = text.trim();
    let head = text.get(..name.len()).ok_or_else(|| bad("it is not a call of that name"))?;
    if !head.eq_ignore_ascii_case(name) {
        return Err(bad("it is not a call of that name"));
    }
    let rest = text.get(name.len()..).unwrap_or_default().trim_start();
    let inner = rest.strip_prefix('(').and_then(|r| r.strip_suffix(')')).ok_or_else(|| bad("it is not a parenthesised list"))?;
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut args = Vec::new();
    for piece in inner.split(',') {
        if args.len() >= max_args {
            return Err(bad("it holds too many arguments"));
        }
        match piece.trim().parse::<f64>() {
            Ok(number) if number.is_finite() => args.push(number),
            _ => return Err(bad("an argument is not a finite number")),
        }
    }
    Ok(args)
}
