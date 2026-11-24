//! The colours of a gradient between its stops (item 3):
//! how the bytes blend (no gamma curve, no premultiplication), where the stops sit, and how a stop's see-through share counts.
//!
//! Every vector reads pixels at their centres, on a gradient along x over a shape 2 inches wide from x = 1 to 3, so the pixel at `x` is at
//! `t = (x - 1) / 2`.

use diacraft_doc::RowKey;
use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

/// A 2 by 2 inch square at (1, 1) with a linear gradient along x of these stops `(colour, see-through share, position)`, and no outline.
fn along_x(list: &[(&str, f64, f64)]) -> ShapeBuilder {
    let mut sb = rect(1, 1.0, 1.0, 2.0, 2.0)
        .cell("LinePattern", "0")
        .cell("FillGradientEnabled", "1")
        .cell("FillGradientDir", "0")
        .cell("FillGradientAngle", "0");
    for (i, (color, trans, pos)) in list.iter().enumerate() {
        let row = RowKey::Ix(i as u32);
        sb = sb
            .row_cell("FillGradient", None, row.clone(), "GradientStopColor", color)
            .row_cell("FillGradient", None, row.clone(), "GradientStopColorTrans", &trans.to_string())
            .row_cell("FillGradient", None, row, "GradientStopPosition", &pos.to_string());
    }
    sb
}

/// The centre of pixel column `k`, in paper inches.
fn centre(k: u32) -> f64 {
    (f64::from(k) + 0.5) / 100.0
}

/// The pixel at column `k` through the middle of the square, as the colour on the paper.
fn at(pic: &Pic, k: u32) -> [u8; 4] {
    pic.at(centre(k), 2.0)
}

fn near(p: [u8; 4], rgb: [f64; 3], tol: f64) -> bool {
    p[3] == 255 && p[..3].iter().zip(rgb).all(|(a, b)| (f64::from(*a) - b).abs() <= tol)
}

#[test]
fn the_bytes_blend_straight_with_no_gamma_curve() {
    // Black at 0 to #C80064 at 1: the middle line is #640032, and a quarter along is #320019. A blend in linear light would
    // brighten the middle to about #A10000 + ... (a 50% mix of linear values is far above the mean of the bytes).
    let pic = draw(&one_page(vec![along_x(&[("#000000", 0.0, 0.0), ("#C80064", 0.0, 1.0)])]));
    for k in [125, 150, 175, 200, 225, 250, 275] {
        let t = (centre(k) - 1.0) / 2.0;
        assert!(near(at(&pic, k), [200.0 * t, 0.0, 100.0 * t], 3.0), "t {t}: {:?}", at(&pic, k));
    }
}

#[test]
fn the_blend_of_two_bright_colours_does_not_darken_in_the_middle() {
    // The same in the other direction: red to blue is #7F007F in the middle of the bytes, not the darker #5A005A that a blend through
    // premultiplied or linearised values gives.
    let pic = draw(&one_page(vec![along_x(&[("#FF0000", 0.0, 0.0), ("#0000FF", 0.0, 1.0)])]));
    let t = (centre(200) - 1.0) / 2.0;
    assert!(near(at(&pic, 200), [255.0 * (1.0 - t), 0.0, 255.0 * t], 3.0), "{:?}", at(&pic, 200));
}

#[test]
fn a_stop_that_is_see_through_blends_its_colour_and_its_alpha_apart() {
    // Our rule: the straight bytes and the straight alpha. Red, then blue that is fully clear: the middle is #7F007F at half
    // opacity, which on white paper is #BF7FBF. A blend of premultiplied values would lose the blue and give a red that fades, #FF7F7F.
    let pic = draw(&one_page(vec![along_x(&[("#FF0000", 0.0, 0.0), ("#0000FF", 1.0, 1.0)])]));
    let t = (centre(200) - 1.0) / 2.0;
    let (r, b) = (255.0 * (1.0 - t), 255.0 * t);
    let alpha = 1.0 - t;
    let paper = 255.0 * (1.0 - alpha);
    assert!(near(at(&pic, 200), [r * alpha + paper, paper, b * alpha + paper], 4.0), "{:?}", at(&pic, 200));
}

#[test]
fn a_stop_with_a_share_of_see_through_lets_the_page_show_by_that_share() {
    // STROKE-like transparency on a stop: black at 0.5 clear over white paper is the grey #808080 at its stop and beyond (the end holds).
    let pic = draw(&one_page(vec![along_x(&[("#000000", 0.5, 0.0), ("#000000", 0.5, 1.0)])]));
    assert!(near(at(&pic, 110), [128.0, 128.0, 128.0], 3.0), "{:?}", at(&pic, 110));
    assert!(near(at(&pic, 290), [128.0, 128.0, 128.0], 3.0), "{:?}", at(&pic, 290));
}

#[test]
fn a_stop_list_that_starts_after_0_and_ends_before_1_holds_its_end_colours() {
    // Stops at 0.2 and 0.8: the colour is the first from the left edge to 0.2 and the last from 0.8, not stretched to 0 and 1.
    let pic = draw(&one_page(vec![along_x(&[("#FF0000", 0.0, 0.2), ("#0000FF", 0.0, 0.8)])]));
    assert!(near(at(&pic, 120), [255.0, 0.0, 0.0], 2.0), "t 0.1 holds the first colour: {:?}", at(&pic, 120));
    assert!(near(at(&pic, 280), [0.0, 0.0, 255.0], 2.0), "t 0.9 holds the last: {:?}", at(&pic, 280));
    assert!(near(at(&pic, 200), [127.5, 0.0, 127.5], 5.0), "the middle of the two: {:?}", at(&pic, 200));
}

#[test]
fn two_stops_at_one_position_make_a_hard_edge_and_the_rows_are_taken_in_position_order() {
    // Red at 0, green at 0.5 and blue at 0.5 (the later row after the earlier): green up to the middle, blue after it. The
    // rows are listed out of order, with the last one first in the list, and the sort is stable.
    let pic = draw(&one_page(vec![along_x(&[("#0000FF", 0.0, 0.5), ("#FF0000", 0.0, 0.0), ("#00FF00", 0.0, 0.5)])]));
    // Row order is the tie-break: blue (row 0) then green (row 2) at 0.5 after the sort; red at 0.
    assert!(
        near(at(&pic, 120), [255.0 * (1.0 - 0.1 / 0.5), 0.0, 255.0 * (0.1 / 0.5)], 6.0),
        "between red and the first 0.5 stop: {:?}",
        at(&pic, 120)
    );
    assert!(near(at(&pic, 280), [0.0, 255.0, 0.0], 3.0), "after the edge: the stop the sort leaves last: {:?}", at(&pic, 280));
}

#[test]
fn only_the_first_ten_rows_are_stops() {
    // Twelve rows, the first ten count. Row 0 is red at the start; rows 1 to 9 are green; rows 10 and 11 are blue at 0.91
    // and 1 and are past the ten: the colour beyond row 9 (at 9/11 = 0.818) is its green, held to the end.
    let mut sb = rect(1, 1.0, 1.0, 2.0, 2.0)
        .cell("LinePattern", "0")
        .cell("FillGradientEnabled", "1")
        .cell("FillGradientDir", "0")
        .cell("FillGradientAngle", "0");
    for i in 0..12u32 {
        let row = RowKey::Ix(i);
        let color = if i == 0 {
            "#FF0000"
        } else if i >= 10 {
            "#0000FF"
        } else {
            "#00FF00"
        };
        sb = sb
            .row_cell("FillGradient", None, row.clone(), "GradientStopColor", color)
            .row_cell("FillGradient", None, row.clone(), "GradientStopColorTrans", "0")
            .row_cell("FillGradient", None, row, "GradientStopPosition", &(f64::from(i) / 11.0).to_string());
    }
    let pic = draw(&one_page(vec![sb]));
    assert!(near(at(&pic, 100), [255.0, 0.0, 0.0], 12.0), "row 0 is the first stop: {:?}", at(&pic, 100));
    assert!(near(at(&pic, 290), [0.0, 255.0, 0.0], 3.0), "rows 10 and 11 are past the ten: {:?}", at(&pic, 290));
}
