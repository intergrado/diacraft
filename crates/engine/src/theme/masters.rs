//! **A theme reaches an instance through its master** (the owner's ruling of 2026-11-27, "Link masters to the theme").
//!
//! The VSDX application reads an instance's own `LineStyle`, `FillStyle` and `TextStyle` before its master's cells of that family (observed on the project's
//! test PC): an instance linked to the `Theme` style itself read the style's `THEMEVAL()` for its line ends, pattern and fill, a solid line with no ends, whatever its master held.
//! The files it writes link their masters' shapes to the `Theme` style and leave the instances with none (links sit on master shapes, instances usually
//! carry none; most `Themed` values an instance inherits sit over a master's cell). So a shape that stands for a shape of a master (an instance, or a member of one) follows
//! the theme by **its master's shape** following the `Theme` style for the family, and keeps no link of its own; a shape with no master links the style itself, as before.
//!
//! **One master serves every page.** The style's cells are worked out on the instance, and their scheme cells hold 65534, which reads the instance's own page
//! So two pages of two themes or variants share the master; a page that selects no theme reads the root style's cells
//! through the same style, which is what an instance of a master on the root drew. **No master is copied.**
//!
//! **What would change a shape the pass does not theme** keeps the master as it is: a shape that reads the family through the master's shape (it has no link of its own for it)
//! and is not one the pass themes, on a page that selects a theme or under a sheet that selects one of its own (a shape on a locked layer; a shape of
//! another page that does not follow its page's theme, a paste from a page with none, say). The shapes the pass themes then link the style themselves, as before, so nothing
//! draws otherwise than it did. [`can_follow`] finds those shapes in one walk of the drawing, made only when a master's shape is still to be linked.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{Document, Links, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE, RawAttrs, Shape, ShapeList, Sheet};

use crate::draw::Problem;

/// The scheme cells a sheet selects a theme by: the theme, the colour, effect, connector and font schemes.
const SCHEME_CELLS: [&str; 5] = ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"];

/// The attributes of a sheet's element that hold its style links: a link written must not leave the text of the old one behind (raw wins on a write).
pub(super) const LINK_ATTRS: [&str; 3] = ["LineStyle", "FillStyle", "TextStyle"];

/// The most shapes of the drawing one look for the other shapes of a master visits ([`can_follow`]): past it no master's shape is linked by the pass, and the shapes it themes link
/// the style themselves, as before.
pub(super) const MAX_CENSUS_SHAPES: usize = 4_000_000;

/// A shape of a master: the master's `ID` and the place of the shape among the master's shapes (a path of places, as a page's is).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) struct MasterShape {
    pub master: u32,
    pub path: Vec<u32>,
}

/// For each family (line, fill, text), whether a master's shape follows the `Theme` style for it: what [`can_follow`] gives and [`link`] writes.
pub(super) type Families = BTreeMap<MasterShape, [bool; 3]>;

/// The link of a family: 0 line, 1 fill, 2 text.
pub(super) fn family(links: Links, f: usize) -> Option<u32> {
    match f {
        0 => links.line,
        1 => links.fill,
        _ => links.text,
    }
}

/// The link of a family, to write.
pub(super) fn family_mut(links: &mut Links, f: usize) -> &mut Option<u32> {
    match f {
        0 => &mut links.line,
        1 => &mut links.fill,
        _ => &mut links.text,
    }
}

/// Whether a shape was read from an `.odg`: it holds the data of an element of one. **Such a shape follows the theme by links of its own**, as every shape did before: an `.odg` holds
/// no master another program reads, the shape's automatic style names the style it follows as its parent, and a save into the file writes that parent from the
/// shape's own links. A save to VSDX takes off the copy a link that is its master's (`across/onto.rs`).
pub(super) fn from_odg(shape: &Shape) -> bool {
    shape.ext.get().is_some_and(|data| data.odf != diacraft_doc::OdfShape::default())
}

/// Takes the link attributes a sheet keeps as text out of it, so that the links the model holds are the ones written.
pub(super) fn drop_raw_links(sheet: &mut Sheet) {
    if sheet.raw.iter().any(|(name, _)| LINK_ATTRS.contains(&name)) {
        sheet.raw = RawAttrs::from_pairs(sheet.raw.iter().filter(|(name, _)| !LINK_ATTRS.contains(name)));
    }
}

/// The places and links of the shapes of one master: what an instance, or a member of one, stands for.
#[derive(Default)]
struct MasterIndex {
    /// The first top-level shape that is not deleted: what an instance of the master stands for.
    first: Option<(Vec<u32>, Links)>,
    /// Every shape by `ID` (the first of an `ID` met): what a member names by `MasterShape`.
    by_id: HashMap<u32, (Vec<u32>, Links)>,
}

impl MasterIndex {
    /// One pass over the master's shapes with a stack, no deeper than the model allows (a master past the cap of shapes is indexed as far as the cap).
    fn of(doc: &Document, id: u32) -> MasterIndex {
        let mut index = MasterIndex::default();
        let Some(master) = doc.master(id) else { return index };
        index.first = master.shapes.iter().enumerate().find(|(_, s)| !s.del).and_then(|(k, s)| Some((vec![u32::try_from(k).ok()?], s.sheet.links)));
        let mut seen = 0_usize;
        let mut stack: Vec<(&ShapeList, Vec<u32>)> = vec![(&master.shapes, Vec::new())];
        while let Some((list, prefix)) = stack.pop() {
            for (k, shape) in list.iter().enumerate() {
                seen = seen.saturating_add(1);
                if seen > MAX_SHAPES_PER_PAGE {
                    return index;
                }
                let Ok(k) = u32::try_from(k) else { return index };
                let mut path = prefix.clone();
                path.push(k);
                if !shape.children.is_empty() && path.len() < MAX_SHAPE_DEPTH {
                    stack.push((&shape.children, path.clone()));
                }
                index.by_id.entry(shape.id).or_insert((path, shape.sheet.links));
            }
        }
        index
    }
}

/// The masters' indexes a pass reads, each made when first needed: a master is indexed once however many shapes stand for its shapes.
#[derive(Default)]
pub(super) struct Masters(HashMap<u32, MasterIndex>);

impl Masters {
    /// The shape of a master that `shape` stands for, and that shape's links: the master's first top-level shape for an instance, the shape `MasterShape` names for a member of one
    /// `above` is the master of the nearest ancestor that has one. `None` for a shape that stands for none, or names a shape the master does not hold.
    pub(super) fn stands_for(&mut self, doc: &Document, shape: &Shape, above: Option<u32>) -> Option<(MasterShape, Links)> {
        let (m, id) = match (shape.master, shape.master_shape, above) {
            (Some(m), _, _) => (m, None),
            (None, Some(id), Some(m)) => (m, Some(id)),
            _ => return None,
        };
        let index = self.0.entry(m).or_insert_with(|| MasterIndex::of(doc, m));
        let (path, links) = match id {
            None => index.first.as_ref()?,
            Some(id) => index.by_id.get(&id)?,
        };
        Some((MasterShape { master: m, path: path.clone() }, *links))
    }
}

/// Whether a cell of a sheet's own holds what selects a theme: a value that is not the number 0 (no theme) and not 65534 (the page's, or the group's). A formula's text, an error or a
/// text counts as one: the look is to err toward keeping a master as it is.
fn selects(sheet: &Sheet) -> bool {
    SCHEME_CELLS.iter().any(|name| {
        sheet.cell(name).is_some_and(|cell| match cell.value.as_ref().and_then(diacraft_doc::Value::number) {
            Some(n) => n != 0.0 && n != 65_534.0,
            None => cell.value.is_some(),
        })
    })
}

/// Whether a page selects a theme: any of its scheme cells holds a value that is not the number 0 (65534 on a page is no number a page should hold, and counts).
fn page_selects(doc: &Document, page: u32) -> bool {
    doc.page(page).is_some_and(|p| {
        SCHEME_CELLS.iter().any(|name| p.sheet.cell(name).is_some_and(|cell| cell.value.as_ref().and_then(diacraft_doc::Value::number) != Some(0.0)))
    })
}

/// What a look at the other shapes of the masters is given: the master's shapes and families a pass would have follow the `Theme` style (`wanted`), the pages whose every shape the
/// pass planned (`covered`: their shapes that read through a master and are not themed are the pass's `held`, the shapes on locked layers), and the shapes the pass themes on the
/// other pages (`themed`, by page and place).
pub(super) struct Census<'a> {
    pub wanted: Families,
    pub covered: &'a [u32],
    pub held: &'a Families,
    pub themed: HashSet<(u32, Vec<u32>)>,
}

/// Which of the master's shapes and families `census.wanted` names can follow the `Theme` style without changing the look of a shape the pass does not theme: none of the shapes
/// that stand for it and read the family through it (no link of their own for it) is on a page that selects a theme, or under a sheet that selects one of its own, unless the pass
/// themes it. **One walk of the drawing**, of the pages that are not covered, at most [`MAX_CENSUS_SHAPES`] shapes and no deeper than the model allows; past the cap nothing
/// can follow, so the pass links the shapes themselves, as before.
pub(super) fn can_follow(doc: &Document, census: Census<'_>) -> Families {
    let Census { mut wanted, covered, held, themed } = census;
    for (at, families) in held {
        if let Some(want) = wanted.get_mut(at) {
            for (w, h) in want.iter_mut().zip(families) {
                *w &= !*h;
            }
        }
    }
    let masters: HashSet<u32> = wanted.keys().map(|at| at.master).collect();
    let mut index = Masters::default();
    let mut seen = 0_usize;
    for page in doc.pages.iter() {
        if covered.contains(&page.id) {
            continue;
        }
        let page_themed = page_selects(doc, page.id);
        // The list walked, the place of the next shape in it, the master in force there and whether a sheet above selects a theme: one frame for each level.
        let mut stack: Vec<(&ShapeList, usize, Option<u32>, bool)> = vec![(&page.shapes, 0, None, page_themed)];
        let mut path: Vec<u32> = Vec::new();
        while let Some(frame) = stack.last_mut() {
            let (list, at, above, themed_above) = *frame;
            let Some(shape) = list.get(at) else {
                stack.pop();
                continue;
            };
            frame.1 = at.saturating_add(1);
            seen = seen.saturating_add(1);
            if seen > MAX_CENSUS_SHAPES {
                return Families::new();
            }
            let depth = stack.len();
            path.truncate(depth.saturating_sub(1));
            path.push(u32::try_from(at).unwrap_or(u32::MAX));
            let themed_here = themed_above || selects(&shape.sheet);
            let master = shape.master.or(above);
            if themed_here
                && master.is_some_and(|m| masters.contains(&m))
                && !themed.contains(&(page.id, path.clone()))
                && let Some((ms, _)) = index.stands_for(doc, shape, above)
                && let Some(want) = wanted.get_mut(&ms)
            {
                for (f, w) in want.iter_mut().enumerate() {
                    if family(shape.sheet.links, f).is_none() {
                        *w = false;
                    }
                }
            }
            if depth < MAX_SHAPE_DEPTH && !shape.children.is_empty() {
                stack.push((&shape.children, 0, master, themed_here));
            }
        }
    }
    wanted.retain(|_, families| families.iter().any(|f| *f));
    wanted
}

/// The shape of a master at a place, to write.
fn master_shape_mut<'d>(doc: &'d mut Document, at: &MasterShape) -> Option<&'d mut Shape> {
    let k = doc.masters.iter().position(|m| m.id == at.master)?;
    let master = Arc::make_mut(doc.masters.get_mut(k)?);
    let (first, rest) = at.path.split_first()?;
    let mut shape = Arc::make_mut(master.shapes.get_mut(usize::try_from(*first).ok()?)?);
    for step in rest {
        shape = Arc::make_mut(shape.children.get_mut(usize::try_from(*step).ok()?)?);
    }
    Some(shape)
}

/// Links each master's shape of `follow` to the style `theme` for the families it names (a link it holds already is left). → how many links were written.
pub(super) fn link(doc: &mut Document, follow: &Families, theme: u32) -> Result<usize, Problem> {
    let mut written = 0_usize;
    for (at, families) in follow {
        let shape = master_shape_mut(doc, at).ok_or_else(|| Problem::failed("a shape of a master is not there"))?;
        let sheet = Arc::make_mut(&mut shape.sheet);
        let mut changed = false;
        for (f, follows) in families.iter().enumerate() {
            let link = family_mut(&mut sheet.links, f);
            if *follows && *link != Some(theme) {
                *link = Some(theme);
                changed = true;
                written = written.saturating_add(1);
            }
        }
        if changed {
            drop_raw_links(sheet);
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_doc::{Cell, CellName, Master, Page, ShapeType, Value};

    use super::*;

    /// A drawing of one master of one shape on the root, and `pages` pages, each with an instance of it (`ID` 1) whose own links are `own`.
    fn drawing(pages: &[(u32, Links)]) -> Document {
        let mut doc = Document::empty();
        let mut master = Master { id: 7, ..Master::default() };
        let mut shape = Shape::new(1);
        shape.kind = ShapeType::Shape;
        Arc::make_mut(&mut shape.sheet).links = Links { line: Some(0), fill: Some(0), text: Some(0) };
        master.shapes.push(Arc::new(shape));
        doc.masters.push(Arc::new(master));
        for &(id, own) in pages {
            let mut page = Page::new(id, "P");
            let mut instance = Shape::new(1);
            instance.kind = ShapeType::Shape;
            instance.master = Some(7);
            Arc::make_mut(&mut instance.sheet).links = own;
            page.shapes.push(Arc::new(instance));
            doc.pages.push(Arc::new(page));
        }
        doc
    }

    fn select(doc: &mut Document, page: u32, number: &str) {
        let at = doc.pages.iter().position(|p| p.id == page).unwrap();
        let p = Arc::make_mut(doc.pages.get_mut(at).unwrap());
        for name in SCHEME_CELLS {
            Arc::make_mut(&mut p.sheet).cells.insert(CellName::new(name), Cell::constant(Value::from_text(number)));
        }
    }

    fn wanted() -> Families {
        Families::from([(MasterShape { master: 7, path: vec![0] }, [true; 3])])
    }

    fn census<'a>(covered: &'a [u32], held: &'a Families, themed: &[(u32, Vec<u32>)]) -> Census<'a> {
        Census { wanted: wanted(), covered, held, themed: themed.iter().cloned().collect() }
    }

    #[test]
    fn a_master_whose_other_instances_are_on_pages_with_no_theme_can_follow() {
        let doc = drawing(&[(1, Links::default()), (2, Links::default())]);
        let none = Families::new();
        assert_eq!(can_follow(&doc, census(&[1], &none, &[])), wanted(), "page 2 selects no theme");
    }

    #[test]
    fn an_instance_on_another_themed_page_that_reads_through_the_master_keeps_it_and_one_with_a_link_of_its_own_does_not() {
        let mut doc = drawing(&[(1, Links::default()), (2, Links::default())]);
        select(&mut doc, 2, "1001");
        let none = Families::new();
        assert_eq!(can_follow(&doc, census(&[1], &none, &[])), Families::new(), "page 2's instance would change");
        assert_eq!(can_follow(&doc, census(&[], &none, &[(1, vec![0]), (2, vec![0])])), wanted(), "unless the pass themes it");
        let mut doc = drawing(&[(1, Links::default()), (2, Links { line: Some(3), fill: None, text: Some(3) })]);
        select(&mut doc, 2, "1001");
        let only_fill = Families::from([(MasterShape { master: 7, path: vec![0] }, [true, false, true])]);
        assert_eq!(can_follow(&doc, census(&[1], &none, &[])), only_fill, "a family it links itself is not read through the master");
    }

    #[test]
    fn a_shape_that_selects_a_theme_of_its_own_on_a_page_with_none_keeps_the_master() {
        let mut doc = drawing(&[(1, Links::default()), (2, Links::default())]);
        let p = Arc::make_mut(doc.pages.get_mut(1).unwrap());
        let shape = Arc::make_mut(p.shapes.get_mut(0).unwrap());
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("ColorSchemeIndex"), Cell::constant(Value::from_text("40")));
        let none = Families::new();
        assert_eq!(can_follow(&doc, census(&[1], &none, &[])), Families::new());
    }

    #[test]
    fn what_the_planner_held_on_a_locked_layer_keeps_the_master() {
        let doc = drawing(&[(1, Links::default())]);
        let held = Families::from([(MasterShape { master: 7, path: vec![0] }, [false, true, false])]);
        let left = Families::from([(MasterShape { master: 7, path: vec![0] }, [true, false, true])]);
        assert_eq!(can_follow(&doc, census(&[1], &held, &[])), left);
    }

    #[test]
    fn linking_a_master_writes_its_shape_once_and_takes_the_kept_text_of_the_old_links_away() {
        let mut doc = drawing(&[(1, Links::default())]);
        let m = Arc::make_mut(doc.masters.get_mut(0).unwrap());
        let shape = Arc::make_mut(m.shapes.get_mut(0).unwrap());
        Arc::make_mut(&mut shape.sheet).raw.push("LineStyle", "00");
        assert_eq!(link(&mut doc, &wanted(), 5).unwrap(), 3);
        let shape = doc.master(7).unwrap().shapes.get(0).unwrap();
        assert_eq!(shape.sheet.links, Links { line: Some(5), fill: Some(5), text: Some(5) });
        assert!(shape.sheet.raw.iter().all(|(name, _)| name != "LineStyle"));
        assert_eq!(link(&mut doc, &wanted(), 5).unwrap(), 0, "a link it holds is left");
        assert!(link(&mut doc, &Families::from([(MasterShape { master: 9, path: vec![0] }, [true; 3])]), 5).is_err(), "a master that is not there");
    }
}
