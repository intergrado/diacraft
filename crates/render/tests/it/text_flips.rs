//! Text under a flip at any level.
//!
//! No text record in the sample is mirrored once its own transform and those of every group above it are taken, and 272 sit under a
//! mirroring group with a counter-mirror of their own. The shape's own flip is cancelled by `text.rs`; these tests hold the flips of
//! the groups above a shape to the same, with an `L`, which has its stem on the left and its bar along the bottom and so is none of its mirror
//! images. Where the angle of a block under a group's flip is not in the specification, the rule here is ours: a flip of
//! a group is cancelled in the block's frame about the block's local pin, as the shape's own is, so the net turn of the text is the at
//! every level, and a member of a flipped group draws its text as the shape would draw it were it flipped itself.

use diacraft_doc::RowKey;
use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

/// An `L` of 1.4 inch in a 2 by 2 inch box with its lower left corner at (`x`, `y`) and its pin at the middle, so that a flip of the shape
/// leaves the box where it is; no fill and no line, black.
fn letter(id: u32, x: f64, y: f64) -> ShapeBuilder {
    rect(id, x, y, 2.0, 2.0)
        .cell("PinX", &(x + 1.0).to_string())
        .cell("PinY", &(y + 1.0).to_string())
        .cell("LocPinX", "1")
        .cell("LocPinY", "1")
        .cell("FillPattern", "0")
        .cell("LinePattern", "0")
        .text("L")
        .row_cell("Character", None, RowKey::Ix(0), "Size", "1.4")
        .row_cell("Character", None, RowKey::Ix(0), "Color", "#000000")
}

/// A group 4 by 4 over the whole page, turned `degrees` and flipped as asked about its middle, which stays at (2, 2).
fn group(id: u32, flip_x: bool, flip_y: bool, degrees: f64) -> ShapeBuilder {
    shape_at(id, 2.0, 2.0, 4.0, 4.0)
        .cell("LocPinX", "2")
        .cell("LocPinY", "2")
        .cell("FlipX", if flip_x { "1" } else { "0" })
        .cell("FlipY", if flip_y { "1" } else { "0" })
        .cell("Angle", &degrees.to_radians().to_string())
        .group()
}

const FLIPS: [(bool, bool); 4] = [(false, false), (true, false), (false, true), (true, true)];

/// How many dark pixels lie left of, right of, above and below the middle of the ink: (left, right, top, bottom).
fn halves(pic: &Pic) -> (usize, usize, usize, usize) {
    let b = pic.ink_box().expect("the letter is drawn");
    let (mid_x, mid_y) = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
    let (w, h) = (pic.r.width as usize, pic.r.height as usize);
    let (mut left, mut right, mut top, mut bottom) = (0usize, 0usize, 0usize, 0usize);
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

fn centre(pic: &Pic) -> (f64, f64) {
    let b = pic.ink_box().expect("the letter is drawn");
    ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0)
}

/// The `L` stands upright: its stem on the left and its bar at the bottom.
fn assert_upright(pic: &Pic, what: &str) {
    let (left, right, top, bottom) = halves(pic);
    assert!(left > right, "{what}: the stem is on the left ({left} against {right})");
    assert!(bottom > top, "{what}: the bar is at the bottom ({bottom} against {top})");
}

/// Where the middle of a box at (0.2, 0.5) to (2.2, 2.5) of a group's frame is on the page when the group is flipped as asked about (2, 2).
fn expected_middle(flip_x: bool, flip_y: bool) -> (f64, f64) {
    (if flip_x { 4.0 - 1.2 } else { 1.2 }, if flip_y { 4.0 - 1.5 } else { 1.5 })
}

#[test]
fn the_text_of_a_member_of_a_flipped_group_is_not_mirrored_and_goes_where_the_flip_puts_the_shape() {
    // The member is not flipped itself; its group is, about the middle of the page. The member's box is off the middle, so a
    // flip carries it across, and the letter goes with it, upright.
    for (fx, fy) in FLIPS {
        let pic = draw(&one_page(vec![group(10, fx, fy, 0.0).child(letter(11, 0.2, 0.5))]));
        assert_upright(&pic, &format!("group FlipX {fx} FlipY {fy}"));
        let (cx, cy) = centre(&pic);
        let (ex, ey) = expected_middle(fx, fy);
        assert!((cx - ex).abs() < 0.3 && (cy - ey).abs() < 0.3, "group FlipX {fx} FlipY {fy}: the ink is at ({cx}, {cy}), the box at ({ex}, {ey})");
    }
}

#[test]
fn a_member_that_is_flipped_itself_in_a_flipped_group_is_not_mirrored_either() {
    // The flips of the group and of the member each cancel: every one of the sixteen pairs reads upright.
    for (gx, gy) in FLIPS {
        for (fx, fy) in FLIPS {
            let member = letter(11, 0.2, 0.5).cell("FlipX", if fx { "1" } else { "0" }).cell("FlipY", if fy { "1" } else { "0" });
            let pic = draw(&one_page(vec![group(10, gx, gy, 0.0).child(member)]));
            assert_upright(&pic, &format!("group FlipX {gx} FlipY {gy}, member FlipX {fx} FlipY {fy}"));
        }
    }
}

#[test]
fn text_nested_two_groups_deep_is_not_mirrored_by_either_group() {
    // Each of the sixteen pairs of flips, outer and inner, reads upright, and the box ends where the two flips together put it: the flips of
    // one axis in both groups cancel.
    for (ox, oy) in FLIPS {
        for (ix, iy) in FLIPS {
            let inner = group(11, ix, iy, 0.0).child(letter(12, 0.2, 0.5));
            let pic = draw(&one_page(vec![group(10, ox, oy, 0.0).child(inner)]));
            let what = format!("outer FlipX {ox} FlipY {oy}, inner FlipX {ix} FlipY {iy}");
            assert_upright(&pic, &what);
            let (cx, cy) = centre(&pic);
            let (ex, ey) = expected_middle(ox != ix, oy != iy);
            assert!((cx - ex).abs() < 0.3 && (cy - ey).abs() < 0.3, "{what}: the ink is at ({cx}, {cy}), the box at ({ex}, {ey})");
        }
    }
}

#[test]
fn flips_in_different_frames_turn_the_text_as_the_picture_turns_and_do_not_mirror_it() {
    // Three groups: the outer one flipped in x, the middle one turned a quarter counterclockwise, the inner one flipped in y. The outer mirror
    // makes the middle group's quarter turn a quarter turn clockwise on the page, and the two flips are each cancelled for the text, so the text
    // runs a quarter turn clockwise: down the page. An `L` so turned has its stem along the top and its bar down the left side. (A mirror that
    // reached the glyphs would put the stem along the bottom.)
    let inner = group(12, false, true, 0.0).child(letter(13, 1.0, 1.0));
    let middle = group(11, false, false, 90.0).child(inner);
    let pic = draw(&one_page(vec![group(10, true, false, 0.0).child(middle)]));
    let (left, right, top, bottom) = halves(&pic);
    assert!(top > bottom, "the stem is along the top ({top} against {bottom})");
    assert!(left > right, "the bar is down the left ({left} against {right})");
}

#[test]
fn text_in_a_turned_and_flipped_group_is_turned_with_the_group_and_not_mirrored() {
    // A quarter turn counterclockwise puts the stem along the bottom and the bar up the right side: more ink below the middle than above and
    // more to the right than to the left. A flip before the turn that reached the glyphs would put the stem along the top.
    for (fx, fy) in [(true, false), (false, true), (true, true)] {
        let pic = draw(&one_page(vec![group(10, fx, fy, 90.0).child(letter(11, 1.0, 1.0))]));
        let (left, right, top, bottom) = halves(&pic);
        assert!(bottom > top, "group FlipX {fx} FlipY {fy} turned 90: the stem is along the bottom ({bottom} against {top})");
        assert!(right > left, "group FlipX {fx} FlipY {fy} turned 90: the bar is up the right ({right} against {left})");
    }
    // The control: the group turned and not flipped reads the same.
    let pic = draw(&one_page(vec![group(10, false, false, 90.0).child(letter(11, 1.0, 1.0))]));
    let (left, right, top, bottom) = halves(&pic);
    assert!(bottom > top && right > left, "a turned group alone: {:?}", (left, right, top, bottom));
}

#[test]
fn a_member_of_a_flipped_group_draws_its_text_as_the_shape_would_flipped_itself() {
    // Our rule: the block turned by 30 degrees, in a member of a group flipped about the middle, is the picture of the same
    // shape, flipped itself and at the middle, with the same `TxtAngle`. Gives the second (`Angle` - `TxtAngle` under one flip,
    // `Angle` + `TxtAngle` under two), and the first is held to it.
    let turned = |sb: ShapeBuilder| sb.cell("TxtAngle", &30f64.to_radians().to_string());
    let differing = |a: &Pic, b: &Pic| {
        a.r.rgba_premul.chunks(4).zip(b.r.rgba_premul.chunks(4)).filter(|(p, q)| p.iter().zip(q.iter()).any(|(x, y)| x.abs_diff(*y) > 3)).count()
    };
    for (fx, fy) in FLIPS {
        let alone = letter(1, 1.0, 1.0).cell("FlipX", if fx { "1" } else { "0" }).cell("FlipY", if fy { "1" } else { "0" });
        let alone = draw(&one_page(vec![turned(alone)]));
        let member = turned(letter(11, 1.0, 1.0));
        let grouped = draw(&one_page(vec![group(10, fx, fy, 0.0).child(member)]));
        assert!(alone.ink_pixels() > 200, "FlipX {fx} FlipY {fy}: the letter is drawn");
        assert_eq!(alone.ink_pixels() > 0, grouped.ink_pixels() > 0);
        let n = differing(&alone, &grouped);
        assert!(n < 12, "FlipX {fx} FlipY {fy}: the group's picture differs from the flipped shape's in {n} pixels");
    }
}

#[test]
fn a_group_that_is_not_flipped_leaves_the_text_of_its_members_as_it_was() {
    // The control: the group is turned and moved, and a flip of the member itself keeps: upright, in the place the member is.
    let pic = draw(&one_page(vec![group(10, false, false, 0.0).child(letter(11, 0.2, 0.5))]));
    assert_upright(&pic, "an unflipped group");
    let (cx, cy) = centre(&pic);
    assert!((cx - 1.2).abs() < 0.3 && (cy - 1.5).abs() < 0.3, "({cx}, {cy})");
}

#[test]
fn the_text_of_a_shape_a_flipped_instance_takes_from_its_master_is_not_mirrored() {
    // The members of an instance of a master group stand for the master's shapes, and the instance is the group above them: its flip is the
    // flip of a group, and the letter of the master's shape is upright.
    for (fx, fy) in FLIPS {
        let master = diacraft_testkit::build::MasterBuilder::new().shape(group(10, false, false, 0.0).child(letter(11, 0.2, 0.5)));
        let instance = group(100, fx, fy, 0.0).master(1).child(ShapeBuilder::new(101).master_shape(11));
        let doc = base().master(1, master).page(1, page(4.0, 4.0).shape(instance)).build();
        let pic = draw(&doc);
        assert_upright(&pic, &format!("instance FlipX {fx} FlipY {fy}"));
        let (cx, cy) = centre(&pic);
        let (ex, ey) = expected_middle(fx, fy);
        assert!(
            (cx - ex).abs() < 0.3 && (cy - ey).abs() < 0.3,
            "instance FlipX {fx} FlipY {fy}: the ink is at ({cx}, {cy}), the box at ({ex}, {ey})"
        );
    }
}
