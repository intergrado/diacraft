//! A page's changes turned into edits of the bytes of its part, and the page's new index.
//!
//! The changes come from [`compare`](super::compare) as ordinals, and the scan ([`scan`](super::scan)) says where each element of the page
//! is. For each shape that changed: the frame, the ends or the route is written ([`Editor`]); a deleted shape's unit is taken out, and with
//! it every element inside it, the ones the model never held included; a level whose shapes changed order is put in the new order, by moving
//! as few units as can do it (a reordered shape's element moves); and a connector that lost its glue has the attributes taken out.
//!
//! **Order.** Where a page's shapes carry `draw:z-index`, drawing order is the attribute's, and the document order is not the
//! drawing order: a reorder there writes the attribute, on each shape whose place changed, and moves no element; each of the values the shapes
//! had is given to the shape that now has its place, so the set of numbers stays as the file had it. A shape of such a page that has no
//! attribute takes its place from where it stands among the others (the reader numbers it so), which a delete before it changes: the plan
//! works out the order the reader will read and writes an attribute on a shape that would otherwise be read in another place. Where no
//! shape carries the attribute (and in a group, whose members never do) drawing order is document order, and a reorder moves elements: those
//! outside a longest run that keeps its order move, each put after the sister that stays before it, or before the first that stays.
//!
//! **The new index.** After a save the file's elements are numbered anew by the reader's rule, and the model's shapes keep their `ID`s:
//! the index of the save maps each `ID` to the ordinal its element now has, worked out from the page's tree with the deleted shapes taken
//! out and the moved ones in their new places.

use std::collections::{HashMap, HashSet};

use super::attrs::TagEdit;
use super::compare::{FrameChange, Level, PageDiff, ShapeDiff};
use super::frame::Editor;
use super::route::{self, Route};
use super::scan::{Kind, ScanPage, ScanShape};
use super::stylemap::StyleEdit;
use super::{SaveNote, WriteError};
use crate::index::PageShapes;
use crate::read::names::Ns;

/// What applying a page's changes left: which elements are gone (by ordinal, index `ordinal`), and the page's new index.
pub(crate) struct Applied {
    pub gone: Vec<bool>,
    pub shapes: PageShapes,
    /// The ordinal each element of the page has after the save, by the ordinal it had (0 for one that is gone; index 0 is the page).
    pub new_ord: Vec<u32>,
    /// The new shapes' start tags: the ordinal each has after the save, and the attributes written on it.
    pub new_tags: Vec<(u32, Vec<(Ns, &'static str)>)>,
    /// The paragraphs written on the new shapes, by the ordinal each shape has after the save, with the text each reads as.
    pub new_texts: Vec<(u32, Vec<(usize, String)>)>,
    /// The attributes written on the geometry elements of the new shapes, by the ordinal each shape has after the save.
    pub new_kids: Vec<(u32, Vec<(Ns, &'static str)>)>,
    /// The new shapes that carry tier 2, by the ordinal each has after the save, with what each carries: what the check of the bytes made reads the file back
    /// for.
    pub new_tier2: Vec<(u32, std::sync::Arc<crate::ext::payload::Tier2>)>,
}

/// `n` copies of `value`, an error and not an abort when the memory for them is not had: the lists of a plan are as long as the
/// page's elements.
fn filled<T: Clone>(n: usize, value: T) -> Result<Vec<T>, WriteError> {
    let mut list = Vec::new();
    list.try_reserve_exact(n).map_err(|_| WriteError::Limit { what: "memory for the plan of a page", max: n as u64 })?;
    list.resize(n, value);
    Ok(list)
}

/// Applies the changes of one page to the bytes of its part through `ed`, and works out the page's new index. `held` is the index the caller
/// holds for the page.
pub(crate) fn apply_page(ed: &mut Editor<'_, '_>, scan: &ScanPage, diff: &PageDiff, held: &PageShapes) -> Result<Applied, WriteError> {
    if scan.shapes.len() != diff.elements as usize {
        return Err(WriteError::Mismatch("the part holds other shape elements than the file was read with"));
    }
    ed.height = diff.height;
    ed.unit = diff.unit;
    let gone = closure(scan, &diff.deleted)?;
    for change in &diff.shapes {
        apply_shape(ed, scan, diff, change, &gone)?;
    }
    for &(ord, start) in &diff.unglue {
        let shape = element(scan, ord)?;
        ed.unglue(shape, start)?;
    }
    // A group of the file whose frame is not the box of its members is written as its members, and said.
    for &group in &diff.unkept {
        let (glued, glued_total) = super::groups::glued_to(&diff.glued_to_groups, group);
        ed.notes.push(crate::SaveNote::GroupFrameNotKept { page: diff.page_at, shape: group, glued, glued_total });
    }
    for ord in 1..=scan.shapes.len() as u32 {
        let Some(shape) = scan.shape(ord) else { continue };
        if gone.get(ord as usize).copied().unwrap_or(false) && !gone.get(shape.parent as usize).copied().unwrap_or(false) {
            ed.splicer.remove(shape.lead, shape.end);
            // The styles the unit named lose those references, and a style nothing names after the save goes.
            if let Some(book) = ed.book.as_mut() {
                book.unit_removed(shape.lead, shape.end, ed.work);
            }
        }
    }
    let mut children = children_of(scan, &gone)?;
    let mut orders = apply_orders(ed, scan, diff, held, &gone, &mut children)?;
    // The page's comments: the annotations the reader took are taken out, and the new ones are put at the end of the page, in the same edit as the new shapes
    // when the page is written as an empty element.
    let block = match (&diff.comments, diff.held_at) {
        (Some(edit), Some(_)) => {
            let made = super::comments::prepare(ed, scan, scan.scope, diff, edit, false)?;
            // What the annotations need declared is declared once, on the page's start tag, before its end (`>` or `/>`).
            if !made.declare.is_empty() {
                ed.splicer.insert(scan.open.1.saturating_sub(if scan.empty { 2 } else { 1 }), made.declare.clone());
            }
            made.text
        }
        _ => String::new(),
    };
    if scan.empty && !diff.new.is_empty() {
        orders.tail.clone_from(&block);
    }
    let wrote = place_new(ed, scan, diff, held, &gone, &mut children, &orders)?;
    if !block.is_empty() && !(scan.empty && !diff.new.is_empty()) {
        if scan.empty {
            let end = scan.open.1;
            ed.splicer.replace(end.saturating_sub(2), end, format!(">{block}</{}>", scan.qname));
        } else {
            ed.splicer.insert(scan.close_start, block);
        }
    }
    let (shapes, new_ord) = renumber(scan, diff, held, &children, &gone)?;
    let shapes = {
        let routes = kept_routes(ed, diff, held, &shapes)?;
        let mut shapes = shapes.with_kept_routes(routes);
        // Which of the page's annotation elements are the model's comments after the save: the ones the page held, or the ones this save wrote.
        let (places, total) = match (&diff.comments, diff.held_at) {
            (Some(edit), Some(_)) => super::comments::places_after(edit),
            _ => {
                let (places, total) = held.comments();
                (places.to_vec(), total)
            }
        };
        shapes.set_comments(places, total);
        shapes
    };
    let pseudo = scan.shapes.len() + 1;
    let (mut new_tags, mut new_texts, mut new_kids, mut new_tier2) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (k, w) in wrote.into_iter().enumerate() {
        let ord = new_ord.get(pseudo + k).copied().unwrap_or(0);
        if ord != 0
            && let Some(t) = &w.tier2
        {
            new_tier2.push((ord, std::sync::Arc::clone(t)));
        }
        new_tags.push((ord, w.attrs));
        if !w.paras.is_empty() {
            new_texts.push((ord, w.paras));
        }
        if !w.kid.is_empty() {
            new_kids.push((ord, w.kid));
        }
    }
    Ok(Applied { gone, shapes, new_ord, new_tags, new_texts, new_kids, new_tier2 })
}

/// What `apply_orders` settled of the page's top level, which the new shapes are put by.
struct Orders {
    /// The page's shapes carry `draw:z-index`: what the plan of the attribute made of the shapes of the file that remain.
    z: Option<ZState>,
    /// The page's top level is put in another order in the same save as new shapes are added, on a page whose order is the document's: the elements
    /// move and the new ones are put among them by one plan (`reorder_with_new`), and `apply_orders` has moved none.
    top_with_new: Option<usize>,
    /// The page's comments as text, which a page written as an empty element gets in the same edit as its new shapes; empty for any other page.
    tail: String,
}

/// Writes the page's new shapes and puts them into the page's top level of the plan: each new shape takes a place number after the page's elements
/// (the `k`-th is `scan.shapes.len() + 1 + k`) and stands after the shape it follows, those after one shape in the order they were made. Gives the
/// attributes written on each new shape's start tag.
///
/// **A reorder in the same save**. Where the order is the document's, the new shapes and the shapes that move are put by one plan
/// in the model's order ([`reorder_with_new`]: one place can take a shape that moves and a new shape, and they keep the order the model has). Where the
/// page's shapes carry `draw:z-index`, a new shape is written with the value of the shape before it in the model's order and its element is put after
/// that shape's element, so the reader (which sorts by the value, then by the place) finds it just after it; each shape of the file that has no value is
/// given the one the plan says when the new elements would change the place the reader numbers it at; and the order the reader will read is worked out
/// and must be the model's, or the page is refused in words.
fn place_new(
    ed: &mut Editor<'_, '_>,
    scan: &ScanPage,
    diff: &PageDiff,
    held: &PageShapes,
    gone: &[bool],
    children: &mut Vec<Vec<u32>>,
    orders: &Orders,
) -> Result<Vec<super::shapes::NewWrote>, WriteError> {
    if diff.new.is_empty() {
        return Ok(Vec::new());
    }
    let modelled: HashSet<u32> = held.ordinal_map().values().copied().collect();
    let top: Vec<u32> = children.first().cloned().unwrap_or_default();
    for n in &diff.new {
        if n.parent.is_none()
            && let super::compare::Anchor::Ord(o) = n.after
            && gone.get(o as usize).copied().unwrap_or(false)
        {
            return Err(WriteError::Unsupported("a new shape after a shape the same save deletes"));
        }
    }
    let pseudo = scan.shapes.len() + 1;
    let zs = match &orders.z {
        Some(z) => z_of_new(diff, z)?,
        None => vec![None; diff.new.len()],
    };
    let (texts, wrote) = ed.new_shapes(scan, diff, held, &zs)?;
    children.try_reserve(diff.new.len()).map_err(|_| WriteError::Limit { what: "memory for the plan of a page", max: diff.new.len() as u64 })?;
    children.resize_with(pseudo + diff.new.len(), Vec::new);
    // The members of a new group are the group's numbered children, in the order they are written in it: the numbering of the page goes into the group before it
    // goes on, and the group's members get the ordinals the reader will number them with.
    for (k, n) in diff.new.iter().enumerate() {
        if let Some(group) = n.parent {
            let place = u32::try_from(pseudo + k).unwrap_or(u32::MAX);
            children.get_mut(pseudo + group).ok_or(WriteError::Mismatch("a new shape is a member of a group that is not made"))?.push(place);
        }
    }
    if let Some(at) = orders.top_with_new {
        let level = diff.levels.get(at).ok_or(WriteError::Mismatch("the plan names a level the page does not have"))?;
        let slot = children.first_mut().ok_or(WriteError::Mismatch("a page has no plan"))?;
        reorder_with_new(ed, scan, level, diff, &modelled, slot, texts, pseudo)?;
        return Ok(wrote);
    }
    // Who follows whom: a shape of the file, an earlier new shape, or nothing.
    let mut after_ord: HashMap<u32, Vec<usize>> = HashMap::new();
    let mut after_new: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut first: Vec<usize> = Vec::new();
    for (k, n) in diff.new.iter().enumerate().filter(|(_, n)| n.parent.is_none()) {
        match n.after {
            super::compare::Anchor::First => first.push(k),
            super::compare::Anchor::Ord(o) => after_ord.entry(o).or_default().push(k),
            super::compare::Anchor::New(j) => after_new.entry(j).or_default().push(k),
        }
    }
    let mut out: Vec<u32> = Vec::with_capacity(top.len() + diff.new.len());
    let emit = |starts: &[usize], out: &mut Vec<u32>| {
        // Depth first, a stack of the new shapes still to be put: those after one shape come before those after the next.
        let mut stack: Vec<usize> = starts.iter().rev().copied().collect();
        while let Some(k) = stack.pop() {
            out.push(u32::try_from(pseudo + k).unwrap_or(u32::MAX));
            if let Some(next) = after_new.get(&k) {
                stack.extend(next.iter().rev().copied());
            }
        }
    };
    emit(&first, &mut out);
    for &ord in &top {
        out.push(ord);
        if let Some(list) = after_ord.get(&ord) {
            emit(list, &mut out);
        }
    }
    // The elements go where their anchors say: each after the element of the shape it follows, or first on the page; those after one anchor keep the
    // order they were made in.
    let mut at: Vec<usize> = Vec::with_capacity(diff.new.len());
    for n in &diff.new {
        // A member of a new group is put inside the group's element: no place of its own (and no text of its own, below).
        if n.parent.is_some() {
            at.push(scan.open.1);
            continue;
        }
        at.push(match n.after {
            // First on the page, after the children the schema puts before the shapes.
            super::compare::Anchor::First => scan.first_place.unwrap_or(scan.open.1),
            super::compare::Anchor::Ord(o) => element(scan, o)?.end,
            super::compare::Anchor::New(k) => *at.get(k).ok_or(WriteError::Mismatch("a new shape follows one that is not made yet"))?,
        });
    }
    if let Some(z) = &orders.z {
        verify_z(ed, scan, &out, &modelled, pseudo, &zs, z, &top_model_order(diff, &z.now, pseudo))?;
    }
    // A member of a new group has no text of its own here: its element is inside its group's.
    let tops = texts.into_iter().enumerate().filter(|(k, _)| diff.new.get(*k).is_some_and(|n| n.parent.is_none()));
    if scan.empty {
        // A page written as an empty element (`<draw:page .../>`) ends where its start tag does: its last two bytes, `/>`, are replaced by `>`, the new shapes in the order
        // they were made and the end tag, in one edit **inside** the element, so that it goes with the element when the page is put in another place (an edit at
        // the edge of a unit that moves stays where the unit was).
        let end = scan.open.1;
        let mut body = String::from(">");
        for (_, text) in tops {
            body.push_str(&text);
        }
        body.push_str(&orders.tail);
        body.push_str(&format!("</{}>", scan.qname));
        ed.splicer.replace(end.saturating_sub(2), end, body);
    } else {
        for (k, text) in tops {
            ed.splicer.insert(*at.get(k).ok_or(WriteError::Mismatch("a new shape has no place"))?, text);
        }
    }
    if let Some(slot) = children.first_mut() {
        *slot = out;
    }
    Ok(wrote)
}

/// The page's top level in the model's order, the shapes of the file that remain (`now`, in drawing order) and the new shapes after the ones they follow:
/// each new shape by its place number, `pseudo` and its place among the new shapes.
fn top_model_order(diff: &PageDiff, now: &[u32], pseudo: usize) -> Vec<u32> {
    let mut after_ord: HashMap<u32, Vec<usize>> = HashMap::new();
    let mut after_new: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut first: Vec<usize> = Vec::new();
    for (k, n) in diff.new.iter().enumerate().filter(|(_, n)| n.parent.is_none()) {
        match n.after {
            super::compare::Anchor::First => first.push(k),
            super::compare::Anchor::Ord(o) => after_ord.entry(o).or_default().push(k),
            super::compare::Anchor::New(j) => after_new.entry(j).or_default().push(k),
        }
    }
    let mut out: Vec<u32> = Vec::with_capacity(now.len() + diff.new.len());
    let emit = |starts: &[usize], out: &mut Vec<u32>| {
        let mut stack: Vec<usize> = starts.iter().rev().copied().collect();
        while let Some(k) = stack.pop() {
            out.push(u32::try_from(pseudo + k).unwrap_or(u32::MAX));
            if let Some(next) = after_new.get(&k) {
                stack.extend(next.iter().rev().copied());
            }
        }
    };
    emit(&first, &mut out);
    for &ord in now {
        out.push(ord);
        if let Some(list) = after_ord.get(&ord) {
            emit(list, &mut out);
        }
    }
    out
}

/// The `draw:z-index` each new shape is written with: the value the shape before it in the model's order has after this save's writes, and for one that
/// is first the least value of the shapes of the file that remain (0 when none remains). Those after one shape share its value, and the reader's tie
/// (the place) puts each after the one before it, since the elements are put in that order.
fn z_of_new(diff: &PageDiff, z: &ZState) -> Result<Vec<Option<i64>>, WriteError> {
    let low = z.final_z.values().min().copied().unwrap_or(0);
    let mut out: Vec<Option<i64>> = Vec::new();
    out.try_reserve_exact(diff.new.len()).map_err(|_| WriteError::Limit { what: "memory for the plan of a page", max: diff.new.len() as u64 })?;
    for n in &diff.new {
        // A member of a new group has no `draw:z-index`: the group has one, as a shape of the top level.
        if n.parent.is_some() {
            out.push(None);
            continue;
        }
        let value = match n.after {
            super::compare::Anchor::First => low,
            super::compare::Anchor::Ord(o) => {
                z.final_z.get(&o).copied().ok_or(WriteError::Mismatch("a new shape follows a shape that has no place in the order"))?
            }
            super::compare::Anchor::New(j) => {
                out.get(j).copied().flatten().ok_or(WriteError::Mismatch("a new shape follows a new shape that is not made yet"))?
            }
        };
        out.push(Some(value));
    }
    Ok(out)
}

/// Checks, and where it can mends, what the reader will read of a page that carries `draw:z-index` after the new elements are put in: `doc_order` is the page's
/// top level as the document will hold it, and the reader numbers each shape without the attribute by its place among the shapes it models, which a new
/// element changes. A shape of the file that has no attribute and would be read at another value than the order needs is given one. The order the reader
/// will read (the value, then the place) must be `model`, or the page is refused in words (values that repeat cannot always say it).
#[allow(clippy::too_many_arguments)]
fn verify_z(
    ed: &mut Editor<'_, '_>,
    scan: &ScanPage,
    doc_order: &[u32],
    modelled: &HashSet<u32>,
    pseudo: usize,
    zs: &[Option<i64>],
    z: &ZState,
    model: &[u32],
) -> Result<(), WriteError> {
    let new_z = |item: u32| -> Option<i64> { (item as usize >= pseudo).then(|| zs.get(item as usize - pseudo).copied().flatten()).flatten() };
    let items: Vec<u32> = doc_order.iter().copied().filter(|o| modelled.contains(o) || *o as usize >= pseudo).collect();
    ed.work.order_steps = ed.work.order_steps.saturating_add((items.len() + model.len()) as u64);
    let mut read: Vec<(i64, usize, u32)> = Vec::new();
    read.try_reserve_exact(items.len()).map_err(|_| WriteError::Limit { what: "memory for the plan of a page", max: items.len() as u64 })?;
    for (place, &item) in items.iter().enumerate() {
        let place_value = i64::try_from(place).unwrap_or(i64::MAX);
        let value = if item as usize >= pseudo {
            new_z(item).unwrap_or(place_value)
        } else if let Some(v) = z.wrote.get(&item).copied().or_else(|| z.attr.get(&item).copied()) {
            v
        } else {
            // A shape of the file with no attribute is read at its place: when the order needs another value it is given one.
            let want = z.final_z.get(&item).copied().unwrap_or(place_value);
            if want != place_value {
                ed.set_z(element(scan, item)?, want)?;
            }
            want
        };
        read.push((value, place, item));
    }
    read.sort_unstable();
    if read.into_iter().map(|(_, _, o)| o).ne(model.iter().copied()) {
        return Err(WriteError::Unsupported(
            "a new shape or a reorder on a page whose draw:z-index values the reader would read in another order than the drawing's (values that repeat)",
        ));
    }
    Ok(())
}

/// A reorder of the page's top level and new shapes in one save, on a page whose order is the document's: one plan in the model's order. The shapes that
/// stay are a longest run that keeps its order (as [`move_level`] finds it); each shape that moves, and each new shape, goes after the shape that stays
/// before it in the model's order, or before the first that stays, **and several that go to one place are put there in the model's order** (the edits at
/// one place keep the order they were made in, so a moved unit and a new element are made in turn). `kids` is the page's numbered children, and is given
/// back as they are after the save.
#[allow(clippy::too_many_arguments)]
fn reorder_with_new(
    ed: &mut Editor<'_, '_>,
    scan: &ScanPage,
    level: &Level,
    diff: &PageDiff,
    modelled: &HashSet<u32>,
    kids: &mut Vec<u32>,
    texts: Vec<String>,
    pseudo: usize,
) -> Result<(), WriteError> {
    let before: Vec<u32> = kids.iter().copied().filter(|o| modelled.contains(o)).collect();
    if before != level.was {
        return Err(WriteError::Mismatch("a level's shapes are not in the order the file was read with"));
    }
    let place: HashMap<u32, usize> = before.iter().enumerate().map(|(i, o)| (*o, i)).collect();
    let positions: Vec<usize> = level
        .now
        .iter()
        .map(|o| place.get(o).copied().ok_or(WriteError::Mismatch("a reordered shape is not in its level")))
        .collect::<Result<_, _>>()?;
    let stay: HashSet<usize> = longest_run(&positions).into_iter().collect();
    let first_staying = level.now.iter().zip(&positions).find(|(_, p)| stay.contains(p)).map(|(o, _)| *o);
    let model = top_model_order(diff, &level.now, pseudo);
    ed.work.order_steps = ed.work.order_steps.saturating_add((model.len() + before.len()) as u64);
    let mut texts: Vec<Option<String>> = texts.into_iter().map(Some).collect();
    let mut anchor: Option<u32> = None;
    let mut after: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut front: Vec<u32> = Vec::new();
    for &item in &model {
        if (item as usize) < pseudo {
            let at = place.get(&item).copied().ok_or(WriteError::Mismatch("a reordered shape is not in its level"))?;
            if stay.contains(&at) {
                anchor = Some(item);
                continue;
            }
        }
        let to = match (anchor, first_staying) {
            (Some(a), _) => element(scan, a)?.end,
            (None, Some(f)) => element(scan, f)?.lead,
            (None, None) => return Err(WriteError::Mismatch("a reordered level has no shape that stays")),
        };
        if (item as usize) < pseudo {
            let unit = element(scan, item)?;
            ed.splicer.move_unit((unit.lead, unit.end), to);
        } else {
            let k = item as usize - pseudo;
            let text = texts.get_mut(k).and_then(Option::take).ok_or(WriteError::Mismatch("a new shape is put twice"))?;
            ed.splicer.insert(to, text);
        }
        match anchor {
            Some(a) => after.entry(a).or_default().push(item),
            None => front.push(item),
        }
    }
    // The page's numbered children as they are after the save: those that stay in place, the movers and the new shapes after their anchors.
    // (`place` holds the shapes of the file alone: a new shape has none, and is no mover.)
    let movers: HashSet<u32> = model.iter().copied().filter(|o| place.get(o).is_some_and(|at| !stay.contains(at))).collect();
    let mut out: Vec<u32> = Vec::with_capacity(kids.len() + diff.new.len());
    for &ord in kids.iter().filter(|o| !movers.contains(o)) {
        if Some(ord) == first_staying {
            out.extend(front.iter().copied());
        }
        out.push(ord);
        if let Some(list) = after.get(&ord) {
            out.extend(list.iter().copied());
        }
    }
    *kids = out;
    Ok(())
}

/// The element of the shape numbered `ord`.
fn element(scan: &ScanPage, ord: u32) -> Result<&ScanShape, WriteError> {
    scan.shape(ord).ok_or(WriteError::Mismatch("a shape of the model has no element in the part"))
}

/// Which elements go with the deleted shapes: each of them, and every element inside one, flagged by ordinal (index 0 is the page, never gone).
fn closure(scan: &ScanPage, deleted: &[u32]) -> Result<Vec<bool>, WriteError> {
    let mut gone = filled(scan.shapes.len() + 1, false)?;
    for &ord in deleted {
        if let Some(slot) = gone.get_mut(ord as usize) {
            *slot = true;
        }
    }
    for (i, shape) in scan.shapes.iter().enumerate() {
        // A group's members follow it in the pre-order, so its flag is set before theirs are read.
        if gone.get(shape.parent as usize).copied().unwrap_or(false)
            && let Some(slot) = gone.get_mut(i + 1)
        {
            *slot = true;
        }
    }
    Ok(gone)
}

fn apply_shape(ed: &mut Editor<'_, '_>, scan: &ScanPage, diff: &PageDiff, change: &ShapeDiff, gone: &[bool]) -> Result<(), WriteError> {
    let shape = element(scan, change.ord)?;
    if gone.get(change.ord as usize).copied().unwrap_or(false) {
        return Err(WriteError::Mismatch("a shape of the model is inside an element the model deleted"));
    }
    let kind_agrees =
        shape.kind.one_d() == change.one_d && (shape.kind == Kind::Connector) == change.connector && (shape.kind == Kind::Group) == change.group;
    if !kind_agrees {
        return Err(WriteError::Mismatch("the part holds another kind of element than the model's shape"));
    }
    let who = (diff.page_at, change.id);
    // A turn of a shape whose connectors are glued to points LibreOffice places on the box the shape takes up on the page: a `draw:glue-point` of a kind whose
    // points it places so, or a standard point of a polygon, a polyline or a path. The file would put them elsewhere than the model's rows: refused in words.
    if let Some(glue) = change.turned_glue {
        let outline = matches!(shape.kind, Kind::Polygon | Kind::Polyline | Kind::Path);
        let boxed = outline || matches!(shape.kind, Kind::Custom | Kind::Rect | Kind::Ellipse | Kind::Circle | Kind::Frame);
        if (glue.elements && boxed) || (glue.standard && outline) {
            return Err(WriteError::Unsupported(super::compare::WORDS_TURNED_GLUE));
        }
    }
    // The layer the save declares that the model put the shape on.
    if let Some(name) = &change.layer {
        ed.set_layer(shape, name)?;
    }
    match &change.frame {
        FrameChange::None => {}
        FrameChange::Translate { dx, dy } => ed.translate(shape, *dx, -*dy)?,
        FrameChange::Reshape { pose, angle } => ed.reshape(shape, pose, *angle, who)?,
        FrameChange::Ends(ends) => ed.set_ends(shape, *ends)?,
    }
    // A custom shape that holds text after a retext has `draw:auto-grow-height="false"` in its style (the retext row).
    let grow = shape.kind == Kind::Custom && change.text.as_ref().is_some_and(super::text::TextEdit::holds_text);
    // The style, the frame's `draw:auto-grow-height`, and what DiaCraft's extension carries of the shape (a group's own style carries none).
    let tier2 = change.tier2.as_ref().filter(|_| !change.group);
    if (change.style.is_some() || grow || tier2.is_some())
        && let Some(carried) = ed.restyle(shape, change.style.as_ref().unwrap_or(&StyleEdit::default()), grow, tier2)?
    {
        ed.tier2_edited.push((change.ord, carried));
    }
    if let Some(edit) = &change.text {
        let done = ed.retext(shape, edit, who)?;
        if !done.styles.is_empty() {
            ed.inserted.push((change.ord, done.styles));
        }
        if !done.expect.is_empty() {
            ed.zero.push((change.ord, done.expect));
        }
    }
    if let Some(geo) = &change.geo {
        let wrote = ed.outline(shape, geo)?;
        if !wrote.is_empty() {
            ed.kid_edits.push((change.ord, wrote));
        }
    }
    // **The route is written from the cells when the connector was routed anew; otherwise the file's own path is kept, and a change of the page's height shifts it**
    // The height edit keeps `PinY`, so every end of the page moves down it by what the page grew, and the path, which is a fixed distance
    // from the page's top, goes with them. A route the writer cannot write (a hop) is kept as the file has it and said, shifted too: the path and the ends
    // then differ by the hop, as they did before the page changed, and not by the page. **It is said in the save that keeps the path for it, and not again** at the saves after
    // it that only change the page's height (`classify` gives such a connector no route, so it reaches the shift below with none). A path the writer
    // cannot place is another matter: it is not shifted, so the save that cannot shift it says so each time.
    let mut said = false;
    let written = match &change.route {
        Some(Route::Path(segs)) => match route::text(segs, diff.height) {
            Some((path, view_box)) => {
                ed.set_route(shape, &path, &view_box, who)?;
                true
            }
            None => {
                said = true;
                ed.notes.push(SaveNote::RouteNotWritten { page: diff.page_at, shape: change.id });
                false
            }
        },
        Some(Route::Unreadable) => {
            said = true;
            ed.notes.push(SaveNote::RouteNotWritten { page: diff.page_at, shape: change.id });
            false
        }
        None => false,
    };
    let grew = diff.grew_by();
    if change.connector && !written && grew != 0.0 && !ed.shift_route(shape, grew * route::HUNDREDTHS_PER_INCH)? && !said {
        // The file's path cannot be placed: it stays as it is, and the save says the route was not written.
        ed.notes.push(SaveNote::RouteNotWritten { page: diff.page_at, shape: change.id });
    }
    Ok(())
}

/// The numbered children of the page (index 0) and of each shape element, by ordinal, the elements gone left out, in document order.
fn children_of(scan: &ScanPage, gone: &[bool]) -> Result<Vec<Vec<u32>>, WriteError> {
    let mut children: Vec<Vec<u32>> = filled(scan.shapes.len() + 1, Vec::new())?;
    for (i, shape) in scan.shapes.iter().enumerate() {
        let ord = u32::try_from(i + 1).unwrap_or(u32::MAX);
        if !gone.get(i + 1).copied().unwrap_or(false)
            && let Some(list) = children.get_mut(shape.parent as usize)
        {
            list.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the plan of a page", max: scan.shapes.len() as u64 })?;
            list.push(ord);
        }
    }
    Ok(children)
}

fn apply_orders(
    ed: &mut Editor<'_, '_>,
    scan: &ScanPage,
    diff: &PageDiff,
    held: &PageShapes,
    gone: &[bool],
    children: &mut [Vec<u32>],
) -> Result<Orders, WriteError> {
    let modelled: HashSet<u32> = held.ordinal_map().values().copied().collect();
    let top = diff.levels.iter().position(|l| l.parent == 0);
    let top_deleted = diff.deleted.iter().any(|o| scan.shape(*o).is_some_and(|s| s.parent == 0));
    let news = !diff.new.is_empty();
    // The attributes of the page's shapes are looked at only when the page's top level changed.
    let by_z = (top.is_some() || top_deleted || news) && carries_z(ed, scan, children.first().map_or(&[][..], Vec::as_slice), &modelled)?;
    let mut orders = Orders { z: None, top_with_new: None, tail: String::new() };
    if by_z {
        orders.z = Some(write_z(ed, scan, top.and_then(|at| diff.levels.get(at)), children.first().map_or(&[][..], Vec::as_slice), &modelled)?);
    }
    for (at, level) in diff.levels.iter().enumerate() {
        if level.parent == 0 && by_z {
            continue;
        }
        if gone.get(level.parent as usize).copied().unwrap_or(false) {
            continue;
        }
        // New shapes and a reorder of the top level, where the order is the document's: one plan puts both (`place_new`).
        if level.parent == 0 && news {
            orders.top_with_new = Some(at);
            continue;
        }
        let Some(slot) = children.get_mut(level.parent as usize) else { continue };
        move_level(ed, scan, level, &modelled, slot)?;
    }
    Ok(orders)
}

/// Whether any modelled top-level shape element of the page carries `draw:z-index`.
fn carries_z(ed: &mut Editor<'_, '_>, scan: &ScanPage, top: &[u32], modelled: &HashSet<u32>) -> Result<bool, WriteError> {
    for &ord in top.iter().filter(|o| modelled.contains(o)) {
        if z_of(ed, element(scan, ord)?)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The `draw:z-index` an element carries, when it carries one the reader reads.
fn z_of(ed: &mut Editor<'_, '_>, shape: &ScanShape) -> Result<Option<i64>, WriteError> {
    let tag = TagEdit::new(ed.bytes, shape.open, shape.scope, ed.work)?;
    Ok(tag.find(&mut ed.scopes, Ns::Draw, "z-index", ed.work).and_then(|at| crate::read::units::integer(&tag.text(at))))
}

/// What the plan of `draw:z-index` settled for the shapes of the file that remain on a page that carries the attribute.
#[derive(Debug)]
struct ZState {
    /// The shapes of the file that remain, in drawing order.
    now: Vec<u32>,
    /// The value each is read at after the save's writes.
    final_z: HashMap<u32, i64>,
    /// The attributes the shapes had, and the values the save wrote.
    attr: HashMap<u32, i64>,
    wrote: HashMap<u32, i64>,
}

/// Writes `draw:z-index` where the order the reader will read differs from the model's (see the module documentation). `top` is the level
/// that was reordered, if it was; `kids` the page's numbered children that remain.
fn write_z(ed: &mut Editor<'_, '_>, scan: &ScanPage, top: Option<&Level>, kids: &[u32], modelled: &HashSet<u32>) -> Result<ZState, WriteError> {
    // The baseline's modelled top-level elements in document order, and the value the reader gave each (the attribute, else its place).
    let before: Vec<u32> = scan
        .shapes
        .iter()
        .enumerate()
        .filter(|(_, s)| s.parent == 0)
        .map(|(i, _)| u32::try_from(i + 1).unwrap_or(u32::MAX))
        .filter(|o| modelled.contains(o))
        .collect();
    let mut z_was: HashMap<u32, i64> = HashMap::new();
    let mut attr: HashMap<u32, i64> = HashMap::new();
    for (k, &ord) in before.iter().enumerate() {
        let own = z_of(ed, element(scan, ord)?)?;
        if let Some(z) = own {
            attr.insert(ord, z);
        }
        z_was.insert(ord, own.unwrap_or(i64::try_from(k).unwrap_or(i64::MAX)));
    }
    let survivors: Vec<u32> = kids.iter().copied().filter(|o| modelled.contains(o)).collect();
    // The plan counts its own work in `order_steps`: each element of each pass over the shapes and each lookup of one by its ordinal is a step.
    let (writes, now, final_z) = z_values(&before, &survivors, top.map(|l| l.now.as_slice()), &z_was, &attr, &mut ed.work.order_steps)?;
    let mut wrote: HashMap<u32, i64> = HashMap::new();
    for (ord, value) in writes {
        ed.set_z(element(scan, ord)?, value)?;
        wrote.insert(ord, value);
    }
    Ok(ZState { now, final_z, attr, wrote })
}

/// What the plan of `draw:z-index` gives: the writes, the shapes that remain in drawing order, and the value each is read at.
type ZValues = (Vec<(u32, i64)>, Vec<u32>, HashMap<u32, i64>);

/// The `draw:z-index` to write, as `(ordinal, value)`, for a page that carries them, the shapes that remain in drawing order, and the value each is read at. `before` are the baseline's modelled top-level elements in
/// document order, `survivors` those the model still has, `now` the model's drawing order when it reordered the page, `z_was` the value the reader
/// gave each (its attribute, else its place) and `attr` the attributes there are. Each of the values the survivors had goes to the shape that
/// now has its place; a shape is written when its attribute differs, or when it has none and the place it would be read at is not the value. The
/// order the reader will read must come out as the model's: where it cannot (values that repeat), the page is refused.
///
/// **The work is counted** in `steps`: one for each element of each pass over the shapes (a sort is one pass: its comparisons are about `log n` more, which is not counted) and
/// one for each lookup of a shape by its ordinal, so that a test can hold the plan to a number of steps that is linear in the page's shapes and never to a time
/// (the test asserted two seconds and failed once at a load of 75).
fn z_values(
    before: &[u32],
    survivors: &[u32],
    now: Option<&[u32]>,
    z_was: &HashMap<u32, i64>,
    attr: &HashMap<u32, i64>,
    steps: &mut u64,
) -> Result<ZValues, WriteError> {
    let mut count = |n: usize| *steps = steps.saturating_add(n as u64);
    // A shape's membership and place among the survivors are looked up in maps made once, never in the list: a lookup in the list inside a loop over
    // the shapes is quadratic in the shapes of the page.
    let alive: HashSet<u32> = survivors.iter().copied().collect();
    let place_of: HashMap<u32, usize> = survivors.iter().enumerate().map(|(k, o)| (*o, k)).collect();
    count(2 * survivors.len());
    // The drawing order now: the model's, or the baseline's with the deleted taken out when the model did not reorder.
    let now: Vec<u32> = match now {
        Some(order) => {
            count(order.len());
            order.to_vec()
        }
        None => {
            // A lookup in `alive` and in `z_was` for each of the baseline's shapes, then the sort and the list of what is left.
            count(3 * before.len());
            let mut order: Vec<(i64, usize, u32)> =
                before.iter().enumerate().filter(|(_, o)| alive.contains(o)).map(|(k, o)| (z_was.get(o).copied().unwrap_or(0), k, *o)).collect();
            order.sort_unstable();
            count(2 * order.len());
            order.into_iter().map(|(_, _, o)| o).collect()
        }
    };
    let mut values: Vec<i64> = survivors.iter().filter_map(|o| z_was.get(o).copied()).collect();
    values.sort_unstable();
    count(2 * survivors.len());
    let mut final_z: HashMap<u32, i64> = HashMap::new();
    let mut writes = Vec::new();
    for (&ord, &value) in now.iter().zip(&values) {
        // A lookup of the place and of the attribute, and the value put in the map.
        count(3);
        let place = i64::try_from(place_of.get(&ord).copied().unwrap_or(0)).unwrap_or(0);
        match attr.get(&ord) {
            Some(&had) if had == value => {}
            None if value == place => {}
            _ => writes.push((ord, value)),
        }
        final_z.insert(ord, value);
    }
    // What the reader will read: each shape by the value it has now and its place, which must be the model's order.
    let mut read: Vec<(i64, usize, u32)> = survivors.iter().enumerate().map(|(k, o)| (final_z.get(o).copied().unwrap_or(0), k, *o)).collect();
    read.sort_unstable();
    count(3 * survivors.len());
    if read.into_iter().map(|(_, _, o)| o).ne(now.iter().copied()) {
        return Err(WriteError::Unsupported("a reorder on a page whose draw:z-index values repeat"));
    }
    Ok((writes, now, final_z))
}

/// Moves the units of a level into the order the model has, and gives the level's children in their new order.
fn move_level(ed: &mut Editor<'_, '_>, scan: &ScanPage, level: &Level, modelled: &HashSet<u32>, kids: &mut Vec<u32>) -> Result<(), WriteError> {
    let before: Vec<u32> = kids.iter().copied().filter(|o| modelled.contains(o)).collect();
    if before != level.was {
        return Err(WriteError::Mismatch("a level's shapes are not in the order the file was read with"));
    }
    let place: HashMap<u32, usize> = before.iter().enumerate().map(|(i, o)| (*o, i)).collect();
    let positions: Vec<usize> = level
        .now
        .iter()
        .map(|o| place.get(o).copied().ok_or(WriteError::Mismatch("a reordered shape is not in its level")))
        .collect::<Result<_, _>>()?;
    let stay: HashSet<usize> = longest_run(&positions).into_iter().collect();
    // Each shape that moves goes after the sister that stays before it in the new order, or before the first that stays.
    let first_staying = level.now.iter().zip(&positions).find(|(_, p)| stay.contains(p)).map(|(o, _)| *o);
    let mut after: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut front: Vec<u32> = Vec::new();
    let mut anchor: Option<u32> = None;
    for (&ord, position) in level.now.iter().zip(&positions) {
        if stay.contains(position) {
            anchor = Some(ord);
            continue;
        }
        let unit = element(scan, ord)?;
        let to = match (anchor, first_staying) {
            (Some(a), _) => element(scan, a)?.end,
            (None, Some(f)) => element(scan, f)?.lead,
            (None, None) => return Err(WriteError::Mismatch("a reordered level has no shape that stays")),
        };
        ed.splicer.move_unit((unit.lead, unit.end), to);
        match anchor {
            Some(a) => after.entry(a).or_default().push(ord),
            None => front.push(ord),
        }
    }
    // The level's children as they are after the moves: those that stay in place, the movers after their anchors.
    let movers: HashSet<u32> = level.now.iter().zip(&positions).filter(|(_, p)| !stay.contains(p)).map(|(o, _)| *o).collect();
    let mut out: Vec<u32> = Vec::with_capacity(kids.len());
    for &ord in kids.iter().filter(|o| !movers.contains(o)) {
        if Some(ord) == first_staying {
            out.extend(front.iter().copied());
        }
        out.push(ord);
        if let Some(list) = after.get(&ord) {
            out.extend(list.iter().copied());
        }
    }
    *kids = out;
    Ok(())
}

/// The positions (of `values`, indices) of a longest strictly increasing run, in order: the shapes that need not move.
pub(super) fn longest_run(values: &[usize]) -> Vec<usize> {
    // Patience sorting: `tails[k]` is the index of the smallest value that ends a run of k + 1, `from[i]` the index before `i` in its run.
    let mut tails: Vec<usize> = Vec::new();
    let mut from: Vec<Option<usize>> = vec![None; values.len()];
    for (i, &v) in values.iter().enumerate() {
        let at = tails.partition_point(|&t| values.get(t).is_some_and(|x| *x < v));
        from[i] = at.checked_sub(1).and_then(|k| tails.get(k).copied());
        match tails.get_mut(at) {
            Some(slot) => *slot = i,
            None => tails.push(i),
        }
    }
    let mut run = Vec::with_capacity(tails.len());
    let mut at = tails.last().copied();
    while let Some(i) = at {
        run.push(values.get(i).copied().unwrap_or(0));
        at = from.get(i).copied().flatten();
    }
    run.reverse();
    run
}

/// The routes the file holds of the connectors whose path the save kept, for the index to carry: the ones this save kept (`ShapeDiff::file_route`),
/// and the ones an earlier save kept that this save did not route again (a connector it did route has a route written, which is the file's, or kept, which is in the first).
/// A route that could not be written for its numbers (`route::text`, past a trillion hundredths of a millimetre) is the note's and has no entry: a model whose route is that
/// far off the page is compared with the baseline, as before. What is kept is charged to the memory a save may hold, as everything the index keeps is.
fn kept_routes(
    ed: &mut Editor<'_, '_>,
    diff: &PageDiff,
    held: &PageShapes,
    shapes: &PageShapes,
) -> Result<HashMap<u32, Vec<(f64, f64)>>, WriteError> {
    let out = collect_kept(diff, held, |id| shapes.ordinal(id).is_some());
    ed.work.hold(kept_bytes(&out), "memory for the routes a save kept")?;
    Ok(out)
}

/// The bytes the routes `kept` hold: 64 for each connector and 16 for each corner.
fn kept_bytes(kept: &HashMap<u32, Vec<(f64, f64)>>) -> u64 {
    kept.values().fold(0u64, |sum, corners| sum.saturating_add(64).saturating_add((corners.len() as u64).saturating_mul(16)))
}

/// [`kept_routes`] without the charge: `modelled` says whether a connector is still in the model. **An entry carried is carried as it is**, across a change of the page's
/// height too: the corners are measured from the page's bottom, and the save shifts the file's path by what the page grew (`apply_shape`), so the path the file
/// holds is at the same corners in the page as the save leaves it. (Before a change the path stayed where it was and the entry was raised by the page's growth.)
fn collect_kept(diff: &PageDiff, held: &PageShapes, modelled: impl Fn(u32) -> bool) -> HashMap<u32, Vec<(f64, f64)>> {
    let changes = &diff.shapes;
    let routed: HashSet<u32> = changes.iter().filter(|c| c.route.is_some()).map(|c| c.id).collect();
    let mut out: HashMap<u32, Vec<(f64, f64)>> = HashMap::new();
    for id in held.kept_route_ids() {
        if !routed.contains(&id)
            && modelled(id)
            && let Some(corners) = held.kept_route(id)
        {
            out.insert(id, corners);
        }
    }
    for change in changes {
        if let Some(corners) = &change.file_route {
            out.insert(change.id, corners.clone());
        }
    }
    out
}

/// The page's index after the save: each shape of the model at the ordinal its element now has, the kept elements at theirs.
fn renumber(scan: &ScanPage, diff: &PageDiff, held: &PageShapes, children: &[Vec<u32>], gone: &[bool]) -> Result<(PageShapes, Vec<u32>), WriteError> {
    let mut new_ord: Vec<u32> = filled(children.len().max(scan.shapes.len() + 1), 0)?;
    let mut counter = 0u32;
    // Pre-order from the page: a stack of the children still to number, the last first.
    let mut stack: Vec<u32> = children.first().map(|c| c.iter().rev().copied().collect()).unwrap_or_default();
    while let Some(ord) = stack.pop() {
        counter = counter.checked_add(1).ok_or(WriteError::Limit { what: "shape elements on a page", max: u64::from(u32::MAX) })?;
        if let Some(slot) = new_ord.get_mut(ord as usize) {
            *slot = counter;
        }
        if let Some(list) = children.get(ord as usize) {
            stack.extend(list.iter().rev().copied());
        }
    }
    let mut ordinals: HashMap<u32, u32> =
        diff.survivors.iter().filter_map(|(id, ord)| new_ord.get(*ord as usize).copied().filter(|n| *n != 0).map(|n| (*id, n))).collect();
    // The new shapes, by the place numbers they were given after the page's elements.
    let pseudo = scan.shapes.len() + 1;
    let mut high_water = held.high_water();
    for (k, n) in diff.new.iter().enumerate() {
        if let Some(now) = new_ord.get(pseudo + k).copied().filter(|o| *o != 0) {
            ordinals.insert(n.shape.id, now);
        }
        high_water = high_water.max(n.shape.id);
    }
    let kept: Vec<u32> = {
        let mut kept: Vec<u32> = held
            .kept()
            .iter()
            .filter(|o| !gone.get(**o as usize).copied().unwrap_or(false))
            .filter_map(|o| new_ord.get(*o as usize).copied())
            .filter(|n| *n != 0)
            .collect();
        kept.sort_unstable();
        kept
    };
    Ok((PageShapes::from_parts(diff.source, ordinals, kept, counter, high_water), new_ord))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    use super::{collect_kept, kept_bytes, longest_run, z_values};
    use crate::index::{PageShapes, PageSource};
    use crate::write::compare::{FrameChange, PageDiff, ShapeDiff};
    use crate::write::route::Route;

    /// The review's worst input to the `draw:z-index` plan: a page of `n` shapes, each with its attribute, the first deleted (so the others'
    /// places shift) or the last put first, or the whole page reversed. Looking each shape up in a list of the others was quadratic and uncharged: at 100,000 shapes it was
    /// seconds. **Counted, never timed** (the test asserted two seconds and failed once at a load of 75): the plan's own count of its steps is linear, so a plan
    /// of twice the shapes takes at most 2.1 times the steps of the plan of the shapes, and each shape costs at most a bounded number. A quadratic plan fails both at once.
    fn steps_of_the_z_index_plan(n: u32) -> u64 {
        let before: Vec<u32> = (1..=n).collect();
        let z_was: HashMap<u32, i64> = before.iter().map(|o| (*o, i64::from(*o) * 2)).collect();
        let attr = z_was.clone();
        let survivors: Vec<u32> = (2..=n).collect();
        let reversed: Vec<u32> = survivors.iter().rev().copied().collect();
        let mut steps = 0u64;
        // The first deleted, nothing reordered: nothing to write.
        assert!(z_values(&before, &survivors, None, &z_was, &attr, &mut steps).unwrap().0.is_empty());
        // The last brought to the front of the drawing order: each shape takes the value of the place it has now.
        let mut front = vec![n];
        front.extend(2..n);
        let writes = z_values(&before, &survivors, Some(&front), &z_was, &attr, &mut steps).unwrap().0;
        assert_eq!(writes.len(), survivors.len(), "every shape takes the value of another place");
        // A whole page reversed: every shape's value changes.
        assert_eq!(
            z_values(&before, &survivors, Some(&reversed), &z_was, &attr, &mut steps).unwrap().0.len(),
            survivors.len() - usize::from(survivors.len() % 2 == 1)
        );
        steps
    }

    #[test]
    fn the_z_index_plan_of_a_big_page_is_linear_in_the_steps_it_counts() {
        let (small, big) = (steps_of_the_z_index_plan(100_000), steps_of_the_z_index_plan(200_000));
        assert!(big * 10 <= small * 21, "twice the shapes took {big} steps against {small}: more than 2.1 times");
        assert!(big >= small * 19 / 10, "twice the shapes took {big} steps against {small}: the plan does not look at every shape");
        // Each shape costs a bounded number of steps over the three plans: about forty here.
        assert!(small <= 60 * 100_000 && big <= 60 * 200_000, "{small} and {big} steps are more than sixty a shape");
        // And a single pass is counted: one shape, a count of its own.
        let mut one = 0u64;
        z_values(&[1], &[1], None, &HashMap::from([(1, 0)]), &HashMap::new(), &mut one).unwrap();
        assert!(one > 0 && one <= 60, "{one}");
    }

    /// The steps the plan counts are the ones its comment says, to the step, for two small plans worked out by hand: the survivors' two maps (2 each), the drawing order now (the
    /// model's, one a shape; or the baseline's, a lookup in two maps and the sort's pass, one a shape for each of the baseline's and two for each that remains), the values (two
    /// a survivor), three for each shape the values reach (its place, its attribute and the value put down), and three a survivor for the read at the end. They are added to
    /// what the counter held.
    #[test]
    fn the_z_index_plan_counts_each_pass_over_the_shapes_and_each_lookup() {
        let z_was: HashMap<u32, i64> = HashMap::from([(1, 0), (2, 1), (3, 2)]);
        // Three survivors in the order the model gives: 6 + 3 + 6 + 3 * 3 + 3 * 3.
        let mut steps = 0u64;
        z_values(&[1, 2, 3], &[1, 2, 3], Some(&[1, 3, 2]), &z_was, &HashMap::new(), &mut steps).unwrap();
        assert_eq!(steps, 6 + 3 + 6 + 9 + 9);
        // The second and fourth of five shapes deleted, the order the baseline's: 2 * 3 + 3 * 5 + 2 * 3 + 2 * 3 + 3 * 3 + 3 * 3, and the five the counter held. (The sizes are
        // such that a product is not a sum.)
        let five: HashMap<u32, i64> = HashMap::from([(1, 0), (2, 1), (3, 2), (4, 3), (5, 4)]);
        let mut steps = 5u64;
        z_values(&[1, 2, 3, 4, 5], &[1, 3, 5], None, &five, &HashMap::new(), &mut steps).unwrap();
        assert_eq!(steps, 5 + 6 + 15 + 6 + 6 + 9 + 9);
        // A plan that is refused has been counted all the same (the steps are the work, whether or not it was to the point).
        let repeated: HashMap<u32, i64> = HashMap::from([(1, 5), (2, 5), (3, 5)]);
        let mut steps = 0u64;
        assert!(z_values(&[1, 2, 3], &[1, 2, 3], Some(&[3, 2, 1]), &repeated, &repeated, &mut steps).is_err());
        assert_eq!(steps, 6 + 3 + 6 + 9 + 9);
    }

    /// The wall-clock check the counted test above replaced: at 100,000 and 200,000 shapes the plan takes a fraction of two seconds on an idle machine. Run by hand
    /// (`cargo test -- --ignored`); a machine under load may fail it, which is why it is no part of the suite.
    #[test]
    #[ignore = "a wall-clock bound: it fails on a loaded machine, and the counted test holds the plan to its steps"]
    fn the_z_index_plan_of_a_big_page_takes_less_than_two_seconds() {
        for n in [100_000u32, 200_000] {
            let started = Instant::now();
            steps_of_the_z_index_plan(n);
            assert!(started.elapsed() < Duration::from_secs(2), "{n} shapes took {:?}", started.elapsed());
        }
    }

    #[test]
    fn the_longest_run_is_what_need_not_move() {
        assert_eq!(longest_run(&[]), Vec::<usize>::new());
        assert_eq!(longest_run(&[0, 1, 2, 3]), [0, 1, 2, 3]);
        assert_eq!(longest_run(&[3, 0, 1, 2]), [0, 1, 2], "the first goes last: the others stay");
        assert_eq!(longest_run(&[1, 2, 3, 0]), [1, 2, 3]);
        assert_eq!(longest_run(&[1, 0]).len(), 1);
        assert_eq!(longest_run(&[2, 0, 3, 1, 4]).len(), 3);
        let up: Vec<usize> = (0..1000).collect();
        assert_eq!(longest_run(&up).len(), 1000);
        let down: Vec<usize> = (0..1000).rev().collect();
        assert_eq!(longest_run(&down).len(), 1);
        // A run is strictly increasing: a value that repeats is not a longer run.
        assert_eq!(longest_run(&[2, 2, 2]), [2]);
        assert_eq!(longest_run(&[0, 1, 1, 2]), [0, 1, 2]);
    }

    #[test]
    fn a_shape_without_an_attribute_is_written_unless_the_value_it_gets_is_the_place_it_is_read_at() {
        // Three shapes in a page that carries `draw:z-index` on none of the three here: the values are their places 0, 1, 2. The model puts the
        // third second. The first keeps its place and value (no write); the other two each take the value of the place the other had.
        let (before, survivors) = (vec![1, 2, 3], vec![1, 2, 3]);
        let z_was: HashMap<u32, i64> = HashMap::from([(1, 0), (2, 1), (3, 2)]);
        let writes = z_values(&before, &survivors, Some(&[1, 3, 2]), &z_was, &HashMap::new(), &mut 0).unwrap().0;
        assert_eq!(writes, [(3, 1), (2, 2)]);
        // One with an attribute: written when the value it now gets is another, and not when it is the same.
        let attr: HashMap<u32, i64> = HashMap::from([(2, 1)]);
        let writes = z_values(&before, &survivors, Some(&[1, 3, 2]), &z_was, &attr, &mut 0).unwrap().0;
        assert_eq!(writes, [(3, 1), (2, 2)], "shape 2 had 1 and gets 2");
        let same = z_values(&before, &survivors, Some(&[1, 2, 3]), &z_was, &HashMap::from([(1, 0), (2, 1), (3, 2)]), &mut 0).unwrap().0;
        assert!(same.is_empty(), "nothing moved, so nothing is written: {same:?}");
        // Shapes with the same value cannot be told apart by the reader once the order changes: refused.
        let repeated: HashMap<u32, i64> = HashMap::from([(1, 5), (2, 5), (3, 5)]);
        assert!(z_values(&before, &survivors, Some(&[3, 2, 1]), &repeated, &repeated, &mut 0).is_err());
        // A delete with no reorder: the survivors keep the order their values gave; the third, now second by place, is written with the value it
        // had (2), since the reader would read it at its place (1) otherwise, and the first keeps place and value.
        assert_eq!(z_values(&before, &[1, 3], None, &z_was, &HashMap::new(), &mut 0).unwrap().0, [(3, 2)]);
    }

    /// A change of connector `id` with a route, as `classify` makes it: its `route`, and the corners the file holds when the route is one the save does not write.
    fn routed(id: u32, route: Option<Route>, file_route: Option<Vec<(f64, f64)>>) -> ShapeDiff {
        ShapeDiff {
            ord: id,
            id,
            one_d: true,
            connector: true,
            group: false,
            frame: FrameChange::None,
            route,
            file_route,
            style: None,
            text: None,
            geo: None,
            tier2: None,
            turned_glue: None,
            layer: None,
        }
    }

    /// A page's diff with the changes `shapes` and a height `grew` inches more than the file's (the page is 10 inches high in the file).
    fn page_diff(shapes: Vec<ShapeDiff>, grew: f64) -> PageDiff {
        PageDiff {
            page_at: 0,
            held_at: Some(0),
            source: PageSource::Page(0),
            height: 10.0 + grew,
            height_was: 10.0,
            unit: crate::read::units::LenUnit::Cm,
            size: None,
            elements: 0,
            shapes,
            deleted: Vec::new(),
            levels: Vec::new(),
            unglue: Vec::new(),
            survivors: Vec::new(),
            new: Vec::new(),
            new_connects: Vec::new(),
            layers: Default::default(),
            unkept: Vec::new(),
            glued_to_groups: Vec::new(),
            one_flip: Default::default(),
            layer_flags: Vec::new(),
            comments: None,
            tied: Vec::new(),
        }
    }

    fn page_with(kept: &[(u32, Vec<(f64, f64)>)]) -> PageShapes {
        PageShapes::new(PageSource::Page(0)).with_kept_routes(kept.iter().cloned().collect())
    }

    /// The index's half: the corners of the route the file holds are kept for the connector a save kept (the hop said), carried across a save that
    /// does not route it again (a change of it with no route, or no change at all), replaced by a save that keeps it again, and dropped by a save that writes its route, or
    /// when the connector is gone.
    #[test]
    fn the_routes_the_file_holds_of_the_connectors_a_save_kept_are_carried_replaced_and_dropped() {
        let old: Vec<(f64, f64)> = vec![(1.0, 2.0), (3.0, 4.0)];
        let new: Vec<(f64, f64)> = vec![(5.0, 6.0), (7.0, 8.0), (9.0, 10.0)];
        let held = page_with(&[(1, old.clone()), (2, old.clone()), (3, old.clone()), (4, old.clone()), (5, old.clone())]);
        let path = Route::Path(vec![crate::write::route::Seg::Move((0.0, 0.0)), crate::write::route::Seg::Line((1.0, 0.0))]);
        let changes = vec![
            // Kept again, the file's corners are the ones the index held: the same.
            routed(1, Some(Route::Unreadable), Some(old.clone())),
            // Written: the file holds the model's route now, no entry.
            routed(2, Some(path), None),
            // Changed in another way, not routed: carried.
            routed(3, None, None),
            // A connector that had none, kept now: the baseline's corners.
            routed(6, Some(Route::Unreadable), Some(new.clone())),
        ];
        // Connector 4 is not in the model any more; 5 is, and no change touches it.
        let kept = collect_kept(&page_diff(changes, 0.0), &held, |id| id != 4);
        let mut ids: Vec<u32> = kept.keys().copied().collect();
        ids.sort_unstable();
        assert_eq!(ids, [1, 3, 5, 6], "2 was written, 4 is gone: {kept:?}");
        assert_eq!((kept.get(&1), kept.get(&3), kept.get(&5), kept.get(&6)), (Some(&old), Some(&old), Some(&old), Some(&new)));
        // A route the save kept replaces the entry the index held (the index's own is what `classify` gave it back as the file's, so a new value is a new fact).
        let again = collect_kept(&page_diff(vec![routed(1, Some(Route::Unreadable), Some(new.clone()))], 0.0), &held, |_| true);
        assert_eq!(again.get(&1), Some(&new));
        // Nothing held and nothing kept: nothing.
        assert!(collect_kept(&page_diff(Vec::new(), 0.0), &page_with(&[]), |_| true).is_empty());
    }

    /// The page's height (it was): an entry is carried **as it is** across a save of a page that grew or shrank, and one the save
    /// kept is the corners `classify` gave it, as given. The corners are measured from the page's bottom, and the save shifts the file's path by what the page grew, so the
    /// path the file holds is at the same corners in the page as the save leaves it (before a change the path stayed, and a carried entry was raised by the growth).
    #[test]
    fn a_carried_route_is_carried_as_it_is_across_a_page_that_grew_or_shrank() {
        let old: Vec<(f64, f64)> = vec![(1.0, 2.0), (3.0, 4.0)];
        let held = page_with(&[(1, old.clone()), (3, old.clone())]);
        let now = vec![(5.0, 6.0)];
        let changes = || vec![routed(1, Some(Route::Unreadable), Some(now.clone()))];
        for grew in [2.5, -1.0, 0.0] {
            let kept = collect_kept(&page_diff(changes(), grew), &held, |_| true);
            assert_eq!(kept.get(&3), Some(&old), "carried across a page that grew by {grew}: as it was");
            assert_eq!(kept.get(&1), Some(&now), "kept by this save: as it was given");
        }
    }

    /// What the routes kept cost the memory a save may hold: 64 bytes for each connector and 16 for each corner.
    #[test]
    fn the_routes_kept_are_charged_by_the_connector_and_the_corner() {
        let kept: HashMap<u32, Vec<(f64, f64)>> = HashMap::from([(1, vec![(0.0, 0.0); 3]), (2, vec![(0.0, 0.0); 5]), (3, Vec::new())]);
        assert_eq!(kept_bytes(&kept), 3 * 64 + 8 * 16);
        assert_eq!(kept_bytes(&HashMap::new()), 0);
    }
}
