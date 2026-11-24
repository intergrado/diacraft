//! `ArcTo`.

use diacraft_geom::kurbo::{ParamCurve, PathSeg, Point};
use diacraft_geom::{ArcFallback, WarningKind, circular_arc};

use crate::{CURVE_EPS, EPS, arc_to, assert_near, assert_pt, build_plain, extents, ln, mv, passes_through, segments, size, vertices, y_at_x};

/// The arc of `ArcTo` (x, y) with bow `a` from (px, py).
fn arc(px: f64, py: f64, x: f64, y: f64, bow: f64) -> diacraft_geom::CircularArc {
    circular_arc(Point::new(px, py), Point::new(x, y), bow, size(1.0, 1.0)).unwrap()
}

/// Given a pen at (0, 0), `ArcTo` (4, 0) with `A` 1, expect the arc through (2, -1), centre (2, 1.5),
/// radius 2.5, sweep about 106.26 degrees counterclockwise, and extents from y -1 to y 0. A reversed sign gives (2, 1) at the top.
#[test]
fn arcto_a_positive_bow_bulges_to_the_right_of_travel() {
    let a = arc(0.0, 0.0, 4.0, 0.0, 1.0);
    assert_pt(a.center, 2.0, 1.5, EPS, "centre");
    assert_near(a.radius, 2.5, EPS, "radius");
    assert_near(a.sweep.to_degrees(), 106.26, 0.01, "sweep in degrees");
    assert_near(a.sweep, 4.0 * 0.5_f64.atan(), EPS, "sweep is 4 atan(2A/c)");
    assert!(a.sweep > 0.0, "counterclockwise");
    assert_pt(a.middle, 2.0, -1.0, EPS, "the middle of the arc");

    let built = build_plain(&[mv(0.0, 0.0), arc_to(4.0, 0.0, 1.0)]);
    let e = extents(&built.stroke);
    assert_near(e.y0, -1.0, CURVE_EPS, "lowest y");
    assert_near(e.y1, 0.0, CURVE_EPS, "highest y");
    assert!(passes_through(&built.stroke, Point::new(2.0, -1.0), CURVE_EPS));
    assert!(!passes_through(&built.stroke, Point::new(2.0, 1.0), 0.01), "the reversed sign would put the top here");
    assert!(built.warnings.is_empty());
}

/// Given the same with `A` -1, expect the arc through (2, 1), centre (2, -1.5), radius 2.5, clockwise. Taking `|A|`
/// gives the first arc.
#[test]
fn arcto_a_negative_bow_bulges_to_the_left() {
    let a = arc(0.0, 0.0, 4.0, 0.0, -1.0);
    assert_pt(a.center, 2.0, -1.5, EPS, "centre");
    assert_near(a.radius, 2.5, EPS, "radius");
    assert!(a.sweep < 0.0, "clockwise");
    assert_near(a.sweep, -4.0 * 0.5_f64.atan(), EPS, "sweep");
    assert_pt(a.middle, 2.0, 1.0, EPS, "the middle of the arc");
    let built = build_plain(&[mv(0.0, 0.0), arc_to(4.0, 0.0, -1.0)]);
    let e = extents(&built.stroke);
    assert_near(e.y0, 0.0, CURVE_EPS, "lowest y");
    assert_near(e.y1, 1.0, CURVE_EPS, "highest y");
}

/// Given a pen at (1, 0) and `ArcTo` (1, 2) with `A` 1, going up, expect the arc through (2, 1), to the right of
/// travel. A rule fixed to "positive is below" gives (1, 1) or no answer.
#[test]
fn arcto_right_of_travel_is_right_whichever_way_the_pen_goes() {
    let a = arc(1.0, 0.0, 1.0, 2.0, 1.0);
    assert_pt(a.middle, 2.0, 1.0, EPS, "middle");
    assert_pt(a.center, 1.0, 1.0, EPS, "centre: a half circle");
    let built = build_plain(&[mv(1.0, 0.0), arc_to(1.0, 2.0, 1.0)]);
    let e = extents(&built.stroke);
    assert_near(e.x0, 1.0, CURVE_EPS, "leftmost x");
    assert_near(e.x1, 2.0, CURVE_EPS, "rightmost x");
    // And going down, the same bow is to the left of the page: the right of travel changes with the direction.
    let down = arc(1.0, 2.0, 1.0, 0.0, 1.0);
    assert_pt(down.middle, 0.0, 1.0, EPS, "middle going down");
}

/// Given a rectangle from (0, 0) to (4, 4) run counterclockwise, each side an `ArcTo` with `A` -0.5, expect each
/// side bowed in, so the middle of the bottom side is (2, 0.5). The reversed sign gives (2, -0.5), a bulge.
#[test]
fn arcto_a_counterclockwise_rectangle_of_negative_bows_is_bowed_in() {
    let built = build_plain(&[mv(0.0, 0.0), arc_to(4.0, 0.0, -0.5), arc_to(4.0, 4.0, -0.5), arc_to(0.0, 4.0, -0.5), arc_to(0.0, 0.0, -0.5)]);
    let at_2 = y_at_x(&built.stroke, 2.0);
    assert!(at_2.iter().any(|y| (y - 0.5).abs() < 1e-4), "the bottom side's middle is (2, 0.5): {at_2:?}");
    assert!(at_2.iter().any(|y| (y - 3.5).abs() < 1e-4), "the top side's middle is (2, 3.5): {at_2:?}");
    assert!(!at_2.iter().any(|y| (y + 0.5).abs() < 1e-4), "the reversed sign would bulge to (2, -0.5)");
    let e = extents(&built.stroke);
    assert!(e.x0 >= -1e-9 && e.y0 >= -1e-9 && e.x1 <= 4.0 + 1e-9 && e.y1 <= 4.0 + 1e-9, "bowed in: nothing leaves the square: {e:?}");
}

/// Given a pen at (0, 0), `ArcTo` (4, 0) with `A` 3, expect the longer arc through (2, -3), radius 13/6 (about 2.1667),
/// centre (2, -5/6), sweep about 225.24 degrees, and x from about -0.1667 to about 4.1667. Clamping the sweep to a half circle gives x within 0 to 4.
#[test]
fn arcto_a_bow_above_half_the_chord_is_more_than_half_a_circle() {
    let a = arc(0.0, 0.0, 4.0, 0.0, 3.0);
    assert_near(a.radius, 13.0 / 6.0, EPS, "radius");
    assert_pt(a.center, 2.0, -5.0 / 6.0, EPS, "centre");
    assert_near(a.sweep.to_degrees(), 225.24, 0.01, "sweep");
    assert_pt(a.middle, 2.0, -3.0, EPS, "middle");
    let built = build_plain(&[mv(0.0, 0.0), arc_to(4.0, 0.0, 3.0)]);
    let e = extents(&built.stroke);
    assert_near(e.x0, 2.0 - 13.0 / 6.0, CURVE_EPS, "leftmost x");
    assert_near(e.x1, 2.0 + 13.0 / 6.0, CURVE_EPS, "rightmost x");
    assert!(e.x0 < -0.16 && e.x1 > 4.16, "naive: a sweep clamped to a half circle stays within 0 to 4");
    assert_near(e.y0, -3.0, CURVE_EPS, "lowest y");
}

/// Given `ArcTo` (4, 0) with `A` 0, expect a straight segment and no division by zero. Given `A` 1E-300, expect the same
/// within rounding. The radius formula alone gives infinity.
#[test]
fn arcto_no_bow_is_a_straight_chord() {
    for bow in [0.0, -0.0, 1e-300, -1e-300, 5e-324] {
        let built = build_plain(&[mv(0.0, 0.0), arc_to(4.0, 0.0, bow)]);
        let segs = segments(&built.stroke);
        assert_eq!(segs.len(), 1, "bow {bow}");
        assert!(matches!(segs[0], PathSeg::Line(_)), "bow {bow}: {:?}", segs[0]);
        assert_pt(segs[0].end(), 4.0, 0.0, EPS, "end");
        assert!(built.warnings.is_empty());
        assert_eq!(
            circular_arc(Point::new(0.0, 0.0), Point::new(4.0, 0.0), bow, size(1.0, 1.0)).unwrap_err(),
            ArcFallback::Flat,
            "no arc is made of it"
        );
    }
    // The radius formula alone would give this.
    assert!(((4.0f64 / 4.0).powi(2) + 1e-300f64.powi(2)) / (2.0 * 1e-300) > 1e299);
}

/// Our rule, as another reader does: an end equal to the pen with `A` not 0 has no chord direction; a line to the end is drawn, which
/// is a point, so nothing visible. The row is not an error.
#[test]
fn arcto_a_bow_with_no_chord_draws_a_point() {
    let built = build_plain(&[mv(1.0, 1.0), arc_to(1.0, 1.0, 2.0), ln(3.0, 1.0)]);
    assert!(built.warnings.is_empty());
    let segs = segments(&built.stroke);
    assert_eq!(segs.len(), 2);
    assert!(matches!(segs[0], PathSeg::Line(l) if l.p0 == l.p1));
    assert_pt(segs[1].start(), 1.0, 1.0, EPS, "the next row starts at the pen");
    assert_eq!(circular_arc(Point::new(1.0, 1.0), Point::new(1.0, 1.0), 2.0, size(1.0, 1.0)).unwrap_err(), ArcFallback::NoChord);
}

/// Given a pen at (0, 0) and `ArcTo` (4, 0) with `A` 1, and a write of 2 to `A`, expect the end still (4, 0), the
/// arc's middle at (2, -2), and the next row's start unchanged.
#[test]
fn a_geometry_the_bow_changes_the_curve_and_nothing_else() {
    let after = build_plain(&[mv(0.0, 0.0), arc_to(4.0, 0.0, 2.0), ln(4.0, 4.0)]);
    let segs = segments(&after.stroke);
    assert!(segs.len() >= 2, "the arc may be several Beziers, then the line");
    let last = segs.last().unwrap();
    assert_pt(last.start(), 4.0, 0.0, EPS, "the next row's start is the arc's end");
    assert_pt(last.end(), 4.0, 4.0, EPS, "the next row's end");
    assert!(passes_through(&after.stroke, Point::new(2.0, -2.0), CURVE_EPS), "the middle is (2, -2)");
    assert_near(extents(&after.stroke).y0, -2.0, CURVE_EPS, "lowest y");
    let before = build_plain(&[mv(0.0, 0.0), arc_to(4.0, 0.0, 1.0), ln(4.0, 4.0)]);
    assert_pt(segments(&before.stroke).last().unwrap().start(), 4.0, 0.0, EPS, "same start with A 1");
}

/// The path of an arc starts at the pen and ends exactly at the row's end, to the last digit: the next row joins it with no gap.
#[test]
fn an_arc_ends_exactly_at_its_end_point() {
    for (end, bow) in [((4.0, 0.0), 1.0), ((0.1, 0.7), -0.3), ((123.456, -78.9), 40.0), ((1.0, 1e-3), 5e3)] {
        let built = build_plain(&[mv(0.5, 0.25), arc_to(end.0, end.1, bow)]);
        let v = vertices(&built.stroke);
        assert_eq!(*v.first().unwrap(), Point::new(0.5, 0.25));
        assert_eq!(*v.last().unwrap(), Point::new(end.0, end.1), "end of the arc with bow {bow}");
    }
}

/// Arcs of every size stay on their circle: a sample of the path is at the radius from the centre.
#[test]
fn an_arc_stays_on_its_circle() {
    for bow in [0.01, 0.4, 2.0, 2.5, 7.0, -0.9, -3.0] {
        let a = arc(0.0, 0.0, 4.0, 0.0, bow);
        let built = build_plain(&[mv(0.0, 0.0), arc_to(4.0, 0.0, bow)]);
        for seg in built.stroke.segments() {
            for i in 0..=50 {
                let pt = seg.eval(i as f64 / 50.0);
                assert_near(pt.distance(a.center), a.radius, CURVE_EPS * a.radius.max(1.0), &format!("bow {bow}"));
            }
        }
    }
}

/// Hostile bows give a defined result, never a panic: a number that is not finite skips the row (`NonFinite`), and a huge one is a finite arc or, where its
/// arithmetic overflows, a line to the end with an `Overflow` warning (`an_arc_whose_arithmetic_overflows_is_a_line_with_a_warning`).
#[test]
fn hostile_bows_do_not_panic() {
    let skipped = build_plain(&[mv(0.0, 0.0), arc_to(4.0, 0.0, f64::NAN), arc_to(4.0, 0.0, f64::INFINITY)]);
    assert_eq!(skipped.warnings.len(), 2);
    assert!(skipped.warnings.iter().all(|w| w.kind == WarningKind::NonFinite));
    for bow in [1e308, -1e308, 1e154, 1e-154, f64::MAX, f64::MIN_POSITIVE] {
        let built = build_plain(&[mv(0.0, 0.0), arc_to(4.0, 0.0, bow), ln(1.0, 1.0)]);
        assert!(built.stroke.is_finite(), "bow {bow}: {:?}", built.stroke);
    }
    // Chords whose square overflows: no panic, whatever comes back.
    let _ = circular_arc(Point::new(0.0, 0.0), Point::new(f64::MAX, f64::MAX), 1.0, size(1.0, 1.0));
    let _ = circular_arc(Point::new(-f64::MAX, 0.0), Point::new(f64::MAX, 0.0), f64::MAX, size(1.0, 1.0));
}

/// Review finding 3: a chord at rounding noise is no chord. Pen 0.30000000000000004 and end 0.3 with a bow of 0.5 drew a full circle, while an exact end draws
/// a point: the test `chord > 0` was discontinuous at rounding noise.
#[test]
fn a_chord_at_rounding_noise_draws_a_point_not_a_circle() {
    let noisy = 0.1 + 0.2;
    assert_ne!(noisy, 0.3);
    for (from, to) in [(noisy, 0.3), (0.3, noisy)] {
        let built = build_plain(&[mv(from, 0.0), arc_to(to, 0.0, 0.5), ln(1.0, 1.0)]);
        let segs = segments(&built.stroke);
        assert_eq!(segs.len(), 2, "a point and the next line, from ({from}, 0) to ({to}, 0): {segs:?}");
        assert!(matches!(segs[0], PathSeg::Line(_)), "{:?}", segs[0]);
        assert!(built.warnings.is_empty());
        assert!(extents(&built.stroke).width() < 1.0, "no circle of radius a quarter");
    }
}

/// The same from the other side: the noise is a share of the shape's size (`rules::ARC_CHORD_RATIO`, 1E-9 of the larger of `Width` and `Height`), so a
/// chord of the same length is noise on a big shape and a chord on a small one, and a chord a little over the share is an arc. Both sides are tested, as the review asks.
#[test]
fn the_noise_is_a_share_of_the_shapes_size() {
    let chord_at =
        |size_of_shape: f64, chord: f64| circular_arc(Point::new(0.0, 0.0), Point::new(chord, 0.0), 0.5, size(size_of_shape, size_of_shape));
    // Size 1: the share is 1E-9.
    assert_eq!(chord_at(1.0, 0.9e-9).unwrap_err(), ArcFallback::NoChord, "under the share");
    assert_eq!(chord_at(1.0, 1e-9).unwrap_err(), ArcFallback::NoChord, "at the share");
    assert!(chord_at(1.0, 1.1e-9).is_ok(), "over the share it is an arc (here nearly a whole circle, a bow of 0.5 on a chord that small)");
    // The same chord on a shape a thousand times as big is under its share, and on one a thousand times smaller it is over.
    assert_eq!(chord_at(1000.0, 1e-7).unwrap_err(), ArcFallback::NoChord);
    assert!(chord_at(1e-3, 1e-7).is_ok());
    // The larger of `Width` and `Height` counts, not the smaller.
    assert_eq!(circular_arc(Point::new(0.0, 0.0), Point::new(1e-7, 0.0), 0.5, size(1000.0, 1.0)).unwrap_err(), ArcFallback::NoChord);
    assert_eq!(circular_arc(Point::new(0.0, 0.0), Point::new(1e-7, 0.0), 0.5, size(1.0, 1000.0)).unwrap_err(), ArcFallback::NoChord);
    // A shape with no size (a collapsed one) takes the larger coordinate of the points: noise at 5 is not an arc, and 1E-7 at 5 is.
    let collapsed = size(0.0, 0.0);
    assert_eq!(circular_arc(Point::new(5.0, 5.0), Point::new(5.0 + 8.9e-16, 5.0), 0.5, collapsed).unwrap_err(), ArcFallback::NoChord);
    assert_eq!(circular_arc(Point::new(0.0, 0.0), Point::new(0.0, 0.0), 0.5, collapsed).unwrap_err(), ArcFallback::NoChord);
    assert!(circular_arc(Point::new(5.0, 5.0), Point::new(5.0 + 1e-7, 5.0), 0.5, collapsed).is_ok());
    assert!(circular_arc(Point::new(1e-12, 0.0), Point::new(0.0, 0.0), 1e-12, collapsed).is_ok(), "a tiny shape is not all noise");
}

/// Review round 2, finding 1: the stand-in size had a hole. A pen of 5.55E-17 and an end at the origin, with a bow of 0.5, drew a whole circle on a shape of size (0, 0), (-3, -3) or
/// (-3, 0), and a pen one ulp from 5 on a shape of (1E-12, 0) did too. The noise is a share of the larger of `|Width|`, `|Height|`, the coordinates and `|bow|`, none of which has a
/// floor at the origin; sizes can be negative (hold thousands of such cells).
#[test]
fn rounding_noise_is_no_chord_whatever_the_sign_or_the_size() {
    let sizes = [(0.0, 0.0), (-3.0, -3.0), (-3.0, 0.0), (0.0, -3.0), (1e-12, 0.0), (-1e-12, -1e-12), (1.0, 1.0), (1e6, 1e6), (-1e6, 1.0)];
    let five_up = f64::from_bits(5.0f64.to_bits() + 1);
    for (width, height) in sizes {
        let shape = size(width, height);
        let at = format!("size ({width}, {height})");
        // At the origin: 5.55E-17 is what 0.1 + 0.2 - 0.3 leaves.
        assert_eq!(circular_arc(Point::new(5.55e-17, 0.0), Point::new(0.0, 0.0), 0.5, shape).unwrap_err(), ArcFallback::NoChord, "{at}");
        assert_eq!(circular_arc(Point::new(0.0, 0.0), Point::new(0.0, -5.55e-17), 0.5, shape).unwrap_err(), ArcFallback::NoChord, "{at}");
        // One ulp from 5, either way.
        assert_eq!(circular_arc(Point::new(5.0, 0.0), Point::new(five_up, 0.0), 0.5, shape).unwrap_err(), ArcFallback::NoChord, "{at}");
        assert_eq!(circular_arc(Point::new(five_up, 3.0), Point::new(5.0, 3.0), -0.5, shape).unwrap_err(), ArcFallback::NoChord, "{at}");
        // Through `build_paths` it is a point and no warning.
        let built = crate::build(&[mv(5.55e-17, 0.0), arc_to(0.0, 0.0, 0.5), ln(1.0, 1.0)], width, height);
        let segs = segments(&built.stroke);
        assert!(built.warnings.is_empty(), "{at}: {:?}", built.warnings);
        assert_eq!(segs.len(), 2, "{at}: a point and the next line: {segs:?}");
        assert!(matches!(segs[0], PathSeg::Line(_)));
    }
    // The numbers of the first round still hold: the share is 1E-9 of the size, so a chord over it is an arc, and a tiny shape is not all noise.
    assert!(circular_arc(Point::new(0.0, 0.0), Point::new(1.1e-9, 0.0), 0.5, size(1.0, 1.0)).is_ok());
    assert!(circular_arc(Point::new(0.0, 0.0), Point::new(1e-7, 0.0), 0.5, size(1e-3, 1e-3)).is_ok());
    assert!(circular_arc(Point::new(5.0, 5.0), Point::new(5.0 + 1e-7, 5.0), 0.5, size(0.0, 0.0)).is_ok());
    assert!(circular_arc(Point::new(1e-12, 0.0), Point::new(0.0, 0.0), 1e-12, size(0.0, 0.0)).is_ok());
    assert!(circular_arc(Point::new(1e-12, 0.0), Point::new(0.0, 0.0), 1e-12, size(-1e-12, 0.0)).is_ok());
    // A bow counts toward the scale, so a huge bow over a chord of 1E-7 on a unit shape is noise (1E-9 of the bow is 1E-6): the bow is what gives the arc its size.
    assert_eq!(circular_arc(Point::new(0.0, 0.0), Point::new(1e-7, 0.0), 1000.0, size(1.0, 1.0)).unwrap_err(), ArcFallback::NoChord);
}

/// The scale is the largest of five kinds of number, each by its absolute value, and each kind must be the largest somewhere. A chord of 2E-9 with a bow of 0.5 is noise
/// where the scale is above 2 (1E-9 of it is 2E-9) and an arc where it is 0.5 (the bow alone) or 1: this separates a width or height taken with its sign, a coordinate
/// taken with its sign, a bow taken with its sign, and a scale that leaves out the coordinates or the size (found by mutation after review round 2).
#[test]
fn each_part_of_the_noise_scale_counts_by_its_absolute_value() {
    let chord = 2e-9;
    let is_noise = |start: (f64, f64), shape: (f64, f64), bow: f64| {
        let end = (start.0 + chord, start.1);
        circular_arc(Point::new(start.0, start.1), Point::new(end.0, end.1), bow, size(shape.0, shape.1)) == Err(ArcFallback::NoChord)
    };
    // The size alone is the largest (3 against a bow of 0.5): by its absolute value, for the width and for the height.
    for shape in [(3.0, 3.0), (-3.0, -3.0), (-3.0, 0.0), (0.0, -3.0), (3.0, -3.0)] {
        assert!(is_noise((0.0, 0.0), shape, 0.5), "size {shape:?}");
    }
    assert!(!is_noise((0.0, 0.0), (-1.0, -1.0), 0.5), "a scale of 1 leaves 2E-9 an arc");
    assert!(!is_noise((0.0, 0.0), (0.0, 0.0), 0.5), "and so does the bow's 0.5");
    // The coordinates alone are the largest (5 against a bow of 0.5 and no size): by absolute value.
    assert!(is_noise((5.0, 5.0), (0.0, 0.0), 0.5));
    assert!(is_noise((-5.0, -5.0), (0.0, 0.0), 0.5));
    assert!(is_noise((0.0, -5.0), (0.0, 0.0), 0.5));
    // The bow alone is the largest (a size of 0.1 and coordinates at the origin): by absolute value.
    assert!(is_noise((0.0, 0.0), (0.1, 0.1), 3.0));
    assert!(is_noise((0.0, 0.0), (0.1, 0.1), -3.0));
    assert!(!is_noise((0.0, 0.0), (0.1, 0.1), 0.5));
}

/// Review (small items): a bow whose arithmetic overflows (a radius that is not a number) draws the chord, as the spec's fallback, and says so.
#[test]
fn an_arc_whose_arithmetic_overflows_is_a_line_with_a_warning() {
    for bow in [1e200, -1e200, 1e308, f64::MAX] {
        let built = build_plain(&[mv(0.0, 0.0), arc_to(4.0, 0.0, bow), ln(1.0, 1.0)]);
        assert_eq!(built.warnings.len(), 1, "bow {bow}");
        assert_eq!(built.warnings[0].kind, WarningKind::Overflow);
        assert_eq!(built.warnings[0].row, 1);
        let segs = segments(&built.stroke);
        assert!(matches!(segs[0], PathSeg::Line(_)), "bow {bow}: {:?}", segs[0]);
        assert_pt(segs[0].end(), 4.0, 0.0, EPS, "the chord");
    }
}

/// A bow of a two-thousandth of the chord is a real arc: a mutation that raised `ARC_FLAT_RATIO` to 1E-3 drew it straight and no test said so (review).
#[test]
fn a_very_shallow_bow_is_still_an_arc() {
    let built = build_plain(&[mv(0.0, 0.0), arc_to(4.0, 0.0, 0.002)]);
    assert!(segments(&built.stroke).iter().all(|s| matches!(s, PathSeg::Cubic(_))), "{:?}", built.stroke);
    assert_near(extents(&built.stroke).y0, -0.002, 1e-7, "the arc's middle is 0.002 from the chord");
}
