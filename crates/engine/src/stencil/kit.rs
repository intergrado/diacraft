//! The kit the second batch's families build from: connection point sets, and figures that more than one family draws.
//!
//! **Original figures.** Everything here is plain geometry made by a function, drawn from the public notations' generic shapes (a dog-eared note, a cylinder, a stick figure) and nothing is
//! traced or copied. A figure is a `Vec<OwnedPath>` for [`super::Outline::Code`]: shares of the box, fixed lengths in inches and one clamped corner radius ([`super::Co`]), so a resize moves
//! every vertex. **A fixed length and a corner radius follow the page's drop scale** (`Co` writes them times the factor a drop beyond the range of eight wrote), so that the dog ear
//! of a note and the corner of a rounded box are as big on paper at 1:100 as at 1:1. **Every point set is on the outline it is made for and faces away from it**: the unit tests of this file and the catalogue test hold it.
//!
//! * Point sets: [`SIDES`], [`GRID3`], [`RING8`], [`TIPS4`], [`BAR`], and the stem of a lifeline ([`stem_points`], [`stem_array`]). A point set is a `const` a master names: `points: &GRID3`.
//! * A diagonal point of [`RING8`] leaves along the horizontal axis, since the router knows the four ways out and no more.
//! * Figures: [`note`], [`rounded_box`], [`cylinder_v`], [`cylinder_h`], [`stick_figure`], [`diamond`], [`folded_document`], [`box_3d`], and the label box [`LABEL_BELOW`].
//! * Compartments of a group master: [`compartment`] and [`compartments`] give the frames of members that stack from the top of the group by rank, each as tall as its text.

#![allow(dead_code, reason = "the kit the families build from (STENCIL2): a figure or a point set is used when the family that draws it lands")]

use super::group_master::MemberFrame;
use super::{Co, Flag, OwnedPath, Point, Seg, TextBox, fh, fk, fr, fw, mix, pt};

/// The four middles of the sides of a box, top first and then anticlockwise.
pub(super) const SIDES: [Point; 4] = [pt(0.5, 1.0, 0.0, 1.0), pt(0.0, 0.5, -1.0, 0.0), pt(0.5, 0.0, 0.0, -1.0), pt(1.0, 0.5, 1.0, 0.0)];

/// A diamond's four vertices: the middles of the sides of its box, as [`SIDES`] has them.
pub(super) const TIPS4: [Point; 4] = SIDES;

/// Three points on each side, at a quarter, a half and three quarters: twelve, taken anticlockwise round the box from the top's right end.
pub(super) const GRID3: [Point; 12] = grid3();

const fn grid3() -> [Point; 12] {
    let shares = [0.25, 0.5, 0.75];
    let mut out = [pt(0.0, 0.0, 0.0, 0.0); 12];
    let mut i = 0;
    while i < 3 {
        out[i] = pt(shares[2 - i], 1.0, 0.0, 1.0);
        out[3 + i] = pt(0.0, shares[2 - i], -1.0, 0.0);
        out[6 + i] = pt(shares[i], 0.0, 0.0, -1.0);
        out[9 + i] = pt(1.0, shares[i], 1.0, 0.0);
        i += 1;
    }
    out
}

/// Eight points on the ellipse that fills the box, every 45 degrees anticlockwise from the top. A diagonal one leaves along the horizontal axis.
pub(super) const RING8: [Point; 8] = [
    pt(0.5, 1.0, 0.0, 1.0),
    pt(0.146_446_609_406_726_2, 0.853_553_390_593_273_8, -1.0, 0.0),
    pt(0.0, 0.5, -1.0, 0.0),
    pt(0.146_446_609_406_726_2, 0.146_446_609_406_726_2, -1.0, 0.0),
    pt(0.5, 0.0, 0.0, -1.0),
    pt(0.853_553_390_593_273_8, 0.146_446_609_406_726_2, 1.0, 0.0),
    pt(1.0, 0.5, 1.0, 0.0),
    pt(0.853_553_390_593_273_8, 0.853_553_390_593_273_8, 1.0, 0.0),
];

/// Five points on each long side of a bar and one at each end: twelve. The top's run from the left, then the left end, the bottom's run from the left, and the right end.
pub(super) const BAR: [Point; 12] = bar();

const fn bar() -> [Point; 12] {
    let mut out = [pt(0.0, 0.0, 0.0, 0.0); 12];
    let mut i = 0;
    while i < 5 {
        let share = (i + 1) as f64 / 6.0;
        out[i] = pt(share, 1.0, 0.0, 1.0);
        out[6 + i] = pt(share, 0.0, 0.0, -1.0);
        i += 1;
    }
    out[5] = pt(0.0, 0.5, -1.0, 0.0);
    out[11] = pt(1.0, 0.5, 1.0, 0.0);
    out
}

/// The most steps of a lifeline's stem.
pub(crate) const MAX_STEMS: usize = 32;

/// The ways out of the point `ring_point(k, n)` of the ellipse: where it is, as shares of the box (the point `k` of `n`, anticlockwise from the top).
///
/// Total: an `n` of 0 gives the top, and a `k` past `n` goes round again.
pub(crate) fn ring_point(k: usize, n: usize) -> (f64, f64) {
    if n == 0 {
        return (0.5, 1.0);
    }
    let turn = (k % n) as f64 / n as f64;
    let angle = std::f64::consts::FRAC_PI_2 + turn * std::f64::consts::TAU;
    ((0.5 + 0.5 * angle.cos()).clamp(0.0, 1.0), (0.5 + 0.5 * angle.sin()).clamp(0.0, 1.0))
}

/// The points of a lifeline's stem for `n` steps (at most [`MAX_STEMS`]): at each step a pair on the stem's line, down the middle, one that leaves to the left and one to the right, at
/// even shares of the height from the top (`(i + 1) / (n + 1)`). A master's table holds a constant array: [`stem_array`] makes it.
pub(super) fn stem_points(n: usize) -> Vec<Point> {
    let n = n.min(MAX_STEMS);
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        let y = 1.0 - (i + 1) as f64 / (n + 1) as f64;
        out.push(pt(0.5, y, -1.0, 0.0));
        out.push(pt(0.5, y, 1.0, 0.0));
    }
    out
}

/// [`stem_points`] as a constant, for a master's table: `N` is twice the steps (`stem_array::<40>()` is `STEM(20)`).
pub(super) const fn stem_array<const N: usize>() -> [Point; N] {
    let steps = N / 2;
    let mut out = [pt(0.0, 0.0, 0.0, 0.0); N];
    let mut i = 0;
    while i < steps {
        let y = 1.0 - (i + 1) as f64 / (steps + 1) as f64;
        out[2 * i] = pt(0.5, y, -1.0, 0.0);
        out[2 * i + 1] = pt(0.5, y, 1.0, 0.0);
        i += 1;
    }
    out
}

/// The text block of a label that sits under its figure: 1.6 times as wide as the box, and 0.35 of its height tall, from 0.4 of the height below it to 0.05 below. Its place is a formula of
/// `Width` and `Height`, so that it follows the figure.
pub(super) const LABEL_BELOW: TextBox = TextBox { x: -0.3, y: -0.4, w: 1.6, h: 0.35 };

// ---- compartments -------------------------------------------------------------------------------------------------------------------

/// The frame of one compartment of a group master: as wide as the group, as tall as its text needs and at least `min_in` inches on the master's paper (the least follows the page's drop scale: times the group's `DropOnPageScale`, 1 where it holds none), standing under the compartments of lower `rank` (0 is the top). The
/// order of the members in the table does not stack them: a member's rank does, so the member last in the table (the one typing goes to) can be the name at the top. The group is as tall as its
/// compartments together, so it holds `LockHeight` and a line typed in a compartment makes it taller.
pub(super) const fn compartment(rank: u8, min_in: f64) -> MemberFrame {
    MemberFrame::Compartment { rank, min_in }
}

/// The frames of `N` compartments from the top, with the least height of each in inches: `compartments([0.35, 0.5, 0.5])` is the ranks 0, 1 and 2. A master's table gives each member one, in a
/// `const`: `frame: FRAMES[1]`, in whatever order the members stand in the table.
pub(super) const fn compartments<const N: usize>(min_in: [f64; N]) -> [MemberFrame; N] {
    let mut out = [MemberFrame::Compartment { rank: 0, min_in: 0.0 }; N];
    let mut i = 0;
    while i < N {
        let rank = if i > 255 { 255 } else { i as u8 };
        out[i] = MemberFrame::Compartment { rank, min_in: min_in[i] };
        i += 1;
    }
    out
}

// ---- figures ------------------------------------------------------------------------------------------------------------------------

const Z: Co = fw(0.0);
const W: Co = fw(1.0);
const H: Co = fh(1.0);

/// A quarter circle's midpoint is `1 - sqrt(0.5)` of the radius from its bounding corner.
const INSET: f64 = 0.292_893_218_813_452_4;

fn owned(fill: bool, segs: Vec<Seg>) -> OwnedPath {
    OwnedPath { fill, segs, no_show: Flag::Off, no_line: Flag::Off }
}

/// How far the corner of a dog-eared note is cut, in inches.
const NOTE_CUT: f64 = 0.15;

/// A dog-eared note: a rectangle whose top right corner is cut by 0.15 inch (less on a small box), and the fold as a stroke.
pub(super) fn note() -> Vec<OwnedPath> {
    let (across, up) = (W.plus(fr(NOTE_CUT, -1.0)), H.plus(fr(NOTE_CUT, -1.0)));
    vec![
        owned(true, vec![Seg::Move(Z, Z), Seg::Line(W, Z), Seg::Line(W, up), Seg::Line(across, H), Seg::Line(Z, H), Seg::Line(Z, Z)]),
        owned(false, vec![Seg::Move(across, H), Seg::Line(across, up), Seg::Line(W, up)]),
    ]
}

/// A document with a folded corner: the corner is cut by a quarter of the width, and the fold is a stroke.
pub(super) fn folded_document() -> Vec<OwnedPath> {
    let (across, up) = (fw(0.75), mix(-0.25, 1.0));
    vec![
        owned(true, vec![Seg::Move(Z, Z), Seg::Line(W, Z), Seg::Line(W, up), Seg::Line(across, H), Seg::Line(Z, H), Seg::Line(Z, Z)]),
        owned(false, vec![Seg::Move(across, H), Seg::Line(across, up), Seg::Line(W, up)]),
    ]
}

/// A rectangle with rounded corners of `r_in` inches, which follow the box when it is smaller than four of them (`MIN(r, MIN(Width,Height)*0.25)`). Generalised from Gane-Sarson's corner.
pub(super) fn rounded_box(r_in: f64) -> Vec<OwnedPath> {
    let (r, q) = (fr(r_in, 1.0), fr(r_in, INSET));
    let (wr, hr) = (W.plus(fr(r_in, -1.0)), H.plus(fr(r_in, -1.0)));
    let (wq, hq) = (W.plus(fr(r_in, -INSET)), H.plus(fr(r_in, -INSET)));
    vec![owned(
        true,
        vec![
            Seg::Move(r, Z),
            Seg::Line(wr, Z),
            Seg::Arc { to: (W, r), via: (wq, q) },
            Seg::Line(W, hr),
            Seg::Arc { to: (wr, H), via: (wq, hq) },
            Seg::Line(r, H),
            Seg::Arc { to: (Z, hr), via: (q, hq) },
            Seg::Line(Z, r),
            Seg::Arc { to: (r, Z), via: (q, q) },
        ],
    )]
}

/// A vertical cylinder: the body outlined with a half-ellipse at the bottom and one on top (each 0.12 of the height tall), and the front half of the top ellipse as a stroke.
pub(super) fn cylinder_v() -> Vec<OwnedPath> {
    let (rx, ry) = (fw(0.5), fh(0.12));
    vec![
        owned(
            true,
            vec![
                Seg::Move(Z, fh(0.88)),
                Seg::Line(Z, fh(0.12)),
                Seg::Half { to: (W, fh(0.12)), via: (fw(0.5), Z), rx, ry },
                Seg::Line(W, fh(0.88)),
                Seg::Half { to: (Z, fh(0.88)), via: (fw(0.5), H), rx, ry },
            ],
        ),
        owned(false, vec![Seg::Move(Z, fh(0.88)), Seg::Half { to: (W, fh(0.88)), via: (fw(0.5), fh(0.76)), rx, ry }]),
    ]
}

/// A horizontal cylinder (a queue): the body outlined with a half-ellipse at each end (each 0.12 of the width wide), and the inner half of the left end as a stroke.
pub(super) fn cylinder_h() -> Vec<OwnedPath> {
    let (rx, ry) = (fw(0.12), fh(0.5));
    vec![
        owned(
            true,
            vec![
                Seg::Move(fw(0.12), H),
                Seg::Line(fw(0.88), H),
                Seg::Half { to: (fw(0.88), Z), via: (W, fh(0.5)), rx, ry },
                Seg::Line(fw(0.12), Z),
                Seg::Half { to: (fw(0.12), H), via: (Z, fh(0.5)), rx, ry },
            ],
        ),
        owned(false, vec![Seg::Move(fw(0.12), H), Seg::Half { to: (fw(0.12), Z), via: (fw(0.24), fh(0.5)), rx, ry }]),
    ]
}

/// A stick figure: a round head (0.12 of the height in radius, at the top), and a body, arms and legs as strokes.
pub(super) fn stick_figure() -> Vec<OwnedPath> {
    let r = fh(0.12);
    let (left, right) = (mix(0.5, -0.12), mix(0.5, 0.12));
    vec![
        owned(
            true,
            vec![
                Seg::Move(left, fh(0.88)),
                Seg::Half { to: (right, fh(0.88)), via: (fw(0.5), H), rx: r, ry: r },
                Seg::Half { to: (left, fh(0.88)), via: (fw(0.5), fh(0.76)), rx: r, ry: r },
            ],
        ),
        owned(false, vec![Seg::Move(fw(0.5), fh(0.76)), Seg::Line(fw(0.5), fh(0.42))]),
        owned(false, vec![Seg::Move(Z, fh(0.62)), Seg::Line(W, fh(0.62))]),
        owned(false, vec![Seg::Move(Z, Z), Seg::Line(fw(0.5), fh(0.42)), Seg::Line(W, Z)]),
    ]
}

/// A diamond that fills the box.
pub(super) fn diamond() -> Vec<OwnedPath> {
    vec![owned(true, vec![Seg::Move(fw(0.5), Z), Seg::Line(W, fh(0.5)), Seg::Line(fw(0.5), H), Seg::Line(Z, fh(0.5)), Seg::Line(fw(0.5), Z)])]
}

/// A 3-D box: a front face, and a top and a side face `depth_in` inches deep. Each face takes the fill.
pub(super) fn box_3d(depth_in: f64) -> Vec<OwnedPath> {
    let d = fk(depth_in);
    let (wd, hd) = (W.plus(fk(-depth_in)), H.plus(fk(-depth_in)));
    vec![
        owned(true, vec![Seg::Move(Z, Z), Seg::Line(wd, Z), Seg::Line(wd, hd), Seg::Line(Z, hd), Seg::Line(Z, Z)]),
        owned(true, vec![Seg::Move(Z, hd), Seg::Line(d, H), Seg::Line(W, H), Seg::Line(wd, hd), Seg::Line(Z, hd)]),
        owned(true, vec![Seg::Move(wd, Z), Seg::Line(W, d), Seg::Line(W, H), Seg::Line(wd, hd), Seg::Line(wd, Z)]),
    ]
}

#[cfg(test)]
mod tests {
    use super::super::group_master::Factor;
    use super::*;

    /// A point of a set at the default size of a box of `w` by `h` inches.
    fn at(p: &Point, w: f64, h: f64) -> (f64, f64) {
        (p.x.value(w, h), p.y.value(w, h))
    }

    #[test]
    fn the_sides_are_the_four_middles_and_grid3_has_three_on_each_side_facing_outward() {
        let middles: Vec<(f64, f64)> = SIDES.iter().map(|p| at(p, 1.0, 1.0)).collect();
        assert_eq!(middles, [(0.5, 1.0), (0.0, 0.5), (0.5, 0.0), (1.0, 0.5)]);
        assert_eq!(TIPS4.iter().map(|p| at(p, 1.0, 1.0)).collect::<Vec<_>>(), middles);
        let mut per_side = [0; 4];
        for p in &GRID3 {
            let (x, y) = at(p, 1.0, 1.0);
            let side = match (p.dx, p.dy) {
                (dx, dy) if dx == 0.0 && dy == 1.0 => 0,
                (dx, dy) if dx == -1.0 && dy == 0.0 => 1,
                (dx, dy) if dx == 0.0 && dy == -1.0 => 2,
                _ => 3,
            };
            per_side[side] += 1;
            // On the side it faces, at a quarter, a half or three quarters.
            let (on, along) = match side {
                0 => (y == 1.0, x),
                1 => (x == 0.0, y),
                2 => (y == 0.0, x),
                _ => (x == 1.0, y),
            };
            assert!(on && [0.25, 0.5, 0.75].contains(&along), "{p:?}");
        }
        assert_eq!(per_side, [3, 3, 3, 3]);
        let mut seen: Vec<String> = GRID3.iter().map(|p| format!("{:?}", at(p, 1.0, 1.0))).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 12, "twelve different places");
    }

    #[test]
    fn the_ring_is_on_the_ellipse_and_every_point_leaves_along_an_axis_away_from_the_centre() {
        assert_eq!(RING8.len(), 8);
        for (k, p) in RING8.iter().enumerate() {
            let (x, y) = at(p, 1.0, 1.0);
            let (rx, ry) = ring_point(k, 8);
            assert!((x - rx).abs() < 1e-9 && (y - ry).abs() < 1e-9, "point {k}: ({x}, {y}) against ({rx}, {ry})");
            assert!(((x - 0.5).powi(2) + (y - 0.5).powi(2) - 0.25).abs() < 1e-9, "point {k} is on the ellipse");
            assert_eq!(p.dx.abs() + p.dy.abs(), 1.0, "point {k} leaves along an axis");
            assert!((x - 0.5) * p.dx + (y - 0.5) * p.dy >= -1e-9, "point {k} faces away from the centre");
        }
        // On a box that is not a square the points are shares of it.
        let (x, y) = at(&RING8[2], 2.0, 1.0);
        assert_eq!((x, y), (0.0, 0.5));
    }

    #[test]
    fn ring_point_is_total_and_stays_in_the_box() {
        assert_eq!(ring_point(0, 0), (0.5, 1.0));
        assert_eq!(ring_point(5, 0), (0.5, 1.0));
        for n in 1..=64 {
            for k in 0..n {
                let (x, y) = ring_point(k, n);
                assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y), "{k}/{n}");
                assert!(((x - 0.5).powi(2) + (y - 0.5).powi(2) - 0.25).abs() < 1e-9, "{k}/{n} is on the unit ellipse");
            }
        }
        assert_eq!(ring_point(8, 8), ring_point(0, 8), "a k past n goes round again");
        assert_eq!(ring_point(0, 4), (0.5, 1.0));
        let left = ring_point(1, 4);
        assert!((left.0).abs() < 1e-9 && (left.1 - 0.5).abs() < 1e-9, "a quarter turn is the left middle: {left:?}");
    }

    #[test]
    fn the_bar_has_five_on_each_long_side_and_one_at_each_end() {
        let all: Vec<(f64, f64)> = BAR.iter().map(|p| at(p, 6.0, 0.1)).collect();
        assert_eq!(all.iter().filter(|(_, y)| (*y - 0.1).abs() < 1e-12).count(), 5, "top");
        assert_eq!(all.iter().filter(|(_, y)| *y == 0.0).count(), 5, "bottom");
        assert_eq!(all.iter().filter(|(x, _)| *x == 0.0).count(), 1, "left end");
        assert_eq!(all.iter().filter(|(x, _)| *x == 6.0).count(), 1, "right end");
        for p in &BAR {
            let (x, y) = at(p, 6.0, 0.1);
            assert!((0.0..=6.0).contains(&x) && (-1e-12..=0.1 + 1e-12).contains(&y));
            assert_eq!(p.dx.abs() + p.dy.abs(), 1.0);
        }
    }

    #[test]
    fn the_stem_has_a_pair_at_each_even_step_down_the_middle_and_the_array_agrees_with_the_list() {
        let list = stem_points(4);
        assert_eq!(list.len(), 8);
        let ys: Vec<f64> = list.iter().step_by(2).map(|p| at(p, 1.0, 1.0).1).collect();
        assert_eq!(ys.iter().map(|y| (y * 10.0).round() / 10.0).collect::<Vec<_>>(), [0.8, 0.6, 0.4, 0.2]);
        for pair in list.chunks(2) {
            assert_eq!((pair[0].dx, pair[1].dx), (-1.0, 1.0), "one faces left and one right");
            assert_eq!(at(&pair[0], 2.0, 3.0), at(&pair[1], 2.0, 3.0), "on the same place of the stem");
            assert_eq!(at(&pair[0], 2.0, 3.0).0, 1.0, "down the middle");
        }
        let array = stem_array::<8>();
        for (a, b) in list.iter().zip(array.iter()) {
            assert_eq!((at(a, 1.0, 1.0), a.dx, a.dy), (at(b, 1.0, 1.0), b.dx, b.dy));
        }
        assert_eq!(stem_points(0).len(), 0);
        assert_eq!(stem_points(1_000).len(), MAX_STEMS * 2, "capped");
        assert_eq!(stem_array::<40>().len(), 40);
        // The share is in (0, 1) and grows down the stem as the height falls.
        for n in 1..=MAX_STEMS {
            let steps = stem_points(n);
            let shares: Vec<f64> = steps.iter().step_by(2).map(|p| at(p, 1.0, 1.0).1).collect();
            assert!(shares.iter().all(|y| *y > 0.0 && *y < 1.0));
            assert!(shares.windows(2).all(|w| w[0] > w[1]), "each step is lower than the one before: {shares:?}");
        }
    }

    #[test]
    fn the_label_box_is_below_the_figure_and_wider_than_it() {
        let b = LABEL_BELOW;
        assert!(b.y + b.h <= 0.0, "it ends at or below the figure's bottom");
        assert!(b.x < 0.0 && b.x + b.w > 1.0, "it is wider than the figure, centred");
        assert!((b.x + b.w / 2.0 - 0.5).abs() < 1e-12);
    }

    #[test]
    fn a_corner_radius_gives_way_on_a_small_box_and_a_coordinate_with_a_length_writes_it() {
        // The value is the one at the master's own scale, whatever factor the formula names.
        let corner = fr(0.15, 1.0);
        assert!((corner.value(1.6, 1.2) - 0.15).abs() < 1e-12);
        assert!((corner.value(0.4, 0.4) - 0.1).abs() < 1e-12, "a quarter of the shorter side when that is less");
        assert!((W.plus(fk(-0.2)).value(1.0, 1.0) - 0.8).abs() < 1e-12);
        assert!((fk(0.2).value(5.0, 5.0) - 0.2).abs() < 1e-12);
        // A fixed length and a corner radius are written times the factor of the page's drop scale: a shape reads its own, a member of a group the group's. A share of the box is not.
        let own = "IF(DropOnPageScale>0,DropOnPageScale,1)";
        let group = "IF(Sheet.5!DropOnPageScale>0,Sheet.5!DropOnPageScale,1)";
        assert_eq!(corner.formula(Factor::Shape), Some(format!("MIN(0.15*{own},MIN(Width,Height)*0.25)")));
        assert_eq!(W.plus(fr(0.15, -1.0)).formula(Factor::Shape), Some(format!("Width-MIN(0.15*{own},MIN(Width,Height)*0.25)")));
        assert_eq!(fr(0.15, -0.5).formula(Factor::Shape), Some(format!("-0.5*MIN(0.15*{own},MIN(Width,Height)*0.25)")));
        assert_eq!(W.plus(fk(-0.2)).formula(Factor::Shape), Some(format!("Width-0.2*{own}")));
        assert_eq!(W.plus(fk(-0.2)).formula(Factor::Group), Some(format!("Width-0.2*{group}")));
        assert_eq!(fk(0.2).formula(Factor::Shape), Some(format!("0.2*{own}")), "a fixed length alone is a length times the factor");
        assert_eq!(fk(-0.1).formula(Factor::Group), Some(format!("-0.1*{group}")));
        assert_eq!(fk(0.0).formula(Factor::Shape), None, "no length is the constant 0");
        assert_eq!(mix(0.5, -0.12).formula(Factor::Shape).as_deref(), Some("Width*0.5-Height*0.12"), "a share of the box follows the box");
        assert_eq!(fw(0.0).formula(Factor::Group), None);
        // The cell holds the value at the master's own scale.
        assert_eq!(fk(0.2).cell(1.0, 1.0, Factor::Shape).value.as_ref().and_then(|v| v.number()), Some(0.2));
        assert_eq!(fw(0.0).cell(1.0, 1.0, Factor::Shape).formula.text(), None, "0 is a plain constant");
    }
}
