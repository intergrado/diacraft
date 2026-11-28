//! The tests of the kept scene: what a scene reads, what it reports, that one put right equals one read whole after random edits, what it costs by count, what it
//! holds, and what a session keeps.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod build;
mod caps;
mod counted;
mod diff;
mod kept;
mod links;
mod memory;
mod oracle;
mod props;
mod read;
mod settings;
mod timed;

use diacraft_route::{Rect, Work};

use super::{ConnectorFacts, Cut, PageCells, Scene};
use crate::draw::obstacle::ObstacleFacts;

/// A work that does not run out.
pub(super) fn plenty() -> Work {
    Work::new(u64::MAX)
}

/// A candidate: its `ID`, its facts, whether it is an obstacle, and (when it is) the obstacle the router holds: its box and its two pass flags.
pub(super) type CandidateRow = (u32, ObstacleFacts, bool, Option<(Rect, bool, bool)>);

/// Every field of a scene that is not history (the box it is settled on is), sorted, so that two scenes can be held equal.
#[derive(Debug, PartialEq)]
pub(super) struct Dump {
    pub page: u32,
    /// Each candidate, whether it is in the router's obstacles, and (when it is) the obstacle the router holds.
    pub candidates: Vec<CandidateRow>,
    pub obstacle_count: usize,
    pub connectors: Vec<ConnectorFacts>,
    pub wire_count: usize,
    pub wire_legs: usize,
    pub glue_from: Vec<(u32, [Option<u32>; 2])>,
    pub glue_to: Vec<u32>,
    pub cut: Cut,
    pub cells: PageCells,
    pub tops: usize,
    pub held: u64,
}

impl Scene {
    pub(super) fn dump(&self) -> Dump {
        let mut candidates: Vec<_> = self
            .cands
            .iter()
            .map(|(id, c)| (*id, c.facts, c.obstacle, self.obstacles.get(*id).map(|o| (o.rect, o.pass_level, o.pass_upright))))
            .collect();
        candidates.sort_by_key(|c| c.0);
        let mut connectors: Vec<ConnectorFacts> = self.tops.keys().filter_map(|id| self.connector(*id)).collect();
        connectors.sort_by_key(|c| c.id);
        let mut glue_from: Vec<_> = self.glue.from.iter().map(|(k, v)| (*k, *v)).collect();
        glue_from.sort_by_key(|c| c.0);
        let mut glue_to: Vec<u32> = self.glue.to.iter().copied().collect();
        glue_to.sort_unstable();
        Dump {
            page: self.page,
            candidates,
            obstacle_count: self.obstacles.len(),
            connectors,
            wire_count: self.wires.len(),
            wire_legs: self.wires.leg_count(),
            glue_from,
            glue_to,
            cut: self.cut,
            cells: self.cells,
            tops: self.tops.len(),
            held: self.held_bytes(),
        }
    }

    /// A scene of page `page` of `doc` with the caps `caps`, read whole.
    pub(super) fn read_with(doc: &diacraft_doc::Document, page: u32, caps: super::Caps) -> Scene {
        let mut scene = Scene::with_caps(page, caps);
        scene.put_right(doc, page, &mut plenty());
        scene
    }

    /// The `ID`s the router's indexes answer for a fixed set of windows: a scene whose buckets were made some other way answers the same.
    pub(super) fn answers(&self) -> Vec<(Vec<u32>, Vec<u32>)> {
        let mut out = Vec::new();
        for k in 0..24 {
            let (x, y) = (f64::from(k % 6) * 3.0 - 2.0, f64::from(k / 6) * 3.0 - 2.0);
            let window = Rect { x0: x, y0: y, x1: x + 2.5 + f64::from(k % 3), y1: y + 2.5 + f64::from(k % 2) };
            let hits = self.obstacles.query(&window, &mut plenty()).hits.iter().map(|o| o.id).collect();
            out.push((hits, self.wires.meeting(&window, &mut plenty())));
        }
        out
    }
}

/// Two scenes of one drawing are the same: every field and every answer.
pub(super) fn assert_same(put_right: &Scene, fresh: &Scene, context: &str) {
    assert_eq!(put_right.dump(), fresh.dump(), "{context}: a scene put right is not the one read whole");
    assert_eq!(put_right.answers(), fresh.answers(), "{context}: the indexes of a scene put right answer otherwise");
}
