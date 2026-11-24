//! Test-only: the adapter (`adapter.rs`, through `craft-snap`) held equal to the snapping it replaced (`oracle.rs`) on seeded random pages (commit a).
//!
//! **What "equal" means**, written before the first run. For one query the landing point (`dx`, `dy`), every hit (target, axis, anchor, shape, point, the line it
//! lies on, the place, the span and the gap), the glue, the holds and the truncated flag are equal within a billionth of a unit (relative far from the origin).
//! Seven things are counted apart, and none is compared as a value:
//!
//! * **ties**: where the old function offered several targets of the same rank at the same distance (within a billionth), it kept the first it was given; the
//!   library keeps the lowest id. The result is then required to be *one of the tied offers* (the old function logs every offer, `oracle::Trace`);
//! * **cut**: a query that met a cap (more shapes near than a query looks into) is only required to say `truncated` in both;
//! * **row mode**: while `Policy::TODAY.spacing_row` is not the library's exact reading of today's rule, a result that holds a spacing hit in either is counted and not compared;
//! * **today breaks the invariant**: the library re-reads an equal-gap place where the subject lands, and draws no gap of nothing. A case where the two
//!   differ is in this class only when **today's result fails the invariant** (its claim of an equal gap is not true where the shape lands, or its hit has no
//!   mark: a gap of 0) **and the library's passes it**;
//! * **a flip** (a resize): today's `snap` returns a hit that, applied, leaves the box with no width or height, and its only caller (`snap_resize`) then drops the
//!   whole snap; the library never offers such a candidate and takes the next. A case is in this class only when **today's result applied leaves a box of no size**,
//!   the library's box has a size, and the axis today's result did not flip is equal;
//! * **a band** (a box with places of its own): the engine lined a box up with the shapes within the larger of two inches and a pull of the box *and of the
//!   places of the shapes that move with it*; the library measures the reach from the box, and the adapter grows it by how far those places lie outside the
//!   box, so a shape in the thin band between the two windows is a target of the library only. A case is in this class only when the shape the library lined up with
//!   lies **outside the old window** and every other axis is equal;
//! * **tolerance**: the library's `Coincide` against today's billionth. A case where the two differ is in this class only when they agree again at all eight
//!   offsets of the subject by 2e-8 (so the difference is the width of a tolerance and no more).
//!
//! The pages of the review's L9 (mirrored about a line, exact reaches, boxes that end at one place, and all of them with turned shapes) add classes, each folded out of the two
//! results by a function of its own with its conditions tested on a case made by hand:
//!
//! * **anchor ties**: one place reached by two anchors of the subject at one distance (the old function took the first, the library the other);
//! * **naming ties**: the same line or gap, the same landing, named by another of the shapes that have it;
//! * **at rest**: both leave the box where it is on an axis and tell different things about it;
//! * **the edge of a reach**: a hit exactly a pull away, which a division's last bit decides;
//! * **the width of a tolerance on one axis**: a landing under 3e-8 inch is a hit of nothing;
//! * **recorded, not explained** (held to a number by the test that meets it): *unmarked*, a spacing hit the library tells with no gap and draws no gap marks for, under the
//!   harness's policy, where its neighbours have left the row at the landing and another pair holds the same centre
//!   (`a_spacing_hit_whose_neighbours_left_the_row_is_told_without_its_gap_while_another_pair_holds_the_same_centre`). Two others of the earlier pin are gone: the marks of two spacing hits at
//!   once were those of the first axis only, and a long pull today makes beside a hit at rest was not made by the library (`a_pull_beside_a_hit_at_rest_is_made_by_both`), so
//!   either now is a difference and fails the harness.
//!
//! The tolerance is [`Eps`]: relative near the origin, a few steps of the numbers far from it (it was a billionth of the page's distance, scaled by the number compared again).
//!
//! The counters of work (`examined`, `visited`, `candidates`, ...) are the library's own and are not compared here; the tests that hold a cost to a number
//! are the integration tests'.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_doc::Document;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

use super::adapter::{Policy, snap_with};
use super::guides::add_guide;
use super::oracle::{self, Offer, TRACE};
use super::{
    Anchor, Axis, Edges, Hit, MAX_OWN_POINTS, P, Query, Rect, STRENGTH_MAX, SnapPage, SnapSettings, Snapped, Strength, Subject, Target, Use,
};
use crate::draw::{Turn, drop_shape, rotate_shapes};
use crate::stencil::find_master;

/// A seeded generator: no clock, no entropy.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
    fn between(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.next()
    }
    fn below(&mut self, n: usize) -> usize {
        ((self.next() * n as f64) as usize).min(n.saturating_sub(1))
    }
    fn chance(&mut self, p: f64) -> bool {
        self.next() < p
    }
}

/// What a class of pages looks like.
#[derive(Clone, Copy)]
struct Config {
    /// Coordinates are multiples of an eighth: exact ties and exact reaches.
    lattice: bool,
    /// Every coordinate is moved by this much (far drawings).
    offset: f64,
    /// Shapes turned by other than quarter turns, and flipped ones.
    turned: bool,
    /// Instances of the stencil's rectangle and ellipse (connection points and outlines).
    masters: bool,
    /// Zoom, in screen points to a paper inch.
    zooms: &'static [f64],
    /// Shapes in a row beside the subject (equal spacing).
    rows: bool,
    /// The page is symmetric about the vertical line through its middle (each shape has a mirror image there, turned the other way and flipped the other way), and
    /// the subject is put on that line or a step off it: everything that is to the left of it has an equal to the right (L9).
    mirrored: bool,
    /// Coordinates are multiples of a sixty-fourth of an inch, the zooms are powers of two, and the subject is put exactly the pull of a kind of target from a line or a
    /// place: the distance is the reach, to the bit (L9).
    exact: bool,
    /// Coordinates are multiples of half an inch and the sizes half an inch, an inch and one and a half: many ends at one place and many equal gaps (L9).
    coarse: bool,
}

const ZOOMS: &[f64] = &[96.0, 96.0, 48.0, 192.0, 768.0, 20.0, 5.5, 3.0];

/// The zooms of the pages of exact reaches: 6, 8 and 12 points over each are multiples of a sixty-fourth of an inch.
const ZOOMS_EXACT: &[f64] = &[32.0, 64.0, 128.0];

/// What the classes of a run came to.
#[derive(Default, Debug)]
struct Counts {
    cases: usize,
    equal: usize,
    ties: usize,
    cut: usize,
    row_mode: usize,
    today_breaks: usize,
    tolerance: usize,
    flips: usize,
    bands: usize,
    /// Every difference there was, and the first of them, told.
    differences: usize,
    anchor_ties: usize,
    name_ties: usize,
    at_rest: usize,
    edges_of_reach: usize,
    /// A spacing hit the library tells with no gap and no gap mark, where today's claim does not hold where the box lands either (the other axis moved it out of the row).
    unmarked: usize,
    places: usize,
    aligned: usize,
    spaced: usize,
    held: usize,
    glued: usize,
    resized: usize,
    differ: Vec<String>,
}

/// The strengths the harness's cases take unless a case is given others: **the pulls its classes of difference were worked out at**, which were the engine's until a later change
/// (points 12, guides 8, geometry 8 and the alignment 6 screen points; the grid and the ruler from any distance, here the largest pull a strength has, 999 pixels). They are test
/// data: the harness holds the library to the old function at whatever the pulls are, and the engine's own defaults (the spec's) are the commands' tests'.
fn calibrated() -> Strength {
    Strength { ruler: STRENGTH_MAX, grid: STRENGTH_MAX, guides: 8, points: 12, geometry: 8, extensions: 6 }
}

struct Case {
    doc: Document,
    subject: Subject,
    using: Use,
    moving: Vec<u32>,
    own: Vec<P>,
    settings: SnapSettings,
    zoom: f64,
}

/// The step the coordinates of a page are multiples of, if they are.
fn step_of(c: &Config) -> Option<f64> {
    if c.exact {
        Some(1.0 / 64.0)
    } else if c.coarse {
        Some(0.5)
    } else if c.lattice {
        Some(0.125)
    } else {
        None
    }
}

fn rounded(c: &Config, v: f64) -> f64 {
    step_of(c).map_or(v, |step| (v / step).round() * step)
}

fn page_of(r: &mut Rng, c: &Config) -> (Document, Vec<Rect>) {
    let off = c.offset;
    let mut page = PageBuilder::new().cell("PageWidth", "16").cell("PageHeight", "16");
    if r.chance(0.05) {
        page = page.cell("InhibitSnap", "1");
    }
    let mut boxes: Vec<Rect> = Vec::new();
    let n = if c.rows { 2 + r.below(5) } else { r.below(11) };
    // Each shape: where its pin is, its size, its angle and whether it is flipped.
    let mut specs: Vec<(f64, f64, f64, f64, f64, bool)> = Vec::new();
    for _ in 0..n {
        let (w, h) = (rounded(c, r.between(0.3, 3.0)).max(0.125), rounded(c, r.between(0.3, 3.0)).max(0.125));
        let (x, y) = if c.rows {
            (rounded(c, r.between(0.0, 14.0)), rounded(c, r.between(-0.4, 1.6)))
        } else {
            (rounded(c, r.between(0.5, 14.5)), rounded(c, r.between(0.5, 14.5)))
        };
        let angle = if c.turned && r.chance(0.35) {
            r.between(0.0, std::f64::consts::TAU)
        } else if r.chance(0.12) {
            std::f64::consts::FRAC_PI_2 * (1 + r.below(3)) as f64
        } else {
            0.0
        };
        let flip = c.turned && r.chance(0.2);
        specs.push((x, y, w, h, angle, flip));
    }
    // A mirrored page: the image of each shape in the vertical line x = 8 (the page is 16 inches wide), turned the other way (and flipped as it is: a box is the same either way).
    if c.mirrored {
        let images: Vec<_> = specs.iter().map(|(x, y, w, h, angle, flip)| (16.0 - x, *y, *w, *h, -angle, *flip)).collect();
        specs.extend(images);
    }
    for (i, (x, y, w, h, angle, flip)) in specs.into_iter().enumerate() {
        let mut shape = ShapeBuilder::new(i as u32 + 1)
            .cell("PinX", &(x + off).to_string())
            .cell("PinY", &(y + off).to_string())
            .cell("Width", &w.to_string())
            .cell("Height", &h.to_string())
            .cell("LocPinX", &(w / 2.0).to_string())
            .cell("LocPinY", &(h / 2.0).to_string())
            .cell("Angle", &angle.to_string());
        if flip {
            shape = shape.cell("FlipX", "1");
        }
        page = page.shape(shape);
        let (s, co) = angle.sin_cos();
        let (hw, hh) = ((w * co.abs() + h * s.abs()) / 2.0, (w * s.abs() + h * co.abs()) / 2.0);
        boxes.push(Rect { x0: x + off - hw, y0: y + off - hh, x1: x + off + hw, y1: y + off + hh });
    }
    let mut doc = DocBuilder::new().page(1, page).build();
    if c.masters {
        for name in ["rectangle", "rectangle", "ellipse"] {
            if r.chance(0.6)
                && let Some(spec) = find_master(name)
            {
                let (x, y) = (r.between(1.0, 14.0) + off, r.between(1.0, 14.0) + off);
                if let Ok(info) = drop_shape(&mut doc, 1, spec, x, y) {
                    // A turned page turns its stencil shapes as well, which draw paths: under today's policy the box is round their corners, as the old function had it
                    // (row 14 is off in `Policy::TODAY`, and a turned ellipse is where the two boxes differ).
                    let angle = if c.turned && r.chance(0.5) { r.between(0.0, std::f64::consts::TAU) } else { 0.0 };
                    if angle != 0.0 {
                        let _ = rotate_shapes(&mut doc, 1, &[info.id], Turn::To(angle));
                    }
                    let (s, co) = angle.sin_cos();
                    let (hw, hh) = ((info.width * co.abs() + info.height * s.abs()) / 2.0, (info.width * s.abs() + info.height * co.abs()) / 2.0);
                    boxes.push(Rect { x0: x - hw, y0: y - hh, x1: x + hw, y1: y + hh });
                }
            }
        }
    }
    for _ in 0..r.below(4) {
        let b = boxes.get(r.below(boxes.len().max(1))).copied().unwrap_or(Rect { x0: 4.0, y0: 4.0, x1: 5.0, y1: 5.0 });
        let at = [b.x0, (b.x0 + b.x1) / 2.0, b.x1, b.y0, (b.y0 + b.y1) / 2.0, b.y1][r.below(6)] + r.between(-0.12, 0.12);
        let _ = add_guide(&mut doc, 1, if r.chance(0.5) { Axis::X } else { Axis::Y }, rounded(c, at), 0);
    }
    (doc, boxes)
}

fn settings_of(r: &mut Rng) -> SnapSettings {
    let mut s = SnapSettings::none();
    s.snap = r.chance(0.95);
    s.glue = r.chance(0.8);
    s.ruler = r.chance(0.3);
    s.grid = r.chance(0.4);
    s.guides = r.chance(0.8);
    s.snap_handles = r.chance(0.5);
    s.snap_vertices = r.chance(0.5);
    s.snap_connection_points = r.chance(0.5);
    s.snap_geometry = r.chance(0.4);
    s.alignment_box = r.chance(0.5);
    s.intersections = r.chance(0.4);
    s.glue_guides = r.chance(0.7);
    s.glue_handles = r.chance(0.5);
    s.glue_vertices = r.chance(0.5);
    s.glue_connection_points = r.chance(0.6);
    s.glue_geometry = r.chance(0.4);
    s.dynamic_grid = r.chance(0.6);
    s
}

fn case_of(r: &mut Rng, c: &Config) -> Case {
    let (doc, boxes) = page_of(r, c);
    let zoom = c.zooms.get(r.below(c.zooms.len())).copied().unwrap_or(96.0);
    let reach = 6.0 / zoom;
    let ids: Vec<u32> = (1..=boxes.len() as u32).collect();
    let moving: Vec<u32> = ids.iter().copied().filter(|_| r.chance(0.12)).collect();
    // A place near a feature: a shape's edge, middle or corner, plus a distance of about a pull.
    let b = boxes.get(r.below(boxes.len().max(1))).copied().unwrap_or(Rect { x0: 4.0, y0: 4.0, x1: 5.0, y1: 5.0 });
    let (fx, fy) = ([b.x0, (b.x0 + b.x1) / 2.0, b.x1][r.below(3)], [b.y0, (b.y0 + b.y1) / 2.0, b.y1][r.below(3)]);
    // The distance of the subject from a feature: about a pull, and on a page of exact reaches (L9) often exactly the pull of a kind of target (6, 8 or 12 points over a
    // power of two), either way.
    let noise = |r: &mut Rng| {
        if c.exact && r.chance(0.6) {
            [6.0, 8.0, 12.0][r.below(3)] / zoom * if r.chance(0.5) { 1.0 } else { -1.0 }
        } else if r.chance(0.2) {
            r.between(-4.0, 4.0) * reach
        } else {
            r.between(-1.4, 1.4) * reach
        }
    };
    let kind = r.next();
    let (w, h) = (rounded(c, r.between(0.3, 2.5)).max(0.125), rounded(c, r.between(0.3, 2.5)).max(0.125));
    let (anchor_x, anchor_y) = ([0.0, w / 2.0, w][r.below(3)], [0.0, h / 2.0, h][r.below(3)]);
    let (x0, y0) = if c.rows {
        // Near a place that repeats a gap of the row: beyond a shape at the gap it keeps to the one before it, or before it at that gap.
        let mut by_x: Vec<Rect> = boxes.clone();
        by_x.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        let i = r.below(by_x.len().saturating_sub(1).max(1));
        let x = match (by_x.get(i), by_x.get(i + 1)) {
            (Some(a), Some(b)) if b.x0 > a.x1 && r.chance(0.7) => {
                if r.chance(0.5) {
                    b.x1 + (b.x0 - a.x1) + noise(r)
                } else {
                    a.x0 - (b.x0 - a.x1) - w + noise(r)
                }
            }
            _ => r.between(0.0, 14.0),
        };
        (rounded(c, x), rounded(c, r.between(-0.2, 0.6)))
    } else {
        (rounded(c, fx - anchor_x + noise(r)), rounded(c, fy - anchor_y + noise(r)))
    };
    // A mirrored page: the subject is put on the page's line of symmetry (its middle at x = 8) or a step off it, so that what lies to its left has an equal to its right.
    let x0 = match (c.mirrored, step_of(c)) {
        (true, step) if r.chance(0.6) => rounded(c, 8.0 - w / 2.0 + [0.0, 1.0, -1.0][r.below(3)] * step.unwrap_or(0.0)),
        _ => x0,
    };
    let rect = Rect { x0, y0, x1: x0 + w, y1: y0 + h };
    let mut using = Use::Place;
    let subject = if kind < 0.25 {
        if r.chance(0.4) {
            using = Use::Connect;
        }
        Subject::Point((rounded(c, fx + noise(r)), rounded(c, fy + noise(r))))
    } else if kind < 0.75 {
        if r.chance(0.08) {
            using = Use::Connect;
        }
        Subject::Box(rect)
    } else {
        let mut e = Edges { left: r.chance(0.5), bottom: r.chance(0.5), right: r.chance(0.5), top: r.chance(0.5) };
        if !(e.left || e.right || e.top || e.bottom) {
            e.right = true;
        }
        Subject::Edges(rect, e)
    };
    let own: Vec<P> = if matches!(subject, Subject::Box(_)) {
        (0..r.below(4).min(MAX_OWN_POINTS)).map(|_| (rect.x0 + r.between(-0.2, 1.2) * w, rect.y0 + r.between(-0.2, 1.2) * h)).collect()
    } else {
        Vec::new()
    };
    // The strengths are the settings': most cases take the defaults, and some a spread (a wide grid and a narrow ruler among them, since the lattices pull from their own reach now).
    let strength = if r.chance(0.15) { Strength { points: 5, guides: 10, geometry: 4, extensions: 9, ruler: 3, grid: 30 } } else { calibrated() };
    Case { doc, subject, using, moving, own, settings: SnapSettings { strength, ..settings_of(r) }, zoom }
}

/// How close two numbers must be to be equal. **Near the origin** a number is compared to a billionth of itself (a billionth of an inch for those under an inch). **Far from
/// it** the numbers are known to a few steps of themselves: a step of 1e7 inches is 1.9e-9 inch, so the tolerance is a few of those, in inches, for the lines and the
/// distances alike. (It was a billionth of the page's distance, scaled again by the number compared: at 1e7 it accepted a landing 0.01 inch off, and a line 1e5 inches off.)
#[derive(Clone, Copy, Debug, PartialEq)]
struct Eps {
    /// A fraction of the larger number compared, or of 1 when both are smaller.
    rel: f64,
    /// An absolute number of inches.
    abs: f64,
}

impl Eps {
    const NEAR: Eps = Eps { rel: 1.0e-9, abs: 0.0 };

    /// The tolerance on a page whose shapes are `offset` inches from the origin (0 for a page about it).
    fn at(offset: f64) -> Eps {
        if offset > 0.0 { Eps { rel: 0.0, abs: 1.0e-9 + 16.0 * f64::EPSILON * offset } } else { Eps::NEAR }
    }

    /// How far apart `a` and `b` may be.
    fn limit(&self, a: f64, b: f64) -> f64 {
        self.abs.max(self.rel * 1.0_f64.max(a.abs()).max(b.abs()))
    }
}

fn close(a: f64, b: f64, eps: Eps) -> bool {
    (a - b).abs() <= eps.limit(a, b)
}

fn close_pair(a: Option<(f64, f64)>, b: Option<(f64, f64)>, eps: Eps) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => close(a.0, b.0, eps) && close(a.1, b.1, eps),
        _ => false,
    }
}

fn same_hit(a: &Hit, b: &Hit, eps: Eps) -> bool {
    a.target == b.target
        && a.axis == b.axis
        && a.anchor == b.anchor
        && a.shape == b.shape
        && a.point == b.point
        && close(a.line, b.line, eps)
        && close_pair(a.at, b.at, eps)
        && close_pair(a.span, b.span, eps)
        && match (a.gap, b.gap) {
            (None, None) => true,
            (Some(x), Some(y)) => close(x, y, eps),
            _ => false,
        }
}

#[derive(Debug)]
enum Class {
    Equal,
    Tie,
    Cut,
    RowMode,
    TodayBreaks,
    Tolerance,
    Flip,
    Band,
    /// The same place from two anchors of the subject at one distance (L9).
    AnchorTie,
    /// The same line and the same landing, named by another of the shapes that have the line (L9).
    NameTie,
    /// Both leave the box where it is on an axis, and tell different things about it (L9).
    AtRest,
    /// The old result's hit lies exactly a pull from the subject and the library's does not (L9).
    EdgeOfReach,
    /// The recorded difference of the library that is left (L9): see [`fold_unmarked`].
    Unmarked,
}

fn has_spacing(s: &Snapped) -> bool {
    s.hits.iter().any(|h| h.target == Target::Spacing)
}

/// The offers of the old function that were as good as the best one, within `eps`: the results it might as well have given.
fn tied(log: &[Offer], eps: Eps) -> Vec<&Offer> {
    let Some(best) = log.iter().min_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1))) else { return Vec::new() };
    log.iter().filter(|o| o.0 == best.0 && o.1 <= best.1 + eps.limit(best.1, 0.0)).collect()
}

fn judge(old: &Snapped, new: &Snapped, eps: Eps, exact_rows: bool) -> Result<Class, String> {
    if !exact_rows && (has_spacing(old) || has_spacing(new)) {
        return Ok(Class::RowMode);
    }
    compare(old, new, eps)
}

fn compare(old: &Snapped, new: &Snapped, eps: Eps) -> Result<Class, String> {
    if old.truncated || new.truncated {
        return if old.truncated == new.truncated { Ok(Class::Cut) } else { Err(format!("truncated: old {} new {}", old.truncated, new.truncated)) };
    }
    let mut was_tie = false;
    let place_old = old.hits.iter().find(|h| h.axis.is_none());
    let place_new = new.hits.iter().find(|h| h.axis.is_none());
    match (place_old, place_new) {
        (None, None) => {}
        (Some(_), None) | (None, Some(_)) => return Err(format!("a place on one side only: old {:?} new {:?}", old.hits, new.hits)),
        (Some(o), Some(n)) => {
            if !(old.hits.len() == 1 && new.hits.len() == 1) {
                return Err("a place with other hits".into());
            }
            let delta_ok = close(old.dx, new.dx, eps) && close(old.dy, new.dy, eps);
            if same_hit(o, n, eps) && delta_ok {
                if old.glue != new.glue {
                    return Err(format!("glue: old {:?} new {:?}", old.glue, new.glue));
                }
            } else {
                let ok = TRACE.with(|t| {
                    tied(&t.borrow().place, eps).iter().any(|m| same_hit(&m.3, n, eps) && close(m.2.0, new.dx, eps) && close(m.2.1, new.dy, eps))
                });
                if !ok {
                    return Err(format!("place: old {o:?} ({}, {}) new {n:?} ({}, {})", old.dx, old.dy, new.dx, new.dy));
                }
                was_tie = true;
            }
            return Ok(if was_tie { Class::Tie } else { Class::Equal });
        }
    }
    for (i, axis) in [Axis::X, Axis::Y].into_iter().enumerate() {
        let (o, n) = (old.hits.iter().find(|h| h.axis == Some(axis)), new.hits.iter().find(|h| h.axis == Some(axis)));
        let (od, nd) = if axis == Axis::X { (old.dx, new.dx) } else { (old.dy, new.dy) };
        match (o, n) {
            (None, None) => {
                if !(close(od, 0.0, eps) && close(nd, 0.0, eps)) {
                    return Err(format!("{axis:?}: no hit but a move {od} {nd}"));
                }
            }
            (Some(o), Some(n)) => {
                if same_hit(o, n, eps) && close(od, nd, eps) {
                    continue;
                }
                let ok = TRACE.with(|t| {
                    tied(&t.borrow().axes[i], eps)
                        .iter()
                        .any(|m| same_hit(&m.3, n, eps) && close(if axis == Axis::X { m.2.0 } else { m.2.1 }, nd, eps))
                });
                if !ok {
                    return Err(format!("{axis:?}: old {o:?} d {od}; new {n:?} d {nd}"));
                }
                was_tie = true;
            }
            (o, n) => return Err(format!("{axis:?}: a hit on one side only: old {o:?} new {n:?}")),
        }
    }
    if !was_tie {
        let same_holds = old.holds.len() == new.holds.len()
            && old
                .holds
                .iter()
                .zip(&new.holds)
                .all(|(a, b)| a.guide == b.guide && a.axis == b.axis && a.anchor == b.anchor && close(a.line, b.line, eps));
        if !same_holds {
            return Err(format!("holds: old {:?} new {:?}", old.holds, new.holds));
        }
        if old.glue != new.glue {
            return Err(format!("glue: old {:?} new {:?}", old.glue, new.glue));
        }
    }
    Ok(if was_tie { Class::Tie } else { Class::Equal })
}

/// The extent of a box on an axis.
fn along(r: &Rect, axis: Axis) -> (f64, f64) {
    if axis == Axis::X { (r.x0, r.x1) } else { (r.y0, r.y1) }
}

/// A box of a row: its `ID`, its extent along the row and across it.
type Row = (u32, (f64, f64), (f64, f64));

/// Whether the equal-gap claim of a spacing hit is true where the subject lands, as the library reads it there: the shape the hit names is still the nearest
/// box on its side, the gap to it is the gap the hit says, and a gap equal to it can be drawn: the subject's gap on its other side, or the gap between that
/// neighbour and the box beyond it, with both in the reach of the row where the subject lands (a gap the row no longer holds has no mark).
fn claim_holds(page: &SnapPage, case: &Case, s: &Snapped, hit: &Hit, tolerance: Eps) -> bool {
    let (Subject::Box(r), Some(axis), Some(neighbour), Some(gap)) = (case.subject, hit.axis, hit.shape, hit.gap) else { return false };
    let landed = Rect { x0: r.x0 + s.dx, y0: r.y0 + s.dy, x1: r.x1 + s.dx, y1: r.y1 + s.dy };
    let across_axis = if axis == Axis::X { Axis::Y } else { Axis::X };
    let ((lo, hi), (c0, c1)) = (along(&landed, axis), along(&landed, across_axis));
    let overlaps = |a: (f64, f64), b: (f64, f64)| a.0 < b.1 && b.0 < a.1;
    // A billionth of an inch, and on a far page the few steps of its numbers.
    let eps = tolerance.abs.max(1.0e-9);
    let reach = (4.0 * (hi - lo)).max(super::SPACING_REACH);
    let boxes: Vec<Row> = page
        .items_for_test()
        .iter()
        .filter(|i| !case.moving.contains(&i.id))
        .map(|i| (i.id, along(&i.bbox, axis), along(&i.bbox, across_axis)))
        .filter(|b| b.1.1 >= lo - reach - eps && b.1.0 <= hi + reach + eps)
        .collect();
    let meets_strip = |b: &Row| b.2.1 >= c0 - eps && b.2.0 <= c1 + eps;
    let across: Vec<_> = boxes.iter().filter(|b| overlaps(b.2, (c0, c1))).collect();
    let low = across.iter().filter(|b| b.1.1 <= lo + eps).max_by(|a, b| a.1.1.total_cmp(&b.1.1));
    let high = across.iter().filter(|b| b.1.0 >= hi - eps).min_by(|a, b| a.1.0.total_cmp(&b.1.0));
    let (gap_low, gap_high) = (low.map(|b| lo - b.1.1), high.map(|b| b.1.0 - hi));
    let near = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() <= eps);
    let named = page.items_for_test().iter().find(|i| i.id == neighbour).map(|i| along(&i.bbox, axis));
    let is_low = low.is_some_and(|b| b.0 == neighbour || named.is_some_and(|n| (b.1.1 - n.1).abs() <= eps));
    let is_high = high.is_some_and(|b| b.0 == neighbour || named.is_some_and(|n| (b.1.0 - n.0).abs() <= eps));
    let mine = (is_low && near(gap_low, gap)) || (is_high && near(gap_high, gap));
    let other = near(gap_low, gap) && near(gap_high, gap);
    let beyond = |n: Option<&&Row>, before: bool| {
        n.is_some_and(|n| {
            boxes.iter().any(|b| {
                b.0 != n.0
                    && meets_strip(b)
                    && overlaps(b.2, n.2)
                    && if before {
                        b.1.1 <= n.1.0 + eps && ((n.1.0 - b.1.1) - gap).abs() <= eps
                    } else {
                        b.1.0 >= n.1.1 - eps && ((b.1.0 - n.1.1) - gap).abs() <= eps
                    }
            })
        })
    };
    mine && (other || beyond(low, true) || beyond(high, false))
}

/// A resize box with a snap applied the way `snap_resize` does.
fn applied(r: &Rect, s: &Snapped) -> Rect {
    let mut a = *r;
    for h in &s.hits {
        match (h.axis, h.anchor) {
            (Some(Axis::X), Anchor::Low) => a.x0 += s.dx,
            (Some(Axis::X), Anchor::High) => a.x1 += s.dx,
            (Some(Axis::Y), Anchor::Low) => a.y0 += s.dy,
            (Some(Axis::Y), Anchor::High) => a.y1 += s.dy,
            _ => {}
        }
    }
    a
}

/// A resize whose snap today flips the box: the library's box has a size, and the axis that did not flip is equal.
fn flip_class(case: &Case, old: &Snapped, new: &Snapped, eps: Eps) -> Option<Result<Class, String>> {
    let Subject::Edges(r, _) = case.subject else { return None };
    let (a, b) = (applied(&r, old), applied(&r, new));
    let flips = [a.x1 - a.x0 <= 0.0, a.y1 - a.y0 <= 0.0];
    if !(flips[0] || flips[1]) {
        return None;
    }
    if b.x1 - b.x0 <= 0.0 || b.y1 - b.y0 <= 0.0 {
        return Some(Err("the library's box has no size either".into()));
    }
    for (i, axis) in [Axis::X, Axis::Y].into_iter().enumerate() {
        if flips[i] {
            continue;
        }
        let (o, n) = (old.hits.iter().find(|h| h.axis == Some(axis)), new.hits.iter().find(|h| h.axis == Some(axis)));
        let (od, nd) = if axis == Axis::X { (old.dx, new.dx) } else { (old.dy, new.dy) };
        let same = match (o, n) {
            (None, None) => true,
            (Some(o), Some(n)) => same_hit(o, n, eps) && close(od, nd, eps),
            _ => false,
        };
        if !same {
            return Some(Err(format!("a flip on one axis, and the other differs: {axis:?}: old {o:?} {od} new {n:?} {nd}")));
        }
    }
    Some(Ok(Class::Flip))
}

/// A box lined up, by the library, with a shape the engine's window did not reach.
fn band_class(page: &SnapPage, case: &Case, old: &Snapped, new: &Snapped, eps: Eps) -> Option<Result<Class, String>> {
    let Subject::Box(r) = case.subject else { return None };
    if case.own.is_empty() {
        return None;
    }
    let s = &case.settings.strength;
    let reach = super::ALIGN_REACH.max(f64::from(s.points.max(s.geometry).max(s.guides)) / case.zoom);
    let mut window = r;
    for p in &case.own {
        window = Rect { x0: window.x0.min(p.0), y0: window.y0.min(p.1), x1: window.x1.max(p.0), y1: window.y1.max(p.1) };
    }
    let window = window.grown(reach);
    let mut any = false;
    for (axis, od, nd) in [(Axis::X, old.dx, new.dx), (Axis::Y, old.dy, new.dy)] {
        let (o, n) = (old.hits.iter().find(|h| h.axis == Some(axis)), new.hits.iter().find(|h| h.axis == Some(axis)));
        let same = match (o, n) {
            (None, None) => true,
            (Some(o), Some(n)) => same_hit(o, n, eps) && close(od, nd, eps),
            _ => false,
        };
        if same {
            continue;
        }
        let outside = n.is_some_and(|n| {
            n.target == Target::Alignment
                && n.shape.and_then(|id| page.items_for_test().iter().find(|i| i.id == id)).is_some_and(|i| !i.bbox.meets(&window))
        });
        if !outside {
            return None;
        }
        any = true;
    }
    any.then_some(Ok(Class::Band))
}

/// The places of the subject that pull: the nine of a box, or its point, and the places of the shapes that move with it.
fn anchors_of(case: &Case) -> Vec<P> {
    let mut places: Vec<P> = case.own.clone();
    match case.subject {
        Subject::Point(p) => places.push(p),
        Subject::Box(r) | Subject::Edges(r, _) => {
            for x in [r.x0, (r.x0 + r.x1) / 2.0, r.x1] {
                for y in [r.y0, (r.y0 + r.y1) / 2.0, r.y1] {
                    places.push((x, y));
                }
            }
        }
    }
    places
}

/// **An anchor tie** (L9): both took one place (the same target, shape and number, at the same spot), and the two deltas are two anchors of the subject coming to it at the
/// same distance (a box and its mirror image about a place on its line of symmetry, or a place a quarter of the way between two of its anchors). The old function took the
/// first anchor it came to, the library the other. Only a single place hit in each is in the class, with the same length of travel, and the library's delta must bring one
/// of the subject's places onto the place it says.
fn anchor_tie(case: &Case, old: &Snapped, new: &Snapped, eps: Eps) -> bool {
    let (Some(o), Some(n)) = (old.hits.iter().find(|h| h.axis.is_none()), new.hits.iter().find(|h| h.axis.is_none())) else { return false };
    let Some(at) = n.at else { return false };
    old.hits.len() == 1
        && new.hits.len() == 1
        && same_hit(o, n, eps)
        && close(old.dx.hypot(old.dy), new.dx.hypot(new.dy), eps)
        && anchors_of(case).iter().any(|q| close(q.0 + new.dx, at.0, eps) && close(q.1 + new.dy, at.1, eps))
}

/// How far, in paper inches, each kind of target pulls at the case's zoom (`None` for a whole number of angle steps, which pull from anywhere).
fn pull_of(case: &Case, target: Target) -> Option<f64> {
    let s = &case.settings.strength;
    let points = match target {
        Target::Guide => s.guides,
        Target::Alignment | Target::PageMargin | Target::Spacing | Target::Size | Target::Angle => s.extensions,
        Target::Outline | Target::Intersection => s.geometry,
        Target::ConnectionPoint | Target::Vertex | Target::Handle => s.points,
        Target::Grid => s.grid,
        Target::Ruler => s.ruler,
        Target::AngleStep => return None,
    };
    Some(f64::from(points) / case.zoom)
}

/// Whether the shape a hit names has what the hit claims, at the box's landing `d` along the hit's axis: for an alignment the line among its edges and middle, for a
/// spacing the gap between it and the box.
fn carries(page: &SnapPage, case: &Case, hit: &Hit, claimed: &Hit, d: f64, eps: Eps) -> bool {
    let (Some(id), Some(axis)) = (hit.shape, hit.axis) else { return false };
    let Some(item) = page.items_for_test().iter().find(|i| i.id == id) else { return false };
    let (a, b) = along(&item.bbox, axis);
    match claimed.target {
        Target::Alignment => [a, (a + b) / 2.0, b].iter().any(|v| close(*v, claimed.line, eps)),
        Target::Spacing => {
            let (Subject::Box(r), Some(gap)) = (case.subject, claimed.gap) else { return false };
            let (lo, hi) = along(&r, axis);
            close(lo + d - b, gap, eps) || close(a - (hi + d), gap, eps)
        }
        _ => false,
    }
}

/// **A naming tie** (L9): on an axis both results have the same target, anchor, line and landing, and name different shapes, each of which has the line (an alignment) or the
/// gap (a spacing) the hit claims: two of the shapes of the page are equal there (the images of one another on a mirrored page, two boxes ending at one place), the old
/// function named the first it met and the library names the lowest `ID`. The names (and for an alignment the span, which follows the shape named) are taken out of both.
fn fold_names(page: &SnapPage, case: &Case, old: &Snapped, new: &Snapped, eps: Eps) -> Option<(Snapped, Snapped)> {
    let (mut o, mut n) = (old.clone(), new.clone());
    let mut folded = false;
    for (axis, od, nd) in [(Axis::X, old.dx, new.dx), (Axis::Y, old.dy, new.dy)] {
        let (Some(i), Some(j)) = (o.hits.iter().position(|h| h.axis == Some(axis)), n.hits.iter().position(|h| h.axis == Some(axis))) else {
            continue;
        };
        let (a, b) = (o.hits[i], n.hits[j]);
        let alike = a.target == b.target && a.anchor == b.anchor && close(a.line, b.line, eps) && close(od, nd, eps);
        if !(alike && a.shape != b.shape && matches!(a.target, Target::Alignment | Target::Spacing)) {
            continue;
        }
        if carries(page, case, &a, &a, od, eps) && carries(page, case, &b, &a, od, eps) {
            for h in [&mut o.hits[i], &mut n.hits[j]] {
                h.shape = None;
                if a.target == Target::Alignment {
                    h.span = None;
                }
            }
            folded = true;
        }
    }
    folded.then_some((o, n))
}

/// **The width of a tolerance, on one axis** (L9): a hit whose landing is under 3e-8 inch (the library's same-place rule is 2e-8, today's a billionth) is a hit of nothing: it
/// is taken out of its result, with its landing, on that axis. What the two tell of the other axis is still compared; the whole-case form of this class is the one that
/// moves the subject by 2e-8 each way and asks the two to agree again.
fn fold_tiny(old: &Snapped, new: &Snapped) -> Option<(Snapped, Snapped)> {
    let (mut o, mut n) = (old.clone(), new.clone());
    let mut folded = false;
    for s in [&mut o, &mut n] {
        for axis in [Axis::X, Axis::Y] {
            let d = if axis == Axis::X { s.dx } else { s.dy };
            if d.abs() < 3.0e-8 && s.hits.iter().any(|h| h.axis == Some(axis)) {
                s.hits.retain(|h| h.axis != Some(axis));
                if axis == Axis::X {
                    s.dx = 0.0;
                } else {
                    s.dy = 0.0;
                }
                folded = true;
            }
        }
    }
    folded.then_some((o, n))
}

/// **At rest** (L9): on an axis both results leave the box where it is (a landing of nothing, to the tolerance) and tell different things about it, a line the box lies on
/// already against another it lies on (a spacing, an alignment, a lattice line): what is told of a box that does not move is what the two functions rank first, and the
/// invariant (the guide drawn is where the shape lands) holds in both. The hits of that axis are taken out of both.
fn fold_rest(old: &Snapped, new: &Snapped, eps: Eps) -> Option<(Snapped, Snapped)> {
    let (mut o, mut n) = (old.clone(), new.clone());
    let mut folded = false;
    for (axis, od, nd) in [(Axis::X, old.dx, new.dx), (Axis::Y, old.dy, new.dy)] {
        let (a, b) = (o.hits.iter().find(|h| h.axis == Some(axis)).copied(), n.hits.iter().find(|h| h.axis == Some(axis)).copied());
        if let (Some(a), Some(b)) = (a, b)
            && close(od, 0.0, eps)
            && close(nd, 0.0, eps)
            && !same_hit(&a, &b, eps)
        {
            o.hits.retain(|h| h.axis != Some(axis));
            n.hits.retain(|h| h.axis != Some(axis));
            folded = true;
        }
    }
    folded.then_some((o, n))
}

/// **The edge of a reach** (L9): the old result's hit on an axis lies exactly the pull of its kind from the subject (6, 8 or 12 points over the zoom, to the bit on a page of
/// exact reaches) and the library's result has nothing there or a hit that is nearer: whether a target exactly at the reach pulls is the last bit of a division. The old hit is
/// taken out of its result, and the comparison is made again.
fn fold_edge(case: &Case, old: &Snapped, new: &Snapped, eps: Eps) -> Option<(Snapped, Snapped)> {
    let (mut o, mut n) = (old.clone(), new.clone());
    let mut folded = false;
    for (axis, od, nd) in [(Axis::X, old.dx, new.dx), (Axis::Y, old.dy, new.dy)] {
        let Some(a) = o.hits.iter().find(|h| h.axis == Some(axis)).copied() else { continue };
        let Some(pull) = pull_of(case, a.target) else { continue };
        let b = n.hits.iter().find(|h| h.axis == Some(axis)).copied();
        if close(od.abs(), pull, eps) && b.is_none_or(|b| nd.abs() < od.abs() && !same_hit(&a, &b, eps)) {
            o.hits.retain(|h| h.axis != Some(axis));
            n.hits.retain(|h| h.axis != Some(axis));
            if axis == Axis::X {
                (o.dx, n.dx) = (0.0, 0.0);
            } else {
                (o.dy, n.dy) = (0.0, 0.0);
            }
            folded = true;
        }
    }
    folded.then_some((o, n))
}

/// **Unmarked** (L9, recorded): both results hold the same spacing hit on an axis (the same line and landing) while the library has a hit on the other axis too, today's gap and span are
/// told and the library's are not. A later change made the gap marks of each direction come from that direction's own hit, which removed four of the five cases the harness met at the
/// earlier pin; the one left is another thing: the other axis's pull moves the box out of the row of the neighbours the hit names, and another pair holds the same centre there
/// (a mirrored page), so the library keeps the hit and, under the harness's policy (a gap equal to a neighbour's, the next box beyond it, only the marks of what was snapped to), has no
/// gap to tell or mark for the neighbours it named. The adopted policy (any gap of a band, the marks of the final box) tells and marks the gaps the box has
/// (`a_spacing_hit_whose_neighbours_left_the_row_is_told_without_its_gap_while_another_pair_holds_the_same_centre`). The gap and span are taken out of the old hit, and the cases are
/// counted and held to a number.
fn fold_unmarked(_page: &SnapPage, _case: &Case, old: &Snapped, new: &Snapped, eps: Eps) -> Option<(Snapped, Snapped)> {
    let mut o = old.clone();
    let mut folded = false;
    for (axis, od, nd) in [(Axis::X, old.dx, new.dx), (Axis::Y, old.dy, new.dy)] {
        let (Some(i), Some(b)) = (o.hits.iter().position(|h| h.axis == Some(axis)), new.hits.iter().find(|h| h.axis == Some(axis)).copied()) else {
            continue;
        };
        let a = o.hits[i];
        let both =
            a.target == Target::Spacing && b.target == Target::Spacing && a.anchor == b.anchor && close(a.line, b.line, eps) && close(od, nd, eps);
        let other = if axis == Axis::X { Axis::Y } else { Axis::X };
        if both && b.span.is_none() && b.gap.is_none() && a.span.is_some() && new.hits.iter().any(|h| h.axis == Some(other)) {
            o.hits[i].span = None;
            o.hits[i].gap = None;
            folded = true;
        }
    }
    folded.then_some((o, new.clone()))
}

/// The classes of L9 that fold what the two functions tell apart, or that name a difference the library is held to.
fn l9_class(page: &SnapPage, case: &Case, old: &Snapped, new: &Snapped, eps: Eps) -> Result<Class, String> {
    if anchor_tie(case, old, new, eps) {
        return Ok(Class::AnchorTie);
    }
    let (mut o, mut n) = (old.clone(), new.clone());
    let mut last = None;
    type Fold = fn(&SnapPage, &Case, &Snapped, &Snapped, Eps) -> Option<(Snapped, Snapped)>;
    let folds: [(Class, Fold); 4] = [
        (Class::Unmarked, fold_unmarked),
        (Class::Tolerance, |_, _, o, n, _| fold_tiny(o, n)),
        (Class::AtRest, |_, _, o, n, e| fold_rest(o, n, e)),
        (Class::EdgeOfReach, |_, c, o, n, e| fold_edge(c, o, n, e)),
    ];
    for (class, fold) in folds {
        if let Some((a, b)) = fold(page, case, &o, &n, eps) {
            (o, n) = (a, b);
            last = Some(class);
        }
    }
    let left = match (last, compare(&o, &n, eps)) {
        (Some(class), Ok(_)) => return Ok(class),
        (_, Err(why)) => why,
        (None, Ok(_)) => "no fold applied".to_owned(),
    };
    Err(format!("after the folds of L9 the two still differ: {left}"))
}

/// A difference that is not a tie or a cut: today's result breaking the invariant while the library's holds it, or the width of a tolerance.
fn classify_failure(page: &SnapPage, case: &Case, old: &Snapped, new: &Snapped, eps: Eps, why: String) -> Result<Class, String> {
    // Which of equal shapes is named is no difference (a naming tie), and one that is left leaves the rest of the classes to be judged without it.
    let folded = fold_names(page, case, old, new, eps);
    if let Some((o, n)) = &folded
        && compare(o, n, eps).is_ok()
    {
        return Ok(Class::NameTie);
    }
    let (old, new) = folded.as_ref().map_or((old, new), |(o, n)| (o, n));
    if let Some(verdict) = flip_class(case, old, new, eps) {
        return verdict.map_err(|e| format!("{why}\n    {e}"));
    }
    if let Some(verdict) = band_class(page, case, old, new, eps) {
        return verdict.map_err(|e| format!("{why}\n    {e}"));
    }
    let old_spacing = old.hits.iter().find(|h| h.target == Target::Spacing);
    let new_spacing = new.hits.iter().find(|h| h.target == Target::Spacing);
    let old_breaks = old_spacing.is_some_and(|h| h.gap.is_none_or(|g| g <= 1.0e-9) || !claim_holds(page, case, old, h, eps));
    let new_holds = new_spacing.is_none_or(|h| h.gap.is_some_and(|g| g > 1.0e-9) && claim_holds(page, case, new, h, eps));
    if exact() && old_breaks && new_holds {
        return Ok(Class::TodayBreaks);
    }
    // The width of a tolerance: the two agree again with the subject moved by 2e-8 any way.
    let step = 2.0e-8;
    let moved = |dx: f64, dy: f64| -> Option<Subject> {
        Some(match case.subject {
            Subject::Point(p) => Subject::Point((p.0 + dx, p.1 + dy)),
            Subject::Box(r) => Subject::Box(Rect { x0: r.x0 + dx, y0: r.y0 + dy, x1: r.x1 + dx, y1: r.y1 + dy }),
            Subject::Edges(r, e) => Subject::Edges(Rect { x0: r.x0 + dx, y0: r.y0 + dy, x1: r.x1 + dx, y1: r.y1 + dy }, e),
        })
    };
    let mut every = true;
    for (dx, dy) in [(step, 0.0), (-step, 0.0), (0.0, step), (0.0, -step), (step, step), (step, -step), (-step, step), (-step, -step)] {
        let Some(subject) = moved(dx, dy) else { continue };
        let query = Query { subject, using: case.using, moving: &case.moving, zoom: case.zoom, own_points: &case.own };
        let (o, n) = (oracle::snap(&case.doc, page, &case.settings, &query), snap_with(Policy::TODAY, &case.doc, page, &case.settings, &query));
        if compare(&o, &n, eps).is_err() {
            every = false;
            break;
        }
    }
    if every {
        Ok(Class::Tolerance)
    } else if let Ok(class) = l9_class(page, case, old, new, eps) {
        Ok(class)
    } else {
        let left = l9_class(page, case, old, new, eps).err().unwrap_or_default();
        Err(format!(
            "{why}\n    {left}\n    library: {:?}\n    shapes: {:?}",
            super::adapter::LAST.with(|l| l.borrow().clone().map(|r| (r.hits, r.marks))),
            page.items_for_test().iter().map(|i| (i.id, i.bbox.x0, i.bbox.y0, i.bbox.x1, i.bbox.y1)).collect::<Vec<_>>()
        ))
    }
}

fn run(config: Config, seed: u64, cases: usize, exact_rows: bool) -> Counts {
    let mut rng = Rng(seed);
    let mut counts = Counts::default();
    for _ in 0..cases {
        let case = case_of(&mut rng, &config);
        let Some(page) = SnapPage::read(&case.doc, 1) else { continue };
        let query = Query { subject: case.subject, using: case.using, moving: &case.moving, zoom: case.zoom, own_points: &case.own };
        let old = oracle::snap(&case.doc, &page, &case.settings, &query);
        let new = snap_with(Policy::TODAY, &case.doc, &page, &case.settings, &query);
        // The same query on the same page is the same answer: the targets kept on the page are the same set.
        let again = snap_with(Policy::TODAY, &case.doc, &page, &case.settings, &query);
        counts.cases += 1;
        let eps = Eps::at(config.offset);
        if again != new {
            counts.differ.push(format!("the same query twice gave two answers: {new:?} then {again:?}"));
        }
        let verdict = match judge(&old, &new, eps, exact_rows) {
            Err(why) => classify_failure(&page, &case, &old, &new, eps, why),
            v => v,
        };
        match verdict {
            Ok(Class::TodayBreaks) => counts.today_breaks += 1,
            Ok(Class::Tolerance) => counts.tolerance += 1,
            Ok(Class::Flip) => counts.flips += 1,
            Ok(Class::Band) => counts.bands += 1,
            Ok(Class::Equal) => counts.equal += 1,
            Ok(Class::Tie) => counts.ties += 1,
            Ok(Class::Cut) => counts.cut += 1,
            Ok(Class::RowMode) => counts.row_mode += 1,
            Ok(Class::AnchorTie) => counts.anchor_ties += 1,
            Ok(Class::NameTie) => counts.name_ties += 1,
            Ok(Class::AtRest) => counts.at_rest += 1,
            Ok(Class::EdgeOfReach) => counts.edges_of_reach += 1,
            Ok(Class::Unmarked) => counts.unmarked += 1,
            Err(why) => {
                counts.differences += 1;
                // The first twelve are told (`SNAP_EQ_SHOW=<n>` for more, when a class is being worked out).
                let shown = std::env::var("SNAP_EQ_SHOW").ok().and_then(|v| v.parse().ok()).unwrap_or(12);
                if counts.differ.len() < shown {
                    counts.differ.push(format!(
                        "{why}\n    marks of the adapter: {:?}\n    settings {:?}\n    subject {:?} using {:?} moving {:?} zoom {} own {:?}",
                        new.marks, case.settings, case.subject, case.using, case.moving, case.zoom, case.own
                    ));
                }
                continue;
            }
        }
        counts.places += usize::from(new.hits.iter().any(|h| h.axis.is_none()));
        counts.aligned += usize::from(new.hits.iter().any(|h| h.target == Target::Alignment));
        counts.spaced += usize::from(has_spacing(&new));
        counts.held += usize::from(!new.holds.is_empty());
        counts.glued += usize::from(new.glue.is_some());
        counts.resized += usize::from(matches!(case.subject, Subject::Edges(..)) && !new.hits.is_empty());
    }
    counts
}

fn assert_equal(name: &str, c: &Counts) {
    eprintln!("{name}: {c:?}");
    assert!(c.differ.is_empty(), "{name}: {} differences, the first:\n{}", c.differences.max(c.differ.len()), c.differ.join("\n"));
}

const PLAIN: Config =
    Config { lattice: false, offset: 0.0, turned: false, masters: false, zooms: ZOOMS, rows: false, mirrored: false, exact: false, coarse: false };

fn exact() -> bool {
    !matches!(Policy::TODAY.spacing_row, craft_snap::SpacingRow::Linked)
}

#[test]
fn the_tolerance_far_from_the_origin_is_a_few_steps_and_never_a_hundredth_of_an_inch() {
    // L9 of the review: `1e-9 * (1 + offset)` handed to a comparison that scales by the number again accepted a landing 0.01 inch off at 1e7 (and a line 1e5 inches off).
    let far = Eps::at(1.0e7);
    assert!(!close(0.03, 0.04, far) && !close(-0.03, 0.03, far), "a landing a hundredth of an inch off is not equal");
    assert!(!close(1.0e7, 1.0e7 + 1.0e-5, far), "a line a hundred thousandth of an inch off is not equal");
    assert!(close(1.0e7, 1.0e7 + 1.9e-9, far) && close(0.03, 0.03 + 1.0e-8, far), "a step or two of the number is");
    assert!(far.limit(0.03, 0.04) < 1.0e-6 && far.limit(1.0e7, 1.0e7) < 1.0e-6, "{far:?}");
    // The same two landings, as a comparison of two answers: they differ.
    let hit = |d: f64| Snapped {
        dx: d,
        hits: vec![Hit {
            target: Target::Alignment,
            axis: Some(Axis::X),
            normal: None,
            line: 1.0e7 + 2.0,
            at: None,
            anchor: Anchor::Low,
            shape: Some(1),
            point: None,
            span: None,
            gap: None,
        }],
        ..Snapped::default()
    };
    assert!(compare(&hit(-0.03), &hit(-0.04), far).is_err(), "dx -0.03 against -0.04 at 1e7");
    assert!(compare(&hit(-0.03), &hit(-0.03 + 1.0e-8), far).is_ok());
    // Near the origin it is a billionth of the number, as it was.
    assert_eq!(Eps::at(0.0), Eps::NEAR);
    assert!(
        close(5.0, 5.0 + 4.0e-9, Eps::NEAR)
            && !close(5.0, 5.0 + 6.0e-9, Eps::NEAR)
            && close(0.0, 9.0e-10, Eps::NEAR)
            && !close(0.0, 2.0e-9, Eps::NEAR)
    );
}

#[test]
fn random_pages_plain_shapes() {
    let c = run(PLAIN, 11, 4_000, exact());
    assert_equal("plain", &c);
    assert!(c.aligned > 400 && c.held > 20, "{c:?}");
}

#[test]
fn random_pages_with_the_stencils_rectangle_and_ellipse() {
    let c = run(Config { masters: true, ..PLAIN }, 12, 2_500, exact());
    assert_equal("masters", &c);
    assert!(c.places > 200 && c.glued > 20, "{c:?}");
}

#[test]
fn random_pages_with_turned_and_flipped_shapes() {
    let c = run(Config { turned: true, masters: true, ..PLAIN }, 13, 2_500, exact());
    assert_equal("turned", &c);
    assert!(c.places > 100, "{c:?}");
}

#[test]
fn pages_on_an_eighth_of_an_inch_have_exact_ties_and_exact_reaches() {
    let c = run(Config { lattice: true, masters: true, ..PLAIN }, 14, 2_500, exact());
    assert_equal("lattice", &c);
    assert!(c.ties > 20, "ties are there to be counted: {c:?}");
}

#[test]
fn mirrored_pages_have_an_equal_for_everything_on_the_other_side_of_the_subject() {
    let c = run(Config { mirrored: true, lattice: true, masters: true, ..PLAIN }, 21, 4_000, exact());
    assert_equal("mirrored", &c);
    assert!(c.ties > 20 && c.anchor_ties > 0 && c.name_ties > 0, "ties are there to be counted: {c:?}");
}

#[test]
fn pages_with_the_subject_exactly_a_pull_from_a_line_or_a_place() {
    let c = run(Config { exact: true, masters: true, zooms: ZOOMS_EXACT, ..PLAIN }, 22, 4_000, exact());
    assert_equal("exact reach", &c);
    assert!(c.aligned > 100 && c.anchor_ties > 0, "{c:?}");
}

#[test]
fn pages_of_boxes_that_end_at_one_place_with_equal_gaps() {
    let c = run(Config { coarse: true, rows: true, ..PLAIN }, 23, 4_000, exact());
    assert_equal("coarse rows", &c);
    // The long pull beside a hit at rest that the library did not make is made: a case of it is a difference now, with no class to go in.
    assert_eq!(c.unmarked, 0, "{c:?}");
    let c = run(Config { coarse: true, masters: true, ..PLAIN }, 24, 4_000, exact());
    assert_equal("coarse", &c);
}

#[test]
fn pages_of_everything_at_once_mirrored_exact_equal_and_turned() {
    let c = run(Config { mirrored: true, exact: true, turned: true, masters: true, zooms: ZOOMS_EXACT, ..PLAIN }, 25, 4_000, exact());
    assert_equal("mixed", &c);
    // The recorded difference of the library that is left (a spacing hit whose neighbours left the row at the landing, where another pair holds the same centre): held to the
    // number these pages meet. Five cases met the marks of the first axis only at the earlier pin, and four of them are gone.
    assert_eq!(c.unmarked, 1, "{c:?}");
    assert!(c.name_ties > 0, "{c:?}");
}

#[test]
fn pages_far_from_the_origin() {
    for (seed, offset) in [(15, 1.0e3), (16, 1.0e6), (17, 1.0e7)] {
        let c = run(Config { offset, turned: true, ..PLAIN }, seed, 800, exact());
        assert_equal(&format!("far {offset}"), &c);
    }
}

#[test]
fn rows_and_staircases() {
    let c = run(Config { rows: true, ..PLAIN }, 18, 9_000, exact());
    eprintln!("rows (spacing compared: {}): {c:?}", exact());
    assert!(c.differ.is_empty(), "{}", c.differ.join("\n"));
    assert!(c.spaced + c.row_mode > 150, "{c:?}");
}

#[test]
fn a_zoom_that_is_no_zoom_is_the_default_zoom_in_both() {
    for zoom in [0.0, -3.0, f64::NAN, f64::INFINITY] {
        let mut rng = Rng(19);
        for _ in 0..200 {
            let mut case = case_of(&mut rng, &Config { masters: true, ..PLAIN });
            case.zoom = zoom;
            let page = SnapPage::read(&case.doc, 1).unwrap();
            let query = Query { subject: case.subject, using: case.using, moving: &case.moving, zoom, own_points: &case.own };
            let old = oracle::snap(&case.doc, &page, &case.settings, &query);
            let new = snap_with(Policy::TODAY, &case.doc, &page, &case.settings, &query);
            judge(&old, &new, Eps::NEAR, exact()).unwrap_or_else(|why| panic!("zoom {zoom}: {why}"));
        }
    }
}

#[test]
fn a_page_with_more_shapes_near_than_a_query_looks_into_says_so_in_both() {
    // 90 small shapes in a cluster, the subject among them: both cut the shapes near at 64 and say `truncated`.
    let mut page = PageBuilder::new().cell("PageWidth", "16").cell("PageHeight", "16");
    for i in 0..90u32 {
        let (x, y) = (5.0 + f64::from(i % 10) * 0.12, 5.0 + f64::from(i / 10) * 0.12);
        page = page.shape(
            ShapeBuilder::new(i + 1)
                .cell("PinX", &x.to_string())
                .cell("PinY", &y.to_string())
                .cell("Width", "0.1")
                .cell("Height", "0.1")
                .cell("LocPinX", "0.05")
                .cell("LocPinY", "0.05")
                .cell("Angle", "0"),
        );
    }
    let doc = DocBuilder::new().page(1, page).build();
    let page = SnapPage::read(&doc, 1).unwrap();
    let mut s = SnapSettings::none();
    s.snap = true;
    s.alignment_box = true;
    s.snap_handles = true;
    let query =
        Query { subject: Subject::Box(Rect { x0: 5.4, y0: 5.4, x1: 5.5, y1: 5.5 }), using: Use::Place, moving: &[], zoom: 96.0, own_points: &[] };
    let old = oracle::snap(&doc, &page, &s, &query);
    let new = snap_with(Policy::TODAY, &doc, &page, &s, &query);
    assert!(old.truncated && new.truncated, "old {} new {}", old.truncated, new.truncated);
}

#[test]
fn a_drag_builds_its_targets_once() {
    let mut rng = Rng(20);
    let case = case_of(&mut rng, &Config { masters: true, ..PLAIN });
    let page = SnapPage::read(&case.doc, 1).unwrap();
    let mut s = case.settings;
    s.snap = true;
    s.alignment_box = true;
    let moving = [1u32];
    for i in 0..40 {
        let r = Rect { x0: 3.0 + f64::from(i) * 0.01, y0: 3.0, x1: 4.0, y1: 4.0 };
        let query = Query { subject: Subject::Box(r), using: Use::Place, moving: &moving, zoom: 96.0, own_points: &[] };
        let _ = snap_with(Policy::TODAY, &case.doc, &page, &s, &query);
    }
    assert_eq!(page.target_builds(), 1, "forty moves of one drag are one build");
    let query =
        Query { subject: Subject::Box(Rect { x0: 3.0, y0: 3.0, x1: 4.0, y1: 4.0 }), using: Use::Place, moving: &[2], zoom: 96.0, own_points: &[] };
    let _ = snap_with(Policy::TODAY, &case.doc, &page, &s, &query);
    assert_eq!(page.target_builds(), 2, "other shapes moving are another set");
    // A snap that needs no shape reads none.
    let mut grid_only = SnapSettings::none();
    grid_only.snap = true;
    grid_only.grid = true;
    let fresh = SnapPage::read(&case.doc, 1).unwrap();
    let query = Query { subject: Subject::Point((1.1, 1.1)), using: Use::Place, moving: &[], zoom: 96.0, own_points: &[] };
    let _ = snap_with(Policy::TODAY, &case.doc, &fresh, &grid_only, &query);
    assert!(!fresh.shapes_read(), "a grid-only snap reads no shape");
}

// ------------------------------------------------------------------------------------------------------------------ crafted inputs the brief names

/// A page of boxes, each `(id, x0, y0, x1, y1)` in paper inches.
fn boxes_page(boxes: &[(u32, f64, f64, f64, f64)]) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "400").cell("PageHeight", "400");
    for (id, x0, y0, x1, y1) in boxes {
        let (w, h) = (x1 - x0, y1 - y0);
        page = page.shape(
            ShapeBuilder::new(*id)
                .cell("PinX", &((x0 + x1) / 2.0).to_string())
                .cell("PinY", &((y0 + y1) / 2.0).to_string())
                .cell("Width", &w.to_string())
                .cell("Height", &h.to_string())
                .cell("LocPinX", &(w / 2.0).to_string())
                .cell("LocPinY", &(h / 2.0).to_string())
                .cell("Angle", "0"),
        );
    }
    DocBuilder::new().page(1, page).build()
}

fn dynamic_grid_only() -> SnapSettings {
    let mut s = SnapSettings::none();
    s.snap = true;
    s.dynamic_grid = true;
    s
}

/// Asks both and says whether they agree and what they said.
fn both(doc: &Document, s: &SnapSettings, subject: Subject, moving: &[u32]) -> (Snapped, Snapped, Result<Class, String>) {
    let page = SnapPage::read(doc, 1).unwrap();
    let query = Query { subject, using: Use::Place, moving, zoom: 96.0, own_points: &[] };
    let old = oracle::snap(doc, &page, s, &query);
    let new = snap_with(Policy::TODAY, doc, &page, s, &query);
    let verdict = judge(&old, &new, Eps::NEAR, exact());
    (old, new, verdict)
}

#[test]
fn a_staircase_is_no_row_in_either() {
    // The case of the first report of this task: the subject x 4.05 to 5.05, y 0 to 1; L x 2 to 3, y 0.5 to 1.5 (it overlaps the subject across); B x 0 to 1, y 1.2 to 2.2
    // (it overlaps L across and misses the subject's strip). The box beyond a neighbour must lie wholly beyond it, and in the strip: B is neither. `Linked` took it.
    let doc = boxes_page(&[(1, 2.0, 0.5, 3.0, 1.5), (2, 0.0, 1.2, 1.0, 2.2)]);
    let (old, new, verdict) = both(&doc, &dynamic_grid_only(), Subject::Box(Rect { x0: 4.05, y0: 0.0, x1: 5.05, y1: 1.0 }), &[]);
    assert!(matches!(verdict, Ok(Class::Equal)), "{verdict:?} {old:?} {new:?}");
    assert!(!has_spacing(&old) && !has_spacing(&new) && old.dx == 0.0 && new.dx == 0.0, "{old:?} {new:?}");
    // The same two shapes with B wholly beyond L and in the strip: both find the gap and agree.
    let doc = boxes_page(&[(1, 2.0, 0.5, 3.0, 1.5), (2, 0.0, 0.6, 1.0, 1.2)]);
    let (old, new, verdict) = both(&doc, &dynamic_grid_only(), Subject::Box(Rect { x0: 4.05, y0: 0.0, x1: 5.05, y1: 1.0 }), &[]);
    assert!(matches!(verdict, Ok(Class::Equal)) && has_spacing(&old) && has_spacing(&new), "{verdict:?} {old:?} {new:?}");
    assert!((new.dx + 0.05).abs() < 1.0e-9, "{new:?}");
}

#[test]
fn one_page_asked_under_two_policies_keeps_each_policys_targets() {
    // A shape with no line (`LinePattern` 0): the old policy lines a box up with it, the adopted one does not. The two asked of one `SnapPage`, in either order: the
    // targets kept on the page are keyed by the policy as well as by the shapes that move.
    let page = PageBuilder::new().cell("PageWidth", "16").cell("PageHeight", "16").shape(
        ShapeBuilder::new(1)
            .cell("PinX", "3.5")
            .cell("PinY", "3.5")
            .cell("Width", "1")
            .cell("Height", "1")
            .cell("LocPinX", "0.5")
            .cell("LocPinY", "0.5")
            .cell("Angle", "0")
            .cell("LinePattern", "0"),
    );
    let doc = DocBuilder::new().page(1, page).build();
    let s = dynamic_grid_only();
    let query =
        Query { subject: Subject::Box(Rect { x0: 3.03, y0: 5.0, x1: 4.03, y1: 6.0 }), using: Use::Place, moving: &[], zoom: 96.0, own_points: &[] };
    for order in [[Policy::TODAY, Policy::ADOPTED], [Policy::ADOPTED, Policy::TODAY]] {
        let page = SnapPage::read(&doc, 1).unwrap();
        let first = snap_with(order[0], &doc, &page, &s, &query);
        let second = snap_with(order[1], &doc, &page, &s, &query);
        let (today, adopted) = if order[0] == Policy::TODAY { (first, second) } else { (second, first) };
        assert!((today.dx + 0.03).abs() < 1e-9, "today lines up with a shape that has no line: {today:?}");
        assert_eq!(adopted.dx, 0.0, "the adopted policy does not: {adopted:?}");
        assert_eq!(page.target_builds(), 2, "one build for each policy");
    }
}

#[test]
fn a_long_row_of_equal_gaps_crosses_the_caps_and_both_say_so() {
    // 300 boxes 1 by 1 inch, 1 inch apart, in a row; the subject drops into the middle of it. Both cut the row at 64 shapes and say `truncated`, and neither takes
    // more than the cap's worth of work (the hits are not compared past a cut).
    let boxes: Vec<(u32, f64, f64, f64, f64)> = (0..300).map(|i| (i + 1, f64::from(i) * 2.0, 5.0, f64::from(i) * 2.0 + 1.0, 6.0)).collect();
    let doc = boxes_page(&boxes);
    // A subject 2 inches wide, so that its row reaches 8 inches along; the strip holds the shapes within it, and the page the other 290: only the cut matters.
    let (old, new, verdict) = both(&doc, &dynamic_grid_only(), Subject::Box(Rect { x0: 300.03, y0: 5.0, x1: 302.03, y1: 6.0 }), &[]);
    assert!(old.truncated == new.truncated, "{old:?} {new:?}");
    assert!(verdict.is_ok(), "{verdict:?} {old:?} {new:?}");
    // Past the cap the hits stay the nearest ones: a box beside the end of the row still finds its gap.
    let (old, new, verdict) = both(&doc, &dynamic_grid_only(), Subject::Box(Rect { x0: 600.03, y0: 5.0, x1: 601.03, y1: 6.0 }), &[]);
    assert!(verdict.is_ok(), "{verdict:?} {old:?} {new:?}");
    assert!(new.candidates <= 3 * 64 && new.examined <= 3 * 4_096, "{new:?}");
}

// ------------------------------------------------------------------------------------------------------------------ measurements

/// A page of `n` boxes in rows of 300, 1.5 inches apart.
fn crowded_page(n: u32) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "1000").cell("PageHeight", "1000");
    for i in 0..n {
        let (x, y) = (f64::from(i % 300) * 1.5 + 1.0, f64::from(i / 300) * 1.5 + 1.0);
        page = page.shape(
            ShapeBuilder::new(i + 1)
                .cell("PinX", &x.to_string())
                .cell("PinY", &y.to_string())
                .cell("Width", "1")
                .cell("Height", "1")
                .cell("LocPinX", "0.5")
                .cell("LocPinY", "0.5")
                .cell("Angle", "0"),
        );
    }
    DocBuilder::new().page(1, page).build()
}

#[test]
fn a_page_of_a_hundred_thousand_shapes_costs_the_cap_and_a_logarithm() {
    // 5,000 shapes are targets (`MAX_INDEXED`); the page is read for its guides once, whatever it holds, and every result says `truncated`.
    let doc = crowded_page(100_000);
    let page = SnapPage::read(&doc, 1).unwrap();
    assert_eq!(page.scanned, 100_000, "one kind check for each shape of the page, once");
    assert!(!page.shapes_read());
    let mut s = SnapSettings::none();
    s.snap = true;
    s.alignment_box = true;
    // Over the 30th box of the 5th row (the first 5,000 shapes are the first 16 rows).
    let r = Rect { x0: 45.0, y0: 7.53, x1: 46.0, y1: 8.53 };
    let query = Query { subject: Subject::Box(r), using: Use::Place, moving: &[], zoom: 96.0, own_points: &[] };
    let out = super::snap(&doc, &page, &s, &query);
    let index = page.shapes(&doc);
    let report = page.target_report().unwrap();
    let work = super::adapter::LAST.with(|l| l.borrow().clone()).unwrap().work;
    eprintln!(
        "100,000 shapes: kind checks {} frames read {} of {} outline {:?} the app's lines {} (places {}) objects {} (so object lines {}: six an unturned object, by construction; the library's report counts the app's own only) bisection steps {} gathers {} line steps {} truncated {}",
        page.scanned,
        index.read,
        index.total,
        out.outline,
        report.lines,
        report.line_places,
        report.objects,
        6 * report.objects,
        work.bisect_steps,
        work.gathers,
        work.line_steps,
        out.truncated
    );
    assert_eq!((index.read, index.total, report.objects as usize), (super::index::MAX_INDEXED, 100_000, super::index::MAX_INDEXED));
    assert!(out.truncated && index.truncated, "the shapes after the first 5,000 are not targets, and the result says so");
    assert_eq!((out.outline.bytes, out.outline.elements), (0, 0), "no outline was built: no setting asked for one");
    assert_eq!((work.gathers, work.visited), (0, 0), "alignment gathers nothing");
    // Measured: 132 steps of bisection, whatever the page holds (a page of 100 shapes costs 264 with three kinds on; one of 5,000 the same as this one).
    assert!((100..=170).contains(&work.bisect_steps), "{}", work.bisect_steps);
}

#[test]
fn the_cap_on_the_frames_read_is_one_number_and_the_work_follows_it_by_a_count_and_a_logarithm() {
    // The cap is `read_with`'s one number (`MAX_INDEXED` for `read`). On a page of 12,000 shapes, a cap of 1,000, 5,000 and 12,000: the page is scanned for its
    // guides once, whatever the cap; the index reads exactly that many frames and says `truncated` unless it read all; the library is given that many objects (6
    // lines each, so 6 times as many lines as the shapes read); and the bisections do not follow the cap.
    let doc = crowded_page(12_000);
    let mut steps = Vec::new();
    for cap in [1_000usize, 5_000, 12_000] {
        let page = SnapPage::read_with(&doc, 1, cap).unwrap();
        assert_eq!(page.scanned, 12_000, "one kind check for each shape of the page, once, whatever the cap");
        let mut s = SnapSettings::none();
        s.snap = true;
        s.alignment_box = true;
        let r = Rect { x0: 45.0, y0: 2.53, x1: 46.0, y1: 3.53 };
        let query = Query { subject: Subject::Box(r), using: Use::Place, moving: &[], zoom: 96.0, own_points: &[] };
        let out = super::snap(&doc, &page, &s, &query);
        let (index, report) = (page.shapes(&doc), page.target_report().unwrap());
        assert_eq!((index.read, index.total, report.objects as usize), (cap, 12_000, cap), "a cap of {cap}");
        assert_eq!((index.truncated, out.truncated), (cap < 12_000, cap < 12_000), "a cap of {cap}");
        steps.push(super::adapter::LAST.with(|l| l.borrow().clone()).unwrap().work.bisect_steps);
    }
    assert!(steps.iter().all(|s| (100..=170).contains(s)), "the same logarithm at 1,000 and at 12,000 shapes: {steps:?}");
}

/// The resident set and its peak, in kilobytes, from the process's own status file (0 where there is none).
fn resident_kb() -> (u64, u64) {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(name))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0)
    };
    (field("VmRSS:"), field("VmHWM:"))
}

#[test]
#[ignore = "a measurement, not a test: SNAP_MEASURE_N=<shapes> prints what reading a page of that many shapes costs when every frame is read (the cap of the frames read), with its yardsticks; run with --ignored --nocapture"]
fn measure_the_whole_read_of_a_page_of_many_shapes() {
    use std::time::Instant;
    let n: usize = std::env::var("SNAP_MEASURE_N").ok().and_then(|v| v.parse().ok()).unwrap_or(5_000);
    let (rss_start, _) = resident_kb();
    let doc = crowded_page(u32::try_from(n).unwrap());
    let (rss_doc, _) = resident_kb();
    // The yardsticks: one cell read for each shape (the cost of one cell), and the scan of the page for its guides that every revision pays whatever the cap.
    let reader = crate::draw::Reader::new(&doc);
    let page_ref = doc.page(1).unwrap();
    let started = Instant::now();
    let mut sum = 0.0;
    for i in 0..page_ref.shapes.len() {
        let at = diacraft_doc::ShapeRef { owner: diacraft_doc::Owner::Page(1), path: vec![u32::try_from(i).unwrap()] };
        sum += reader.number(&diacraft_doc::SheetRef::Shape(at), "Width");
    }
    let one_cell = started.elapsed();
    let started = Instant::now();
    let page = SnapPage::read_with(&doc, 1, n).unwrap();
    let scan = started.elapsed();
    // The index: every frame read (the cells of a frame: Width, Height, the pin and its place, the angle, the flips, the line pattern, whether it is one-dimensional).
    let started = Instant::now();
    let index = page.shapes(&doc);
    let frames = started.elapsed();
    let (rss_index, _) = resident_kb();
    // The library's targets, built by the first query of a drag, and the second query, which finds them built.
    let mut s = SnapSettings::none();
    s.snap = true;
    s.alignment_box = true;
    s.dynamic_grid = true;
    let r = Rect { x0: 45.0, y0: 2.53, x1: 46.0, y1: 3.53 };
    let query = Query { subject: Subject::Box(r), using: Use::Place, moving: &[], zoom: 96.0, own_points: &[] };
    let started = Instant::now();
    let _ = super::snap(&doc, &page, &s, &query);
    let first = started.elapsed();
    let (rss_targets, _) = resident_kb();
    let started = Instant::now();
    let _ = super::snap(&doc, &page, &s, &query);
    let second = started.elapsed();
    let report = page.target_report().unwrap();
    let (_, peak) = resident_kb();
    eprintln!(
        "MEASURE n={n} read={} one_cell_read_ms={:.2} guide_scan_ms={:.2} frames_ms={:.2} ({:.2} us a frame, {:.1} cells' worth) first_query_ms={:.2} (targets built: {} objects, {} lines) second_query_us={:.0} | doc_mb={:.1} index_mb={:.1} ({:.0} bytes a shape) targets_mb={:.1} ({:.0} bytes a shape) peak_mb={:.1} sum={sum}",
        index.read,
        one_cell.as_secs_f64() * 1e3,
        scan.as_secs_f64() * 1e3,
        frames.as_secs_f64() * 1e3,
        frames.as_secs_f64() * 1e6 / n as f64,
        frames.as_secs_f64() / one_cell.as_secs_f64().max(1e-9),
        first.as_secs_f64() * 1e3,
        report.objects,
        report.lines,
        second.as_secs_f64() * 1e6,
        rss_doc.saturating_sub(rss_start) as f64 / 1024.0,
        rss_index.saturating_sub(rss_doc) as f64 / 1024.0,
        rss_index.saturating_sub(rss_doc) as f64 * 1024.0 / n as f64,
        rss_targets.saturating_sub(rss_index) as f64 / 1024.0,
        rss_targets.saturating_sub(rss_index) as f64 * 1024.0 / n as f64,
        peak as f64 / 1024.0,
    );
}

#[test]
#[ignore = "a measurement, not a test: prints the cost of reading every frame of a page of 100,000 shapes (the cap of the frames read); run with --ignored --nocapture"]
fn measure_the_read_of_all_frames_of_a_hundred_thousand_shapes() {
    let doc = crowded_page(100_000);
    let reader = crate::draw::Reader::new(&doc);
    let page_ref = doc.page(1).unwrap();
    let started = std::time::Instant::now();
    let mut read = 0usize;
    for (i, shape) in page_ref.shapes.iter().enumerate() {
        let at = diacraft_doc::ShapeRef { owner: diacraft_doc::Owner::Page(1), path: vec![i as u32] };
        if reader.corners(&at).iter().all(|p| p.0.is_finite()) && !shape.del {
            read += 1;
        }
    }
    let frames = started.elapsed();
    let started = std::time::Instant::now();
    let mut b = craft_snap::TargetsBuilder::new(craft_snap::Limits::default());
    for i in 0..100_000u64 {
        let c = craft_snap::TurnedBox::from_rect(kurbo::Rect::new(
            (i % 300) as f64 * 1.5,
            (i / 300) as f64 * 1.5,
            (i % 300) as f64 * 1.5 + 1.0,
            (i / 300) as f64 * 1.5 + 1.0,
        ));
        let _ = b.object(craft_snap::Id(i), craft_snap::Object { frame: c, bounds: None, angle: None, parts: craft_snap::Parts::ALL });
    }
    let targets = b.build();
    let built = started.elapsed();
    eprintln!(
        "MEASURE: {read} frames read in {frames:?} ({:?} each); {} objects built in {built:?}; object {} bytes, line {} bytes, line places {}",
        frames / read.max(1) as u32,
        targets.report().objects,
        std::mem::size_of::<craft_snap::Object>(),
        std::mem::size_of::<(f64, f64, f64, f64, f64, f64, u64)>(),
        targets.report().line_places
    );
}

// ------------------------------------------------------------------------------------------------------------------ the classes of L9, each on a case made by hand

fn hit(target: Target, axis: Option<Axis>, line: f64, anchor: Anchor, shape: Option<u32>) -> Hit {
    Hit { target, axis, normal: None, line, at: None, anchor, shape, point: None, span: None, gap: None }
}

fn result(dx: f64, dy: f64, hits: Vec<Hit>) -> Snapped {
    Snapped { dx, dy, hits, ..Snapped::default() }
}

fn case_of_subject(doc: Document, subject: Subject, own: Vec<P>, zoom: f64) -> Case {
    Case { doc, subject, using: Use::Place, moving: Vec::new(), own, settings: SnapSettings { strength: calibrated(), ..SnapSettings::none() }, zoom }
}

#[test]
fn an_anchor_tie_is_one_place_reached_by_two_anchors_at_one_distance_and_nothing_else() {
    // The box (4, 4) to (5, 5) has anchors at y 4, 4.5 and 5 along x = 4.5: the place (4.5, 4.75) is 0.25 from the middle and from the top.
    let case = case_of_subject(page_of_nothing(), Subject::Box(Rect { x0: 4.0, y0: 4.0, x1: 5.0, y1: 5.0 }), Vec::new(), 96.0);
    let place = Hit { at: Some((4.5, 4.75)), line: 4.5, ..hit(Target::Handle, None, 4.5, Anchor::Point, Some(3)) };
    let (old, new) = (result(0.0, 0.25, vec![place]), result(0.0, -0.25, vec![place]));
    let eps = Eps::NEAR;
    assert!(anchor_tie(&case, &old, &new, eps));
    // A delta of another length is not the same distance, one that brings no place of the subject onto the place is not an anchor, and two hits are not one place.
    assert!(!anchor_tie(&case, &old, &result(0.0, -0.3, vec![place]), eps));
    assert!(!anchor_tie(&case, &old, &result(0.25, 0.0, vec![place]), eps));
    assert!(!anchor_tie(&case, &old, &result(0.0, -0.25, vec![place, hit(Target::Guide, Some(Axis::X), 1.0, Anchor::Low, Some(9))]), eps));
    // Another place is another hit.
    let other = Hit { at: Some((4.5, 4.7)), ..place };
    assert!(!anchor_tie(&case, &old, &result(0.0, -0.25, vec![other]), eps));
    // With a place of the moving shapes' own the anchors are those places as well: (4.2, 4.75) is 0.25 from the own place (4.2, 4.5) and from the box's top left corner (4, 5)? no:
    // 0.25 up from (4.2, 4.5) and 0.25 down from (4.2, 5.0), where an own place stands.
    let case = case_of_subject(page_of_nothing(), Subject::Box(Rect { x0: 4.0, y0: 4.0, x1: 5.0, y1: 5.0 }), vec![(4.2, 5.0)], 96.0);
    let place = Hit { at: Some((4.2, 4.75)), line: 4.2, ..place };
    assert!(anchor_tie(&case, &result(0.0, 0.25, vec![place]), &result(0.0, -0.25, vec![place]), eps));
    // A point has the one place.
    let case = case_of_subject(page_of_nothing(), Subject::Point((4.5, 4.5)), Vec::new(), 96.0);
    assert!(!anchor_tie(&case, &old, &new, eps));
}

fn page_of_nothing() -> Document {
    boxes_page(&[])
}

#[test]
fn a_naming_tie_needs_both_shapes_to_have_what_the_hit_claims() {
    // A (1) and B (2) have their left sides on x = 2; C (3) has not. An alignment of the box's left side on x = 2 named A by one and B by the other is a naming tie; named C
    // is not.
    let doc = boxes_page(&[(1, 2.0, 0.0, 3.0, 1.0), (2, 2.0, 5.0, 4.0, 6.0), (3, 2.5, 8.0, 3.5, 9.0)]);
    let page = SnapPage::read(&doc, 1).unwrap();
    let _ = page.shapes(&doc); // the index the classes read
    let case = case_of_subject(doc, Subject::Box(Rect { x0: 2.03, y0: 2.0, x1: 3.03, y1: 3.0 }), Vec::new(), 96.0);
    let with_span = |shape: u32, span: (f64, f64)| Hit { span: Some(span), ..hit(Target::Alignment, Some(Axis::X), 2.0, Anchor::Low, Some(shape)) };
    let eps = Eps::NEAR;
    let (old, new) = (result(-0.03, 0.0, vec![with_span(1, (0.0, 3.0))]), result(-0.03, 0.0, vec![with_span(2, (2.0, 6.0))]));
    let (o, n) = fold_names(&page, &case, &old, &new, eps).expect("a naming tie");
    assert!(o.hits[0].shape.is_none() && n.hits[0].shape.is_none() && o.hits[0].span.is_none() && n.hits[0].span.is_none());
    assert!(compare(&o, &n, eps).is_ok());
    // C has no side there: not the same line.
    assert!(fold_names(&page, &case, &old, &result(-0.03, 0.0, vec![with_span(3, (2.0, 9.0))]), eps).is_none());
    // The same shape named: nothing to fold. A different landing, anchor or target: not a naming tie.
    assert!(fold_names(&page, &case, &old, &old, eps).is_none());
    assert!(fold_names(&page, &case, &old, &result(-0.04, 0.0, vec![with_span(2, (2.0, 6.0))]), eps).is_none());
    let high = Hit { anchor: Anchor::High, ..with_span(2, (2.0, 6.0)) };
    assert!(fold_names(&page, &case, &old, &result(-0.03, 0.0, vec![high]), eps).is_none());
    let guide = Hit { target: Target::Guide, ..with_span(2, (2.0, 6.0)) };
    assert!(
        fold_names(
            &page,
            &case,
            &Snapped { hits: vec![Hit { target: Target::Guide, ..old.hits[0] }], ..old.clone() },
            &result(-0.03, 0.0, vec![guide]),
            eps
        )
        .is_none()
    );
    // A spacing: L (1) from x 0 to 1 and R (2) from 6 to 7, the box landing at 3 to 4: both are 2 away, and the hit says a gap of 2. Named L by one and R by the other.
    let doc = boxes_page(&[(1, 0.0, 3.0, 1.0, 4.0), (2, 6.0, 3.0, 7.0, 4.0), (3, 9.0, 3.0, 10.0, 4.0)]);
    let page = SnapPage::read(&doc, 1).unwrap();
    let _ = page.shapes(&doc);
    let case = case_of_subject(doc, Subject::Box(Rect { x0: 3.03, y0: 3.0, x1: 4.03, y1: 4.0 }), Vec::new(), 96.0);
    let spacing =
        |shape: u32| Hit { gap: Some(2.0), span: Some((1.0, 3.0)), ..hit(Target::Spacing, Some(Axis::X), 3.5, Anchor::Middle, Some(shape)) };
    let (old, new) = (result(-0.03, 0.0, vec![spacing(1)]), result(-0.03, 0.0, vec![spacing(2)]));
    let (o, n) = fold_names(&page, &case, &old, &new, eps).expect("a naming tie of equal gaps");
    assert!(o.hits[0].shape.is_none() && n.hits[0].shape.is_none() && o.hits[0].span.is_some(), "the span of a spacing is the gap's, and is kept");
    // Shape 3 is 5 away: not the gap of 2.
    assert!(fold_names(&page, &case, &old, &result(-0.03, 0.0, vec![spacing(3)]), eps).is_none());
}

#[test]
fn a_hit_at_rest_is_folded_only_where_both_leave_the_box_where_it_is() {
    let eps = Eps::NEAR;
    let a = hit(Target::Spacing, Some(Axis::X), 14.5, Anchor::Middle, Some(6));
    let b = hit(Target::Ruler, Some(Axis::X), 14.0, Anchor::Low, None);
    let (o, n) = fold_rest(&result(0.0, 0.0, vec![a]), &result(0.0, 0.0, vec![b]), eps).expect("both at rest");
    assert!(o.hits.is_empty() && n.hits.is_empty());
    // The same hit told twice is equal, not folded; a landing on either side is not at rest; a result with no hit on the axis has nothing to fold.
    assert!(fold_rest(&result(0.0, 0.0, vec![a]), &result(0.0, 0.0, vec![a]), eps).is_none());
    assert!(fold_rest(&result(0.0, 0.0, vec![a]), &result(0.01, 0.0, vec![b]), eps).is_none());
    assert!(fold_rest(&result(0.01, 0.0, vec![a]), &result(0.0, 0.0, vec![b]), eps).is_none());
    assert!(fold_rest(&result(0.0, 0.0, vec![a]), &result(0.0, 0.0, Vec::new()), eps).is_none());
    // The other axis is kept: a hit on y stays to be compared.
    let y = hit(Target::Guide, Some(Axis::Y), 3.0, Anchor::Low, Some(8));
    let (o, n) = fold_rest(&result(0.0, 0.2, vec![a, y]), &result(0.0, 0.2, vec![b, y]), eps).unwrap();
    assert_eq!((o.hits.len(), n.hits.len()), (1, 1));
}

#[test]
fn the_edge_of_a_reach_is_a_hit_exactly_a_pull_away_that_the_library_did_not_take() {
    let eps = Eps::NEAR;
    // 6 points over 96 is 0.0625 inch: an alignment exactly that far, against nothing or against a nearer hit of another kind.
    let case = case_of_subject(page_of_nothing(), Subject::Box(Rect { x0: 4.0, y0: 4.0, x1: 5.0, y1: 5.0 }), Vec::new(), 96.0);
    let align = hit(Target::Alignment, Some(Axis::Y), 7.4375, Anchor::Point, Some(7));
    let grid = hit(Target::Grid, Some(Axis::Y), 7.5, Anchor::Point, None);
    let (o, n) = fold_edge(&case, &result(0.0, -0.0625, vec![align]), &result(0.0, -0.03125, vec![grid]), eps).expect("an edge");
    assert!(o.hits.is_empty() && n.hits.is_empty() && o.dy == 0.0 && n.dy == 0.0);
    assert!(fold_edge(&case, &result(0.0, -0.0625, vec![align]), &result(0.0, 0.0, Vec::new()), eps).is_some());
    // Not at the edge (half the pull), or a lattice (pulls from anywhere), or the library's hit is the same, or no nearer: nothing is folded.
    assert!(fold_edge(&case, &result(0.0, -0.03125, vec![align]), &result(0.0, 0.0, Vec::new()), eps).is_none());
    assert!(fold_edge(&case, &result(0.0, -0.0625, vec![grid]), &result(0.0, 0.0, Vec::new()), eps).is_none());
    assert!(fold_edge(&case, &result(0.0, -0.0625, vec![align]), &result(0.0, -0.0625, vec![align]), eps).is_none());
    assert!(fold_edge(&case, &result(0.0, -0.0625, vec![align]), &result(0.0, -0.09, vec![grid]), eps).is_none());
    // The x axis, with the pull of a guide (8 points).
    let guide = hit(Target::Guide, Some(Axis::X), 6.0, Anchor::Low, Some(2));
    assert!(fold_edge(&case, &result(8.0 / 96.0, 0.0, vec![guide]), &result(0.0, 0.0, Vec::new()), eps).is_some());
}

#[test]
fn a_hit_within_the_width_of_a_tolerance_is_a_hit_of_nothing() {
    let a = hit(Target::Spacing, Some(Axis::X), 8.0, Anchor::Middle, Some(9));
    let (o, n) = fold_tiny(&result(0.0, 0.0, Vec::new()), &result(2.0e-8, 0.0, vec![a])).expect("a landing of 2e-8");
    assert!(n.hits.is_empty() && n.dx == 0.0 && o.hits.is_empty());
    // On y as well; 1e-7 is a landing; a hit on an axis that has none to remove folds nothing.
    assert!(fold_tiny(&result(0.0, 0.0, Vec::new()), &result(0.0, -2.0e-8, vec![Hit { axis: Some(Axis::Y), ..a }])).is_some());
    assert!(fold_tiny(&result(0.0, 0.0, Vec::new()), &result(1.0e-7, 0.0, vec![a])).is_none());
    assert!(fold_tiny(&result(0.0, 0.0, Vec::new()), &result(0.0, 0.0, Vec::new())).is_none());
    // A landing of nothing without a hit is not a hit to remove.
    assert!(fold_tiny(&result(1.0e-9, 0.0, Vec::new()), &result(0.0, 0.0, Vec::new())).is_none());
}

#[test]
fn an_unmarked_spacing_hit_is_told_by_the_library_without_its_gap_while_the_other_axis_has_a_hit() {
    let eps = Eps::NEAR;
    let told = Hit { gap: Some(0.5), span: Some((2.0, 2.5)), ..hit(Target::Spacing, Some(Axis::Y), 1.5, Anchor::Middle, Some(22)) };
    let bare = Hit { gap: None, span: None, ..told };
    let x = hit(Target::Spacing, Some(Axis::X), 3.5, Anchor::Middle, Some(11));
    let doc = boxes_page(&[(22, 0.0, 0.0, 1.0, 1.0)]);
    let page = SnapPage::read(&doc, 1).unwrap();
    let case = case_of_subject(doc, Subject::Box(Rect { x0: 3.0, y0: 0.0, x1: 4.0, y1: 1.0 }), Vec::new(), 96.0);
    let (old, new) = (result(0.02, -0.05, vec![x, told]), result(0.02, -0.05, vec![x, bare]));
    let (o, n) = fold_unmarked(&page, &case, &old, &new, eps).expect("unmarked");
    assert!(o.hits[1].gap.is_none() && o.hits[1].span.is_none() && n.hits[1].gap.is_none());
    assert!(compare(&o, &n, eps).is_ok());
    // The library's hit has its gap: nothing to fold. No hit on the other axis: nothing to fold. A different line or landing, or another target: not the same hit.
    assert!(fold_unmarked(&page, &case, &old, &old, eps).is_none());
    assert!(fold_unmarked(&page, &case, &result(0.0, -0.05, vec![told]), &result(0.0, -0.05, vec![bare]), eps).is_none());
    assert!(fold_unmarked(&page, &case, &old, &result(0.02, -0.06, vec![x, bare]), eps).is_none());
    assert!(fold_unmarked(&page, &case, &old, &result(0.02, -0.05, vec![x, Hit { line: 1.6, ..bare }]), eps).is_none());
    assert!(fold_unmarked(&page, &case, &old, &result(0.02, -0.05, vec![x, Hit { target: Target::Alignment, ..bare }]), eps).is_none());
    // The x axis, with a hit on y beside it.
    let told_x = Hit { axis: Some(Axis::X), ..told };
    let bare_x = Hit { axis: Some(Axis::X), ..bare };
    let y = hit(Target::Guide, Some(Axis::Y), 1.0, Anchor::Low, Some(5));
    assert!(fold_unmarked(&page, &case, &result(0.0, 0.02, vec![told_x, y]), &result(0.0, 0.02, vec![bare_x, y]), eps).is_some());
}

#[test]
fn a_spacing_hit_whose_neighbours_left_the_row_is_told_without_its_gap_while_another_pair_holds_the_same_centre() {
    // The one case of the mixed pages that is left in the class `unmarked`, cut down: a mirrored page of two pairs of boxes. The box is centred between 9 and 3
    // (gaps 0.6875, a hit on x) and its top is pulled onto their lower edge (0.0156 down), which takes it out of their row; 5 and 11 lie across its new place, a mirror image of one
    // another about the same centre line, with gaps of 0.844. Today's function tells the gap of 9 and 3 (0.6875), which is not true where the box lands. The library keeps the hit at the
    // same landing and, under the harness's policy (only the marks of what was snapped to), has no gap to tell for the neighbours it named and draws none. The adopted policy tells
    // the gaps the box has (0.844, both marked) and, with its own pulls, lands the box where they hold. When the library tells the gap of the row the landing has under the harness's
    // policy too, the `unmarked` assert of the mixed page fails and the class goes.
    let doc = boxes_page(&[
        (3, 9.7578125, 5.90625, 12.3359375, 7.40625),
        (9, 3.6640625, 5.90625, 6.2421875, 7.40625),
        (5, 4.008058621816709, 4.010559966096738, 6.085691378183291, 6.676940033903262),
        (11, 9.91430862181671, 4.010559966096738, 11.99194137818329, 6.676940033903262),
    ]);
    let page = SnapPage::read(&doc, 1).unwrap();
    let mut s = SnapSettings::none();
    s.snap = true;
    s.alignment_box = true;
    s.dynamic_grid = true;
    s.strength = Strength { points: 5, guides: 10, geometry: 4, extensions: 9, ..Strength::default() };
    let query = Query {
        subject: Subject::Box(Rect { x0: 6.9375, y0: 4.8125, x1: 9.078125, y1: 5.921875 }),
        using: Use::Place,
        moving: &[],
        zoom: 32.0,
        own_points: &[],
    };
    let x_hit = |r: &Snapped| r.hits.iter().find(|h| h.axis == Some(Axis::X)).copied().expect("a hit on x");
    let old = oracle::snap(&doc, &page, &s, &query);
    let today = snap_with(Policy::TODAY, &doc, &page, &s, &query);
    assert_eq!((old.dx, old.dy), (-0.0078125, -0.015625));
    assert_eq!((today.dx, today.dy), (-0.0078125, -0.015625), "the same landing");
    assert_eq!(x_hit(&old).gap, Some(0.6875), "today's claim is the gap of 9 and 3");
    let held = x_hit(&today);
    assert_eq!((held.target, held.line), (Target::Spacing, 8.0));
    assert!(held.gap.is_none() && held.span.is_none(), "{held:?}");
    assert!(!format!("{:?}", today.marks).contains("Gap {"), "no gap is drawn: {:?}", today.marks);
    // The adopted policy tells the gaps of the row the box has.
    let adopted = snap_with(Policy::ADOPTED, &doc, &page, &s, &query);
    let told = x_hit(&adopted);
    assert!(told.gap.is_some_and(|g| (g - 0.844).abs() < 1.0e-3), "{told:?}");
    assert_eq!(format!("{:?}", adopted.marks).matches("Gap {").count(), 2, "{:?}", adopted.marks);
    // Both go through the harness as the class: the folded old hit and the library's are one.
    let eps = Eps::NEAR;
    let case = case_of_subject(doc, query.subject, Vec::new(), 32.0);
    assert!(fold_unmarked(&page, &case, &old, &today, eps).is_some_and(|(o, n)| compare(&o, &n, eps).is_ok()));
}

#[test]
fn a_pull_beside_a_hit_at_rest_is_made_by_both() {
    // The reproducer of the difference the library had until a later change (it was the harness's `rest_pair` class), found by the coarse pages: two shapes, a box whose middle lies on
    // the page's bottom margin and on the lower edge of shape 2, and a line of shape 2 that is 1.75 inch from the box's right side at 3 points to the inch (a pull of 2).
    // Under the row rule of before the library a third shape in the row made an equal gap as near as that line, the library kept the gap alone for the direction, dropped it where
    // the box lands and did not try the line; it now tries the next place of the direction, so both make the pull, as they do 0.25 inch nearer (a pull of 1.5).
    let mut s = SnapSettings::none();
    s.snap = true;
    s.guides = true;
    s.alignment_box = true;
    s.dynamic_grid = true;
    let at = |x: f64| {
        let doc = boxes_page(&[(1, 10.25, 0.5, 12.75, 2.5), (2, x, 0.25, x + 0.5, 1.75)]);
        let page = SnapPage::read(&doc, 1).unwrap();
        let query =
            Query { subject: Subject::Box(Rect { x0: 3.0, y0: 0.0, x1: 4.5, y1: 0.5 }), using: Use::Place, moving: &[], zoom: 3.0, own_points: &[] };
        let old = oracle::snap(&doc, &page, &s, &query);
        let new = snap_with(Policy::TODAY, &doc, &page, &s, &query);
        let x = |r: &Snapped| r.hits.iter().any(|h| h.axis == Some(Axis::X));
        (x(&old), x(&new))
    };
    assert_eq!(at(6.25), (true, true), "both pull 1.75 inch");
    assert_eq!(at(6.0), (true, true), "a pull of 1.5 inch is made by both");
}

#[test]
fn every_line_a_call_snapped_to_has_its_mark_at_that_line_whichever_of_its_queries_found_it() {
    // The invariant of the design, on the adopted policy: a call that asks the library again (the other axis alone, each side of a resize) told the hits of the later queries and
    // the marks of the first. On 6,000 seeded pages 128 of 1,991 alignments, guides and margins snapped to had no mark on their line (found by the new pages of L9).
    use super::SnapMark;
    let mut rng = Rng(31);
    let (mut checked, mut missing, mut first) = (0usize, 0usize, None::<String>);
    for _ in 0..6_000 {
        let case = case_of(&mut rng, &Config { masters: true, ..PLAIN });
        let Some(page) = SnapPage::read(&case.doc, 1) else { continue };
        let query = Query { subject: case.subject, using: case.using, moving: &case.moving, zoom: case.zoom, own_points: &case.own };
        let out = snap_with(Policy::ADOPTED, &case.doc, &page, &case.settings, &query);
        for h in out.hits.iter().filter(|h| matches!(h.target, Target::Alignment | Target::Guide | Target::PageMargin)) {
            let Some(axis) = h.axis else { continue };
            checked += 1;
            let drawn =
                out.marks.iter().any(|m| matches!(m, SnapMark::Line { axis: a, at, hit: true, .. } if *a == axis && (at - h.line).abs() < 1e-6));
            if !drawn {
                missing += 1;
                first.get_or_insert_with(|| format!("{h:?} of {:?}: marks {:?}", case.subject, out.marks));
            }
        }
    }
    assert!(checked > 1_500 && missing == 0, "{missing} of {checked} snapped lines have no mark, the first: {first:?}");
}

#[test]
fn a_crowd_of_twelve_shapes_with_three_own_places_is_no_cut_in_either_and_seventeen_are_a_cut_in_both() {
    // The review's M2: 12 shapes round the box, 3 places of the moving shapes' own, zoom 5.5. The old function read the twelve outlines once and said nothing; the adapter asked
    // the library twice (the outlines apart) and counted the twelve against its sixteen twice, and said `truncated`. Sixteen are the cap of the old function, so 17 are cut in both.
    for (n, cut) in [(12u32, false), (16, false), (17, true)] {
        let shapes: Vec<(u32, f64, f64, f64, f64)> = (0..n)
            .map(|i| {
                let (x, y) = (8.0 + f64::from(i % 5) * 0.3, 8.0 + f64::from(i / 5) * 0.3);
                (i + 1, x, y, x + 0.2, y + 0.2)
            })
            .collect();
        let doc = boxes_page(&shapes);
        let page = SnapPage::read(&doc, 1).unwrap();
        let mut s = SnapSettings::none();
        s.snap = true;
        s.alignment_box = true;
        s.dynamic_grid = true;
        s.snap_vertices = true;
        s.snap_geometry = true;
        s.intersections = true;
        let own = [(8.5, 8.5), (8.7, 8.7), (9.0, 9.0)];
        let query =
            Query { subject: Subject::Box(Rect { x0: 8.2, y0: 8.2, x1: 9.2, y1: 9.2 }), using: Use::Place, moving: &[], zoom: 5.5, own_points: &own };
        let old = oracle::snap(&doc, &page, &s, &query);
        let new = snap_with(Policy::TODAY, &doc, &page, &s, &query);
        assert_eq!((old.truncated, new.truncated), (cut, cut), "{n} shapes: old {old:?} new {new:?}");
        assert!(judge(&old, &new, Eps::NEAR, exact()).is_ok(), "{n} shapes");
    }
}
