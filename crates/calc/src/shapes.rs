//! Finding a shape by `ID` or by name without walking the part for each lookup.
//!
//! A formula's `Sheet.<id>!` and `<name>!` reach a shape of the same page. `Document::find_shape` walks the whole part for each call,
//! which is a quadratic bill for a page of many shapes each naming another, so the evaluator keeps an index per part, built the first
//! time a lookup needs it, with an explicit stack (a part is nested at most [`MAX_SHAPE_DEPTH`] deep, and nothing here recurses).

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_doc::{MAX_SHAPE_DEPTH, ShapeList};

/// No parent: a top-level shape.
const NO_PARENT: u32 = u32::MAX;

/// The most shapes an index holds; past it the later shapes of a part are not found by a formula. A project rule: it bounds the index of
/// a hostile part at about 64 MB, and is far above the 1,000,000 shapes `doc` allows a page (`MAX_SHAPES_PER_PAGE` is checked at the
/// door, and a part built by hand can pass it).
pub(crate) const MAX_INDEXED_SHAPES: usize = 4_000_000;

#[derive(Clone, Copy)]
struct Entry {
    parent: u32,
    at: u32,
}

/// The first shape with each `ID` in one part (in document order: a group before its members), kept as each shape's parent and place
/// so that the memory does not grow with the depth of the tree.
pub(crate) struct ShapeIndex {
    entries: Vec<Entry>,
    by_id: HashMap<u32, u32>,
}

/// A charge for work done in lumps: `true` to go on, `false` when the budget is spent and the work ends.
pub(crate) type Charge<'a> = &'a mut dyn FnMut(usize) -> bool;

/// How many shapes are walked between two charges.
const CHARGE_EVERY: usize = 1024;

impl ShapeIndex {
    /// Builds the index of a part. Each shape walked is a step, charged every [`CHARGE_EVERY`] shapes with the memory it holds (the budget's
    /// owner decides what a unit costs); a charge that is refused ends the walk, and the index holds what it had reached.
    pub(crate) fn build(list: &ShapeList, charge: Charge<'_>) -> ShapeIndex {
        let mut index = ShapeIndex { entries: Vec::new(), by_id: HashMap::new() };
        // The shape list being walked at each level, the entry of the shape it belongs to, and the next place to visit.
        let mut stack: Vec<(u32, &ShapeList, usize)> = vec![(NO_PARENT, list, 0)];
        let mut uncharged = 0usize;
        while let Some(top) = stack.last_mut() {
            let (parent, shapes, next) = (top.0, top.1, top.2);
            let Some(shape) = shapes.get(next) else {
                stack.pop();
                continue;
            };
            top.2 = next + 1;
            if index.entries.len() >= MAX_INDEXED_SHAPES {
                break;
            }
            let me = u32::try_from(index.entries.len()).unwrap_or(NO_PARENT);
            if me == NO_PARENT {
                break;
            }
            uncharged += 1;
            if uncharged >= CHARGE_EVERY {
                if !charge(uncharged) {
                    return index;
                }
                uncharged = 0;
            }
            index.entries.push(Entry { parent, at: u32::try_from(next).unwrap_or(u32::MAX) });
            index.by_id.entry(shape.id).or_insert(me);
            if stack.len() < MAX_SHAPE_DEPTH {
                stack.push((me, &shape.children, 0));
            }
        }
        if uncharged > 0 {
            charge(uncharged);
        }
        index
    }

    /// The path of the first shape with this `ID`: the place of each shape on the way down from the top level.
    pub(crate) fn path(&self, id: u32) -> Option<Vec<u32>> {
        let mut entry = *self.by_id.get(&id)?;
        let mut path = Vec::new();
        loop {
            let found = self.entries.get(usize::try_from(entry).ok()?)?;
            path.push(found.at);
            if found.parent == NO_PARENT {
                break;
            }
            // Parents are earlier entries, so the walk up ends; the bound is for an index not built here.
            if path.len() > MAX_SHAPE_DEPTH {
                return None;
            }
            entry = found.parent;
        }
        path.reverse();
        Some(path)
    }
}

/// The shapes of one level that share a name: the first, and for a name that more than one shape holds, the first of each `ID`.
pub(crate) enum Places {
    One(u32),
    Many { first: u32, by_id: HashMap<u32, u32> },
}

impl Places {
    /// The place whose `ID` is `id`, or the first when there is no `ID`. `level` is the level the places are in.
    pub(crate) fn pick(&self, level: &ShapeList, id: Option<u32>) -> Option<u32> {
        match (self, id) {
            (Places::One(place), None) => Some(*place),
            (Places::Many { first, .. }, None) => Some(*first),
            (Places::One(place), Some(id)) => level.get(usize::try_from(*place).ok()?).filter(|s| s.id == id).map(|_| *place),
            (Places::Many { by_id, .. }, Some(id)) => by_id.get(&id).copied(),
        }
    }
}

/// The shapes of one level by name: `NameU` first, then `Name`; the places with each name.
pub(crate) type PeerNames = HashMap<Arc<str>, Places>;

/// Builds the name table of one level of shapes (the top level of a part, or the members of a group). Each shape is a step, charged as
/// [`ShapeIndex::build`] charges; a refused charge ends the walk.
pub(crate) fn peer_names(level: &ShapeList, charge: Charge<'_>) -> PeerNames {
    let mut names: PeerNames = HashMap::new();
    let mut uncharged = 0usize;
    for at in 0..level.len() {
        let Some(shape) = level.get(at) else { continue };
        let Ok(place) = u32::try_from(at) else { break };
        uncharged += 1;
        if uncharged >= CHARGE_EVERY {
            if !charge(uncharged) {
                return names;
            }
            uncharged = 0;
        }
        let Some(name) = shape.name_u.clone().or_else(|| shape.name.clone()) else { continue };
        match names.get_mut(&name) {
            None => {
                names.insert(name, Places::One(place));
            }
            Some(Places::Many { by_id, .. }) => {
                by_id.entry(shape.id).or_insert(place);
            }
            Some(slot) => {
                // The second shape of this name: the first one's `ID` joins the table, then this one's.
                let Places::One(first) = *slot else { continue };
                let first_id = usize::try_from(first).ok().and_then(|f| level.get(f)).map(|s| s.id);
                let mut by_id = HashMap::new();
                if let Some(id) = first_id {
                    by_id.insert(id, first);
                }
                by_id.entry(shape.id).or_insert(place);
                *slot = Places::Many { first, by_id };
            }
        }
    }
    if uncharged > 0 {
        charge(uncharged);
    }
    names
}
