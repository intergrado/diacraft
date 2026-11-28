//! Trim's touching: the end of an open path within [`MERGE`] of another path touches it.
//!
//! The path library cuts a path where another meets it exactly (to the digits of the arithmetic and a millionth of a unit or so of its own reach), not where the end of a line
//! stops 5E-6 short of one. The specification says the closure tolerance, 1E-5, decides what touches, so before the library is asked each end of an open path that
//! lies within [`MERGE`] of a segment of another path (or of its own, farther along) is moved onto the nearest point of that segment, and the library then finds a touch.
//!
//! * **Only the ends of open paths move**, and only onto a segment they are not already the end of. An end within [`MERGE`] of the *end of another open path* is two ends that
//!   meet, which cuts nothing (Join chains such paths), and it stays. An end near a vertex that is not the end of an open path (a corner of a ring, a joint of a polyline)
//!   goes to the vertex itself.
//! * Two paths whose middles pass within [`MERGE`] of each other without crossing, and the vertex of a ring near a path, are not moved: a touch of those is the library's own.
//!
//! **The work is counted.** The segments are put in a grid of at most 128 cells across the drawing, a segment in each cell its box (grown by [`MERGE`]) covers, or in a list
//! every end looks through when it would take more than [`MAX_CELLS`] of them. Every registration, every segment an end is tried against and the nearest-point search of the
//! few that survive are charged a step, and the command is refused in words past [`STEPS`].

use std::collections::HashMap;

use kurbo::{CubicBez, Line, ParamCurve, ParamCurveExtrema, ParamCurveNearest, PathSeg, Point, Rect};

use super::MERGE;
use super::runs::{Run, Seg};
use crate::draw::Problem;

/// The most steps the snapping takes: each is a few nanoseconds.
pub(super) const STEPS: u64 = 20_000_000;

/// The most cells one segment is put in; one that covers more is looked through for every end.
const MAX_CELLS: i64 = 256;

/// How many cells the grid has across the drawing.
const ACROSS: f64 = 128.0;

/// The steps the nearest-point search of one segment is charged.
const NEAREST: u64 = 16;

/// One segment of a run, with its box grown by [`MERGE`].
struct Item {
    run: usize,
    k: usize,
    seg: PathSeg,
    from: Point,
    to: Point,
    rect: Rect,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum End {
    Start,
    Finish,
}

fn charge(steps: &mut u64, n: u64) -> Result<(), Problem> {
    *steps = steps.checked_sub(n).ok_or_else(|| {
        Problem::failed(format!(
            "Trim would take more than {STEPS} steps to find which paths touch: too many paths end close to others, and the command is refused"
        ))
    })?;
    Ok(())
}

fn items_of(runs: &[Run]) -> Vec<Item> {
    let mut items = Vec::new();
    for (run, r) in runs.iter().enumerate() {
        let mut pen = r.start;
        for (k, seg) in r.segs.iter().enumerate() {
            let path_seg = match *seg {
                Seg::Line(p) => PathSeg::Line(Line::new(pen, p)),
                Seg::Cubic(a, b, p) => PathSeg::Cubic(CubicBez::new(pen, a, b, p)),
                // A run read from a shape holds no arc (only Fit Curve makes one, `runs.rs`), so no command reaches this arm: the chord is all one segment can say.
                Seg::Arc { to, .. } => PathSeg::Line(Line::new(pen, to)),
            };
            let rect = path_seg.bounding_box().inflate(MERGE, MERGE);
            items.push(Item { run, k, seg: path_seg, from: pen, to: seg.end(), rect });
            pen = seg.end();
        }
    }
    items
}

/// The grid: the origin and size of a cell, the items by cell, and the items too big for it.
struct Grid {
    origin: Point,
    size: f64,
    cells: HashMap<(i64, i64), Vec<usize>>,
    big: Vec<usize>,
}

impl Grid {
    fn cell(&self, p: Point) -> (i64, i64) {
        let at = |v: f64, o: f64| ((v - o) / self.size).floor().clamp(0.0, 4.0 * ACROSS) as i64;
        (at(p.x, self.origin.x), at(p.y, self.origin.y))
    }

    fn build(items: &[Item], steps: &mut u64) -> Result<Grid, Problem> {
        let everything = items.iter().map(|i| i.rect).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO);
        let size = (everything.width().max(everything.height()) / ACROSS).max(8.0 * MERGE);
        let mut grid = Grid { origin: Point::new(everything.x0, everything.y0), size, cells: HashMap::new(), big: Vec::new() };
        for (n, item) in items.iter().enumerate() {
            let ((x0, y0), (x1, y1)) = (grid.cell(Point::new(item.rect.x0, item.rect.y0)), grid.cell(Point::new(item.rect.x1, item.rect.y1)));
            charge(steps, 1)?;
            if (x1 - x0 + 1).saturating_mul(y1 - y0 + 1) > MAX_CELLS {
                grid.big.push(n);
                continue;
            }
            for x in x0..=x1 {
                for y in y0..=y1 {
                    charge(steps, 1)?;
                    grid.cells.entry((x, y)).or_default().push(n);
                }
            }
        }
        Ok(grid)
    }
}

/// Where the end `at` of the run `run` (at `own` of its segments) should go, if it touches a segment: the nearest point of the nearest segment within [`MERGE`], or the
/// vertex it is near.
fn target(runs: &[Run], items: &[Item], grid: &Grid, run: usize, own: usize, at: Point, steps: &mut u64) -> Result<Option<Point>, Problem> {
    let mut best: Option<(f64, Point)> = None;
    let mut look = |n: usize, steps: &mut u64| -> Result<(), Problem> {
        charge(steps, 1)?;
        let Some(item) = items.get(n) else { return Ok(()) };
        let inside = item.rect.x0 <= at.x && at.x <= item.rect.x1 && item.rect.y0 <= at.y && at.y <= item.rect.y1;
        // The segment the end belongs to and the one next to it are the path the end is on: it touches no other part of itself within a joint.
        if (item.run == run && item.k.abs_diff(own) <= 1) || !inside {
            return Ok(());
        }
        charge(steps, NEAREST)?;
        let nearest = item.seg.nearest(at, 1.0e-9);
        let distance = nearest.distance_sq.sqrt();
        if distance.is_nan() || distance > MERGE {
            return Ok(());
        }
        let on = item.seg.eval(nearest.t);
        // The ends of open paths are the places two paths meet end to end, which cuts nothing. A vertex of anything else is where the end goes.
        let other = runs.get(item.run);
        let open = other.is_some_and(|r| !r.closed());
        let last = other.is_some_and(|r| item.k + 1 == r.segs.len());
        let (start_is_end, finish_is_end) = (open && item.k == 0, open && last);
        let vertex = [(item.from, start_is_end), (item.to, finish_is_end)].into_iter().find(|(p, _)| p.distance(on) <= MERGE);
        let place = match vertex {
            Some((_, true)) => return Ok(()),
            Some((p, false)) => p,
            None => on,
        };
        if distance > 0.0 && best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, place));
        }
        Ok(())
    };
    for &n in &grid.big {
        look(n, steps)?;
    }
    if let Some(cell) = grid.cells.get(&grid.cell(at)) {
        for &n in cell {
            look(n, steps)?;
        }
    }
    Ok(best.map(|(_, p)| p))
}

/// Moves each end of an open run that touches another path (within [`MERGE`]) onto it. How many ends moved.
pub(super) fn snap_ends(runs: &mut [Run], steps: &mut u64) -> Result<usize, Problem> {
    let ends: Vec<(usize, End, Point)> =
        runs.iter().enumerate().filter(|(_, r)| !r.closed()).flat_map(|(i, r)| [(i, End::Start, r.start), (i, End::Finish, r.end())]).collect();
    if ends.is_empty() {
        return Ok(0);
    }
    let items = items_of(runs);
    let grid = Grid::build(&items, steps)?;
    let mut moves: Vec<(usize, End, Point)> = Vec::new();
    for (run, end, at) in ends {
        let own = match end {
            End::Start => 0,
            End::Finish => runs.get(run).map_or(0, |r| r.segs.len().saturating_sub(1)),
        };
        if let Some(to) = target(runs, &items, &grid, run, own, at, steps)? {
            moves.push((run, end, to));
        }
    }
    // The moves are made together, after every end has looked at the paths as they were drawn: an end that moved is not a place another end snaps to.
    let count = moves.len();
    for (run, end, to) in moves {
        let Some(r) = runs.get_mut(run) else { continue };
        match end {
            End::Start => r.start = to,
            End::Finish => match r.segs.last_mut() {
                Some(Seg::Line(p) | Seg::Cubic(_, _, p) | Seg::Arc { to: p, .. }) => *p = to,
                None => {}
            },
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn line(a: (f64, f64), b: (f64, f64)) -> Run {
        Run { start: p(a.0, a.1), segs: vec![Seg::Line(p(b.0, b.1))] }
    }

    fn snapped(runs: &mut [Run]) -> usize {
        let mut steps = STEPS;
        snap_ends(runs, &mut steps).unwrap()
    }

    #[test]
    fn an_end_just_short_of_a_line_goes_onto_it_and_one_farther_does_not() {
        let mut runs = [line((0.0, 0.0), (4.0, 0.0)), line((2.0, 0.000_005), (2.0, 2.0))];
        assert_eq!(snapped(&mut runs), 1);
        assert_eq!(runs[1].start, p(2.0, 0.0));
        assert_eq!(runs[0], line((0.0, 0.0), (4.0, 0.0)), "the line it touched is as it was");
        let mut far = [line((0.0, 0.0), (4.0, 0.0)), line((2.0, 0.000_02), (2.0, 2.0))];
        assert_eq!(snapped(&mut far), 0);
        assert_eq!(far[1].start, p(2.0, 0.000_02));
    }

    #[test]
    fn the_end_of_a_path_goes_onto_a_curve_and_the_finish_of_a_path_moves_as_well_as_the_start() {
        let arch = Run { start: p(0.0, 0.0), segs: vec![Seg::Cubic(p(1.0, 2.0), p(3.0, 2.0), p(4.0, 0.0))] };
        // The middle of the arch is at (2, 1.5).
        let mut runs = [arch, line((2.0, 3.0), (2.0, 1.500_004))];
        assert_eq!(snapped(&mut runs), 1);
        let Seg::Line(end) = runs[1].segs[0] else { panic!("a line") };
        assert!((end.x - 2.0).abs() < 1e-6 && (end.y - 1.5).abs() < 1e-6, "{end:?}");
    }

    #[test]
    fn two_ends_that_meet_end_to_end_stay_and_a_path_does_not_snap_to_the_end_it_has() {
        let mut runs = [line((0.0, 0.0), (1.0, 0.0)), line((1.000_004, 0.0), (2.0, 0.0))];
        assert_eq!(snapped(&mut runs), 0, "an end meeting an end cuts nothing");
        // A segment shorter than the tolerance at the end of a polyline is not a path the end touches.
        let mut tiny = [Run { start: p(0.0, 0.0), segs: vec![Seg::Line(p(1.0, 0.0)), Seg::Line(p(1.000_003, 0.0))] }];
        assert_eq!(snapped(&mut tiny), 0);
    }

    #[test]
    fn an_end_near_a_vertex_of_a_ring_goes_to_the_vertex() {
        let ring =
            Run { start: p(0.0, 0.0), segs: vec![Seg::Line(p(2.0, 0.0)), Seg::Line(p(2.0, 2.0)), Seg::Line(p(0.0, 2.0)), Seg::Line(p(0.0, 0.0))] };
        let mut runs = [ring, line((5.0, -3.0), (2.000_004, -0.000_003))];
        assert_eq!(snapped(&mut runs), 1);
        assert_eq!(runs[1].segs[0], Seg::Line(p(2.0, 0.0)), "onto the corner, which is where the ring's two edges meet");
    }

    #[test]
    fn a_path_that_touches_itself_farther_along_is_snapped_and_a_closed_ring_is_left_alone() {
        // A "6": the end comes back to just short of the first segment.
        let six = Run {
            start: p(0.0, 0.0),
            segs: vec![Seg::Line(p(4.0, 0.0)), Seg::Line(p(4.0, 4.0)), Seg::Line(p(1.0, 4.0)), Seg::Line(p(1.0, 0.000_004))],
        };
        let mut runs = [six];
        assert_eq!(snapped(&mut runs), 1);
        assert_eq!(runs[0].end(), p(1.0, 0.0));
        let ring = Run { start: p(0.0, 0.0), segs: vec![Seg::Line(p(2.0, 0.0)), Seg::Line(p(2.0, 2.0)), Seg::Line(p(0.0, 0.0))] };
        let mut rings = [ring.clone(), line((1.0, -5.0), (1.0, -0.000_004))];
        assert_eq!(snapped(&mut rings), 1, "the line's end goes onto the ring");
        assert_eq!(rings[0], ring, "and the ring, which has no open end, is as it was");
    }

    #[test]
    fn the_steps_of_a_crowd_are_counted_and_the_command_is_refused_when_they_run_out() {
        // 3,000 short segments in one cell and 3,000 open ends in the same cell, none within the tolerance of one: every end looks at every segment.
        let mut runs: Vec<Run> = (0..3_000).map(|i| line((f64::from(i) * 1e-9, 0.0), (f64::from(i) * 1e-9, 4.0e-5))).collect();
        runs.push(line((0.0, 2.0e-5 + 5.0e-5), (3.0e-6, 1.0)));
        let mut steps = 50_000;
        let error = snap_ends(&mut runs, &mut steps).unwrap_err();
        assert!(!error.bad_params && error.message.contains("Trim would take more than"), "{error:?}");
    }
}
