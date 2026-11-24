//! Gradients, patterns and fill order.

use diacraft_doc::RowKey;
use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

/// A 2 by 2 inch square at (1, 1) with a linear red-to-blue gradient at `angle` radians (red is the first stop), and no outline.
fn gradient(angle: f64) -> ShapeBuilder {
    let sb = rect(1, 1.0, 1.0, 2.0, 2.0)
        .cell("LinePattern", "0")
        .cell("FillGradientEnabled", "1")
        .cell("FillGradientDir", "0")
        .cell("FillGradientAngle", &angle.to_string());
    stops(sb, &[("#FF0000", 0.0), ("#0000FF", 1.0)])
}

fn stops(mut sb: ShapeBuilder, list: &[(&str, f64)]) -> ShapeBuilder {
    for (i, (color, pos)) in list.iter().enumerate() {
        let row = RowKey::Ix(i as u32);
        sb = sb
            .row_cell("FillGradient", None, row.clone(), "GradientStopColor", color)
            .row_cell("FillGradient", None, row.clone(), "GradientStopColorTrans", "0")
            .row_cell("FillGradient", None, row, "GradientStopPosition", &pos.to_string());
    }
    sb
}

/// Whether the pixel is nearer red than blue, by a margin.
fn reddish(pic: &Pic, x: f64, y: f64) -> bool {
    let p = pic.at(x, y);
    i32::from(p[0]) > i32::from(p[2]) + 100
}

fn bluish(pic: &Pic, x: f64, y: f64) -> bool {
    let p = pic.at(x, y);
    i32::from(p[2]) > i32::from(p[0]) + 100
}

#[test]
fn a_gradient_angle_runs_clockwise_so_ninety_degrees_puts_the_first_stop_at_the_top() {
    // Low confidence: the angle grows clockwise as seen on the page. Counterclockwise would put red at the bottom.
    let half_turn = std::f64::consts::PI;
    let ninety = draw(&one_page(vec![gradient(half_turn / 2.0)]));
    assert!(reddish(&ninety, 2.0, 2.9) && bluish(&ninety, 2.0, 1.1), "90 degrees: red at the top, blue below");
    let zero = draw(&one_page(vec![gradient(0.0)]));
    assert!(reddish(&zero, 1.1, 2.0) && bluish(&zero, 2.9, 2.0), "0 degrees: red at the left, where both senses agree");
    let one_eighty = draw(&one_page(vec![gradient(half_turn)]));
    assert!(reddish(&one_eighty, 2.9, 2.0) && bluish(&one_eighty, 1.1, 2.0), "180 degrees: red at the right");
    let two_seventy = draw(&one_page(vec![gradient(1.5 * half_turn)]));
    assert!(reddish(&two_seventy, 2.0, 1.1) && bluish(&two_seventy, 2.0, 2.9), "270 degrees: red at the bottom");
}

#[test]
fn a_gradient_goes_through_its_stops_in_position_order_and_holds_the_end_colours() {
    // Three stops, the middle one listed last: they are sorted by position. Green sits in the middle.
    let sb = stops(
        rect(1, 1.0, 1.0, 2.0, 2.0).cell("LinePattern", "0").cell("FillGradientEnabled", "1").cell("FillGradientAngle", "0"),
        &[("#FF0000", 0.0), ("#0000FF", 1.0), ("#00FF00", 0.5)],
    );
    let pic = draw(&one_page(vec![sb]));
    assert!(pic.is(2.0, 2.0, (0, 255, 0), 40), "the middle is the middle stop, got {:?}", pic.at(2.0, 2.0));
    assert!(reddish(&pic, 1.02, 2.0) && bluish(&pic, 2.98, 2.0));
}

#[test]
fn a_gradient_that_is_not_enabled_paints_the_fill_colour_whatever_its_rows_say() {
    let sb = gradient(0.0).cell("FillGradientEnabled", "0").cell("FillForegnd", "#00FF00");
    let pic = draw(&one_page(vec![sb]));
    assert!(pic.is(1.2, 2.0, (0, 255, 0), 6) && pic.is(2.8, 2.0, (0, 255, 0), 6));
}

#[test]
fn a_gradient_paints_over_the_pattern_number() {
    // With the gradient on, pattern 1 does not decide: the gradient paints.
    let pic = draw(&one_page(vec![gradient(0.0).cell("FillPattern", "1").cell("FillForegnd", "#00FF00")]));
    assert!(reddish(&pic, 1.1, 2.0));
}

#[test]
fn a_legacy_gradient_pattern_number_paints_foreground_to_background() {
    // Pattern 28 is the foreground at the top edge fading down, and the stops are the two fill colours.
    let sb =
        rect(1, 1.0, 1.0, 2.0, 2.0).cell("LinePattern", "0").cell("FillPattern", "28").cell("FillForegnd", "#FF0000").cell("FillBkgnd", "#0000FF");
    let pic = draw(&one_page(vec![sb]));
    assert!(reddish(&pic, 2.0, 2.9) && bluish(&pic, 2.0, 1.1));
    let left =
        rect(1, 1.0, 1.0, 2.0, 2.0).cell("LinePattern", "0").cell("FillPattern", "25").cell("FillForegnd", "#FF0000").cell("FillBkgnd", "#0000FF");
    let pic = draw(&one_page(vec![left]));
    assert!(reddish(&pic, 1.1, 2.0) && bluish(&pic, 2.9, 2.0), "25: the foreground at the left edge");
}

#[test]
fn a_radial_gradient_has_its_first_stop_at_its_origin() {
    // Direction 3 is the centre; direction 2 the bottom-left corner.
    let centre = gradient(0.0).cell("FillGradientDir", "3");
    let pic = draw(&one_page(vec![centre]));
    assert!(reddish(&pic, 2.0, 2.0) && bluish(&pic, 1.05, 1.05), "red at the centre, blue at the corner");
    let corner = gradient(0.0).cell("FillGradientDir", "2");
    let pic = draw(&one_page(vec![corner]));
    assert!(reddish(&pic, 1.1, 1.1) && bluish(&pic, 2.9, 2.9), "red at the bottom left");
}

#[test]
fn a_rectangular_gradient_has_square_bands_from_its_origin() {
    // Direction 10 is the centre: the colour depends on the larger of the horizontal and vertical distances, so (2.9, 2.0) and (2.0, 2.9)
    // are alike and both far from (2.9, 2.9) only by a corner.
    let pic = draw(&one_page(vec![gradient(0.0).cell("FillGradientDir", "10")]));
    let a = pic.at(2.8, 2.0);
    let b = pic.at(2.0, 2.8);
    assert!((i32::from(a[0]) - i32::from(b[0])).abs() < 24, "{a:?} {b:?}");
    assert!(reddish(&pic, 2.0, 2.0) && bluish(&pic, 2.95, 2.0));
}

#[test]
fn a_gradient_follows_the_shape_when_it_turns_and_the_page_when_it_does_not() {
    // With RotateGradientWithShape 1 the frame is the shape's own, so turning the shape a quarter turn carries red from the left
    // edge of the shape to its bottom (counterclockwise). With 0 the frame is the page's and red stays on the left.
    let turned = |rotate: &str| {
        let sb = gradient(0.0)
            .cell("RotateGradientWithShape", rotate)
            .cell("PinX", "2")
            .cell("PinY", "2")
            .cell("LocPinX", "1")
            .cell("LocPinY", "1")
            .cell("Angle", &std::f64::consts::FRAC_PI_2.to_string());
        draw(&one_page(vec![sb]))
    };
    let follows = turned("1");
    assert!(reddish(&follows, 2.0, 1.1) && bluish(&follows, 2.0, 2.9), "the left edge of the shape is now at the bottom");
    let page = turned("0");
    assert!(reddish(&page, 1.1, 2.0) && bluish(&page, 2.9, 2.0), "the page's left is still red");
}

#[test]
fn a_hatch_paints_marks_over_a_ground() {
    // Pattern 6 is spaced horizontal lines; the marks are the foreground and what lies between them the background.
    let sb =
        rect(1, 1.0, 1.0, 2.0, 2.0).cell("LinePattern", "0").cell("FillPattern", "6").cell("FillForegnd", "#FF0000").cell("FillBkgnd", "#0000FF");
    let pic = draw(&one_page(vec![sb]));
    let column: Vec<[u8; 4]> = (0..200).map(|i| pic.at(2.0, 1.0 + f64::from(i) * 0.01 + 0.005)).collect();
    assert!(column.iter().any(|p| i32::from(p[0]) > i32::from(p[2]) + 40), "foreground marks");
    assert!(column.iter().any(|p| i32::from(p[2]) > i32::from(p[0]) + 100), "background between them");
}

#[test]
fn a_transparent_fill_lets_what_is_behind_show_through() {
    let back = rect(1, 1.0, 1.0, 2.0, 2.0).cell("FillForegnd", "#0000FF");
    let front = rect(2, 1.5, 1.5, 2.0, 2.0).cell("FillForegnd", "#FF0000").cell("FillForegndTrans", "0.5");
    let pic = draw(&one_page(vec![back, front]));
    let p = pic.at(2.0, 2.0);
    assert!(p[0] > 100 && p[2] > 100, "half red over blue: {p:?}");
}
