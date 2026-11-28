//! One walk of a page for the commands that take many shape `ID`s: the clipboard commands and group and ungroup, and every command that finds the shapes it names
//! ([`find_named`] and [`Named`]: the format, arrange, move, delete, snap, Change Shape and callout commands among them).
//!
//! **Why.** `Document::find_shape` walks the page from the top for each `ID` it is asked for, so a command that looks up N shapes on a page of M
//! costs N times M node visits (4,096 ids on a page near the cap of a million shapes is about 4e9). The commands here walk the page once
//! ([`scan`]), note the first shape in document order with each `ID` asked for (the one `find_shape` would give), the highest `ID` and the number of
//! shapes, and work from that. The walk is charged one node budget for the call: [`MAX_SHAPES_PER_PAGE`] nodes and no more.

use std::collections::{HashMap, HashSet};

use diacraft_doc::{Document, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE, Owner, ShapeList, ShapeRef};

use super::Problem;

// How many shapes the walks of this thread have visited, for the tests that hold a command to one walk of the page. Not kept outside tests.
#[cfg(test)]
thread_local! {
    pub(crate) static NODES_WALKED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// What one walk of a page found.
#[derive(Debug, Default)]
pub(crate) struct Scan {
    /// The first shape (document order, a group before its members) with each wanted `ID`.
    pub found: HashMap<u32, ShapeRef>,
    /// The highest `ID` on the page, nested shapes counted.
    pub highest: u32,
    /// How many shapes the page holds, nested ones counted.
    pub count: usize,
}

/// Walks page `page` once, depth first in document order (the order of `Document::find_shape`), noting the shapes with the `ID`s in `wanted`, the
/// highest `ID` and the count. Iterative, no deeper than the model allows, and at most [`MAX_SHAPES_PER_PAGE`] nodes: a page with more is an
/// error and not a long walk.
pub(crate) fn scan(doc: &Document, page: u32, wanted: &HashSet<u32>) -> Result<Scan, Problem> {
    scan_where(doc, page, |id| wanted.contains(&id), None)
}

/// The first shape (document order, a group before its members) with each `ID` of `wanted`, found in **one walk of the page that stops at the last of them**: what a command that is
/// given the `ID`s of many shapes asks for them once, where a search by `Document::find_shape` for each walks the page from the top for each (4,096 names on a page
/// of 30,000 shapes were 60 million visits). An `ID` not on the page is left out of the answer, and the walk then goes to the end of the page, no further than [`MAX_SHAPES_PER_PAGE`]
/// nodes. Nothing is walked for no `ID` (the page is not even looked for). The highest `ID` and the count of a [`Scan`] are not asked for here, so the walk does not go on for them.
pub(crate) fn find_named(doc: &Document, page: u32, wanted: &HashSet<u32>) -> Result<HashMap<u32, ShapeRef>, Problem> {
    if wanted.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(scan_where(doc, page, |id| wanted.contains(&id), Some(wanted.len()))?.found)
}

/// The shapes a command names, found in **one walk of the page** ([`find_named`]) and then taken one at a time in the caller's order: a loop that asked
/// `require_shape` for each `ID` walked the page from its top for each (4,096 names on a page of 30,000 shapes were 58 million visits). [`Named::require`] refuses a shape
/// that is not on the page in `require_shape`'s words, so the loop refuses the same `ID`, the first one named that is missing, as it did; a page that is not there is refused
/// by [`Named::of`] in the same words too, and for no `ID` nothing is walked and the page is not looked for, as no search was made.
pub(crate) struct Named {
    page: u32,
    found: HashMap<u32, ShapeRef>,
}

impl Named {
    /// The shapes with `ids` on page `page`, in one walk of it.
    pub(crate) fn of(doc: &Document, page: u32, ids: impl IntoIterator<Item = u32>) -> Result<Named, Problem> {
        let wanted: HashSet<u32> = ids.into_iter().collect();
        Ok(Named { page, found: find_named(doc, page, &wanted)? })
    }

    /// Where the shape with `id` is (the first in document order, as `Document::find_shape` gives it), if it is on the page.
    pub(crate) fn get(&self, id: u32) -> Option<&ShapeRef> {
        self.found.get(&id)
    }

    /// Where the shape with `id` is, or why there is none, in the words of `require_shape`.
    pub(crate) fn require(&self, id: u32) -> Result<ShapeRef, Problem> {
        self.found.get(&id).cloned().ok_or_else(|| Problem::bad(format!("page {} has no shape {id}", self.page)))
    }
}

/// Every shape of a page, found by its `ID` in one walk of it ([`scan`] with every `ID` wanted): what a walk that goes on from shape to shape asks once, where a [`scan`] for each round would walk the page again.
pub(crate) fn scan_every(doc: &Document, page: u32) -> Result<HashMap<u32, ShapeRef>, Problem> {
    Ok(scan_where(doc, page, |_| true, None)?.found)
}

/// [`scan`] for the shapes whose `ID` `wanted` says yes to. With `stop_at`, the walk ends as soon as that many `ID`s have been found, and the `highest` and the `count` of what it
/// returns are of the part walked.
fn scan_where(doc: &Document, page: u32, wanted: impl Fn(u32) -> bool, stop_at: Option<usize>) -> Result<Scan, Problem> {
    let top = &doc.page(page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?.shapes;
    let mut out = Scan::default();
    // The list being walked and the place of the next shape in it, one frame for each level.
    let mut stack: Vec<(&ShapeList, usize)> = vec![(top, 0)];
    let mut path: Vec<u32> = Vec::new();
    while let Some(frame) = stack.last_mut() {
        let (list, at) = *frame;
        let Some(shape) = list.get(at) else {
            stack.pop();
            continue;
        };
        frame.1 = at.saturating_add(1);
        out.count = out.count.saturating_add(1);
        #[cfg(test)]
        NODES_WALKED.with(|c| c.set(c.get() + 1));
        if out.count > MAX_SHAPES_PER_PAGE {
            return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
        }
        out.highest = out.highest.max(shape.id);
        let depth = stack.len();
        path.truncate(depth.saturating_sub(1));
        path.push(u32::try_from(at).unwrap_or(u32::MAX));
        if wanted(shape.id) && !out.found.contains_key(&shape.id) {
            out.found.insert(shape.id, ShapeRef { owner: Owner::Page(page), path: path.clone() });
            if stop_at.is_some_and(|n| out.found.len() >= n) {
                return Ok(out);
            }
        }
        if depth < MAX_SHAPE_DEPTH && !shape.children.is_empty() {
            stack.push((&shape.children, 0));
        }
    }
    Ok(out)
}

/// Shapes of a page found by their `ID`s in walks of it ([`scan`]), and every `ID` looked for, found or not, so that an `ID` is looked for once: the
/// settling of an edit looks up the shapes it changed, the connectors glued to them and the shapes those are glued to, in a walk or two of the page in
/// all and not a search each (a guide that carries a thousand shapes made a thousand searches of the page).
#[derive(Debug, Default)]
pub(crate) struct Located {
    paths: HashMap<u32, ShapeRef>,
    looked: HashSet<u32>,
}

impl Located {
    /// Where the shape with `id` is (the first in document order, as `Document::find_shape` gives it), if it was looked for and found.
    pub(crate) fn get(&self, id: &u32) -> Option<&ShapeRef> {
        self.paths.get(id)
    }

    /// Looks for those of `ids` not looked for yet, in one walk of the page; none when every one was.
    pub(crate) fn find(&mut self, doc: &Document, page: u32, ids: impl IntoIterator<Item = u32>) -> Result<(), Problem> {
        let wanted: HashSet<u32> = ids.into_iter().filter(|id| !self.looked.contains(id)).collect();
        if wanted.is_empty() {
            return Ok(());
        }
        let found = scan(doc, page, &wanted)?.found;
        self.looked.extend(wanted);
        self.paths.extend(found);
        Ok(())
    }
}

/// The page a command about shapes works on: the one named, else the first page that holds every one of `ids` (found with one walk of a page for
/// all of them, where `Document::find_shape` is asked once for each), else the first page that is not a background ([`Document::first_foreground_page`]). `None` for a document with no pages.
/// **The pages are looked at in the order of the tabs**: the pages that are not backgrounds, then the backgrounds. A shape's `ID` is its page's own, so a background page and another may both
/// have a shape 1, and a file lists its background pages first: a search in the model's order would take the background's shape for the one a person means.
pub fn page_holding(doc: &Document, named: Option<u32>, ids: &[u32]) -> Option<u32> {
    if named.is_some() {
        return named;
    }
    let wanted: HashSet<u32> = ids.iter().copied().collect();
    if !wanted.is_empty() {
        let tabs = doc.pages.iter().filter(|p| !p.background).chain(doc.pages.iter().filter(|p| p.background));
        for page in tabs {
            if find_named(doc, page.id, &wanted).is_ok_and(|found| found.len() == wanted.len()) {
                return Some(page.id);
            }
        }
    }
    doc.first_foreground_page().map(|p| p.id)
}

/// The `ID`s named, each once, in the order they are named.
pub(crate) fn distinct(ids: &[u32]) -> Vec<u32> {
    let mut seen: HashSet<u32> = HashSet::with_capacity(ids.len());
    ids.iter().copied().filter(|id| seen.insert(*id)).collect()
}

/// The shapes named by `ids` on a page, each once, in document order: the path to each. Every `ID` must be on the page.
pub(crate) fn named(doc: &Document, page: u32, ids: &[u32]) -> Result<(Scan, Vec<(u32, ShapeRef)>), Problem> {
    let ids = distinct(ids);
    let wanted: HashSet<u32> = ids.iter().copied().collect();
    let scan = scan(doc, page, &wanted)?;
    let mut shapes: Vec<(u32, ShapeRef)> = Vec::with_capacity(ids.len());
    for id in ids {
        let at = scan.found.get(&id).ok_or_else(|| Problem::bad(format!("page {page} has no shape {id}")))?;
        shapes.push((id, at.clone()));
    }
    shapes.sort_by(|a, b| a.1.path.cmp(&b.1.path));
    Ok((scan, shapes))
}

/// The shapes of `shapes` (in document order, as [`named`] gives them) that are not inside another of them. A group that is moved, copied or
/// ungrouped takes its members with it, so a member named with its group is left to the group.
pub(crate) fn outermost(shapes: Vec<(u32, ShapeRef)>) -> Vec<(u32, ShapeRef)> {
    let mut kept: Vec<(u32, ShapeRef)> = Vec::with_capacity(shapes.len());
    for (id, at) in shapes {
        // In document order a shape's members follow it directly, so the last one kept is the only one that can hold this one.
        if kept.last().is_some_and(|(_, last)| at.path.len() > last.path.len() && at.path.starts_with(&last.path)) {
            continue;
        }
        kept.push((id, at));
    }
    kept
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Arc;

    use diacraft_doc::{Page, Shape};

    use super::*;

    fn group(id: u32, members: Vec<Shape>) -> Shape {
        let mut g = Shape::new(id);
        g.children = ShapeList::from_vec(members.into_iter().map(Arc::new).collect());
        g
    }

    /// 1, group 2 holding 3 and group 4 (holding 5), then 6, then a second 3 at the top level.
    fn document() -> Document {
        let mut doc = Document::empty();
        let mut page = Page::new(1, "P");
        let shapes = vec![Shape::new(1), group(2, vec![Shape::new(3), group(4, vec![Shape::new(5)])]), Shape::new(6), Shape::new(3)];
        page.shapes = ShapeList::from_vec(shapes.into_iter().map(Arc::new).collect());
        doc.pages.push(Arc::new(page));
        doc
    }

    #[test]
    fn a_scan_finds_the_first_shape_in_document_order_and_counts_them_all() {
        let doc = document();
        let wanted: HashSet<u32> = [3, 5, 6, 99].into_iter().collect();
        let scan = scan(&doc, 1, &wanted).unwrap();
        assert_eq!((scan.highest, scan.count), (6, 7));
        assert_eq!(scan.found.get(&3).unwrap().path, [1, 0], "the member of the group, which comes before the shape at the top level");
        assert_eq!(scan.found.get(&5).unwrap().path, [1, 1, 0]);
        assert_eq!(scan.found.get(&6).unwrap().path, [2]);
        assert!(!scan.found.contains_key(&99));
        // It is the shape `find_shape` gives.
        for id in [3, 5, 6] {
            assert_eq!(doc.find_shape(Owner::Page(1), id).unwrap(), *scan.found.get(&id).unwrap());
        }
        assert!(super::scan(&doc, 7, &wanted).is_err(), "a page that is not there");
    }

    #[test]
    fn find_named_gives_the_first_shape_with_each_id_and_stops_at_the_last_one_found() {
        let doc = document();
        let wanted: HashSet<u32> = [3, 1].into_iter().collect();
        NODES_WALKED.with(|c| c.set(0));
        let found = find_named(&doc, 1, &wanted).unwrap();
        assert_eq!(found.get(&3).unwrap().path, [1, 0], "the member of the group, which comes before the shape at the top level");
        assert_eq!(found.get(&1).unwrap().path, [0]);
        // They are the shapes `find_shape` gives.
        for id in [3, 1] {
            assert_eq!(doc.find_shape(Owner::Page(1), id).unwrap(), *found.get(&id).unwrap());
        }
        // The shapes are 1, 2, 3 (in the group), 4, 5, 6 and the second 3: it stops at the 3 in the group, the third shape, and goes no further.
        assert_eq!(NODES_WALKED.with(std::cell::Cell::get), 3);
        // An `ID` that is not there is left out, and the walk then goes to the end of the page.
        NODES_WALKED.with(|c| c.set(0));
        let wanted: HashSet<u32> = [6, 99].into_iter().collect();
        let found = find_named(&doc, 1, &wanted).unwrap();
        assert_eq!((found.len(), found.contains_key(&6)), (1, true));
        assert_eq!(NODES_WALKED.with(std::cell::Cell::get), 7, "every shape of the page");
        // No `ID` wanted: nothing is walked, and the page is not even looked for.
        NODES_WALKED.with(|c| c.set(0));
        assert!(find_named(&doc, 42, &HashSet::new()).unwrap().is_empty());
        assert_eq!(NODES_WALKED.with(std::cell::Cell::get), 0);
        assert!(find_named(&doc, 42, &wanted).unwrap_err().message.contains("no page 42"));
    }

    #[test]
    fn named_gives_the_shapes_find_shape_gives_and_refuses_one_that_is_not_there_in_the_words_of_require_shape() {
        let doc = document();
        NODES_WALKED.with(|c| c.set(0));
        let named = Named::of(&doc, 1, [5, 3, 5, 99]).unwrap();
        assert_eq!(NODES_WALKED.with(std::cell::Cell::get), 7, "one walk, to the end of the page for the `ID` that is not there");
        for id in [3, 5] {
            assert_eq!(named.require(id).unwrap(), doc.find_shape(Owner::Page(1), id).unwrap());
            assert_eq!(named.get(id), doc.find_shape(Owner::Page(1), id).as_ref());
        }
        // An `ID` that is not on the page is refused in the words of `require_shape`; one that was not asked for is not among the answers either.
        let (ours, theirs) = (named.require(99).unwrap_err(), super::super::require_shape(&doc, 1, 99).unwrap_err());
        assert_eq!((ours.message.as_str(), ours.bad_params), (theirs.message.as_str(), theirs.bad_params));
        assert_eq!(ours.message, "page 1 has no shape 99");
        assert!(named.get(99).is_none() && named.get(6).is_none());
        // A page that is not there, refused as `require_shape` refuses it; no `ID`, no walk and no look for the page.
        let (ours, theirs) = (Named::of(&doc, 42, [1]).err().unwrap(), super::super::require_shape(&doc, 42, 1).unwrap_err());
        assert_eq!((ours.message, ours.bad_params), (theirs.message, theirs.bad_params));
        NODES_WALKED.with(|c| c.set(0));
        assert!(Named::of(&doc, 42, []).unwrap().get(1).is_none());
        assert_eq!(NODES_WALKED.with(std::cell::Cell::get), 0);
    }

    #[test]
    fn the_page_holding_every_id_is_found_with_one_walk_of_each_page() {
        let mut doc = document();
        let mut second = Page::new(2, "Q");
        second.shapes = ShapeList::from_vec(vec![Arc::new(Shape::new(8)), Arc::new(Shape::new(9)), Arc::new(Shape::new(3))]);
        doc.pages.push(Arc::new(second));
        // The first page that holds them all, the page named whatever it holds, and the first page when none does or none are named.
        assert_eq!(page_holding(&doc, None, &[8, 9, 3]), Some(2));
        assert_eq!(page_holding(&doc, None, &[3, 5]), Some(1));
        assert_eq!(page_holding(&doc, None, &[5, 8]), Some(1), "no page holds both: the first page");
        assert_eq!(page_holding(&doc, Some(2), &[5]), Some(2));
        assert_eq!(page_holding(&doc, None, &[]), Some(1));
        assert_eq!(page_holding(&Document::empty(), None, &[1]), None);
        // 4,096 ids on a page of many shapes: found one by one each would be a walk of the page, and here it is the one.
        let mut big = Document::empty();
        let mut page = Page::new(1, "P");
        page.shapes = ShapeList::from_vec((1..=30_000).map(|id| Arc::new(Shape::new(id))).collect());
        big.pages.push(Arc::new(page));
        let ids: Vec<u32> = (0..4_096).map(|k| 1 + k * 7).collect();
        NODES_WALKED.with(|c| c.set(0));
        diacraft_doc::work::reset();
        assert_eq!(page_holding(&big, None, &ids), Some(1));
        assert!(NODES_WALKED.with(std::cell::Cell::get) <= 30_000, "one walk of the page");
        assert_eq!(diacraft_doc::work::finds_visited(), 0);
    }

    #[test]
    fn named_lists_each_shape_once_in_document_order_and_refuses_one_that_is_not_there() {
        let doc = document();
        let (_, shapes) = named(&doc, 1, &[6, 5, 6, 1]).unwrap();
        assert_eq!(shapes.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [1, 5, 6]);
        let err = named(&doc, 1, &[1, 42]).unwrap_err();
        assert!(err.bad_params && err.message.contains("no shape 42"), "{err:?}");
    }

    #[test]
    fn outermost_leaves_a_member_named_with_its_group_to_the_group() {
        let doc = document();
        let (_, shapes) = named(&doc, 1, &[2, 3, 4, 5, 6]).unwrap();
        let ids: Vec<u32> = outermost(shapes).iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [2, 6]);
        // A shape with the id of a member, at the top level, is a different shape and is not inside the group.
        let (_, only) = named(&doc, 1, &[4, 5]).unwrap();
        assert_eq!(outermost(only).iter().map(|(id, _)| *id).collect::<Vec<_>>(), [4]);
    }

    #[test]
    fn a_page_deeper_than_the_model_allows_is_walked_to_its_limit_and_not_further() {
        let mut shape = Shape::new(1);
        for id in 2..=(MAX_SHAPE_DEPTH as u32 + 50) {
            let mut outer = Shape::new(id);
            outer.children = ShapeList::from_vec(vec![Arc::new(shape)]);
            shape = outer;
        }
        let mut doc = Document::empty();
        let mut page = Page::new(1, "P");
        page.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
        doc.pages.push(Arc::new(page));
        let scan = scan(&doc, 1, &HashSet::new()).unwrap();
        assert_eq!(scan.count, MAX_SHAPE_DEPTH, "the walk stops at the depth the model allows");
    }
}
