//! Trim's pieces (SHAPEOPS-TRIM): every path cut where it crosses, touches or meets itself, and which pieces stay closed.
//!
//! **The cutting is the library's** ([`ops::trim_pieces`]: Pathfinder Outline on a planar map of all the paths, with its own counted limits); this file reads what it
//! gives. Its pieces are in the order of R35 (the paths in the order they were named, each one's pieces in path order from its first cut), and a stretch two paths run
//! along is kept once, in the place of the earlier.
//!
//! **Open and closed pieces** (of the first plan: every piece that cutting made is open). A path that nothing crosses comes back whole, and a closed one
//! stays closed with `NoFill` 0. Every other piece is open, `NoFill` 1, even where its ends meet: a ring cut at one place is one open piece starting and ending there, and the
//! lobes of a figure of eight are two. The library gives an uncut ring and a ring cut at one place the same way (one piece that starts and ends at a point), so the two are told
//! apart by the place: **a closed piece is whole when no other piece has an end at the place it starts**. A ring that was cut was cut by something that meets it there: another
//! path ends or crosses at the place (its pieces end there), or the ring meets itself there (its other lobe ends there).
//!
//! **What this is charged.** The pieces are counted when the library gives them: a command makes at most [`MAX_PRODUCED`] shapes, and a Trim that would make more is refused in
//! words, which is the library's own answer too for a drawing it cannot cut in bounded work. Looking for the ends at a place goes through a hash of cells [`MERGE`] wide, every
//! entry looked at charged a step and the command refused past [`STEPS`].

use std::collections::HashMap;

use kurbo::Point;

use super::runs::Run;
use super::{MAX_PRODUCED, MERGE, ops, snap};
use crate::draw::Problem;

/// The most entries of the hash of ends that a Trim looks at: each is a few nanoseconds.
pub(super) const STEPS: u64 = 50_000_000;

/// A piece Trim makes.
#[derive(Debug)]
pub(super) struct Cut {
    pub run: Run,
    /// Whether the piece is a closed path that nothing cut: it stays closed, `NoFill` 0. **Every other piece is open, `NoFill` 1.**
    pub whole: bool,
}

fn cell_of(p: Point) -> (i64, i64) {
    // The casts saturate: a point so far out that its cell is the last one is looked at with the others there, and the test of the distance is exact.
    ((p.x / MERGE).floor() as i64, (p.y / MERGE).floor() as i64)
}

fn too_many() -> Problem {
    Problem::failed(format!(
        "Trim would make more than {MAX_PRODUCED} shapes, and a command makes at most {MAX_PRODUCED}: the paths cross or touch in too many places"
    ))
}

/// The ends of the pieces, in a hash of cells [`MERGE`] wide: which piece, and where.
fn ends_of(pieces: &[Run]) -> HashMap<(i64, i64), Vec<(usize, Point)>> {
    let mut grid: HashMap<(i64, i64), Vec<(usize, Point)>> = HashMap::new();
    for (k, piece) in pieces.iter().enumerate() {
        for p in [piece.start, piece.end()] {
            grid.entry(cell_of(p)).or_default().push((k, p));
        }
    }
    grid
}

/// Whether some piece other than `k` has an end within [`MERGE`] of `at`: a place where something meets the piece `k`. Every entry looked at is charged to `steps`.
fn met_at(grid: &HashMap<(i64, i64), Vec<(usize, Point)>>, k: usize, at: Point, steps: &mut u64) -> Result<bool, Problem> {
    let (cx, cy) = cell_of(at);
    for dx in -1..=1_i64 {
        for dy in -1..=1_i64 {
            let Some(entries) = grid.get(&(cx.saturating_add(dx), cy.saturating_add(dy))) else { continue };
            *steps = steps.checked_sub(entries.len() as u64).ok_or_else(|| {
                Problem::failed(format!("Trim would take more than {STEPS} steps to find which cut paths meet: too many paths end close together, and the command is refused"))
            })?;
            if entries.iter().any(|(other, p)| *other != k && p.distance(at) <= MERGE) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// The pieces of the paths `runs`, in the order of R35, each with whether it is a closed path that nothing cut. Refused in words when the library cannot cut them in its
/// bounds, when it gives nothing for paths that draw something, and when it gives more than [`MAX_PRODUCED`].
pub(super) fn cuts(runs: &mut [Run]) -> Result<Vec<Cut>, Problem> {
    // Each path comes back as a piece at least, unless every stretch of it lies along an earlier path's (which the library refuses past its limits on stacked copies): more paths
    // than a command makes shapes of are refused before the library is asked, which costs nothing.
    if runs.len() > MAX_PRODUCED {
        return Err(too_many());
    }
    // The end of an open path within the tolerance of another path touches it: it is moved onto it, and the library then finds the touch.
    let mut snap_steps = snap::STEPS;
    snap::snap_ends(runs, &mut snap_steps)?;
    let pieces = ops::trim_pieces(runs)?;
    if pieces.is_empty() {
        return Err(Problem::failed("the path library found nothing to cut in the shapes' paths"));
    }
    if pieces.len() > MAX_PRODUCED {
        return Err(too_many());
    }
    let grid = ends_of(&pieces);
    let mut steps = STEPS;
    let mut wholes = Vec::with_capacity(pieces.len());
    for (k, run) in pieces.iter().enumerate() {
        wholes.push(run.closed() && !met_at(&grid, k, run.start, &mut steps)?);
    }
    Ok(pieces.into_iter().zip(wholes).map(|(run, whole)| Cut { run, whole }).collect())
}
