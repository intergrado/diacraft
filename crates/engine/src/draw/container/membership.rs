//! Who is in which container: joining, leaving, the shapes a drop or a move puts in or takes out, and the ties kept on both sides.
//!
//! (ours where the specifications are silent: they say a shape dropped inside joins, a shape moved a
//! little inside joins, a locked container takes and loses none, and give no rule for "inside"). A shape is **inside** a container when the middle of its box is in the
//! container's box; the container that takes it is the **innermost** one (the smallest box) that it is inside, and a shape is a direct member of at most one container, which is
//! the innermost one it is inside: a shape moved into an inner container leaves the outer one, which holds the inner container. A shape leaves its container when its middle is
//! no longer in the container's box. A locked container takes and loses no member: a shape that is in one stays in it, and cannot be moved out of it, and one dropped over it does
//! not join. A shape never joins a container it holds, itself or through its members, and a list takes no member by a drop. A **container** that is dropped, moved or pasted joins
//! another only when its whole box is inside (a copy a quarter inch off its original has its middle in the original, and is not inside it).
//!
//! A shape whose `Relationships` formula is not a list of ties (a `SUM` of `DEPENDSON` calls) is never rewritten: the automatic steps (a move's membership, a
//! fit) leave it and its containers as they are and say nothing, and a command that names it is refused in words.
//!
//! (a shape on a locked layer is not moved or edited, and the specifications say nothing of a container on one, so it is passed
//! over and the command says so). A container whose own list names a locked layer takes and loses no member by a drop, a move or a paste: a shape over it does not join it (it
//! joins the container under it that is not, if one is), a member moved out of it stays a member, and it is not fitted; the command notes each container it passed over
//! ([`Page::pass_over`]). A tie to a shape that is gone still goes, as a connector's glue to a shape deleted does.

use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap, HashSet};

use diacraft_doc::{CellAddr, Document, ShapeType};

use super::page::{Page, Unique};
use crate::draw::ties::{IN_CONTAINER, IN_LIST, LIST_MEMBERS, MAX_TIE_MARKS, MEMBERS, ON_EDGE, Ties};
use crate::draw::{Problem, shape_mut};

/// The ties of the shapes a command changes, read once and written once.
#[derive(Default)]
pub(super) struct Graph {
    /// The ties of each shape touched, and where the shape is in the page's list.
    ties: HashMap<u32, (usize, Ties)>,
    dirty: BTreeSet<u32>,
    /// The shapes a walk met whose `Relationships` is no list of ties: they hold no member and are in no container for it, and are not read again.
    odd: HashSet<u32>,
}

/// The refusal of a tie that would put a container inside what it holds.
pub(super) fn holds_words(member: u32, container: u32) -> Problem {
    Problem::bad(format!("shape {member} holds container {container}: a container is not put inside what it holds"))
}

impl Graph {
    pub(super) fn new() -> Graph {
        Graph::default()
    }

    fn load(&mut self, page: &Page<'_>, id: u32) -> Result<&mut Ties, Problem> {
        if let Entry::Vacant(slot) = self.ties.entry(id) {
            let at = page.at(id).ok_or_else(|| Problem::failed(format!("page {} has no shape {id}", page.id)))?;
            slot.insert((at, page.ties(id)?));
        }
        self.ties.get_mut(&id).map(|(_, t)| t).ok_or_else(|| Problem::failed("a shape went away"))
    }

    /// The shapes next to `id` as this graph has the ties now: its members (`down`) or the containers it is in. A shape whose ties are no list has none, and each shape's ties are
    /// read once for the command, whatever the number of walks that meet it.
    fn near(&mut self, page: &Page<'_>, id: u32, down: bool) -> Result<Vec<u32>, Problem> {
        if self.odd.contains(&id) {
            return Ok(Vec::new());
        }
        if !self.ties.contains_key(&id) {
            let (Some(at), Some(ties)) = (page.at(id), page.plain_ties(id)?) else {
                self.odd.insert(id);
                return Ok(Vec::new());
            };
            self.ties.insert(id, (at, ties));
        }
        let Some((_, ties)) = self.ties.get(&id) else { return Ok(Vec::new()) };
        // What a shape holds, or what holds it, counts a list too (a lane is in its list, which is in the frame): a tie that would close a loop through one is refused too.
        let named: Vec<u32> = if down {
            [MEMBERS, LIST_MEMBERS].into_iter().flat_map(|code| ties.ids(code)).collect()
        } else {
            std::iter::once(IN_CONTAINER).chain(ON_EDGE).chain([IN_LIST]).flat_map(|code| ties.ids(code)).collect()
        };
        Ok(named.into_iter().filter(|other| *other != id && page.has(*other)).collect())
    }

    /// Walks from `from` down through the members, or up through the containers, each shape once and a step of the command's budget each: `stop` ends the walk when it is met. → whether
    /// `stop` was met, and every shape reached.
    fn walk(&mut self, page: &Page<'_>, from: u32, down: bool, stop: Option<u32>) -> Result<(bool, HashSet<u32>), Problem> {
        let mut seen: HashSet<u32> = HashSet::new();
        let mut stack = vec![from];
        while let Some(next) = stack.pop() {
            for other in self.near(page, next, down)? {
                page.work.charge(1)?;
                if Some(other) == stop {
                    return Ok((true, seen));
                }
                if other != from && seen.insert(other) {
                    stack.push(other);
                }
            }
        }
        Ok((false, seen))
    }

    /// The shapes the container `id` holds, nested ones too, as this graph has them now: what the command changed and the page's ties for the rest (the container a
    /// shape joins is never among them).
    pub(super) fn contents(&mut self, page: &Page<'_>, id: u32) -> Result<HashSet<u32>, Problem> {
        Ok(self.walk(page, id, true, None)?.1)
    }

    /// Whether `member` holds `container`, itself or through the shapes it holds, as this graph has the ties now: by its members, or, for a container, by the containers that
    /// `container` is in (a file can hold one side of a tie without the other). A tie between them would make a loop.
    pub(super) fn would_loop(&mut self, page: &Page<'_>, container: u32, member: u32) -> Result<bool, Problem> {
        if container == member || self.walk(page, member, true, Some(container))?.0 {
            return Ok(true);
        }
        // Only a container is a shape that other shapes name as the container they are in: a walk up from `container` is needed for one alone.
        Ok(page.is_container(member) && self.walk(page, container, false, Some(member))?.0)
    }

    /// The ties of a shape as this graph has them now.
    pub(super) fn ties(&mut self, page: &Page<'_>, id: u32) -> Result<&Ties, Problem> {
        self.load(page, id).map(|t| &*t)
    }

    /// The containers the shape is in now, with what this graph has changed.
    pub(super) fn containers_of(&mut self, page: &Page<'_>, id: u32) -> Result<Vec<u32>, Problem> {
        let ties = self.load(page, id)?;
        let mut out = Unique::new();
        for code in std::iter::once(IN_CONTAINER).chain(ON_EDGE) {
            for c in ties.ids(code) {
                if c != id && page.has(c) {
                    out.push(c);
                }
            }
        }
        Ok(out.into_vec())
    }

    /// Makes `member` a member of `container`: the container lists it under 1, and it names the container under 4 (or under the code of an edge, when `edge` says which). **A tie
    /// that would put a container inside what it holds is refused in words**, whichever command asks for it: membership stays a tree.
    pub(super) fn join(&mut self, page: &Page<'_>, container: u32, member: u32, edge: Option<usize>) -> Result<(), Problem> {
        if self.would_loop(page, container, member)? {
            return Err(holds_words(member, container));
        }
        let code = edge.and_then(|e| ON_EDGE.get(e).copied()).unwrap_or(IN_CONTAINER);
        let c = self.load(page, container)?;
        let mut changed = c.add(MEMBERS, member);
        if !c.has_code(MEMBERS) {
            c.ensure(MEMBERS);
            changed = true;
        }
        if changed {
            self.dirty.insert(container);
        }
        let m = self.load(page, member)?;
        let mut changed = false;
        for other in std::iter::once(IN_CONTAINER).chain(ON_EDGE).filter(|c| *c != code) {
            changed |= m.remove(other, container);
        }
        changed |= m.add(code, container);
        if changed {
            self.dirty.insert(member);
        }
        Ok(())
    }

    /// Puts `member` in the list `list` at the place `at` of its order (the end when `None`), on both sides: the list names it under 2 and it names the list under 5 (a shape is in one list).
    /// **A tie that would put a list inside what it holds is refused in words**.
    pub(super) fn list_put(&mut self, page: &Page<'_>, list: u32, member: u32, at: Option<usize>) -> Result<(), Problem> {
        if self.would_loop(page, list, member)? {
            return Err(holds_words(member, list));
        }
        let l = self.load(page, list)?;
        let mut order = l.ids(LIST_MEMBERS);
        order.retain(|m| *m != member);
        let place = at.map_or(order.len(), |a| a.min(order.len()));
        order.insert(place, member);
        if l.set_order(LIST_MEMBERS, &order) {
            self.dirty.insert(list);
        }
        let m = self.load(page, member)?;
        if m.set_order(IN_LIST, &[list]) {
            self.dirty.insert(member);
        }
        Ok(())
    }

    /// Puts the members of the list `list` in the order `order`, on the list's side (each member names its list as before).
    pub(super) fn list_order(&mut self, page: &Page<'_>, list: u32, order: &[u32]) -> Result<(), Problem> {
        let l = self.load(page, list)?;
        if l.set_order(LIST_MEMBERS, order) {
            self.dirty.insert(list);
        }
        Ok(())
    }

    /// Takes `member` out of `container`, on both sides.
    pub(super) fn leave(&mut self, page: &Page<'_>, container: u32, member: u32) -> Result<(), Problem> {
        self.leave_many(page, container, &[member])
    }

    /// Takes many shapes out of `container`, on both sides: the container's list once, and each shape's own.
    pub(super) fn leave_many(&mut self, page: &Page<'_>, container: u32, members: &[u32]) -> Result<(), Problem> {
        let set: HashSet<u32> = members.iter().copied().collect();
        let c = self.load(page, container)?;
        if c.remove_many(MEMBERS, &set) > 0 {
            self.dirty.insert(container);
        }
        for member in members {
            let m = self.load(page, *member)?;
            let mut changed = false;
            for code in std::iter::once(IN_CONTAINER).chain(ON_EDGE) {
                changed |= m.remove(code, container);
            }
            if changed {
                self.dirty.insert(*member);
            }
        }
        Ok(())
    }

    /// Writes the ties that changed. A shape left with no ties holds no `Relationships` cell of its own. A list that no formula can hold (the parser's node budget) is refused in
    /// words, and nothing is written.
    pub(super) fn write(self, doc: &mut Document, page: u32) -> Result<usize, Problem> {
        let Graph { mut ties, dirty, .. } = self;
        let mut writes: Vec<(usize, String)> = Vec::with_capacity(dirty.len());
        for id in &dirty {
            let Some((at, t)) = ties.get_mut(id) else { continue };
            // A call that a shape left empty goes, but a container's list of members stays, empty: it says the shape is a container.
            t.drop_emptied(&[MEMBERS]);
            let source = t.print();
            if t.named() > MAX_TIE_MARKS || (source != "SUM()" && !diacraft_formula::parse(&source).is_parsed()) {
                return Err(Problem::failed(format!(
                    "shape {id} would name more shapes than a formula can hold ({MAX_TIE_MARKS} at most): it holds too many containers or members"
                )));
            }
            writes.push((*at, source));
        }
        let mut written = 0_usize;
        for (at, source) in writes {
            let index = u32::try_from(at).map_err(|_| Problem::failed("a shape's place is out of range"))?;
            let shape = shape_mut(doc, page, &[index]).ok_or_else(|| Problem::failed("a shape went away"))?;
            let mut counted = shape.counted_sheet();
            let old = counted.sheet.cell("Relationships").cloned();
            if source == "SUM()" {
                counted.remove_cell(&CellAddr::direct("Relationships"));
            } else {
                let mut cell = diacraft_doc::Cell::with_formula(&source, Some(diacraft_doc::Value::from_text("0")));
                cell.unit = old.and_then(|c| c.unit);
                counted.set_cell(&CellAddr::direct("Relationships"), cell).map_err(|e| Problem::failed(e.to_string()))?;
            }
            written += 1;
        }
        Ok(written)
    }
}

/// The innermost container a shape is inside: the smallest box among the containers that take members and whose box has the middle of the shape's, never the shape itself or a
/// container the shape holds, never a locked one, and never one on a locked layer (which is noted as passed over when it would have been the one). `None`
/// when there is none, or the shape has no box. A container drawn later (in front) wins a tie. A spent budget is an error, and never "none".
///
/// `arriving` is the shape's containers when it has just been dropped, moved or pasted: a **container** then joins another only when its whole box is inside it, and stays in the
/// one it is in while its middle is. Any other shape, and the commands that name the container, go by the middle.
///
/// **What the shape holds is the graph's**: the ties the command has made so far count, not only the page's as it was read. A container put round
/// shapes holds them before the one it goes in is looked for, and a container that took another a moment earlier in the same command holds it (the page's read of the ties still
/// said it held nothing, so each was put in the other).
pub(super) fn underlying(page: &Page<'_>, graph: &mut Graph, id: u32, arriving: Option<&[u32]>) -> Result<Option<u32>, Problem> {
    let Some(me) = page.bounds(id)? else { return Ok(None) };
    let centre = me.centre();
    let held: HashSet<u32> = if page.is_container(id) { graph.contents(page, id)? } else { HashSet::new() };
    // Each container looked at is a step in eight, and what a container is is looked at once for the whole read.
    page.work.charge(page.containers.len() / 8 + 1)?;
    let mut best: Option<(f64, u32)> = None;
    // The innermost container on a locked layer the shape is inside, which it would have joined: passed over, and noted when it is no larger than the one it joins.
    let mut frozen: Option<(f64, u32)> = None;
    for c in &page.containers {
        if *c == id || held.contains(c) {
            continue;
        }
        let cand = page.candidate(*c)?;
        if cand.locked || !cand.takes || !cand.usable {
            continue;
        }
        let Some(b) = cand.rect else { continue };
        let strict = arriving.is_some_and(|now| page.is_container(id) && !now.contains(c));
        if !(if strict { b.holds(&me) } else { b.contains(centre) }) {
            continue;
        }
        let area = b.area();
        let slot = if cand.on_locked_layer { &mut frozen } else { &mut best };
        if slot.is_none_or(|(a, _)| area <= a) {
            *slot = Some((area, *c));
        }
    }
    if let Some((area, c)) = frozen
        && best.is_none_or(|(a, _)| area <= a)
    {
        page.pass_over(c);
    }
    // A container that is in the shape by the other side of its ties (a file can hold one side of a tie and not the other) is not one to go into either.
    match best {
        Some((_, c)) if graph.would_loop(page, c, id)? => Ok(None),
        best => Ok(best.map(|(_, c)| c)),
    }
}

/// Whether a shape takes part in membership: not a guide.
fn is_candidate(page: &Page<'_>, id: u32) -> bool {
    page.doc.page(page.id).and_then(|p| p.shapes.get(page.at(id)?)).is_some_and(|s| s.kind != ShapeType::Guide)
}

/// What a move, a drop or a paste of `ids` does to membership: each shape that is still inside its container stays; one whose middle left its container leaves, and one inside
/// another joins it. A shape that moves with its container (the container is among the shapes moved) is left alone, and so is one in a locked container, one in
/// a container on a locked layer, or one whose ties, or those of its containers, are no list. → the containers whose members changed, so
/// that the caller fits them.
pub(super) fn arrive(page: &Page<'_>, graph: &mut Graph, ids: &[u32]) -> Result<Vec<u32>, Problem> {
    let moved: HashSet<u32> = ids.iter().copied().collect();
    let mut affected = Unique::new();
    'shapes: for id in ids {
        if !page.has(*id) || !is_candidate(page, *id) || !page.usable(*id)? {
            continue;
        }
        // A member of a list DiaCraft lays out is in its place by the list: it is not made a member of the frame that holds the list (`lists::after_move`).
        if super::lists::is_laid(page, *id)? {
            continue;
        }
        let now = graph.containers_of(page, *id)?;
        // It moves rigidly with its container, which was moved as well: nothing changes between them.
        if now.iter().any(|c| moved.contains(c)) {
            continue;
        }
        let mut locked = false;
        let mut frozen = false;
        for c in &now {
            let cand = page.candidate(*c)?;
            if !cand.usable {
                continue 'shapes;
            }
            locked |= cand.locked;
            frozen |= cand.on_locked_layer;
        }
        if frozen {
            // A container on a locked layer takes and loses no member: the shape stays in the containers it is in. One it is still inside is fitted as
            // its setting says (the fit passes over the one on the locked layer, and notes it); one it left is passed over here.
            let me = page.bounds(*id)?;
            for c in &now {
                let rect = page.candidate(*c)?.rect;
                let inside = match (me, rect) {
                    (Some(me), Some(b)) => b.contains(me.centre()),
                    _ => true,
                };
                if inside {
                    affected.push(*c);
                } else if page.on_locked_layer(*c)? {
                    page.pass_over(*c);
                }
            }
            continue;
        }
        if locked {
            // Membership is frozen, but the container still has to hold the shape where it went.
            now.iter().for_each(|c| {
                affected.push(*c);
            });
            continue;
        }
        let want = underlying(page, graph, *id, Some(&now))?;
        match (want, now.as_slice()) {
            (Some(w), [only]) if *only == w => {
                // Where an edge-attached shape was moved by hand, it is a plain member again.
                if !graph.ties(page, *id)?.contains(IN_CONTAINER, w) {
                    graph.join(page, w, *id, None)?;
                }
                affected.push(w);
            }
            (None, []) => {}
            (want, now) => {
                for c in now {
                    graph.leave(page, *c, *id)?;
                    affected.push(*c);
                }
                if let Some(w) = want {
                    graph.join(page, w, *id, None)?;
                    affected.push(w);
                }
            }
        }
    }
    Ok(affected.into_vec())
}
