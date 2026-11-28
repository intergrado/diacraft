//! What the model has no typed place for, kept as the file had it: unknown attributes ([`RawAttrs`]) and unknown or unused
//! elements, comments and processing instructions ([`Passthrough`]).
//!
//! The rule is: an attribute or element the model does not understand survives a save,
//! at its place among its siblings and in its order. The model keeps three things apart:
//!
//! - what it has typed fields for (an `ID`, a `Master`, an `F`), which the reader reads into those fields;
//! - what MS-VSDX calls unused (`OriginalID`, `IsCustomName`, `Icon`, `ViewScale`): not typed, kept here, in file order;
//! - what no one knows (a foreign-namespace attribute, a `Tab` element under `Tabs`, `<?NewValue V?>`): kept here as well.
//!
//! An attribute whose typed field could not hold it (an `ID` of 4294967296) is kept here as the
//! text it was, and the typed field holds the nearest value its type allows: the entry here is the file's truth, and a writer
//! gives it back as it is.
//!
//! These are plain data. The reader of a file applies the caps of (element depth 200, and the size of
//! a part) before it builds them, so a [`RawElement`] is never deeper than a part's XML is allowed to be.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::Arc;

/// A list of attribute `(name, value)` pairs in file order, names with their prefix as written. Cheap to clone and empty
/// (a null pointer) for the common case of none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawAttrs(Option<Arc<AttrList>>);

/// Attribute `(name, value)` pairs.
pub type AttrList = Vec<(Arc<str>, Arc<str>)>;

impl RawAttrs {
    /// No attributes.
    pub fn new() -> RawAttrs {
        RawAttrs(None)
    }

    /// The attributes, in this order.
    pub fn from_pairs<N: AsRef<str>, V: AsRef<str>>(pairs: impl IntoIterator<Item = (N, V)>) -> RawAttrs {
        let list: AttrList = pairs.into_iter().map(|(n, v)| (Arc::from(n.as_ref()), Arc::from(v.as_ref()))).collect();
        if list.is_empty() { RawAttrs(None) } else { RawAttrs(Some(Arc::new(list))) }
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.0.as_ref().is_none_or(|list| list.is_empty())
    }

    /// How many.
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |list| list.len())
    }

    /// The pairs, in file order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().flat_map(|list| list.iter()).map(|(n, v)| (&**n, &**v))
    }

    /// The first attribute called `name`, matched exactly (attribute names are case-sensitive in XML).
    pub fn get(&self, name: &str) -> Option<&str> {
        self.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    /// The list behind the pointer it is shared by: a copy of these attributes holds the same one, and a count of the memory a model holds counts it once (`Arc::as_ptr`). `None` when
    /// there are none.
    pub fn shared_list(&self) -> Option<&Arc<AttrList>> {
        self.0.as_ref()
    }

    /// Adds an attribute after the others. Copies the list first if another copy of this one shares it.
    pub fn push(&mut self, name: &str, value: &str) {
        let list = self.0.get_or_insert_with(|| Arc::new(Vec::new()));
        Arc::make_mut(list).push((Arc::from(name), Arc::from(value)));
    }
}

/// An element kept whole: its name as written (prefix included), its attributes (the `xmlns` declarations among them) and
/// everything inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawElement {
    pub name: Arc<str>,
    pub attrs: RawAttrs,
    pub children: Vec<RawNode>,
}

/// A node kept as the file had it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawNode {
    Element(RawElement),
    /// Text, exactly: blanks, line breaks and all (`xml:space="preserve"`).
    Text(Arc<str>),
    Comment(Arc<str>),
    /// A processing instruction, such as `<?NewValue V?>`.
    Pi {
        target: Arc<str>,
        data: Arc<str>,
    },
}

/// A node with where it sat: `after` is how many of the typed children of its parent (cells, sections, rows, shapes: as the
/// parent counts them in file order) came before it. A writer that puts the typed children back in order puts this node
/// back after that many of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    pub after: u32,
    pub node: RawNode,
}

/// The unknown or unused child nodes of one element. Cheap to clone, and empty (a null pointer) for the common case.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Passthrough(Option<Arc<Vec<Placed>>>);

impl Passthrough {
    /// Nothing.
    pub fn new() -> Passthrough {
        Passthrough(None)
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.0.as_ref().is_none_or(|list| list.is_empty())
    }

    /// How many.
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |list| list.len())
    }

    /// The nodes, in file order.
    pub fn iter(&self) -> impl Iterator<Item = &Placed> {
        self.0.iter().flat_map(|list| list.iter())
    }

    /// The list behind the pointer it is shared by (see [`RawAttrs::shared_list`]). `None` when there are none.
    pub fn shared_list(&self) -> Option<&Arc<Vec<Placed>>> {
        self.0.as_ref()
    }

    /// Adds a node after the others. Copies the list first if another copy of this one shares it.
    pub fn push(&mut self, after: u32, node: RawNode) {
        let list = self.0.get_or_insert_with(|| Arc::new(Vec::new()));
        Arc::make_mut(list).push(Placed { after, node });
    }

    /// The nodes to change in place: the list is copied first if another copy of this one shares it (the `DocumentSettings` element, whose
    /// words an edit changes).
    pub fn nodes_mut(&mut self) -> &mut Vec<Placed> {
        Arc::make_mut(self.0.get_or_insert_with(|| Arc::new(Vec::new())))
    }

    /// A passthrough of these nodes, in this order; none at all gives the empty one (a null pointer).
    pub fn from_nodes(nodes: Vec<Placed>) -> Passthrough {
        if nodes.is_empty() { Passthrough(None) } else { Passthrough(Some(Arc::new(nodes))) }
    }

    /// Brings the places of the nodes in step with an edit of the typed children they count (`Placed::after`), so that **a node goes on standing after the child it stood
    /// after**. The edit is of the run of children that begins after `offset` others (a section's rows come after its cells, a sheet's sections after its cells) and was
    /// `old_len` long and is now `new_len`: `boundary(b)` says where the boundary before the old child `b` went (`b` from 0, the start of the run, to `old_len`, its end), as a
    /// number of children of the new run before it.
    ///
    /// * A node that stood among the `offset` children before the run is not moved.
    /// * A node that stood at a boundary of the run goes to where `boundary` says. A node that follows a child follows it still, ahead of the children that were put in right after
    ///   it (a writer that patches a tree puts a new child ahead of the nodes that trailed the child before it, and so does this).
    /// * A node that stood after the run is moved by what the run gained or lost.
    ///
    /// The list is copied only when a node moves (a copy of this passthrough that shares it keeps what it had), and never when there are none. The nodes keep their order, and
    /// a `boundary` that never gives a smaller place to a larger boundary leaves their places in order.
    pub fn reposition(&mut self, offset: usize, old_len: usize, new_len: usize, boundary: impl Fn(usize) -> usize) {
        let Some(shared) = self.0.as_mut() else { return };
        crate::work::place_reads(shared.len());
        let moved = |after: u32| after_edit(after, offset, old_len, new_len, &boundary);
        if shared.iter().all(|node| moved(node.after) == node.after) {
            return;
        }
        for node in Arc::make_mut(shared).iter_mut() {
            node.after = moved(node.after);
        }
    }

    /// New children were put into the run (see [`Passthrough::reposition`]): `at` are their places in the new run, in rising order (the place each has in the run once all
    /// of them are in), and `old_len` is the length of the run before. A node that stood at the start of the run stays ahead of the first child when `lead_stays` (the rows of
    /// a section: a new row goes after what stands before the first row; the cells of an element) and follows the children put in first when not (the sections of a sheet: a
    /// new first section goes right after the cells, ahead of the nodes that stood there).
    pub fn children_inserted(&mut self, offset: usize, old_len: usize, at: &[usize], lead_stays: bool) {
        if self.is_empty() || at.is_empty() {
            return;
        }
        // A new child in place `f` has `f - j` old children before it when `j` new ones are before it, so it is before the old child `b` when that is at most `b`.
        let bases: Vec<usize> = at.iter().enumerate().map(|(j, place)| place.saturating_sub(j)).collect();
        self.reposition(offset, old_len, old_len.saturating_add(at.len()), |b| {
            if b == 0 && lead_stays { 0 } else { b.saturating_add(bases.partition_point(|base| *base <= b)) }
        });
    }

    /// Children were taken out of the run (see [`Passthrough::reposition`]): `gone` are their places in the old run, in rising order, and `old_len` is its length before. A
    /// node that stood after a child that went stands after the child before it.
    pub fn children_removed(&mut self, offset: usize, old_len: usize, gone: &[usize]) {
        if self.is_empty() || gone.is_empty() {
            return;
        }
        self.reposition(offset, old_len, old_len.saturating_sub(gone.len()), |b| b.saturating_sub(gone.partition_point(|place| *place < b)));
    }

    /// A run of children was rebuilt (see [`Passthrough::reposition`]): the children of the run that begins after `offset` others were `old` and are `new`, each told by a key (a shape's
    /// `ID`), and **a child whose key is in both is the same child, wherever it went**. The edit may take out, put in and move any number of them (a delete, an add, a change of the
    /// drawing order, a group, an ungroup), and the rule is the one of [`Passthrough::children_inserted`] and [`Passthrough::children_removed`]:
    ///
    /// * a node goes on following the child it followed, wherever that child goes, and the nodes that followed it stay in their order;
    /// * a node whose child is gone follows the nearest child before it that is not;
    /// * a child that is new goes ahead of the nodes that stood after the child before it (the nodes follow the new children that are put in right after the child they followed);
    /// * a node that stood before the first child stays before it.
    ///
    /// The keys are read once each, in a map of the new run, and the nodes once; a list that holds no node is not read at all. A key that comes twice is the first child that has it, which
    /// is as good as another for a list nobody should hold, and no place is ever past the end of the new run. The nodes are put in order of their places again when a child moved.
    pub fn children_followed<K: Eq + Hash + Copy>(&mut self, offset: usize, old: &[K], new: &[K]) {
        if self.is_empty() || old == new {
            return;
        }
        crate::work::follow_key_reads(old.len().saturating_add(new.len()));
        let boundaries = followed_boundaries(old, new);
        let new_len = new.len();
        self.reposition(offset, old.len(), new_len, |b| boundaries.get(b).copied().unwrap_or(new_len));
        let in_order = self.iter().map(|node| node.after).is_sorted();
        if !in_order {
            self.nodes_mut().sort_by_key(|node| node.after);
        }
    }
}

impl Passthrough {
    /// [`Passthrough::children_followed`] for children told by a key that cannot be copied (a row's key, a section's): a child of the old run and one of the new run with an equal key are the
    /// same child, and **a child with no key (`None`) is the same as no other**, so a row with neither `IX` nor `N` is gone when the rows are made again and the one in its place is new.
    /// Nothing is read for a list that holds no node, and the keys are compared first, so a run whose children did not change costs one pass over the two lists and no map.
    pub fn children_followed_by<K: Eq + Hash>(&mut self, offset: usize, old: &[Option<K>], new: &[Option<K>]) {
        if self.is_empty() || old == new {
            return;
        }
        let mut ids: HashMap<&K, usize> = HashMap::with_capacity(old.len());
        let mut next = 0usize;
        let old_ids = interned(old, &mut ids, &mut next);
        let new_ids = interned(new, &mut ids, &mut next);
        self.children_followed(offset, &old_ids, &new_ids);
    }
}

/// A number for each key, the same for equal keys and another for each child that has none: the keys are read once, in `ids`, the numbers given are counted in `next`.
fn interned<'a, K: Eq + Hash>(keys: &'a [Option<K>], ids: &mut HashMap<&'a K, usize>, next: &mut usize) -> Vec<usize> {
    keys.iter()
        .map(|key| {
            let fresh = *next;
            match key {
                Some(key) => *ids.entry(key).or_insert_with(|| {
                    *next = next.saturating_add(1);
                    fresh
                }),
                None => {
                    *next = next.saturating_add(1);
                    fresh
                }
            }
        })
        .collect()
}

/// Where each boundary of the old run (0 to its length: the start, and after each child) went in the new one, as a number of children of the new run before it: the boundary after a
/// child that is still there is after it and the children that are new right behind it, and one after a child that is gone is where the boundary after the child before it went.
fn followed_boundaries<K: Eq + Hash + Copy>(old: &[K], new: &[K]) -> Vec<usize> {
    let held: HashSet<K> = old.iter().copied().collect();
    let mut at: HashMap<K, usize> = HashMap::with_capacity(new.len());
    for (place, key) in new.iter().enumerate() {
        at.entry(*key).or_insert(place);
    }
    // `fresh[p]`: how many children that were not in the old run follow the place `p` of the new run, one after another.
    let mut fresh = vec![0usize; new.len()];
    for place in (0..new.len().saturating_sub(1)).rev() {
        let next = place.saturating_add(1);
        if new.get(next).is_some_and(|key| !held.contains(key)) {
            let run = fresh.get(next).copied().unwrap_or(0).saturating_add(1);
            if let Some(slot) = fresh.get_mut(place) {
                *slot = run;
            }
        }
    }
    let mut boundaries = Vec::with_capacity(old.len().saturating_add(1));
    boundaries.push(0);
    let mut last = 0usize;
    for key in old {
        if let Some(place) = at.get(key).copied() {
            last = place.saturating_add(1).saturating_add(fresh.get(place).copied().unwrap_or(0)).min(new.len());
        }
        boundaries.push(last);
    }
    boundaries
}

/// Where a node that stood after `after` typed children stands once the run of children that begins after the first `offset` (`old_len` of them) is `new_len` long, and
/// `boundary` says where each boundary of the run went ([`Passthrough::reposition`]). A place is a `u32`: a larger one is the largest.
pub(crate) fn after_edit(after: u32, offset: usize, old_len: usize, new_len: usize, boundary: impl Fn(usize) -> usize) -> u32 {
    let at = usize::try_from(after).unwrap_or(usize::MAX);
    let Some(inside) = at.checked_sub(offset) else { return after };
    let moved =
        if inside <= old_len { offset.saturating_add(boundary(inside).min(new_len)) } else { at.saturating_add(new_len).saturating_sub(old_len) };
    u32::try_from(moved).unwrap_or(u32::MAX)
}
