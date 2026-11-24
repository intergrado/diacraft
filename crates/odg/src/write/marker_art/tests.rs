//! The art of the line-end markers: the vectors and every number at every band class, checked point by point against
//! the pieces of R29, which are the oracle: a point is in a mark when it is in any of the pieces, and the path must fill alike by the even-odd and the
//! nonzero rule.

use super::*;

type F = (f64, f64);

/// The subpaths of a path of `M`, `L`, `C` and `Z`, curves flattened into 40 lines each.
fn parse(d: &str) -> Vec<Vec<F>> {
    let tokens: Vec<&str> = d.split(' ').collect();
    let (mut out, mut at): (Vec<Vec<F>>, usize) = (Vec::new(), 0);
    let num = |k: usize| -> f64 { tokens[k].parse().unwrap() };
    while at < tokens.len() {
        match tokens[at] {
            "M" => {
                out.push(vec![(num(at + 1), num(at + 2))]);
                at += 3;
            }
            "L" => {
                out.last_mut().unwrap().push((num(at + 1), num(at + 2)));
                at += 3;
            }
            "C" => {
                let sub = out.last_mut().unwrap();
                let p0 = *sub.last().unwrap();
                let (p1, p2, p3) = ((num(at + 1), num(at + 2)), (num(at + 3), num(at + 4)), (num(at + 5), num(at + 6)));
                for s in 1..=40 {
                    let t = f64::from(s) / 40.0;
                    let u = 1.0 - t;
                    let x = u * u * u * p0.0 + 3.0 * u * u * t * p1.0 + 3.0 * u * t * t * p2.0 + t * t * t * p3.0;
                    let y = u * u * u * p0.1 + 3.0 * u * u * t * p1.1 + 3.0 * u * t * t * p2.1 + t * t * t * p3.1;
                    sub.push((x, y));
                }
                at += 7;
            }
            "Z" => at += 1,
            other => panic!("token {other} in {d}"),
        }
    }
    out
}

/// The winding number of the subpaths about `p`.
fn nonzero(paths: &[Vec<F>], p: F) -> i32 {
    let mut w = 0;
    for c in paths {
        for i in 0..c.len() {
            let (a, b) = (c[i], c[(i + 1) % c.len()]);
            let side = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
            if a.1 <= p.1 {
                if b.1 > p.1 && side > 0.0 {
                    w += 1;
                }
            } else if b.1 <= p.1 && side < 0.0 {
                w -= 1;
            }
        }
    }
    w
}

/// Whether the even-odd rule fills `p`: the crossings of a ray to the right are odd.
fn even_odd(paths: &[Vec<F>], p: F) -> bool {
    let mut inside = false;
    for c in paths {
        for i in 0..c.len() {
            let (a, b) = (c[i], c[(i + 1) % c.len()]);
            if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0) {
                inside = !inside;
            }
        }
    }
    inside
}

fn built(n: u32, band: u32) -> Built {
    build(n, band, &mut 0).unwrap_or_else(|| panic!("number {n} at band {band} is not built"))
}

/// Whether the mark's path fills the point of the view box `(x, y)` by the even-odd rule, and the same by the nonzero one.
fn fills(b: &Built, x: f64, y: f64) -> bool {
    let paths = parse(&b.d);
    let (eo, nz) = (even_odd(&paths, (x, y)), nonzero(&paths, (x, y)) != 0);
    assert_eq!(eo, nz, "the two rules differ at ({x}, {y}) in {}", b.d);
    eo
}

fn view(b: &Built) -> Vec<i64> {
    b.view_box.split(' ').map(|p| p.parse().unwrap()).collect()
}

#[test]
fn odg_styles_a_hollow_triangle_is_a_triangle_and_a_hole_running_the_other_way() {
    // EndArrow 16, size 2, 0.75 pt: S 3.27 pt, band class 23.
    let b = built(16, 23);
    assert_eq!(b.view_box, "0 0 2000 2000");
    assert_eq!(b.d, "M 1000 0 L 2000 2000 L 0 2000 Z M 1000 514 L 372 1770 L 1628 1770 Z");
    for (x, y, filled) in [(1000.0, 1300.0, false), (1000.0, 1000.0, false), (1000.0, 1800.0, true), (1000.0, 300.0, true), (400.0, 1500.0, true)] {
        assert_eq!(fills(&b, x, y), filled, "({x}, {y})");
    }
    // The solid form fills (1000, 1300), and a band of 115 units (the weight's share of the width) leaves (1000, 1800) empty: both are wrong.
    assert!(fills(&built(4, 0), 1000.0, 1300.0));
    let paths = parse(&b.d);
    assert!(nonzero(&paths, (1000.0, 1800.0)) != 0);
}

#[test]
fn odg_styles_a_band_wider_than_the_inradius_leaves_the_whole_triangle() {
    // 5 pt at class 2: S 7.52 pt, band class 66, more than the triangle's inradius 0.618, so the inset is empty.
    let b = built(16, 66);
    assert_eq!(b.view_box, "0 0 2000 2000");
    assert_eq!(b.d, "M 1000 0 L 2000 2000 L 0 2000 Z");
    assert!(fills(&b, 1000.0, 1300.0));
    // At 61 the inset is a small triangle still; at 62 it has turned over and is empty.
    assert!(build(16, 61, &mut 0).unwrap().d.matches('M').count() == 2);
    assert!(build(16, 62, &mut 0).unwrap().d.matches('M').count() == 1);
}

#[test]
fn odg_styles_a_crossbar_is_united_with_the_spine_that_puts_the_top_of_the_box_on_the_vertex() {
    let b = built(24, 23);
    assert_eq!(b.view_box, "0 0 1500 1615");
    for (x, y, filled) in [
        (750.0, 700.0, true),
        (300.0, 1500.0, true),
        (300.0, 700.0, false),
        (300.0, 1350.0, false),
        (640.0, 5.0, true),
        (630.0, 5.0, false),
        (870.0, 5.0, false),
    ] {
        assert_eq!(fills(&b, x, y), filled, "({x}, {y})");
    }
    // The bar spans y 1385 to 1615 and the spine x 635 to 865 from y 0: one contour, a T upside down.
    assert_eq!(b.d, "M 635 0 L 865 0 L 865 1385 L 1500 1385 L 1500 1615 L 0 1615 L 0 1385 L 635 1385 Z");
}

#[test]
fn odg_styles_a_slanted_stroke_through_the_vertex_is_centred_and_reaches_one_point_one_six_two_six() {
    let b = built(9, 23);
    assert_eq!(b.view_box, "0 0 2000 2326");
    // The vertex is the box's centre, and the stroke is on both sides of it.
    assert!(fills(&b, 1000.0, 1163.0));
    assert!(fills(&b, 1900.0, 1163.0 - 900.0 - 20.0) || fills(&b, 1900.0, 1163.0 - 900.0 + 20.0));
    let paths = parse(&b.d);
    let ys: Vec<f64> = paths.iter().flatten().map(|p| p.1).collect();
    assert_eq!((ys.iter().cloned().fold(f64::MAX, f64::min), ys.iter().cloned().fold(f64::MIN, f64::max)), (0.0, 2326.0));
}

#[test]
fn odg_styles_the_boxes_of_the_unmeasured_marks_and_the_art_in_them() {
    // 44: an open head and one crossbar, the bar 0.75 S behind the head's back: y 2635 to 2865.
    let b = built(44, 23);
    assert_eq!(b.view_box, "0 0 2000 2865");
    assert!(fills(&b, 400.0, 2750.0) && !fills(&b, 400.0, 2600.0) && fills(&b, 1000.0, 2400.0));
    // A bar that followed the head at a leading bar's 1.5 S would make the box 3615 high.
    assert!(view(&b)[3] < 3000);
    // 8: a solid head with the back hollowed to 1.5 S, in 4's box.
    let b = built(8, 0);
    assert_eq!((b.view_box.as_str(), b.d.as_str()), ("0 0 2000 2000", "M 1000 0 L 2000 2000 L 1000 1500 L 0 2000 Z"));
    assert!(fills(&b, 1000.0, 1400.0) && !fills(&b, 1000.0, 1600.0));
    // 19: the same head hollow: the outer contour, and against it the hole.
    let b = built(19, 23);
    assert_eq!(b.view_box, "0 0 2000 2000");
    assert_eq!(b.d, "M 1000 0 L 2000 2000 L 1000 1500 L 0 2000 Z M 1000 514 L 514 1486 L 1000 1243 L 1486 1486 Z");
    assert!(!fills(&b, 1000.0, 1000.0) && fills(&b, 1000.0, 1400.0) && fills(&b, 1000.0, 300.0));
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------
// Every number at every band class against the pieces.

/// Whether `p` (in the frame of the pieces) is in the piece.
fn in_piece(piece: &Piece, p: F) -> bool {
    match piece {
        Piece::Disc { cy, r, hole } => {
            let d = p.0.hypot(p.1 - *cy as f64);
            d <= *r as f64 && d > *hole as f64
        }
        Piece::Poly(contours) => {
            let paths: Vec<Vec<F>> = contours.iter().map(|c| c.iter().map(|q| (q.0 as f64, q.1 as f64)).collect()).collect();
            nonzero(&paths, p) != 0
        }
    }
}

/// How far `p` is from the nearest edge of any piece.
fn edge_distance(pieces: &[Piece], p: F) -> f64 {
    let mut best = f64::MAX;
    for piece in pieces {
        match piece {
            Piece::Disc { cy, r, hole } => {
                let d = p.0.hypot(p.1 - *cy as f64);
                best = best.min((d - *r as f64).abs());
                if *hole > 0 {
                    best = best.min((d - *hole as f64).abs());
                }
            }
            Piece::Poly(contours) => {
                for c in contours {
                    for i in 0..c.len() {
                        let (a, b) = (c[i], c[(i + 1) % c.len()]);
                        let (a, b) = ((a.0 as f64, a.1 as f64), (b.0 as f64, b.1 as f64));
                        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                        let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                        best = best.min((p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy));
                    }
                }
            }
        }
    }
    best
}

/// The band classes to build for number `n`: 0 for a mark with no band, else 1 to 99.
fn bands(n: u32) -> Vec<u32> {
    if has_band(n) { (1..=99).collect() } else { vec![0] }
}

/// The path of the marker of `n` and `band` against its pieces on a grid of `step` units, leaving out the points within `slack` of an edge (the path's corners
/// are whole numbers and its cuts are rounded, so an edge is where it was to within a unit).
fn check_against_pieces(n: u32, band: u32, step: f64, slack: f64) {
    let b = built(n, band);
    let all = pieces(n, f64::from(band) / 100.0).unwrap();
    let (mut x0, mut y0) = (i64::MAX, i64::MAX);
    for (a, c, _, _) in all.iter().map(bounds) {
        (x0, y0) = (x0.min(a), y0.min(c));
    }
    let paths = parse(&b.d);
    let size = view(&b);
    let (w, h) = (size[2] as f64, size[3] as f64);
    let mut y = step / 3.0;
    while y < h {
        let mut x = step / 7.0;
        while x < w {
            let p = (x + x0 as f64, y + y0 as f64);
            if edge_distance(&all, p) > slack {
                let want = all.iter().any(|piece| in_piece(piece, p));
                let (eo, nz) = (even_odd(&paths, (x, y)), nonzero(&paths, (x, y)) != 0);
                assert_eq!((eo, nz), (want, want), "number {n} band {band} at ({x}, {y}): {}", b.d);
            }
            x += step;
        }
        y += step;
    }
}

#[test]
fn every_number_at_every_band_class_fills_the_union_of_its_pieces_by_both_rules() {
    for n in 1..=45 {
        for band in bands(n) {
            check_against_pieces(n, band, 61.0, 3.0);
        }
    }
}

#[test]
fn a_finer_grid_for_the_bands_in_use_and_the_extremes() {
    for n in 1..=45 {
        for band in bands(n).into_iter().filter(|b| [0, 1, 2, 4, 8, 16, 23, 33, 44, 54, 66, 75, 90, 99].contains(b)) {
            check_against_pieces(n, band, 17.0, 3.0);
        }
    }
}

#[test]
fn every_marker_is_within_its_bounds_with_whole_numbers_and_the_spread_of_its_number() {
    for n in 1..=45 {
        for band in bands(n) {
            let b = built(n, band);
            let size = view(&b);
            assert_eq!((size[0], size[1]), (0, 0), "{n}/{band}");
            // The width is the number's spread, in thousandths.
            let spread = crate::read::props::spread(n).unwrap();
            assert_eq!(size[2], (spread * 1000.0).round() as i64, "number {n} band {band}: {}", b.view_box);
            assert!(b.d.len() <= MOST_PATH_BYTES, "{n}/{band}: {} bytes", b.d.len());
            // Every number of the path is a whole number within the box.
            let (mut lo, mut hi) = (i64::MAX, i64::MIN);
            for t in b.d.split(' ').filter(|t| t.parse::<f64>().is_ok()) {
                let v: i64 = t.parse().unwrap_or_else(|_| panic!("{n}/{band}: {t} is not whole in {}", b.d));
                (lo, hi) = (lo.min(v), hi.max(v));
            }
            assert!(lo >= 0 && hi <= size[2].max(size[3]), "{n}/{band}: {lo}..{hi} in {}", b.view_box);
            // The box is touched on all four sides by the path (the path's own box is the view box).
            let paths = parse(&b.d);
            let xs: Vec<f64> = paths.iter().flatten().map(|p| p.0).collect();
            let ys: Vec<f64> = paths.iter().flatten().map(|p| p.1).collect();
            let (min_x, max_x) = (xs.iter().cloned().fold(f64::MAX, f64::min), xs.iter().cloned().fold(f64::MIN, f64::max));
            let (min_y, max_y) = (ys.iter().cloned().fold(f64::MAX, f64::min), ys.iter().cloned().fold(f64::MIN, f64::max));
            assert!(min_x.abs() < 1e-6 && min_y.abs() < 1e-6, "{n}/{band}: {min_x} {min_y}");
            assert!((max_x - size[2] as f64).abs() < 1.0 && (max_y - size[3] as f64).abs() < 1.0, "{n}/{band}: {max_x} {max_y} in {}", b.view_box);
        }
    }
}

#[test]
fn no_two_sides_of_a_united_path_cross() {
    for n in 1..=45 {
        for band in bands(n) {
            let b = built(n, band);
            if b.d.contains('C') {
                continue;
            }
            let paths = parse(&b.d);
            let sides: Vec<(IPt, IPt)> = paths
                .iter()
                .flat_map(|c| (0..c.len()).map(move |i| (c[i], c[(i + 1) % c.len()])))
                .map(|(a, c)| ((a.0 as i64, a.1 as i64), (c.0 as i64, c.1 as i64)))
                .collect();
            for (i, (a, c)) in sides.iter().enumerate() {
                for (j, (p, q)) in sides.iter().enumerate().skip(i + 1) {
                    // Sides that meet only at a shared corner are fine; a crossing, or a corner on another side's inside, is not.
                    let proper = {
                        let cr = |o: IPt, x: IPt, y: IPt| (x.0 - o.0) * (y.1 - o.1) - (x.1 - o.1) * (y.0 - o.0);
                        let (d1, d2, d3, d4) = (cr(*p, *q, *a), cr(*p, *q, *c), cr(*a, *c, *p), cr(*a, *c, *q));
                        ((d1 > 0) != (d2 > 0) && d1 != 0 && d2 != 0) && ((d3 > 0) != (d4 > 0) && d3 != 0 && d4 != 0)
                    };
                    assert!(!proper, "number {n} band {band}: sides {i} and {j} cross in {}", b.d);
                }
            }
        }
    }
}

#[test]
fn the_open_side_of_each_open_piece_is_horizontal_at_the_back_or_the_front() {
    // `back` is the side farthest from the vertex and `front` the nearest: the sides the table names are the horizontal ones at the greatest and the least y.
    for (shape, side, back) in [(tri(0.0, 2.0, 1.0), 1, true), (curved(), 2, true), (fork(), 0, false)] {
        let n = shape.len();
        let (a, b) = (shape[side], shape[(side + 1) % n]);
        assert_eq!(a.1, b.1, "{shape:?}");
        let extreme = shape.iter().map(|p| p.1).fold(if back { f64::MIN } else { f64::MAX }, |m, y| if back { m.max(y) } else { m.min(y) });
        assert_eq!(a.1, extreme, "{shape:?}");
    }
}

#[test]
fn a_number_or_band_that_does_not_belong_is_not_built() {
    for (n, band) in [(0, 0), (46, 0), (16, 0), (16, 100), (4, 23), (254, 0)] {
        assert_eq!(build(n, band, &mut 0), None, "{n}/{band}");
    }
}

#[test]
fn which_marks_have_a_band_and_which_are_centred_is_table() {
    let banded: Vec<u32> = (1..=45).filter(|n| has_band(*n)).collect();
    assert_eq!(banded.len(), 35);
    for n in [2, 4, 5, 6, 8, 10, 11, 13, 39, 42] {
        assert!(!has_band(n), "{n}");
    }
    let centred: Vec<u32> = (1..=45).filter(|n| is_centred(*n)).collect();
    assert_eq!(centred, [9, 10, 11, 20, 21, 42]);
}

#[test]
fn an_inset_that_is_not_a_simple_polygon_with_every_side_keeping_its_direction_is_empty() {
    let t = tri(0.0, 2.0, 1.0);
    assert!(inset(&t, &[0.1; 3]).is_some());
    assert!(inset(&t, &[0.618; 3]).is_some());
    assert!(inset(&t, &[0.62; 3]).is_none(), "past the inradius the triangle has turned over");
    // The inset of V32 to the thousandth: apex (0, 0.514) and the base corners (±0.628, 1.77) at a band of 0.23.
    let w = inset(&t, &[0.23; 3]).unwrap();
    assert!((w[0].1 - 0.5143).abs() < 1e-3 && (w[1].0 - 0.6278).abs() < 1e-3 && (w[1].1 - 1.77).abs() < 1e-9 && (w[2].0 + 0.6278).abs() < 1e-3);
}

#[test]
fn a_union_of_pieces_that_overlap_is_one_contour_and_pieces_that_touch_are_apart() {
    let mut steps = 0;
    // Two rectangles that overlap: one outline.
    let a = vec![(0, 0), (100, 0), (100, 100), (0, 100)];
    let b = vec![(50, 50), (150, 50), (150, 150), (50, 150)];
    let u = unite(&[a.clone(), b], &mut steps).unwrap();
    assert_eq!(u.len(), 1);
    assert_eq!(u[0].len(), 8);
    // Two that share a side: one rectangle, the shared side gone.
    let c = vec![(100, 0), (200, 0), (200, 100), (100, 100)];
    let u = unite(&[a.clone(), c], &mut steps).unwrap();
    assert_eq!(u, vec![vec![(0, 0), (200, 0), (200, 100), (0, 100)]]);
    // Two that touch at a corner stay two outlines' worth of edges (a pinch).
    let d = vec![(100, 100), (200, 100), (200, 200), (100, 200)];
    let u = unite(&[a, d], &mut steps).unwrap();
    assert_eq!(u.iter().map(Vec::len).sum::<usize>(), 8);
    assert!(steps > 0);
}

#[test]
fn a_union_of_too_many_edges_is_refused_and_the_work_is_counted() {
    let many: Vec<Vec<IPt>> = (0..50).map(|k| vec![(k * 10, 0), (k * 10 + 5, 0), (k * 10 + 5, 5), (k * 10, 5)]).collect();
    assert_eq!(unite(&many, &mut 0), None, "200 edges are past {MOST_EDGES}");
    let at_the_cap: Vec<Vec<IPt>> = (0..16).map(|k| vec![(k * 10, 0), (k * 10 + 5, 0), (k * 10 + 5, 5), (k * 10, 5)]).collect();
    assert!(unite(&at_the_cap, &mut 0).is_some(), "64 edges are the cap and pass");
    let past_the_cap: Vec<Vec<IPt>> = (0..17).map(|k| vec![(k * 10, 0), (k * 10 + 5, 0), (k * 10 + 5, 5), (k * 10, 5)]).collect();
    assert_eq!(unite(&past_the_cap, &mut 0), None, "68 edges are past it");
    let mut steps = 0;
    let few: Vec<Vec<IPt>> = (0..4).map(|k| vec![(k * 10, 0), (k * 10 + 5, 0), (k * 10 + 5, 5), (k * 10, 5)]).collect();
    assert!(unite(&few, &mut steps).is_some());
    // Sixteen edges: the pairs tried, 16 times 16, and the winding counts of the cuts.
    assert!((256..256 + 16 * 2 * 20 * 4).contains(&steps), "{steps}");
}

#[test]
fn the_work_of_building_every_marker_is_counted_and_follows_the_pieces_not_the_band() {
    let (mut total, mut worst, mut longest, mut count) = (0u64, (0u64, 0u32, 0u32), (0usize, 0u32, 0u32), 0u64);
    for n in 1..=45 {
        for band in bands(n) {
            let mut steps = 0;
            let b = built_counting(n, band, &mut steps);
            total += steps;
            count += 1;
            if steps > worst.0 {
                worst = (steps, n, band);
            }
            if b.d.len() > longest.0 {
                longest = (b.d.len(), n, band);
            }
        }
    }
    // 35 numbers with a band at 99 band classes each, and ten without: 3,475 markers, all that there can be.
    assert_eq!(count, 35 * 99 + 10);
    // Measured: 1,654,177 steps in all; the worst marker is 45's at band 1 with 2,736 (the most edges: two heads, a bar and two spines); the longest path is 499
    // bytes (33 at band 50). A marker costs the pairs of its edges and the edges walked by a winding count of each cut: no more than its pieces make.
    assert!((1_600_000..=1_700_000).contains(&total), "{total} steps in all");
    assert!(worst.0 >= 2_500 && worst.0 <= 2_800, "the worst marker: {worst:?}");
    assert!(longest.0 >= 450 && longest.0 <= 520, "the longest path: {longest:?}");
    // The cap on a path is far above the longest, and the cap on a cluster of edges above the largest cluster.
    assert!(MOST_PATH_BYTES >= 2 * longest.0);
    let mut largest = 0;
    for n in 1..=45 {
        for band in bands(n) {
            let all = pieces(n, f64::from(band) / 100.0).unwrap();
            for members in clusters(&all) {
                let edges: usize =
                    members.iter().filter_map(|i| all.get(*i)).map(|p| if let Piece::Poly(c) = p { c.iter().map(Vec::len).sum() } else { 0 }).sum();
                largest = largest.max(edges);
            }
        }
    }
    assert!((20..=30).contains(&largest), "the largest cluster has {largest} edges");
    assert!(MOST_EDGES >= 2 * largest);
}

fn built_counting(n: u32, band: u32, steps: &mut u64) -> Built {
    build(n, band, steps).unwrap_or_else(|| panic!("number {n} at band {band} is not built"))
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------
// The pieces of every number, pinned: the box and the area of the mark at eleven band classes (the oracle above shows that the path is the union of the
// pieces the code makes; this shows the pieces are the ones R29 gives, as a change to any of them moves a box or an area).

/// The area of a path in thousandths of `S` squared, rounded: polygons exactly, a circle by the 160 chords it is flattened into, a hole taking its area off.
fn path_area(d: &str) -> i64 {
    let mut twice = 0.0;
    for c in parse(d) {
        for i in 0..c.len() {
            let (a, b) = (c[i], c[(i + 1) % c.len()]);
            twice += a.0 * b.1 - b.0 * a.1;
        }
    }
    (twice / 2.0).round() as i64
}

/// The band classes the table is made at.
const PINNED_BANDS: [u32; 11] = [1, 5, 10, 23, 30, 45, 60, 66, 75, 90, 99];

/// One line for each number and band class: the number, the band class, the view box's width and height, the area and the count of subpaths.
fn pinned_lines() -> Vec<String> {
    let mut out = Vec::new();
    for n in 1..=45 {
        let bands: Vec<u32> = if has_band(n) { PINNED_BANDS.to_vec() } else { vec![0] };
        for band in bands {
            let b = built(n, band);
            let size = view(&b);
            out.push(format!("{n} {band} {} {} {} {}", size[2], size[3], path_area(&b.d), b.d.matches('M').count()));
        }
    }
    out
}

#[test]
#[ignore = "writes the pinned table: run by hand with ODG_LE_GOLDEN set to the file"]
fn zz_write_the_pinned_table() {
    let path = std::env::var("ODG_LE_GOLDEN").unwrap();
    std::fs::write(path, pinned_lines().join("\n") + "\n").unwrap();
}

#[test]
fn the_box_and_the_area_of_every_number_at_eleven_band_classes_are_the_pinned_ones() {
    let lines = pinned_lines();
    let pinned: Vec<&str> = include_str!("pinned.txt").lines().collect();
    assert_eq!(lines.len(), pinned.len());
    for (got, want) in lines.iter().zip(&pinned) {
        assert_eq!(got, want);
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------
// The helpers one by one, with values worked out by hand: the oracle above shows what the whole makes, and these what each step of it does.

#[test]
fn the_area_of_a_polygon_is_twice_its_signed_area_wherever_it_stands() {
    let square = [(0, 0), (2, 0), (2, 3), (0, 3)];
    assert_eq!(area2(&square), 12);
    let reversed = [(0, 3), (2, 3), (2, 0), (0, 0)];
    assert_eq!(area2(&reversed), -12);
    // A square moved far from the origin has the same area: the sum is of products of corners, which a sum of corners does not give.
    let moved = [(100, 200), (102, 200), (102, 203), (100, 203)];
    assert_eq!(area2(&moved), 12);
    assert_eq!(area2(&[(0, 0), (4, 0), (0, 4)]), 16);
    assert_eq!(area2(&[(0, 0), (1, 1)]), 0);
    assert_eq!(area2(&[]), 0);
    let f = |p: &[(i64, i64)]| p.iter().map(|q| (q.0 as f64 + 0.5, q.1 as f64 + 0.25)).collect::<Vec<Pt>>();
    assert_eq!(area2_f(&f(&square)), 12.0);
    assert_eq!(area2_f(&f(&reversed)), -12.0);
    assert_eq!(area2_f(&f(&moved)), 12.0);
    assert_eq!(area2_f(&[]), 0.0);
}

#[test]
fn a_polygon_runs_positive_after_positive_and_a_contour_with_no_area_is_left_as_it_is() {
    let square = vec![(0, 0), (2, 0), (2, 3), (0, 3)];
    assert_eq!(positive(square.clone()), square);
    let mut flipped = square.clone();
    flipped.reverse();
    assert_eq!(positive(flipped), square);
    let line = vec![(0, 0), (1, 1), (2, 2)];
    assert_eq!(positive(line.clone()), line, "no area: not turned over");
}

#[test]
fn a_length_is_thousandths_half_away_from_zero_and_a_polygon_loses_its_repeated_corners() {
    // Values that are exact in binary at the half, so that the tie is the tie.
    for (v, want) in
        [(0.0, 0), (0.0004, 0), (0.0006, 1), (0.0625, 63), (0.1875, 188), (-0.0004, 0), (-0.0006, -1), (-0.0625, -63), (-0.1875, -188), (1.5, 1500)]
    {
        assert_eq!(thousandths(v), want, "{v}");
    }
    assert_eq!(rounded(&[(0.0, 0.0), (0.0001, 0.0002), (1.0, 0.0), (1.0, 0.0004), (1.0, 1.0)]), vec![(0, 0), (1000, 0), (1000, 1000)]);
    assert_eq!(rounded(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0), (1.0, 0.0)]), vec![(0, 0), (1000, 0), (1000, 1000), (0, 0), (1000, 0)]);
    // The last corner that is the first again goes, once; one corner alone stays; two that differ stay.
    assert_eq!(rounded(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0001, 0.0)]), vec![(0, 0), (1000, 0), (1000, 1000)]);
    assert_eq!(rounded(&[(0.0001, 0.0)]), vec![(0, 0)]);
    assert_eq!(rounded(&[(0.0, 0.0), (0.0001, 0.0)]), vec![(0, 0)]);
    assert_eq!(rounded(&[(0.0, 0.0), (0.001, 0.0)]), vec![(0, 0), (1, 0)]);
    assert!(rounded(&[]).is_empty());
}

#[test]
fn the_named_shapes_are_the_points_gives() {
    assert_eq!(tri(0.0, 2.0, 1.0), vec![(0.0, 0.0), (1.0, 2.0), (-1.0, 2.0)]);
    assert_eq!(tri(2.0, 4.0, 1.0), vec![(0.0, 2.0), (1.0, 4.0), (-1.0, 4.0)]);
    assert_eq!(head(2.0, 0.995, 1.75), vec![(0.0, 0.0), (0.995, 2.0), (0.0, 1.75), (-0.995, 2.0)]);
    assert_eq!(diamond(0.0, 4.5, 1.12), vec![(0.0, 0.0), (1.12, 2.25), (0.0, 4.5), (-1.12, 2.25)]);
    assert_eq!(diamond(1.5, 6.0, 1.12), vec![(0.0, 1.5), (1.12, 3.75), (0.0, 6.0), (-1.12, 3.75)]);
    assert_eq!(square(0.0, 0.75), vec![(-0.75, -0.75), (0.75, -0.75), (0.75, 0.75), (-0.75, 0.75)]);
    assert_eq!(square(2.0, 0.5), vec![(-0.5, 1.5), (0.5, 1.5), (0.5, 2.5), (-0.5, 2.5)]);
    assert_eq!(fork(), vec![(-0.75, 0.0), (0.75, 0.0), (0.0, 1.5)]);
    assert_eq!(curved(), vec![(0.0, 0.08), (0.48, 0.5), (0.96, 1.84), (-0.96, 1.84), (-0.48, 0.5)]);
}

#[test]
fn bars_spines_strokes_and_discs_are_the_pieces_names() {
    let poly = |points: &[(i64, i64)]| Piece::Poly(vec![points.to_vec()]);
    // bar(y, h): x from -h to h, y from y - b/2 to y + b/2; spine(y0, y1): x from -b/2 to b/2.
    assert_eq!(bar(1.5, 0.75, 0.23), poly(&[(-750, 1385), (750, 1385), (750, 1615), (-750, 1615)]));
    assert_eq!(spine(0.0, 1.615, 0.23), poly(&[(-115, 0), (115, 0), (115, 1615), (-115, 1615)]));
    // stroke(P, Q): a parallelogram with vertical sides at the x of P and of Q, lifted by (b/2) |PQ| / |dx| (1.1626 at b = 0.23 for the slope of 9).
    let nine = stroke((1.0, -1.0), (-1.0, 1.0), 0.23);
    // (the corners run the other way when they are made, and are turned over: positive)
    assert_eq!(nine, poly(&[(1000, -837), (-1000, 1163), (-1000, 837), (1000, -1163)]));
    // A vertical stroke has no across to lift by: its sides are the same line.
    assert_eq!(stroke((0.0, 0.0), (0.0, 1.0), 0.2), Piece::Poly(vec![vec![(0, 0), (0, 1000)]]));
    // A stroke that goes the other way (up the page) is the same parallelogram, still running positive.
    let up = stroke((-1.0, 1.0), (1.0, -1.0), 0.23);
    let Piece::Poly(contours) = &up else { panic!("not a polygon") };
    assert_eq!(contours.len(), 1);
    assert!(area2(&contours[0]) > 0);
    assert_eq!(disc(0.75, 0.75), Piece::Disc { cy: 750, r: 750, hole: 0 });
    // A ring of a disc: the hole is R less the band, and none once that is not above 0.
    assert_eq!(disc_ring(0.75, 0.75, 0.23), Piece::Disc { cy: 750, r: 750, hole: 520 });
    assert_eq!(disc_ring(0.75, 0.75, 0.74), Piece::Disc { cy: 750, r: 750, hole: 10 });
    assert_eq!(disc_ring(0.75, 0.75, 0.75), Piece::Disc { cy: 750, r: 750, hole: 0 });
    assert_eq!(disc_ring(0.75, 0.75, 0.99), Piece::Disc { cy: 750, r: 750, hole: 0 });
}

#[test]
fn a_corner_of_an_inset_is_where_the_two_moved_sides_meet() {
    // The square 0..2: the corner at the origin, both sides moved in by a half, is (0.5, 0.5); the sides are the ones before and at the corner.
    let sq = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)];
    let c = corner(&sq, &[0.5; 4], 0).unwrap();
    assert!((c.0 - 0.5).abs() < 1e-12 && (c.1 - 0.5).abs() < 1e-12, "{c:?}");
    let c = corner(&sq, &[0.5; 4], 2).unwrap();
    assert!((c.0 - 1.5).abs() < 1e-12 && (c.1 - 1.5).abs() < 1e-12, "{c:?}");
    // With one side not moved (open), the corner slides along it.
    let c = corner(&sq, &[0.0, 0.5, 0.5, 0.5], 1).unwrap();
    assert!((c.0 - 1.5).abs() < 1e-12 && c.1.abs() < 1e-12, "{c:?}");
    // A side of no length, and two sides on one line, have no corner.
    assert_eq!(corner(&[(0.0, 0.0), (0.0, 0.0), (1.0, 2.0)], &[0.1; 3], 1), None);
    assert_eq!(corner(&[(0.0, 0.0), (0.0, 0.0), (1.0, 2.0)], &[0.1; 3], 0), None);
    assert_eq!(corner(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (1.0, 1.0)], &[0.1; 4], 1), None);
    // Sides that are nearly on one line are a corner far away (and not refused).
    assert!(corner(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.01), (1.0, 1.0)], &[0.1; 4], 1).is_some());
}

#[test]
fn a_polygon_is_simple_when_no_two_sides_meet_but_at_a_shared_corner() {
    let square = [(0, 0), (4, 0), (4, 4), (0, 4)];
    assert!(is_simple(&square));
    assert!(is_simple(&[(0, 0), (4, 0), (2, 3)]));
    // A bow tie: two sides cross.
    assert!(!is_simple(&[(0, 0), (6, 0), (6, 4), (3, -2), (0, 4)]), "two sides cross");
    assert!(!is_simple(&[(0, 0), (4, 4), (4, 0), (0, 4)]), "a bow tie has no area");
    // Running the other way is not a polygon of the orientation, and fewer than three corners are none.
    assert!(!is_simple(&[(0, 4), (4, 4), (4, 0), (0, 0)]));
    assert!(!is_simple(&[(0, 0), (4, 0)]));
    assert!(!is_simple(&[]));
    // A corner on another side's inside (a pinch), and a corner on another side's end, are meetings.
    assert!(!is_simple(&[(0, 0), (4, 0), (4, 4), (2, 0), (0, 4)]));
    assert!(!is_simple(&[(0, 0), (4, 0), (4, 4), (0, 4), (4, 0)]));
    // Two sides that share a corner and fold back on each other overlap (a spike), whichever pair of the polygon they are, the wrap-round pair too.
    assert!(!is_simple(&[(0, 0), (4, 0), (2, 0), (2, 3)]));
    assert!(!is_simple(&[(0, 0), (2, 0), (4, 0), (4, 4), (0, 4), (1, 0)]));
    // Sides that share a corner and go on in one direction are not an overlap, nor are sides that turn.
    assert!(is_simple(&[(0, 0), (2, 0), (4, 0), (4, 4), (0, 4)]));
    // Far apart sides in a long polygon: no meeting.
    assert!(is_simple(&[(0, 0), (10, 0), (10, 1), (1, 1), (1, 5), (0, 5)]));
}

#[test]
fn two_segments_meet_when_they_cross_touch_or_overlap() {
    let m = |a, b, p, q| segments_meet(a, b, p, q);
    assert!(m((0, 0), (4, 4), (0, 4), (4, 0)), "crossing");
    // A crossing is a crossing whichever way each of the two segments runs: the four orders put each pair of signs in each place.
    for (a, b, p, q) in [((0, 0), (4, 4), (0, 4), (4, 0)), ((0, 2), (6, 3), (2, 0), (3, 6)), ((1, 5), (7, 1), (3, 0), (4, 9))] {
        assert!(m(a, b, p, q) && m(b, a, p, q) && m(a, b, q, p) && m(b, a, q, p), "{a:?} {b:?} {p:?} {q:?}");
        assert!(m(p, q, a, b) && m(q, p, a, b) && m(p, q, b, a) && m(q, p, b, a), "{p:?} {q:?} {a:?} {b:?}");
    }
    // The same segments moved apart so that one stops short of the other, in all the orders.
    for (a, b, p, q) in [((0, 0), (4, 4), (0, 8), (3, 5)), ((0, 2), (6, 3), (2, 6), (3, 9))] {
        assert!(!m(a, b, p, q) && !m(b, a, p, q) && !m(a, b, q, p) && !m(b, a, q, p), "{a:?} {b:?} {p:?} {q:?}");
    }
    assert!(m((0, 0), (4, 0), (2, 0), (2, 3)), "a corner on a side's inside");
    // A T, whichever end of whichever segment is the foot of it: each of the four ends on the other segment's inside, in both orders of that segment.
    for (a, b, p, q) in
        [((2, 0), (2, 5), (0, 0), (4, 0)), ((2, 5), (2, 0), (0, 0), (4, 0)), ((0, 0), (4, 0), (2, 0), (2, 5)), ((0, 0), (4, 0), (2, 5), (2, 0))]
    {
        assert!(m(a, b, p, q) && m(a, b, q, p) && m(b, a, p, q) && m(b, a, q, p), "{a:?} {b:?} {p:?} {q:?}");
    }
    // The foot of the T short of the other segment is a miss, from each side.
    for (a, b, p, q) in [((2, 1), (2, 5), (0, 0), (4, 0)), ((0, 0), (4, 0), (2, 1), (2, 5))] {
        assert!(!m(a, b, p, q) && !m(a, b, q, p) && !m(b, a, p, q) && !m(b, a, q, p), "{a:?} {b:?} {p:?} {q:?}");
    }
    assert!(m((0, 0), (4, 0), (4, 0), (6, 3)), "a shared end");
    assert!(m((0, 0), (4, 0), (6, 0), (1, 0)) && m((0, 0), (4, 0), (1, 0), (6, 0)), "overlapping on one line");
    assert!(m((0, 0), (4, 0), (2, 0), (3, 0)), "one inside the other");
    assert!(!m((0, 0), (4, 0), (5, 0), (9, 0)), "on one line, apart");
    assert!(!m((0, 0), (4, 0), (0, 1), (4, 1)), "parallel");
    assert!(!m((0, 0), (4, 0), (2, 1), (2, 5)), "a line that would cross, short of it");
    assert!(!m((0, 0), (4, 4), (5, 0), (7, 3)), "apart, not parallel");
    assert!(!m((0, 0), (1, 1), (3, 3), (4, 4)), "collinear, apart on both");
    assert!(m((4, 0), (0, 0), (2, 3), (2, 0)), "the same, the other way round");
    // Corners far from the origin: the products are of differences.
    assert!(m((1000, 1000), (1004, 1004), (1000, 1004), (1004, 1000)));
    assert!(!m((1000, 1000), (1004, 1000), (1000, 1001), (1004, 1001)));
}

#[test]
fn a_point_is_inside_a_polygon_by_its_winding_and_on_its_edge_counts() {
    let sq = [(0, 0), (4, 0), (4, 4), (0, 4)];
    assert!(inside(&sq, (2, 2)));
    assert!(inside(&sq, (0, 0)) && inside(&sq, (4, 4)) && inside(&sq, (2, 0)) && inside(&sq, (4, 2)), "its edge");
    assert!(!inside(&sq, (5, 2)) && !inside(&sq, (-1, 2)) && !inside(&sq, (2, 5)) && !inside(&sq, (2, -1)));
    // A notch: the point in the notch is outside, the points beside it inside.
    let u = [(0, 0), (6, 0), (6, 6), (4, 6), (4, 2), (2, 2), (2, 6), (0, 6)];
    assert!(inside(&u, (1, 4)) && inside(&u, (5, 4)) && inside(&u, (3, 1)));
    assert!(!inside(&u, (3, 4)));
    // A point level with corners of the polygon (the rule for a corner on the ray is the same on both sides of it): a diamond's middle row, a notched hexagon's.
    let diamond = [(0, 0), (4, 4), (0, 8), (-4, 4)];
    for (at, want) in [
        ((1, 4), true),
        ((-1, 4), true),
        ((0, 4), true),
        ((3, 4), true),
        ((5, 4), false),
        ((-5, 4), false),
        ((2, 0), false),
        ((2, 8), false),
        ((1, 1), true),
    ] {
        assert_eq!(inside(&diamond, at), want, "{at:?}");
        let mut flipped = diamond.to_vec();
        flipped.reverse();
        assert_eq!(inside(&flipped, at), want, "{at:?} the other way");
    }
    let hex = [(0, 0), (6, 0), (8, 3), (6, 6), (0, 6), (-2, 3)];
    for (at, want) in [
        ((1, 3), true),
        ((7, 3), true),
        ((-1, 3), true),
        ((9, 3), false),
        ((-3, 3), false),
        ((3, 3), true),
        ((3, 0), true),
        ((3, 7), false),
        ((3, -1), false),
    ] {
        assert_eq!(inside(&hex, at), want, "{at:?}");
    }
    // The same polygon running the other way has winding -1 and is inside as well.
    let mut back = u.to_vec();
    back.reverse();
    assert!(inside(&back, (1, 4)) && !inside(&back, (3, 4)));
}

#[test]
fn an_inset_moves_every_side_in_and_is_empty_when_the_polygon_would_turn_over_or_cross_itself() {
    // The square 0..2 by a half: the square 0.5..1.5, the same orientation.
    let sq = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)];
    let w = inset(&sq, &[0.5; 4]).unwrap();
    for (got, want) in w.iter().zip([(0.5, 0.5), (1.5, 0.5), (1.5, 1.5), (0.5, 1.5)]) {
        assert!((got.0 - want.0).abs() < 1e-12 && (got.1 - want.1).abs() < 1e-12, "{w:?}");
    }
    // By a whole: a point, a side of no length: empty. Past it: turned over, empty.
    assert_eq!(inset(&sq, &[1.0; 4]), None);
    assert_eq!(inset(&sq, &[1.2; 4]), None);
    // By a band on one side only the other sides keep their length.
    assert!(inset(&sq, &[0.0, 0.5, 0.5, 0.5]).is_some());
    // The notch of a concave head moves inward as the sides do: the V32 numbers for 19's hole at 0.23.
    let w = inset(&head(2.0, 1.0, 1.5), &[0.23; 4]).unwrap();
    for (got, want) in w.iter().zip([(0.0, 0.5143), (0.4857, 1.4857), (0.0, 1.2429), (-0.4857, 1.4857)]) {
        assert!((got.0 - want.0).abs() < 1e-3 && (got.1 - want.1).abs() < 1e-3, "{w:?}");
    }
    // A zero offset moves nothing.
    let same = inset(&sq, &[0.0; 4]).unwrap();
    assert_eq!(same, sq.to_vec());
    // A polygon of fewer sides than offsets, or with a repeated corner, has none.
    assert_eq!(inset(&[(0.0, 0.0), (0.0, 0.0), (1.0, 1.0)], &[0.1; 3]), None);
}

#[test]
fn a_ring_is_the_polygon_and_a_hole_against_it_and_an_open_band_is_one_contour_along_all_sides_but_one() {
    let t = tri(0.0, 2.0, 1.0);
    // The outer triangle and the hole against it, from its first corner on.
    assert_eq!(ring(&t, 0.23), Piece::Poly(vec![vec![(0, 0), (1000, 2000), (-1000, 2000)], vec![(0, 514), (-628, 1770), (628, 1770)]]));
    // Past the inradius the piece is the whole triangle.
    assert_eq!(ring(&t, 0.62), solid(&t));
    // An open triangle, its back open: the band along the two sides, one contour, from the end of the open side round to its start and back along the inset.
    let Piece::Poly(band) = open(&t, 1, 0.23) else { panic!("not a polygon") };
    assert_eq!(band.len(), 1);
    // (the back side is not moved, so the inset's corners at its ends are on it, at x = 743: where the moved slanted side meets y = 2)
    assert_eq!(band[0], vec![(-1000, 2000), (0, 0), (1000, 2000), (743, 2000), (0, 514), (-743, 2000)]);
    // An open band stays a band past the inradius, while its back is not moved: the moved sides meet at 0.89 * sqrt(5) = 1.99 above the back at 2, and at 0.9 below it.
    assert_ne!(open(&t, 1, 0.62), solid(&t));
    assert_ne!(open(&t, 1, 0.89), solid(&t));
    assert_eq!(open(&t, 1, 0.9), solid(&t));
    // The band of an open fork (its front open) runs along the other two sides.
    let Piece::Poly(fork_band) = open(&fork(), 0, 0.23) else { panic!("not a polygon") };
    assert_eq!(fork_band.len(), 1);
    // The fork is 1,125,000 in area; its inset keeps the base and is 986 wide and 986 high: the band is what is left, 638,902.
    assert_eq!(area2(&fork_band[0]) / 2, 638_902, "the area of the fork's band at 0.23");
}

#[test]
fn which_pieces_overlap_is_by_their_boxes_and_a_touching_box_is_no_overlap() {
    let a = rect(0.0, 0.0, 1.0, 1.0);
    let b = rect(0.5, 0.5, 1.5, 1.5);
    let c = rect(1.0, 0.0, 2.0, 1.0);
    let d = rect(1.4, 1.4, 3.0, 3.0);
    let e = rect(0.0, 1.0, 1.0, 2.0);
    // a and b overlap; c shares a side with a and touches b; d overlaps b only: so a, b, d are one group (d through b), and c a group of its own until b joins it (c and b overlap).
    assert_eq!(clusters(&[a.clone(), c.clone()]), vec![vec![0], vec![1]]);
    // Touching is no overlap whichever side the other piece is on: left, right, above, below, and at each corner.
    let centre = rect(0.0, 0.0, 1.0, 1.0);
    for (x, y) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (1.0, 1.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0)] {
        let other = rect(x, y, x + 1.0, y + 1.0);
        assert_eq!(clusters(&[centre.clone(), other.clone()]), vec![vec![0], vec![1]], "({x}, {y})");
        assert_eq!(clusters(&[other, centre.clone()]), vec![vec![0], vec![1]], "({x}, {y}) first");
    }
    // And a sliver of overlap on each side is one.
    for (x, y) in [(0.9, 0.0), (-0.9, 0.0), (0.0, 0.9), (0.0, -0.9), (0.9, 0.9), (-0.9, -0.9)] {
        let other = rect(x, y, x + 1.0, y + 1.0);
        assert_eq!(clusters(&[centre.clone(), other.clone()]), vec![vec![0, 1]], "({x}, {y})");
        assert_eq!(clusters(&[other, centre.clone()]), vec![vec![0, 1]], "({x}, {y}) first");
    }
    assert_eq!(clusters(&[a.clone(), b.clone()]), vec![vec![0, 1]]);
    assert_eq!(clusters(&[a.clone(), e.clone()]), vec![vec![0], vec![1]], "sharing a side");
    assert_eq!(clusters(&[a.clone(), rect(1.0, 1.0, 2.0, 2.0)]), vec![vec![0], vec![1]], "sharing a corner");
    assert_eq!(clusters(&[a.clone(), b.clone(), d.clone()]), vec![vec![0, 1, 2]], "d overlaps b, b overlaps a");
    assert_eq!(clusters(&[d.clone(), a.clone(), b.clone()]), vec![vec![0, 1, 2]], "in any order");
    assert_eq!(clusters(&[a.clone(), c.clone(), b.clone()]), vec![vec![0, 1, 2]]);
    assert!(clusters(&[]).is_empty());
    // The box of a disc is its radius about its centre; the box of a polygon is its corners'.
    assert_eq!(bounds(&Piece::Disc { cy: 750, r: 750, hole: 100 }), (-750, 0, 750, 1500));
    assert_eq!(bounds(&a), (0, 0, 1000, 1000));
    assert_eq!(bounds(&Piece::Poly(vec![vec![(-5, 7), (3, -2)], vec![(1, 9)]])), (-5, -2, 3, 9));
}

#[test]
fn the_cuts_of_edges_are_where_other_edges_meet_them() {
    let at = |a, b, c, e| {
        let mut out = Vec::new();
        meets_of(a, b, c, e, &mut out);
        out.iter().map(|m| (m.at, m.num, m.den)).collect::<Vec<_>>()
    };
    // A crossing in the middle, and the same seen from the other edge.
    assert_eq!(at((0, 0), (10, 0), (5, -5), (5, 5)), vec![((5, 0), 50, 100)]);
    assert_eq!(at((5, -5), (5, 5), (0, 0), (10, 0)), vec![((5, 0), 50, 100)]);
    // A crossing at a half-integer is rounded the same from either edge, away from zero on the point itself.
    assert_eq!(at((0, 0), (3, 3), (0, 3), (3, 0)).iter().map(|m| m.0).collect::<Vec<_>>(), vec![(2, 2)]);
    assert_eq!(at((0, 3), (3, 0), (0, 0), (3, 3)).iter().map(|m| m.0).collect::<Vec<_>>(), vec![(2, 2)]);
    assert_eq!(at((-3, -3), (0, 0), (-3, 0), (0, -3)).iter().map(|m| m.0).collect::<Vec<_>>(), vec![(-2, -2)]);
    // An end of the other edge on this one's inside: a cut there; an end on this one's end: none.
    assert_eq!(at((0, 0), (10, 0), (4, 0), (4, 5)), vec![((4, 0), 20, 50)]);
    assert!(at((0, 0), (10, 0), (10, 0), (10, 5)).is_empty());
    assert!(at((0, 0), (10, 0), (0, 0), (0, 5)).is_empty());
    // Parallel and apart, and a crossing off the end: none.
    assert!(at((0, 0), (10, 0), (0, 1), (10, 1)).is_empty());
    assert!(at((0, 0), (10, 0), (12, -5), (12, 5)).is_empty());
    assert!(at((0, 0), (10, 0), (5, 1), (5, 5)).is_empty());
    // Two edges on one line: the ends of the stretch they share that lie inside this one, from either side.
    assert_eq!(at((0, 0), (10, 0), (4, 0), (14, 0)), vec![((4, 0), 40, 100)]);
    assert_eq!(at((0, 0), (10, 0), (14, 0), (4, 0)), vec![((4, 0), 40, 100)]);
    assert_eq!(at((0, 0), (10, 0), (3, 0), (6, 0)), vec![((3, 0), 30, 100), ((6, 0), 60, 100)]);
    assert!(at((0, 0), (10, 0), (11, 0), (14, 0)).is_empty());
    // Two edges on one slanted line: the share is by the dot product along the edge, 4 x 10 + 4 x 10 of 200, and the same going the other way and downward.
    assert_eq!(at((0, 0), (10, 10), (4, 4), (14, 14)), vec![((4, 4), 80, 200)]);
    assert_eq!(at((0, 0), (10, 10), (14, 14), (4, 4)), vec![((4, 4), 80, 200)]);
    assert_eq!(at((0, 0), (10, -10), (3, -3), (6, -6)), vec![((3, -3), 60, 200), ((6, -6), 120, 200)]);
    assert_eq!(at((10, 10), (0, 0), (4, 4), (14, 14)), vec![((4, 4), 120, 200)]);
    assert_eq!(at((0, 0), (6, 8), (3, 4), (9, 12)), vec![((3, 4), 50, 100)]);
    assert!(at((0, 0), (10, 10), (11, 11), (14, 14)).is_empty() && at((0, 0), (10, 10), (-4, -4), (0, 0)).is_empty());
    // A slanted crossing at a point that is not a whole number: the share and the rounded point.
    assert_eq!(at((0, 0), (7, 3), (2, 5), (3, -2)).iter().map(|m| m.0).collect::<Vec<_>>(), vec![(3, 1)]); // x = 2 + 29/52 = 2.56, y = 5 - 7 x 29/52 = 1.10
    assert!(at((0, 0), (10, 0), (-4, 0), (0, 0)).is_empty());
    // A sign: the same crossing with the other edge running the other way.
    assert_eq!(at((0, 0), (10, 0), (5, 5), (5, -5)), vec![((5, 0), 50, 100)]);
    assert_eq!(at((10, 0), (0, 0), (5, -5), (5, 5)), vec![((5, 0), 50, 100)]);
    // Rounding is half away from zero on whole values.
    assert_eq!((div_round(5, 2), div_round(-5, 2), div_round(4, 2), div_round(-4, 2), div_round(7, 3), div_round(-7, 3)), (3, -3, 2, -2, 2, -2));
    assert_eq!((div_round(1, 3), div_round(-1, 3), div_round(0, 5)), (0, 0, 0));
}

#[test]
fn a_union_walks_cuts_in_order_weighs_them_and_chains_what_it_keeps() {
    // Edges of the contours, none of no length.
    let sq = vec![(0, 0), (4, 0), (4, 4), (0, 4)];
    assert_eq!(edges_of(std::slice::from_ref(&sq)), vec![((0, 0), (4, 0)), ((4, 0), (4, 4)), ((4, 4), (0, 4)), ((0, 4), (0, 0))]);
    assert_eq!(edges_of(&[vec![(0, 0), (0, 0), (4, 0)]]), vec![((0, 0), (4, 0)), ((4, 0), (0, 0))]);
    assert!(edges_of(&[]).is_empty());
    // Cuts in order along each edge, the true ends kept: two squares crossing.
    let other = vec![(2, 2), (6, 2), (6, 6), (2, 6)];
    let edges = edges_of(&[sq.clone(), other.clone()]);
    let mut steps = 0;
    let cuts = cut_edges(&edges, &mut steps);
    assert_eq!(steps, 64, "each of eight edges is tried against each of eight");
    // 8 edges, the four that cross are cut once: 12 parts.
    assert_eq!(cuts.len(), 12);
    assert_eq!(cuts.iter().filter(|c| c.p == (4, 2) || c.q == (4, 2)).count(), 4);
    assert!(cuts.iter().all(|c| c.p != c.q));
    // The first cut of the first edge runs from its start to the crossing.
    assert_eq!((cuts[0].p, cuts[0].q), ((0, 0), (4, 0)));
    let mut s = 0;
    // The boundary of two squares, the cuts that have the outside on one side: 8 of 12 are the outline of the union, the other four are inside.
    let kept = boundary(&[sq.clone(), other.clone()], &cuts, &mut s).unwrap();
    assert_eq!(kept.len(), 8);
    assert!(kept.iter().all(|(a, b)| a != b));
    let chains = chain(&kept).unwrap();
    assert_eq!(chains.len(), 1);
    assert_eq!(chains[0].len(), 8);
    // A cut that is not balanced does not chain.
    assert_eq!(chain(&[((0, 0), (1, 0)), ((1, 0), (2, 0))]), None);
    assert_eq!(chain(&[]), Some(Vec::new()));
    // Two loops that touch at a point are two contours, however the edges come.
    let touching = vec![((0, 0), (1, 0)), ((1, 0), (1, 1)), ((1, 1), (0, 0)), ((1, 0), (2, 0)), ((2, 0), (2, 1)), ((2, 1), (1, 0))];
    assert_eq!(chain(&touching).unwrap().iter().map(Vec::len).sum::<usize>(), 6);
}

#[test]
fn the_winding_counts_the_pieces_that_cover_a_point_and_follows_a_half_open_rule_on_a_corner_or_a_side() {
    let sq = vec![(0, 0), (4, 0), (4, 4), (0, 4)];
    let other = vec![(2, 2), (6, 2), (6, 6), (2, 6)];
    // The winding counts the pieces that cover a point: 1, 2 and 0 for squares that overlap.
    let mut s = 0;
    assert_eq!(winding(&[sq.clone(), other.clone()], (1.0, 1.0), &mut s), 1);
    assert_eq!(winding(&[sq.clone(), other.clone()], (3.0, 3.0), &mut s), 2);
    assert_eq!(winding(&[sq.clone(), other.clone()], (5.0, 0.5), &mut s), 0);
    // A point level with a corner of the contour (the rule for a corner on the ray is the same on both sides of it): a diamond's middle row.
    let diamond_c = vec![(0, 0), (2, 2), (0, 4), (-2, 2)];
    for (at, want) in
        [((0.5, 2.0), 1), ((-0.5, 2.0), 1), ((3.0, 2.0), 0), ((-3.0, 2.0), 0), ((0.0, 2.0), 1), ((0.5, 0.0), 0), ((0.5, 4.0), 0), ((0.0, -1.0), 0)]
    {
        assert_eq!(winding(std::slice::from_ref(&diamond_c), at, &mut s), want, "{at:?}");
        let mut flipped = diamond_c.clone();
        flipped.reverse();
        assert_eq!(winding(std::slice::from_ref(&flipped), at, &mut s), -want, "{at:?} the other way");
    }
    // A point exactly on a side is not one the count is made for (the cuts are sampled a thousandth off): by the half-open rule a square counts its left and
    // bottom sides as inside and its right and top sides as outside.
    for (at, want) in [((0.0, 2.0), 1), ((2.0, 0.0), 1), ((4.0, 2.0), 0), ((2.0, 4.0), 0)] {
        assert_eq!(winding(std::slice::from_ref(&sq), at, &mut s), want, "{at:?}");
    }
    // The same level with a notch: two corners on the ray, the point between them outside.
    let notched = vec![(0, 0), (6, 0), (6, 4), (4, 2), (2, 2), (0, 4)];
    for (at, want) in [((1.0, 2.0), 1), ((5.0, 2.0), 1), ((3.0, 2.0), 0), ((7.0, 2.0), 0), ((3.0, 3.0), 0), ((3.0, 1.0), 1)] {
        assert_eq!(winding(std::slice::from_ref(&notched), at, &mut s), want, "{at:?}");
    }
    assert_eq!(winding(std::slice::from_ref(&sq), (-0.5, 2.0), &mut s), 0);
    assert_eq!(winding(std::slice::from_ref(&sq), (2.0, -0.5), &mut s), 0);
    assert_eq!(winding(std::slice::from_ref(&sq), (2.0, 4.5), &mut s), 0);
    assert_eq!(winding(std::slice::from_ref(&sq), (4.5, 2.0), &mut s), 0);
    assert_eq!(winding(&[sq.iter().rev().copied().collect()], (2.0, 2.0), &mut s), -1);
    assert_eq!(s, 3 * 8 + 16 * 4 + 4 * 4 + 6 * 6 + 4 * 4 + 4, "the edges of every contour, walked for each count");
}

#[test]
fn a_contour_is_tidied_to_its_corners_and_started_at_the_top() {
    // Repeated corners, corners on a straight run, and a spike go; a contour of fewer than three corners is none.
    assert_eq!(tidy(vec![(0, 0), (2, 0), (4, 0), (4, 4), (0, 4)]), vec![(0, 0), (4, 0), (4, 4), (0, 4)]);
    assert_eq!(tidy(vec![(0, 0), (4, 0), (4, 0), (4, 4), (0, 4)]), vec![(0, 0), (4, 0), (4, 4), (0, 4)]);
    assert_eq!(tidy(vec![(0, 0), (4, 0), (6, 0), (4, 0), (4, 4), (0, 4)]), vec![(0, 0), (4, 0), (4, 4), (0, 4)]);
    assert_eq!(tidy(vec![(0, 0), (1, 1), (2, 2), (3, 3)]), Vec::<IPt>::new());
    assert_eq!(tidy(vec![(0, 0), (1, 1)]), Vec::<IPt>::new());
    assert_eq!(tidy(vec![(0, 0), (4, 0), (4, 4)]), vec![(0, 0), (4, 0), (4, 4)]);
    // The wrap-round corner counts too: the first corner on the run of the last and the second.
    assert_eq!(tidy(vec![(2, 0), (4, 0), (4, 4), (0, 4), (0, 0)]), vec![(4, 0), (4, 4), (0, 4), (0, 0)]);
    // From the top, then the left; the direction stays.
    assert_eq!(from_the_top(vec![(4, 4), (0, 4), (0, 0), (4, 0)]), vec![(0, 0), (4, 0), (4, 4), (0, 4)]);
    assert_eq!(from_the_top(vec![(4, 0), (4, 4), (0, 4), (0, 0)]), vec![(0, 0), (4, 0), (4, 4), (0, 4)]);
    assert_eq!(from_the_top(vec![(5, 1), (3, 1), (4, 5)]), vec![(3, 1), (4, 5), (5, 1)]);
    assert_eq!(from_the_top(Vec::new()), Vec::<IPt>::new());
}

#[test]
fn a_disc_is_four_cubic_curves_and_a_polygon_a_run_of_lines_moved_to_the_corner() {
    // The disc of number 10 at (0, 0.75) radius 0.75: 750 across, the curves' controls at 0.5523 of the radius, 414.
    assert_eq!(built(10, 0).d, "M 750 0 C 1164 0 1500 336 1500 750 C 1500 1164 1164 1500 750 1500 C 336 1500 0 1164 0 750 C 0 336 336 0 750 0 Z");
    // A ring: the outer circle the way the polygons run and the inner one against it.
    let ring = built(20, 23);
    assert_eq!(ring.view_box, "0 0 1500 1500");
    assert_eq!(
        ring.d,
        "M 750 0 C 1164 0 1500 336 1500 750 C 1500 1164 1164 1500 750 1500 C 336 1500 0 1164 0 750 C 0 336 336 0 750 0 Z \
         M 750 230 C 463 230 230 463 230 750 C 230 1037 463 1270 750 1270 C 1037 1270 1270 1037 1270 750 C 1270 463 1037 230 750 230 Z"
    );
    assert_eq!(
        circle(100, 100, 50, false),
        "M 100 50 C 128 50 150 72 150 100 C 150 128 128 150 100 150 C 72 150 50 128 50 100 C 50 72 72 50 100 50 Z"
    );
    assert_eq!(
        circle(100, 100, 50, true),
        "M 100 50 C 72 50 50 72 50 100 C 50 128 72 150 100 150 C 128 150 150 128 150 100 C 150 72 128 50 100 50 Z"
    );
    assert_eq!(polygon(&[(10, 20), (30, 20), (30, 40)], 10, 20), "M 0 0 L 20 0 L 20 20 Z");
}

#[test]
fn each_number_is_the_list_of_pieces_gives_it() {
    // The pieces of each number at band 0.23: bars, spines, discs, bands and solid forms, counted.
    let counts: [(u32, usize); 45] = [
        (1, 2),
        (2, 1),
        (3, 2),
        (4, 1),
        (5, 1),
        (6, 1),
        (7, 2),
        (8, 1),
        (9, 2),
        (10, 1),
        (11, 1),
        (12, 2),
        (13, 1),
        (14, 1),
        (15, 1),
        (16, 1),
        (17, 1),
        (18, 1),
        (19, 1),
        (20, 1),
        (21, 1),
        (22, 1),
        (23, 2),
        (24, 2),
        (25, 3),
        (26, 4),
        (27, 2),
        (28, 4),
        (29, 2),
        (30, 3),
        (31, 3),
        (32, 4),
        (33, 5),
        (34, 2),
        (35, 3),
        (36, 4),
        (37, 5),
        (38, 2),
        (39, 2),
        (40, 2),
        (41, 1),
        (42, 1),
        (43, 3),
        (44, 3),
        (45, 5),
    ];
    for (n, count) in counts {
        assert_eq!(pieces(n, 0.23).unwrap().len(), count, "number {n}");
    }
    assert!(pieces(0, 0.23).is_none() && pieces(46, 0.23).is_none() && pieces(254, 0.23).is_none());
}

#[test]
fn a_side_keeps_its_direction_when_its_new_vector_points_the_same_way() {
    assert!(keeps_direction((2.0, 0.0), (0.4, 0.0)));
    assert!(keeps_direction((0.0, 2.0), (0.0, 0.4)));
    assert!(keeps_direction((2.0, 1.0), (1.0, 0.5)));
    assert!(keeps_direction((-3.0, 4.0), (-0.3, 0.4)));
    assert!(!keeps_direction((2.0, 0.0), (-0.4, 0.0)));
    assert!(!keeps_direction((0.0, 2.0), (0.0, -0.4)));
    assert!(!keeps_direction((2.0, 1.0), (-1.0, -0.5)));
    assert!(!keeps_direction((2.0, 0.0), (0.0, 0.0)), "a side that has shrunk to a point");
    assert!(!keeps_direction((2.0, 0.0), (0.0, 1.0)), "a side that has turned a right angle");
    assert!(!keeps_direction((2.0, 0.0), (f64::NAN, 0.0)));
}

#[test]
fn a_hole_is_cut_out_only_when_it_is_simple_inside_and_the_polygon_is_simple() {
    let outer = [(0, 0), (100, 0), (100, 100), (0, 100)];
    let hole = [(20, 20), (80, 20), (80, 80), (20, 80)];
    assert!(usable_hole(&outer, &hole));
    // A hole that crosses itself, one with a corner outside, one that runs the other way (it must run positive, as the inset does), and an outer that crosses itself.
    assert!(!usable_hole(&outer, &[(20, 20), (80, 80), (80, 20), (20, 80)]));
    assert!(!usable_hole(&outer, &[(20, 20), (80, 20), (80, 150), (20, 80)]));
    assert!(!usable_hole(&outer, &[(20, 80), (80, 80), (80, 20), (20, 20)]));
    assert!(!usable_hole(&[(0, 0), (100, 100), (100, 0), (0, 100)], &hole));
    assert!(!usable_hole(&outer, &[(20, 20), (80, 20)]), "fewer than three corners");
    // Every corner of the hole counts, not the first alone, and not the last alone.
    assert!(!usable_hole(&outer, &[(20, 20), (80, 20), (80, 80), (200, 80)]));
    assert!(!usable_hole(&outer, &[(-20, 20), (80, 20), (80, 80), (20, 80)]));
}

#[test]
fn pieces_one_unit_apart_in_either_direction_stay_two_pieces() {
    // The count on each side of a cut is taken a thousandth of a unit off it, so a neighbour a whole unit away is never counted: rectangles 10 across with a gap
    // of one unit across, down and on the slant are united as two contours of four corners each, in every order.
    let rect_at = |x: i64, y: i64| vec![(x, y), (x + 10, y), (x + 10, y + 10), (x, y + 10)];
    for (x, y) in [(11, 0), (0, 11), (11, 11), (-11, 0), (0, -11), (-11, 5), (5, 11)] {
        for flip in [false, true] {
            let (a, b) = (rect_at(0, 0), rect_at(x, y));
            let pieces = if flip { [b, a] } else { [a, b] };
            let u = unite(&pieces, &mut 0).unwrap_or_else(|| panic!("({x}, {y}) is not united"));
            assert_eq!(u.iter().map(Vec::len).collect::<Vec<_>>(), [4, 4], "({x}, {y}), flipped {flip}: {u:?}");
        }
    }
}

#[test]
fn the_box_of_all_the_pieces_is_the_least_and_greatest_corners_and_none_without_a_corner() {
    assert_eq!(box_of_all(&[]), None);
    assert_eq!(box_of_all(&[Piece::Poly(Vec::new())]), None);
    assert_eq!(box_of_all(&[rect(0.0, 0.0, 1.0, 2.0)]), Some((0, 0, 1000, 2000)));
    assert_eq!(box_of_all(&[rect(0.0, 0.0, 1.0, 2.0), rect(-3.0, 1.0, -2.0, 5.0)]), Some((-3000, 0, 1000, 5000)));
    assert_eq!(box_of_all(&[rect(0.0, 0.0, 1.0, 2.0), Piece::Disc { cy: 3000, r: 500, hole: 0 }]), Some((-500, 0, 1000, 3500)));
    // A box with no width (a single vertical line) is a box.
    assert_eq!(box_of_all(&[Piece::Poly(vec![vec![(5, 0), (5, 10)]])]), Some((5, 0, 5, 10)));
    assert_eq!(box_of_all(&[Piece::Poly(vec![vec![(0, 7), (10, 7)]])]), Some((0, 7, 10, 7)));
}
