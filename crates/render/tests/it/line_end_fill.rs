//! The inside of a hollow line end takes the shape's own fill (the owner's ruling of 2026-11-26, which supersedes: "Connectors should use the fill
//! colour/effects to fill the closed parts of a connector/line ending. For example, the square's fill is always transparent, but I should be able to colour it").
//!
//! * **Hollow marks** (the numbers whose closed parts are outlines: 14 to 22, 29 to 34, 40 and 41) paint the shape's fill, in its colour, transparency, pattern or gradient, inside the
//!   closed part, **before** the outline; the outline keeps the line's colour and transparency.
//! * **Solid marks** take the line's colour as they did, and **open marks** have no closed part.
//! * A shape with no fill (`FillPattern` 0) stays transparent inside, as it was.
//! * A mark stays whole on a dashed line, and so does what is inside it.
//! * A gradient is measured over the mark's own box, in the shape's own frame (the rule `draw_gradient` has for a shape: the gradient turns with the shape unless it says it does not).

use diacraft_render::limits::{PREVIEW_MAX_WORK, WORK_PER_END_FILL};
use diacraft_render::{Limits, Renderer, SkipKind, View};

use crate::support::*;

/// A horizontal line 3 inches long from (0.5, `y`) to (3.5, `y`) with the end `number` at class `size`, as a connector is: its geometry says `NoFill`, its fill cells say a solid red
/// fill, its line is black and `weight` inches heavy. `fill` is the shape's `FillForegnd` and pattern 1.
fn connector(id: u32, y: f64, weight: f64, number: i64, size: i64) -> diacraft_testkit::build::ShapeBuilder {
    let line = geometry_of(shape_at(id, 0.5, y, 3.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.0), ("Y", 0.0)])]);
    geom_flag(line, "NoFill", "1")
        .cell("FillPattern", "1")
        .cell("FillForegnd", "#FF0000")
        .cell("FillBkgnd", "#00FF00")
        .cell("LineWeight", &weight.to_string())
        .cell("LinePattern", "1")
        .cell("LineCap", "1")
        .cell("LineColor", "#000000")
        .cell("EndArrow", &number.to_string())
        .cell("EndArrowSize", &size.to_string())
}

/// The point `t` units back from the tip of a mark at the end (3.5, `y`), `a` units across the line, `s` inches to the unit.
fn at_s(y: f64, s: f64, t: f64, a: f64) -> (f64, f64) {
    (3.5 - t * s, y + a * s)
}

/// The colour of the pixel at `(x, y)` as red, green and blue over the white paper.
fn rgb(pic: &Pic, x: f64, y: f64) -> [u8; 3] {
    let p = pic.at(x, y);
    [p[0], p[1], p[2]]
}

/// Class 6 and a weight of 0.02 inch: `S` is 0.25 + 0.02 = 0.27 inch, 27 pixels to the unit, so every probe is well inside or on an outline.
const S: f64 = 0.27;

/// The hollow numbers, each with a point well inside its closed parts (`t` back from the tip and `a` across, in `S`), and a point on an outline away from the axis, where the
/// shaft cannot be. Typed from `ends.rs`'s art: 29 and 30 are a hollow disc behind a fork or a bar, 31 and 33 a hollow disc with bars, 34 a hollow disc and a
/// hollow long diamond (two closed parts, so two interiors), 40 two hollow triangles.
type Probe = (i64, &'static [(f64, f64)], (f64, f64));
const HOLLOW: [Probe; 17] = [
    (14, &[(1.5, 0.0)], (1.5, 0.5)),
    (15, &[(0.5, 0.0)], (0.5, 0.5)),
    (16, &[(1.0, 0.0)], (1.0, 0.5)),
    (17, &[(0.8, 0.0)], (1.0, 0.5)),
    (18, &[(1.0, 0.0)], (1.0, 0.505)),
    (19, &[(0.8, 0.0)], (1.0, 0.5)),
    (20, &[(0.0, 0.0)], (0.0, 0.75)),
    (21, &[(0.0, 0.0)], (0.0, 0.75)),
    (22, &[(2.25, 0.0)], (1.125, 0.56)),
    (29, &[(2.25, 0.0)], (2.25, 0.75)),
    (30, &[(2.25, 0.0)], (2.25, 0.75)),
    (31, &[(0.75, 0.0)], (0.75, 0.75)),
    (33, &[(0.75, 0.0)], (0.75, 0.75)),
    (34, &[(0.75, 0.0), (3.75, 0.0)], (2.625, 0.56)),
    (40, &[(1.0, 0.0), (3.0, 0.0)], (1.0, 0.5)),
    (41, &[(0.75, 0.0)], (0.75, 0.75)),
    (32, &[(0.75, 0.0)], (0.75, 0.75)),
];

#[test]
fn a_hollow_mark_is_filled_with_the_shapes_fill_inside_and_outlined_in_the_line_colour() {
    // The owner's ruling of 2026-11-26, as amended: the closed part of a hollow mark takes the fill (red here), the outline stays the line's black, and what is outside is paper.
    let mut wrong = Vec::new();
    for (number, inside, edge) in HOLLOW {
        let pic = draw(&one_page(vec![connector(1, 2.0, 0.02, number, 6)]));
        for (t, a) in inside {
            let (x, y) = at_s(2.0, S, *t, *a);
            if !pic.is_red(x, y) {
                wrong.push(format!("{number}: ({t}, {a}) inside is {:?}, want red", rgb(&pic, x, y)));
            }
        }
        let (x, y) = at_s(2.0, S, edge.0, edge.1);
        if !pic.is_dark(x, y) {
            wrong.push(format!("{number}: ({}, {}) on the outline is {:?}, want the line's black", edge.0, edge.1, rgb(&pic, x, y)));
        }
        // Past the tip nothing is drawn, and across the line beyond the mark's spread it is paper.
        let (x, y) = at_s(2.0, S, -1.2, 0.0);
        if !pic.is_white(x, y) {
            wrong.push(format!("{number}: past the tip is {:?}", rgb(&pic, x, y)));
        }
    }
    assert!(wrong.is_empty(), "{} probes wrong:\n{}", wrong.len(), wrong.join("\n"));
}

#[test]
fn the_hollow_square_of_the_owners_example_can_be_coloured_at_either_end() {
    // "The square's fill is always transparent, but I should be able to colour it": a hollow square (21) at the begin and the end, each red inside, the line black.
    let line = connector(1, 2.0, 0.02, 21, 6).cell("BeginArrow", "21").cell("BeginArrowSize", "6");
    let pic = draw(&one_page(vec![line]));
    for x in [0.5, 3.5] {
        assert!(pic.is_red(x, 2.0), "inside the square centred on ({x}, 2)");
        assert!(pic.is_dark(x, 2.0 + 0.75 * S), "its outline at {x}");
    }
    assert!(pic.is_dark(2.0, 2.0) && pic.is_white(2.0, 2.3), "the shaft between them is the line's");
}

#[test]
fn solid_marks_take_the_lines_colour_and_open_marks_have_no_closed_part_to_fill() {
    // 4, 10, 11 and 13 are solid: black inside, as they were. 1, 3, 12, 24 and 43 are open: no ink is red anywhere, and between the strokes it is paper.
    for number in [2, 4, 5, 6, 10, 11, 13, 39] {
        let pic = draw(&one_page(vec![connector(1, 2.0, 0.02, number, 6)]));
        let (x, y) = at_s(2.0, S, if number == 10 || number == 11 { 0.0 } else { 0.8 }, 0.0);
        assert!(pic.is_dark(x, y), "{number} is solid in the line's colour, not the fill's: {:?}", rgb(&pic, x, y));
    }
    for number in [1, 3, 7, 12, 23, 24, 25, 26, 27, 28, 43, 44, 45] {
        let pic = draw(&one_page(vec![connector(1, 2.0, 0.02, number, 6)]));
        let red = pic.r.rgba_premul.chunks(4).filter(|p| p[3] > 0 && p[0] > 200 && p[1] < 60 && p[2] < 60).count();
        assert_eq!(red, 0, "{number} is open: nothing of it is filled");
    }
    // The solid part of a mark that has a hollow one (38: a solid disc and a hollow diamond) stays the line's, and the hollow one is filled.
    let pic = draw(&one_page(vec![connector(1, 2.0, 0.02, 38, 6)]));
    let (x, y) = at_s(2.0, S, 0.75, 0.0);
    assert!(pic.is_dark(x, y), "38's disc is solid");
    let (x, y) = at_s(2.0, S, 3.75, 0.0);
    assert!(pic.is_red(x, y), "38's diamond is hollow: {:?}", rgb(&pic, x, y));
}

#[test]
fn a_shape_with_no_fill_stays_transparent_inside_its_hollow_mark() {
    // `FillPattern` 0: nothing to paint, so what is inside shows what is behind (here the paper, and a blue box).
    for number in [14, 21, 22] {
        let line = connector(1, 2.0, 0.02, number, 6).cell("FillPattern", "0");
        let behind = rect(2, 3.0, 1.5, 1.0, 1.0).cell("FillForegnd", "#0000FF").cell("LinePattern", "0");
        let pic = draw(&one_page(vec![behind, line]));
        let (x, y) = at_s(2.0, S, if number == 21 { 0.0 } else { 1.0 }, 0.0);
        assert!(pic.is(x, y, (0, 0, 255), 6), "{number}: the box behind shows inside the mark: {:?}", rgb(&pic, x, y));
    }
}

#[test]
fn the_fill_keeps_its_transparency_inside_and_the_outline_keeps_the_lines() {
    // A half see-through fill is red at half over what is behind (the paper here): (255, 128, 128). The outline is the line's own colour, alone.
    let pic = draw(&one_page(vec![connector(1, 2.0, 0.02, 21, 6).cell("FillForegndTrans", "0.5")]));
    let [r, g, b] = rgb(&pic, 3.5, 2.0);
    assert!(r > 250 && (120..=136).contains(&g) && (120..=136).contains(&b), "half red over white: {:?}", [r, g, b]);
    assert!(pic.is_dark(3.5, 2.0 + 0.75 * S), "the outline is black");
    // A half see-through line, 0.06 inch heavy (S is 0.31 inch, the outline 6 pixels wide and centred on the square's side 0.2325 inch from the middle). The inside is the fill's red whole,
    // with no tint of the line's. The outline over the fill is the line's black at half over red, as a shape's own outline is over its fill; on its outer half it is that black at half
    // over the paper, a grey: the outline's colour and transparency are the line's alone.
    let pic = draw(&one_page(vec![connector(1, 2.0, 0.06, 21, 6).cell("LineColorTrans", "0.5")]));
    assert!(pic.is_red(3.5, 2.0), "the inside is the fill's own colour: {:?}", rgb(&pic, 3.5, 2.0));
    let side = 0.75 * 0.31;
    let [r, g, b] = rgb(&pic, 3.5, 2.0 + side + 0.0175);
    assert!(
        (110..=146).contains(&r) && r.abs_diff(g) < 12 && r.abs_diff(b) < 12,
        "the line's black at half over the paper, on the outer half of the outline: {:?}",
        [r, g, b]
    );
    let [r, g, b] = rgb(&pic, 3.5, 2.0 + side - 0.0175);
    assert!((110..=146).contains(&r) && g < 12 && b < 12, "the same black at half over the fill's red, on the inner half: {:?}", [r, g, b]);
}

#[test]
fn a_gradient_runs_across_the_marks_own_box_in_the_shapes_frame() {
    // A left to right gradient from red to blue (angle 0, the stops of `FillGradient`). The shape is 3 inches long; measured over the shape's box both sides of the mark would
    // be about the same blue. Measured over the mark's own box (the square, 0.405 inch wide) its left side is red and its right side blue, and the line's frame is the page's.
    let stops = [(0, 0.0, "#FF0000"), (1, 1.0, "#0000FF")];
    let mut line = connector(1, 2.0, 0.02, 21, 6).cell("FillGradientEnabled", "1").cell("FillGradientDir", "0").cell("FillGradientAngle", "0");
    for (ix, pos, colour) in stops {
        let key = diacraft_doc::RowKey::Ix(ix);
        line = line.row_cell("FillGradient", None, key.clone(), "GradientStopColor", colour).row_cell(
            "FillGradient",
            None,
            key,
            "GradientStopPosition",
            &pos.to_string(),
        );
    }
    let pic = draw(&one_page(vec![line]));
    let (left, right) = (rgb(&pic, 3.5 - 0.15, 2.0).map(i32::from), rgb(&pic, 3.5 + 0.15, 2.0).map(i32::from));
    assert!(left[0] > right[0] + 80 && right[2] > left[2] + 80, "red on the left, blue on the right of the square: {left:?} {right:?}");
    assert!(left[0] > 150 && right[2] > 150, "each end is mostly its stop: {left:?} {right:?}");
}

#[test]
fn a_pattern_fill_paints_its_marks_inside_the_hollow_mark() {
    // Pattern 13 (heavy horizontal bars): red bars on a green ground. Every pixel well inside the square is red or green, and both show.
    let line = connector(1, 2.0, 0.02, 21, 6).cell("FillPattern", "13");
    let pic = draw(&one_page(vec![line]));
    let (mut red, mut green, mut blend) = (0, 0, 0);
    for k in 0..30 {
        for j in 0..30 {
            let (x, y) = (3.5 - 0.15 + f64::from(k) * 0.01, 2.0 - 0.15 + f64::from(j) * 0.01);
            let p = pic.at(x, y);
            assert!(
                p[3] > 250 && p[2] < 30 && u32::from(p[0]) + u32::from(p[1]) > 200,
                "({x}, {y}) is a red and green mixture, no paper and no ink: {p:?}"
            );
            if pic.is_red(x, y) {
                red += 1;
            } else if pic.is(x, y, (0, 255, 0), 12) {
                green += 1;
            } else {
                blend += 1;
            }
        }
    }
    assert!(red > 100 && green > 100, "bars and ground both show: {red} red, {green} green, {blend} where a bar's edge meets the ground");
}

#[test]
fn a_dashed_line_does_not_dash_the_mark_or_what_is_inside_it() {
    // A mark stays whole on a dashed line. The fill is whole too: every probe inside the square and on its four sides.
    for pattern in [2, 3, 5, 9, 23] {
        let pic = draw(&one_page(vec![connector(1, 2.0, 0.02, 21, 6).cell("LinePattern", &pattern.to_string())]));
        for (dx, dy) in [(0.0, 0.0), (0.1, 0.1), (-0.1, 0.1), (0.1, -0.1), (-0.1, -0.1), (0.15, 0.0), (-0.15, 0.0)] {
            assert!(pic.is_red(3.5 + dx, 2.0 + dy), "pattern {pattern}: inside at ({dx}, {dy}): {:?}", rgb(&pic, 3.5 + dx, 2.0 + dy));
        }
        for (dx, dy) in [(0.0, 0.75 * S), (0.0, -0.75 * S), (0.75 * S, 0.1), (-0.75 * S, 0.1), (-0.75 * S, -0.1)] {
            assert!(pic.is_dark(3.5 + dx, 2.0 + dy), "pattern {pattern}: outline at ({dx}, {dy})");
        }
    }
}

#[test]
fn a_turned_line_fills_its_mark_turned_and_a_slanting_one_too() {
    // A line from (0.5, 0.5) to (3.5, 3.0): the triangle (14) at its end points along it, its tip on the vertex, red inside along the axis.
    let line = geom_flag(
        geometry_of(shape_at(1, 0.5, 0.5, 3.0, 2.5), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.0), ("Y", 2.5)])]),
        "NoFill",
        "1",
    )
    .cell("FillPattern", "1")
    .cell("FillForegnd", "#FF0000")
    .cell("LineWeight", "0.02")
    .cell("LinePattern", "1")
    .cell("LineColor", "#000000")
    .cell("EndArrow", "14")
    .cell("EndArrowSize", "6");
    let pic = draw(&one_page(vec![line]));
    let len = (3.0_f64.powi(2) + 2.5_f64.powi(2)).sqrt();
    let (ux, uy) = (3.0 / len, 2.5 / len);
    // 1.5 S back from the tip along the axis: inside the triangle of length 3 S.
    let (x, y) = (3.5 - 1.5 * S * ux, 3.0 - 1.5 * S * uy);
    assert!(pic.is_red(x, y), "inside the turned triangle: {:?}", rgb(&pic, x, y));
    // Across the axis at 2.5 S back the triangle (half width 0.83 S there) does not reach 1.2 S away: paper.
    let (x, y) = (3.5 - 1.0 * S * ux - 1.2 * S * uy, 3.0 - 1.0 * S * uy + 1.2 * S * ux);
    assert!(pic.is_white(x, y), "beside the mark it is paper: {:?}", rgb(&pic, x, y));
}

#[test]
fn a_transparency_that_is_no_number_or_out_of_range_is_read_as_the_cell_reads_it_and_nothing_fails() {
    // `FillForegndTrans` that is not a number reads as opaque, a negative one is clamped to opaque, one above 1 is clear (and so is infinity, which a cell reads as the largest number a
    // drawing holds): the same as for a shape's fill (`with_trans`).
    for (value, opaque) in
        [("1.#QNAN", true), ("-1", true), ("-1e300", true), ("2", false), ("1e300", false), ("1.#INF", false), ("0", true), ("1", false)]
    {
        let pic = draw(&one_page(vec![connector(1, 2.0, 0.02, 21, 6).cell("FillForegndTrans", value)]));
        assert_eq!(pic.r.count(SkipKind::ShapeFailed) + pic.r.count(SkipKind::Recovered), 0, "{value}: {:?}", pic.r.skipped);
        let inside = rgb(&pic, 3.5, 2.0);
        if opaque {
            assert!(pic.is_red(3.5, 2.0), "{value}: opaque red, got {inside:?}");
        } else {
            assert!(pic.is_white(3.5, 2.0), "{value}: clear, got {inside:?}");
        }
    }
}

#[test]
fn the_fill_adds_no_shadow_since_a_line_end_casts_none() {
    // The shadow of a shape is its fill and its outline moved (the shaft here); a line end has never cast one, solid or hollow, and the fill inside a hollow one adds none: the
    // pictures of a connector with a shadow, with a fill and with none, differ only inside the mark's own box.
    let shadowed = |fill: &str| {
        let line = connector(1, 2.0, 0.02, 21, 6)
            .cell("FillPattern", fill)
            .cell("ShdwPattern", "1")
            .cell("ShapeShdwType", "1")
            .cell("ShapeShdwOffsetX", "0.1")
            .cell("ShapeShdwOffsetY", "-0.1")
            .cell("ShdwForegnd", "#00FF00");
        draw(&one_page(vec![line]))
    };
    let (with, without) = (shadowed("1"), shadowed("0"));
    let width = with.r.width as usize;
    let mut outside = 0;
    let mut inside = 0;
    for (k, (a, b)) in with.r.rgba_premul.chunks(4).zip(without.r.rgba_premul.chunks(4)).enumerate() {
        if a == b {
            continue;
        }
        // The pixel's place in paper inches: the mark's box is the square 0.2025 inch each way of (3.5, 2), with the outline's half width.
        let (x, y) = ((k % width) as f64 / 100.0 + 0.005, with.page_h - (k / width) as f64 / 100.0 - 0.005);
        if (x - 3.5).abs() <= 0.2125 && (y - 2.0).abs() <= 0.2125 {
            inside += 1;
        } else {
            outside += 1;
        }
    }
    assert!(inside > 100, "the fill is there: {inside} pixels differ inside the mark");
    assert_eq!(outside, 0, "and nothing outside the mark's box differs: the fill casts no shadow");
}

#[test]
fn a_gradient_whose_stops_are_no_numbers_or_out_of_range_fills_the_mark_without_a_fault() {
    // The stops of a gradient: a position that is no number reads 0, one outside 0 to 1 is cut to it, a transparency that is no number is opaque and one past 1 is clear. The
    // mark is filled whatever they say, and nothing fails.
    for (pos, trans) in [("1.#QNAN", "0"), ("-5", "0"), ("7", "0"), ("0", "1.#QNAN"), ("1", "-5"), ("1", "9"), ("1.#INF", "1.#INF")] {
        let mut line = connector(1, 2.0, 0.02, 21, 6).cell("FillGradientEnabled", "1").cell("FillGradientDir", "0").cell("FillGradientAngle", "0");
        for (ix, p, colour) in [(0u32, "0", "#FF0000"), (1, pos, "#0000FF")] {
            let key = diacraft_doc::RowKey::Ix(ix);
            line = line
                .row_cell("FillGradient", None, key.clone(), "GradientStopColor", colour)
                .row_cell("FillGradient", None, key.clone(), "GradientStopColorTrans", if ix == 1 { trans } else { "0" })
                .row_cell("FillGradient", None, key, "GradientStopPosition", p);
        }
        let pic = draw(&one_page(vec![line]));
        assert_eq!(
            pic.r.count(SkipKind::ShapeFailed) + pic.r.count(SkipKind::Recovered),
            0,
            "position {pos}, transparency {trans}: {:?}",
            pic.r.skipped
        );
        assert!(pic.is_dark(3.5, 2.0 + 0.75 * S), "the outline is still the line's: position {pos}, transparency {trans}");
    }
}

#[test]
fn a_mark_at_the_smallest_and_the_largest_size_is_filled_without_a_fault() {
    // Class 0 at weight 0 is the smallest mark there is (S 0.02 inch, an outline of a hairline); class 6 at a weight of a thousand points is the largest the cells allow
    // (S about 14 inches), and a weight past what a stroke is (1e9, 1e300) is cut to it. None fails; the big ones cover the page in red inside.
    for (class, weight) in [(0, 0.0), (0, 1e-9), (6, 0.0), (6, 13.9), (6, 1e9), (6, 1e300), (6, 1.0e15)] {
        for number in [21, 14, 22] {
            let doc = one_page(vec![connector(1, 2.0, weight, number, class)]);
            let pic = draw(&doc);
            assert_eq!(
                pic.r.count(SkipKind::ShapeFailed) + pic.r.count(SkipKind::Recovered),
                0,
                "{number} class {class} weight {weight}: {:?}",
                pic.r.skipped
            );
        }
    }
    let big = draw(&one_page(vec![connector(1, 2.0, 13.9, 21, 6)]));
    let red = big.r.rgba_premul.chunks(4).filter(|p| p[3] > 0 && p[0] > 200 && p[1] < 60 && p[2] < 60).count();
    assert!(red > 10_000, "a mark 14 inches across fills most of the page: {red} red pixels");
    let tiny = draw(&one_page(vec![connector(1, 2.0, 0.0, 21, 0)]));
    assert_eq!(tiny.r.count(SkipKind::WorkLimit), 0);
}

/// `count` connectors in a grid, each with a hollow square at both ends and a left to right gradient from red to blue, on a page `side` inches square.
fn many(count: u32, side: f64, filled: bool) -> diacraft_doc::Document {
    let columns = (f64::from(count)).sqrt().ceil() as u32;
    let pitch = side / f64::from(columns);
    let mut page = page(side, side);
    for k in 0..count {
        let (x, y) = (f64::from(k % columns) * pitch + pitch * 0.1, f64::from(k / columns) * pitch + pitch * 0.5);
        let mut line = geom_flag(
            geometry_of(
                shape_at(k + 1, x, y, pitch * 0.8, 0.0),
                vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", pitch * 0.8), ("Y", 0.0)])],
            ),
            "NoFill",
            "1",
        )
        .cell("FillPattern", if filled { "1" } else { "0" })
        .cell("FillForegnd", "#FF0000")
        .cell("FillBkgnd", "#0000FF")
        .cell("FillGradientEnabled", "1")
        .cell("FillGradientDir", "0")
        .cell("FillGradientAngle", "0")
        .cell("LineWeight", &(pitch * 0.02).to_string())
        .cell("LinePattern", "1")
        .cell("LineColor", "#000000")
        .cell("BeginArrow", "21")
        .cell("EndArrow", "22")
        .cell("BeginArrowSize", "3")
        .cell("EndArrowSize", "3");
        for (ix, pos, colour) in [(0u32, "0", "#FF0000"), (1, "1", "#0000FF")] {
            let key = diacraft_doc::RowKey::Ix(ix);
            line = line.row_cell("FillGradient", None, key.clone(), "GradientStopColor", colour).row_cell(
                "FillGradient",
                None,
                key,
                "GradientStopPosition",
                pos,
            );
        }
        page = page.shape(line);
    }
    base().page(1, page).build()
}

#[test]
fn ten_thousand_connectors_with_hollow_ends_and_gradients_stay_within_the_frame_budgets_and_are_counted() {
    // 10,000 connectors, two hollow ends each, every one a gradient: within the preview's budget of work (`Limits::preview`, 2,000,000 units, a count and never a time), with every
    // connector drawn and the frame never cut. The fills are work charged before they are made, so what the frame did is a count.
    let count = 10_000u32;
    let ppi = 40.0;
    let counted = |filled: bool| {
        let doc = many(count, 50.0, filled);
        let mut renderer = Renderer::new();
        renderer.set_limits(Limits::preview());
        let view = View::fit_page(&doc, 1, ppi).expect("the page has a size");
        let r = renderer.render(&doc, 1, &view);
        (r, renderer.work_done())
    };
    let (r, work) = counted(true);
    assert_eq!(r.count(SkipKind::WorkLimit), 0, "{:?}", r.skipped);
    assert_eq!(r.count(SkipKind::ShapeFailed) + r.count(SkipKind::Recovered), 0, "{:?}", r.skipped);
    assert!(work <= PREVIEW_MAX_WORK, "{work} units");
    // The lines and the paper are black and white and the squares and diamonds are red to blue: the pixels with a colour are the fills, and a hollow end with no fill leaves none.
    let coloured = r.rgba_premul.chunks(4).filter(|p| p[3] > 250 && p[0].abs_diff(p[2]) > 60).count();
    assert!(coloured > 20_000, "{coloured} pixels are red or blue: 20,000 marks are filled");
    // Counted: the same page with no fill is the work of the shapes alone, and the fills are charged on top: each connector's two hollow marks, one closed part each, cost
    // `WORK_PER_END_FILL` apiece (a linear gradient has no bands), so the difference is exactly two for each connector.
    let (bare, plain) = counted(false);
    assert_eq!(bare.count(SkipKind::WorkLimit), 0, "{:?}", bare.skipped);
    assert!(plain >= WORK_PER_SHAPE_AND_DRAWN * u64::from(count), "{plain} is at least the shapes' own work");
    assert_eq!(
        work - plain,
        2 * u64::from(count) * WORK_PER_END_FILL,
        "the fills are charged before they are painted, two closed parts for each connector"
    );
    let uncoloured = bare.rgba_premul.chunks(4).filter(|p| p[3] > 250 && p[0].abs_diff(p[2]) > 60).count();
    assert_eq!(uncoloured, 0, "a hollow end with no fill leaves nothing red or blue");
}

/// What a connector costs the walk before its marks: entered ([`diacraft_render::limits::WORK_PER_SHAPE`]) and drawn ([`diacraft_render::limits::WORK_PER_DRAWN_SHAPE`]).
const WORK_PER_SHAPE_AND_DRAWN: u64 = diacraft_render::limits::WORK_PER_SHAPE + diacraft_render::limits::WORK_PER_DRAWN_SHAPE;

#[test]
fn a_hollow_end_with_no_fill_costs_no_more_work_than_the_line_with_no_end() {
    // `FillPattern` 0: nothing to paint inside the mark, so nothing is charged for it (the work of the walk and the stroke only).
    let work = |number: i64, fill: &str| {
        let mut renderer = Renderer::new();
        draw_with(&mut renderer, &one_page(vec![connector(1, 2.0, 0.02, number, 6).cell("FillPattern", fill)]));
        renderer.work_done()
    };
    assert_eq!(work(21, "0"), work(0, "0"), "no fill, no charge");
    assert_eq!(work(14, "0"), work(0, "0"), "no fill, no charge");
}
