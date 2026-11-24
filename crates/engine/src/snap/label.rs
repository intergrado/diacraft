//! How a length or an angle drawn beside a mark is written (spec): the number with at most three places, trailing zeros and a bare point
//! dropped, a space, and the unit's short name with no point: "1 in", "25.4 mm", "33 deg". A length is shown in the page's **drawing units** (`paper × DrawingScale ÷
//! PageScale`) and in the unit the page's ruler counts in; an angle in degrees, signed, from just over -180 up to 180.
//!
//! Ours: no published image shows a number beside a guide, a gap or a size. The form follows what the status bar shows for a selected shape.

use diacraft_units::Unit;

/// The unit a length is shown in, and its short name. A compound unit (`F_I`, `P_PT`) shows in its larger unit with decimals; a unit that is not in the list of
/// short names (the printer's units other than the point, the nautical mile) shows in inches (ours: the specification names no other).
fn shown_in(unit: Unit) -> (Unit, &'static str) {
    match unit {
        Unit::Foot | Unit::FeetInches => (Unit::Foot, "ft"),
        Unit::Yard => (Unit::Yard, "yd"),
        Unit::Mile | Unit::MileFine => (Unit::Mile, "mi"),
        Unit::Millimetre => (Unit::Millimetre, "mm"),
        Unit::Centimetre => (Unit::Centimetre, "cm"),
        Unit::Metre => (Unit::Metre, "m"),
        Unit::Kilometre => (Unit::Kilometre, "km"),
        Unit::Point => (Unit::Point, "pt"),
        _ => (Unit::Inch, "in"),
    }
}

/// A number with at most three places, rounded half away from zero, with no trailing zeros and no minus sign on a zero. A value of 1E12 or more is written with a
/// power of ten, as the window's fields write it. A number that is not finite is written "0".
fn number(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    let rounded = (v * 1000.0).round() / 1000.0;
    // `+ 0.0` turns a minus zero into a zero.
    let rounded = if rounded.is_finite() { rounded + 0.0 } else { v };
    if rounded.abs() >= 1.0e12 { format!("{rounded:e}") } else { format!("{rounded}") }
}

/// A length of `paper_inches` on a page whose drawing is `k` times its paper, shown in the page's unit `unit`.
pub fn length(paper_inches: f64, k: f64, unit: Unit) -> String {
    let (unit, name) = shown_in(unit);
    let k = if k.is_finite() && k > 0.0 { k } else { 1.0 };
    format!("{} {name}", number(unit.from_internal(paper_inches * k)))
}

/// An angle in degrees, signed, from just over -180 up to 180: 330 is "-30 deg", a half turn "180 deg".
pub fn angle(degrees: f64) -> String {
    if !degrees.is_finite() {
        return "0 deg".to_owned();
    }
    let mut a = degrees.rem_euclid(360.0);
    if a > 180.0 {
        a -= 360.0;
    }
    format!("{} deg", number(a))
}

/// A line to label, in the window's screen units: where it begins, where it ends and the size (width, height) of its label's text.
pub type Labelled = ((f64, f64), (f64, f64), (f64, f64));

/// A box in screen units: left, top, right, bottom.
pub type LabelBox = [f64; 4];

/// Where the labels of lines go so that none overlaps another or one of the boxes to `avoid` (`craft_snap::labels::place`): centred on its line when the line is long enough, else
/// slid along it, else beside it, farther out each time, in screen units, **for any direction the line runs in** (a gap of a turned row). The text is the window's to measure
/// and to draw; this says only where its box goes. One box a line (at most 32 lines are labelled and 64 boxes avoided: a line past them has none, and the boxes are in the
/// order of the lines that have one).
pub fn place(lines: &[Labelled], avoid: &[LabelBox]) -> Vec<LabelBox> {
    let to_point = |p: (f64, f64)| kurbo::Point::new(p.0, p.1);
    let lines: Vec<craft_snap::labels::Labelled> =
        lines.iter().map(|(a, b, size)| (to_point(*a), to_point(*b), kurbo::Size::new(size.0, size.1))).collect();
    let avoid: Vec<kurbo::Rect> = avoid.iter().map(|b| kurbo::Rect::new(b[0], b[1], b[2], b[3])).collect();
    craft_snap::labels::place(&lines, &avoid).into_iter().map(|r| [r.x0, r.y0, r.x1, r.y1]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_of_lines_that_meet_go_apart_and_a_long_line_carries_its_label_in_the_middle() {
        let size = (30.0, 14.0);
        // A long line: the label is centred on it. Two short lines at one place: the second label is not on the first.
        let placed = place(&[((0.0, 50.0), (200.0, 50.0), size)], &[]);
        assert_eq!(placed, vec![[85.0, 43.0, 115.0, 57.0]]);
        let placed = place(&[((0.0, 0.0), (10.0, 0.0), size), ((0.0, 0.0), (10.0, 0.0), size)], &[]);
        assert_eq!(placed.len(), 2);
        let overlap = |a: &LabelBox, b: &LabelBox| a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];
        assert!(!overlap(&placed[0], &placed[1]), "{placed:?}");
        // A box to avoid: the label of a long line that would be on it goes elsewhere.
        let placed = place(&[((0.0, 50.0), (200.0, 50.0), size)], &[[85.0, 43.0, 115.0, 57.0]]);
        assert!(!overlap(&placed[0], &[85.0, 43.0, 115.0, 57.0]), "{placed:?}");
        // A line that is not a line (a number that is not finite) has no label; nothing to label has none.
        assert!(place(&[((f64::NAN, 0.0), (1.0, 0.0), size)], &[]).is_empty());
        assert!(place(&[], &[]).is_empty());
    }

    #[test]
    fn a_length_is_shown_in_its_unit_with_that_units_short_name_and_an_unlisted_unit_in_inches() {
        // One of each unit, as the inches it is.
        for (unit, inches, want) in [
            (Unit::Foot, 12.0, "1 ft"),
            (Unit::FeetInches, 12.0, "1 ft"),
            (Unit::Yard, 36.0, "1 yd"),
            (Unit::Mile, 63_360.0, "1 mi"),
            (Unit::MileFine, 63_360.0, "1 mi"),
            (Unit::Millimetre, 1.0 / 25.4, "1 mm"),
            (Unit::Centimetre, 1.0 / 2.54, "1 cm"),
            (Unit::Metre, 1.0 / 0.0254, "1 m"),
            (Unit::Kilometre, 1.0 / 0.0000254, "1 km"),
            (Unit::Point, 1.0 / 72.0, "1 pt"),
            (Unit::Inch, 1.0, "1 in"),
        ] {
            assert_eq!(length(inches, 1.0, unit), want, "{unit:?}");
        }
    }

    #[test]
    fn a_page_scale_that_is_no_scale_is_a_scale_of_1() {
        for k in [0.0, -2.0, f64::NAN, f64::INFINITY] {
            assert_eq!(length(1.5, k, Unit::Inch), "1.5 in", "{k}");
        }
        assert_eq!(length(1.5, 2.0, Unit::Inch), "3 in");
    }

    #[test]
    fn lengths_in_inches_have_at_most_three_places() {
        // 1/3 as "0.333 in", 2.5 as "2.5 in", 2 as "2 in", 0.0004 as "0 in", and minus zero as "0 in".
        for (v, want) in [
            (1.0 / 3.0, "0.333 in"),
            (2.5, "2.5 in"),
            (2.0, "2 in"),
            (0.0004, "0 in"),
            (-0.0, "0 in"),
            (-0.0004, "0 in"),
            (0.0625, "0.063 in"),
            (-0.0625, "-0.063 in"),
        ] {
            assert_eq!(length(v, 1.0, Unit::Inch), want, "{v}");
        }
        // Naive: four places gave "0.3333 in".
        assert_ne!(length(1.0 / 3.0, 1.0, Unit::Inch), "0.3333 in");
    }

    #[test]
    fn lengths_are_in_the_drawing_units_of_the_page() {
        // A paper inch on a page at 1 inch to 1 foot is 12 drawing inches, shown as 1 ft; in millimetres at 1:1, 25.4 mm; a compound unit shows in its larger.
        assert_eq!(length(1.0, 12.0, Unit::Foot), "1 ft");
        assert_eq!(length(1.0, 1.0, Unit::Millimetre), "25.4 mm");
        assert_eq!(length(1.0, 1.0, Unit::FeetInches), "0.083 ft");
        assert_eq!(length(42.0, 1.0, Unit::FeetInches), "3.5 ft");
        assert_eq!(length(1.0, 1.0, Unit::Point), "72 pt");
        assert_eq!(length(1.0, 1.0, Unit::DrawingDefault), "1 in");
        // A scale that is not a number is no scale.
        assert_eq!(length(2.0, f64::NAN, Unit::Inch), "2 in");
        assert_eq!(length(2.0, -3.0, Unit::Inch), "2 in");
        assert_eq!(length(f64::NAN, 1.0, Unit::Inch), "0 in");
    }

    #[test]
    fn angles_are_signed_from_just_over_minus_180_to_180() {
        for (v, want) in [
            (33.0, "33 deg"),
            (-30.0, "-30 deg"),
            (330.0, "-30 deg"),
            (180.0, "180 deg"),
            (-180.0, "180 deg"),
            (-0.0625, "-0.063 deg"),
            (33.3333, "33.333 deg"),
            (-0.0004, "0 deg"),
            (720.0 + 33.0, "33 deg"),
        ] {
            assert_eq!(angle(v), want, "{v}");
        }
        // Naive: 0 to 360 gave "330 deg"; two places for angles gave "-0.06 deg".
        assert_ne!(angle(-30.0), "330 deg");
        assert_ne!(angle(-0.0625), "-0.06 deg");
        assert_eq!(angle(f64::INFINITY), "0 deg");
    }

    #[test]
    fn a_value_of_a_trillion_or_more_is_written_with_a_power_of_ten() {
        assert_eq!(length(1.0e12, 1.0, Unit::Inch), "1e12 in");
        assert_eq!(length(-2.5e15, 1.0, Unit::Inch), "-2.5e15 in");
        assert_eq!(length(9.99e11, 1.0, Unit::Inch), "999000000000 in");
    }
}
