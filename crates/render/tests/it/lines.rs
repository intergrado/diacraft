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

// ---- compound lines -----------------------------------------------

/// The line of `line` with a compound type, 0.3 inch heavy so that a band is 0.1 inch (ten pixels) and each probe has room.
fn compound(kind: u32) -> ShapeBuilder {
    line(0.3, 1, 0).cell("CompoundType", &kind.to_string())
}

/// What the line shows at x = 2 along its height, from the top of the weight (2.15) to the bottom (1.85), a character for each pixel: `#` where there is ink.
fn column(pic: &Pic) -> String {
    (0..30).map(|k| if pic.is_dark(2.0, 2.15 - (f64::from(k) + 0.5) / 100.0) { '#' } else { '.' }).collect()
}

/// The bands of a [`column`] as runs: how many pixels thick each is, from the top. Anti-aliasing at an edge may move a run by a pixel.
fn runs(column: &str) -> Vec<usize> {
    column.split('.').filter(|r| !r.is_empty()).map(str::len).collect()
}

#[test]
fn compound_type_1_draws_two_bands_sharing_the_weight() {
    // One third line, one third gap, one third line, across the line's full weight. A 0.3 inch line is bands of 0.1 inch with 0.1 between.
    let pic = draw(&one_page(vec![compound(1)]));
    let col = column(&pic);
    let bands = runs(&col);
    assert_eq!(bands.len(), 2, "two bands, not one line: {col}");
    assert!(bands.iter().all(|b| (9..=11).contains(b)), "each a third of the weight, ten pixels: {col}");
    assert!(pic.is_dark(2.0, 2.10) && pic.is_dark(2.0, 1.90), "the bands' middles, a third of the weight out from the centre line");
    assert!(
        pic.is_white(2.0, 2.0) && pic.is_white(2.0, 2.03) && pic.is_white(2.0, 1.97),
        "the gap between them is a third of the weight: nothing at the centre line"
    );
    // The whole is as wide as the weight, no wider.
    let ink = pic.ink_box().expect("a line is drawn");
    assert!(((ink.3 - ink.1) - 0.3).abs() < 0.021, "the bands share the weight: {ink:?}");
    assert!(pic.is_white(2.0, 2.17) && pic.is_white(2.0, 1.83));
    // No note: the bands are the drawing, and nothing in the frame is a stand-in.
    assert!(pic.r.skipped.is_empty(), "{:?}", pic.r.skipped);
    // A line of type 0 is one line, and is not the double of anything.
    let single = draw(&one_page(vec![compound(0)]));
    assert_eq!(runs(&column(&single)), [30], "{}", column(&single));
}

#[test]
fn compound_types_2_and_3_put_the_thick_band_above_or_below_the_thin_one_for_a_line_running_left_to_right() {
    // 2 is a thick line then a thin one, 3 a thin then a thick; the thick band is 0.6 of the weight, the gap and the thin band 0.2 each (thick above thin in 2).
    let two = column(&draw(&one_page(vec![compound(2)])));
    let three = column(&draw(&one_page(vec![compound(3)])));
    let (a, b) = (runs(&two), runs(&three));
    assert_eq!(a.len(), 2, "{two}");
    assert_eq!(b.len(), 2, "{three}");
    assert!((17..=19).contains(&a[0]) && (5..=7).contains(&a[1]), "thick (0.18 inch) then thin (0.06): {two}");
    assert!((5..=7).contains(&b[0]) && (17..=19).contains(&b[1]), "thin (0.06 inch) then thick (0.18): {three}");
    // The picture's top position is the top of the page: y up, the left of a line that runs to the right.
    assert!(
        two.starts_with('#') && two.ends_with('#') && three.starts_with('#') && three.ends_with('#'),
        "the weight is shared whole: {two} {three}"
    );
    // Running the other way turns the bands over: the same line drawn from its right end to its left has the thick band below.
    let back =
        geometry_of(shape_at(1, 3.5, 2.0, 3.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", -3.0), ("Y", 0.0)])])
            .cell("FillPattern", "0")
            .cell("LineWeight", "0.3")
            .cell("LineColor", "#000000")
            .cell("CompoundType", "2");
    let turned = runs(&column(&draw(&one_page(vec![back]))));
    assert!((5..=7).contains(&turned[0]) && (17..=19).contains(&turned[1]), "the bands are left and right of the direction of travel: {turned:?}");
}

#[test]
fn compound_type_4_is_thin_thick_thin() {
    // One sixth, one sixth, one third, one sixth, one sixth: 0.05, 0.05, 0.1, 0.05, 0.05 inch of a 0.3 inch line (thin, thick, thin).
    let col = column(&draw(&one_page(vec![compound(4)])));
    let bands = runs(&col);
    assert_eq!(bands.len(), 3, "{col}");
    assert!((4..=6).contains(&bands[0]) && (9..=11).contains(&bands[1]) && (4..=6).contains(&bands[2]), "thin, thick, thin: {col}");
}

#[test]
fn a_compound_line_turns_its_bands_with_the_path_and_keeps_its_gap_round_a_corner() {
    // An elbow: right from (1, 1) to (3, 1), then up to (3, 3), a flat cap. On each leg the bands lie either side of the path, and the gap runs through the corner.
    let elbow = polyline(&[(1.0, 1.0), (3.0, 1.0), (3.0, 3.0)], 0.3, 1).cell("CompoundType", "1");
    let pic = draw(&one_page(vec![elbow]));
    assert!(pic.is_dark(2.0, 1.10) && pic.is_dark(2.0, 0.90) && pic.is_white(2.0, 1.0), "the first leg's bands and gap");
    assert!(pic.is_dark(3.10, 2.0) && pic.is_dark(2.90, 2.0) && pic.is_white(3.0, 2.0), "the second leg's bands and gap, turned a quarter");
    // At the corner the inner band turns on the inside and the outer one on the outside: the gap's corner is a corner (the point (3, 1) is no ink), and the outer band reaches round it.
    assert!(pic.is_white(3.0, 1.0), "the gap runs into the corner and out of it");
    assert!(pic.is_dark(3.10, 0.90), "the outer band goes round the corner, to the corner of its own path");
    assert!(pic.is_white(3.2, 0.8) && pic.is_white(2.0, 1.17), "and nothing is drawn wider than the weight");
}

#[test]
fn a_dashed_compound_line_dashes_each_band_alike_and_says_it_is_an_approximation() {
    // Unknown: each band takes the stroke's dash list and cap, which is our rule where no source says; the note says so. A plain double line says nothing.
    let pic = draw(&one_page(vec![line(0.1, 2, 1).cell("CompoundType", "1")]));
    let upper = pic.dark_runs_along_x(2.0 + 1.0 / 30.0, 0.5, 3.5);
    let lower = pic.dark_runs_along_x(2.0 - 1.0 / 30.0, 0.5, 3.5);
    assert!(upper.len() >= 3, "a dashed line has several marks on each band: {upper:?}");
    assert_eq!(upper, lower, "the bands dash alike");
    assert!(pic.dark_runs_along_x(2.0, 0.5, 3.5).is_empty(), "and the gap between them is empty all along");
    assert_eq!(pic.r.count(diacraft_render::SkipKind::CompoundApproximate), 1, "{:?}", pic.r.skipped);
    let solid = draw(&one_page(vec![line(0.1, 1, 1).cell("CompoundType", "1")]));
    assert_eq!(solid.r.count(diacraft_render::SkipKind::CompoundApproximate), 0, "{:?}", solid.r.skipped);
}

#[test]
fn a_compound_line_with_an_end_draws_the_end_once_at_the_lines_full_size() {
    // The line end is drawn once at the stroke's full size, whatever the bands; the bands stop behind it. A 0.1 inch line's number 4 is 2 units of (0.035 + 0.1) back from the tip.
    let pic = draw(&one_page(vec![line(0.1, 1, 1).cell("CompoundType", "1").cell("EndArrow", "4").cell("EndArrowSize", "2")]));
    assert!(pic.is_dark(3.4, 2.0), "the head is one solid triangle: its middle is ink where the gap between the bands would be");
    assert!(pic.is_white(3.53, 2.0), "nothing past the tip");
    assert!(pic.is_white(2.0, 2.0) && pic.is_dark(2.0, 2.0 + 1.0 / 30.0), "the bands before the head");
    assert_eq!(pic.r.count(diacraft_render::SkipKind::CompoundApproximate), 1, "an end on a compound line is our rule too: {:?}", pic.r.skipped);
}

#[test]
fn a_compound_line_round_a_curve_and_a_degenerate_path_is_drawn_without_trouble() {
    // A closed shape's outline in two bands: a circle of radius 1 with a compound outline has its two bands one third of the weight either side of the circle.
    let circle =
        geometry_of(shape_at(1, 1.0, 1.0, 2.0, 2.0), vec![("Ellipse", vec![("X", 1.0), ("Y", 1.0), ("A", 2.0), ("B", 1.0), ("C", 1.0), ("D", 2.0)])])
            .cell("FillPattern", "0")
            .cell("LineWeight", "0.3")
            .cell("LineColor", "#000000")
            .cell("CompoundType", "1");
    let pic = draw(&one_page(vec![circle]));
    // On the circle's right at (3, 2): the radius is 1, the bands lie at 1 plus and minus 0.1 from the centre, and the gap between them is on the circle itself.
    assert!(pic.is_dark(3.10, 2.0) && pic.is_dark(2.90, 2.0) && pic.is_white(3.0, 2.0), "the circle's two bands, the gap on the circle");
    assert!(pic.is_dark(2.0, 3.10) && pic.is_dark(2.0, 2.90) && pic.is_white(2.0, 3.0), "and on top of it");
    assert!(pic.r.skipped.is_empty(), "{:?}", pic.r.skipped);
    // A path of repeated points has no direction to offset by: it is drawn without a panic and without a stand-in.
    let dot = polyline(&[(2.0, 2.0), (2.0, 2.0), (2.0, 2.0)], 0.3, 0).cell("CompoundType", "1");
    let dots = draw(&one_page(vec![dot]));
    assert!(dots.r.skipped.is_empty(), "{:?}", dots.r.skipped);
    let repeated = polyline(&[(0.5, 2.0), (0.5, 2.0), (3.5, 2.0), (3.5, 2.0)], 0.3, 0).cell("CompoundType", "1");
    let drawn = draw(&one_page(vec![repeated]));
    assert_eq!(runs(&column(&drawn)).len(), 2, "a repeated vertex changes nothing about the bands: {}", column(&drawn));
}

#[test]
fn a_compound_line_is_charged_before_it_is_drawn_and_cut_by_the_work_budget() {
    // Each band is a stroke of its own, and the path is offset for it: the work is counted before it is done. A budget a unit short of the line's work draws none of it.
    use diacraft_render::{Limits, Renderer, SkipKind};
    let work_of = |kind: u32, max_work: u64| {
        let mut renderer = Renderer::new();
        renderer.set_limits(Limits { max_work, ..Limits::unlimited_time() });
        let pic = draw_with(&mut renderer, &one_page(vec![compound(kind)]));
        (pic, renderer.work_done())
    };
    let (_, single) = work_of(0, u64::MAX);
    let (_, double) = work_of(1, u64::MAX);
    let (_, triple) = work_of(4, u64::MAX);
    assert!(double > single, "two bands cost more than one line: {single} and {double}");
    assert!(triple > double, "three cost more than two: {double} and {triple}");
    let (cut, work) = work_of(1, double - 1);
    assert!(work < double && cut.r.count(SkipKind::WorkLimit) > 0, "the line is left out with a note: {:?}", cut.r.skipped);
    assert_eq!(cut.ink_pixels(), 0, "none of it is drawn");
}

#[test]
fn a_curved_compound_line_costs_more_than_a_straight_one_and_a_long_run_of_curves_is_counted() {
    // A curve is offset by a search that can take many pieces, so it is charged more than a line is, and a path of many of them is cut by the budget and not left to run.
    use diacraft_render::{Limits, Renderer, SkipKind};
    let curves = |n: usize| {
        // A run of `n` cubics across a box of 3 by 1 inches, each an S-bend: the rows are relative to the box (`RelCubBezTo`), and the move is in inches.
        let share = 1.0 / n as f64;
        let mut rows = vec![("MoveTo", vec![("X", 0.0), ("Y", 0.5)])];
        for k in 0..n {
            let x = share * (k as f64 + 1.0);
            rows.push(("RelCubBezTo", vec![("X", x), ("Y", 0.5), ("A", x - 0.6 * share), ("B", 1.0), ("C", x - 0.3 * share), ("D", 0.0)]));
        }
        geometry_of(shape_at(1, 0.5, 1.5, 3.0, 1.0), rows)
            .cell("FillPattern", "0")
            .cell("LineWeight", "0.02")
            .cell("LineColor", "#000000")
            .cell("CompoundType", "1")
    };
    let work = |n: usize, max_work: u64| {
        let mut renderer = Renderer::new();
        renderer.set_limits(Limits { max_work, ..Limits::unlimited_time() });
        let pic = draw_with(&mut renderer, &one_page(vec![curves(n)]));
        (pic, renderer.work_done())
    };
    let (_, ten) = work(10, u64::MAX);
    let (_, twenty) = work(20, u64::MAX);
    assert!(twenty > ten, "twice the curves, more work: {ten} and {twenty}");
    let (cut, spent) = work(200, 1_000);
    assert!(spent <= 1_000, "{spent} of 1000");
    assert!(cut.r.count(SkipKind::WorkLimit) > 0, "{:?}", cut.r.skipped);
}

/// A horizontal line like [`line`], with the text of the pattern's cell as it is given (a fraction, or a number the table has no row for).
fn line_with_pattern_text(weight: f64, pattern: &str) -> ShapeBuilder {
    geometry_of(shape_at(1, 0.5, 2.0, 3.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.0), ("Y", 0.0)])])
        .cell("FillPattern", "0")
        .cell("LineWeight", &weight.to_string())
        .cell("LinePattern", pattern)
        .cell("LineCap", "0")
        .cell("LineColor", "#000000")
}

#[test]
fn a_line_pattern_outside_the_table_draws_solid_and_says_so() {
    // A number outside the table's list, and a fraction, draw solid, with the cell kept and a warning. A fraction was rounded to the
    // number beside it: 2.5 drew as 3 (dots), 1.6 as 2 (dashes) and 0.4 as 0 (no line at all).
    for text in ["2.5", "0.4", "1.6", "22.5", "-0.5", "24", "100", "-1", "255"] {
        let pic = draw(&one_page(vec![line_with_pattern_text(0.05, text)]));
        let runs = pic.dark_runs_along_x(2.0, 0.0, 4.0);
        assert_eq!(runs.len(), 1, "{text}: one solid run, got {runs:?}");
        assert!(runs[0].1 >= 295, "{text}: the whole 3 inches is inked, got {runs:?}");
        assert!(pic.r.skipped.iter().any(|s| s.kind == diacraft_render::SkipKind::PatternOutsideTable), "{text}: said so, got {:?}", pic.r.skipped);
    }
}

#[test]
fn the_numbers_of_the_table_and_the_two_that_are_no_row_of_it_say_nothing_of_the_pattern() {
    // 0 draws no line, 1 a solid one, 2 and 23 dashes; none is outside the table, so none is a warning. 254 is a custom pattern, which has its own.
    for (text, runs_at_least) in [("0", 0), ("1", 1), ("2", 3), ("23", 3)] {
        let pic = draw(&one_page(vec![line_with_pattern_text(0.05, text)]));
        assert!(pic.dark_runs_along_x(2.0, 0.0, 4.0).len() >= runs_at_least, "{text}");
        assert!(pic.r.skipped.iter().all(|s| s.kind != diacraft_render::SkipKind::PatternOutsideTable), "{text}: {:?}", pic.r.skipped);
    }
    let custom = draw(&one_page(vec![line_with_pattern_text(0.05, "254")]));
    assert!(custom.r.skipped.iter().any(|s| s.kind == diacraft_render::SkipKind::CustomPattern), "{:?}", custom.r.skipped);
    assert!(custom.r.skipped.iter().all(|s| s.kind != diacraft_render::SkipKind::PatternOutsideTable), "{:?}", custom.r.skipped);
}
