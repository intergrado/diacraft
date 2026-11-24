//! Line patterns, caps, line ends and weights.

use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

/// A horizontal line 3 inches long from (0.5, 2) with these cells and no fill.
fn line(weight: f64, pattern: u32, cap: u32) -> ShapeBuilder {
    geometry_of(shape_at(1, 0.5, 2.0, 3.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.0), ("Y", 0.0)])])
        .cell("FillPattern", "0")
        .cell("LineWeight", &weight.to_string())
        .cell("LinePattern", &pattern.to_string())
        .cell("LineCap", &cap.to_string())
        .cell("LineColor", "#000000")
}

#[test]
fn a_dash_is_a_multiple_of_the_weight_so_a_heavier_line_dashes_longer() {
    // Pattern 2 is a mark of 7 weights and a gap of 5. Under a round cap the mark as seen is 8 weights (the cap adds half a weight at each
    // end) and the gap 4. Dashes that did not follow the weight would be as long for 0.1 inch as for 0.05.
    let first_run = |weight: f64| {
        let pic = draw(&one_page(vec![line(weight, 2, 0)]));
        let runs = pic.dark_runs_along_x(2.0, 0.0, 4.0);
        assert!(runs.len() >= 3, "a dashed line has several marks, got {runs:?}");
        (runs[0].1, runs[1].0 - (runs[0].0 + runs[0].1))
    };
    let (thin, thin_gap) = first_run(0.05);
    let (thick, thick_gap) = first_run(0.10);
    assert!((38..=42).contains(&thin), "8 weights of 5 pixels, got {thin}");
    assert!((18..=22).contains(&thin_gap), "4 weights of 5 pixels, got {thin_gap}");
    assert!((78..=82).contains(&thick), "8 weights of 10 pixels, got {thick}");
    assert!((38..=42).contains(&thick_gap), "got {thick_gap}");
}

#[test]
fn a_flat_cap_keeps_the_same_mark_as_seen_by_lengthening_the_list() {
    // The table's list is for a round or projecting cap. Under a flat cap each mark is a weight longer and each gap a weight
    // shorter, so a mark still shows as 8 weights. One list for all caps draws the flat cap's dashes 7 weights long.
    let pic = draw(&one_page(vec![line(0.05, 2, 1)]));
    let runs = pic.dark_runs_along_x(2.0, 0.0, 4.0);
    assert!((38..=42).contains(&runs[0].1), "8 weights of 5 pixels, got {runs:?}");
    assert!(runs[0].0 >= 49 && runs[0].0 <= 51, "a flat cap starts at the vertex, not half a weight before it: {runs:?}");
    let square = draw(&one_page(vec![line(0.05, 2, 2)]));
    let runs = square.dark_runs_along_x(2.0, 0.0, 4.0);
    assert!((38..=42).contains(&runs[0].1), "got {runs:?}");
    assert!(runs[0].0 >= 46 && runs[0].0 <= 48, "a projecting cap reaches half a weight before the vertex: {runs:?}");
}

#[test]
fn a_dot_is_a_mark_of_no_length_that_the_cap_makes_visible() {
    // Pattern 3 is a dot and a gap of 5 weights. A dash list of zero-length marks that the rasteriser drops draws nothing.
    let pic = draw(&one_page(vec![line(0.06, 3, 0)]));
    assert!(pic.is_dark(0.5, 2.0), "a round dot sits on the first vertex");
    let runs = pic.dark_runs_along_x(2.0, 0.0, 4.0);
    assert!(runs.len() >= 6, "dots at every six weights along three inches, got {runs:?}");
    assert!(runs.iter().all(|r| r.1 <= 8), "each dot is about a weight wide: {runs:?}");
}

#[test]
fn a_fine_pattern_on_a_long_path_is_drawn_solid_and_says_so() {
    let pic = draw(&one_page(vec![line(0.000001, 2, 0)]));
    assert!(pic.r.skipped.iter().any(|s| s.kind == diacraft_render::SkipKind::DashesTooMany), "{:?}", pic.r.skipped);
}

#[test]
fn a_line_end_has_its_tip_on_the_vertex_and_nothing_goes_past_it() {
    // Number 4 is a solid triangle reaching 2 units back from the vertex and 2 units across, where a unit is the medium size (0.035 inch)
    // plus the line's weight (0.02): 0.055 inch. A mark placed with its back on the vertex would reach 0.11 inch past the tip.
    let arrow = line(0.02, 1, 0).cell("EndArrow", "4").cell("EndArrowSize", "2");
    let pic = draw(&one_page(vec![arrow]));
    assert!(pic.is_dark(3.45, 2.0), "ink right behind the tip");
    assert!(pic.is_white(3.53, 2.0), "nothing past the vertex at 3.5, not even the line's cap");
    assert!(pic.is_white(3.56, 2.0));
    // At 0.08 inch back from the tip the triangle is 0.08 wide on each side of the axis halved: ±0.04.
    assert!(pic.is_dark(3.42, 2.03), "the head is wider than the 0.02 line");
    assert!(pic.is_white(3.42, 2.07), "and no wider than its box");
    // The line itself carries on to the head.
    assert!(pic.is_dark(2.0, 2.0));
}

#[test]
fn the_start_of_a_line_has_its_own_end_with_the_tip_on_the_first_vertex() {
    let arrow = line(0.02, 1, 0).cell("BeginArrow", "4").cell("BeginArrowSize", "2");
    let pic = draw(&one_page(vec![arrow]));
    assert!(pic.is_dark(0.55, 2.0), "ink right behind the tip at 0.5");
    assert!(pic.is_white(0.47, 2.0), "nothing before the first vertex");
    assert!(pic.is_dark(0.58, 2.03));
}

#[test]
fn a_line_end_grows_with_the_weight() {
    // S is the size class's base plus the weight: a 0.1 inch line has a unit of 0.135 inch, so the head reaches 0.27 back from the vertex.
    let heavy = line(0.1, 1, 0).cell("EndArrow", "4").cell("EndArrowSize", "2");
    let pic = draw(&one_page(vec![heavy]));
    assert!(pic.is_dark(3.3, 2.09), "the head still covers 0.2 back, at nearly its full width of ±0.1");
    assert!(pic.is_white(3.56, 2.0));
}

#[test]
fn a_closed_path_has_no_line_ends() {
    let closed = |arrow: &str| {
        geometry_of(shape_at(1, 1.0, 1.0, 2.0, 2.0), rect_rows(2.0, 2.0))
            .cell("FillPattern", "0")
            .cell("LineWeight", "0.02")
            .cell("EndArrow", arrow)
            .cell("BeginArrow", arrow)
    };
    let with = draw(&one_page(vec![closed("4")]));
    let without = draw(&one_page(vec![closed("0")]));
    assert_eq!(with.r.rgba_premul, without.r.rgba_premul, "A closed outline draws no line end");
}

#[test]
fn a_line_with_no_pattern_draws_no_line_and_no_ends() {
    let none = line(0.02, 0, 0).cell("EndArrow", "4");
    assert_eq!(draw(&one_page(vec![none])).ink_pixels(), 0, "LinePattern 0 draws nothing at all");
}

#[test]
fn a_weight_is_on_paper_whatever_the_scale() {
    // At a drawing scale of 1:2: a line weight of 0.1 inch is 10 pixels wide, not 5.
    let l = geometry_of(shape_at(1, 2.0, 4.0, 8.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 8.0), ("Y", 0.0)])])
        .cell("FillPattern", "0")
        .cell("LineWeight", "0.1")
        .cell("LineColor", "#000000");
    let p = diacraft_testkit::build::PageBuilder::new()
        .cell("PageWidth", "8")
        .cell("PageHeight", "8")
        .cell("PageScale", "1")
        .cell("DrawingScale", "2")
        .shape(l);
    let pic = draw(&base().page(1, p).build());
    assert!(pic.is_dark(2.0, 2.04) && pic.is_dark(2.0, 1.96), "0.1 inch of paper is ±0.05 about the line at paper y = 2");
    assert!(pic.is_white(2.0, 2.07) && pic.is_white(2.0, 1.93));
}

/// A polyline shape from `(x, y)` points in page inches, with this weight and cap.
fn polyline(points: &[(f64, f64)], weight: f64, cap: u32) -> ShapeBuilder {
    let mut rows = vec![("MoveTo", vec![("X", points[0].0), ("Y", points[0].1)])];
    rows.extend(points[1..].iter().map(|(x, y)| ("LineTo", vec![("X", *x), ("Y", *y)])));
    geometry_of(shape_at(1, 0.0, 0.0, 4.0, 4.0), rows)
        .cell("FillPattern", "0")
        .cell("LineWeight", &weight.to_string())
        .cell("LineCap", &cap.to_string())
        .cell("LineColor", "#000000")
}

#[test]
fn a_flat_or_projecting_cap_joins_with_a_mitre_up_to_a_limit_of_three_and_a_round_one_with_a_round_join() {
    // A right angle at (3, 1) under a flat cap: the outer corner is square. A bevel (a mitre limit of 1) cuts it off.
    let right = draw(&one_page(vec![polyline(&[(1.0, 1.0), (3.0, 1.0), (3.0, 3.0)], 0.1, 1)]));
    assert!(right.is_dark(3.045, 0.955), "a mitre join reaches the corner of the square");
    let round = draw(&one_page(vec![polyline(&[(1.0, 1.0), (3.0, 1.0), (3.0, 3.0)], 0.1, 0)]));
    assert!(round.is_white(3.045, 0.955), "a round join does not");
    // A corner of 19 degrees would mitre to six weights: past the limit it is bevelled, and the spike is not drawn.
    let sharp = draw(&one_page(vec![polyline(&[(1.0, 2.0), (3.0, 2.0), (1.0, 2.7)], 0.1, 1)]));
    assert!(sharp.is_white(3.2, 1.966), "past the mitre limit of 3 the corner is cut off");
}

#[test]
fn the_shaft_meets_the_back_of_a_solid_mark_with_no_gap_and_stops_at_the_back_of_a_hollow_one() {
    // Number 4 is solid, 16 the same shape hollow. The unit is 0.035 + 0.02 = 0.055, so the back is 0.11 from the vertex at 3.5.
    let solid = draw(&one_page(vec![line(0.02, 1, 0).cell("EndArrow", "4")]));
    assert!(solid.is_dark(3.385, 2.0) && solid.is_dark(3.378, 2.0), "no seam between the shaft and the mark");
    let hollow = draw(&one_page(vec![line(0.02, 1, 0).cell("EndArrow", "16")]));
    assert!(hollow.is_dark(3.378, 2.0), "the shaft reaches the back of the hollow mark");
    assert!(hollow.is_white(3.45, 2.0), "and does not run through it");
}

#[test]
fn a_pattern_finer_than_a_pixel_is_drawn_solid_and_says_so() {
    // The period is 12 weights of 0.0001 inch: 0.12 pixel. It makes only 1,667 dashes on an inch, so only the floor stops it.
    let pic = draw(&one_page(vec![line(0.0001, 2, 0)]));
    assert!(pic.r.count(diacraft_render::SkipKind::DashesTooMany) >= 1, "{:?}", pic.r.skipped);
}

/// A polyline from these points with a number 4 line end on both ends, a weight of 0.02 inch and no fill.
fn arrowed(points: &[(f64, f64)]) -> ShapeBuilder {
    polyline(points, 0.02, 0).cell("BeginArrow", "4").cell("EndArrow", "4")
}

/// Whether two frames hold the same pixels, and the first one's line ends are there: ink right behind each tip of the clean line.
fn same_as_the_clean_line(repeated: &[(f64, f64)]) {
    let clean = draw(&one_page(vec![arrowed(&[(0.5, 2.0), (3.5, 2.0)])]));
    let pic = draw(&one_page(vec![arrowed(repeated)]));
    assert!(pic.is_dark(3.42, 2.03) && pic.is_white(3.42, 2.07), "A head on the last vertex, with its tip there");
    assert!(pic.is_dark(0.58, 2.03) && pic.is_white(0.58, 2.07), "And a head on the first");
    assert_eq!(pic.r.rgba_premul, clean.r.rgba_premul, "a repeated vertex changes nothing about the line");
    assert!(pic.r.skipped.is_empty(), "{:?}", pic.r.skipped);
}

#[test]
fn a_repeated_last_vertex_keeps_both_line_ends() {
    // A LineTo to the point the pen is on has no direction, and the head of the last vertex, and then the first, were lost with it.
    same_as_the_clean_line(&[(0.5, 2.0), (3.5, 2.0), (3.5, 2.0)]);
}

#[test]
fn a_repeated_first_vertex_keeps_both_line_ends() {
    same_as_the_clean_line(&[(0.5, 2.0), (0.5, 2.0), (3.5, 2.0)]);
}

#[test]
fn several_repeated_vertices_at_both_ends_and_between_keep_both_line_ends() {
    same_as_the_clean_line(&[(0.5, 2.0), (0.5, 2.0), (0.5, 2.0), (2.0, 2.0), (2.0, 2.0), (3.5, 2.0), (3.5, 2.0), (3.5, 2.0), (3.5, 2.0)]);
}

#[test]
fn a_head_after_repeated_vertices_lies_along_the_last_segment_that_has_a_direction() {
    // An elbow: up the left from (0.5, 0.5) to (0.5, 2), then right to (3.5, 2), with the last vertex repeated and the first as well. The
    // head at the end points right, and the head at the start points up (its axis is on the nearest segment with a direction).
    let pic = draw(&one_page(vec![arrowed(&[(0.5, 0.5), (0.5, 0.5), (0.5, 2.0), (3.5, 2.0), (3.5, 2.0)])]));
    assert!(pic.is_dark(3.42, 2.03) && pic.is_white(3.42, 2.07), "the end head lies back along the last real segment, tip at (3.5, 2)");
    assert!(pic.is_white(3.56, 2.0), "and nothing is drawn past the tip");
    assert!(pic.is_dark(0.53, 0.58) && pic.is_white(0.57, 0.58), "the start head lies up along the first real segment, tip at (0.5, 0.5)");
    assert!(pic.is_white(0.5, 0.45), "and nothing is drawn before it");
}

#[test]
fn a_path_whose_only_segments_have_no_length_draws_no_line_end_and_says_why() {
    // Every segment is a LineTo onto the point it starts at: there is no direction anywhere, so no mark can turn to face one.
    let pic = draw(&one_page(vec![arrowed(&[(2.0, 2.0), (2.0, 2.0), (2.0, 2.0)])]));
    assert_eq!(pic.r.count(diacraft_render::SkipKind::LineEndNoDirection), 1, "{:?}", pic.r.skipped);
    assert_eq!(pic.r.skipped.len(), 1, "it is its own kind, not a stand-in: {:?}", pic.r.skipped);
    assert!(pic.r.skipped[0].first.contains("direction"), "it says why: {:?}", pic.r.skipped);
    // A mark would reach 0.11 inch from the point; the line's own round cap reaches 0.01.
    assert!(pic.is_white(2.06, 2.0) && pic.is_white(1.94, 2.0) && pic.is_white(2.0, 2.06) && pic.is_white(2.0, 1.94), "no head is drawn");
    // With no line end asked for there is nothing to say.
    let plain = draw(&one_page(vec![polyline(&[(2.0, 2.0), (2.0, 2.0), (2.0, 2.0)], 0.02, 0)]));
    assert!(plain.r.skipped.is_empty(), "{:?}", plain.r.skipped);
}

#[test]
fn a_closed_path_with_a_repeated_vertex_still_has_no_line_ends_and_says_nothing() {
    // The path comes back to its first vertex, so it is closed, whatever repeats.
    let closed = |arrow: &str| {
        let mut rows = rect_rows(2.0, 2.0);
        rows.insert(2, rows[2].clone());
        let last = rows.len() - 1;
        rows.insert(last, rows[last].clone());
        geometry_of(shape_at(1, 1.0, 1.0, 2.0, 2.0), rows)
            .cell("FillPattern", "0")
            .cell("LineWeight", "0.02")
            .cell("EndArrow", arrow)
            .cell("BeginArrow", arrow)
    };
    let (with, without) = (draw(&one_page(vec![closed("4")])), draw(&one_page(vec![closed("0")])));
    assert_eq!(with.r.rgba_premul, without.r.rgba_premul);
    assert!(with.r.skipped.is_empty(), "{:?}", with.r.skipped);
}
