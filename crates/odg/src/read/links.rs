//! Tier 3, a stale link to a master (the design's "A stale master link").
//!
//! An instance of a master holds only what differs from the master (design decision 7), and the rest is inherited: its geometry, its shape data (the `Prop` rows)
//! and its user rows (the `User` rows). When another program edits the outline the master gave the shape, **the link is stale** (`ext::tier3::judge` takes it
//! out: the shape's native outline is what it is now, and it is no longer the master's). The data was never in the markup, so a shape that only dropped the link
//! would lose it. **A stale link keeps the shape's own copies of the inherited shape data and user rows**: for each row of the master's counterpart of the shape
//! (`Prop` and `User`) that the shape does not hold itself, the shape gets a copy, and the shape's own rows stay as they are. Shape data and user cells are
//! never lost.
//!
//! **Where the master is.** The master's body is in DiaCraft's document file, which a LibreOffice save drops: then there is no master to copy
//! from and the shape keeps what it holds (the spec question of the report: where the copies come from after such a save).
//!
//! **The work.** One walk of each page that has a stale link (charged by the shapes it visits), the shapes found by an index of the links by `ID`, the masters by
//! an index made once, and each master's shapes by a map of their `ID`s made once for a master a member link asks about.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_doc::{Document, Master, Row, Section, Shape};

use super::Reader;
use crate::error::OdgError;
use crate::index::ShapeIndex;

use crate::ext::tier3::INHERITED_SECTIONS;
use crate::index::MasterLink;

/// The rows of the inherited sections of a master's shape, by the section's name.
type Rows = Vec<(&'static str, Vec<Arc<Row>>)>;

/// Gives each shape whose link to a master was stale and left out its own copies of the rows it inherited (see the module documentation).
pub(super) fn keep_copies(r: &mut Reader, doc: &mut Document, index: &ShapeIndex) -> Result<(), OdgError> {
    let mut masters: Option<HashMap<u32, usize>> = None;
    for at in 0..index.pages() {
        let Some(held) = index.page(at) else { continue };
        if held.stale_links().is_empty() {
            continue;
        }
        let mut links: HashMap<u32, MasterLink> = HashMap::new();
        r.take_items(held.stale_links().len())?;
        for (id, link) in held.stale_links() {
            links.entry(*id).or_insert(*link);
        }
        if masters.is_none() {
            r.take_items(doc.masters.len())?;
            masters = Some(doc.masters.iter().enumerate().map(|(i, m)| (m.id, i)).collect());
        }
        let Some(by_id) = masters.as_ref() else { continue };
        let Some(page) = doc.pages.get(at) else { continue };
        let found = find(r, &page.shapes, &links)?;
        if found.is_empty() {
            continue;
        }
        let mut copies: Vec<(Vec<usize>, Rows)> = Vec::new();
        let mut shape_maps: HashMap<u32, HashMap<u32, &Shape>> = HashMap::new();
        for hit in &found {
            let Some(&mi) = hit.master.and_then(|m| by_id.get(&m)) else {
                r.warn_counted("diacraft_stale_link_without_master", 1, || {
                    "a link to a master went stale and the file holds no body of that master, so there is nothing to keep copies of".to_owned()
                });
                continue;
            };
            let Some(master) = doc.masters.get(mi) else { continue };
            let counterpart = match hit.counterpart {
                Counterpart::Single => single_shape(r, master)?,
                Counterpart::Member(wanted) => {
                    let map = match shape_maps.entry(master.id) {
                        std::collections::hash_map::Entry::Occupied(held) => held.into_mut(),
                        std::collections::hash_map::Entry::Vacant(free) => free.insert(ids_of(r, master)?),
                    };
                    map.get(&wanted).copied()
                }
            };
            let Some(source) = counterpart else { continue };
            copies.push((hit.path.clone(), inherited_rows(r, source)?));
        }
        let Some(page) = doc.pages.get_mut(at) else { continue };
        let page = Arc::make_mut(page);
        for (path, rows) in copies {
            put_rows(r, &mut page.shapes, &path, rows)?;
        }
    }
    Ok(())
}

/// What a shape with a stale link is linked to in the master.
enum Counterpart {
    /// The master's one live top-level shape (the shape is an instance).
    Single,
    /// The master's shape with this `ID` (the shape is a member of an instance).
    Member(u32),
}

/// A shape with a stale link: where it is, the master it was an instance of or a member of, and the shape of the master it stands for.
struct Hit {
    path: Vec<usize>,
    master: Option<u32>,
    counterpart: Counterpart,
}

/// The shapes of a page whose `ID` is in `links`, with the path to each: one walk, the visits charged, a stack of its own.
fn find(r: &mut Reader, shapes: &diacraft_doc::ShapeList, links: &HashMap<u32, MasterLink>) -> Result<Vec<Hit>, OdgError> {
    let mut out: Vec<Hit> = Vec::new();
    // Each entry: the shape, its path, and the master of the nearest instance above it (a member's link is to that master).
    let mut stack: Vec<(&Shape, Vec<usize>, Option<u32>)> = Vec::new();
    for (i, s) in shapes.iter().enumerate().rev() {
        stack.push((s, vec![i], None));
    }
    while let Some((shape, path, above)) = stack.pop() {
        r.take_items(1)?;
        let link = links.get(&shape.id).copied();
        let own_master = link.and_then(|l| l.0).or(shape.master);
        if let Some((master, master_shape)) = link {
            match (master, master_shape) {
                (Some(m), _) => out.push(Hit { path: path.clone(), master: Some(m), counterpart: Counterpart::Single }),
                (None, Some(wanted)) => out.push(Hit { path: path.clone(), master: above, counterpart: Counterpart::Member(wanted) }),
                (None, None) => {}
            }
        }
        let below = own_master.or(above);
        for (i, child) in shape.children.iter().enumerate().rev() {
            if path.len() >= diacraft_doc::MAX_SHAPE_DEPTH {
                break;
            }
            let mut next = path.clone();
            next.push(i);
            stack.push((child, next, below));
        }
    }
    Ok(out)
}

/// The one top-level shape of a master that has exactly one that is not deleted.
fn single_shape<'m>(r: &mut Reader, master: &'m Master) -> Result<Option<&'m Shape>, OdgError> {
    r.take_items(master.shapes.len())?;
    let mut live = master.shapes.iter().filter(|s| !s.del);
    Ok(match (live.next(), live.next()) {
        (Some(only), None) => Some(&**only),
        _ => None,
    })
}

/// The shapes of a master by `ID`, members at any depth, the first of an `ID` winning: one walk, charged.
fn ids_of<'m>(r: &mut Reader, master: &'m Master) -> Result<HashMap<u32, &'m Shape>, OdgError> {
    let mut map: HashMap<u32, &Shape> = HashMap::new();
    let mut stack: Vec<(&Shape, usize)> = master.shapes.iter().rev().map(|s| (&**s, 1)).collect();
    while let Some((shape, depth)) = stack.pop() {
        r.take_items(1)?;
        map.entry(shape.id).or_insert(shape);
        if depth < diacraft_doc::MAX_SHAPE_DEPTH {
            stack.extend(shape.children.iter().rev().map(|c| (&**c, depth + 1)));
        }
    }
    Ok(map)
}

/// The rows of the inherited sections of `source`, by section: what the shape keeps copies of.
fn inherited_rows(r: &mut Reader, source: &Shape) -> Result<Rows, OdgError> {
    let mut out = Vec::new();
    for name in INHERITED_SECTIONS {
        let Some(section) = source.sheet.sections.iter().find(|s| s.name.as_str() == name) else { continue };
        r.take_items(section.rows.len())?;
        out.push((name, section.rows.iter().cloned().collect()));
    }
    Ok(out)
}

/// Puts the copies into the shape at `path`: a row the shape does not hold (by its name, or by its `IX` where it has none) is added to its section of the name,
/// which is made when the shape has none; the shape's own rows stay as they are.
fn put_rows(r: &mut Reader, shapes: &mut diacraft_doc::ShapeList, path: &[usize], rows: Rows) -> Result<(), OdgError> {
    let Some(shape) = shape_at(shapes, path) else { return Ok(()) };
    let sheet = Arc::make_mut(&mut shape.sheet);
    for (name, copies) in rows {
        let at = match sheet.sections.iter().position(|s| s.name.as_str() == name) {
            Some(at) => at,
            None => {
                r.take_items(1)?;
                if sheet.sections.len() >= diacraft_doc::MAX_SECTIONS_PER_SHEET {
                    return Err(OdgError::TooComplex { what: "sections in one sheet", max: diacraft_doc::MAX_SECTIONS_PER_SHEET as u64 });
                }
                sheet.sections.push(Arc::new(Section::new(name, None)));
                sheet.sections.len() - 1
            }
        };
        let Some(section) = sheet.sections.get_mut(at).map(Arc::make_mut) else { continue };
        r.take_items(section.rows.len().saturating_add(copies.len()))?;
        let held: std::collections::HashSet<(Option<std::sync::Arc<str>>, Option<u32>)> =
            section.rows.iter().map(|row| (row.name.clone(), row.ix)).collect();
        for row in copies {
            if !held.contains(&(row.name.clone(), row.ix)) {
                section.rows.push(row);
            }
        }
    }
    Ok(())
}

/// The shape at `path` in `shapes`, mutably.
fn shape_at<'s>(shapes: &'s mut diacraft_doc::ShapeList, path: &[usize]) -> Option<&'s mut Shape> {
    let (first, rest) = path.split_first()?;
    let mut shape = Arc::make_mut(shapes.get_mut(*first)?);
    for i in rest {
        shape = Arc::make_mut(shape.children.get_mut(*i)?);
    }
    Some(shape)
}

#[cfg(test)]
mod tests {
    use diacraft_doc::{CellName, Page, ShapeList, Value};

    use super::*;
    use crate::index::{PageShapes, PageSource, ReadCost};
    use crate::limits::Limits;

    fn row(name: &str, value: &str) -> Arc<Row> {
        let mut row = Row { name: Some(Arc::from(name)), ..Row::default() };
        row.cells.insert(CellName::new("Value"), diacraft_doc::Cell::constant(Value::from_text(value)));
        Arc::new(row)
    }

    /// A shape with a `Property` section of the rows `(name, value)`.
    fn shape(id: u32, rows: &[(&str, &str)]) -> Shape {
        let mut s = Shape::new(id);
        if !rows.is_empty() {
            let mut prop = Section::new("Property", None);
            for (name, value) in rows {
                prop.rows.push(row(name, value));
            }
            Arc::make_mut(&mut s.sheet).sections.push(Arc::new(prop));
        }
        s
    }

    fn master(id: u32, shapes: Vec<Shape>) -> Arc<Master> {
        Arc::new(Master { id, shapes: ShapeList::from_vec(shapes.into_iter().map(Arc::new).collect()), ..Master::default() })
    }

    fn page_of(shapes: Vec<Shape>) -> Arc<Page> {
        let mut page = Page::new(0, "p");
        page.shapes = ShapeList::from_vec(shapes.into_iter().map(Arc::new).collect());
        Arc::new(page)
    }

    /// An index whose page 0 says each `(id, link)` was found stale.
    fn index_of(stale: &[(u32, MasterLink)]) -> ShapeIndex {
        let mut page = PageShapes::new(PageSource::Page(0));
        for (id, link) in stale {
            page.note_stale(*id, Vec::new(), Some(*link));
        }
        ShapeIndex::from_pages(vec![page], ReadCost::default())
    }

    fn prop_names(shape: &Shape) -> Vec<String> {
        shape
            .sheet
            .sections
            .iter()
            .filter(|s| s.name.as_str() == "Property")
            .flat_map(|s| s.rows.iter().filter_map(|r| r.name.as_deref().map(str::to_owned)).collect::<Vec<_>>())
            .collect()
    }

    fn run(doc: &mut Document, index: &ShapeIndex) -> (Result<(), OdgError>, u64) {
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        let done = keep_copies(&mut r, doc, index);
        (done, r.report.work.items)
    }

    /// An instance gets the rows of the master's one live shape that it does not hold itself; its own rows stay and a row of its own name keeps its own value.
    #[test]
    fn an_instance_gets_the_rows_of_the_masters_one_live_shape_and_keeps_its_own() {
        let mut deleted = shape(2, &[("Gone", "x")]);
        deleted.del = true;
        let mut doc = Document::empty();
        doc.masters.push(master(7, vec![shape(1, &[("A", "master a"), ("B", "master b")]), deleted]));
        doc.pages.push(page_of(vec![shape(10, &[("B", "mine")])]));
        let (done, items) = run(&mut doc, &index_of(&[(10, (Some(7), None))]));
        assert!(done.is_ok());
        assert!(items > 0, "the work is counted");
        let s = doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
        assert_eq!(prop_names(&s), ["B", "A"], "its own row stays first, the master's other row is added, the deleted shape gives none");
        let b = s.sheet.sections[0].rows.iter().find(|r| r.name.as_deref() == Some("B")).unwrap();
        assert_eq!(b.cells.get("Value").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()).as_deref(), Some("mine"));
    }

    /// A master of two live shapes has no one shape an instance stands for, and a master the document lacks has none: nothing is copied (and the second is said).
    #[test]
    fn a_master_with_two_live_shapes_or_none_in_the_document_gives_no_copies() {
        let mut doc = Document::empty();
        doc.masters.push(master(7, vec![shape(1, &[("A", "a")]), shape(2, &[("B", "b")])]));
        doc.pages.push(page_of(vec![shape(10, &[]), shape(11, &[])]));
        let before = doc.pages.get(0).unwrap().clone();
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        keep_copies(&mut r, &mut doc, &index_of(&[(10, (Some(7), None)), (11, (Some(99), None))])).unwrap();
        assert_eq!(*doc.pages.get(0).unwrap(), before);
        assert_eq!(r.report.warning_counts.get("diacraft_stale_link_without_master"), Some(&1));
    }

    /// A member of an instance gets the rows of the master's shape that has the `ID` it stood for, at any depth of the master, the first of an `ID` winning; the master
    /// is the one of the nearest ancestor that holds `Master`.
    #[test]
    fn a_member_gets_the_rows_of_the_masters_shape_it_stood_for() {
        let mut inner = shape(21, &[("Deep", "d")]);
        inner.children = ShapeList::from_vec(vec![Arc::new(shape(22, &[("Deeper", "dd")]))]);
        let mut top = shape(20, &[("Top", "t")]);
        top.children = ShapeList::from_vec(vec![Arc::new(inner), Arc::new(shape(21, &[("Second", "s")]))]);
        let mut doc = Document::empty();
        doc.masters.push(master(7, vec![top, shape(30, &[("Sibling", "s")])]));
        // The instance (a group that holds `Master`) with two members and a member's member.
        let mut member_of_member = shape(6, &[]);
        let mut member = shape(5, &[]);
        member_of_member.children = ShapeList::new();
        member.children = ShapeList::from_vec(vec![Arc::new(member_of_member)]);
        let mut instance = shape(1, &[]);
        instance.master = Some(7);
        instance.children = ShapeList::from_vec(vec![Arc::new(member), Arc::new(shape(8, &[]))]);
        doc.pages.push(page_of(vec![instance]));
        let index = index_of(&[(5, (None, Some(21))), (6, (None, Some(22))), (8, (None, Some(30))), (1, (None, None))]);
        let (done, _) = run(&mut doc, &index);
        assert!(done.is_ok());
        let group = doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
        let m5 = group.children.get(0).unwrap().clone();
        assert_eq!(prop_names(&m5), ["Deep"], "the first shape of ID 21, at depth 2 of the master");
        assert_eq!(prop_names(m5.children.get(0).unwrap()), ["Deeper"], "a member of a member, its master the nearest ancestor's");
        assert_eq!(prop_names(group.children.get(1).unwrap()), ["Sibling"]);
        assert!(prop_names(&group).is_empty(), "a link of neither number gives nothing");
    }

    /// A member with no ancestor that holds `Master` has no master to ask, a link to a shape the master lacks gives nothing, and the walk stops at the depth a model keeps.
    #[test]
    fn a_member_with_no_master_above_it_or_a_shape_the_master_lacks_or_a_tree_too_deep_gives_nothing() {
        let mut doc = Document::empty();
        doc.masters.push(master(7, vec![shape(1, &[("A", "a")])]));
        let mut group = shape(2, &[]);
        group.children = ShapeList::from_vec(vec![Arc::new(shape(3, &[]))]);
        let mut instance = shape(4, &[]);
        instance.master = Some(7);
        instance.children = ShapeList::from_vec(vec![Arc::new(shape(5, &[]))]);
        doc.pages.push(page_of(vec![group, instance]));
        let before = doc.pages.get(0).unwrap().clone();
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        keep_copies(&mut r, &mut doc, &index_of(&[(3, (None, Some(1))), (5, (None, Some(404)))])).unwrap();
        assert_eq!(*doc.pages.get(0).unwrap(), before);
        assert_eq!(r.report.warning_counts.get("diacraft_stale_link_without_master"), Some(&1), "the member that has no master above it is said");
        // Nested to the depth a model keeps, and one past it: the walk of the page stops, the deepest shape is not found.
        let depth = diacraft_doc::MAX_SHAPE_DEPTH;
        let mut deepest = shape(1000, &[]);
        for k in 0..depth {
            let mut outer = shape(2000 + k as u32, &[]);
            outer.children = ShapeList::from_vec(vec![Arc::new(deepest)]);
            deepest = outer;
        }
        let mut doc = Document::empty();
        doc.masters.push(master(7, vec![shape(1, &[("A", "a")])]));
        doc.pages.push(page_of(vec![deepest]));
        // The wrapper at the depth the model keeps (id 2000) is reached, and the shape inside it, one past, is not.
        let (done, _) = run(&mut doc, &index_of(&[(1000, (Some(7), None)), (2000, (Some(7), None))]));
        assert!(done.is_ok(), "a tree that deep ends the walk and is no error");
        let mut found = doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
        let mut levels = 1;
        while let Some(child) = found.children.get(0).cloned() {
            if child.id == 1000 || levels + 1 == depth {
                break;
            }
            found = child;
            levels += 1;
        }
        let at_the_depth = found.children.get(0).cloned().unwrap_or_else(|| found.clone());
        assert_eq!(at_the_depth.id, 2000, "the wrapper at the depth the model keeps");
        assert_eq!(prop_names(&at_the_depth), ["A"], "it is reached");
        let past = at_the_depth.children.get(0).unwrap().clone();
        assert_eq!(past.id, 1000);
        assert!(prop_names(&past).is_empty(), "the shape past the depth is not reached");
    }

    /// The master's shapes are mapped to the depth a model keeps: a shape at that depth is found, and one past it is not.
    #[test]
    fn the_shapes_of_a_master_are_mapped_to_the_depth_a_model_keeps() {
        let depth = diacraft_doc::MAX_SHAPE_DEPTH;
        // A chain of `depth + 1` shapes whose ids are 1 (the top) to `depth + 1`; the one at level `depth` has a row, and so has the one past it.
        let mut chain = shape(depth as u32 + 1, &[("Past", "p")]);
        for level in (1..=depth as u32).rev() {
            let mut outer = shape(level, if level == depth as u32 { &[("At", "a")] } else { &[] });
            outer.children = ShapeList::from_vec(vec![Arc::new(chain)]);
            chain = outer;
        }
        let m = master(7, vec![chain]);
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        let map = ids_of(&mut r, &m).unwrap();
        assert!(map.contains_key(&(depth as u32)), "the shape at the depth is mapped");
        assert!(!map.contains_key(&(depth as u32 + 1)), "and the one past it is not");
        assert_eq!(map.len(), depth);
        assert_eq!(r.report.work.items, depth as u64, "an item for each shape visited");
    }

    /// The master's shapes by `ID` are mapped once for each master however many members ask, and every shape visited is an item: the work of many members is the master's
    /// shapes once and the members' own.
    #[test]
    fn the_shapes_of_a_master_are_mapped_once_and_charged_by_the_shapes_visited() {
        let master_shapes: Vec<Shape> = (0..50u32).map(|k| shape(100 + k, &[("R", "r")])).collect();
        let members = |n: u32| -> (Document, ShapeIndex) {
            let mut doc = Document::empty();
            doc.masters.push(master(7, master_shapes.clone()));
            let mut instance = shape(1, &[]);
            instance.master = Some(7);
            instance.children = ShapeList::from_vec((0..n).map(|k| Arc::new(shape(10 + k, &[]))).collect());
            doc.pages.push(page_of(vec![instance]));
            let stale: Vec<(u32, MasterLink)> = (0..n).map(|k| (10 + k, (None, Some(100 + k % 50)))).collect();
            (doc, index_of(&stale))
        };
        let cost = |n: u32| {
            let (mut doc, index) = members(n);
            let (done, items) = run(&mut doc, &index);
            assert!(done.is_ok());
            let group = doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
            assert!(group.children.iter().all(|m| prop_names(m) == ["R"]), "every member has its copy");
            items
        };
        let (a, b, c) = (cost(50), cost(100), cost(200));
        // The extra members cost their own visits and rows (a linear step), not a walk of the master for each: 50 more members cost less than the 50 shapes of the master
        // and the rows twice over.
        assert!(b - a < 400 && c - b < 800, "{a} {b} {c}");
        assert!(b - a > 50, "the members are charged: {a} {b}");
        assert!(a >= 50 + 50, "the master's 50 shapes are walked, and the page's: {a}");
    }

    /// **Of the review.** The walk of a page for the shapes that went stale is one visit for each shape of the page, charged: the plain shapes beside the stale one cost
    /// one item each.
    #[test]
    fn the_walk_of_a_page_for_the_stale_shapes_is_charged_by_the_shapes_it_visits() {
        let cost = |plain: u32| {
            let mut doc = Document::empty();
            doc.masters.push(master(7, vec![shape(1, &[("A", "a")])]));
            let mut shapes = vec![shape(10, &[])];
            shapes.extend((0..plain).map(|k| shape(1000 + k, &[])));
            doc.pages.push(page_of(shapes));
            let (done, items) = run(&mut doc, &index_of(&[(10, (Some(7), None))]));
            assert!(done.is_ok());
            assert_eq!(prop_names(doc.pages.get(0).unwrap().shapes.get(0).unwrap()), ["A"]);
            items
        };
        assert_eq!(cost(110) - cost(10), 100, "one item for each shape of the page that is only looked at");
    }

    /// **Of the review.** The copies are put into a section the shape does not hold only while the shape has room for one more section; past the cap of a sheet the
    /// read says so and stops (a model that holds more sections than a sheet can is no model), and with room for one it is made.
    #[test]
    fn a_section_the_shape_lacks_is_made_only_while_the_sheet_has_room_for_it() {
        let with_sections = |n: usize| {
            let mut doc = Document::empty();
            doc.masters.push(master(7, vec![shape(1, &[("A", "a")])]));
            let mut stale = shape(10, &[]);
            let sheet = Arc::make_mut(&mut stale.sheet);
            for k in 0..n {
                sheet.sections.push(Arc::new(Section::new(&format!("S{k}"), None)));
            }
            doc.pages.push(page_of(vec![stale]));
            let (done, _) = run(&mut doc, &index_of(&[(10, (Some(7), None))]));
            (done, doc)
        };
        let most = diacraft_doc::MAX_SECTIONS_PER_SHEET;
        let (done, doc) = with_sections(most - 1);
        assert!(done.is_ok(), "room for one more");
        let held = doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
        assert_eq!(held.sheet.sections.len(), most);
        assert_eq!(prop_names(&held), ["A"]);
        let (done, _) = with_sections(most);
        assert!(matches!(done, Err(OdgError::TooComplex { max, .. }) if max == most as u64), "{done:?}");
    }
}
