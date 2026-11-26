//! Setting the ordered children of one element side by side by what they are, and not by the place they stand in.
//!
//! The check compares the children of an element in order, and the start and the end of two lists that are equal are matched at once ([`super::Cmp::items`]). What lies between,
//! the **middle**, is what this module sets in order: it says which child of the first list and which of the second are the same thing, so that **a child put in or taken out is one
//! difference and every sibling after it is still compared with its own**, in depth. Set side by side by place, the first child after a put-in element was compared with the
//! put-in element, the second with the first, and so on to the end of the list: the list was counted different where it only shifted, and nothing inside the shifted children was
//! looked at, so that a change planted deep in one of them was never seen.
//!
//! # How a child finds its match
//!
//! Each child has three keys, tried in turn:
//! 1. **what it holds** (the hash of its whole content): a child that is the same on both sides;
//! 2. **which thing it is** ([`Facet::identity`]: its name and the attributes the format says name a thing, [`super::Rules::identified_by`]): a page or a shape that changed but is
//!    still the page or the shape of that name;
//! 3. **what it is called** ([`Facet::name`]: the kind and the name of an element, or "text", or an instruction's target), **when the name is the only one of its kind in the list on each
//!    side**: the one `body` or `styles` of a parent, however it changed. A name that two children of a list have tells nothing of which is which, so it matches none.
//!
//! The first two match among the children **not matched by the keys before them**, and only where **both lists hold the same number of children of a key**: then the first of one
//! side with the first of the other, the second with the second, and so on. Where the numbers differ nothing says which one was put in or taken out, and none is matched by that
//! key. A pair of the first key is a child that is the same on both sides, so it may be anywhere; a child that is the same thing, or the one of its name, is matched as well wherever it
//! stands, and in another order it is a reorder and a change, both found.
//!
//! **Why it is built so, and not as an LCS.** The count of the check is the same whichever document is first (`count(a, b) == count(b, a)`; the fuzz target and the property tests hold
//! it). A longest common subsequence has several equal answers, and which one a program takes depends on which list is first; two answers pair different children, and the pairs
//! differ inside. Here a match is made only where the two sides say the same of it (the same number of children of one key), so that swapping the lists swaps each pair and nothing
//! else, and the work is a sort for each key (`n log n`), where an LCS is `n * m` and needs a cap that sends a long list back to the place-by-place reading. What a key does not decide is left to the
//! gaps (below), as it always was.
//!
//! # Order
//!
//! The matched pairs are in order when each is after the one before it in both lists. Where two **cross**, the same children are in another order: that is one difference, the
//! order, however many cross (the children's own differences are found in their pairs, as ever). A pair that crosses no other is an **anchor**, and the anchors split what is left
//! into **gaps**: between two anchors, the children that no key matched, of each side, are set side by side by place (first with first), which is what a *replaced* child is, one
//! difference, and what is left over of one side is put in or taken out.
//!
//! # What it costs
//!
//! Two sorts of at most `n` keys for each of the three keys, a merge of each pair of sorted lists and two passes over the pairs: `n log n` steps, each comparison of a sort and
//! each place looked at one step, counted in [`Work::canon_steps`]. No list longer than
//! [`super::MAX_ALIGNED`] is set in order by this: the caller sets it side by side by place and counts it ([`Work::canon_by_place`]).

use crate::work::Work;

/// What the alignment knows of one child: three numbers, each only an aligner (a collision can pair two children that are not alike, and every pair is compared in full).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Facet {
    /// What the child holds.
    pub content: u64,
    /// What it is called: its kind and name.
    pub name: u64,
    /// Which thing it is, when the format says it is told by an attribute it has.
    pub identity: Option<u64>,
}

/// How two lists of children are set in order.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Alignment {
    /// For each child of the first list, the place of its match in the second: each place of the second is the match of at most one.
    pub a_partner: Vec<Option<usize>>,
    /// For each child of the second list, whether it is matched.
    pub b_matched: Vec<bool>,
    /// Whether two pairs cross: the same children in another order.
    pub crossed: bool,
    /// The pairs `(place in the first, place in the second)` that cross no other, in order: each is after the one before it in both lists.
    pub anchors: Vec<(usize, usize)>,
}

/// Sets the lists `a` and `b` in order by their keys; `identified` is whether the format names any attribute that says which thing an element is (without one, no child has an
/// identity and that key is not tried). Counted in `work`.
pub(super) fn align(a: &[Facet], b: &[Facet], identified: bool, work: &mut Work) -> Alignment {
    let mut found = Alignment { a_partner: vec![None; a.len()], b_matched: vec![false; b.len()], ..Alignment::default() };
    match_by(a, b, |f| Some(f.content), &mut found, work);
    // What is left on each side: when one has nothing left, the other keys have nothing to match.
    let left = |found: &Alignment| (found.a_partner.iter().any(Option::is_none), found.b_matched.iter().any(|m| !m));
    if identified && left(&found) == (true, true) {
        match_by(a, b, |f| f.identity, &mut found, work);
    }
    if left(&found) == (true, true) {
        match_by_unique_name(a, b, &mut found, work);
    }
    // The pairs by the first list: the places of the first, in order, that have a match.
    let pairs: Vec<(usize, usize)> = found.a_partner.iter().enumerate().filter_map(|(i, j)| j.map(|j| (i, j))).collect();
    Work::add(&mut work.canon_steps, a.len().saturating_add(pairs.len().saturating_mul(2)));
    found.crossed = pairs.windows(2).any(|w| matches!(w, [p, q] if q.1 < p.1));
    found.anchors = anchors(&pairs);
    found
}

/// The children of `list` that are left, by `key`, each with its place, sorted: the first of one key is the first in the list. Each comparison of the sort is a step.
fn keyed(list: &[Facet], left: impl Fn(usize) -> bool, key: fn(&Facet) -> Option<u64>, work: &mut Work) -> Vec<(u64, usize)> {
    let mut out: Vec<(u64, usize)> = list.iter().enumerate().filter_map(|(at, f)| if left(at) { key(f).map(|k| (k, at)) } else { None }).collect();
    let mut compared = 0u64;
    out.sort_unstable_by(|p, q| {
        compared += 1;
        p.cmp(q)
    });
    work.canon_steps = work.canon_steps.saturating_add(compared).saturating_add(list.len() as u64);
    out
}

/// The end of the run of a key from a place of a sorted list: a run is the start of what is left.
fn run(list: &[(u64, usize)], from: usize, k: u64) -> usize {
    from.saturating_add(list.get(from..).map_or(0, |rest| rest.partition_point(|e| e.0 <= k)))
}

/// Puts `i` of the first list and `j` of the second as a pair, when neither is matched.
fn pair_up(found: &mut Alignment, i: usize, j: usize) {
    if found.a_partner.get(i).is_some_and(Option::is_none)
        && found.b_matched.get(j).is_some_and(|m| !m)
        && let (Some(a), Some(b)) = (found.a_partner.get_mut(i), found.b_matched.get_mut(j))
    {
        *a = Some(j);
        *b = true;
    }
}

/// Matches the children that are left of `a` and `b` by `key`: where each side holds the same number of children of a key, the first with the first, the second with the second,
/// and so on; the matches are put in `found`.
fn match_by(a: &[Facet], b: &[Facet], key: fn(&Facet) -> Option<u64>, found: &mut Alignment, work: &mut Work) {
    let ka = keyed(a, |at| found.a_partner.get(at).is_some_and(Option::is_none), key, work);
    let kb = keyed(b, |at| found.b_matched.get(at).is_some_and(|m| !m), key, work);
    let (mut x, mut y) = (0usize, 0usize);
    while let (Some(&(k1, _)), Some(&(k2, _))) = (ka.get(x), kb.get(y)) {
        let (ex, ey) = (run(&ka, x, k1), run(&kb, y, k2));
        match k1.cmp(&k2) {
            std::cmp::Ordering::Less => x = ex,
            std::cmp::Ordering::Greater => y = ey,
            std::cmp::Ordering::Equal => {
                let (ra, rb) = (ka.get(x..ex).unwrap_or_default(), kb.get(y..ey).unwrap_or_default());
                if ra.len() == rb.len() {
                    for (&(_, i), &(_, j)) in ra.iter().zip(rb) {
                        pair_up(found, i, j);
                    }
                }
                (x, y) = (ex, ey);
            }
        }
        // A step for each run met.
        Work::add(&mut work.canon_steps, 1);
    }
}

/// Matches the children that are left of `a` and `b` by their names, **where the name is the only one of its kind in its whole list on each side**: a name that has one child left
/// on each side and that no child already matched has. A name two children of a list have says nothing of which is which, however many of them are left.
fn match_by_unique_name(a: &[Facet], b: &[Facet], found: &mut Alignment, work: &mut Work) {
    let ka = keyed(a, |at| found.a_partner.get(at).is_some_and(Option::is_none), |f| Some(f.name), work);
    let kb = keyed(b, |at| found.b_matched.get(at).is_some_and(|m| !m), |f| Some(f.name), work);
    // The names that one child of each side has left, with the two children.
    let mut candidates: Vec<(u64, usize, usize)> = Vec::new();
    let (mut x, mut y) = (0usize, 0usize);
    while let (Some(&(k1, i)), Some(&(k2, j))) = (ka.get(x), kb.get(y)) {
        let (ex, ey) = (run(&ka, x, k1), run(&kb, y, k2));
        match k1.cmp(&k2) {
            std::cmp::Ordering::Less => x = ex,
            std::cmp::Ordering::Greater => y = ey,
            std::cmp::Ordering::Equal => {
                if ex.saturating_sub(x) == 1 && ey.saturating_sub(y) == 1 {
                    candidates.push((k1, i, j));
                }
                (x, y) = (ex, ey);
            }
        }
        Work::add(&mut work.canon_steps, 1);
    }
    if candidates.is_empty() {
        return;
    }
    // A name that a child already matched has, on either side, is not the only one of its kind: a binary search of the candidates for each child that is matched.
    let mut shared = vec![false; candidates.len()];
    let matched = a.iter().enumerate().filter(|(at, _)| found.a_partner.get(*at).is_some_and(Option::is_some));
    let matched = matched.chain(b.iter().enumerate().filter(|(at, _)| found.b_matched.get(*at).copied().unwrap_or(false)));
    let mut looked = 0usize;
    for (_, f) in matched {
        looked = looked.saturating_add(1);
        if let Ok(k) = candidates.binary_search_by(|c| c.0.cmp(&f.name))
            && let Some(slot) = shared.get_mut(k)
        {
            *slot = true;
        }
    }
    Work::add(&mut work.canon_steps, looked.saturating_mul((usize::BITS - candidates.len().leading_zeros()) as usize));
    for (&(_, i, j), &shared) in candidates.iter().zip(&shared) {
        if !shared {
            pair_up(found, i, j);
        }
    }
}

/// The pairs, which are by the first and have each place of the second once, that cross no other: a pair is one when every pair before it has a smaller place in the second and
/// every pair after it a larger one.
pub(super) fn anchors(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    // The least place in the second of the pairs after each.
    let mut after = vec![usize::MAX; pairs.len()];
    let mut low = usize::MAX;
    for (pair, slot) in pairs.iter().zip(after.iter_mut()).rev() {
        *slot = low;
        low = low.min(pair.1);
    }
    let mut out = Vec::new();
    // The greatest place in the second of the pairs before each.
    let mut high: Option<usize> = None;
    for (pair, &below) in pairs.iter().zip(&after) {
        if high.is_none_or(|h| h < pair.1) && pair.1 < below {
            out.push(*pair);
        }
        high = Some(high.map_or(pair.1, |h| h.max(pair.1)));
    }
    out
}
