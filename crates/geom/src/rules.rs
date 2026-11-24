//! The readings of the spec that a render may disagree with, each in one place with the spec's id.
//!
//! If a picture shows an arc bowing the wrong way, a reference that names the wrong row, or an ellipse of the wrong shape, the
//! fix is a change in this file and nowhere else: every other module takes these values and functions from here.

/// A project reading from samples; no document gives the sign of the bow. A positive `A` bulges to the right of
/// travel as the pen moves from the start to the end, a negative one to the left, as seen with y up. `+1.0` is that reading,
/// and `-1.0` would turn every `ArcTo` inside out: [`effective_bow`] is the only place the sign is applied.
pub const ARC_BOW_SIGN: f64 = 1.0;

/// The bow of an `ArcTo` after the sign rule. Positive is to the right of travel.
pub fn effective_bow(a: f64) -> f64 {
    ARC_BOW_SIGN * a
}

/// A bow that is this small a share of the chord is a straight line. The radius formula of divides
/// by the bow, so a bow of 0 or of 1E-300 would give an infinite radius; at this ratio the arc leaves the chord by less than a
/// billionth of its length.
pub const ARC_FLAT_RATIO: f64 = 1e-9;

/// A chord this small a share of the numbers it is made of is no chord. **This is our rule:** the specification says an end equal to the pen has no
/// direction and gives no tolerance, and a test of `chord > 0` is discontinuous at rounding noise (a pen of 0.30000000000000004 and an end of 0.3 with a bow of
/// 0.5 drew a whole circle, where an exact end drew a point). The share is: 1E-9 of the larger of `Width` and `Height`, with the other numbers of the row added
/// to what is larger ([`arc_chord_noise`]).
pub const ARC_CHORD_RATIO: f64 = 1e-9;

/// The chord length at or below which an `ArcTo` from `start` to `end` with bow `bow` has no chord ([`ARC_CHORD_RATIO`]): that share of the largest of
/// `|Width|`, `|Height|`, the absolute values of the four coordinates, and `|bow|`. Also our rule, and for these reasons: the sizes can be negative (
/// have thousands of such cells) and zero (the collapsed shapes); rounding noise is a share of the numbers that were rounded, so the coordinates are in it, and
/// near the origin they are nothing, so the bow, which is the arc's own size, is in it too: a pen of 5.55E-17 and an end at the origin on a shape of size (0, 0) was
/// a whole circle when the scale was the size or else the coordinates, with no floor at the origin.
pub fn arc_chord_noise(width: f64, height: f64, start: kurbo::Point, end: kurbo::Point, bow: f64) -> f64 {
    let scale = [height, start.x, start.y, end.x, end.y, bow].iter().fold(width.abs(), |larger, number| larger.max(number.abs()));
    ARC_CHORD_RATIO * scale
}

/// How a section's runs (the rows between moves) join in the fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunJoin {
    /// The page's reading: a move after the first row counts as a straight segment for the fill, so one section is one ring and the runs are joined by
    /// straight edges and closed once.
    StraightEdge,
    /// D2, another reader's reading: each run is closed on its own, so a section with several runs is several rings (the naive result: two lines and no fill).
    CloseEachRun,
}

/// Against D2: which reading the fill follows. The page's; M2's render check compares the two, and
/// [`crate::build_paths_with`] takes either, so the comparison needs no rebuild.
pub const FILL_RUN_JOIN: RunJoin = RunJoin::StraightEdge;

/// The points are turned by this many times `C` before the division, to bring the ellipse's axes onto the x and y axes.
/// The spec says "turn all three points by -`C`", so the sign is `-1.0`. A test case catches the other sign (a centre near (21.7, -14.4)).
pub const AXIS_TURN_SIGN: f64 = -1.0;

/// `D` is the length of the axis along `C` over the other, so in the turned frame each x is divided by `D`, and the
/// ellipse's half-axis along `C` is `r` times `D`. A test case catches the inverse (a centre at (0, -40) for `D` of 3).
pub fn ratio_scale(d: f64) -> f64 {
    1.0 / d
}

/// The page warns that a ratio of 0 or less, or over 1000, is unreliable. Above this the row is drawn as its chord.
pub const MAX_ELLIPTICAL_RATIO: f64 = 1000.0;

/// Three points whose circle is flatter than this, as the area they span over the square of their longest side,
/// lie on one line: the row draws the chord. That share of 1E-9 is the closeness the counts use for "the same".
pub const COLLINEAR_RATIO: f64 = 1e-9;

/// A path is closed when its start and end are this close, in the shape's frame. The page prints `10E-6`, which as
/// written is 1E-5. A test case holds under both readings (1E-5 and 1E-6).
pub const CLOSED_TOLERANCE: f64 = 1e-5;

/// The first and second points of an `Ellipse` are read as conjugate half-diameters. They are on perpendicular axes
/// when the cosine of the angle between them is 0; above this the row is flagged as unsettled. 0.1 is the cut the spec's counts use
/// ("off by a cosine above 0.1 only 9").
pub const OBLIQUE_COSINE: f64 = 0.1;

/// Knots that fall by less than this are equal (the digits of a repeated knot differ between `A` and `E`).
pub const KNOT_ROUNDING: f64 = 1e-9;

/// The most a spline's degree may be (MS-VSDX allows 0 to 25).
pub const MAX_DEGREE: usize = 25;

/// A reference `Geometry<n>.<letter><row>` names a section by its `IX` plus this, and a row by its `IX` plus
/// this. MS-VSDX says one for the row too, and samples say none: 258,625 references have their row only by `IX`
/// itself. Two test cases catch the other reading.
pub const SECTION_NUMBER_OVER_IX: u32 = 1;
/// See [`SECTION_NUMBER_OVER_IX`]: the row number of a reference is the row's `IX`.
pub const ROW_NUMBER_OVER_IX: u32 = 0;

/// The section `IX` and the row `IX` a `Geometry<section>.<letter><row>` reference names. `Geometry2.X4` is section
/// `IX` 1, row `IX` 4. `None` for a number that no `IX` has (`Geometry0`, which would be `IX` -1).
pub fn geometry_reference(section_number: u32, row_number: u32) -> Option<(u32, u32)> {
    Some((geometry_section_ix(section_number)?, row_number.checked_sub(ROW_NUMBER_OVER_IX)?))
}

/// The section `IX` that `Geometry<section_number>` names (`Geometry1.Path` is section `IX` 0), or `None` for 0.
pub fn geometry_section_ix(section_number: u32) -> Option<u32> {
    section_number.checked_sub(SECTION_NUMBER_OVER_IX)
}
