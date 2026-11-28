//! Join's chains (SHAPEOPS-JOIN): the paths whose ends meet, within [`MERGE`], are one path.
//!
//! **The library's `join` is not this.** `craft_pathops::join` goes nearest end first and bridges a gap with a straight segment (so two lines 0.01 apart would come out
//! as one path of three segments, where V16 wants two sections), and it closes a path that is alone (one open path stays open, in a section of its own). What the
//! specification says is built here, on the [`Run`]s of [`super::runs`]:
//!
//! * the paths are the runs of the shown sections of the operands, in selection order, then section order, then the order of the runs in a section;
//! * a run that is closed (its ends meet) passes through as a section of its own, at its place in that order;
//! * the others are chained: a chain starts with the first path not yet used. While some unused path has an end within [`MERGE`] of the chain's tail, the earliest such
//!   path is attached (turned round if its end is the one that meets), then the same at the head. A chain whose ends have met is closed and takes nothing more (a third
//!   path ending at the point goes to another chain: where three or more ends meet, the earliest pair joins and the rest go to other chains);
//! * the vertex where two paths join is written once (the first path's segment before it and the second's after), and segments in line with each other are not merged:
//!   segments are carried as they are, lines exactly and curves as the cubics they are.
//!
//! **The work is counted.** Ends are found by a hash of cells [`MERGE`] wide, looked up in the nine cells round a point. Every entry looked at is charged a step, before
//! it is looked at, and the command is refused in words past [`STEPS`]. A cell keeps its entries in the order of the paths with a cursor past the used ones at its front, so
//! a star of paths that all meet at one point costs a few steps for each path and not the square of them; what a crafted drawing can still make costly (many ends that lie
//! in the cells round a point and are none of them within [`MERGE`] of it) is what the steps count.

use std::collections::{HashMap, VecDeque};

use kurbo::Point;

use super::MERGE;
use super::runs::{Run, Seg};
use crate::draw::Problem;

/// The most entries of the hash a Join looks at (SHAPEOPS-CAPS): each is a few nanoseconds, so this is a fraction of a second.
pub(super) const STEPS: u64 = 40_000_000;

/// Which end of a path an entry of the hash is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum End {
    Start,
    Finish,
}

/// The entries of one cell of the hash: the path, its end and where it is, in the order of the paths, with the cursor before which every path is used.
#[derive(Default)]
struct Cell {
    entries: Vec<(usize, End, Point)>,
    live: usize,
}

type Grid = HashMap<(i64, i64), Cell>;

fn cell_of(p: Point) -> (i64, i64) {
    // The casts saturate: a point so far out that its cell is the last one is looked at with the others there, and the test of the distance is exact.
    ((p.x / MERGE).floor() as i64, (p.y / MERGE).floor() as i64)
}

/// A chain being built: its start and its segments, which can grow at either end.
struct Chain {
    start: Point,
    segs: VecDeque<Seg>,
}

impl Chain {
    fn of(run: &Run) -> Chain {
        Chain { start: run.start, segs: run.segs.iter().copied().collect() }
    }

    fn tail(&self) -> Point {
        self.segs.back().map_or(self.start, Seg::end)
    }

    fn closed(&self) -> bool {
        !self.segs.is_empty() && self.start.distance(self.tail()) <= MERGE
    }

    /// The path `run`, which starts at the tail, after it.
    fn push_back(&mut self, run: &Run) {
        self.segs.extend(run.segs.iter().copied());
    }

    /// The path `run`, which ends at the head, before it: the chain starts where the path did.
    fn push_front(&mut self, run: &Run) {
        for seg in run.segs.iter().rev() {
            self.segs.push_front(*seg);
        }
        self.start = run.start;
    }

    fn into_run(self) -> Run {
        Run { start: self.start, segs: self.segs.into() }
    }
}

fn charge(steps: &mut u64) -> Result<(), Problem> {
    *steps = steps.checked_sub(1).ok_or_else(|| {
        Problem::failed(format!(
            "Join would take more than {STEPS} steps to find which paths meet: too many paths end close together, and the command is refused"
        ))
    })?;
    Ok(())
}

/// The earliest unused path that has an end within [`MERGE`] of `at`, with the end that meets: the nine cells round `at` are looked into, each from its cursor, and the
/// cursors move past the paths that are used. Every entry looked at is charged to `steps` first.
fn earliest(grid: &mut Grid, used: &[bool], at: Point, steps: &mut u64) -> Result<Option<(usize, End)>, Problem> {
    let (cx, cy) = cell_of(at);
    let mut best: Option<(usize, End)> = None;
    for dx in -1..=1_i64 {
        for dy in -1..=1_i64 {
            let Some(cell) = grid.get_mut(&(cx.saturating_add(dx), cy.saturating_add(dy))) else { continue };
            // The cursor goes past the used paths at the front, for good.
            while let Some((j, ..)) = cell.entries.get(cell.live) {
                if !used.get(*j).copied().unwrap_or(true) {
                    break;
                }
                charge(steps)?;
                cell.live += 1;
            }
            for (j, end, p) in cell.entries.iter().skip(cell.live) {
                charge(steps)?;
                if best.is_some_and(|(b, _)| *j >= b) {
                    // The entries are in the order of the paths: nothing later in this cell is earlier than what is found.
                    break;
                }
                if !used.get(*j).copied().unwrap_or(true) && p.distance(at) <= MERGE {
                    best = Some((*j, *end));
                    break;
                }
            }
        }
    }
    Ok(best)
}

fn mark(used: &mut [bool], at: usize) {
    if let Some(flag) = used.get_mut(at) {
        *flag = true;
    }
}

/// The chains of `runs`, in the order their first paths came: one run for each. `steps` is the budget of the hash's lookups, spent as they are made.
pub(super) fn chains(runs: &[Run], steps: &mut u64) -> Result<Vec<Run>, Problem> {
    let mut grid: Grid = HashMap::new();
    for (j, run) in runs.iter().enumerate().filter(|(_, r)| !r.closed()) {
        for (end, p) in [(End::Start, run.start), (End::Finish, run.end())] {
            charge(steps)?;
            grid.entry(cell_of(p)).or_default().entries.push((j, end, p));
        }
    }
    let mut used = vec![false; runs.len()];
    let mut out: Vec<Run> = Vec::new();
    for (i, run) in runs.iter().enumerate() {
        if used.get(i).copied().unwrap_or(true) {
            continue;
        }
        mark(&mut used, i);
        if run.closed() {
            // A path that is already closed passes through as a section of its own.
            out.push(run.clone());
            continue;
        }
        let mut chain = Chain::of(run);
        // The tail first, then the head.
        while !chain.closed() {
            let Some((j, end)) = earliest(&mut grid, &used, chain.tail(), steps)? else { break };
            let Some(next) = runs.get(j) else { break };
            mark(&mut used, j);
            match end {
                End::Start => chain.push_back(next),
                End::Finish => chain.push_back(&next.reversed()),
            }
        }
        while !chain.closed() {
            let Some((j, end)) = earliest(&mut grid, &used, chain.start, steps)? else { break };
            let Some(next) = runs.get(j) else { break };
            mark(&mut used, j);
            match end {
                End::Finish => chain.push_front(next),
                End::Start => chain.push_front(&next.reversed()),
            }
        }
        out.push(chain.into_run());
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

    fn line(a: (f64, f64), b: (f64, f64)) -> Run {
        Run { start: p(a.0, a.1), segs: vec![Seg::Line(p(b.0, b.1))] }
    }

    fn joined(runs: &[Run]) -> Vec<Run> {
        let mut steps = STEPS;
        chains(runs, &mut steps).unwrap()
    }

    fn points(run: &Run) -> Vec<(f64, f64)> {
        std::iter::once(run.start).chain(run.segs.iter().map(Seg::end)).map(|q| (q.x, q.y)).collect()
    }

    #[test]
    fn lines_end_to_end_chain_and_a_line_met_by_its_end_is_turned_round() {
        let out = joined(&[line((0.0, 0.0), (2.0, 0.0)), line((2.0, 0.0), (2.0, 2.0)), line((0.0, 2.0), (2.0, 2.0))]);
        assert_eq!(out.len(), 1);
        assert_eq!(points(&out[0]), [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
    }

    #[test]
    fn a_chain_whose_ends_meet_is_closed_and_takes_nothing_more() {
        // A third path also starts at (0, 0), where the chain closes: it is another chain.
        let out = joined(&[line((0.0, 0.0), (1.0, 0.0)), line((1.0, 0.0), (1.0, 1.0)), line((1.0, 1.0), (0.0, 0.0)), line((0.0, 0.0), (-1.0, -1.0))]);
        assert_eq!(out.len(), 2);
        assert!(out[0].closed() && !out[1].closed());
        assert_eq!(points(&out[1]), [(0.0, 0.0), (-1.0, -1.0)]);
    }

    #[test]
    fn the_head_grows_when_the_tail_cannot() {
        let out = joined(&[line((1.0, 0.0), (2.0, 0.0)), line((0.0, 0.0), (1.0, 0.0))]);
        assert_eq!(points(&out[0]), [(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)]);
    }

    #[test]
    fn a_gap_just_inside_the_tolerance_joins_and_just_outside_does_not() {
        assert_eq!(joined(&[line((0.0, 0.0), (1.0, 0.0)), line((1.000_009, 0.0), (2.0, 0.0))]).len(), 1);
        assert_eq!(joined(&[line((0.0, 0.0), (1.0, 0.0)), line((1.000_011, 0.0), (2.0, 0.0))]).len(), 2);
        // The same across a cell's edge of the hash: the ends are in different cells.
        assert_eq!(joined(&[line((0.0, 0.0), (0.999_995, 0.0)), line((1.000_004, 0.0), (2.0, 0.0))]).len(), 1);
    }

    #[test]
    fn a_closed_run_passes_through_in_its_place() {
        let square = Run { start: p(5.0, 5.0), segs: vec![Seg::Line(p(6.0, 5.0)), Seg::Line(p(6.0, 6.0)), Seg::Line(p(5.0, 5.0))] };
        let out = joined(&[line((0.0, 0.0), (1.0, 0.0)), square.clone(), line((1.0, 0.0), (1.0, 1.0))]);
        assert_eq!(out.len(), 2);
        assert_eq!(points(&out[0]), [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]);
        assert_eq!(out[1], square);
    }

    #[test]
    fn a_star_of_paths_at_one_point_costs_a_few_steps_for_each_and_not_the_square_of_them() {
        let n = 5_000_usize;
        let runs: Vec<Run> = (0..n).map(|i| line((0.0, 0.0), (1.0 + i as f64, 1.0))).collect();
        let mut steps = STEPS;
        let out = chains(&runs, &mut steps).unwrap();
        // The earliest pair at the centre joins, then the earliest pair of those left, and so on: each chain is two paths.
        assert_eq!(out.len(), n / 2);
        let spent = STEPS - steps;
        assert!(spent < 20 * n as u64, "{spent} steps for {n} paths");
    }

    #[test]
    fn crowded_cells_are_refused_in_words_when_the_steps_run_out() {
        // Two crowds of starts 1.5 MERGE apart, in neighbouring cells: no start of one is within MERGE of a start of the other, and each lookup at one crowd reads the other.
        let n = 1_000_usize;
        let mut runs: Vec<Run> = Vec::new();
        for i in 0..n {
            runs.push(line((0.0, 0.0), (5.0, i as f64)));
            runs.push(line((0.000_015, 0.0), (-5.0, i as f64)));
        }
        // What the hash costs to fill is 2 steps for each of the 2n paths' ends, and a lookup beyond it is a step for each entry it reads.
        let mut steps = 4 * n as u64 + 2_000;
        let error = chains(&runs, &mut steps).unwrap_err();
        assert!(!error.bad_params && error.message.contains("Join would take more than"), "{error:?}");
        let mut enough = STEPS;
        assert!(chains(&runs, &mut enough).is_ok(), "with the steps of a command it is finished");
    }
}
