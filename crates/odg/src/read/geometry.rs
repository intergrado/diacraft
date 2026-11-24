//! The geometry of the plain shapes: view boxes, `draw:points`, SVG path
//! data, the kinds of `draw:circle` and `draw:ellipse`, and `draw:regular-polygon`, as the rows of `Geometry` sections, each coordinate a share
//! of the frame with y flipped (a view-box point (*px*, *py*) of the box (*L*, *T*, *VW*, *VH*) is the share ((*px* - *L*) / *VW*,
//! 1 - (*py* - *T*) / *VH*)).
//!
//! **Bounded**: every command and every coordinate pair is charged to the page's budget ([`Budget`]) before it is
//! stored, and a single `draw:points` is held to [`crate::Limits::max_points`] pairs. Past either, `xml_too_complex`.

use std::f64::consts::PI;

use super::units;
use crate::error::OdgError;

/// A view box (`svg:viewBox`): left, top, width and height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ViewBox {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

impl ViewBox {
    /// A view-box point as a share of the frame, y flipped.
    pub(crate) fn share(&self, x: f64, y: f64) -> (f64, f64) {
        ((x - self.left) / self.width, 1.0 - (y - self.top) / self.height)
    }
}

/// Why a view box is not one: `None` from [`view_box`] means the attribute is absent or unreadable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoxFault {
    /// A width or height of 0 (or less): a box that maps onto no frame.
    Empty,
}

/// `svg:viewBox`: four numbers separated by white space or commas. `Err(Empty)` for one of width or height 0 or less.
pub(crate) fn view_box(text: &str) -> Option<Result<ViewBox, BoxFault>> {
    let numbers: Vec<f64> =
        text.split(|c: char| c.is_ascii_whitespace() || c == ',').filter(|s| !s.is_empty()).map(units::number_any).collect::<Option<_>>()?;
    let [left, top, width, height] = numbers.as_slice() else { return None };
    if *width <= 0.0 || *height <= 0.0 {
        return Some(Err(BoxFault::Empty));
    }
    Some(Ok(ViewBox { left: *left, top: *top, width: *width, height: *height }))
}

/// What a page may still spend on path commands and coordinate pairs.
pub(crate) struct Budget {
    pub left: usize,
    pub max: usize,
    /// What was spent, for the read's work count.
    pub spent: usize,
}

impl Budget {
    pub(crate) fn new(max: usize) -> Budget {
        Budget { left: max, max, spent: 0 }
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<(), OdgError> {
        self.left = self.left.checked_sub(n).ok_or(OdgError::TooComplex { what: "path commands in one page", max: self.max as u64 })?;
        self.spent = self.spent.saturating_add(n);
        Ok(())
    }
}

/// The `C` and `D` of an elliptical arc row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ArcAxes {
    /// No turn: `C` the value 0 and `D` the formula `<k>*Width/Height`.
    Upright { k: f64 },
    /// Turned: `C` and `D` the formulas of `_ELLIPSE_THETA` and `_ELLIPSE_ECC` from the axis angle `c0` and ratio `d0` in the frame at
    /// the size it was read at.
    Turned { c0: f64, d0: f64 },
    /// A value of each: `C` 0 and `D` the ratio (the kinds of `draw:ellipse` and `draw:circle`, R6: all as values).
    Values { d: f64 },
}

/// One row of a section, in shares of the frame. The variants are named after the model's row types.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Row {
    RelMoveTo(f64, f64),
    RelLineTo(f64, f64),
    /// End `X`, `Y`; first control `A`, `B`; second control `C`, `D`.
    RelCubBezTo {
        x: f64,
        y: f64,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
    },
    /// End `X`, `Y`; control `A`, `B`.
    RelQuadBezTo {
        x: f64,
        y: f64,
        a: f64,
        b: f64,
    },
    /// End `X`, `Y`; the arc's middle point `A`, `B`; its axes.
    RelEllipticalArcTo {
        x: f64,
        y: f64,
        a: f64,
        b: f64,
        axes: ArcAxes,
    },
}

/// A section: its rows and whether it is open and so not filled (`NoFill` 1, R25 by ODF §20.118).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Section {
    pub rows: Vec<Row>,
    pub no_fill: bool,
}

/// The rectangle: one section of `RelMoveTo` (0, 0) and `RelLineTo` (1, 0), (1, 1), (0, 1), (0, 0).
pub(crate) fn rectangle() -> Vec<Section> {
    vec![Section {
        rows: vec![Row::RelMoveTo(0.0, 0.0), Row::RelLineTo(1.0, 0.0), Row::RelLineTo(1.0, 1.0), Row::RelLineTo(0.0, 1.0), Row::RelLineTo(0.0, 0.0)],
        no_fill: false,
    }]
}

/// `draw:points`: pairs `x,y` separated by white space. `None` when a pair cannot be read.
pub(crate) fn points(text: &str, max: usize, budget: &mut Budget) -> Result<Option<Vec<(f64, f64)>>, OdgError> {
    let mut out = Vec::new();
    for pair in text.split_ascii_whitespace() {
        if out.len() >= max {
            return Err(OdgError::TooComplex { what: "coordinate pairs in one draw:points", max: max as u64 });
        }
        budget.take(1)?;
        let Some((x, y)) = pair.split_once(',') else { return Ok(None) };
        let (Some(x), Some(y)) = (units::number_any(x), units::number_any(y)) else { return Ok(None) };
        out.push((x, y));
    }
    Ok(Some(out))
}

/// The section of a polyline (open, so `NoFill` 1) or polygon (closed by a `RelLineTo` to its first point) through `points`.
pub(crate) fn poly(points: &[(f64, f64)], vb: &ViewBox, closed: bool) -> Vec<Section> {
    let mut rows = Vec::with_capacity(points.len() + 1);
    for (i, (x, y)) in points.iter().enumerate() {
        let (sx, sy) = vb.share(*x, *y);
        rows.push(if i == 0 { Row::RelMoveTo(sx, sy) } else { Row::RelLineTo(sx, sy) });
    }
    if closed && let Some((x, y)) = points.first() {
        let (sx, sy) = vb.share(*x, *y);
        rows.push(Row::RelLineTo(sx, sy));
    }
    if rows.is_empty() {
        return Vec::new();
    }
    vec![Section { rows, no_fill: !closed }]
}

/// Why path data could not be read: it breaks SVG's syntax.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Malformed;

/// A reader of SVG path data's tokens. **The writer reads a path with this one too** (`write/route.rs`, `shifted`): what the reader reads as a path the writer
/// reads as one, and a path the reader refuses the writer leaves as it is.
pub(crate) struct Tokens<'a> {
    bytes: &'a [u8],
    /// Where the next token begins: the bytes before it are read.
    pub(crate) at: usize,
}

impl<'a> Tokens<'a> {
    /// The tokens of `text`, from its first byte.
    pub(crate) fn new(text: &'a str) -> Tokens<'a> {
        Tokens { bytes: text.as_bytes(), at: 0 }
    }

    /// Passes over white space and commas.
    pub(crate) fn skip(&mut self) {
        self.at += self.count_from(self.at, |b| b.is_ascii_whitespace() || b == b',');
    }

    /// How many bytes from `at` on are `is`.
    fn count_from(&self, at: usize, is: impl Fn(u8) -> bool) -> usize {
        self.bytes.get(at..).map_or(0, |rest| rest.iter().take_while(|b| is(**b)).count())
    }

    /// The next command letter, if the next token is one.
    pub(crate) fn command(&mut self) -> Option<u8> {
        self.skip();
        let b = *self.bytes.get(self.at)?;
        if b.is_ascii_alphabetic() && !matches!(b, b'e' | b'E') {
            self.at += 1;
            Some(b)
        } else {
            None
        }
    }

    /// The next number (SVG's grammar: a sign, digits, a fraction, an exponent; numbers may run together, `1-2` and `.5.5`).
    pub(crate) fn number(&mut self) -> Result<f64, Malformed> {
        self.number_at().map(|(_, value)| value)
    }

    /// [`Tokens::number`], and the place its text has in the data: the byte it begins at and the byte after it ends.
    pub(crate) fn number_at(&mut self) -> Result<((usize, usize), f64), Malformed> {
        self.skip();
        let start = self.at;
        let mut i = self.at;
        if matches!(self.bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let whole = self.count_from(i, |b| b.is_ascii_digit());
        i += whole;
        let mut fraction = 0;
        if self.bytes.get(i) == Some(&b'.') {
            fraction = self.count_from(i + 1, |b| b.is_ascii_digit());
            i += 1 + fraction;
        }
        if whole + fraction == 0 {
            return Err(Malformed);
        }
        if matches!(self.bytes.get(i), Some(b'e' | b'E')) {
            let sign = usize::from(matches!(self.bytes.get(i + 1), Some(b'+' | b'-')));
            let exponent = self.count_from(i + 1 + sign, |b| b.is_ascii_digit());
            if exponent > 0 {
                i += 1 + sign + exponent;
            }
        }
        let text = std::str::from_utf8(self.bytes.get(start..i).ok_or(Malformed)?).map_err(|_| Malformed)?;
        self.at = i;
        text.parse::<f64>().ok().filter(|n| n.is_finite()).map(|n| ((start, i), n)).ok_or(Malformed)
    }

    /// An arc's flag: one `0` or `1`, which may run into what follows.
    pub(crate) fn flag(&mut self) -> Result<bool, Malformed> {
        self.skip();
        let b = self.bytes.get(self.at).copied().ok_or(Malformed)?;
        self.at += 1;
        match b {
            b'0' => Ok(false),
            b'1' => Ok(true),
            _ => Err(Malformed),
        }
    }

    fn done(&mut self) -> bool {
        self.skip();
        self.at >= self.bytes.len()
    }
}

/// A subpath being built, in view-box units.
struct Building {
    rows: Vec<Row>,
    start: (f64, f64),
    closed: bool,
}

/// SVG path data (`svg:d`) as sections, one per subpath: `M` and `m` start a section; `L`, `H`, `V` give `RelLineTo`; `C` and `S`
/// `RelCubBezTo` (`S` reflecting the previous control point); `Q` and `T` `RelQuadBezTo`; `Z` closes back to the subpath's start, with no
/// row when the pen is there already; `A` gives `RelEllipticalArcTo` pieces. A subpath not closed by `Z` is open and so not filled.
/// `Err(Malformed)` for data that breaks the syntax; a budget passed is the read's error.
pub(crate) fn path(text: &str, vb: &ViewBox, size: (f64, f64), budget: &mut Budget) -> Result<Result<Vec<Section>, Malformed>, OdgError> {
    let mut t = Tokens::new(text);
    let mut out: Vec<Section> = Vec::new();
    let mut sub: Option<Building> = None;
    let mut pen = (0.0, 0.0);
    // The second control point of the last cubic, or the control of the last quadratic, for `S` and `T`.
    let mut last_cubic: Option<(f64, f64)> = None;
    let mut last_quad: Option<(f64, f64)> = None;
    let mut command: Option<u8> = None;
    let finish = |sub: Option<Building>, out: &mut Vec<Section>| {
        if let Some(b) = sub
            && b.rows.len() > 1
        {
            out.push(Section { rows: b.rows, no_fill: !b.closed });
        }
    };
    loop {
        if t.done() {
            break;
        }
        let c = match t.command() {
            Some(c) => c,
            None => match command {
                // A repeated command: after `M` the pairs are lines.
                Some(b'M') => b'L',
                Some(b'm') => b'l',
                Some(c) if !matches!(c, b'Z' | b'z') => c,
                _ => return Ok(Err(Malformed)),
            },
        };
        budget.take(1)?;
        command = Some(c);
        let relative = c.is_ascii_lowercase();
        let rel = |p: (f64, f64), pen: (f64, f64)| if relative { (p.0 + pen.0, p.1 + pen.1) } else { p };
        let pair = |t: &mut Tokens<'_>| -> Result<(f64, f64), Malformed> { Ok((t.number()?, t.number()?)) };
        let res: Result<(), Malformed> = (|| {
            match c.to_ascii_uppercase() {
                b'M' => {
                    let p = rel(pair(&mut t)?, pen);
                    finish(sub.take(), &mut out);
                    let s = vb.share(p.0, p.1);
                    sub = Some(Building { rows: vec![Row::RelMoveTo(s.0, s.1)], start: p, closed: false });
                    pen = p;
                    last_cubic = None;
                    last_quad = None;
                }
                b'Z' => {
                    if let Some(b) = sub.as_mut() {
                        if pen != b.start {
                            let s = vb.share(b.start.0, b.start.1);
                            b.rows.push(Row::RelLineTo(s.0, s.1));
                        }
                        b.closed = true;
                        pen = b.start;
                        let start = b.start;
                        finish(sub.take(), &mut out);
                        // A command after `Z` that is not a move starts a new subpath at the same point (SVG's rule).
                        let s = vb.share(start.0, start.1);
                        sub = Some(Building { rows: vec![Row::RelMoveTo(s.0, s.1)], start, closed: false });
                    }
                    last_cubic = None;
                    last_quad = None;
                }
                other => {
                    let b = sub.get_or_insert_with(|| {
                        let s = vb.share(pen.0, pen.1);
                        Building { rows: vec![Row::RelMoveTo(s.0, s.1)], start: pen, closed: false }
                    });
                    match other {
                        b'L' => {
                            let p = rel(pair(&mut t)?, pen);
                            let s = vb.share(p.0, p.1);
                            b.rows.push(Row::RelLineTo(s.0, s.1));
                            pen = p;
                            last_cubic = None;
                            last_quad = None;
                        }
                        b'H' => {
                            let x = t.number()?;
                            let p = (if relative { x + pen.0 } else { x }, pen.1);
                            let s = vb.share(p.0, p.1);
                            b.rows.push(Row::RelLineTo(s.0, s.1));
                            pen = p;
                            last_cubic = None;
                            last_quad = None;
                        }
                        b'V' => {
                            let y = t.number()?;
                            let p = (pen.0, if relative { y + pen.1 } else { y });
                            let s = vb.share(p.0, p.1);
                            b.rows.push(Row::RelLineTo(s.0, s.1));
                            pen = p;
                            last_cubic = None;
                            last_quad = None;
                        }
                        b'C' | b'S' => {
                            let c1 = if other == b'C' {
                                rel(pair(&mut t)?, pen)
                            } else {
                                last_cubic.map_or(pen, |(x, y)| (2.0 * pen.0 - x, 2.0 * pen.1 - y))
                            };
                            let c2 = rel(pair(&mut t)?, pen);
                            let p = rel(pair(&mut t)?, pen);
                            let (s1, s2, s) = (vb.share(c1.0, c1.1), vb.share(c2.0, c2.1), vb.share(p.0, p.1));
                            b.rows.push(Row::RelCubBezTo { x: s.0, y: s.1, a: s1.0, b: s1.1, c: s2.0, d: s2.1 });
                            pen = p;
                            last_cubic = Some(c2);
                            last_quad = None;
                        }
                        b'Q' | b'T' => {
                            let q = if other == b'Q' {
                                rel(pair(&mut t)?, pen)
                            } else {
                                last_quad.map_or(pen, |(x, y)| (2.0 * pen.0 - x, 2.0 * pen.1 - y))
                            };
                            let p = rel(pair(&mut t)?, pen);
                            let (sq, s) = (vb.share(q.0, q.1), vb.share(p.0, p.1));
                            b.rows.push(Row::RelQuadBezTo { x: s.0, y: s.1, a: sq.0, b: sq.1 });
                            pen = p;
                            last_quad = Some(q);
                            last_cubic = None;
                        }
                        b'A' => {
                            let (rx, ry) = (t.number()?, t.number()?);
                            let phi = t.number()?;
                            let (large, sweep) = (t.flag()?, t.flag()?);
                            let p = rel(pair(&mut t)?, pen);
                            arc_rows(&mut b.rows, pen, (rx, ry, phi), (large, sweep), p, vb, size);
                            pen = p;
                            last_cubic = None;
                            last_quad = None;
                        }
                        _ => return Err(Malformed),
                    }
                }
            }
            Ok(())
        })();
        // More numbers after a command repeat it; numbers after `Z`, which takes none, are refused on the next pass, as no command repeats
        // after it.
        if res.is_err() {
            return Ok(Err(Malformed));
        }
    }
    finish(sub, &mut out);
    Ok(Ok(out))
}

/// The rows of an SVG arc from `from` to `to`: the arc as SVG's implementation notes define it (centre, radii scaled up when too
/// small, F.6.5 and F.6.6), cut into pieces of at most 180 degrees, each one `RelElliptical ArcTo` with its end and its middle point (half
/// the piece's sweep of the ellipse's parameter). A radius of 0, or two equal ends, is a line or nothing, as SVG says.
fn arc_rows(rows: &mut Vec<Row>, from: (f64, f64), radii: (f64, f64, f64), flags: (bool, bool), to: (f64, f64), vb: &ViewBox, size: (f64, f64)) {
    let (mut rx, mut ry, phi_deg) = (radii.0.abs(), radii.1.abs(), radii.2);
    if from == to {
        return;
    }
    if rx == 0.0 || ry == 0.0 {
        let s = vb.share(to.0, to.1);
        rows.push(Row::RelLineTo(s.0, s.1));
        return;
    }
    let phi = phi_deg.to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();
    let (dx, dy) = ((from.0 - to.0) / 2.0, (from.1 - to.1) / 2.0);
    let (x1p, y1p) = (cos_phi * dx + sin_phi * dy, -sin_phi * dx + cos_phi * dy);
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut coef = if den == 0.0 { 0.0 } else { (num / den).max(0.0).sqrt() };
    if flags.0 == flags.1 {
        coef = -coef;
    }
    let (cxp, cyp) = (coef * rx * y1p / ry, -coef * ry * x1p / rx);
    let (cx, cy) = (cos_phi * cxp - sin_phi * cyp + (from.0 + to.0) / 2.0, sin_phi * cxp + cos_phi * cyp + (from.1 + to.1) / 2.0);
    let angle = |ux: f64, uy: f64, vx: f64, vy: f64| {
        let a = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        if a.is_finite() { a } else { 0.0 }
    };
    let theta1 = angle(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut sweep = angle((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
    if !flags.1 && sweep > 0.0 {
        sweep -= 2.0 * PI;
    } else if flags.1 && sweep < 0.0 {
        sweep += 2.0 * PI;
    }
    let point = |t: f64| {
        let (st, ct) = t.sin_cos();
        let (ex, ey) = (rx * ct, ry * st);
        (cos_phi * ex - sin_phi * ey + cx, sin_phi * ex + cos_phi * ey + cy)
    };
    let axes = if phi_deg.rem_euclid(360.0) == 0.0 {
        ArcAxes::Upright { k: (rx * vb.height) / (ry * vb.width) }
    } else {
        let (c0, d0) = frame_axes(rx, ry, phi, vb, size);
        ArcAxes::Turned { c0, d0 }
    };
    let pieces = ((sweep.abs() / PI) - 1e-9).ceil().max(1.0) as usize;
    let step = sweep / pieces as f64;
    for i in 0..pieces {
        let (t0, t1) = (theta1 + step * i as f64, theta1 + step * (i + 1) as f64);
        let end = if i + 1 == pieces { to } else { point(t1) };
        let mid = point((t0 + t1) / 2.0);
        let (se, sm) = (vb.share(end.0, end.1), vb.share(mid.0, mid.1));
        rows.push(Row::RelEllipticalArcTo { x: se.0, y: se.1, a: sm.0, b: sm.1, axes });
    }
}

/// The axis angle and ratio, in the frame at its size (inches, y up), of a view-box ellipse of half-axes `rx`, `ry` turned `phi`: the
/// image of the ellipse under the view box's map, whose axes are the singular vectors of that map's linear part (the `c0`, `d0`).
fn frame_axes(rx: f64, ry: f64, phi: f64, vb: &ViewBox, size: (f64, f64)) -> (f64, f64) {
    let (sx, sy) = (size.0 / vb.width, -size.1 / vb.height);
    let (s, c) = phi.sin_cos();
    // M = diag(sx, sy) * R(phi) * diag(rx, ry).
    let (a, b, cc, d) = (sx * c * rx, -sx * s * ry, sy * s * rx, sy * c * ry);
    let (e, f, g, h) = ((a + d) / 2.0, (a - d) / 2.0, (cc + b) / 2.0, (cc - b) / 2.0);
    let (q, r) = (e.hypot(h), f.hypot(g));
    let (major, minor) = (q + r, (q - r).abs());
    let beta = (g.atan2(f) + h.atan2(e)) / 2.0;
    let ratio = if minor > 0.0 { major / minor } else { 1.0 };
    (if beta.is_finite() { beta } else { 0.0 }, if ratio.is_finite() { ratio } else { 1.0 })
}

/// A point of a circle of the kinds: the angle `t` degrees, from the right-middle, counterclockwise as seen, as a share of the box
/// `(x0, y0, w, h)` given in shares of the frame.
fn kind_point(t: f64, square: (f64, f64, f64, f64)) -> (f64, f64) {
    let (x0, y0, w, h) = square;
    let (s, c) = t.to_radians().sin_cos();
    (x0 + w * (0.5 + 0.5 * c), y0 + h * (0.5 + 0.5 * s))
}

/// The kinds of `draw:ellipse` and `draw:circle` other than `full`: `section` a slice from the centre, `cut` a
/// segment closed by its chord, `arc` the open arc (`NoFill` 1). `square` is the box of the curve in shares of the frame (the whole frame for
/// an ellipse, the circle's centred square for a circle), and `ratio` the length of its x axis over its y axis. The arc runs from `start`
/// to `end` degrees counterclockwise; equal angles are no slice, and `None` says the shape is read as a full one.
pub(crate) fn kind(kind: &str, start: f64, end: f64, square: (f64, f64, f64, f64), ratio: f64) -> Option<Vec<Section>> {
    let mut sweep = (end - start).rem_euclid(360.0);
    if sweep == 0.0 {
        return None;
    }
    if !sweep.is_finite() {
        sweep = 360.0;
    }
    let from = kind_point(start, square);
    let to = kind_point(start + sweep, square);
    let mid = kind_point(start + sweep / 2.0, square);
    let arc = Row::RelEllipticalArcTo { x: to.0, y: to.1, a: mid.0, b: mid.1, axes: ArcAxes::Values { d: ratio } };
    let centre = (square.0 + square.2 / 2.0, square.1 + square.3 / 2.0);
    let section = match kind {
        "section" => Section {
            rows: vec![Row::RelMoveTo(centre.0, centre.1), Row::RelLineTo(from.0, from.1), arc, Row::RelLineTo(centre.0, centre.1)],
            no_fill: false,
        },
        "cut" => Section { rows: vec![Row::RelMoveTo(from.0, from.1), arc, Row::RelLineTo(from.0, from.1)], no_fill: false },
        "arc" => Section { rows: vec![Row::RelMoveTo(from.0, from.1), arc], no_fill: true },
        _ => return None,
    };
    Some(vec![section])
}

/// `draw:regular-polygon`: `corners` points evenly spaced on a circle, or with `concave` twice as many alternating with a circle of
/// `1 - sharpness` the radius, the first at the top (of the inner circle for an even concave one), counterclockwise as seen, the box of the
/// points scaled to the frame, as one closed section.
pub(crate) fn regular_polygon(corners: u32, concave: bool, sharpness: f64) -> Option<Vec<Section>> {
    if corners < 3 {
        return None;
    }
    let count = if concave { corners.checked_mul(2)? } else { corners };
    let inner = (1.0 - sharpness).clamp(0.0, 1.0);
    let first_inner = concave && corners.is_multiple_of(2);
    let mut points = Vec::with_capacity(count as usize);
    for k in 0..count {
        let angle = PI / 2.0 + 2.0 * PI * f64::from(k) / f64::from(count);
        let on_inner = concave && ((k % 2 == 0) == first_inner);
        let r = if on_inner { inner } else { 1.0 };
        points.push((r * angle.cos(), r * angle.sin()));
    }
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for (x, y) in &points {
        x0 = x0.min(*x);
        x1 = x1.max(*x);
        y0 = y0.min(*y);
        y1 = y1.max(*y);
    }
    let (w, h) = (x1 - x0, y1 - y0);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let share = |(x, y): (f64, f64)| ((x - x0) / w, (y - y0) / h);
    let mut rows: Vec<Row> = Vec::with_capacity(points.len() + 1);
    for (i, p) in points.iter().enumerate() {
        let (sx, sy) = share(*p);
        rows.push(if i == 0 { Row::RelMoveTo(sx, sy) } else { Row::RelLineTo(sx, sy) });
    }
    let (sx, sy) = share(*points.first()?);
    rows.push(Row::RelLineTo(sx, sy));
    Some(vec![Section { rows, no_fill: false }])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vb(w: f64, h: f64) -> ViewBox {
        ViewBox { left: 0.0, top: 0.0, width: w, height: h }
    }

    #[test]
    fn numbers_run_together_and_flags_compress() {
        let mut budget = Budget::new(100);
        let sections = path("M0,0L10-5.5.5 1Z", &vb(10.0, 10.0), (1.0, 1.0), &mut budget).unwrap().unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].rows.len(), 4);
        let arcs = path("M0 5a5 5 0 0110 0", &vb(10.0, 10.0), (1.0, 1.0), &mut budget).unwrap().unwrap();
        assert!(matches!(arcs[0].rows[1], Row::RelEllipticalArcTo { .. }));
        assert!(path("M0 0 L", &vb(1.0, 1.0), (1.0, 1.0), &mut budget).unwrap().is_err());
        assert!(path("Q 1", &vb(1.0, 1.0), (1.0, 1.0), &mut budget).unwrap().is_err());
        assert!(path("M0 0 L1 1 Z 2 2", &vb(1.0, 1.0), (1.0, 1.0), &mut budget).unwrap().is_err(), "no numbers after Z");
    }

    #[test]
    fn numbers_take_exponents_and_signs_as_svg_writes_them() {
        let mut budget = Budget::new(100);
        let sections = path("M0 0 L1e1 5E-1 l-2.5e+0,+1.5", &vb(10.0, 10.0), (1.0, 1.0), &mut budget).unwrap().unwrap();
        let rows = &sections[0].rows;
        assert_eq!(rows[1], Row::RelLineTo(1.0, 1.0 - 0.05));
        assert_eq!(rows[2], Row::RelLineTo(0.75, 1.0 - 0.2));
        // An `e` with no digits after it is not an exponent.
        assert!(path("M0 0 L1e 2", &vb(10.0, 10.0), (1.0, 1.0), &mut budget).unwrap().is_err());
        // A fraction of several digits is one number.
        let sections = path("M0 0 L2.375 7.5", &vb(10.0, 10.0), (1.0, 1.0), &mut budget).unwrap().unwrap();
        assert_eq!(sections[0].rows[1], Row::RelLineTo(0.2375, 1.0 - 0.75));
    }

    #[test]
    fn pairs_after_a_move_are_lines() {
        let mut budget = Budget::new(100);
        let absolute = path("M0 0 10 0 10 10", &vb(10.0, 10.0), (1.0, 1.0), &mut budget).unwrap().unwrap();
        assert_eq!(absolute.len(), 1);
        assert_eq!(absolute[0].rows, vec![Row::RelMoveTo(0.0, 1.0), Row::RelLineTo(1.0, 1.0), Row::RelLineTo(1.0, 0.0)]);
        let relative = path("m1 1 2 0 0 2", &vb(10.0, 10.0), (1.0, 1.0), &mut budget).unwrap().unwrap();
        assert_eq!(relative[0].rows, vec![Row::RelMoveTo(0.1, 0.9), Row::RelLineTo(0.3, 0.9), Row::RelLineTo(0.3, 0.7)]);
        assert!(relative[0].no_fill, "open");
    }

    /// Asserts a row's numbers within 1E-12, whatever its kind.
    fn near(row: &Row, want: &Row) {
        let numbers = |r: &Row| -> Vec<f64> {
            match *r {
                Row::RelMoveTo(x, y) | Row::RelLineTo(x, y) => vec![x, y],
                Row::RelCubBezTo { x, y, a, b, c, d } => vec![x, y, a, b, c, d],
                Row::RelQuadBezTo { x, y, a, b } => vec![x, y, a, b],
                Row::RelEllipticalArcTo { x, y, a, b, .. } => vec![x, y, a, b],
            }
        };
        assert_eq!(std::mem::discriminant(row), std::mem::discriminant(want), "{row:?} is not {want:?}");
        for (got, w) in numbers(row).iter().zip(numbers(want)) {
            assert!((got - w).abs() < 1e-12, "{row:?} is not {want:?}");
        }
    }

    #[test]
    fn every_command_absolute_and_relative_with_the_reflections() {
        let mut budget = Budget::new(100);
        let rows = &path("M0 0 H5 V5 h1 v1 C1 2 3 4 5 6 S7 8 9 10 Q1 1 2 2 T3 3", &vb(10.0, 10.0), (1.0, 1.0), &mut budget).unwrap().unwrap()[0].rows;
        let want = [
            Row::RelMoveTo(0.0, 1.0),
            Row::RelLineTo(0.5, 1.0),
            Row::RelLineTo(0.5, 0.5),
            Row::RelLineTo(0.6, 0.5),
            Row::RelLineTo(0.6, 0.4),
            Row::RelCubBezTo { x: 0.5, y: 0.4, a: 0.1, b: 0.8, c: 0.3, d: 0.6 },
            Row::RelCubBezTo { x: 0.9, y: 0.0, a: 0.7, b: 0.2, c: 0.7, d: 0.2 },
            Row::RelQuadBezTo { x: 0.2, y: 0.8, a: 0.1, b: 0.9 },
            Row::RelQuadBezTo { x: 0.3, y: 0.7, a: 0.3, b: 0.7 },
        ];
        assert_eq!(rows.len(), want.len());
        for (row, w) in rows.iter().zip(&want) {
            near(row, w);
        }
        let rows = &path("M10 10 c1 1 2 2 3 3 s1 1 2 2 q1 1 2 2 t1 1", &vb(20.0, 20.0), (1.0, 1.0), &mut budget).unwrap().unwrap()[0].rows;
        let s = |v: f64| v / 20.0;
        let f = |v: f64| 1.0 - v / 20.0;
        let want = [
            Row::RelMoveTo(s(10.0), f(10.0)),
            Row::RelCubBezTo { x: s(13.0), y: f(13.0), a: s(11.0), b: f(11.0), c: s(12.0), d: f(12.0) },
            Row::RelCubBezTo { x: s(15.0), y: f(15.0), a: s(14.0), b: f(14.0), c: s(14.0), d: f(14.0) },
            Row::RelQuadBezTo { x: s(17.0), y: f(17.0), a: s(16.0), b: f(16.0) },
            Row::RelQuadBezTo { x: s(18.0), y: f(18.0), a: s(18.0), b: f(18.0) },
        ];
        for (row, w) in rows.iter().zip(&want) {
            near(row, w);
        }
        // A `S` or `T` with no curve before it takes the pen as its first control.
        let rows = &path("M0 0 S2 2 4 0 T8 0", &vb(10.0, 10.0), (1.0, 1.0), &mut budget).unwrap().unwrap()[0].rows;
        near(&rows[1], &Row::RelCubBezTo { x: 0.4, y: 1.0, a: 0.0, b: 1.0, c: 0.2, d: 0.8 });
        near(&rows[2], &Row::RelQuadBezTo { x: 0.8, y: 1.0, a: 0.4, b: 1.0 });
    }

    #[test]
    fn arcs_scale_too_small_radii_and_cut_long_sweeps() {
        let mut budget = Budget::new(100);
        // Radii of 1 between ends 10 apart are scaled up to 5: a half circle over the top (sweep 1 runs clockwise on the y-down page).
        let rows = &path("M0 5 A1 1 0 0 1 10 5", &vb(10.0, 10.0), (1.0, 1.0), &mut budget).unwrap().unwrap()[0].rows;
        assert_eq!(rows.len(), 2);
        match rows[1] {
            Row::RelEllipticalArcTo { x, y, a, b, axes } => {
                assert!((x - 1.0).abs() < 1e-12 && (y - 0.5).abs() < 1e-12);
                assert!((a - 0.5).abs() < 1e-12 && (b - 1.0).abs() < 1e-12, "({a}, {b})");
                assert_eq!(axes, ArcAxes::Upright { k: 1.0 });
            }
            other => panic!("{other:?}"),
        }
        // The other sweep goes under.
        let rows = &path("M0 5 A5 5 0 0 0 10 5", &vb(10.0, 10.0), (1.0, 1.0), &mut budget).unwrap().unwrap()[0].rows;
        assert!(matches!(rows[1], Row::RelEllipticalArcTo { b, .. } if b.abs() < 1e-12));
        // Three quarters of the circle about (0, 0), the large way round clockwise: two pieces of 135 degrees, the first ending on the
        // circle at (-5 cos 45, 5 sin 45) from its start's side, and its middle point there too.
        let rows = &path("M0 5 A5 5 0 1 1 5 0", &vb(10.0, 10.0), (1.0, 1.0), &mut budget).unwrap().unwrap()[0].rows;
        assert_eq!(rows.len(), 3);
        for row in rows.iter().skip(1) {
            if let Row::RelEllipticalArcTo { x, y, a, b, .. } = *row {
                for (u, v) in [(x, y), (a, b)] {
                    let (px, py) = (u * 10.0, (1.0 - v) * 10.0);
                    assert!((px.hypot(py) - 5.0).abs() < 1e-9, "({px}, {py}) on the circle");
                }
            } else {
                panic!("{row:?}");
            }
        }
        // A radius of 0 is a line, and equal ends draw nothing.
        let rows = &path("M0 5 A0 5 0 0 1 10 5 A5 5 0 0 1 10 5", &vb(10.0, 10.0), (1.0, 1.0), &mut budget).unwrap().unwrap()[0].rows;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1], Row::RelLineTo(1.0, 0.5));
        // An ellipse turned 30 degrees: C and D from its image in the frame.
        let rows = &path("M0 5 A5 2 30 0 1 10 5", &vb(10.0, 10.0), (2.0, 1.0), &mut budget).unwrap().unwrap()[0].rows;
        assert!(matches!(rows.last(), Some(Row::RelEllipticalArcTo { axes: ArcAxes::Turned { .. }, .. })));
    }

    #[test]
    fn the_budget_is_charged_per_command() {
        let mut budget = Budget::new(3);
        assert!(path("M0 0 L1 1 L2 2 L3 3", &vb(1.0, 1.0), (1.0, 1.0), &mut budget).is_err());
        let mut budget = Budget::new(4);
        assert!(path("M0 0 L1 1 2 2 3 3", &vb(1.0, 1.0), (1.0, 1.0), &mut budget).unwrap().is_ok());
    }

    #[test]
    fn a_turned_arc_s_axes_are_the_image_s() {
        // A circle stays a circle under a uniform map, so its ratio is 1 whatever the turn.
        let (_, d) = frame_axes(1.0, 1.0, 0.3, &vb(10.0, 10.0), (2.0, 2.0));
        assert!((d - 1.0).abs() < 1e-12);
        // An ellipse 2 by 1 turned 90 degrees in a square box: its long axis runs along y.
        let (c, d) = frame_axes(2.0, 1.0, PI / 2.0, &vb(10.0, 10.0), (1.0, 1.0));
        assert!((d - 2.0).abs() < 1e-9, "{d}");
        assert!((c.abs() - PI / 2.0).abs() < 1e-9, "{c}");
    }

    #[test]
    fn a_turned_arc_s_axes_in_a_frame_of_another_shape_are_the_image_s() {
        // An ellipse of half-axes 2.5 and 1.5 turned 30 degrees, in a view box 10 by 20 drawn on a frame 3 by 1.5 inches: its image's axes,
        // found by walking the image round and taking its longest and shortest radii.
        let (rx, ry, phi) = (2.5, 1.5, 30f64.to_radians());
        let (view, size) = (vb(10.0, 20.0), (3.0, 1.5));
        let (c, d) = frame_axes(rx, ry, phi, &view, size);
        let (sx, sy) = (size.0 / 10.0, -size.1 / 20.0);
        let (s, co) = phi.sin_cos();
        let image = |t: f64| {
            let (x, y) = (rx * t.cos(), ry * t.sin());
            (sx * (co * x - s * y), sy * (s * x + co * y))
        };
        let steps = 200_000;
        let (mut most, mut least, mut at_most) = (0.0f64, f64::MAX, (0.0, 0.0));
        for k in 0..steps {
            let p = image(PI * f64::from(k) / f64::from(steps));
            let length = p.0.hypot(p.1);
            if length > most {
                most = length;
                at_most = p;
            }
            least = least.min(length);
        }
        assert!((d - most / least).abs() < 1e-6, "{d} against {}", most / least);
        // The axis angle is the long axis's, up to a half turn.
        let off = (c - at_most.1.atan2(at_most.0)).rem_euclid(PI);
        assert!(off.min(PI - off) < 1e-4, "{c} against {}", at_most.1.atan2(at_most.0));
    }

    /// The end and middle point of each arc row, in view-box units.
    fn arc_points(d: &str) -> Vec<(f64, f64, f64, f64)> {
        let mut budget = Budget::new(100);
        let sections = path(d, &vb(20.0, 20.0), (1.0, 1.0), &mut budget).unwrap().unwrap();
        sections[0]
            .rows
            .iter()
            .filter_map(|row| match *row {
                Row::RelEllipticalArcTo { x, y, a, b, .. } => Some((x * 20.0, (1.0 - y) * 20.0, a * 20.0, (1.0 - b) * 20.0)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_arc_between_ends_that_are_not_level_turns_its_axes_and_not_its_circle() {
        // On a circle the axis angle changes nothing: the arc from (0, 0) to (6, 8) of radius 10 is the same turned 30 degrees or not.
        let level = arc_points("M0 0 A10 10 0 0 1 6 8");
        let turned = arc_points("M0 0 A10 10 30 0 1 6 8");
        assert_eq!(level.len(), 1);
        assert_eq!(turned.len(), 1);
        let (l, t) = (level[0], turned[0]);
        for (p, q) in [(l.0, t.0), (l.1, t.1), (l.2, t.2), (l.3, t.3)] {
            assert!((p - q).abs() < 1e-9, "{level:?} {turned:?}");
        }
        // The middle point lies on the circle: 10 from the centre, which is 10 from both ends.
        let chord = (36.0f64 + 64.0).sqrt();
        let back = (100.0 - (chord / 2.0).powi(2)).sqrt();
        // Sweep 1 with the small arc: the centre is to the right of the way from (0, 0) to (6, 8) on the y-down page.
        let (ux, uy) = (6.0 / chord, 8.0 / chord);
        let centre = (3.0 - uy * back, 4.0 + ux * back);
        assert!(((l.2 - centre.0).hypot(l.3 - centre.1) - 10.0).abs() < 1e-9, "{l:?} {centre:?}");
        // Sweep 0 with the small arc: the mirror image across the chord, whose line runs through (0, 0).
        let other = arc_points("M0 0 A10 10 0 0 0 6 8")[0];
        let along = l.2 * ux + l.3 * uy;
        let mirrored = (2.0 * along * ux - l.2, 2.0 * along * uy - l.3);
        assert!((other.2 - mirrored.0).abs() < 1e-9 && (other.3 - mirrored.1).abs() < 1e-9, "{other:?} {mirrored:?}");
        // The large arc with sweep 1: about the centre on the other side, in two pieces of under 180 degrees, ending at (6, 8).
        let large = arc_points("M0 0 A10 10 0 1 1 6 8");
        assert_eq!(large.len(), 2);
        let far = (3.0 + uy * back, 4.0 - ux * back);
        for piece in &large {
            assert!(((piece.2 - far.0).hypot(piece.3 - far.1) - 10.0).abs() < 1e-9, "{large:?}");
        }
        assert!((large[1].0 - 6.0).abs() < 1e-9 && (large[1].1 - 8.0).abs() < 1e-9);
        // The large arc turns 300 degrees, the way SVG's sweep 1 turns (the positive angle on the y-down page): the first piece ends 150
        // degrees round from (0, 0), the second at 300, and each piece's middle point is half way along it.
        let turned_by = |degrees: f64| {
            let (s, c) = degrees.to_radians().sin_cos();
            let v = (0.0 - far.0, 0.0 - far.1);
            (far.0 + v.0 * c - v.1 * s, far.1 + v.0 * s + v.1 * c)
        };
        for (piece, (middle, end)) in large.iter().zip([(75.0, 150.0), (225.0, 300.0)]) {
            let (m, e) = (turned_by(middle), turned_by(end));
            assert!((piece.2 - m.0).abs() < 1e-9 && (piece.3 - m.1).abs() < 1e-9, "{piece:?} middle {m:?}");
            assert!((piece.0 - e.0).abs() < 1e-9 && (piece.1 - e.1).abs() < 1e-9, "{piece:?} end {e:?}");
        }
        // The large arc with sweep 0 is its mirror image across the chord, piece by piece.
        let large_other = arc_points("M0 0 A10 10 0 1 0 6 8");
        assert_eq!(large_other.len(), 2);
        for (p, q) in large.iter().zip(&large_other) {
            for (a, b) in [((p.0, p.1), (q.0, q.1)), ((p.2, p.3), (q.2, q.3))] {
                let along = a.0 * ux + a.1 * uy;
                let mirrored = (2.0 * along * ux - a.0, 2.0 * along * uy - a.1);
                assert!((b.0 - mirrored.0).abs() < 1e-9 && (b.1 - mirrored.1).abs() < 1e-9, "{large:?} {large_other:?}");
            }
        }
        // Radii too small between those ends: scaled up to half the chord, 5, about the chord's middle (3, 4), turned or not.
        for d in ["M0 0 A1 1 0 0 1 6 8", "M0 0 A1 1 30 0 1 6 8"] {
            let half = arc_points(d)[0];
            assert!(((half.2 - 3.0).hypot(half.3 - 4.0) - 5.0).abs() < 1e-9, "{d}: {half:?}");
        }
    }
}
