//! Patching a part's tree: the edit is made at the edited nodes and nothing else is touched.
//!
//! A part that an edit touched is parsed to the tree of the container (which keeps attribute order, prefixes and `xmlns` declarations, text,
//! comments and instructions), and this walks the tree with **two models: the one the part was read into, and the one the edit left**. A
//! node is changed only where the two differ:
//!
//! * an **attribute** is set or removed only when the two models give a different one (the lists of `build.rs`, which are the reader's
//!   inverse), so `V="1.50"` of a cell nobody wrote stays `1.50`, and an attribute the model has no field for stays as it is;
//! * a **cell, section or row** is matched to its original by the very allocation (the model shares what an edit did not copy) and else by
//!   its key (a name, an `IX`), and an item the two models hold equal is not looked at again;
//! * an **item the edit added** is made from the model (`build.rs`) and put after the item it follows; an item it took away is taken out
//!   with the white space before it;
//! * **everything else in the tree stays where it is**: unknown elements and attributes, comments and instructions, text between elements,
//!   namespace declarations and the order of all of it. A node that is not one the reader modelled is a trailer of the item before it
//!   ([`patch_list`]), and goes with it.
//!
//! **What this does not do is said, not done wrong.** An edit to something the model keeps and this does not yet write (the unknown nodes
//! of a sheet) is [`PatchError::Unsupported`], and nothing is written: the model and the file would otherwise disagree after a save that
//! said it worked. A part whose tree does not have the items the baseline model says (the file is not what the model was read from) is
//! [`PatchError::Mismatch`]. **After an error the tree is to be discarded**, or a copy of it was the one patched: see [`PatchError`].
//!
//! **The prefix of what the patcher makes.** A cell, row or section it adds is built by `build.rs` with a bare name (`cell_el`, `row_el`,
//! `section_el`), and a vocabulary that reads one namespace and no other will not read it back where the tree gives its elements a prefix. So
//! [`patch_sheet_children_prefixed`] and [`patch_sheet_only_prefixed`] take the prefix of the namespace and give every element they make that prefix
//! (the cell, the row and the section, and the cells and rows inside them), and only those: a kept unknown node, in or out of what is made, is not
//! touched ([`crate::build::with_prefix_sheet`]). [`patch_sheet_children`] and [`patch_sheet_only`] make bare names, which `vsdx` reads as its own.
//!
//! The functions know which elements the reader modelled from the same rules it uses (`ns.rs`: an element is in a namespace the vocabulary
//! reads; `read.rs`: a `Cell` with a name, once; and so on), so the item `k` of the model is the `k`th modelled element of the tree.
//!
//! [`patch_list`], [`Items`] and [`match_items`] are not about sheets: a format patches its own lists (pages, masters, shapes) with them, and
//! gives its own error type, which only has to be made from a [`PatchError`].

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{Cell, Cells, Row, Section, SectionKey, Sheet};

use crate::build::{
    cell_attrs, cell_children, cell_el, prefix_cell, prefix_row, prefix_section, row_attrs, row_el, section_attrs, section_el, sheet_is_empty,
    sheet_only_attrs, sheet_only_el, with_prefix_sheet,
};
use crate::ns::{Scope, Vocabulary};
use crate::passthrough::is_blank;

/// Why a part could not be patched.
///
/// **After an error the tree is to be discarded** (or a copy was the one patched, and the original is kept). Nothing is lost from it, since
/// [`patch_list`] puts every child back whatever the error, but it may be partly edited:
/// * an error found before the first edit leaves the tree as it was: a tree that does not hold the items the model was read from ([`patch_list`]
///   then puts the children back in the order it found them), an unsupported change to the unknown nodes of a sheet, and the cells of a sheet
///   that are not the model's;
/// * an error found after an edit leaves the edits made so far: the sections of a sheet are patched after its cells, a section's rows after
///   its cells, and an item that fails in the middle of a list (an unsupported change in a row, an item the caller cannot make) comes after the
///   items before it were patched in place, which the children put back keep patched.
///
/// Undoing them would need a copy of the part's subtree before every list is patched, on the path of every save of a page of a hundred thousand
/// shapes, to serve a caller that has no use for it: a save that fails writes nothing and drops its trees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchError {
    /// An edit to something the model keeps and the patch does not write.
    Unsupported(&'static str),
    /// The tree is not the one the baseline model was read from.
    Mismatch(&'static str),
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatchError::Unsupported(what) => write!(f, "saving does not yet write this change: {what}"),
            PatchError::Mismatch(what) => write!(f, "the file on record is not the one this drawing was opened from: {what}"),
        }
    }
}

impl std::error::Error for PatchError {}

// ------------------------------------------------------------------------------------------------------------------ attributes

/// Sets the attribute `name`, in its place if the element has it, else last.
pub fn set_attr(el: &mut El, name: &str, value: &str) {
    match el.attrs.iter_mut().find(|(k, _)| k == name) {
        Some(slot) => slot.1 = value.to_owned(),
        None => el.attrs.push((name.to_owned(), value.to_owned())),
    }
}

fn remove_attr(el: &mut El, name: &str) {
    el.attrs.retain(|(k, _)| k != name);
}

/// Makes the attributes of `el` say what `now` says where `now` differs from `base`, and leaves every other attribute as it is.
pub fn sync_attrs(el: &mut El, base: &[(String, String)], now: &[(String, String)]) {
    for (k, _) in base {
        if !now.iter().any(|(n, _)| n == k) {
            remove_attr(el, k);
        }
    }
    for (k, v) in now {
        if base.iter().find(|(n, _)| n == k).map(|(_, bv)| bv) != Some(v) {
            set_attr(el, k, v);
        }
    }
}

fn unsupported_unless(same: bool, what: &'static str) -> Result<(), PatchError> {
    if same { Ok(()) } else { Err(PatchError::Unsupported(what)) }
}

// ------------------------------------------------------------------------------------------------------------------- the list

/// What `patch_list` needs to know of one kind of item.
pub struct Items<'a, V: Vocabulary, T, K> {
    /// Whether a child of the parent is an element the reader read as one of these (given the scope inside it).
    pub modelled: &'a dyn Fn(&El, &Scope<V>) -> bool,
    /// What an item is called, to find the same item in the other model when the allocation is not the same. `None`: no name.
    pub key: &'a dyn Fn(&T) -> Option<K>,
    /// Where new items go when none of the items before them is there: an index into the nodes before the first item of the list.
    pub first_at: &'a dyn Fn(&[Node]) -> usize,
    /// Whether the element is the one the baseline item was read from (the same `ID`, the same name): a part that is not the file the model
    /// was read from fails here, and a wrong file is not written.
    pub is_it: &'a dyn Fn(&El, &T) -> bool,
}

/// Matches each item of `now` to an item of `base`: the same allocation first (what an edit did not copy), then the same key, first come
/// first served. → for each item of `now` the index of its match, and for each item of `base` whether it was matched.
pub fn match_items<T, K: Hash + Eq>(base: &[&T], now: &[&T], key: &dyn Fn(&T) -> Option<K>) -> (Vec<Option<usize>>, Vec<bool>) {
    let n = base.len();
    let mut used = vec![false; n];
    let mut matched: Vec<Option<usize>> = vec![None; now.len()];
    let mut by_ptr: HashMap<*const T, usize> = HashMap::with_capacity(n);
    for (i, b) in base.iter().enumerate() {
        by_ptr.entry(std::ptr::from_ref(*b)).or_insert(i);
    }
    for (j, item) in now.iter().enumerate() {
        if let Some(&i) = by_ptr.get(&std::ptr::from_ref(*item))
            && let Some(slot) = used.get_mut(i)
            && !*slot
        {
            *slot = true;
            if let Some(m) = matched.get_mut(j) {
                *m = Some(i);
            }
        }
    }
    let mut by_key: HashMap<K, VecDeque<usize>> = HashMap::new();
    for (i, b) in base.iter().enumerate() {
        if !used.get(i).copied().unwrap_or(true)
            && let Some(k) = key(b)
        {
            by_key.entry(k).or_default().push_back(i);
        }
    }
    for (j, item) in now.iter().enumerate() {
        if matched.get(j).copied().flatten().is_some() {
            continue;
        }
        if let Some(k) = key(item)
            && let Some(queue) = by_key.get_mut(&k)
            && let Some(i) = queue.pop_front()
        {
            if let Some(slot) = used.get_mut(i) {
                *slot = true;
            }
            if let Some(m) = matched.get_mut(j) {
                *m = Some(i);
            }
        }
    }
    (matched, used)
}

/// What [`patch_list`] calls to bring the element of an item that is in both models, and differs, from the baseline item to the new one.
pub type PatchFn<'a, V, T, E> = dyn FnMut(&mut El, &Scope<V>, &T, &T) -> Result<(), E> + 'a;

/// Brings the items among the children of `parent` from the `base` model to the `now` one: items in both and equal are left alone, items in
/// both and different are patched by `patch`, items only in `now` are made by `make`, items only in `base` are taken out. `sc` is the scope
/// inside `parent`.
///
/// The children of `parent` that are not items (`Items::modelled` says no) are **trailers**: each belongs to the item before it, and moves
/// with it; the ones before the first item belong to the start. An item taken out leaves its trailer with the item before it. A new item
/// goes right after the item before it in `now`, ahead of that item's trailer (so a new `Section` comes before the `Text` that followed the
/// last one), and a new item with none before it goes at `Items::first_at`.
///
/// **On an error, every child of `parent` is still there.** When the tree does not hold the items of `base` (nothing has been touched), the
/// children are back in the order they were found. When `patch` or `make` fails, or an item is matched twice, the items before it have been
/// patched and stay patched, the one that failed is as `patch` left it, the items not reached are as they were, and the children are in the
/// order the list had reached (new items that were made are not put in): the tree is then to be discarded ([`PatchError`]).
pub fn patch_list<V: Vocabulary, T: PartialEq, K: Hash + Eq, E: From<PatchError>>(
    parent: &mut El,
    sc: &Scope<V>,
    kind: &Items<'_, V, T, K>,
    base: &[&T],
    now: &[&T],
    patch: &mut PatchFn<'_, V, T, E>,
    make: &mut dyn FnMut(&T) -> Result<El, E>,
) -> Result<(), E> {
    let children = std::mem::take(&mut parent.children);
    let mut items: Vec<Option<(El, Scope<V>)>> = Vec::new();
    // trailers[k]: the nodes before item k (k = 0), or after item k - 1.
    let mut trailers: Vec<Vec<Node>> = vec![Vec::new()];
    for node in children {
        match node {
            Node::El(child) => {
                let csc = sc.enter(&child);
                if (kind.modelled)(&child, &csc) {
                    items.push(Some((child, csc)));
                    trailers.push(Vec::new());
                } else if let Some(last) = trailers.last_mut() {
                    last.push(Node::El(child));
                }
            }
            other => {
                if let Some(last) = trailers.last_mut() {
                    last.push(other);
                }
            }
        }
    }
    if items.len() != base.len() || !items.iter().zip(base).all(|(item, b)| item.as_ref().is_some_and(|(e, _)| (kind.is_it)(e, b))) {
        // Nothing has been moved or changed: the children go back as they were found.
        let mut found = Vec::new();
        put_back(&mut found, &mut items, &mut trailers);
        parent.children = found;
        return Err(PatchError::Mismatch("a part does not hold the items the model was read from").into());
    }

    let n = base.len();
    let (matched, used) = match_items(base, now, kind.key);

    // The chain of an item: the trailer after it, and the trailers after the items taken out right behind it. The start's chain is the same
    // for the nodes before the first item.
    let take_chain = |start: usize, trailers: &mut Vec<Vec<Node>>| -> Vec<Node> {
        let mut chain = trailers.get_mut(start).map(std::mem::take).unwrap_or_default();
        let mut k = start;
        while k < n && !used.get(k).copied().unwrap_or(true) {
            k += 1;
            if let Some(t) = trailers.get_mut(k) {
                chain.append(t);
            }
        }
        chain
    };
    let mut lead = take_chain(0, &mut trailers);
    let mut lead_new: Vec<Node> = Vec::new();
    let mut out: Vec<Node> = Vec::with_capacity(now.len() + lead.len());
    let mut started = false;
    let mut pending: Vec<Node> = Vec::new();
    let mut failure: Option<E> = None;
    for (j, item) in now.iter().enumerate() {
        match matched.get(j).copied().flatten() {
            Some(i) => {
                if !started {
                    started = true;
                    let at = (kind.first_at)(&lead).min(lead.len());
                    lead.splice(at..at, std::mem::take(&mut lead_new));
                    out.append(&mut lead);
                }
                out.append(&mut pending);
                let (Some(b), Some(slot)) = (base.get(i), items.get_mut(i)) else {
                    failure = Some(PatchError::Mismatch("an item of the model has no element").into());
                    break;
                };
                let Some((mut e, esc)) = slot.take() else {
                    failure = Some(PatchError::Mismatch("an item of the model was matched twice").into());
                    break;
                };
                if !(std::ptr::eq(*b, *item) || **b == **item)
                    && let Err(error) = patch(&mut e, &esc, b, item)
                {
                    out.push(Node::El(e));
                    failure = Some(error);
                    break;
                }
                out.push(Node::El(e));
                pending = take_chain(i + 1, &mut trailers);
            }
            None => match make(item) {
                Ok(made) => {
                    let made = Node::El(made);
                    if started { out.push(made) } else { lead_new.push(made) }
                }
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            },
        }
    }
    if let Some(error) = failure {
        // Every child goes back: what was moved to `out`, the start if it was not moved, the chain waiting to follow an item, and then the
        // items and trailers not reached, in the order they were found. The new items made (`lead_new`) were never in the tree.
        if !started {
            out.append(&mut lead);
        }
        out.append(&mut pending);
        put_back(&mut out, &mut items, &mut trailers);
        parent.children = out;
        return Err(error);
    }
    if !started {
        let at = (kind.first_at)(&lead).min(lead.len());
        lead.splice(at..at, lead_new);
        out.append(&mut lead);
    }
    out.append(&mut pending);
    // Whatever is left belongs to no item that is there: it is kept, last, and not dropped.
    for t in &mut trailers {
        out.append(t);
    }
    parent.children = out;
    Ok(())
}

/// Puts the trailers still in `trailers` and the items still in `items` after what is in `out`, in the order they were found: the trailer before
/// item `k` is `trailers[k]`.
fn put_back<V: Vocabulary>(out: &mut Vec<Node>, items: &mut [Option<(El, Scope<V>)>], trailers: &mut [Vec<Node>]) {
    for (k, trailer) in trailers.iter_mut().enumerate() {
        out.append(trailer);
        if let Some(slot) = items.get_mut(k)
            && let Some((e, _)) = slot.take()
        {
            out.push(Node::El(e));
        }
    }
}

/// Index after the last of `names` among the nodes, or 0: where new items go after the elements of these kinds.
#[must_use]
pub fn after_last(nodes: &[Node], names: &[&str]) -> usize {
    nodes.iter().rposition(|n| matches!(n, Node::El(e) if names.iter().any(|name| e.is(name)))).map_or(0, |i| i + 1)
}

/// The end of the nodes: where new items go after everything.
#[must_use]
pub fn at_end(nodes: &[Node]) -> usize {
    nodes.len()
}

/// Whether `el`, which `sc` was entered for, is an element called `local` in a namespace the vocabulary reads.
#[must_use]
pub fn is_ours<V: Vocabulary>(el: &El, sc: &Scope<V>, local: &str) -> bool {
    sc.is_ours(el) && el.is(local)
}

/// Whether `el` has a name the reader would have taken as one: an `N` that is not empty.
#[must_use]
pub fn has_name(el: &El) -> bool {
    el.attr("N").is_some_and(|n| !n.is_empty())
}

// ----------------------------------------------------------------------------------------------------------------- sheets

/// Patches the `Cell` children of `el` (the sheet's, a section's, a row's): where the two models give a different cell, its attributes and
/// content are patched, a cell that is gone is taken out with the white space before it, and a new cell goes after the last cell there.
fn patch_cells<V: Vocabulary>(el: &mut El, sc: &Scope<V>, base: &Cells, now: &Cells, prefix: &str) -> Result<(), PatchError> {
    if base == now {
        return Ok(());
    }
    // The cell elements the reader modelled: a `Cell` of this vocabulary with a name, the first of that name.
    let mut first: HashMap<String, usize> = HashMap::new();
    for (i, node) in el.children.iter().enumerate() {
        if let Node::El(c) = node {
            let csc = sc.enter(c);
            if is_ours(c, &csc, "Cell")
                && let Some(name) = c.attr("N").filter(|n| !n.is_empty())
            {
                first.entry(name.to_owned()).or_insert(i);
            }
        }
    }
    if first.len() != base.len() || !base.keys().all(|k| first.contains_key(k.as_str())) {
        return Err(PatchError::Mismatch("a sheet does not hold the cells the model was read from"));
    }
    let mut remove: HashSet<usize> = HashSet::new();
    for (name, b) in base {
        let Some(&at) = first.get(name.as_str()) else { continue };
        match now.get(name) {
            Some(n) if n == b => {}
            Some(n) => {
                if let Some(Node::El(c)) = el.children.get_mut(at) {
                    patch_cell(c, b, n);
                }
            }
            None => {
                remove.insert(at);
            }
        }
    }
    let added: Vec<Node> = now
        .iter()
        .filter(|(name, _)| !base.contains_key(*name))
        .map(|(name, c)| {
            let mut made = cell_el(name.as_str(), c);
            prefix_cell(&mut made, prefix);
            Node::El(made)
        })
        .collect();
    if remove.is_empty() && added.is_empty() {
        return Ok(());
    }
    // New cells go after the last cell that stays; with none, before the first section; with none, first.
    let last_kept = first.values().filter(|i| !remove.contains(i)).max().copied();
    let first_section = el.children.iter().position(|n| matches!(n, Node::El(e) if e.is("Section")));
    let insert_at = last_kept.map(|i| i + 1).or(first_section).unwrap_or(0);
    let old = std::mem::take(&mut el.children);
    let mut out: Vec<Node> = Vec::with_capacity(old.len() + added.len());
    let mut added = Some(added);
    for (i, node) in old.into_iter().enumerate() {
        if i == insert_at
            && let Some(a) = added.take()
        {
            out.extend(a);
        }
        if remove.contains(&i) {
            if matches!(out.last(), Some(Node::Text(t)) if is_blank(t)) {
                out.pop();
            }
            continue;
        }
        out.push(node);
    }
    if let Some(a) = added {
        out.extend(a);
    }
    el.children = out;
    Ok(())
}

/// A cell that is in both models and differs.
fn patch_cell(el: &mut El, base: &Cell, now: &Cell) {
    sync_attrs(el, &cell_attrs(base), &cell_attrs(now));
    if base.extra != now.extra {
        el.children = cell_children(now);
    }
}

/// The cells and sections of a sheet, among the children of `el` (a `Shape`, a `StyleSheet`, a `PageSheet`, a `DocumentSheet`).
pub fn patch_sheet_children<V: Vocabulary, E: From<PatchError>>(el: &mut El, sc: &Scope<V>, base: &Sheet, now: &Sheet) -> Result<(), E> {
    Ok(patch_sheet(el, sc, base, now, "")?)
}

/// [`patch_sheet_children`] for a tree whose elements carry a prefix: every cell, row and section it makes is given `prefix` (an empty prefix gives
/// none). What was already in the tree, and every kept unknown node, is left as it is.
pub fn patch_sheet_children_prefixed<V: Vocabulary, E: From<PatchError>>(
    el: &mut El,
    sc: &Scope<V>,
    base: &Sheet,
    now: &Sheet,
    prefix: &str,
) -> Result<(), E> {
    Ok(patch_sheet(el, sc, base, now, prefix)?)
}

fn patch_sheet<V: Vocabulary>(el: &mut El, sc: &Scope<V>, base: &Sheet, now: &Sheet, prefix: &str) -> Result<(), PatchError> {
    unsupported_unless(base.passthrough == now.passthrough, "a change to the unknown elements of a sheet")?;
    patch_cells(el, sc, &base.cells, &now.cells, prefix)?;
    if base.sections.is_empty() && now.sections.is_empty() {
        return Ok(());
    }
    let b: Vec<&Section> = base.sections.iter().map(|s| &**s).collect();
    let n: Vec<&Section> = now.sections.iter().map(|s| &**s).collect();
    let kind = Items {
        modelled: &|c: &El, csc: &Scope<V>| is_ours(c, csc, "Section") && has_name(c),
        key: &|s: &Section| Some(SectionKey { name: s.name.clone(), ix: s.ix }),
        first_at: &|lead: &[Node]| after_last(lead, &["Cell"]),
        is_it: &|e: &El, s: &Section| e.attr("N") == Some(s.name.as_str()),
    };
    patch_list(el, sc, &kind, &b, &n, &mut |e, esc, b, n| patch_section(e, esc, b, n, prefix), &mut |s| {
        let mut made = section_el(s);
        prefix_section(&mut made, prefix);
        Ok(made)
    })
}

fn patch_section<V: Vocabulary>(el: &mut El, sc: &Scope<V>, base: &Section, now: &Section, prefix: &str) -> Result<(), PatchError> {
    sync_attrs(el, &section_attrs(base), &section_attrs(now));
    unsupported_unless(base.passthrough == now.passthrough, "a change to the unknown elements of a section")?;
    patch_cells(el, sc, &base.cells, &now.cells, prefix)?;
    if base.rows.is_empty() && now.rows.is_empty() {
        return Ok(());
    }
    let b: Vec<&Row> = base.rows.iter().map(|r| &**r).collect();
    let n: Vec<&Row> = now.rows.iter().map(|r| &**r).collect();
    let name = now.name.as_str();
    let kind = Items {
        modelled: &|c: &El, csc: &Scope<V>| is_ours(c, csc, "Row"),
        key: &|r: &Row| r.key(name),
        first_at: &at_end,
        is_it: &|_: &El, _: &Row| true,
    };
    patch_list(el, sc, &kind, &b, &n, &mut |e, esc, b, n| patch_row(e, esc, b, n, prefix), &mut |r| {
        let mut made = row_el(r);
        prefix_row(&mut made, prefix);
        Ok(made)
    })
}

fn patch_row<V: Vocabulary>(el: &mut El, sc: &Scope<V>, base: &Row, now: &Row, prefix: &str) -> Result<(), PatchError> {
    sync_attrs(el, &row_attrs(base), &row_attrs(now));
    unsupported_unless(base.passthrough == now.passthrough, "a change to the unknown elements of a row")?;
    patch_cells(el, sc, &base.cells, &now.cells, prefix)
}

/// An element that is only a sheet (`PageSheet`, `DocumentSheet`), found among the children of `parent` by its name, made if the new sheet has
/// anything and the old one had no element, and left alone if neither holds anything. `before` names the child a new one is put before.
pub fn patch_sheet_only<V: Vocabulary, E: From<PatchError>>(
    parent: &mut El,
    sc: &Scope<V>,
    name: &str,
    before: &[&str],
    base: &Sheet,
    now: &Sheet,
) -> Result<(), E> {
    Ok(patch_sheet_element(parent, sc, name, before, base, now, "")?)
}

/// [`patch_sheet_only`] for a tree whose elements carry a prefix: the sheet element it makes, and every cell, row and section it makes, are given
/// `prefix` (an empty prefix gives none). What was already in the tree, and every kept unknown node, is left as it is.
pub fn patch_sheet_only_prefixed<V: Vocabulary, E: From<PatchError>>(
    parent: &mut El,
    sc: &Scope<V>,
    name: &str,
    before: &[&str],
    base: &Sheet,
    now: &Sheet,
    prefix: &str,
) -> Result<(), E> {
    Ok(patch_sheet_element(parent, sc, name, before, base, now, prefix)?)
}

fn patch_sheet_element<V: Vocabulary>(
    parent: &mut El,
    sc: &Scope<V>,
    name: &str,
    before: &[&str],
    base: &Sheet,
    now: &Sheet,
    prefix: &str,
) -> Result<(), PatchError> {
    if base == now {
        return Ok(());
    }
    let found = parent.children.iter().position(|n| matches!(n, Node::El(e) if is_ours(e, &sc.enter(e), name)));
    match found {
        Some(at) => {
            if let Some(Node::El(e)) = parent.children.get_mut(at) {
                let esc = sc.enter(e);
                sync_attrs(e, &sheet_only_attrs(base), &sheet_only_attrs(now));
                patch_sheet(e, &esc, base, now, prefix)?;
            }
            Ok(())
        }
        None if sheet_is_empty(now) => Ok(()),
        None => {
            // The reader found no such element, so the old sheet was empty: the new one is made whole.
            unsupported_unless(sheet_is_empty(base), "a part that lacks the sheet the model was read from")?;
            let at = parent.children.iter().position(|n| matches!(n, Node::El(e) if before.iter().any(|b| e.is(b)))).unwrap_or(parent.children.len());
            let mut made = sheet_only_el(name, now);
            with_prefix_sheet(&mut made, prefix);
            parent.children.insert(at, Node::El(made));
            Ok(())
        }
    }
}
