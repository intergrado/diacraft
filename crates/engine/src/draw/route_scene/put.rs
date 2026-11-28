//! Putting a scene right: the walk of the page's top-level pointers, the reads of what differs, and the updates of the router's indexes (see the parent module's "How it stays right").
//!
//! **One pass, in four steps.** (A) every top-level pointer is compared with the allocation the scene read, which says what differs without reading a cell; (B) what is gone, and what
//! differs, is taken out of the scene (the cells it gave are kept aside, to be found again by `ID` where a shape moved from one top-level shape to another by a group or an ungroup); (C) the
//! top-level shapes are visited in page order, each kept as it is, if it is valid within what is left of the budgets, or read; (D) what a glue changed is put right, and what was kept aside and
//! not found again is reported gone. A pass that reads the page **whole** (`fresh`) takes nothing from the scene but the boxes it was settled on, and builds the router's indexes in one go.
//! **A pass that stops** (its work ran out, or it met a repeated `ID` while the scene was being put right by pointer, and is made again whole) puts the candidates back as it found them
//! ([`Scene::restore_baseline`]: what it took out is kept to the end, whether it was found again or not, and what it put in is listed), so that the pass after it, and the one after that, compare
//! with the boxes the last complete pass settled on. Work is charged **before** what it pays for is read: a pointer, a shape, a group's member, the rows of a route.
//!
//! **Three budgets, each in page order.** The two-dimensional shapes looked at (`Caps::shapes`: a group's members each count), the one-dimensional ones (`Caps::lines`), and the rows of routes
//! read (`Caps::route_rows`, at least one for each connector: a connector of no rows costs the one). A shape is not read once its budget is spent, and **a shape that is not read is held
//! all the same, as skipped, with its pointer**, so that one that has not changed costs the compare and no read, however far past the cap the page goes. What kind a shape is, when its own
//! budget is spent, is told by the four cells that say it is one-dimensional, not by a read of its box.

use std::collections::HashMap;

use diacraft_doc::{Document, Owner, Page, SectionKey, Shape, ShapeRef, SheetRef};
use diacraft_route::{Obstacle, Point, Step, Wire, Work};

use super::price::Parts;
use super::{
    Body, Cand, Changed, ChangedObstacle, Class, Cut, Glue, Inputs, LegsState, Line, Mark, PageCells, Scene, Top, clamp_spacing, obstacle_of,
    rect_of, wire_of,
};
use crate::draw::hand::base_route_within;
use crate::draw::obstacle::{HiddenLayers, NotObstacle, candidates_below, read_candidate};
use crate::draw::read::{Reader, units_per_inch};
use crate::draw::reroute::Reroute;
use crate::draw::routing::{self, page_routing};
use crate::draw::style::path_style;

/// Why a pass stopped.
#[derive(Debug)]
enum Stop {
    /// The work it was given ran out.
    Work,
    /// A shape `ID` was met twice in a pass that was making the scene's own tables: the pass is made again whole, which takes the first shape of an `ID` and leaves the others out.
    Collision,
}

/// What is left of the budgets, as the pass visits the page's shapes in order.
struct Budgets {
    shapes: usize,
    lines: usize,
    rows: usize,
    /// A connector did not fit in the rows that were left: every one after it is cut, whatever it needs.
    rows_cut: bool,
}

/// The router's indexes built in one go, for a pass that reads the page whole.
struct Bulk {
    on: bool,
    obstacles: Vec<Obstacle>,
    wires: Vec<Wire>,
}

/// What one pass carries from step to step.
struct Pass<'a> {
    reader: &'a Reader<'a>,
    hidden: &'a HiddenLayers,
    fresh: bool,
    /// The candidates the pass took out of the scene, to be found again by `ID`: the box each was settled on, and the facts it had. **A candidate found again stays here**, so that a pass that stops
    /// can put every one back ([`Scene::restore_baseline`]).
    removed: HashMap<u32, Cand>,
    /// The `ID`s of the candidates the pass put into the scene, which a pass that stops takes out again.
    added: Vec<u32>,
    reports: Vec<ChangedObstacle>,
    bulk: Bulk,
    budgets: Budgets,
    cut: Cut,
    collided: bool,
}

/// What one visit to a kept connector found.
enum Refresh {
    Fine,
    NeedsRead,
}

fn leg_count(parts: &mut Parts, body: &Body) {
    match body {
        Body::Members(ids) => parts.members += ids.len() as u64,
        Body::Line(line) => {
            parts.lines += 1;
            parts.legs += line.legs.len() as u64;
        }
        Body::Nothing | Body::Own | Body::Skipped(_) => {}
    }
}

impl Scene {
    /// The counts of what the scene keeps.
    pub(crate) fn parts(&self) -> Parts {
        let mut parts = Parts {
            tops: self.tops.len() as u64,
            candidates: self.cands.len() as u64,
            obstacles: self.obstacles.len() as u64,
            wires: self.wires.len() as u64,
            wire_legs: self.wires.leg_count() as u64,
            glue_from: self.glue.from.len() as u64,
            glue_to: self.glue.to.len() as u64,
            ..Parts::default()
        };
        for top in self.tops.values() {
            leg_count(&mut parts, &top.body);
        }
        parts
    }

    /// Everything the scene held, let go: the page is not there, or is another.
    fn clear_all(&mut self) {
        let (reads, caps, page) = (self.reads, self.caps, self.page);
        *self = Scene::with_caps(page, caps);
        self.reads = reads;
    }

    /// The page is not in the document: every obstacle the scene settled on is gone.
    fn page_gone(&mut self, page: u32) -> Changed {
        let mut reports: Vec<ChangedObstacle> =
            self.cands.iter().filter_map(|(id, c)| c.base.map(|old| ChangedObstacle { id: *id, old: Some(old), new: None })).collect();
        reports.sort_by_key(|c| c.id);
        self.clear_all();
        self.page = page;
        Changed { obstacles: reports, incomplete: false }
    }

    pub(super) fn put_right_in(&mut self, doc: &Document, page_id: u32, work: &mut Work) -> Changed {
        let Some(page) = doc.page(page_id) else { return self.page_gone(page_id) };
        if self.page != page_id {
            self.clear_all();
            self.page = page_id;
        }
        let inputs_same = self.inputs.as_ref().is_some_and(|inputs| inputs.same(doc, page));
        let fresh = !inputs_same || self.collided || self.broken;
        let shapes_same = self.shapes.same(page.shapes.top_arc());
        let connects_same = self.connects.same(page.connects.shared_list());
        if !fresh && shapes_same && connects_same && self.dirty.is_empty() {
            return Changed::default();
        }
        let result = match self.pass(doc, page, fresh, work) {
            Err(Stop::Collision) => self.pass(doc, page, true, work),
            other => other,
        };
        match result {
            Ok(changed) => changed,
            Err(_) => {
                // The scene is partly the new page's and partly the old: it says so, and the next put right reads the page whole.
                self.broken = true;
                Changed { obstacles: Vec::new(), incomplete: true }
            }
        }
    }

    /// One pass (see the module's documentation): `fresh` reads the page whole. **A pass that stops leaves the boxes the scene is settled on as the last pass that was complete left them**
    /// ([`Scene::restore_baseline`]): the next one, whether it is the retry a repeated `ID` asks for or comes after work that ran out, reports from them.
    fn pass(&mut self, doc: &Document, page: &Page, fresh: bool, work: &mut Work) -> Result<Changed, Stop> {
        let caps = self.caps;
        let reader = Reader::new(doc);
        let hidden = HiddenLayers::of(doc, Owner::Page(self.page));
        self.epoch = self.epoch.wrapping_add(1);
        let mut pass = Pass {
            reader: &reader,
            hidden: &hidden,
            fresh,
            removed: HashMap::new(),
            added: Vec::new(),
            reports: Vec::new(),
            bulk: Bulk { on: fresh, obstacles: Vec::new(), wires: Vec::new() },
            budgets: Budgets { shapes: caps.shapes, lines: caps.lines, rows: caps.route_rows, rows_cut: false },
            cut: Cut::default(),
            collided: false,
        };
        let result = self.walk_page(doc, page, &mut pass, work);
        if result.is_err() {
            self.restore_baseline(&mut pass);
        }
        result
    }

    /// Puts the scene's candidates back as they were when the pass began, for a pass that stopped: what it put in is taken out and what it took out is put back, with the box each was settled on and
    /// the facts it was read with. (The scene was the last complete pass's when this one began: by this very rule, after a pass that stopped.) The rest of what the pass changed, the shapes, the
    /// router's indexes and the glue, is not put back, and need not be: the scene says it is not the drawing's, or the pass is made again whole, and both read the page whole.
    fn restore_baseline(&mut self, pass: &mut Pass<'_>) {
        for id in pass.added.drain(..) {
            self.cands.remove(&id);
        }
        self.cands.extend(std::mem::take(&mut pass.removed));
    }

    /// The body of [`Scene::pass`]: the four steps over the page.
    fn walk_page(&mut self, doc: &Document, page: &Page, pass: &mut Pass<'_>, work: &mut Work) -> Result<Changed, Stop> {
        let (caps, fresh, epoch) = (self.caps, pass.fresh, self.epoch);
        if fresh {
            self.reads.whole += 1;
            pass.removed = std::mem::take(&mut self.cands);
            self.tops.clear();
            self.dirty.clear();
            self.obstacles = diacraft_route::Obstacles::new(Vec::new());
            self.wires = diacraft_route::Wires::new(Vec::new());
            self.cells = read_cells(doc, self.page);
        }
        // The glue, from one pass over the page's `Connect` rows.
        let connects_same = self.connects.same(page.connects.shared_list());
        let mut old_to = None;
        if fresh || !connects_same {
            let (glue, cut) = read_glue(page, caps.connect_rows, work, &mut self.reads)?;
            old_to = Some(std::mem::replace(&mut self.glue, glue).to);
            self.connects_cut = cut;
        }
        // (A) What differs, by pointer.
        let limit = caps.shapes.saturating_add(caps.lines);
        let mut differs: Vec<usize> = Vec::new();
        let (mut seen, mut collision) = (0usize, false);
        for (i, shape) in page.shapes.iter().take(limit).enumerate() {
            self.reads.walked += 1;
            if !work.charge_for(Step::Visit, 1) {
                return Err(Stop::Work);
            }
            match self.tops.get_mut(&shape.id) {
                Some(top) if top.epoch == epoch => collision = true,
                Some(top) => {
                    top.epoch = epoch;
                    seen += 1;
                    if !top.mark.same(Some(shape)) || (!self.dirty.is_empty() && self.dirty.contains(&shape.id)) {
                        differs.push(i);
                    }
                }
                None => differs.push(i),
            }
        }
        if collision && !fresh {
            return Err(Stop::Collision);
        }
        // (B) What is gone, and what differs, is taken out.
        if seen < self.tops.len() {
            let gone: Vec<u32> = self.tops.iter().filter(|(_, top)| top.epoch != epoch).map(|(id, _)| *id).collect();
            for id in gone {
                self.drop_top(id, pass);
            }
        }
        for i in &differs {
            if let Some(shape) = page.shapes.get(*i) {
                self.drop_top(shape.id, pass);
            }
        }
        // (C) The shapes, in page order.
        let mut next_diff = 0usize;
        for (i, shape) in page.shapes.iter().take(limit).enumerate() {
            let differs_here = differs.get(next_diff) == Some(&i);
            if differs_here {
                next_diff += 1;
            }
            if !differs_here && self.keep_top(shape, pass) {
                continue;
            }
            // The place of the shape is made for a shape that is read: the others cost no allocation.
            let at = ShapeRef { owner: Owner::Page(self.page), path: vec![u32::try_from(i).map_err(|_| Stop::Work)?] };
            self.read_top(shape, &at, pass, work)?;
        }
        // (D) What a glue changed, and what went.
        if let Some(old) = old_to {
            let flips: Vec<u32> = old.symmetric_difference(&self.glue.to).copied().collect();
            for id in flips {
                self.reevaluate(id, pass);
            }
        }
        // What was taken out and is not in the scene now has gone: what was found again is in `cands`.
        for (id, old) in &pass.removed {
            if let (false, Some(base)) = (self.cands.contains_key(id), old.base) {
                pass.reports.push(ChangedObstacle { id: *id, old: Some(base), new: None });
            }
        }
        if fresh {
            self.obstacles = diacraft_route::Obstacles::new(std::mem::take(&mut pass.bulk.obstacles));
            self.wires = diacraft_route::Wires::new(std::mem::take(&mut pass.bulk.wires));
        }
        pass.reports.sort_by_key(|c| c.id);
        pass.cut.shapes |= page.shapes.len() > limit;
        pass.cut.connects = self.connects_cut;
        self.cut = pass.cut;
        self.inputs = Some(Inputs::of(doc, page));
        self.shapes = Mark::of(page.shapes.top_arc());
        self.connects = Mark::of(page.connects.shared_list());
        self.dirty.clear();
        self.broken = false;
        self.collided = pass.collided;
        Ok(Changed { obstacles: std::mem::take(&mut pass.reports), incomplete: false })
    }

    /// Takes the top-level shape `id` out of the scene: its candidates and its wire. What it gave is kept aside for the pass to find again.
    fn drop_top(&mut self, id: u32, pass: &mut Pass<'_>) {
        let Some(top) = self.tops.remove(&id) else { return };
        let ids = match top.body {
            Body::Own => vec![id],
            Body::Members(ids) => ids,
            Body::Line(line) => {
                if !line.legs.is_empty() && !pass.bulk.on {
                    self.wires.replace(id, None);
                }
                Vec::new()
            }
            Body::Nothing | Body::Skipped(_) => Vec::new(),
        };
        for cid in ids {
            if let Some(cand) = self.cands.remove(&cid) {
                if cand.obstacle && !pass.bulk.on {
                    self.obstacles.replace(cid, None);
                }
                pass.removed.insert(cid, cand);
            }
        }
    }

    /// Whether the kept top-level shape (the same allocation) is valid within what is left of the budgets: it then takes its share of them, and nothing is read. A kept connector's glue and
    /// rows are brought up to date without reading it. `false` when it must be read again (and it is taken out first).
    fn keep_top(&mut self, shape: &std::sync::Arc<Shape>, pass: &mut Pass<'_>) -> bool {
        let id = shape.id;
        let Some(top) = self.tops.get(&id) else { return false };
        let (class, cost, limit) = (top.class(), top.cost, top.limit);
        let valid = match class {
            Class::TwoD => {
                if matches!(top.body, Body::Skipped(_)) {
                    if pass.budgets.shapes == 0 {
                        pass.cut.shapes = true;
                        true
                    } else {
                        false
                    }
                } else {
                    let fits = fits(cost, limit, pass.budgets.shapes);
                    if fits {
                        pass.budgets.shapes = pass.budgets.shapes.saturating_sub(usize::try_from(cost).unwrap_or(usize::MAX));
                        pass.cut.shapes |= limit.is_some();
                    }
                    fits
                }
            }
            Class::OneD => {
                if matches!(top.body, Body::Skipped(_)) {
                    if pass.budgets.lines == 0 {
                        pass.cut.lines = true;
                        true
                    } else {
                        false
                    }
                } else if pass.budgets.lines == 0 {
                    false
                } else {
                    pass.budgets.lines -= 1;
                    match self.refresh_line(id, pass) {
                        Refresh::Fine => true,
                        Refresh::NeedsRead => {
                            pass.budgets.lines += 1;
                            false
                        }
                    }
                }
            }
        };
        if !valid {
            self.drop_top(id, pass);
        }
        valid
    }

    /// Brings a kept connector's wire up to date with the rows left and the glue read now, without reading it.
    fn refresh_line(&mut self, id: u32, pass: &mut Pass<'_>) -> Refresh {
        let glue_now = self.glue.from.get(&id).copied();
        let rows_each = self.caps.rows_each;
        let Some(Top { body: Body::Line(line), .. }) = self.tops.get_mut(&id) else { return Refresh::NeedsRead };
        let wired_now = line.ours || glue_now.is_some();
        if wired_now != line.wired {
            return Refresh::NeedsRead;
        }
        if !line.wired {
            return Refresh::Fine;
        }
        // The rows it would cost, tried on a copy: a connector that is to be read takes its rows when it is read.
        let (mut rows, mut rows_cut) = (pass.budgets.rows, pass.budgets.rows_cut);
        let state = rows_state(line.held, &mut rows, &mut rows_cut, rows_each);
        if state != line.state && state == LegsState::Read {
            return Refresh::NeedsRead;
        }
        pass.budgets.rows = rows;
        pass.budgets.rows_cut = rows_cut;
        pass.cut.long_routes |= state == LegsState::Long;
        pass.cut.route_rows |= state == LegsState::Cut;
        if state != line.state {
            let had = !line.legs.is_empty();
            line.legs.clear();
            line.state = state;
            if had && !pass.bulk.on {
                self.wires.replace(id, None);
            }
            return Refresh::Fine;
        }
        let glue = glue_now.unwrap_or_default();
        if glue != line.glue {
            line.glue = glue;
            if !line.legs.is_empty() && !pass.bulk.on {
                self.wires.replace(id, Some(wire_of(id, &line.legs, glue)));
            }
        }
        Refresh::Fine
    }

    /// Reads the top-level shape `shape` (at `at`), which is not in the scene (it is new, differs, or was not valid), and puts what it gives in.
    fn read_top(&mut self, shape: &std::sync::Arc<Shape>, at: &ShapeRef, pass: &mut Pass<'_>, work: &mut Work) -> Result<(), Stop> {
        let id = shape.id;
        if self.tops.contains_key(&id) {
            // A second shape with the `ID` of one that is in the scene.
            pass.collided = true;
            return if pass.fresh { Ok(()) } else { Err(Stop::Collision) };
        }
        let mark = Mark::of(Some(shape));
        let reader = pass.reader;
        let budgets_spent = |b: &Budgets| (b.shapes == 0, b.lines == 0);
        let (shapes_spent, lines_spent) = budgets_spent(&pass.budgets);
        // What kind of shape it is, told without a read of its box when its own budget is spent and the other's is not.
        if shapes_spent && lines_spent {
            return self.skip(id, mark, at, pass, work);
        }
        if shapes_spent {
            if !reader.is_one_d(&SheetRef::Shape(at.clone())) {
                return self.skip_two_d(id, mark, pass, work);
            }
        } else if lines_spent && reader.is_one_d(&SheetRef::Shape(at.clone())) {
            return self.skip_one_d(id, mark, pass, work);
        }
        if !work.charge_for(Step::Visit, 1) {
            return Err(Stop::Work);
        }
        self.reads.shapes += 1;
        match read_candidate(reader, at, pass.hidden) {
            Err(NotObstacle::OneD) => self.read_line(id, mark, at, pass, work),
            read => {
                match read {
                    Ok(facts) if facts.kind.members && !facts.kind.placeable => {
                        // The walk takes the group's own look from the budget with its members'.
                        let before = pass.budgets.shapes;
                        let mut budget = before;
                        // The top's own look was charged above: the walk charges each member's before it reads it.
                        let found = candidates_below(reader, at, facts, pass.hidden, &mut budget, work);
                        let looked = before - budget;
                        let looks = u64::try_from(looked).unwrap_or(u64::MAX);
                        self.reads.shapes += looks.saturating_sub(1);
                        if found.refused {
                            return Err(Stop::Work);
                        }
                        pass.budgets.shapes = budget;
                        pass.cut.shapes |= found.cut;
                        let limit = found.cut.then(|| u32::try_from(before).unwrap_or(u32::MAX));
                        let mut ids = Vec::new();
                        for member in found.obstacles {
                            if self.add_cand(id, member, pass)? {
                                ids.push(member.id);
                            }
                        }
                        let cost = u32::try_from(looked).unwrap_or(u32::MAX);
                        self.tops.insert(id, Top { mark, epoch: self.epoch, body: Body::Members(ids), cost, limit });
                    }
                    Ok(facts) => {
                        pass.budgets.shapes = pass.budgets.shapes.saturating_sub(1);
                        let body = if self.add_cand(id, facts, pass)? { Body::Own } else { Body::Nothing };
                        self.tops.insert(id, Top { mark, epoch: self.epoch, body, cost: 1, limit: None });
                    }
                    Err(_) => {
                        pass.budgets.shapes = pass.budgets.shapes.saturating_sub(1);
                        self.tops.insert(id, Top { mark, epoch: self.epoch, body: Body::Nothing, cost: 1, limit: None });
                    }
                }
                Ok(())
            }
        }
    }

    /// A shape neither budget has room for, whose kind is one of the cells' to tell.
    fn skip(&mut self, id: u32, mark: Mark<Shape>, at: &ShapeRef, pass: &mut Pass<'_>, work: &mut Work) -> Result<(), Stop> {
        if pass.reader.is_one_d(&SheetRef::Shape(at.clone())) { self.skip_one_d(id, mark, pass, work) } else { self.skip_two_d(id, mark, pass, work) }
    }

    fn skip_two_d(&mut self, id: u32, mark: Mark<Shape>, pass: &mut Pass<'_>, work: &mut Work) -> Result<(), Stop> {
        pass.cut.shapes = true;
        self.skipped(id, mark, Class::TwoD, work)
    }

    fn skip_one_d(&mut self, id: u32, mark: Mark<Shape>, pass: &mut Pass<'_>, work: &mut Work) -> Result<(), Stop> {
        pass.cut.lines = true;
        self.skipped(id, mark, Class::OneD, work)
    }

    fn skipped(&mut self, id: u32, mark: Mark<Shape>, class: Class, work: &mut Work) -> Result<(), Stop> {
        if !work.charge_for(Step::Visit, 1) {
            return Err(Stop::Work);
        }
        self.reads.peeks += 1;
        self.tops.insert(id, Top { mark, epoch: self.epoch, body: Body::Skipped(class), cost: 0, limit: None });
        Ok(())
    }

    /// Puts a candidate of top-level shape `top` into the scene: its box settled by the glue now, the router told if it is an obstacle, and what changed reported (a setting is none: see the
    /// parent module's "What it reports"). → `false` when another shape holds the `ID` (the first holds it, and a pass that is not reading the page whole is made again).
    fn add_cand(&mut self, top: u32, facts: crate::draw::obstacle::ObstacleFacts, pass: &mut Pass<'_>) -> Result<bool, Stop> {
        if self.cands.contains_key(&facts.id) {
            pass.collided = true;
            return if pass.fresh { Ok(false) } else { Err(Stop::Collision) };
        }
        let glued = self.glue.to.contains(&facts.id);
        let obstacle = facts.check(glued).is_ok();
        let now = obstacle.then(|| rect_of(&facts));
        // The candidate it was before the pass, if it was one: it stays in `removed`, for a pass that stops to put it back.
        let base = match pass.removed.get(&facts.id) {
            None => {
                if let Some(n) = now {
                    pass.reports.push(ChangedObstacle { id: facts.id, old: None, new: Some(n) });
                }
                now
            }
            Some(old) => {
                let moved = old.facts.bounds != facts.bounds || old.facts.hidden != facts.hidden;
                if moved {
                    if now != old.base {
                        pass.reports.push(ChangedObstacle { id: facts.id, old: old.base, new: now });
                    }
                    now
                } else {
                    old.base
                }
            }
        };
        if obstacle {
            let o = obstacle_of(&facts);
            if pass.bulk.on {
                pass.bulk.obstacles.push(o);
            } else {
                self.obstacles.replace(facts.id, Some(o));
            }
        }
        self.cands.insert(facts.id, Cand { top, facts, obstacle, base });
        pass.added.push(facts.id);
        Ok(true)
    }

    /// A glue came or went for shape `id`: it is an obstacle or not by its facts, with no read of the shape. The box it is settled on stays (item 3).
    fn reevaluate(&mut self, id: u32, pass: &mut Pass<'_>) {
        let glued = self.glue.to.contains(&id);
        let Some(cand) = self.cands.get_mut(&id) else { return };
        let obstacle = cand.facts.check(glued).is_ok();
        if obstacle == cand.obstacle {
            return;
        }
        cand.obstacle = obstacle;
        if pass.bulk.on {
            return;
        }
        let o = obstacle.then(|| obstacle_of(&cand.facts));
        self.obstacles.replace(id, o);
    }

    /// Reads a one-dimensional top-level shape: whether it is a connector, its setting and style, and (for one that is, within the rows left) the legs of its route.
    fn read_line(&mut self, id: u32, mark: Mark<Shape>, at: &ShapeRef, pass: &mut Pass<'_>, work: &mut Work) -> Result<(), Stop> {
        pass.budgets.lines = pass.budgets.lines.saturating_sub(1);
        let reader = pass.reader;
        let sheet = SheetRef::Shape(at.clone());
        let ours = reader.is_routed(&sheet);
        let glue_found = self.glue.from.get(&id).copied();
        let wired = ours || glue_found.is_some();
        let glue = glue_found.unwrap_or_default();
        let page = self.page;
        let mut line = Line {
            ours,
            wired,
            held: 0,
            state: LegsState::Unread,
            setting: Reroute::from_code(reader.number(&sheet, "ConFixedCode")),
            style: path_style(reader, page, &sheet),
            glue,
            legs: Vec::new(),
        };
        if wired {
            line.held = reader.resolver().rows_held(&sheet, &SectionKey::new("Geometry", Some(0))).unwrap_or(0);
            line.state = rows_state(line.held, &mut pass.budgets.rows, &mut pass.budgets.rows_cut, self.caps.rows_each);
            pass.cut.long_routes |= line.state == LegsState::Long;
            pass.cut.route_rows |= line.state == LegsState::Cut;
            if line.state == LegsState::Read {
                let rows = u64::try_from(line.held).unwrap_or(u64::MAX);
                if !work.charge_for(Step::Visit, rows) {
                    return Err(Stop::Work);
                }
                self.reads.lines += 1;
                self.reads.route_rows += rows;
                if let Ok(Some(points)) = base_route_within(reader, &sheet, &reader.pose(&sheet), line.held) {
                    line.legs = points.windows(2).map(|w| (Point::new(w[0].0, w[0].1), Point::new(w[1].0, w[1].1))).collect();
                }
            }
        }
        if !line.legs.is_empty() {
            let wire = wire_of(id, &line.legs, line.glue);
            if pass.bulk.on {
                pass.bulk.wires.push(wire);
            } else {
                self.wires.replace(id, Some(wire));
            }
        }
        self.tops.insert(id, Top { mark, epoch: self.epoch, body: Body::Line(Box::new(line)), cost: 1, limit: None });
        Ok(())
    }

    /// Makes `legs` the legs of connector `id`: see [`Scene::set_legs`].
    pub(super) fn set_legs_of(&mut self, id: u32, legs: Vec<(Point, Point)>) -> bool {
        let Some(Top { body: Body::Line(line), .. }) = self.tops.get_mut(&id) else { return false };
        if !line.wired {
            return false;
        }
        line.legs = legs;
        line.state = LegsState::Read;
        let glue = line.glue;
        if line.legs.is_empty() {
            self.wires.replace(id, None);
        } else {
            self.wires.replace(id, Some(wire_of(id, &line.legs, glue)));
        }
        self.dirty.insert(id);
        true
    }
}

impl Top {
    /// Which kind of shape it was read as.
    fn class(&self) -> Class {
        match &self.body {
            Body::Line(_) => Class::OneD,
            Body::Skipped(class) => *class,
            Body::Nothing | Body::Own | Body::Members(_) => Class::TwoD,
        }
    }
}

/// Whether a kept two-dimensional top-level shape, read for `cost` shapes (and, if its members were cut by the budget, with `limit` shapes of budget), is what a read now would give with `left`
/// shapes of budget left: one read whole fits while it still has the room it took, and one that was cut only when it has the very budget that cut it.
pub(super) fn fits(cost: u32, limit: Option<u32>, left: usize) -> bool {
    match limit {
        None => usize::try_from(cost).is_ok_and(|cost| cost <= left),
        Some(was) => usize::try_from(was).is_ok_and(|was| was == left),
    }
}

/// How much of a connector's route is read, by the rows it holds and the rows left: more rows than a route may have is [`LegsState::Long`] and costs nothing; one that does not fit in what is
/// left, and every one after it, is [`LegsState::Cut`]; the others are read, each costing at least one row (so that connectors with no route are bounded too).
fn rows_state(held: usize, rows: &mut usize, rows_cut: &mut bool, rows_each: usize) -> LegsState {
    if held > rows_each {
        return LegsState::Long;
    }
    let cost = held.max(1);
    if *rows_cut || cost > *rows {
        *rows_cut = true;
        return LegsState::Cut;
    }
    *rows -= cost;
    LegsState::Read
}

/// The glue of the page's connectors from its `Connect` rows, at most `cap` of them, every row passed charged to `work` first. → the glue, and whether rows were left unread.
fn read_glue(page: &Page, cap: usize, work: &mut Work, reads: &mut super::Reads) -> Result<(Glue, bool), Stop> {
    let total = page.connects.len();
    let take = total.min(cap);
    let rows = u64::try_from(take).unwrap_or(u64::MAX);
    if !work.charge_for(Step::Visit, rows) {
        return Err(Stop::Work);
    }
    reads.connect_rows += rows;
    let mut glue = Glue::default();
    for c in page.connects.iter().take(take) {
        let end = match c.from_cell.as_deref() {
            Some("BeginX" | "BeginY") => 0,
            Some("EndX" | "EndY") => 1,
            _ => continue,
        };
        if let Some(slot @ None) = glue.from.entry(c.from_sheet).or_default().get_mut(end) {
            *slot = Some(c.to_sheet);
        }
        if c.to_sheet != c.from_sheet {
            glue.to.insert(c.to_sheet);
        }
    }
    Ok((glue, total > cap))
}

/// The page's routing cells as the router is given them: read through the cells' own readers (`routing::page_routing`), and clamped to one paper inch.
fn read_cells(doc: &Document, page: u32) -> PageCells {
    let routing = page_routing(doc, page);
    let per_inch = units_per_inch(doc, page);
    let clamp = |(x, y): (f64, f64)| (clamp_spacing(x, per_inch), clamp_spacing(y, per_inch));
    PageCells {
        clearance: clamp(routing.connector_to_shape),
        spacing: clamp(routing.connector_to_connector),
        separate: match routing.separate {
            routing::Separate::All => diacraft_route::Separate::All,
            routing::Separate::NoLines => diacraft_route::Separate::None,
            routing::Separate::Unrelated | routing::Separate::ByStyle => diacraft_route::Separate::Unrelated,
        },
        overlap: match routing.overlap {
            routing::Overlap::All => diacraft_route::Overlap::Any,
            routing::Overlap::NoLines => diacraft_route::Overlap::Never,
            routing::Overlap::Related | routing::Overlap::ByStyle => diacraft_route::Overlap::Related,
        },
        per_inch,
    }
}
