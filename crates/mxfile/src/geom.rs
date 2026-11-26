//! Lengths, points, boxes and the places of cells.
//!
//! draw.io's coordinates are **px**, `x` to the right and `y` down, 100 px to the inch. A [`Px`] here is a length that the reader has made safe: **finite, and within the
//! limits' `max_coord_px` (10,000,000)**, so that everything built on it, sums of ancestors' places included, stays a number a model can hold. The small functions are pure and
//! total on every float, NaN and infinity included, so that a proof can hold them (design §9).

/// A length or a coordinate in draw.io's px: finite and within the cap (`Limits::max_coord_px`), by [`clamp_to`] and by every place the reader makes.
pub type Px = f64;

/// Pixels in an inch: draw.io counts 100 px to the inch.
pub const PX_PER_IN: f64 = 100.0;

/// A point, in px.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pt {
    /// To the right.
    pub x: Px,
    /// Down.
    pub y: Px,
}

/// A box, in px: its top-left corner and its size. `width` and `height` are never below 0.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    /// The left side.
    pub x: Px,
    /// The top.
    pub y: Px,
    /// The width.
    pub width: Px,
    /// The height.
    pub height: Px,
}

impl Rect {
    /// The top-left corner.
    pub fn origin(&self) -> Pt {
        Pt { x: self.x, y: self.y }
    }

    /// The middle of the box.
    pub fn centre(&self) -> Pt {
        Pt { x: self.x + self.width * 0.5, y: self.y + self.height * 0.5 }
    }
}

/// What a cell's `mxGeometry` says, as the file states it with the repairs: every number finite and within the cap, a width or height never below 0, missing numbers 0.
/// The places are in the cell's **parent's coordinates**; [`Cell::frame`](crate::cells::Cell::frame) is where that parent's origin is on the page.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Geometry {
    /// The left side of a shape, or the anchor's `x` of a relative geometry (-1 to 1 for a connector's label, a share of the parent's width for a port).
    pub x: f64,
    /// The top of a shape, or the anchor's `y` of a relative geometry.
    pub y: f64,
    /// The width.
    pub width: Px,
    /// The height.
    pub height: Px,
    /// `relative="1"`: `x` and `y` are shares of the parent and not px (a port, a connector's label).
    pub relative: bool,
    /// `mxPoint as="offset"`: added to a relative place, and, for an absolute shape, the label's move only. 0, 0 when missing.
    pub offset: Pt,
    /// `mxPoint as="sourcePoint"`: where a connector begins when its source is not a shape.
    pub source_point: Option<Pt>,
    /// `mxPoint as="targetPoint"`: where a connector ends when its target is not a shape.
    pub target_point: Option<Pt>,
    /// `Array as="points"`: a connector's waypoints in order, at most `Limits::max_points`.
    pub points: Vec<Pt>,
    /// `mxRectangle as="alternateBounds"`: a container's box in its other state.
    pub alternate_bounds: Option<Rect>,
}

/// Whether `text` (already trimmed) is `[+-]? (digits ('.' digits?)? | '.' digits) ([eE] [+-]? digits)?`: a decimal number in any of the forms a number is written, and nothing else.
fn is_decimal(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let digits = |from: usize| bytes.get(from..).map_or(0, |rest| rest.iter().take_while(|b| b.is_ascii_digit()).count());
    let whole = digits(i);
    i += whole;
    let mut fraction = 0;
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        fraction = digits(i);
        i += fraction;
    }
    if whole == 0 && fraction == 0 {
        return false;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        i += usize::from(matches!(bytes.get(i), Some(b'+' | b'-')));
        let exponent = digits(i);
        if exponent == 0 {
            return false;
        }
        i += exponent;
    }
    i == bytes.len()
}

/// `text` as a decimal number, or `None` when it is not a finite one: the value trimmed of white space, then `[+-]digits[.digits][e[+-]digits]` in any of the forms of a decimal
/// number (`.5`, `5.`, `1e3`), and nothing else (no `0x`, no `inf`, no `NaN`, no separators, no unit). Text longer than `max_bytes` is no number (the limits'
/// `max_number_bytes`), white space counted. The value of the numbers; S-R5 reads a style's numbers by the same rule. `-0` is 0.
pub fn parse_px(text: &str, max_bytes: usize) -> Option<f64> {
    if text.len() > max_bytes {
        return None;
    }
    let trimmed = text.trim_matches(|c: char| c.is_ascii_whitespace());
    if !is_decimal(trimmed) {
        return None;
    }
    // A number written out in full that is too large for a float is infinity, which is no number here.
    trimmed.parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| v + 0.0)
}

/// `v` made safe for a length: 0 when it is not a finite number, else within `-max` to `max`, with -0 made 0. A `max` that is not a finite number above 0 gives 0.
pub fn clamp_to(v: f64, max: f64) -> f64 {
    if !v.is_finite() || !max.is_finite() || max <= 0.0 {
        return 0.0;
    }
    v.clamp(-max, max) + 0.0
}

/// [`clamp_to`] at the specification's cap, 10,000,000 px.
pub fn clamp_px(v: f64) -> f64 {
    clamp_to(v, crate::limits::MAX_COORD_PX)
}

/// `px` in inches: a hundredth of an inch to the px, of [`clamp_px`], so finite and within 100,000 inches for any input. It divides by 100 and does not multiply by
/// 0.01, which is not a float of its own and would put a hundredth of an ulp in every length.
pub fn px_to_in(px: f64) -> f64 {
    clamp_px(px) / PX_PER_IN
}

/// The fraction of a connector's route that a label's `x` names: (`x` + 1) / 2, within 0 to 1, and 0.5 for anything that is not a number.
pub fn anchor_t(x: f64) -> f64 {
    if x.is_nan() {
        return 0.5;
    }
    ((x + 1.0) / 2.0).clamp(0.0, 1.0)
}

/// A page point in px as the model has it: `(0.01 * (x - OX), PH - 0.01 * (y - OY))` inches, for the page origin `origin` (`OX`, `OY`) in px and the page's height
/// `page_height_in` in inches. The model's y grows up from the bottom of the page. For points within the cap, so that the answer is a number.
pub fn to_model(p: Pt, origin: Pt, page_height_in: f64) -> (f64, f64) {
    ((p.x - origin.x) / PX_PER_IN, page_height_in - (p.y - origin.y) / PX_PER_IN)
}

/// `v` held within `max` like [`clamp_to`], and whether it had to be: an overflow past the cap is held at it, and a value that is no number is 0 (both are "held").
fn hold(v: f64, max: f64) -> (f64, bool) {
    let held = clamp_to(v, max);
    (held, held != v)
}

/// The box of a shape or a port on the page, from where its parent's origin is (`frame`), the size of the parent's box (`parent`, 0 by 0 for a parent that has none: a layer,
/// a cell that is neither vertex nor edge) and the cell's own `geo`:
///
/// * **absolute** (`relative` not 1): the corner is `frame` plus (`x`, `y`);
/// * **relative** (a port): the corner is `frame` plus (`x` × the parent's width, `y` × the parent's height) plus `offset`.
///
/// The size is the geometry's, not below 0. Every number is held within `max`, and the answer says whether any was held.
pub fn page_box(frame: Pt, parent: (Px, Px), geo: &Geometry, max: f64) -> (Rect, bool) {
    let (x, y) = if geo.relative {
        (frame.x + geo.x * parent.0 + geo.offset.x, frame.y + geo.y * parent.1 + geo.offset.y)
    } else {
        (frame.x + geo.x, frame.y + geo.y)
    };
    let ((x, hx), (y, hy)) = (hold(x, max), hold(y, max));
    let ((width, hw), (height, hh)) = (hold(geo.width.max(0.0), max), hold(geo.height.max(0.0), max));
    (Rect { x, y, width, height }, hx || hy || hw || hh)
}
