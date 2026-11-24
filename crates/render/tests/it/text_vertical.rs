//! Vertical text (`TextDirection` 1): a letter that is not East Asian lies on its side, turned a quarter
//! clockwise, and the characters run down the block from its top. Every assertion is about where ink is.

use diacraft_doc::RowKey;
use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

/// A 2 by 2 inch box at (1, 1) with no fill and no line, showing `text` at `size` inches in black, left-aligned.
fn label(text: &str, size: f64) -> ShapeBuilder {
    rect(1, 1.0, 1.0, 2.0, 2.0)
        .cell("FillPattern", "0")
        .cell("LinePattern", "0")
        .text(text)
        .row_cell("Character", None, RowKey::Ix(0), "Size", &size.to_string())
        .row_cell("Character", None, RowKey::Ix(0), "Color", "#000000")
        .row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "0")
}

/// How much dark ink lies in the left, right, top and bottom halves of the ink box.
fn halves(pic: &Pic) -> (usize, usize, usize, usize) {
    let b = pic.ink_box().expect("drawn");
    let (mid_x, mid_y) = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
    let (w, h) = (pic.r.width as usize, pic.r.height as usize);
    let (mut left, mut right, mut top, mut bottom) = (0, 0, 0, 0);
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let p = &pic.r.rgba_premul[i..i + 4];
            if p[3] > 0 && p[0] < 128 {
                let (px, py) = ((x as f64 + 0.5) / pic.ppi, pic.page_h - (y as f64 + 0.5) / pic.ppi);
                if px < mid_x {
                    left += 1
                } else {
                    right += 1
                }
                if py > mid_y { top += 1 } else { bottom += 1 }
            }
        }
    }
    (left, right, top, bottom)
}

#[test]
fn a_letter_lies_on_its_side_in_vertical_text() {
    // An L has its stem on the left and its bar along the bottom: more ink low than high. Turned a quarter clockwise the stem lies along the top and
    // the bar runs down the left: more ink high than low, and still more at the left.
    let across = halves(&draw(&one_page(vec![label("L", 1.4)])));
    assert!(across.3 > across.2 && across.0 > across.1, "upright: {across:?}");
    let down = halves(&draw(&one_page(vec![label("L", 1.4).cell("TextDirection", "1")])));
    assert!(down.2 > down.3 && down.0 > down.1, "on its side: {down:?}");
}

#[test]
fn the_letters_of_vertical_text_run_down_the_block_from_its_top() {
    // "WWWW" at 0.3 inch is wider than tall across, and a column, hugging the top of the 2 by 2 block, down it.
    let across = draw(&one_page(vec![label("WWWW", 0.3)])).ink_box().expect("drawn");
    assert!(across.2 - across.0 > (across.3 - across.1) * 2.0, "across: {across:?}");
    let down = draw(&one_page(vec![label("WWWW", 0.3).cell("TextDirection", "1")])).ink_box().expect("drawn");
    assert!(down.3 - down.1 > (down.2 - down.0) * 2.0, "down: {down:?}");
    assert!(down.3 > 2.85 && down.3 <= 3.0, "it starts at the top of the block (y 3): {down:?}");
    // Centred across the block, which is `VerticalAlign` 1: the column's centre is at x 2.
    assert!(((down.0 + down.2) / 2.0 - 2.0).abs() < 0.2, "{down:?}");
}

#[test]
fn vertical_align_seats_vertical_text_from_the_right_or_the_left() {
    // `VerticalAlign` 0 from the right edge, 2 from the left.
    let at = |v: &str| {
        let b = draw(&one_page(vec![label("WWWW", 0.3).cell("TextDirection", "1").cell("VerticalAlign", v)])).ink_box().expect("drawn");
        (b.0 + b.2) / 2.0
    };
    let (right, middle, left) = (at("0"), at("1"), at("2"));
    assert!(right > middle + 0.5 && middle > left + 0.5, "right {right}, middle {middle}, left {left}");
}

#[test]
fn an_underline_lies_down_the_side_of_vertical_text() {
    let b = draw(&one_page(vec![label("WWWW", 0.3).cell("TextDirection", "1").row_cell("Character", None, RowKey::Ix(0), "Style", "4")]))
        .ink_box()
        .expect("drawn");
    assert!(b.3 - b.1 > (b.2 - b.0) * 2.0, "{b:?}");
}
