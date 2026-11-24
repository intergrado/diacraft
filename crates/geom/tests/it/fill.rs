//! Filling, stroking and closing.

use diacraft_geom::kurbo::{BezPath, ParamCurve, Shape};
use diacraft_geom::{GeomRow, RunJoin, SectionFlags, build_paths, build_paths_with, combined_fill, even_odd_contains, is_closed, rules};

use crate::{build_plain, ends_with_close, ln, mv, p, segments, size};

/// The rows of an axis-aligned square from (x0, y0) to (x1, y1), counterclockwise or clockwise, back to its start.
fn square(x0: f64, y0: f64, x1: f64, y1: f64, ccw: bool) -> Vec<GeomRow> {
    if ccw {
        vec![mv(x0, y0), ln(x1, y0), ln(x1, y1), ln(x0, y1), ln(x0, y0)]
    } else {
        vec![mv(x0, y0), ln(x0, y1), ln(x1, y1), ln(x1, y0), ln(x0, y0)]
    }
}

fn ring(rows: &[GeomRow]) -> BezPath {
    build_plain(rows).fill
}

/// Given `NoFill` 0 and rows `MoveTo` (0, 0), `LineTo` (2, 0), `LineTo` (2, 2), expect the triangle (0, 0), (2, 0),
/// (2, 2) filled and only the two listed sides stroked. Filling only closed paths leaves it empty, and stroking the closing side adds
/// a third line.
#[test]
fn fill_an_open_path_fills_as_if_closed_and_strokes_only_its_rows() {
    let built = build_plain(&[mv(0.0, 0.0), ln(2.0, 0.0), ln(2.0, 2.0)]);
    assert!(even_odd_contains(&built.fill, p(1.5, 0.5)), "inside the triangle");
    assert!(!even_odd_contains(&built.fill, p(0.5, 1.5)), "outside the triangle");
    assert_eq!(segments(&built.stroke).len(), 2, "two sides stroked, not three");
    assert!(!ends_with_close(&built.stroke));
    assert!(ends_with_close(&built.fill), "the ring that is filled is closed");
}

/// Given one shape with two sections, squares from (0, 0) to (4, 4) and from (1, 1) to (3, 3), both `NoFill` 0,
/// expect (2, 2) unfilled and (0.5, 0.5) filled, whichever way either runs. Filling each section alone fills (2, 2). A winding rule
/// with both counterclockwise fills it too.
#[test]
fn fill_sections_of_one_shape_fill_together_by_odd_crossings() {
    for (outer_ccw, inner_ccw) in [(true, true), (true, false), (false, true), (false, false)] {
        let outer = ring(&square(0.0, 0.0, 4.0, 4.0, outer_ccw));
        let inner = ring(&square(1.0, 1.0, 3.0, 3.0, inner_ccw));
        let both = combined_fill([(&outer, SectionFlags::default()), (&inner, SectionFlags::default())]);
        assert!(!even_odd_contains(&both, p(2.0, 2.0)), "overlap is a hole ({outer_ccw}, {inner_ccw})");
        assert!(even_odd_contains(&both, p(0.5, 0.5)), "outside the inner square ({outer_ccw}, {inner_ccw})");
        // The naive results: one section at a time fills the overlap; a winding rule with both counterclockwise fills it.
        assert!(even_odd_contains(&inner, p(2.0, 2.0)));
        if outer_ccw == inner_ccw {
            assert_ne!(both.winding(p(2.0, 2.0)), 0, "nonzero winding would fill it");
        }
    }
}

/// Given one section of two overlapping squares joined by a `MoveTo`, from (0, 0) to (2, 2) and from (1, 1) to
/// (3, 3), expect (1.5, 1.5) unfilled and (0.5, 0.5) filled. Nonzero winding fills (1.5, 1.5).
#[test]
fn fill_one_section_of_two_squares_joined_by_a_move() {
    let mut rows = square(0.0, 0.0, 2.0, 2.0, true);
    rows.extend(square(1.0, 1.0, 3.0, 3.0, true));
    let built = build_plain(&rows);
    assert!(!even_odd_contains(&built.fill, p(1.5, 1.5)), "the overlap is a hole");
    assert!(even_odd_contains(&built.fill, p(0.5, 0.5)), "the rest of the first square");
    assert!(even_odd_contains(&built.fill, p(2.5, 2.5)), "the rest of the second");
    assert_ne!(built.fill.winding(p(1.5, 1.5)), 0, "naive: nonzero winding fills the overlap");
    // Both squares are closed runs of one section: two closes in the stroke, one ring in the fill.
    let closes = built.stroke.elements().iter().filter(|e| matches!(e, diacraft_geom::kurbo::PathEl::ClosePath)).count();
    assert_eq!(closes, 2);
}

/// Given `NoFill` 0 and rows `MoveTo` (0, 0), `LineTo` (2, 0), `MoveTo` (2, 2), `LineTo` (0, 2), expect the one square
/// from (0, 0) to (2, 2) filled, with strokes only along the bottom and the top. Closing each run on its own gives two lines and no fill.
#[test]
fn fill_a_move_counts_as_a_straight_edge_in_the_fill() {
    let built = build_plain(&[mv(0.0, 0.0), ln(2.0, 0.0), mv(2.0, 2.0), ln(0.0, 2.0)]);
    for inside in [p(1.0, 1.0), p(0.2, 1.8), p(1.9, 0.1)] {
        assert!(even_odd_contains(&built.fill, inside), "{inside:?} is in the square");
    }
    assert!(!even_odd_contains(&built.fill, p(3.0, 1.0)));
    let segs = segments(&built.stroke);
    assert_eq!(segs.len(), 2, "bottom and top only");
    assert_eq!((segs[0].start(), segs[0].end()), (p(0.0, 0.0), p(2.0, 0.0)));
    assert_eq!((segs[1].start(), segs[1].end()), (p(2.0, 2.0), p(0.0, 2.0)));
}

/// D2 (review, small items): follows the page and another reader closes each run on its own, and M2's render check compares the two, so the choice is a named
/// switch, `rules::FILL_RUN_JOIN`, and `build_paths_with` takes either. The rows are the square under the page's reading and two lines and no fill under the reading of D2; the
/// stroke is the same under both.
#[test]
fn closing_each_run_is_the_other_reading_and_it_is_one_switch() {
    assert_eq!(rules::FILL_RUN_JOIN, RunJoin::StraightEdge, "the page's reading is the project's");
    let rows = [mv(0.0, 0.0), ln(2.0, 0.0), mv(2.0, 2.0), ln(0.0, 2.0)];
    let page = build_paths_with(&rows, size(1.0, 1.0), RunJoin::StraightEdge).unwrap();
    let each = build_paths_with(&rows, size(1.0, 1.0), RunJoin::CloseEachRun).unwrap();
    assert!(even_odd_contains(&page.fill, p(1.0, 1.0)), "the square");
    assert!(!even_odd_contains(&each.fill, p(1.0, 1.0)), "two lines and no fill");
    assert_eq!(
        each.fill.elements().iter().filter(|e| matches!(e, diacraft_geom::kurbo::PathEl::ClosePath)).count(),
        2,
        "two rings, each closed on its own"
    );
    assert_eq!(page.stroke, each.stroke);
    assert_eq!(build_paths(&rows, size(1.0, 1.0)).unwrap().fill, page.fill, "`build_paths` follows the switch");
    // A section of one run has one ring under both.
    let one = [mv(0.0, 0.0), ln(2.0, 0.0), ln(2.0, 2.0)];
    assert_eq!(
        build_paths_with(&one, size(1.0, 1.0), RunJoin::StraightEdge).unwrap().fill,
        build_paths_with(&one, size(1.0, 1.0), RunJoin::CloseEachRun).unwrap().fill
    );
}

/// Given a start of (1, 1) and an end of (1, 1.0000005), expect closed under either reading of the tolerance, 1E-5
/// or 1E-6. An exact test says open. Given an end of (1, 1.001), expect open under both.
#[test]
fn fill_closed_within_the_tolerance() {
    assert!(is_closed(p(1.0, 1.0), p(1.0, 1.0000005)));
    assert_ne!(p(1.0, 1.0), p(1.0, 1.0000005), "naive: an exact test says open");
    assert!(!is_closed(p(1.0, 1.0), p(1.0, 1.001)));
    assert!(is_closed(p(3.0, 3.0), p(3.0, 3.0)));
    // Right at the tolerance, in either direction, and in x as well as y.
    assert!(is_closed(p(0.0, 0.0), p(1e-5, 0.0)));
    assert!(!is_closed(p(0.0, 0.0), p(1.1e-5, 0.0)));
    // Hostile: a point that is not a number is not closed.
    assert!(!is_closed(p(f64::NAN, 0.0), p(f64::NAN, 0.0)));
    assert!(!is_closed(p(f64::INFINITY, 0.0), p(f64::INFINITY, 0.0)));
}

/// The stroke of a run whose end is within the tolerance of its start is closed; that end 0.001 away is open.
#[test]
fn a_run_ending_within_the_tolerance_of_its_start_is_closed() {
    let near = build_plain(&[mv(1.0, 1.0), ln(2.0, 1.0), ln(2.0, 2.0), ln(1.0, 1.0000005)]);
    assert!(ends_with_close(&near.stroke));
    let far = build_plain(&[mv(1.0, 1.0), ln(2.0, 1.0), ln(2.0, 2.0), ln(1.0, 1.001)]);
    assert!(!ends_with_close(&far.stroke));
}

/// Given `NoShow` 1 with `NoFill` 0 and `NoLine` 0, expect nothing drawn. Given `NoShow` 0, `NoFill` 1 and `NoLine` 0,
/// expect a stroke only. Given `NoShow` 0, `NoFill` 0, `NoLine` 1, expect a fill only. Given `NoFill` 0 and a shape's `FillPattern` 0,
/// expect no fill. Reading `NoShow` as hiding only the fill strokes the first, and testing `NoFill` alone fills the last.
#[test]
fn fill_the_order_of_tests() {
    let flags = |no_fill, no_line, no_show| SectionFlags { no_fill, no_line, no_show };
    let hidden = flags(false, false, true).paint(false);
    assert!(!hidden.stroke && !hidden.fill, "NoShow hides both");
    let stroke_only = flags(true, false, false).paint(false);
    assert!(stroke_only.stroke && !stroke_only.fill);
    let fill_only = flags(false, true, false).paint(false);
    assert!(!fill_only.stroke && fill_only.fill);
    let no_pattern = flags(false, false, false).paint(true);
    assert!(no_pattern.stroke && !no_pattern.fill, "FillPattern 0 leaves every path unfilled, whatever NoFill says");
    let all = flags(false, false, false).paint(false);
    assert!(all.stroke && all.fill);
    // NoFill 1 and NoLine 1 with the path shown: neither (the path still gives the shape's extents).
    let neither = flags(true, true, false).paint(false);
    assert!(!neither.stroke && !neither.fill);
}

/// Given a shape with section 1, the square from (0, 0) to (4, 4) with `NoFill` 1, and section 2, the strip
/// from (0, 3) to (4, 4) with `NoFill` 0, expect the strip filled and the rest not. Counting section 1 in the crossings leaves the strip
/// empty and fills nothing.
#[test]
fn fill_an_unfilled_section_does_not_count_in_the_crossings() {
    let square_ring = ring(&square(0.0, 0.0, 4.0, 4.0, true));
    let strip = ring(&square(0.0, 3.0, 4.0, 4.0, true));
    let no_fill = SectionFlags { no_fill: true, ..SectionFlags::default() };
    let both = combined_fill([(&square_ring, no_fill), (&strip, SectionFlags::default())]);
    assert!(even_odd_contains(&both, p(2.0, 3.5)), "the strip is filled");
    assert!(!even_odd_contains(&both, p(2.0, 1.0)), "the rest of the square is not");
    // The naive result: counting section 1 leaves the strip empty and fills the rest.
    let naive = combined_fill([(&square_ring, SectionFlags::default()), (&strip, SectionFlags::default())]);
    assert!(!even_odd_contains(&naive, p(2.0, 3.5)));
    assert!(even_odd_contains(&naive, p(2.0, 1.0)));
}

/// Project rule: a section with `NoShow` 1 does not count in the crossings either.
#[test]
fn a_hidden_section_does_not_count_in_the_crossings() {
    let outer = ring(&square(0.0, 0.0, 4.0, 4.0, true));
    let inner = ring(&square(1.0, 1.0, 3.0, 3.0, true));
    let hidden = SectionFlags { no_show: true, ..SectionFlags::default() };
    let both = combined_fill([(&outer, SectionFlags::default()), (&inner, hidden)]);
    assert!(even_odd_contains(&both, p(2.0, 2.0)), "the hidden inner square makes no hole");
    // And a combined fill of nothing fills nothing.
    let none = combined_fill(std::iter::empty());
    assert!(none.elements().is_empty());
    assert!(!even_odd_contains(&none, p(0.0, 0.0)));
}

/// An open fillable path is ordinary; a fill ring is closed whatever the rows do, and a section with a lone move has none.
#[test]
fn the_fill_ring_is_closed_and_a_lone_move_has_no_ring() {
    let lone = build_plain(&[mv(1.0, 1.0)]);
    assert!(lone.fill.elements().is_empty() && lone.stroke.elements().is_empty());
    let two_points = build_plain(&[mv(0.0, 0.0), ln(1.0, 0.0)]);
    assert!(ends_with_close(&two_points.fill));
    assert_eq!(two_points.fill.bounding_box().area(), 0.0, "a ring of two points has no area");
}

/// The sections of one shape come from separate calls; here two of them are built from rows and filled together.
#[test]
fn separate_builds_combine() {
    let a = build_paths(&square(0.0, 0.0, 2.0, 2.0, true), size(1.0, 1.0)).unwrap();
    let b = build_paths(&square(1.0, 0.0, 3.0, 2.0, true), size(1.0, 1.0)).unwrap();
    let both = combined_fill([(&a.fill, SectionFlags::default()), (&b.fill, SectionFlags::default())]);
    assert!(even_odd_contains(&both, p(0.5, 1.0)));
    assert!(!even_odd_contains(&both, p(1.5, 1.0)));
    assert!(even_odd_contains(&both, p(2.5, 1.0)));
}

/// Review finding 2: a point that is not a number is in no ring, and a path that holds one fills nothing. Kurbo's `winding` asserts that its input is a number
/// in a debug build, so the test would panic there without the guard; WebAssembly has no guard to catch it.
#[test]
fn a_point_or_path_that_is_not_a_number_is_filled_by_nothing() {
    let ring = ring(&square(0.0, 0.0, 2.0, 2.0, true));
    for bad in [p(f64::NAN, 1.0), p(1.0, f64::NAN), p(f64::INFINITY, 1.0), p(1.0, f64::NEG_INFINITY)] {
        assert!(!even_odd_contains(&ring, bad), "{bad:?}");
    }
    let mut broken = BezPath::new();
    broken.move_to((0.0, 0.0));
    broken.line_to((f64::NAN, 1.0));
    broken.line_to((2.0, 2.0));
    broken.close_path();
    assert!(!even_odd_contains(&broken, p(1.0, 0.5)));
    assert!(even_odd_contains(&ring, p(1.0, 1.0)), "and a finite point of a finite ring still is");
}

fn flags(no_fill: bool, no_line: bool, no_show: bool) -> SectionFlags {
    SectionFlags { no_fill, no_line, no_show }
}

/// Given `NoFill` 1 on a closed square, expect a stroke and no fill, and given `NoFill` 0 on an open two-segment path, expect a fill. A
/// closed test instead of this cell fails both.
#[test]
fn nofill_the_cell_decides_the_fill_and_closure_does_not() {
    let closed = build_plain(&square(0.0, 0.0, 2.0, 2.0, true));
    assert!(ends_with_close(&closed.stroke), "the square is closed");
    let paint = flags(true, false, false).paint(false);
    assert!(paint.stroke && !paint.fill, "a stroke and no fill");
    let open = build_plain(&[mv(0.0, 0.0), ln(2.0, 0.0), ln(2.0, 2.0)]);
    assert!(!ends_with_close(&open.stroke), "the path is open");
    let paint = flags(false, false, false).paint(false);
    assert!(paint.fill, "a fill, closed or not");
    assert!(even_odd_contains(&open.fill, p(1.5, 0.5)), "and the ring it fills is there");
}

/// Given a section with no `NoFill` cell on any sheet, expect 0 and a fill.
#[test]
fn nofill_no_cell_is_no_fill_0() {
    assert!(!SectionFlags::default().no_fill, "the default is 0");
    assert!(SectionFlags::default().paint(false).fill);
}

/// Given `NoLine` 1 and `NoFill` 0, expect a fill with no stroke, whatever `LinePattern` says. Given `NoLine` 1 and `NoFill` 1 with
/// `NoShow` 0, expect nothing visible and the path still in the shape's extents. (`LinePattern` is the shape's, and not an input here: the stroke is off whatever it is.)
#[test]
fn noline_no_line_keeps_the_fill_and_a_path_with_neither_still_has_extents() {
    let fill_only = flags(false, true, false).paint(false);
    assert!(fill_only.fill && !fill_only.stroke);
    let neither = flags(true, true, false).paint(false);
    assert!(!neither.fill && !neither.stroke, "nothing visible");
    let built = build_plain(&square(0.0, 0.0, 2.0, 3.0, true));
    let extents = built.stroke.bounding_box();
    assert!(extents.width() == 2.0 && extents.height() == 3.0, "the path is still in the shape's extents: {extents:?}");
}

/// The first half: given `NoShow` 1 on a section and `NoShow` 0 on another, expect only the second drawn. (The second half, a formula
/// `Width>3.17` and a write of 4 to `Width`, is Task 6's.)
#[test]
fn noshow_a_hidden_section_is_not_drawn_and_a_shown_one_is() {
    let hidden = flags(false, false, true).paint(false);
    let shown = flags(false, false, false).paint(false);
    assert!(!hidden.stroke && !hidden.fill);
    assert!(shown.stroke && shown.fill);
}

/// The pen: an `InfiniteLine` is the whole path and leaves no pen, so a row that needs one after it is skipped (review: a mutation that kept the pen through an
/// `InfiniteLine` survived).
#[test]
fn an_infinite_line_leaves_no_pen() {
    let rows = [mv(0.0, 0.0), ln(1.0, 0.0), GeomRow::InfiniteLine { x: 0.0, y: 0.0, other_x: 1.0, other_y: 1.0 }, ln(2.0, 0.0)];
    let built = build_plain(&rows);
    assert_eq!(built.infinite_lines.len(), 1);
    assert_eq!(built.warnings.len(), 1);
    assert_eq!(built.warnings[0].row, 3);
    assert_eq!(built.warnings[0].kind, diacraft_geom::WarningKind::NoPen);
    assert_eq!(segments(&built.stroke).len(), 1, "only the line before it");
}

/// A jump of the pen (a row that cannot be drawn) is a straight edge in the fill and the ring goes on (the reading of a move): review found that a mutation that made
/// it a new subpath of the ring survived.
#[test]
fn a_row_that_cannot_be_drawn_is_a_straight_edge_in_the_fill() {
    let unusable = GeomRow::NurbsTo { x: 2.0, y: 2.0, second_last_knot: 0.0, last_weight: 1.0, first_knot: 0.0, first_weight: 1.0, nurbs: None };
    let built = build_plain(&[mv(0.0, 0.0), ln(2.0, 0.0), unusable, ln(0.0, 2.0)]);
    assert_eq!(built.warnings.len(), 1);
    assert!(even_odd_contains(&built.fill, p(1.0, 1.0)), "one ring, the square (0, 0), (2, 0), (2, 2), (0, 2)");
    assert!(!even_odd_contains(&built.fill, p(3.0, 1.0)));
    assert_eq!(built.fill.elements().iter().filter(|e| matches!(e, diacraft_geom::kurbo::PathEl::MoveTo(_))).count(), 1, "the ring is one subpath");
    assert_eq!(segments(&built.stroke).len(), 2, "the stroke has a gap where the pen jumped");
}
