//! The `ID`s of the style sheets that the document file restores.
//!
//! An OpenDocument file numbers its common styles by their places, and the reader gives the style at place *p* the `ID` *p*, and gives every link it makes from the markup
//! (a shape's parent, a style's parent) as that number. A VSDX file's style sheets are numbered by `ID`s with gaps (0, 1, 2, 6, 7, 30), and the model writer writes them in the model's order:
//! **the `style` entry of a style sheet whose `ID` is not its place says its `ID` in `id`**, and this step gives it back, before the entries are read over the sheets, so that every number of the
//! model is an `ID` from then on: the style sheets' own, their links (the markup's parents), and the links of the shapes (a link an entry gives, `line-style` and its two, is an `ID` already).
//!
//! **What the reader does with an `id`.**
//! * The entry names its style as every entry does: `root="1"` for the root, which keeps its `ID` 0, else `name`, by the name the file holds the style under. An entry of no style is the
//!   entry loop's warning and nothing here.
//! * The first claim of an `ID` stands: an `ID` another style sheet holds, or one that is no number (`canonical_u32`), is ignored with a warning, and the style is numbered as one that
//!   claimed none.
//! * **A style that claimed none** (a style another program added, one whose entry was left out) keeps the number of its place when no style holds it, and otherwise the next number past all
//!   that are held and all the places, so that every style sheet holds an `ID` of its own however the file was edited.
//!
//! **Cost.** Nothing is charged for a document file that has no `id` (the common case: a drawing numbered by its places). Otherwise each entry with an `id` is an item, the index of the style
//! sheets by name is made once (charged by the styles, as the entry loop charges the index it makes), and the walk of the shapes that maps their links is an item for each shape visited, depth
//! bounded by the model's ([`diacraft_doc::MAX_SHAPE_DEPTH`]), made only when some style sheet holds an `ID` that is not its place.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{Document, Links, MAX_SHAPE_DEPTH, ShapeList};
use diacraft_sheetxml::passthrough::canonical_u32;

use crate::error::OdgError;
use crate::ext::body::Sc;
use crate::read::Reader;

/// Gives each style sheet of `doc` the `ID` the document file `root` says (see the module documentation) and puts the links the markup gave through the same map. `remap` is false for the
/// document the writer reads a file it is about to write over (it holds the `ID`s already: nothing is mapped, and the same is charged). Gives the index of the styles by the name the file
/// holds them under when it was made (the entry loop uses it, and does not make it again), and `None` when no entry says an `ID`.
pub(super) fn restore(r: &mut Reader<'_>, doc: &mut Document, root: &El, remap: bool) -> Result<Option<HashMap<String, usize>>, OdgError> {
    let sc = Sc::empty().enter(root);
    let entries: Vec<&El> = root
        .children
        .iter()
        .filter_map(|n| match n {
            Node::El(child) if sc.enter(child).is(child, "style") && child.attr("id").is_some() => Some(child),
            _ => None,
        })
        .collect();
    if entries.is_empty() {
        return Ok(None);
    }
    r.take_items(entries.len())?;
    // The styles by the name the file holds them under, as the entry loop indexes them: made once, charged by the styles.
    r.take_items(doc.styles.len())?;
    let mut by_name: HashMap<String, usize> = HashMap::new();
    for (at, s) in doc.styles.iter().enumerate().skip(1) {
        if let Some(name) = s.name_u.as_deref() {
            by_name.entry(name.to_owned()).or_insert(at);
        }
    }
    // The claims, first come first served: the root holds 0.
    let mut claimed: HashSet<u32> = HashSet::from([0]);
    let mut wanted: HashMap<usize, u32> = HashMap::new();
    let mut ignored = 0u64;
    for entry in entries {
        let Some(at) = entry.attr("name").and_then(|name| by_name.get(name)).copied().filter(|_| entry.attr("root").is_none()) else { continue };
        let Some(id) = entry.attr("id").and_then(canonical_u32) else {
            ignored = ignored.saturating_add(1);
            continue;
        };
        if wanted.contains_key(&at) || !claimed.insert(id) {
            ignored = ignored.saturating_add(1);
            continue;
        }
        wanted.insert(at, id);
    }
    if ignored > 0 {
        r.warn_counted("diacraft_style_id_ignored", ignored, || {
            "DiaCraft's document file gives a style sheet an ID that is no number, or that another style sheet holds: it is ignored".to_owned()
        });
    }
    // The numbers: a style that claimed none keeps its place's number when it is free, else the next one past everything.
    let count = doc.styles.len();
    let ids = number(count, &wanted, &claimed).ok_or(OdgError::Limit { kind: crate::error::LimitKind::Styles, max: count as u64 })?;
    let moved = ids.iter().enumerate().any(|(at, id)| u32::try_from(at).ok() != Some(*id));
    if remap && moved {
        for (at, id) in ids.iter().enumerate() {
            if let Some(slot) = doc.styles.get_mut(at)
                && slot.id != *id
            {
                Arc::make_mut(slot).id = *id;
            }
        }
        for at in 0..count {
            let Some(slot) = doc.styles.get_mut(at) else { continue };
            let mapped = map_links(slot.sheet.links, &ids);
            if mapped != slot.sheet.links {
                Arc::make_mut(&mut Arc::make_mut(slot).sheet).links = mapped;
            }
        }
        r.take_items(doc.pages.len())?;
        for at in 0..doc.pages.len() {
            let Some(slot) = doc.pages.get_mut(at) else { continue };
            // A page that holds no shape is not unshared.
            if slot.shapes.is_empty() {
                continue;
            }
            remap_shapes(r, &mut Arc::make_mut(slot).shapes, &ids, 1)?;
        }
    }
    Ok(Some(by_name))
}

/// The `ID` of each of `count` style sheets, by place: the root's is 0, a style that claimed one (`wanted`, by place; the claims are distinct and `claimed` holds them and the root's 0) has it, and a style that
/// claimed none keeps the number of its place when no claim holds it and otherwise takes the next number past every claim and every place (and, when a claim of the greatest numbers leaves none
/// past it, the least number from the count of the style sheets up that nothing holds), so that **no two style sheets hold one `ID`**. `None` when the list cannot be allocated or no number is left. Integer
/// code, bounded by `count`.
fn number(count: usize, wanted: &HashMap<usize, u32>, claimed: &HashSet<u32>) -> Option<Vec<u32>> {
    let from = u32::try_from(count).unwrap_or(u32::MAX);
    // The numbers handed out past the claims, and the least number a style may be given when those run out.
    let mut up = claimed.iter().copied().max().map_or(Some(0), |m| m.checked_add(1)).map(|m| m.max(from));
    let mut low = from;
    let mut given: HashSet<u32> = HashSet::new();
    let mut ids: Vec<u32> = Vec::new();
    ids.try_reserve_exact(count).ok()?;
    for at in 0..count {
        let place = u32::try_from(at).unwrap_or(u32::MAX);
        let id = match wanted.get(&at) {
            Some(id) => *id,
            None if at == 0 || !claimed.contains(&place) => place,
            None => loop {
                let candidate = match up {
                    Some(n) => {
                        up = n.checked_add(1);
                        n
                    }
                    None => {
                        let n = low;
                        low = low.checked_add(1)?;
                        n
                    }
                };
                if !claimed.contains(&candidate) && given.insert(candidate) {
                    break candidate;
                }
            },
        };
        ids.push(id);
    }
    Some(ids)
}

/// `links` with each number a place of the file put through `ids` (the `ID` of the style at that place): a number that is no place is left as it is.
pub(crate) fn map_links(links: Links, ids: &[u32]) -> Links {
    let map = |link: Option<u32>| link.map(|place| usize::try_from(place).ok().and_then(|p| ids.get(p).copied()).unwrap_or(place));
    Links { line: map(links.line), fill: map(links.fill), text: map(links.text) }
}

/// Puts the links of the shapes of `list`, and of their members, through `ids`: one visit for each shape, charged, down to the depth a model keeps. A shape is unshared only when its links
/// change or it holds members to visit.
fn remap_shapes(r: &mut Reader<'_>, list: &mut ShapeList, ids: &[u32], depth: usize) -> Result<(), OdgError> {
    for at in 0..list.len() {
        r.take_items(1)?;
        let Some(slot) = list.get_mut(at) else { continue };
        let mapped = map_links(slot.sheet.links, ids);
        let descend = !slot.children.is_empty() && depth < MAX_SHAPE_DEPTH;
        if mapped == slot.sheet.links && !descend {
            continue;
        }
        let shape = Arc::make_mut(slot);
        if mapped != shape.sheet.links {
            Arc::make_mut(&mut shape.sheet).links = mapped;
        }
        if descend {
            remap_shapes(r, &mut shape.children, ids, depth.saturating_add(1))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(list: &[(usize, u32)]) -> (HashMap<usize, u32>, HashSet<u32>) {
        let mut claimed: HashSet<u32> = HashSet::from([0]);
        let wanted = list.iter().copied().filter(|(at, id)| *at != 0 && claimed.insert(*id)).collect();
        (wanted, claimed)
    }

    /// A style that claims none keeps its place's number when it is free, one that claims takes its claim, and the root is 0: the task's numbering 0, 1, 2, 6, 7, 30 as entries say it.
    #[test]
    fn a_style_keeps_its_places_number_or_takes_its_claim() {
        let (wanted, claimed) = claims(&[(3, 6), (4, 7), (5, 30)]);
        assert_eq!(number(6, &wanted, &claimed), Some(vec![0, 1, 2, 6, 7, 30]));
        let (wanted, claimed) = claims(&[]);
        assert_eq!(number(4, &wanted, &claimed), Some(vec![0, 1, 2, 3]));
        assert_eq!(number(0, &wanted, &claimed), Some(Vec::new()));
    }

    /// A style that claims none and whose place another style holds is numbered past every claim and every place, in the order of the places.
    #[test]
    fn a_style_whose_place_is_held_takes_the_next_number_past_everything() {
        let (wanted, claimed) = claims(&[(1, 5), (2, 4)]);
        // Places 0 to 5: 0, 5, 4, then place 3 (free), place 4 (held by a claim) and place 5 (held by a claim): 6 and 7 (past the claim 5 and the 6 places).
        assert_eq!(number(6, &wanted, &claimed), Some(vec![0, 5, 4, 3, 6, 7]));
        // The claim 4,000,000,000 and many unclaimed places that it does not hold: nothing is counted to it.
        let (wanted, claimed) = claims(&[(1, 4_000_000_000)]);
        assert_eq!(number(3, &wanted, &claimed), Some(vec![0, 4_000_000_000, 2]));
        // A claim of the greatest number leaves none past it: the styles that need one take the least numbers from the count of style sheets up that no claim holds.
        let (wanted, claimed) = claims(&[(1, u32::MAX), (4, 2), (5, 3)]);
        assert_eq!(number(6, &wanted, &claimed), Some(vec![0, u32::MAX, 6, 7, 2, 3]), "places 2 and 3 are held by the claims of two others");
    }

    /// Whatever is claimed (a sweep of claims over six places, some of them held twice or naming the root's 0), no two style sheets hold one `ID` and the root holds 0.
    #[test]
    fn no_two_styles_hold_one_id_whatever_is_claimed() {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..2_000 {
            let count = 1 + usize::try_from(next() % 12).unwrap_or(0);
            let list: Vec<(usize, u32)> = (0..usize::try_from(next() % 10).unwrap_or(0))
                .map(|_| (usize::try_from(next() % 14).unwrap_or(0) % count, u32::try_from(next() % 20).unwrap_or(0)))
                .collect();
            // A claim for a place that holds one already is the entry loop's to ignore: the first stands.
            let mut seen: HashSet<usize> = HashSet::new();
            let list: Vec<(usize, u32)> = list.into_iter().filter(|(at, _)| seen.insert(*at)).collect();
            let (wanted, claimed) = claims(&list);
            let ids = number(count, &wanted, &claimed).unwrap();
            let held: HashSet<u32> = ids.iter().copied().collect();
            assert_eq!(held.len(), ids.len(), "{count} styles, claims {list:?}: {ids:?}");
            assert_eq!(ids.first(), Some(&0), "{ids:?}");
        }
    }

    /// A link that names a place of the file names the `ID` of the style there, a link that names none is left as it is, and no link stays no link.
    #[test]
    fn a_link_is_put_through_the_map_and_a_number_that_is_no_place_is_left() {
        let ids = [0, 1, 2, 6, 7, 30];
        let links = Links { line: Some(3), fill: Some(5), text: None };
        assert_eq!(map_links(links, &ids), Links { line: Some(6), fill: Some(30), text: None });
        let outside = Links { line: Some(6), fill: Some(u32::MAX), text: Some(0) };
        assert_eq!(map_links(outside, &ids), outside, "6 is no place of six style sheets, and nor is the greatest number; the root stays 0");
        assert_eq!(map_links(Links::default(), &ids), Links::default());
        assert_eq!(map_links(links, &[]), links);
    }
}
