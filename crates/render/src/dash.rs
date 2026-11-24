//! Line patterns, caps and joins.
//!
//! **A dash is a multiple of the line's weight.** The pattern's marks and gaps are whole numbers of weights, so a 3-point line
//! dashes three times as long as a 1-point one, on paper, whatever the drawing's scale. A renderer that keeps the lengths in points draws
//! every weight's dashes alike.
//!
//! **A line thinner than its target can show** is drawn at the narrowest width the target has, and its dashes stay in units of its own
//! weight ([`Widths`]).

use kurbo::{Cap, Join, Stroke};

use crate::limits::{MAX_DASHES, MIN_DASH_PERIOD_PX};

/// The mitre limit: the length of a mitre over the weight, past which a corner is bevelled. Files give 3 on every stroke.
pub(crate) const MITRE_LIMIT: f64 = 3.0;

/// Marks and gaps of `LinePattern` 2 to 23, in weights, alternating mark, gap. A mark of 0 is a dot.
pub(crate) fn pattern_list(number: i64) -> Option<&'static [f64]> {
    Some(match number {
        2 => &[7.0, 5.0],
        3 => &[0.0, 5.0],
        4 => &[7.0, 5.0, 0.0, 5.0],
        5 => &[7.0, 5.0, 0.0, 5.0, 0.0, 5.0],
        6 => &[7.0, 5.0, 7.0, 5.0, 0.0, 5.0],
        7 => &[19.0, 5.0, 7.0, 5.0],
        8 => &[19.0, 5.0, 7.0, 5.0, 7.0, 5.0],
        9 => &[3.0, 3.0],
        10 => &[0.0, 3.0],
        11 => &[3.0, 3.0, 0.0, 3.0],
        12 => &[3.0, 3.0, 0.0, 3.0, 0.0, 3.0],
        13 => &[3.0, 3.0, 3.0, 3.0, 0.0, 3.0],
        14 => &[9.0, 3.0, 3.0, 3.0],
        15 => &[9.0, 3.0, 3.0, 3.0, 3.0, 3.0],
        16 => &[15.0, 9.0],
        17 => &[0.0, 9.0],
        18 => &[15.0, 9.0, 0.0, 9.0],
        19 => &[15.0, 9.0, 0.0, 9.0, 0.0, 9.0],
        20 => &[15.0, 9.0, 15.0, 9.0, 0.0, 9.0],
        21 => &[39.0, 9.0, 15.0, 9.0],
        22 => &[39.0, 9.0, 15.0, 9.0, 15.0, 9.0],
        23 => &[1.0, 2.0],
        _ => return None,
    })
}

/// The cap and join of a `LineCap`: 0 is a round cap with a round join; 1 is a flat cap and 2 a projecting square one,
/// both with a mitre join. (The names the sources give 1 and 2, square and extended, are the other way round.)
pub(crate) fn cap_and_join(line_cap: u8) -> (Cap, Join) {
    match line_cap {
        1 => (Cap::Butt, Join::Miter),
        2 => (Cap::Square, Join::Miter),
        _ => (Cap::Round, Join::Round),
    }
}

/// How wide a line is on paper, in each sense a drawing needs.
///
/// **A line is drawn at least as wide as its target can show.** A raster target cannot draw a stroke narrower than a pixel at its weight:
/// the stroke comes out as a faint grey a third as dark as the line's colour. So it draws one a pixel wide, in the line's full colour, and
/// the weight stays what it was for everything else: the size of a line end and the unit of the dashes are the line's
/// own weight, not the width it is drawn at. A writer of SVG or PDF draws the weight as it is and leaves the pixels to the reader.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Widths {
    /// The weight the cell holds, in paper inches, or 0 for a weight of 0 (the unit `S` of a line end takes weight 0).
    pub weight: f64,
    /// The unit of the dash lists, in paper inches: the weight, or for a weight of 0 a quarter point (dash lists take 0.25 points).
    pub unit: f64,
    /// The width the stroke is drawn at, in paper inches.
    pub drawn: f64,
}

/// A quarter point, in paper inches: what a weight of 0 stands for in a vector export, and in the dash lists of a raster.
const ZERO_WEIGHT: f64 = 0.25 / 72.0;

impl Widths {
    /// The widths of a line of `weight` paper inches (not negative) on a target whose narrowest stroke is `hairline` paper inches (0 for none).
    ///
    /// A weight of 0 draws one device pixel on a target that has one, at any zoom, and a quarter point on one that has not.
    pub(crate) fn of(weight: f64, hairline: f64) -> Widths {
        let weight = if weight.is_finite() && weight > 0.0 { weight } else { 0.0 };
        let hairline = if hairline.is_finite() && hairline > 0.0 { hairline } else { 0.0 };
        if weight > 0.0 {
            Widths { weight, unit: weight, drawn: weight.max(hairline) }
        } else {
            Widths { weight: 0.0, unit: ZERO_WEIGHT, drawn: if hairline > 0.0 { hairline } else { ZERO_WEIGHT } }
        }
    }
}

/// How a line is stroked.
pub(crate) struct StrokePlan {
    pub stroke: Stroke,
    /// Whether the pattern was dropped because it would make too many dashes.
    pub solid_for_count: bool,
    /// How many dashes the stroke makes, rounded up: 0 for a solid line (the work of the stroke).
    pub dashes: u64,
}

/// The stroke of a line of `widths` with `pattern`, for a path `length` long (paper inches) drawn at `px_per_inch`.
///
/// The table's lists are for a round or projecting cap, which adds half a weight past each end of a mark; under a flat cap each mark is a
/// weight longer and each gap a weight shorter, so that the mark as seen is the same length under all three caps.
///
/// **A line drawn wider than its unit keeps the marks and gaps the pattern gives it, as seen**. The lists are in units of
/// the line's own weight, not of the width it is drawn at: the same marks fall at the same places on paper in the window, in a PNG, and in an
/// SVG or a PDF, and widening the ink changes how thick the line is and not where its marks are. A cap adds half the drawn width at each end
/// of a mark, so each mark is cut by the width the cap adds beyond the one the list counted, and the gap that follows is lengthened by the
/// same, which keeps the period, and the mark as seen, what the pattern says.
pub(crate) fn plan(widths: Widths, pattern: i64, line_cap: u8, length: f64, px_per_inch: f64) -> StrokePlan {
    let (cap, join) = cap_and_join(line_cap);
    let mut stroke = Stroke::new(widths.drawn).with_caps(cap).with_join(join).with_miter_limit(MITRE_LIMIT);
    let Some(list) = pattern_list(pattern) else {
        return StrokePlan { stroke, solid_for_count: false, dashes: 0 };
    };
    let unit = widths.unit;
    if !unit.is_finite() || unit <= 0.0 {
        return StrokePlan { stroke, solid_for_count: false, dashes: 0 };
    }
    let flat = cap == Cap::Butt;
    // What a round or projecting cap adds to a mark beyond the weight the list counted on: nothing when the line is drawn at its weight.
    let extra = if flat { 0.0 } else { (widths.drawn - unit).max(0.0) };
    // A zero-length mark is a dot; a mark of a thousandth of a weight draws one under a round cap, and under a flat cap the table's own
    // longer form (one weight) is a square dot.
    let floor = unit * 1.0e-3;
    let mut carry = 0.0;
    let dashes: Vec<f64> = list
        .iter()
        .enumerate()
        .map(|(i, &n)| {
            let len = if i % 2 == 0 {
                if flat {
                    (n + 1.0) * unit
                } else {
                    let mark = (n * unit - extra).max(floor);
                    carry = (n * unit - mark).max(0.0);
                    mark
                }
            } else if flat {
                (n - 1.0).max(0.0) * unit
            } else {
                n * unit + std::mem::take(&mut carry)
            };
            len.max(floor)
        })
        .collect();
    let period: f64 = dashes.iter().sum();
    if !(period.is_finite() && period > 0.0) {
        return StrokePlan { stroke, solid_for_count: true, dashes: 0 };
    }
    let count = length / period * dashes.len() as f64;
    if period * px_per_inch < MIN_DASH_PERIOD_PX || !count.is_finite() || count > MAX_DASHES {
        return StrokePlan { stroke, solid_for_count: true, dashes: 0 };
    }
    stroke = stroke.with_dashes(0.0, dashes);
    // At most `MAX_DASHES` and not negative (a length is not), so the cast keeps it.
    StrokePlan { stroke, solid_for_count: false, dashes: count.max(0.0).ceil() as u64 }
}
