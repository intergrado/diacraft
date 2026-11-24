//! The path work: the areas operands fill, laid together by `craft-pathops`, and what it refuses (SHAPEOPS-CAPS).
//!
//! Everything here takes and gives `kurbo::BezPath`s, in a frame the caller chose near the origin, and knows nothing of a document. The library's own limits hold
//! (its `Limits` defaults, never raised, and its README's "Known limits"); each call is a `try_` form, so that a refusal reaches the person in words and does not
//! come back as an empty result that looks like "nothing is left". A panic inside the library's sweep is caught inside it (not on the web, where the build aborts:
//! the reading in [`super::operand`] keeps what goes in well inside its limits).
//!
//! **Fill rules.** An operand's fill is the rings of its sections that take the fill together, filled by the odd-crossings rule, so each goes in with
//! [`FillRule::EvenOdd`]. The library's contours come back counter-clockwise for an outer boundary and clockwise for a hole, which [`super::rows::clean`] checks.

use craft_pathops::{FillRule, PathOpsError, PathfinderOp, Shape, try_pathfinder_with, try_unite_all_with};
use kurbo::{BezPath, ParamCurve, PathEl, PathSeg, Point, Rect, Shape as _, Vec2};

use super::{MERGE, PRECISION};
use crate::draw::Problem;

/// The key a cutting line has in a Divide, which no operand has: the faces it covers (none, it fills nothing) would be keyed by it.
const CUT: u64 = u64::MAX;

/// What a refusal of the library is in words: the three the brief names, and any the library may add.
pub(super) fn refusal(error: PathOpsError) -> Problem {
    let why = match error {
        PathOpsError::TooComplex => {
            "the shapes have too many path elements, or their edges cross or meet in too many places, to be worked on in bounded time (the limits of the path library)"
        }
        PathOpsError::Degenerate => "the shapes are too degenerate to combine (coordinates too large for the sweep, or a shape it cannot take)",
        PathOpsError::NonFinite => "a shape's geometry holds a number that is not finite",
        PathOpsError::OpenPath => "a path of the shapes could not be closed",
        _ => "the path library refused the shapes",
    };
    Problem::failed(why)
}

fn eo(path: &BezPath) -> (&BezPath, FillRule) {
    (path, FillRule::EvenOdd)
}

/// The shape that is the union of the fills: one path with a contour for each boundary, holes included.
pub(super) fn union(fills: &[BezPath]) -> Result<BezPath, Problem> {
    let inputs: Vec<(&BezPath, FillRule)> = fills.iter().map(eo).collect();
    try_unite_all_with(&inputs, PRECISION).map_err(refusal)
}

/// The area every one of the fills covers.
pub(super) fn intersect(fills: &[BezPath]) -> Result<BezPath, Problem> {
    let shapes: Vec<Shape> = fills.iter().map(|f| Shape::new(f.clone(), FillRule::EvenOdd, 0)).collect();
    let made = try_pathfinder_with(PathfinderOp::Intersect, &shapes, PRECISION).map_err(refusal)?;
    Ok(only_path(made))
}

/// `first` less the union of `others`. Everything else of `first` is what is left, holes included.
pub(super) fn subtract(first: &BezPath, others: &[BezPath]) -> Result<BezPath, Problem> {
    // The keys name nothing here: the result has the back-most one, which is the first shape's.
    let mut shapes = vec![Shape::new(first.clone(), FillRule::EvenOdd, 0)];
    shapes.extend(others.iter().map(|f| Shape::new(f.clone(), FillRule::EvenOdd, 1)));
    let made = try_pathfinder_with(PathfinderOp::MinusFront, &shapes, PRECISION).map_err(refusal)?;
    Ok(only_path(made))
}

/// The faces of the fills laid together, cut by the cutting lines: each is one path, outer boundary and holes, in the order SHAPEOPS-ORDER gives: by the top
/// of its box, highest first, then by its left side, leftmost first.
pub(super) fn fragment(fills: &[BezPath], cuts: &[BezPath]) -> Result<Vec<BezPath>, Problem> {
    let mut shapes: Vec<Shape> = fills.iter().enumerate().map(|(i, f)| Shape::new(f.clone(), FillRule::EvenOdd, i as u64)).collect();
    shapes.extend(cuts.iter().flat_map(open_pieces).map(|piece| Shape::new(piece, FillRule::EvenOdd, CUT)));
    let made = try_pathfinder_with(PathfinderOp::Divide, &shapes, PRECISION).map_err(refusal)?;
    let faces: Vec<BezPath> = made.into_iter().filter(|s| s.key != CUT && !s.path.elements().is_empty()).map(|s| s.path).collect();
    Ok(in_order(faces))
}

/// The one path a pathfinder result holds, or an empty path for none.
fn only_path(made: Vec<Shape>) -> BezPath {
    let mut all = BezPath::new();
    for shape in made {
        for el in shape.path.elements() {
            all.push(*el);
        }
    }
    all
}

/// The faces by the top of their boxes, highest first, then by their left sides, leftmost first. The sides are compared to a hundred-millionth, as whole
/// numbers, so that the order is a total one (a comparison with a tolerance is not, and a sort given one may panic) and two edges a rounding apart tie.
fn in_order(mut faces: Vec<BezPath>) -> Vec<BezPath> {
    fn key(path: &BezPath) -> (std::cmp::Reverse<i64>, i64) {
        let rect = path.bounding_box();
        let q = |v: f64| if v.is_finite() { (v / 1.0e-8).round() as i64 } else { 0 };
        (std::cmp::Reverse(q(rect.y1)), q(rect.x0))
    }
    faces.sort_by_cached_key(key);
    faces
}

/// Whether a subpath that starts at `start` and ends at `end` is a ring.
fn is_ring(start: Point, end: Point) -> bool {
    start.distance(end) <= MERGE
}

/// The open pieces a path of cutting lines is made of: every subpath without its closing, and a subpath that comes back to its start in two, so that none
/// ends where it starts (the library takes such a ring for a closed shape and fills it, where a cutting line fills nothing).
pub(super) fn open_pieces(path: &BezPath) -> Vec<BezPath> {
    let mut pieces: Vec<BezPath> = Vec::new();
    let mut run: Vec<PathSeg> = Vec::new();
    let mut start: Option<Point> = None;
    let mut flush = |run: &mut Vec<PathSeg>, start: Option<Point>| {
        let segs = std::mem::take(run);
        if let Some(start) = start {
            split_run(segs, start, &mut pieces);
        }
    };
    let mut pen: Option<Point> = None;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                flush(&mut run, start);
                start = Some(p);
                pen = Some(p);
            }
            PathEl::ClosePath => {
                // The closing edge is part of the cutting line: a straight edge back to the start where the path does not end there.
                if let (Some(from), Some(to)) = (pen, start)
                    && from.distance(to) > 0.0
                {
                    run.push(PathSeg::Line(kurbo::Line::new(from, to)));
                }
                pen = start;
                flush(&mut run, start);
                start = None;
            }
            PathEl::LineTo(p) => push_seg(&mut run, &mut pen, |from| PathSeg::Line(kurbo::Line::new(from, p)), p),
            PathEl::QuadTo(c, p) => push_seg(&mut run, &mut pen, |from| PathSeg::Quad(kurbo::QuadBez::new(from, c, p)), p),
            PathEl::CurveTo(a, b, p) => push_seg(&mut run, &mut pen, |from| PathSeg::Cubic(kurbo::CubicBez::new(from, a, b, p)), p),
        }
    }
    flush(&mut run, start);
    pieces
}

fn push_seg(run: &mut Vec<PathSeg>, pen: &mut Option<Point>, make: impl FnOnce(Point) -> PathSeg, to: Point) {
    if let Some(from) = *pen {
        run.push(make(from));
        *pen = Some(to);
    }
}

/// The segments of one subpath as open paths: whole when it does not come back to its start, in two when it does.
fn split_run(mut segs: Vec<PathSeg>, start: Point, pieces: &mut Vec<BezPath>) {
    let Some(last) = segs.last().map(PathSeg::end) else { return };
    let mut open = |segs: &[PathSeg]| {
        let mut path = BezPath::new();
        if let Some(first) = segs.first() {
            path.move_to(first.start());
            for seg in segs {
                match seg {
                    PathSeg::Line(l) => path.line_to(l.p1),
                    PathSeg::Quad(q) => path.quad_to(q.p1, q.p2),
                    PathSeg::Cubic(c) => path.curve_to(c.p1, c.p2, c.p3),
                }
            }
            pieces.push(path);
        }
    };
    if !is_ring(start, last) {
        open(&segs);
        return;
    }
    if segs.len() == 1 {
        if let Some(only) = segs.pop() {
            let (a, b) = only.subdivide();
            open(&[a]);
            open(&[b]);
        }
        return;
    }
    let (head, tail) = segs.split_at(segs.len() / 2);
    open(head);
    open(tail);
}

/// The points of a path element that a curve is drawn through or pulled by: its end and its control points.
fn points_of(el: &PathEl) -> impl Iterator<Item = Point> + use<> {
    let (a, b, c) = match *el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => (Some(p), None, None),
        PathEl::QuadTo(control, p) => (Some(control), Some(p), None),
        PathEl::CurveTo(first, second, p) => (Some(first), Some(second), Some(p)),
        PathEl::ClosePath => (None, None, None),
    };
    [a, b, c].into_iter().flatten()
}

/// Whether a ring (the elements of one subpath) lies along one line: no point of it, control points included, is more than [`MERGE`] from the line through its first point
/// and the point farthest from it. A ring of one point, a line there and back and a run along one line do, and so cover no area. Worked in lengths scaled by the ring's
/// reach, so that a ring of 1E300 overflows to nothing: where the reach is not a number the ring is not flat, and the library refuses it in words.
fn ring_is_flat(ring: &[PathEl]) -> bool {
    let points = || ring.iter().flat_map(points_of);
    let Some(origin) = points().next() else { return true };
    let reach = points().fold(0.0_f64, |most, p| most.max((p.x - origin.x).abs()).max((p.y - origin.y).abs()));
    if !reach.is_finite() {
        return false;
    }
    if reach <= MERGE {
        return true;
    }
    let unit = |p: Point| Vec2::new((p.x - origin.x) / reach, (p.y - origin.y) / reach);
    let Some(far) = points().map(unit).max_by(|a, b| a.hypot2().total_cmp(&b.hypot2())) else { return true };
    let length = far.hypot();
    let thickest = points().map(unit).map(|v| (far.x * v.y - far.y * v.x).abs() / length).fold(0.0_f64, f64::max);
    thickest * reach <= MERGE
}

/// Whether the fill `path` may cover an area: some ring of it, a subpath, is thicker than [`MERGE`] in some direction. It is the cheap condition that has to hold for a fill
/// area (SHAPEOPS-NEEDS: a line, a point and a run along one line have none), and it is **not the area**: it runs no sweep, and its cost is the elements of the path, which
/// the reading has bounded, where `try_area` is a full sweep of the library and costs what its crossings do. A ring that crosses itself so that its area is nothing under
/// the odd-crossings rule (two equal squares one over the other) passes, and the operation's own result says there is nothing left of it, in words.
pub(super) fn may_cover_area(path: &BezPath) -> bool {
    path.elements().chunk_by(|_, next| !matches!(next, PathEl::MoveTo(_))).any(|ring| !ring_is_flat(ring))
}

/// The box that holds every path, curves by their true extent: `None` for no path or a box that is not a number.
pub(super) fn bounds<'a>(paths: impl IntoIterator<Item = &'a BezPath>) -> Option<Rect> {
    let rect = paths.into_iter().filter(|p| !p.elements().is_empty()).map(|p| p.bounding_box()).reduce(|a, b| a.union(b))?;
    [rect.x0, rect.y0, rect.x1, rect.y1].iter().all(|v| v.is_finite()).then_some(rect)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::draw::shapeops::rows;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
        let mut p = BezPath::new();
        p.move_to((x0, y0));
        p.line_to((x1, y0));
        p.line_to((x1, y1));
        p.line_to((x0, y1));
        p.close_path();
        p
    }

    fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
        let mut p = BezPath::new();
        p.move_to((x0, y0));
        p.line_to((x1, y1));
        p
    }

    fn area_of(path: &BezPath) -> f64 {
        rows::clean(path).iter().map(rows::Loop::area).sum()
    }

    #[test]
    fn two_overlapping_squares_unite_to_the_outline_of_both() {
        let u = union(&[rect(1.0, 1.0, 3.0, 3.0), rect(2.0, 2.0, 4.0, 4.0)]).unwrap();
        let loops = rows::clean(&u);
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].nodes.len(), 8, "The eight corners");
        assert!((area_of(&u) - 7.0).abs() < 1e-9);
    }

    #[test]
    fn a_ring_of_four_bars_unites_to_a_square_and_the_hole_it_encloses() {
        let bars = [rect(0.0, 0.0, 3.0, 1.0), rect(0.0, 2.0, 3.0, 3.0), rect(0.0, 0.0, 1.0, 3.0), rect(2.0, 0.0, 3.0, 3.0)];
        let u = union(&bars).unwrap();
        let loops = rows::clean(&u);
        assert_eq!(loops.len(), 2);
        assert!(
            (loops[0].area() - 9.0).abs() < 1e-9 && (loops[1].area() + 1.0).abs() < 1e-9,
            "{:?}",
            loops.iter().map(rows::Loop::area).collect::<Vec<_>>()
        );
    }

    #[test]
    fn three_squares_intersect_in_the_area_all_of_them_cover() {
        let i = intersect(&[rect(1.0, 1.0, 3.0, 3.0), rect(2.0, 2.0, 4.0, 4.0), rect(2.5, 0.0, 5.0, 5.0)]).unwrap();
        assert!((area_of(&i) - 0.5).abs() < 1e-9, "0.5, and not the 4 that any two cover");
    }

    #[test]
    fn disjoint_shapes_intersect_in_nothing() {
        let i = intersect(&[rect(0.0, 0.0, 1.0, 1.0), rect(2.0, 2.0, 3.0, 3.0)]).unwrap();
        assert!(rows::clean(&i).is_empty());
    }

    #[test]
    fn subtract_takes_away_every_other_shape_and_keeps_the_hole() {
        let s = subtract(&rect(0.0, 0.0, 4.0, 4.0), &[rect(1.0, 1.0, 2.0, 2.0), rect(3.0, 3.0, 5.0, 5.0)]).unwrap();
        let loops = rows::clean(&s);
        assert_eq!(loops.len(), 2, "The outline and the hole");
        assert!((area_of(&s) - 14.0).abs() < 1e-9);
        let nothing = subtract(&rect(1.0, 1.0, 2.0, 2.0), &[rect(0.0, 0.0, 4.0, 4.0)]).unwrap();
        assert!(rows::clean(&nothing).is_empty(), "Nothing of B is left");
    }

    #[test]
    fn two_overlapping_squares_fragment_in_three_faces_ordered_by_top_then_left() {
        let faces = fragment(&[rect(1.0, 1.0, 3.0, 3.0), rect(2.0, 2.0, 4.0, 4.0)], &[]).unwrap();
        assert_eq!(faces.len(), 3);
        let boxes: Vec<(f64, f64, f64)> = faces.iter().map(|f| (f.bounding_box().y1, f.bounding_box().x0, area_of(f))).collect();
        assert!((boxes[0].0 - 4.0).abs() < 1e-9 && (boxes[0].2 - 3.0).abs() < 1e-9, "B less A first: {boxes:?}");
        assert!((boxes[1].0 - 3.0).abs() < 1e-9 && (boxes[1].1 - 1.0).abs() < 1e-9, "A less B second: {boxes:?}");
        assert!((boxes[2].1 - 2.0).abs() < 1e-9 && (boxes[2].2 - 1.0).abs() < 1e-9, "the overlap last: {boxes:?}");
    }

    #[test]
    fn a_cutting_line_splits_a_face_and_leaves_nothing_of_itself() {
        let faces = fragment(&[rect(0.0, 0.0, 4.0, 2.0)], &[line(2.0, -1.0, 2.0, 3.0)]).unwrap();
        assert_eq!(faces.len(), 2);
        assert!(faces.iter().all(|f| (area_of(f) - 4.0).abs() < 1e-9));
        assert!(faces[0].bounding_box().x0 < faces[1].bounding_box().x0, "leftmost first");
    }

    #[test]
    fn a_closed_cutting_path_cuts_and_is_not_a_shape_of_its_own() {
        // A square drawn with NoFill 1 across the middle of a bigger one: it cuts a face out of it and fills nothing.
        let faces = fragment(&[rect(0.0, 0.0, 4.0, 4.0)], &[rect(1.0, 1.0, 3.0, 3.0)]).unwrap();
        assert_eq!(faces.len(), 2, "the inside and the frame round it");
        let total: f64 = faces.iter().map(area_of).sum();
        assert!((total - 16.0).abs() < 1e-9, "{total}");
    }

    #[test]
    fn open_pieces_split_a_ring_and_keep_an_open_path_whole() {
        assert_eq!(open_pieces(&line(0.0, 0.0, 1.0, 1.0)).len(), 1);
        let pieces = open_pieces(&rect(0.0, 0.0, 1.0, 1.0));
        assert_eq!(pieces.len(), 2);
        for piece in &pieces {
            let (first, last) = (piece.segments().next().unwrap().start(), piece.segments().last().unwrap().end());
            assert!(first.distance(last) > 0.1, "no piece ends where it starts");
        }
        let mut loop_of_one_cubic = BezPath::new();
        loop_of_one_cubic.move_to((0.0, 0.0));
        loop_of_one_cubic.curve_to((2.0, 2.0), (-2.0, 2.0), (0.0, 0.0));
        assert_eq!(open_pieces(&loop_of_one_cubic).len(), 2);
    }

    #[test]
    fn each_refusal_of_the_library_is_said_in_its_own_words() {
        for (error, words) in [
            (PathOpsError::TooComplex, "too many path elements"),
            (PathOpsError::Degenerate, "too degenerate"),
            (PathOpsError::NonFinite, "not finite"),
            (PathOpsError::OpenPath, "could not be closed"),
        ] {
            let problem = refusal(error);
            assert!(!problem.bad_params && problem.message.contains(words), "{error:?}: {problem:?}");
        }
    }

    #[test]
    fn a_number_that_is_not_finite_is_refused_in_words() {
        let mut bad = rect(0.0, 0.0, 1.0, 1.0);
        bad.line_to((f64::NAN, 2.0));
        let error = union(&[bad, rect(0.0, 0.0, 1.0, 1.0)]).unwrap_err();
        assert!(!error.bad_params && error.message.contains("not finite"), "{error:?}");
    }

    #[test]
    fn stacked_copies_unite_and_intersect_to_the_shape_itself() {
        let a = rect(0.0, 0.0, 2.0, 2.0);
        let u = union(&[a.clone(), a.clone()]).unwrap();
        let i = intersect(&[a.clone(), a]).unwrap();
        assert!((area_of(&u) - 4.0).abs() < 1e-9 && (area_of(&i) - 4.0).abs() < 1e-9, "Neither gives nothing nor twice the area");
    }

    #[test]
    fn faces_are_ordered_by_the_top_of_their_boxes_then_their_left_sides_and_sides_a_rounding_apart_tie() {
        let faces = vec![
            rect(0.0, 0.0, 1.0, 1.0),
            rect(5.0, 5.0, 6.0, 6.0),
            rect(2.0, 5.0, 3.0, 6.0),
            rect(1.0, 3.0, 2.0, 4.0),
            // The top a ten-billionth above 6 ties with the others at 6: it is ordered by its left side, 4.
            rect(4.0, 5.0, 4.5, 6.0 + 1.0e-10),
        ];
        let order: Vec<(f64, f64)> = in_order(faces).iter().map(|f| (f.bounding_box().x0, f.bounding_box().y0)).collect();
        assert_eq!(order, [(2.0, 5.0), (4.0, 5.0), (5.0, 5.0), (1.0, 3.0), (0.0, 0.0)]);
    }

    fn polygon(points: &[(f64, f64)]) -> BezPath {
        let mut p = BezPath::new();
        for (i, point) in points.iter().enumerate() {
            if i == 0 {
                p.move_to(*point);
            } else {
                p.line_to(*point);
            }
        }
        p.close_path();
        p
    }

    #[test]
    fn a_path_that_lies_along_one_line_may_cover_no_area_and_any_other_may() {
        assert!(!may_cover_area(&BezPath::new()), "nothing");
        assert!(!may_cover_area(&line(0.0, 0.0, 1.0, 1.0)), "a line");
        let mut there_and_back = line(0.0, 0.0, 3.0, 1.0);
        there_and_back.line_to((0.0, 0.0));
        there_and_back.close_path();
        assert!(!may_cover_area(&there_and_back), "a line there and back");
        assert!(!may_cover_area(&polygon(&[(2.0, 2.0)])), "a point");
        assert!(!may_cover_area(&polygon(&[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (5.0, 5.0)])), "a run along one line");
        assert!(!may_cover_area(&polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 0.000_005)])), "thinner than the length two points are one at");
        assert!(may_cover_area(&rect(0.0, 0.0, 1.0, 1.0)), "a square");
        assert!(may_cover_area(&polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 0.1)])), "a triangle");
        assert!(
            may_cover_area(&polygon(&[(0.0, 0.0), (1000.0, 0.0), (1000.0, 0.0001)])),
            "a thin one that is wider than the length two points are one at"
        );
    }

    #[test]
    fn the_thickness_is_the_distance_from_the_line_through_the_farthest_point_and_not_the_cross_product_of_it() {
        // The third point is 0.9E-5 from the diagonal (a shift of 0.9E-5 * sqrt 2 straight down from its end), so the ring is flat, and 1.1E-5 from it, so it is not.
        let across = std::f64::consts::SQRT_2;
        assert!(!may_cover_area(&polygon(&[(0.0, 0.0), (100.0, 100.0), (100.0, 100.0 - 0.9e-5 * across)])), "0.9E-5 from the line");
        assert!(may_cover_area(&polygon(&[(0.0, 0.0), (100.0, 100.0), (100.0, 100.0 - 1.1e-5 * across)])), "1.1E-5 from the line");
        // And whichever side of the first point the others are, the reach is the distance from it: a ring that is not at the origin is judged as one that is.
        assert!(!may_cover_area(&polygon(&[(7.0, 7.0), (107.0, 107.0), (107.0, 107.0 - 0.9e-5 * across)])));
        assert!(may_cover_area(&polygon(&[(-7.0, 7.0), (-107.0, 107.0), (-107.0, 107.0 - 1.1e-5 * across)])));
    }

    #[test]
    fn a_flat_ring_out_near_the_largest_number_is_flat_and_the_reach_is_measured_from_its_first_point() {
        // Measured from the first point the reach is 1E307, finite; measured from the origin (a sum of the coordinates) it would overflow.
        assert!(!may_cover_area(&line(1.5e308, 1.5e308, 1.6e308, 1.6e308)));
        assert!(!may_cover_area(&line(1.5e308, 0.0, 1.6e308, 0.0)));
        assert!(!may_cover_area(&line(0.0, 1.5e308, 0.0, 1.6e308)));
    }

    #[test]
    fn a_ring_that_crosses_itself_so_that_its_signed_area_is_nothing_may_still_cover_area() {
        // A bowtie: two triangles of equal area turned opposite ways, whose signed areas cancel and whose odd-crossings area is two thirds.
        assert!(may_cover_area(&polygon(&[(0.0, 0.0), (2.0, 2.0), (2.0, 0.0), (0.0, 2.0)])));
    }

    #[test]
    fn a_curved_ring_whose_ends_lie_along_a_line_but_whose_controls_do_not_may_cover_area() {
        let mut lens = BezPath::new();
        lens.move_to((0.0, 0.0));
        lens.curve_to((1.0, 2.0), (3.0, 2.0), (4.0, 0.0));
        lens.curve_to((3.0, -2.0), (1.0, -2.0), (0.0, 0.0));
        lens.close_path();
        assert!(may_cover_area(&lens));
        // The same with its controls on the line is flat.
        let mut flat = BezPath::new();
        flat.move_to((0.0, 0.0));
        flat.curve_to((1.0, 0.0), (3.0, 0.0), (4.0, 0.0));
        flat.curve_to((3.0, 0.0), (1.0, 0.0), (0.0, 0.0));
        flat.close_path();
        assert!(!may_cover_area(&flat));
    }

    #[test]
    fn one_ring_among_flat_ones_is_enough_and_every_ring_is_looked_at() {
        let mut path = line(0.0, 0.0, 1.0, 1.0);
        for el in rect(5.0, 5.0, 6.0, 6.0).elements() {
            path.push(*el);
        }
        assert!(may_cover_area(&path), "the second ring is a square");
        let mut flat_after = rect(5.0, 5.0, 6.0, 6.0);
        for el in line(0.0, 0.0, 1.0, 1.0).elements() {
            flat_after.push(*el);
        }
        assert!(may_cover_area(&flat_after), "the first ring is a square");
    }

    #[test]
    fn a_ring_far_out_or_huge_is_judged_by_its_own_shape_and_nothing_overflows() {
        assert!(may_cover_area(&rect(1.0e12, 1.0e12, 1.0e12 + 3.0, 1.0e12 + 3.0)), "a small square at 1E12");
        assert!(!may_cover_area(&line(1.0e12, 1.0e12, 1.0e12 + 3.0, 1.0e12 + 3.0)), "and a line there");
        assert!(may_cover_area(&rect(1.0e300, 1.0e300, 2.0e300, 2.0e300)), "a square of 1E300: the library says it is too degenerate, in words");
        assert!(!may_cover_area(&line(-1.0e300, 0.0, 1.0e300, 0.0)), "a line of 2E300 is flat");
        assert!(may_cover_area(&rect(-1.7e308, -1.7e308, 1.7e308, 1.7e308)), "a reach that overflows is not judged flat");
    }
}
