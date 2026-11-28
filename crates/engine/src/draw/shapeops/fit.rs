//! Fit Curve's refit (SHAPEOPS-FITCURVE): fewer segments within a tolerance, for the runs of a shape.
//!
//! **What the library gives, and what is built of it.** `craft_pathops::simplify_with` fits cubics (and lines where a run is straight) to a path within a tolerance, with
//! the ends of each run kept and a bound on its work (`FIT_WORK`, over every run of the call; `MAX_ELEMENTS` for what it makes): it stops fitting and leaves the rest as it
//! is. It makes no circular arcs, and the flags are not its options. So:
//!
//! * **`Args::DO_CIRCLES` (2)**: before the library sees a run, [`arcs`] looks in it for stretches that a circular arc follows within the tolerance, and each is written as one
//!   `ArcTo`. A stretch is at least five points (four lines, or any cubic), at most 256 segments and half a circle (the bow of an `ArcTo` can be more, but its sign and a
//!   sweep of more than 180 degrees are what readers differ on), and its bow is more than the tolerance (a stretch that a straight line follows is the library's:
//!   a line). The arc passes through the stretch's two ends and a point halfway along it, and every point of the stretch is within the tolerance of it, counting the
//!   distance a chord between two points leaves between them. What the arcs leave is fitted by the library as runs of their own, from arc to arc.
//! * **`Args::ABRUPT` (4)**: the library keeps a corner where the path turns by more than 30 degrees. With the flag that is the angle; without it a spline may go across a
//!   corner as far as the tolerance allows (the angle is 180, which nothing exceeds).
//! * **`Args::PERIODIC` (1)**: the library fits a closed run, one that is given with its close, as a loop that starts at its first corner (or where it started if it has
//!   none), so the seam is not forced to be a vertex. With the flag a closed run that has no arc in it is given that way. Without it the run is given as an open path that ends
//!   where it started, whose two ends are vertices.
//!
//! A run is **replaced only when that makes it smaller**: fewer segments, or with the circles flag, some arc. A run the fit cannot make smaller keeps its rows exactly (an
//! `ArcTo` is not swapped for a cubic that needs the same count), and a tolerance of 0 asks for an exact match, which leaves every run as it is.
//!
//! **One call of the library for the whole command.** Every stretch of every run of every shape goes to `simplify_with` as a subpath of **one path**, so that the library's own
//! counts (`FIT_WORK`, the elements it may make) bound the command and not each call of many (a call for each of 4,096 circles, each with a fresh `FIT_WORK`, would be 4,096
//! times the work). What comes back is a path of the same subpaths in the same order, each given back as it was when the fit does not make it smaller ([`align`] hands each
//! result to the stretch it fits, and a stretch the library dropped or gave up on stays as it was). Looking for arcs charges each point it looks at to `steps`, before it is
//! looked at, and the command is refused in words past [`STEPS`].

use craft_pathops::{SimplifyOptions, simplify_with};
use kurbo::{BezPath, Point};

use super::Args;
use super::MERGE;
use super::runs::{Run, Seg, arc_circle, bow_through, runs_of};
use crate::draw::Problem;

/// The most points a Fit Curve looks at while it looks for arcs (SHAPEOPS-CAPS): each is some tens of nanoseconds.
pub(super) const STEPS: u64 = 40_000_000;

/// The most segments one arc stands for.
const MAX_ARC_SEGMENTS: usize = 256;

/// The fewest points an arc is looked for in: four lines are five points, and a cubic is nine.
const MIN_POINTS: usize = 5;

/// How many points a cubic is looked at in.
const SAMPLES: usize = 8;

/// The share of the tolerance an arc may use: the rest is for the cubics between the points looked at.
const ARC_SHARE: f64 = 0.95;

/// The turn at which the library keeps a corner when the flag asks for it.
const CORNER_DEGREES: f64 = 30.0;

/// What a fit is asked for.
#[derive(Clone, Copy, Debug)]
pub(super) struct Options {
    pub tolerance: f64,
    pub flags: u32,
}

impl Options {
    fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
}

fn charge(steps: &mut u64, n: usize) -> Result<(), Problem> {
    *steps = steps.checked_sub(n as u64).ok_or_else(|| {
        Problem::failed(format!(
            "Fit Curve would look at more than {STEPS} points for circles: the shapes are too big for that flag, and the command is refused"
        ))
    })?;
    Ok(())
}

/// The points a segment is looked at in, after `from`: its end for a line, eight for a cubic.
fn sample(from: Point, seg: &Seg) -> Vec<Point> {
    match *seg {
        Seg::Line(p) => vec![p],
        Seg::Cubic(a, b, p) => {
            let c = kurbo::CubicBez::new(from, a, b, p);
            (1..=SAMPLES).map(|k| kurbo::ParamCurve::eval(&c, k as f64 / SAMPLES as f64)).collect()
        }
        // The runs the reading gives have no arcs: one is no part of an arc.
        Seg::Arc { to, .. } => vec![to],
    }
}

fn sagitta(chord: f64, radius: f64) -> Option<f64> {
    let half = chord / 2.0;
    (half <= radius).then(|| radius - (radius * radius - half * half).sqrt())
}

/// What looking for an arc through some points found.
#[derive(Debug, PartialEq)]
enum Looked {
    /// The arc through them bows no more than the tolerance: a line follows the points as well, and more points may bend it further.
    Flat,
    /// No arc follows the points: not within the tolerance, not the way round, or more than half a circle. More points will not make one.
    Not,
    /// The arc, as an `ArcTo`.
    Arc(Seg),
}

/// The arc through the first of `points`, the last and the one halfway along them, if every point is within the tolerance of it and it turns no more than half a circle
/// the one way.
///
/// Telling that the arc is flat costs a few steps and is not charged; checking the points against an arc that is not costs one for each point, and is charged to `steps`
/// first.
fn arc_through(points: &[Point], tolerance: f64, steps: &mut u64) -> Result<Looked, Problem> {
    let (Some(&first), Some(&last), Some(&middle)) = (points.first(), points.last(), points.get(points.len() / 2)) else { return Ok(Looked::Not) };
    // No chord (the first point is the last) divides by nothing below: a number that is not one, which is not within the tolerance, and `bow_through` gives no arc.
    let chord = last - first;
    let reach = chord.hypot();
    // The middle point's distance from the chord: a stretch whose middle is within the tolerance of its chord is flat, told without the circle through the three points,
    // which for three that are nearly in a line is a circle of a huge radius whose numbers are noise.
    if (middle - first).cross(chord).abs() / reach <= tolerance {
        return Ok(Looked::Flat);
    }
    let Some(bow) = bow_through(first, last, middle) else { return Ok(Looked::Not) };
    if bow.abs() <= tolerance {
        return Ok(Looked::Flat);
    }
    charge(steps, points.len())?;
    Ok(match round_the_arc(points, tolerance, first, last, bow) {
        Some(arc) => Looked::Arc(arc),
        None => Looked::Not,
    })
}

/// The checks of [`arc_through`] on the arc `first`, `last`, `bow`.
fn round_the_arc(points: &[Point], tolerance: f64, first: Point, last: Point, bow: f64) -> Option<Seg> {
    let (centre, radius, _, sweep) = arc_circle(first, last, bow)?;
    if sweep.abs() > std::f64::consts::PI + 1.0e-9 {
        return None;
    }
    let slack = 2.0 * tolerance / radius;
    // The point looked at before this one, how far it is from the circle and the angle it is at round the circle's centre.
    let mut before: Option<(Point, f64, f64)> = None;
    for &p in points {
        let off = ((p - centre).hypot() - radius).abs();
        let angle = (p.y - centre.y).atan2(p.x - centre.x);
        if let Some((q, q_off, from)) = before {
            // The farthest the chord from one point to the next is from the arc: how far the points are from the circle, and the bulge of the arc between them.
            if off.max(q_off) + sagitta(p.distance(q), radius)? > ARC_SHARE * tolerance {
                return None;
            }
            // The way round is the sweep's, within what the tolerance allows for a point that is off the circle.
            let mut delta = angle - from;
            while delta > std::f64::consts::PI {
                delta -= std::f64::consts::TAU;
            }
            while delta <= -std::f64::consts::PI {
                delta += std::f64::consts::TAU;
            }
            if delta * sweep.signum() < -slack {
                return None;
            }
        }
        before = Some((p, off, angle));
    }
    // The points keep to the arc's side of the circle and run the way it runs, each step small (a chord bulges by more than the tolerance past a small angle): what they
    // turn through is the arc's sweep, and no more.
    Some(Seg::Arc { to: last, bow })
}

/// A stretch of a run, by the numbers of its segments, kept as it is for the library or made an arc.
#[derive(Debug, PartialEq)]
enum Item {
    Plain { from: usize, to: usize },
    Arc { from: usize, to: usize, seg: Seg },
}

/// What looking for an arc from one segment of a run found.
struct Found {
    /// The longest arc that starts there, if there is one: where it ends (the number of the segment after it) and the `ArcTo`.
    best: Option<(usize, Seg)>,
    /// How many segments were looked at.
    looked_at: usize,
    /// Whether every stretch looked at was flat (a line follows it as well as an arc), to the end of what was looked at.
    flat_throughout: bool,
}

fn longest_arc(run: &Run, starts: &[Point], pos: usize, tolerance: f64, steps: &mut u64) -> Result<Found, Problem> {
    let mut points: Vec<Point> = vec![*starts.get(pos).ok_or_else(|| Problem::failed("a run lost a point"))?];
    let mut found = Found { best: None, looked_at: 0, flat_throughout: true };
    let end = run.segs.len().min(pos.saturating_add(MAX_ARC_SEGMENTS));
    for k in pos..end {
        let (Some(seg), Some(from)) = (run.segs.get(k), starts.get(k)) else { break };
        if matches!(seg, Seg::Arc { .. }) {
            found.flat_throughout = false;
            break;
        }
        let more = sample(*from, seg);
        charge(steps, more.len())?;
        points.extend(more);
        found.looked_at += 1;
        if points.len() < MIN_POINTS {
            continue;
        }
        match arc_through(&points, tolerance, steps)? {
            Looked::Arc(arc) => {
                found.best = Some((k + 1, arc));
                found.flat_throughout = false;
            }
            Looked::Flat => {}
            Looked::Not => {
                found.flat_throughout = false;
                break;
            }
        }
    }
    Ok(found)
}

/// The run cut into stretches that arcs follow and stretches that do not.
fn arcs(run: &Run, tolerance: f64, steps: &mut u64) -> Result<Vec<Item>, Problem> {
    let m = run.segs.len();
    let mut starts: Vec<Point> = Vec::with_capacity(m + 1);
    starts.push(run.start);
    starts.extend(run.segs.iter().map(Seg::end));
    let (mut items, mut plain_from, mut pos) = (Vec::new(), 0_usize, 0_usize);
    while pos < m {
        charge(steps, 1)?;
        let found = longest_arc(run, &starts, pos, tolerance, steps)?;
        match found.best {
            Some((to, seg)) => {
                if plain_from < pos {
                    items.push(Item::Plain { from: plain_from, to: pos });
                }
                items.push(Item::Arc { from: pos, to, seg });
                (pos, plain_from) = (to, to);
            }
            // A stretch that is flat all the way is flat from the places just after it too: the next place looked at is half way along it, so that a straight run of
            // a hundred thousand segments costs a few hundred looks of 256 and not a hundred thousand.
            None if found.flat_throughout => pos = pos.saturating_add((found.looked_at / 2).max(1)),
            None => pos = pos.saturating_add(1),
        }
    }
    if plain_from < m {
        items.push(Item::Plain { from: plain_from, to: m });
    }
    Ok(items)
}

/// One stretch of a run that goes to the library: where it came from and what it is.
struct Job {
    /// The run it is a stretch of, and the place among that run's items.
    run: usize,
    item: usize,
    /// The stretch as a run of its own: the segments from `from` to `to` of the run, starting where the one before them ended.
    sub: Run,
    /// Whether the stretch is the whole of a closed run that is fitted as a loop (`Args::PERIODIC`): given to the library with its close.
    closed_loop: bool,
}

/// The stretches to fit: one for each plain item of two segments or more (a stretch of one segment cannot be made of fewer).
fn jobs_of(runs: &[Run], items: &[Vec<Item>], o: &Options) -> Vec<Job> {
    let mut jobs = Vec::new();
    for (k, (run, its)) in runs.iter().zip(items).enumerate() {
        let loop_of_all = its.len() == 1 && run.closed() && o.has(Args::PERIODIC);
        for (n, item) in its.iter().enumerate() {
            let Item::Plain { from, to } = item else { continue };
            if to.saturating_sub(*from) < 2 {
                continue;
            }
            let start = if *from == 0 { run.start } else { run.segs.get(from - 1).map_or(run.start, Seg::end) };
            let segs = run.segs.get(*from..*to).unwrap_or_default().to_vec();
            jobs.push(Job { run: k, item: n, sub: Run { start, segs }, closed_loop: loop_of_all });
        }
    }
    jobs
}

/// Whether the library's `result` is the answer for the stretch `job`: an open stretch is given back with its two ends, and a loop as a closed run (which starts at its first
/// corner, so its start is not the stretch's).
fn is_answer(job: &Job, result: &Run) -> bool {
    if job.closed_loop { result.closed() } else { result.start.distance(job.sub.start) <= MERGE && result.end().distance(job.sub.end()) <= MERGE }
}

/// Hands each result of the library to the stretch it fits, in order: the library gives the subpaths back in the order it was given them, a stretch it dropped has no
/// result (it stays as it was), and a result that is not the next stretch's is not taken for it. `None` for a stretch with no result.
fn align(jobs: &[Job], results: Vec<Run>) -> Vec<Option<Run>> {
    let mut next = results.into_iter().peekable();
    jobs.iter()
        .map(|job| match next.peek() {
            Some(result) if is_answer(job, result) => next.next(),
            _ => None,
        })
        .collect()
}

/// The library's fit of every stretch at once, each given as a subpath of one path: what it made of each (`None` where it gave nothing for it).
fn library(jobs: &[Job], o: &Options) -> Vec<Option<Run>> {
    if jobs.is_empty() {
        return Vec::new();
    }
    let mut all = BezPath::new();
    for job in jobs {
        all.extend(job.sub.path().elements().iter().copied());
        // A loop is given with its close: the library then fits it as a loop, and an open path that ends where it began as a path whose two ends are vertices.
        if job.closed_loop {
            all.close_path();
        }
    }
    let corner = if o.has(Args::ABRUPT) { CORNER_DEGREES } else { 180.0 };
    let opts = SimplifyOptions::default().tolerance(o.tolerance).corner_angle_deg(corner);
    align(jobs, runs_of(&simplify_with(&all, &opts)))
}

/// The runs fitted, each `Some` where the fit made it smaller. `steps` is the budget of the points looked at for arcs; the library's own work is its `FIT_WORK` for the
/// whole call.
pub(super) fn fit(runs: &[Run], o: &Options, steps: &mut u64) -> Result<Vec<Option<Run>>, Problem> {
    if o.tolerance <= 0.0 {
        return Ok(vec![None; runs.len()]);
    }
    let mut items: Vec<Vec<Item>> = Vec::with_capacity(runs.len());
    for run in runs {
        items.push(if run.segs.is_empty() {
            Vec::new()
        } else if o.has(Args::DO_CIRCLES) {
            arcs(run, o.tolerance, steps)?
        } else {
            vec![Item::Plain { from: 0, to: run.segs.len() }]
        });
    }
    let jobs = jobs_of(runs, &items, o);
    let mut results: Vec<Option<Run>> = library(&jobs, o);
    // The stretches' answers by the run and item they are for.
    let mut by_place: std::collections::HashMap<(usize, usize), Option<Run>> = std::collections::HashMap::with_capacity(jobs.len());
    for (job, answer) in jobs.iter().zip(results.drain(..)) {
        by_place.insert((job.run, job.item), answer);
    }
    let mut out = Vec::with_capacity(runs.len());
    for (k, (run, its)) in runs.iter().zip(&items).enumerate() {
        let loop_of_all = its.len() == 1 && run.closed() && o.has(Args::PERIODIC);
        let mut fitted = Run { start: run.start, segs: Vec::new() };
        let mut any_arc = false;
        for (n, item) in its.iter().enumerate() {
            match item {
                Item::Arc { seg, .. } => {
                    fitted.segs.push(*seg);
                    any_arc = true;
                }
                Item::Plain { from, to } => {
                    let original = run.segs.get(*from..*to).unwrap_or_default();
                    match by_place.remove(&(k, n)) {
                        Some(Some(better)) if better.segs.len() < original.len() => {
                            if loop_of_all {
                                fitted.start = better.start;
                            }
                            fitted.segs.extend(better.segs);
                        }
                        _ => fitted.segs.extend(original.iter().copied()),
                    }
                }
            }
        }
        let smaller = fitted.segs.len() < run.segs.len() || any_arc;
        out.push((smaller && fitted.segs != run.segs).then_some(fitted));
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    /// The vertices of a circle of radius 1 at every `degrees`, as a closed run of lines.
    fn polygon_of_circle(degrees: f64) -> Run {
        let n = (360.0 / degrees).round() as usize;
        let at = |i: usize| {
            let a = (i % n) as f64 * degrees.to_radians();
            p(a.cos(), a.sin())
        };
        Run { start: at(0), segs: (1..=n).map(|i| Seg::Line(at(i))).collect() }
    }

    fn options(tolerance: f64, flags: u32) -> Options {
        Options { tolerance, flags }
    }

    /// The fit of one run, as the command makes it.
    fn fit_one(run: &Run, o: &Options) -> Option<Run> {
        let mut steps = STEPS;
        fit(std::slice::from_ref(run), o, &mut steps).unwrap().pop().flatten()
    }

    /// The farthest a point of `a` is from the path `b`, by sampling.
    fn stray(a: &Run, b: &Run) -> f64 {
        let (pa, pb) = (a.path(), b.path());
        let mut worst = 0.0_f64;
        for seg in pa.segments() {
            for k in 0..=40 {
                let at = kurbo::ParamCurve::eval(&seg, f64::from(k) / 40.0);
                let d = pb.segments().map(|s| kurbo::ParamCurveNearest::nearest(&s, at, 1e-9).distance_sq.sqrt()).fold(f64::MAX, f64::min);
                worst = worst.max(d);
            }
        }
        worst
    }

    /// Forty-one points of a quarter of a circle of radius 10 (every 2.25 degrees, 0.0393 radians), the eleventh taken back along the circle by `back` radians.
    fn quarter_with_a_point_taken_back(back: f64) -> Vec<Point> {
        (0..=40)
            .map(|i| {
                let a = std::f64::consts::FRAC_PI_2 * f64::from(i) / 40.0 - if i == 10 { back } else { 0.0 };
                p(10.0 * a.cos(), 10.0 * a.sin())
            })
            .collect()
    }

    /// A cubic that no fit can make smaller and that is not circular: an S.
    fn s_curve(from: Point) -> Seg {
        Seg::Cubic(from + kurbo::Vec2::new(1.0, 2.0), from + kurbo::Vec2::new(2.0, -2.0), from + kurbo::Vec2::new(3.0, 0.0))
    }

    #[test]
    fn a_circle_of_thirty_six_lines_is_two_arcs_within_the_tolerance_with_the_circles_flag() {
        // V23.
        let circle = polygon_of_circle(10.0);
        let fitted = fit_one(&circle, &options(0.02, Args::DO_CIRCLES)).expect("fewer segments");
        assert!(fitted.segs.len() < 36);
        assert_eq!(fitted.segs.len(), 2, "two half circles");
        assert!(fitted.segs.iter().all(|s| matches!(s, Seg::Arc { .. })), "circular arcs and no spline");
        assert!(fitted.closed(), "it still ends where it started");
        assert!(stray(&fitted, &circle) <= 0.02, "{}", stray(&fitted, &circle));
        assert!(stray(&circle, &fitted) <= 0.02, "{}", stray(&circle, &fitted));
    }

    #[test]
    fn a_tolerance_of_nothing_leaves_every_run_as_it_is() {
        let circle = polygon_of_circle(10.0);
        assert!(fit_one(&circle, &options(0.0, Args::DO_CIRCLES)).is_none());
        assert!(fit_one(&circle, &options(0.0, 0)).is_none());
    }

    #[test]
    fn without_the_circles_flag_the_circle_is_splines_and_no_arcs() {
        let circle = polygon_of_circle(10.0);
        let fitted = fit_one(&circle, &options(0.02, 0)).expect("fewer segments");
        assert!(fitted.segs.len() < 36 && fitted.segs.iter().all(|s| matches!(s, Seg::Cubic(..))), "{:?}", fitted.segs);
        assert!(stray(&fitted, &circle) <= 0.02 && stray(&circle, &fitted) <= 0.02);
    }

    #[test]
    fn a_straight_run_is_one_line_and_a_run_the_fit_cannot_make_smaller_is_left_as_it_is() {
        let straight = Run { start: p(0.0, 0.0), segs: (1..=50).map(|i| Seg::Line(p(f64::from(i) * 0.1, f64::from(i) * 0.05))).collect() };
        let fitted = fit_one(&straight, &options(0.01, 0)).unwrap();
        assert_eq!(fitted.segs.len(), 1);
        let square =
            Run { start: p(0.0, 0.0), segs: vec![Seg::Line(p(2.0, 0.0)), Seg::Line(p(2.0, 2.0)), Seg::Line(p(0.0, 2.0)), Seg::Line(p(0.0, 0.0))] };
        assert!(fit_one(&square, &options(0.02, 0)).is_none());
        assert!(fit_one(&square, &options(0.02, Args::DO_CIRCLES)).is_none(), "four long chords are no arc");
    }

    #[test]
    fn a_stretch_of_a_run_that_is_circular_is_an_arc_and_the_rest_is_fitted_from_arc_to_arc() {
        // A line, a quarter of a circle of radius 5 (30 lines), another line: the quarter is one arc and the lines stay lines.
        let mut segs = vec![Seg::Line(p(5.0, -3.0))];
        let n = 30;
        for i in 1..=n {
            let a = std::f64::consts::FRAC_PI_2 * f64::from(i) / f64::from(n);
            segs.push(Seg::Line(p(5.0 * a.cos(), 5.0 * a.sin())));
        }
        segs.push(Seg::Line(p(-4.0, 8.0)));
        let run = Run { start: p(5.0, -8.0), segs };
        let fitted = fit_one(&run, &options(0.01, Args::DO_CIRCLES)).unwrap();
        let arcs = fitted.segs.iter().filter(|s| matches!(s, Seg::Arc { .. })).count();
        assert_eq!(arcs, 1, "{:?}", fitted.segs);
        assert!(fitted.segs.len() <= 4, "{:?}", fitted.segs);
        assert!(stray(&fitted, &run) <= 0.01 && stray(&run, &fitted) <= 0.01);
    }

    #[test]
    fn a_stretch_after_an_arc_is_fitted_from_where_the_arc_ends_and_keeps_its_corner() {
        // A quarter of a circle of radius 5 (30 lines) from (5, 0) to (0, 5), then a long line to (-10, 5), a step of 0.005 up and a turn up it, in twelve steps to (-10, 11): the library is given
        // the stretch from the arc's end, so the corner at (-10, 5) stays within the tolerance of the fit. Given it from a later point of the stretch it would cut the corner
        // (a line from the arc's end to (-10, 11)), many times the tolerance from the run.
        let n = 30;
        let mut segs: Vec<Seg> = (1..=n)
            .map(|i| {
                let a = std::f64::consts::FRAC_PI_2 * f64::from(i) / f64::from(n);
                Seg::Line(p(5.0 * a.cos(), 5.0 * a.sin()))
            })
            .collect();
        // The corner is followed by a step of 0.005 up: a start taken at the end of either of the stretch's first two segments is within the tolerance of the corner and
        // so does not hold it.
        segs.push(Seg::Line(p(-10.0, 5.0)));
        segs.push(Seg::Line(p(-10.0, 5.005)));
        segs.extend((1..=12).map(|i| Seg::Line(p(-10.0, 5.005 + f64::from(i) * 0.5))));
        let run = Run { start: p(5.0, 0.0), segs };
        let fitted = fit_one(&run, &options(0.01, Args::DO_CIRCLES)).unwrap();
        assert!(matches!(fitted.segs.first(), Some(Seg::Arc { .. })), "{:?}", fitted.segs);
        assert_eq!(fitted.segs.iter().filter(|g| matches!(g, Seg::Arc { .. })).count(), 1, "{:?}", fitted.segs);
        assert!(fitted.segs.len() < 20, "the stretch is fitted by fewer segments: {:?}", fitted.segs);
        assert!(stray(&fitted, &run) <= 0.01 && stray(&run, &fitted) <= 0.01, "{} and {}", stray(&fitted, &run), stray(&run, &fitted));
    }

    #[test]
    fn a_stretch_that_a_line_follows_is_not_an_arc_and_one_the_tolerance_does_not_allow_is_not_one_either() {
        // A very flat arc: bow under the tolerance.
        let flat = Run {
            start: p(0.0, 0.0),
            segs: (1..=8)
                .map(|i| {
                    let x = f64::from(i);
                    Seg::Line(p(x, 0.0001 * x * (8.0 - x)))
                })
                .collect(),
        };
        let fitted = fit_one(&flat, &options(0.01, Args::DO_CIRCLES)).unwrap();
        assert!(fitted.segs.iter().all(|s| !matches!(s, Seg::Arc { .. })), "a bow under the tolerance is left to the library: {:?}", fitted.segs);
        // Points that wander more than the tolerance from any circle.
        let wavy = Run {
            start: p(0.0, 0.0),
            segs: (1..=12)
                .map(|i| {
                    let x = f64::from(i);
                    Seg::Line(p(x, if i % 2 == 0 { 1.0 } else { 0.0 }))
                })
                .collect(),
        };
        let fitted = fit_one(&wavy, &options(0.05, Args::DO_CIRCLES));
        assert!(fitted.is_none_or(|f| f.segs.iter().all(|s| !matches!(s, Seg::Arc { .. }))));
    }

    #[test]
    fn a_cubic_that_is_a_quarter_circle_becomes_an_arc_with_the_circles_flag_and_stays_a_cubic_without() {
        let k = 0.552_284_749_830_793_4;
        let run = Run { start: p(1.0, 0.0), segs: vec![Seg::Cubic(p(1.0, k), p(k, 1.0), p(0.0, 1.0))] };
        let fitted = fit_one(&run, &options(0.01, Args::DO_CIRCLES)).unwrap();
        assert_eq!(fitted.segs.len(), 1);
        let Seg::Arc { to, bow } = fitted.segs[0] else { panic!("an arc: {:?}", fitted.segs) };
        assert_eq!(to, p(0.0, 1.0));
        assert!((bow - (1.0 - std::f64::consts::FRAC_1_SQRT_2)).abs() < 1e-3, "{bow}: a quarter circle of radius 1 bows by 1 - cos 45");
        assert!(bow > 0.0, "anticlockwise is a positive bow");
        assert!(fit_one(&run, &options(0.01, 0)).is_none(), "one cubic is one cubic: not smaller");
    }

    #[test]
    fn the_abrupt_flag_keeps_a_corner_and_without_it_one_spline_goes_across_it_as_far_as_the_tolerance_allows() {
        // An L: ten points along one side of a unit, ten up the other, a turn of 90 degrees, and a tolerance of 0.2 that one cubic over the corner stays within.
        let mut segs = Vec::new();
        for i in 1..=10 {
            segs.push(Seg::Line(p(f64::from(i) * 0.1, 0.0)));
        }
        for i in 1..=10 {
            segs.push(Seg::Line(p(1.0, f64::from(i) * 0.1)));
        }
        let run = Run { start: p(0.0, 0.0), segs };
        let with = fit_one(&run, &options(0.2, Args::ABRUPT)).unwrap();
        assert_eq!(with.segs, vec![Seg::Line(p(1.0, 0.0)), Seg::Line(p(1.0, 1.0))], "the corner is kept: two lines");
        let without = fit_one(&run, &options(0.2, 0)).unwrap();
        assert_eq!(without.segs.len(), 1, "one spline across the corner");
        assert!(matches!(without.segs[0], Seg::Cubic(..)));
        assert!(stray(&without, &run) <= 0.2 && stray(&run, &without) <= 0.2, "{} {}", stray(&without, &run), stray(&run, &without));
        assert_eq!((without.start, without.end()), (run.start, run.end()), "its ends are the run's");
    }

    #[test]
    fn a_loop_the_library_gives_as_many_or_more_segments_than_the_run_had_is_not_a_fit() {
        // A circle of radius 1 as two half arcs and as four quarter arcs, fitted as a loop: the library gives cubics for them, as many as four of them. A run that is
        // already as small as the library makes it (arcs the library has no kind for) is left as it was and not rewritten as cubics.
        let (half, quarter) = (1.0, 1.0 - std::f64::consts::FRAC_1_SQRT_2);
        let two = Run { start: p(1.0, 0.0), segs: vec![Seg::Arc { to: p(-1.0, 0.0), bow: half }, Seg::Arc { to: p(1.0, 0.0), bow: half }] };
        let four = Run {
            start: p(1.0, 0.0),
            segs: vec![
                Seg::Arc { to: p(0.0, 1.0), bow: quarter },
                Seg::Arc { to: p(-1.0, 0.0), bow: quarter },
                Seg::Arc { to: p(0.0, -1.0), bow: quarter },
                Seg::Arc { to: p(1.0, 0.0), bow: quarter },
            ],
        };
        for (name, run) in [("two", &two), ("four", &four)] {
            for flags in [Args::PERIODIC, Args::PERIODIC | Args::ABRUPT] {
                assert_eq!(fit_one(run, &options(0.01, flags)), None, "{name} arcs, flags {flags}");
            }
        }
    }

    #[test]
    fn a_closed_run_fitted_as_a_loop_needs_no_vertex_at_its_seam_and_without_the_periodic_flag_the_seam_is_one() {
        // A pie slice: a quarter of a circle of radius 4 (40 lines) and the two radii, started in the middle of the arc.
        let n = 40_usize;
        let at = |i: usize| {
            let a = std::f64::consts::FRAC_PI_2 * (i as f64 / n as f64);
            p(a.cos() * 4.0, a.sin() * 4.0)
        };
        // From the arc's middle (point 20) to its end, then the radius to the centre, the other radius out to the arc's start, and the arc up to the middle.
        let mut segs: Vec<Seg> = (21..=n).map(|i| Seg::Line(at(i))).collect();
        segs.push(Seg::Line(p(0.0, 0.0)));
        segs.push(Seg::Line(at(0)));
        segs.extend((1..=20).map(|i| Seg::Line(at(i))));
        let run = Run { start: at(20), segs };
        assert!(run.closed());
        let periodic = fit_one(&run, &options(0.01, Args::PERIODIC | Args::ABRUPT)).unwrap();
        let rigid = fit_one(&run, &options(0.01, Args::ABRUPT)).unwrap();
        assert!(periodic.closed() && rigid.closed());
        assert_eq!(periodic.segs.len(), 3, "the arc, and the two radii: {:?}", periodic.segs);
        assert_eq!(rigid.segs.len(), 4, "the arc is cut at the seam, in the middle of it: {:?}", rigid.segs);
        assert_eq!(rigid.start, run.start, "the seam stays where it was");
        assert!(stray(&periodic, &run) <= 0.01 && stray(&run, &periodic) <= 0.01);
        assert!(stray(&rigid, &run) <= 0.01 && stray(&run, &rigid) <= 0.01);
    }

    #[test]
    fn the_bulge_of_a_chord_on_a_circle_is_the_radius_less_the_chords_distance_from_the_centre() {
        assert_eq!(sagitta(2.0, 1.0), Some(1.0), "a diameter bulges by the radius");
        assert!((sagitta(1.0, 1.0).unwrap() - (1.0 - 0.75_f64.sqrt())).abs() < 1e-12);
        assert!((sagitta(6.0, 5.0).unwrap() - 1.0).abs() < 1e-12, "a 3, 4, 5 triangle: the centre is 4 from the chord");
        assert_eq!(sagitta(0.0, 5.0), Some(0.0));
        assert_eq!(sagitta(3.0, 1.0), None, "no chord is longer than the diameter");
    }

    #[test]
    fn a_point_may_be_a_little_behind_the_one_before_it_round_the_arc_as_far_as_the_tolerance_allows_and_no_further() {
        let mut steps = STEPS;
        // With a tolerance of 0.05 on a radius of 10 the slack is 2 * 0.05 / 10 = 0.01 radians. The points are 0.0393 apart: taken back 0.0483 the eleventh is 0.0090 behind
        // the tenth, and taken back 0.0503 it is 0.0110 behind.
        assert!(matches!(arc_through(&quarter_with_a_point_taken_back(0.0), 0.05, &mut steps).unwrap(), Looked::Arc(_)));
        assert!(
            matches!(arc_through(&quarter_with_a_point_taken_back(0.048_3), 0.05, &mut steps).unwrap(), Looked::Arc(_)),
            "0.009 behind is within 0.01"
        );
        assert_eq!(arc_through(&quarter_with_a_point_taken_back(0.050_3), 0.05, &mut steps).unwrap(), Looked::Not, "0.011 behind is not");
        // A point off the circle by more than the tolerance is not on the arc whichever way it is off.
        for off in [0.06, -0.06] {
            let mut points = quarter_with_a_point_taken_back(0.0);
            points[30] = p(points[30].x * (1.0 + off / 10.0), points[30].y * (1.0 + off / 10.0));
            assert_eq!(arc_through(&points, 0.05, &mut steps).unwrap(), Looked::Not, "{off}");
        }
        // 0.04 off is within 95 per cent of the tolerance (0.0475) with the bulge of the chords beside it added; 0.0476 off is not.
        for (off, arc) in [(0.04, true), (0.047_6, false)] {
            let mut points = quarter_with_a_point_taken_back(0.0);
            points[30] = p(points[30].x * (1.0 + off / 10.0), points[30].y * (1.0 + off / 10.0));
            assert_eq!(matches!(arc_through(&points, 0.05, &mut steps).unwrap(), Looked::Arc(_)), arc, "{off}");
        }
    }

    #[test]
    fn an_arc_that_crosses_the_angle_where_the_circles_angles_wrap_is_an_arc_the_way_it_runs() {
        // Round a centre at the origin, from 160 degrees to 200 degrees anticlockwise and back clockwise: the angle of a point is atan2's, which jumps from 180 to -180
        // half way, and a step across the jump is not a turn of 358 degrees.
        let mut steps = STEPS;
        let arc = |from: f64, to: f64| -> Vec<Point> {
            (0..=20)
                .map(|i| {
                    let a = (from + (to - from) * f64::from(i) / 20.0).to_radians();
                    p(10.0 * a.cos(), 10.0 * a.sin())
                })
                .collect()
        };
        let Looked::Arc(Seg::Arc { bow: anticlockwise, .. }) = arc_through(&arc(160.0, 200.0), 0.05, &mut steps).unwrap() else { panic!("an arc") };
        let Looked::Arc(Seg::Arc { bow: clockwise, .. }) = arc_through(&arc(200.0, 160.0), 0.05, &mut steps).unwrap() else { panic!("an arc") };
        assert!(anticlockwise > 0.0 && clockwise < 0.0, "{anticlockwise} {clockwise}");
        assert!((anticlockwise - 10.0 * (1.0 - 20f64.to_radians().cos())).abs() < 1e-9, "{anticlockwise}");
        assert!((clockwise + anticlockwise).abs() < 1e-9, "the same arc the other way has the bow the other way: {clockwise}");
        // And the same near the other end of the range, from -20 to 20 (no jump), for the sign of the bow.
        let Looked::Arc(Seg::Arc { bow: through_zero, .. }) = arc_through(&arc(-20.0, 20.0), 0.05, &mut steps).unwrap() else { panic!("an arc") };
        assert!(through_zero > 0.0);
    }

    #[test]
    fn what_the_fit_cannot_make_smaller_keeps_its_rows_exactly_alone_and_between_arcs() {
        // Two thousand and one segments: two thousand collinear lines and an S of one cubic, fitted as one run: far fewer segments, the ends are the run's, and it stays within the
        // tolerance of the run both ways.
        let n = 2_000_usize;
        let mut segs: Vec<Seg> = (1..=n).map(|i| Seg::Line(p(i as f64 * 0.001, 0.0))).collect();
        let end = p(n as f64 * 0.001, 0.0);
        let s = s_curve(end);
        segs.push(s);
        let run = Run { start: p(0.0, 0.0), segs };
        let fitted = fit_one(&run, &options(0.01, 0)).unwrap();
        assert!(fitted.segs.len() <= 5, "the lines are one line or two and the S a cubic or two: {:?}", fitted.segs);
        assert_eq!((fitted.start, fitted.end()), (run.start, run.end()), "its ends are the run's");
        assert!(stray(&fitted, &run) <= 0.01 && stray(&run, &fitted) <= 0.01, "{} {}", stray(&fitted, &run), stray(&run, &fitted));
        // An S alone is not smaller, and so is not touched (it would be written again as a refit that is a hair off).
        assert!(fit_one(&Run { start: p(0.0, 0.0), segs: vec![s_curve(p(0.0, 0.0))] }, &options(0.01, 0)).is_none());
        // A tolerance of nothing asks for an exact match: fifty points in a line stay fifty, where any other tolerance makes them one.
        let line = Run { start: p(0.0, 0.0), segs: (1..=50).map(|i| Seg::Line(p(f64::from(i) * 0.1, f64::from(i) * 0.05))).collect() };
        assert!(fit_one(&line, &options(0.0, 0)).is_none() && fit_one(&line, &options(0.0, Args::DO_CIRCLES)).is_none());
        assert_eq!(fit_one(&line, &options(0.001, 0)).unwrap().segs.len(), 1);
        // An arc, then an S between two arcs' worth of points: the S between them keeps its rows too (and the arc is found).
        let arc_points = |from: f64, to: f64, n: usize| -> Vec<Point> {
            (0..=n)
                .map(|i| {
                    let a = (from + (to - from) * i as f64 / n as f64).to_radians();
                    p(10.0 * a.cos(), 10.0 * a.sin())
                })
                .collect()
        };
        let first = arc_points(0.0, 60.0, 30);
        let mut segs: Vec<Seg> = first[1..].iter().map(|q| Seg::Line(*q)).collect();
        let s = s_curve(*first.last().unwrap());
        segs.push(s);
        let after = s.end();
        // A second arc of the same circle's shape, a quarter turn on, starting where the S ended.
        segs.extend((1..=30).map(|i| {
            let a = (i as f64 * 2.0).to_radians();
            Seg::Line(p(after.x + 10.0 * a.sin(), after.y + 10.0 * (1.0 - a.cos())))
        }));
        let run = Run { start: first[0], segs };
        let fitted = fit_one(&run, &options(0.01, Args::DO_CIRCLES)).unwrap();
        assert_eq!(fitted.segs.iter().filter(|g| matches!(g, Seg::Arc { .. })).count(), 2, "{:?}", fitted.segs);
        assert!(fitted.segs.contains(&s), "the S is as it was: {:?}", fitted.segs);
    }

    #[test]
    fn four_lines_are_the_fewest_that_make_an_arc_and_an_outlier_before_one_is_a_segment_of_its_own() {
        let arc_points = |from: f64, to: f64, n: usize| -> Vec<Point> {
            (0..=n)
                .map(|i| {
                    let a = (from + (to - from) * i as f64 / n as f64).to_radians();
                    p(10.0 * a.cos(), 10.0 * a.sin())
                })
                .collect()
        };
        // Four lines of a circle of radius 10 over 40 degrees (a bow of 0.6, each chord bulging 0.038, which a tolerance of 0.05 allows): one arc. Three are not enough to be
        // looked at.
        let four = arc_points(0.0, 40.0, 4);
        let run = Run { start: four[0], segs: four[1..].iter().map(|q| Seg::Line(*q)).collect() };
        let fitted = fit_one(&run, &options(0.05, Args::DO_CIRCLES)).unwrap();
        assert!(matches!(fitted.segs.as_slice(), [Seg::Arc { .. }]), "{:?}", fitted.segs);
        let three = arc_points(0.0, 30.0, 3);
        let run = Run { start: three[0], segs: three[1..].iter().map(|q| Seg::Line(*q)).collect() };
        let fitted = fit_one(&run, &options(0.05, Args::DO_CIRCLES));
        assert!(
            fitted.is_none_or(|f| f.segs.iter().all(|g| !matches!(g, Seg::Arc { .. }))),
            "three lines are not looked at for an arc (the library may still fit them)"
        );
        // A start that is off to the side, then thirty lines of an arc that begins at the first line's end: the looks from the start fail, after four segments, and the next
        // place looked at is the next segment (and not two on, which would give the arc's first line to the library), so the arc is the whole of the thirty.
        let arc = arc_points(0.0, 60.0, 30);
        let mut segs = vec![Seg::Line(arc[0])];
        segs.extend(arc[1..].iter().map(|q| Seg::Line(*q)));
        let run = Run { start: p(arc[0].x + 7.0, arc[0].y - 9.0), segs };
        let fitted = fit_one(&run, &options(0.01, Args::DO_CIRCLES)).unwrap();
        let kinds: Vec<bool> = fitted.segs.iter().map(|g| matches!(g, Seg::Arc { .. })).collect();
        assert_eq!(kinds, [false, true], "a line to the arc's start and then the whole arc: {:?}", fitted.segs);
        assert_eq!(fitted.segs[0].end(), arc[0]);
        assert_eq!(fitted.end(), *arc.last().unwrap());
    }

    #[test]
    fn a_straight_run_longer_than_a_look_is_skipped_half_a_look_at_a_time_and_an_arc_after_it_is_still_found_whole() {
        // 300 collinear lines, more than the 256 a look takes (so the first look is flat to its end and the next place looked at is 128 on), and then an arc of 60 lines of
        // a circle of radius 10 that leaves the line's end at a right angle: a line and the whole arc. Skipping a look and a half (or any more than the look) would pass
        // the arc's start.
        let mut segs: Vec<Seg> = (1..=300).map(|i| Seg::Line(p(f64::from(i) * 0.01, 0.0))).collect();
        let corner = p(3.0, 0.0);
        segs.extend((1..=60).map(|i| {
            let a = f64::from(i).to_radians();
            Seg::Line(p(corner.x - 10.0 + 10.0 * a.cos(), 10.0 * a.sin()))
        }));
        let run = Run { start: p(0.0, 0.0), segs };
        let fitted = fit_one(&run, &options(0.01, Args::DO_CIRCLES)).unwrap();
        let kinds: Vec<bool> = fitted.segs.iter().map(|g| matches!(g, Seg::Arc { .. })).collect();
        assert_eq!(kinds, [false, true], "{:?}", fitted.segs);
        assert_eq!(fitted.segs[0].end(), corner, "the line ends where the arc starts");
        assert_eq!(fitted.end(), run.end());
    }

    #[test]
    fn a_long_straight_run_with_the_circles_flag_costs_a_few_looks_and_is_not_refused_for_them() {
        // A hundred thousand points on a line: every stretch is flat. Looking at each place would cost 256 points for each of the hundred thousand; the next place looked
        // at is half way along, so it costs a few hundred looks.
        let n = 100_000_usize;
        let run = Run { start: p(0.0, 0.0), segs: (1..=n).map(|i| Seg::Line(p(i as f64 * 0.001, i as f64 * 0.0005))).collect() };
        let mut steps = STEPS;
        let fitted = fit(std::slice::from_ref(&run), &options(0.01, Args::DO_CIRCLES), &mut steps)
            .unwrap()
            .pop()
            .flatten()
            .expect("the library makes it lines");
        assert!(fitted.segs.iter().all(|s| matches!(s, Seg::Line(_))), "no arc in a line");
        assert!(STEPS - steps < 2_000_000, "{} steps for a straight run of {n}", STEPS - steps);
    }

    #[test]
    fn an_arc_through_points_is_flat_when_it_bows_no_more_than_the_tolerance_and_not_one_when_the_points_stray() {
        let mut steps = STEPS;
        let on_circle = |from: f64, to: f64, n: usize| -> Vec<Point> {
            (0..=n)
                .map(|i| {
                    let a = (from + (to - from) * i as f64 / n as f64).to_radians();
                    p(10.0 * a.cos(), 10.0 * a.sin())
                })
                .collect()
        };
        assert_eq!(arc_through(&on_circle(0.0, 0.5, 20), 0.01, &mut steps).unwrap(), Looked::Flat, "a half degree of radius 10 bows 0.0002");
        let spent = STEPS - steps;
        assert!(spent == 0, "a flat arc is told at no charge: {spent}");
        assert!(matches!(arc_through(&on_circle(0.0, 60.0, 20), 0.01, &mut steps).unwrap(), Looked::Arc(_)));
        assert!(STEPS - steps >= 21, "an arc is checked against each point, charged");
        let mut wobbly = on_circle(0.0, 60.0, 20);
        wobbly[7] = p(wobbly[7].x + 0.2, wobbly[7].y);
        assert_eq!(arc_through(&wobbly, 0.01, &mut steps).unwrap(), Looked::Not);
        assert_eq!(arc_through(&on_circle(0.0, 200.0, 40), 0.01, &mut steps).unwrap(), Looked::Not, "more than half a circle is not one arc");
        assert_eq!(
            arc_through(&[p(0.0, 0.0), p(1.0, 1.0), p(2.0, 2.0)], 0.01, &mut steps).unwrap(),
            Looked::Flat,
            "three points in a line have no circle: they are a line"
        );
        // Points that are in a line to within a rounding are a line, and not a circle of an enormous radius.
        let noisy: Vec<Point> = (0..30).map(|i| p(f64::from(i) * 0.001, f64::from(i) * 0.0005 + if i % 2 == 0 { 1.0e-14 } else { 0.0 })).collect();
        assert_eq!(arc_through(&noisy, 0.01, &mut steps).unwrap(), Looked::Flat);
        assert_eq!(arc_through(&[p(1.0, 1.0)], 0.01, &mut steps).unwrap(), Looked::Not, "one point");
        assert_eq!(arc_through(&[p(1.0, 1.0), p(2.0, 5.0), p(1.0, 1.0)], 0.01, &mut steps).unwrap(), Looked::Not, "there and back");
        assert_eq!(arc_through(&[], 0.01, &mut steps).unwrap(), Looked::Not);
    }

    #[test]
    fn looking_for_arcs_is_charged_and_past_the_steps_the_fit_is_refused_in_words() {
        let circle = polygon_of_circle(1.0);
        let mut steps = 100_u64;
        let error = fit(std::slice::from_ref(&circle), &options(0.02, Args::DO_CIRCLES), &mut steps).unwrap_err();
        assert!(!error.bad_params && error.message.contains("Fit Curve would look at more than"), "{error:?}");
        let mut enough = STEPS;
        assert!(fit(std::slice::from_ref(&circle), &options(0.02, Args::DO_CIRCLES), &mut enough).is_ok());
        let spent = STEPS - enough;
        assert!(spent > 100 && spent < 5_000_000, "{spent}");
        let mut exact = spent;
        assert!(fit(std::slice::from_ref(&circle), &options(0.02, Args::DO_CIRCLES), &mut exact).is_ok());
        assert_eq!(exact, 0, "every step is charged once");
        let mut one_short = spent - 1;
        assert!(fit(std::slice::from_ref(&circle), &options(0.02, Args::DO_CIRCLES), &mut one_short).is_err());
    }

    #[test]
    fn a_run_of_no_segment_and_runs_of_every_kind_are_looked_at_without_a_panic() {
        assert!(fit_one(&Run { start: p(0.0, 0.0), segs: vec![] }, &options(0.1, Args::DO_CIRCLES)).is_none());
        let odd = Run {
            start: p(0.0, 0.0),
            segs: vec![Seg::Arc { to: p(4.0, 0.0), bow: 1.0 }, Seg::Line(p(4.0, 0.0)), Seg::Cubic(p(4.0, 0.0), p(4.0, 0.0), p(4.0, 0.0))],
        };
        let _ = fit_one(&odd, &options(0.1, Args::DO_CIRCLES | Args::PERIODIC | Args::ABRUPT));
    }

    #[test]
    fn the_answers_of_the_library_go_to_the_stretches_they_fit_in_order_and_a_stretch_the_library_dropped_stays_as_it_was() {
        let line = |a: (f64, f64), b: (f64, f64)| Job {
            run: 0,
            item: 0,
            sub: Run { start: p(a.0, a.1), segs: vec![Seg::Line(p((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)), Seg::Line(p(b.0, b.1))] },
            closed_loop: false,
        };
        let (first, second, third) = (line((0.0, 0.0), (4.0, 0.0)), line((0.0, 5.0), (4.0, 5.0)), line((0.0, 9.0), (4.0, 9.0)));
        let answer = |a: (f64, f64), b: (f64, f64)| Run { start: p(a.0, a.1), segs: vec![Seg::Line(p(b.0, b.1))] };
        // All three answered, in order.
        let all =
            align(&[first, second, third], vec![answer((0.0, 0.0), (4.0, 0.0)), answer((0.0, 5.0), (4.0, 5.0)), answer((0.0, 9.0), (4.0, 9.0))]);
        assert!(all.iter().all(Option::is_some));
        // The second dropped: the third's answer is not taken for it.
        let (first, second, third) = (line((0.0, 0.0), (4.0, 0.0)), line((0.0, 5.0), (4.0, 5.0)), line((0.0, 9.0), (4.0, 9.0)));
        let dropped = align(&[first, second, third], vec![answer((0.0, 0.0), (4.0, 0.0)), answer((0.0, 9.0), (4.0, 9.0))]);
        assert_eq!(dropped.iter().map(Option::is_some).collect::<Vec<_>>(), [true, false, true]);
        // Nothing came back: nothing is answered, and an answer that is no stretch's is not taken.
        let one = line((0.0, 0.0), (4.0, 0.0));
        assert_eq!(align(std::slice::from_ref(&one), vec![]), vec![None]);
        assert_eq!(align(std::slice::from_ref(&one), vec![answer((0.0, 0.0), (7.0, 0.0))]), vec![None], "its end is not the stretch's");
        // A loop is answered by a closed run that starts anywhere.
        let ring = Job {
            run: 0,
            item: 0,
            sub: Run { start: p(0.0, 0.0), segs: vec![Seg::Line(p(2.0, 0.0)), Seg::Line(p(2.0, 2.0)), Seg::Line(p(0.0, 0.0))] },
            closed_loop: true,
        };
        let rotated = Run { start: p(2.0, 0.0), segs: vec![Seg::Line(p(2.0, 2.0)), Seg::Line(p(2.0, 0.0))] };
        assert!(align(std::slice::from_ref(&ring), vec![rotated]).pop().flatten().is_some());
    }

    #[test]
    fn many_runs_are_fitted_by_one_call_in_the_order_given_and_each_gets_its_own_answer() {
        // Three circles of 36 lines, a rectangle and a straight run of fifty points, in one call: the circles and the straight run are made smaller and the rectangle is not.
        let circle = polygon_of_circle(10.0);
        let rectangle =
            Run { start: p(0.0, 0.0), segs: vec![Seg::Line(p(2.0, 0.0)), Seg::Line(p(2.0, 1.0)), Seg::Line(p(0.0, 1.0)), Seg::Line(p(0.0, 0.0))] };
        let straight = Run { start: p(0.0, 0.0), segs: (1..=50).map(|i| Seg::Line(p(f64::from(i) * 0.1, f64::from(i) * 0.05))).collect() };
        let runs = [circle.clone(), rectangle.clone(), straight.clone(), circle];
        let mut steps = STEPS;
        let fitted = fit(&runs, &options(0.02, Args::DO_CIRCLES), &mut steps).unwrap();
        assert_eq!(fitted.len(), 4);
        assert!(
            fitted[0].is_some() && fitted[1].is_none() && fitted[2].is_some() && fitted[3].is_some(),
            "{:?}",
            fitted.iter().map(Option::is_some).collect::<Vec<_>>()
        );
        assert_eq!(fitted[2].as_ref().map(|r| r.segs.len()), Some(1), "the straight run is one line");
        for (old, new) in runs.iter().zip(&fitted) {
            if let Some(new) = new {
                assert!(stray(new, old) <= 0.02 && stray(old, new) <= 0.02);
            }
        }
        // The same runs one at a time give the same answers: nothing depends on the others.
        for (run, together) in runs.iter().zip(&fitted) {
            assert_eq!(fit_one(run, &options(0.02, Args::DO_CIRCLES)).is_some(), together.is_some());
        }
    }

    #[test]
    fn a_polyline_that_is_a_sine_wave_of_many_points_is_fitted_to_fewer_segments_within_the_tolerance() {
        // The library once handed back a run of a few thousand segments as it was (a sine wave of 5,983 points was the most an earlier pin fitted): today's pin fits runs in parts.
        for n in [2_000_usize, 7_500, 20_000] {
            let at = |i: usize| p(i as f64 * 0.0001, (i as f64 * 0.3).sin());
            let run = Run { start: at(0), segs: (1..=n).map(|i| Seg::Line(at(i))).collect() };
            let fitted = fit_one(&run, &options(0.01, 0)).unwrap_or_else(|| panic!("{n} points are fitted"));
            assert!(fitted.segs.len() < n / 4, "{n}: {} segments", fitted.segs.len());
            assert_eq!(fitted.end(), run.end());
        }
    }
}
