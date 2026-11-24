//! The pages of the drawing as a save finds them against the file: which page of the model is which page of the file, which pages the model added, took
//! away or put in another order, and which it renamed.
//!
//! **Identity.** A page of the model is a page of the file by its `ID`, which the index records for each entry it holds ([`crate::PageShapes::page_id`]): the engine
//! adds a page under the highest `ID` plus one, a page that is deleted or moved keeps the `ID`s of the others, and a rename leaves it. When the model's `ID`s are
//! the index's, in the index's order, **nothing structural happened** and the pages are matched by place, as every save did before this one. Otherwise each page
//! of the model is looked up among the index's by `ID`; a page it does not hold is new, and an entry of the index no page of the model has is taken away. A model
//! or an index with an `ID` twice cannot be matched, and a save of a drawing whose pages changed is refused in words (the document file can give two pages one
//! `ID`; a drawing that is not edited is saved as it is).
//!
//! **Order.** A file reads its pages first and its background pages after them (`read::pages::assemble`), and every position the writer keeps (the index, the
//! document file's page entries, the checks of the bytes made) is a position in the file as read. So a save that changes the pages needs the model in that order:
//! the pages that are not backgrounds, then the background pages in the order the file has them. A background page is a master page that holds shapes: this save
//! writes no new one and takes none away.
//!
//! **What a save then does** (`pages.rs`, `mod.rs`): a page taken away is taken out with its shapes, the styles and the package files only they named (the
//! shapes' own rules; its master page and layout stay, a project rule against R13), a page of the model that the file lacks is written as a new element with new shapes, a page put in another place is moved with the edits
//! made inside it, and a page under another name has its `draw:name` written. A name is written when the name the file would read the page under, its own
//! `draw:name` or, when it has none, `Page-` and its place from 1, is not the model's: so a page without a name that moves is given the name it had.

use std::collections::{HashMap, HashSet};

use std::sync::Arc;

use diacraft_doc::{Document, Page, Sheet};

use super::attrs::TagEdit;
use super::compare::PageDiff;
use super::emit::Pfx;
use super::frame::Editor;
use super::scan::{PartKind, ScanPage};
use super::{PageOutputs, PartCtx, WriteError, WriteWork};
use crate::index::{PageShapes, PageSource, ShapeIndex};
use crate::limits::Limits;
use crate::read::names::Ns;

/// A `draw:page` of the file as the save leaves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Slot {
    /// The element that stands at this place among the pages of the file now (from 0).
    Old(usize),
    /// A new element, for the page of the model at this place.
    New(usize),
}

/// What the model did to the pages.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Structure {
    /// For each page of the model, in its order: the place of its entry in the index the caller holds, or `None` for a page the file lacks.
    pub matched: Vec<Option<usize>>,
    /// The `draw:page` elements of the file as the save leaves them, in order: the pages of the model that are not backgrounds.
    pub order: Vec<Slot>,
    /// The places among the file's `draw:page` elements of those the model no longer has, ascending.
    pub deleted: Vec<usize>,
    /// The place of the entry in the index of each of them, in the same order.
    pub deleted_held: Vec<usize>,
    /// How many `draw:page` elements the file has.
    pub elements: usize,
    /// The places of the pages of the model (in the model) whose name is not the one the file has for them.
    pub renamed: Vec<usize>,
    /// The elements stand in another order than the file's, with the model's new pages and the pages it took away left out.
    pub reordered: bool,
    /// The `ID`s of the model's pages were not the index's in its order: the pages were matched by `ID`.
    pub by_id: bool,
    /// When the model added a page: the sheet each page of the file has, by the place of its entry in the index (what the master pages and layouts say of the page's
    /// size, which a new page's size is compared with to say what its own layout differs by). Empty otherwise.
    pub base_sheets: Vec<Arc<Sheet>>,
    /// The place of the entry of the first page of the file (not a background): what the document file's native markup of a page the model added is made from.
    pub template: Option<usize>,
}

impl Structure {
    /// The structure of a drawing of `n` pages that the model left as they were (for the tests of what is made from it).
    #[cfg(test)]
    pub(crate) fn identity(n: usize) -> Structure {
        Structure {
            matched: (0..n).map(Some).collect(),
            order: (0..n).map(Slot::Old).collect(),
            elements: n,
            template: Some(0),
            ..Structure::default()
        }
    }

    /// Whether the model changed the pages of the drawing: added, took away, moved or renamed one.
    pub(crate) fn any(&self) -> bool {
        !self.deleted.is_empty() || self.reordered || !self.renamed.is_empty() || self.order.iter().any(|s| matches!(s, Slot::New(_)))
    }
}

fn refuse(words: &'static str) -> WriteError {
    WriteError::Unsupported(words)
}

/// The words of a refusal of a change of the pages in a flat drawing, which has no page of its own to put one in (a later task writes it).
pub(crate) const FLAT_PAGES: &str =
    "a page added to the drawing, taken from it, put in another place or renamed in a flat drawing (a package is saved with such a change)";

/// The words of a refusal of pages that cannot be told apart.
const SAME_ID: &str = "pages of one ID (the model's or the file's) in a drawing whose pages were added, taken away or put in another place: the pages cannot be told apart";

/// The words of a refusal of a page that is not a background put after a background page.
const AFTER_BACKGROUND: &str = "a page that is not a background put after a background page, or background pages in another order than the file has them (a file reads its pages first and its background pages after them)";

/// The words of a refusal of a new page that holds what a new element of this writer cannot.
const NEW_PAGE_FIELDS: &str = "a new page that holds the markup of a file's page element (a page read from a file and put back by an undo after a save took it away is one: a new element is not written with it), names a background page, is a background page, or holds a relationship of another format's";

/// The words of a refusal of a drawing left with no page that is not a background.
const NO_PAGE: &str = "a drawing with no page that is not a background (the file would hold no page: a drawing keeps one)";

/// The words of a refusal of a background page added or taken away.
const BACKGROUND_CHANGE: &str = "a background page (a master page that holds shapes) added to the drawing or taken from it";

/// The most pages a save writes: the most the reader reads (`Limits::max_pages`, and never more than `diacraft_doc::MAX_PAGES`), so that a file this writer makes is always one the
/// reader reads back.
pub(crate) fn most_pages(limits: &Limits) -> usize {
    limits.max_pages.min(diacraft_doc::MAX_PAGES)
}

/// The refusal of a model of more pages than a drawing holds.
pub(crate) fn too_many(limits: &Limits) -> WriteError {
    WriteError::Limit { what: "pages in the drawing", max: most_pages(limits) as u64 }
}

/// What the model's pages are against the file's. `held` is the index the caller holds and `fresh` the index reading the file again gives; `base` is the model the
/// file reads as. The caller has checked that the two indexes and `base` hold as many pages as each other.
pub(crate) fn of(
    doc: &Document,
    base: &Document,
    held: &ShapeIndex,
    fresh: &ShapeIndex,
    flat: bool,
    limits: &Limits,
    work: &mut WriteWork,
) -> Result<Structure, WriteError> {
    if doc.pages.len() > most_pages(limits) {
        return Err(too_many(limits));
    }
    let n = doc.pages.len();
    work.page_steps = work.page_steps.saturating_add(n as u64).saturating_add(held.pages() as u64);
    work.check_steps()?;
    // The pages of the model are the index's, in its order: matched by place.
    let by_place = n == held.pages() && doc.pages.iter().enumerate().all(|(at, p)| held.page(at).and_then(|e| e.page_id()) == Some(p.id));
    let mut matched: Vec<Option<usize>> = if by_place {
        (0..n).map(Some).collect()
    } else {
        if flat {
            // The same pages in another order are a move, and another number of pages an addition or a deletion: a flat drawing has no way to write either. The same number of
            // pages under other `ID`s is a change of the pages' own fields, which it refuses as it refuses the document file's data.
            let moved = n == held.pages() && by_id(doc, held).is_ok_and(|m| m.iter().all(Option::is_some));
            return Err(if n == held.pages() && !moved { refuse("a change of a page's own fields, size or layers") } else { refuse(FLAT_PAGES) });
        }
        by_id(doc, held)?
    };
    // A page of the model that has the `ID` of a page of the file but not its markup is not that page: the engine gives a page added after the last page was taken away the
    // `ID` the last had, and a model of another file has other pages under the same `ID`s. The file's is taken away and the model's is added, which writes the page the model
    // has (and refuses one that holds markup, as every page added is). A flat drawing keeps the pages it has: a change of their own fields is refused as it was.
    let mut replaced = false;
    if !flat {
        for (page, slot) in doc.pages.iter().zip(matched.iter_mut()) {
            let Some(h) = *slot else { continue };
            let saved = held.page(h).and_then(|e| e.saved());
            let Some(was) = saved.map(|p| &**p).or_else(|| base.pages.get(h).map(|p| &**p)) else { continue };
            if !super::compare::same_page_markup(page, was) {
                *slot = None;
                replaced = true;
            }
        }
    }
    let slow = !by_place || replaced;
    let mut st = Structure { matched, by_id: slow, ..Structure::default() };
    // What each entry of the file is: its place among the elements, from the sources the two indexes agree on.
    let mut foreground = 0usize;
    for at in 0..held.pages() {
        let (Some(h), Some(f)) = (held.page(at), fresh.page(at)) else {
            return Err(WriteError::Mismatch("the index does not hold the pages of the drawing"));
        };
        if h.source != f.source {
            return Err(WriteError::Mismatch("the index does not say where the pages came from"));
        }
        if matches!(h.source, PageSource::Page(_)) {
            foreground += 1;
        }
    }
    st.elements = foreground;
    st.template = (0..held.pages()).find(|at| matches!(held.page(*at).map(|e| e.source), Some(PageSource::Page(_))));
    let mut seen_background = false;
    let mut last_background: Option<usize> = None;
    let mut kept_places: Vec<usize> = Vec::new();
    for (at, page) in doc.pages.iter().enumerate() {
        let entry = st.matched.get(at).copied().flatten();
        let source = entry.and_then(|h| held.page(h)).map(|e| e.source);
        match source {
            Some(PageSource::Page(k)) => {
                if seen_background && slow {
                    return Err(refuse(AFTER_BACKGROUND));
                }
                if page.background {
                    return Err(WriteError::Unsupported("a change of a page's own fields, size or layers"));
                }
                st.order.push(Slot::Old(k));
                kept_places.push(k);
            }
            Some(PageSource::MasterPage(m)) => {
                seen_background = true;
                if !page.background {
                    return Err(WriteError::Unsupported("a change of a page's own fields, size or layers"));
                }
                if slow && last_background.is_some_and(|l| l >= m) {
                    return Err(refuse(AFTER_BACKGROUND));
                }
                last_background = Some(m);
            }
            None => {
                if page.background {
                    return Err(refuse(BACKGROUND_CHANGE));
                }
                if seen_background {
                    return Err(refuse(AFTER_BACKGROUND));
                }
                check_new(page)?;
                st.order.push(Slot::New(at));
            }
        }
    }
    // The file's elements the model has no page for: those of the entries nothing matched.
    let mut used: HashSet<usize> = HashSet::new();
    for h in st.matched.iter().flatten() {
        used.insert(*h);
    }
    let mut gone: Vec<(usize, usize)> = Vec::new();
    for at in 0..held.pages() {
        if used.contains(&at) {
            continue;
        }
        match held.page(at).map(|e| e.source) {
            Some(PageSource::Page(k)) => {
                gone.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the pages taken away", max: gone.len() as u64 })?;
                gone.push((k, at));
            }
            Some(PageSource::MasterPage(_)) => return Err(refuse(BACKGROUND_CHANGE)),
            None => return Err(WriteError::Mismatch("the index does not hold the pages of the drawing")),
        }
    }
    gone.sort_unstable();
    (st.deleted, st.deleted_held) = gone.into_iter().unzip();
    // The elements the model keeps stand in the order the file has them, or move.
    st.reordered = kept_places.windows(2).any(|w| w.first() >= w.get(1));
    // A page under another name than the file's: the model's name against the name **the file reads the page under** (`base`, the file read just now), and not against
    // the name of the page as an earlier save had the model (`PageShapes::saved`): a save that only renamed the page leaves that page as it was, with the name it had
    // before, so the rename back to it would be taken for no change, and a page saved again after a rename would be said to be renamed.
    for (at, page) in doc.pages.iter().enumerate() {
        let Some(h) = st.matched.get(at).copied().flatten() else { continue };
        let was = base.pages.get(h).map(|p| &p.name);
        if was.is_some_and(|w| **w != *page.name) {
            // A background page is a master page, named by its display name: this save does not rename one.
            if page.background {
                return Err(WriteError::Unsupported("a change of a page's own fields, size or layers"));
            }
            st.renamed.push(at);
        }
    }
    // A drawing keeps a page that is not a background: a file with none has no page for the reader to find, and none to put a page by.
    if st.order.is_empty() && st.any() {
        return Err(refuse(NO_PAGE));
    }
    if st.order.iter().any(|s| matches!(s, Slot::New(_))) {
        st.base_sheets = base.pages.iter().map(|p| Arc::clone(&p.sheet)).collect();
    }
    Ok(st)
}

/// Each page of the model, looked up among the index's entries by its `ID`.
fn by_id(doc: &Document, held: &ShapeIndex) -> Result<Vec<Option<usize>>, WriteError> {
    let mut places: HashMap<u32, usize> = HashMap::new();
    places.try_reserve(held.pages()).map_err(|_| WriteError::Limit { what: "memory for the pages of the index", max: held.pages() as u64 })?;
    for at in 0..held.pages() {
        let Some(id) = held.page(at).and_then(|e| e.page_id()) else {
            return Err(WriteError::Mismatch("the index does not say which page of the drawing each entry is"));
        };
        if places.insert(id, at).is_some() {
            return Err(refuse(SAME_ID));
        }
    }
    let mut ids: HashSet<u32> = HashSet::new();
    ids.try_reserve(doc.pages.len()).map_err(|_| WriteError::Limit { what: "memory for the pages of the drawing", max: doc.pages.len() as u64 })?;
    let mut out = Vec::new();
    out.try_reserve_exact(doc.pages.len())
        .map_err(|_| WriteError::Limit { what: "memory for the pages of the drawing", max: doc.pages.len() as u64 })?;
    for page in doc.pages.iter() {
        if !ids.insert(page.id) {
            return Err(refuse(SAME_ID));
        }
        out.push(places.get(&page.id).copied());
    }
    Ok(out)
}

/// A new page holds what a new element of this writer can: no background, no page it names as its background, no markup.
fn check_new(page: &Page) -> Result<(), WriteError> {
    let odf_markup = page.ext.get().is_some_and(|d| !d.odf.kept.raw.is_empty() || !d.odf.kept.kept.is_empty());
    // What the VSDX file held of the page (its attributes and elements, the relationship and the part it came from) rides in DiaCraft's document file; a background and a page that names
    // one, and the markup of an `.odg` page, are what a new page of this writer does not hold.
    if page.background || page.back_page.is_some() || odf_markup {
        return Err(refuse(NEW_PAGE_FIELDS));
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------------------------------------ what a save writes

/// Where each element of `content.xml` is after the save, by the place it has in the scan of the part before it (those the file has, then the pages the model added, in the
/// order the diff has them, each at the place `PageDiff::source` gives it); `usize::MAX` for a page taken away.
pub(super) fn ordinals(st: &Structure, elements: usize) -> Result<Vec<usize>, WriteError> {
    let added = st.order.iter().filter(|s| matches!(s, Slot::New(_))).count();
    let mut map: Vec<usize> = Vec::new();
    map.try_reserve_exact(elements.saturating_add(added))
        .map_err(|_| WriteError::Limit { what: "memory for the pages of the part", max: elements as u64 })?;
    map.extend(0..elements.saturating_add(added));
    if !st.any() && added == 0 {
        return Ok(map);
    }
    if st.elements != elements {
        return Err(WriteError::Mismatch("the part holds other pages than the file was read with"));
    }
    for &k in &st.deleted {
        if let Some(slot) = map.get_mut(k) {
            *slot = usize::MAX;
        }
    }
    let mut next_new = elements;
    for (place, slot) in st.order.iter().enumerate() {
        let at = match slot {
            Slot::Old(k) => *k,
            Slot::New(_) => {
                next_new += 1;
                next_new - 1
            }
        };
        match map.get_mut(at) {
            Some(m) => *m = place,
            None => return Err(WriteError::Mismatch("a page the plan names is not in the part")),
        }
    }
    Ok(map)
}

/// A `Vec` of `n` copies of `value`, an error and not an abort when the memory for it is not had.
fn filled<T: Clone>(n: usize, value: T) -> Result<Vec<T>, WriteError> {
    let mut list = Vec::new();
    list.try_reserve_exact(n).map_err(|_| WriteError::Limit { what: "memory for the plan of the pages", max: n as u64 })?;
    list.resize(n, value);
    Ok(list)
}

/// The words of a refusal of two pages under one name.
const SAME_NAME: &str =
    "two pages under one name, one of them written by this save (`draw:name` is unique among a drawing's pages: rename one of them)";

impl Editor<'_, '_> {
    /// Writes `name` as the `draw:name` of the page whose element is `page`: in its place when it is there, added when it is not.
    fn set_page_name(&mut self, page: &ScanPage, name: &str) -> Result<(), WriteError> {
        let mut tag = TagEdit::untracked(self.bytes, page.open, page.scope, self.work)?;
        self.work.charge_attribute(self.limits)?;
        match tag.find(&mut self.scopes, Ns::Draw, "name", self.work) {
            Some(at) => tag.set(&mut self.splicer, at, name),
            None => tag.add(&mut self.splicer, &mut self.scopes, self.work, Ns::Draw, "name", name),
        }
    }
}

/// Makes the edits of `content.xml` that put the pages as the model has them (the module documentation): the pages taken away are taken out, the pages that stay in
/// another place are moved (those outside a longest run that keeps its order move), the pages the model added are written where they go, and a page is given the name
/// the model has when the file would read it under another. Every loop is counted ([`WriteWork::page_steps`]) and charged before it runs.
pub(super) fn apply(
    ed: &mut Editor<'_, '_>,
    out: &mut PageOutputs,
    scan_pages: &[ScanPage],
    drawing_scope: Option<u32>,
    ctx: &PartCtx<'_>,
    ord_map: &[usize],
) -> Result<(), WriteError> {
    let st = ctx.structure;
    let n = scan_pages.len();
    if st.elements != n {
        return Err(WriteError::Mismatch("the part holds other pages than the file was read with"));
    }
    ed.work.page_steps = ed.work.page_steps.saturating_add(st.order.len() as u64).saturating_add(st.deleted.len() as u64);
    ed.work.check_steps()?;
    // The pages taken away, with everything inside them: the styles and the files only they named go by the shapes' rules.
    for &k in &st.deleted {
        let page = scan_pages.get(k).ok_or(WriteError::Mismatch("a page taken away is not in the part"))?;
        ed.splicer.remove(page.lead, page.end);
        if let Some(book) = ed.book.as_mut() {
            book.unit_removed(page.lead, page.end, ed.work);
        }
        out.gone.insert(u32::try_from(k).unwrap_or(u32::MAX), filled(page.shapes.len().saturating_add(1), true)?);
    }
    // The pages of the model that are not backgrounds, with the element each is.
    let foreground: Vec<&Arc<Page>> = ctx.doc.pages.iter().filter(|p| !p.background).collect();
    if foreground.len() != st.order.len() {
        return Err(WriteError::Mismatch("the pages of the drawing are not the pages the save leaves"));
    }
    let old: Vec<usize> = st
        .order
        .iter()
        .filter_map(|s| match s {
            Slot::Old(k) => Some(*k),
            Slot::New(_) => None,
        })
        .collect();
    let stay: HashSet<usize> = super::plan::longest_run(&old).into_iter().collect();
    let first_stay = old.iter().find(|k| stay.contains(k)).and_then(|k| scan_pages.get(*k)).map(|p| p.lead);
    let front = first_stay.or_else(|| scan_pages.first().map(|p| p.lead));
    // The white space the file puts before a page, which a new page is given too.
    let indent: String = scan_pages
        .first()
        .and_then(|p| ed.bytes.get(p.lead..p.open.0))
        .filter(|b| b.len() <= 64 && b.iter().all(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r')))
        .and_then(|b| std::str::from_utf8(b).ok())
        .unwrap_or_default()
        .to_owned();
    // The pages the model added, by their place in the model: found once, and each page one look.
    let added: HashMap<usize, &PageDiff> = ctx.added.iter().map(|d| (d.page_at, *d)).collect();
    let mut cursor: Option<usize> = None;
    let mut written: HashMap<&str, (usize, bool)> = HashMap::new();
    written.try_reserve(st.order.len()).map_err(|_| WriteError::Limit { what: "memory for the names of the pages", max: st.order.len() as u64 })?;
    for (place, (slot, model)) in st.order.iter().zip(foreground).enumerate() {
        ed.work.page_steps = ed.work.page_steps.saturating_add(1);
        let mut is_written = false;
        match *slot {
            Slot::Old(k) => {
                let page = scan_pages.get(k).ok_or(WriteError::Mismatch("a page the plan names is not in the part"))?;
                if stay.contains(&k) {
                    cursor = Some(page.end);
                } else {
                    let to = cursor.or(front).ok_or(refuse("pages put in another place in a drawing that has no page element to put them by"))?;
                    ed.splicer.move_unit((page.lead, page.end), to);
                }
                // The name the file reads this page under after the save: its own, or `Page-` and its place from 1 when it has none (the reader's rule).
                let differs = match page.name.as_deref() {
                    Some(own) => own != &*model.name,
                    None => format!("Page-{}", place.saturating_add(1)) != *model.name,
                };
                if differs {
                    ed.set_page_name(page, &model.name)?;
                    is_written = true;
                }
            }
            Slot::New(at) => {
                let to = cursor.or(front).ok_or(refuse("a page added to a drawing that has no page element to put it by"))?;
                new_page(ed, out, drawing_scope, ctx, ord_map, &added, &indent, to, at, model)?;
                is_written = true;
            }
        }
        let entry = written.entry(&model.name).or_insert((0, false));
        entry.0 += 1;
        entry.1 |= is_written;
    }
    if written.values().any(|(count, new)| *count > 1 && *new) {
        return Err(refuse(SAME_NAME));
    }
    Ok(())
}

/// Writes the page of the model at `at` as a new element at `to`, with its new shapes.
#[allow(clippy::too_many_arguments)]
fn new_page(
    ed: &mut Editor<'_, '_>,
    out: &mut PageOutputs,
    drawing_scope: Option<u32>,
    ctx: &PartCtx<'_>,
    ord_map: &[usize],
    added: &HashMap<usize, &PageDiff>,
    indent: &str,
    to: usize,
    at: usize,
    model: &Page,
) -> Result<(), WriteError> {
    let diff = added.get(&at).copied().ok_or(WriteError::Mismatch("a page the plan adds has no changes"))?;
    let (_, master) = ctx.plans.new_masters.get(&at).ok_or(WriteError::Mismatch("a page the plan adds has no master page"))?;
    let scope = drawing_scope.ok_or(refuse("a page added to a part with no office:drawing"))?;
    let pfx = Pfx::of(&mut ed.scopes, scope, ed.work);
    let qname = format!("{}:page", pfx.draw);
    ed.work.charge_attribute(ed.limits)?;
    ed.work.charge_attribute(ed.limits)?;
    let tag = format!("{indent}<{qname}{}{}{}", pfx.attr(Ns::Draw, "name", &model.name), pfx.attr(Ns::Draw, "master-page-name", master), pfx.declare);
    // The page has no element in the part: its shapes are put after the start tag, which is where `to` is, in the order the model has them.
    // The page is put in the first `office:drawing`, whose bindings are the ones its new shapes' kept markup is written against, and declares nothing a kept name could use.
    let scan = ScanPage { open: (to, to), scope, ns: Some(Vec::new()), ..ScanPage::default() };
    // The page's comments, written after its shapes.
    let block = match &diff.comments {
        Some(edit) => super::comments::prepare(ed, &scan, scope, diff, edit, true)?,
        None => super::comments::Block::default(),
    };
    let holds = !diff.new.is_empty() || !block.text.is_empty();
    if holds {
        ed.splicer.insert(to, format!("{tag}{}>", block.declare));
    } else {
        ed.splicer.insert(to, format!("{tag}/>"));
    }
    let held = PageShapes::new(diff.source);
    let mut done = super::plan::apply_page(ed, &scan, diff, &held)?;
    if !block.text.is_empty() {
        ed.splicer.insert(to, block.text);
    }
    if let Some(edit) = &diff.comments {
        let (places, total) = super::comments::places_after(edit);
        done.shapes.set_comments(places, total);
    }
    if holds {
        ed.splicer.insert(to, format!("</{qname}>"));
    }
    let (PageSource::Page(scan_at) | PageSource::MasterPage(scan_at)) = diff.source;
    let now_at = ord_map.get(scan_at).copied().ok_or(WriteError::Mismatch("a page the plan adds is not in the part made"))?;
    out.record(ed, PartKind::Content, diff, (scan_at, now_at), false, done);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::ReadCost;

    /// A drawing of `n` pages under the `ID`s `ids` (`n` of them), and the index and baseline of a file that has pages `0` to `n - 1` under those `ID`s.
    fn file_of(n: usize) -> (Document, ShapeIndex) {
        let mut base = Document::empty();
        let mut index = ShapeIndex::default();
        for k in 0..n {
            base.pages.push(Arc::new(Page::new(u32::try_from(k).unwrap(), &format!("p{k}"))));
            index.push(PageShapes::new(PageSource::Page(k)).with_page_id(u32::try_from(k).unwrap()));
        }
        (base, ShapeIndex::from_pages((0..n).filter_map(|k| index.page(k).cloned()).collect(), ReadCost::default()))
    }

    fn model_of(ids: &[u32]) -> Document {
        let mut doc = Document::empty();
        for id in ids {
            doc.pages.push(Arc::new(Page::new(*id, &format!("p{id}"))));
        }
        doc
    }

    fn plan_of(doc: &Document, base: &Document, index: &ShapeIndex, work: &mut WriteWork) -> Result<Structure, WriteError> {
        of(doc, base, index, index, false, &Limits::default(), work)
    }

    #[test]
    fn pages_of_the_same_ids_in_the_same_order_are_no_change_and_each_other_case_is_found_by_id() {
        let (base, index) = file_of(4);
        let mut work = WriteWork::default();
        let same = plan_of(&model_of(&[0, 1, 2, 3]), &base, &index, &mut work).unwrap();
        assert!(!same.any() && !same.by_id && same.deleted.is_empty());
        assert_eq!(same.order, [Slot::Old(0), Slot::Old(1), Slot::Old(2), Slot::Old(3)]);
        // One taken away, one added, the rest in another order.
        let moved = plan_of(&model_of(&[3, 0, 7, 2]), &base, &index, &mut work).unwrap();
        assert!(moved.any() && moved.by_id && moved.reordered);
        assert_eq!(moved.matched, [Some(3), Some(0), None, Some(2)]);
        assert_eq!(moved.order, [Slot::Old(3), Slot::Old(0), Slot::New(2), Slot::Old(2)]);
        assert_eq!((moved.deleted.as_slice(), moved.deleted_held.as_slice()), ([1].as_slice(), [1].as_slice()));
        assert_eq!(moved.elements, 4);
        // A rename alone is a structure that is not a move.
        let mut renamed = model_of(&[0, 1, 2, 3]);
        Arc::make_mut(renamed.pages.get_mut(2).unwrap()).name = Arc::from("other");
        let renamed = plan_of(&renamed, &base, &index, &mut work).unwrap();
        assert!(renamed.any() && !renamed.reordered && renamed.renamed == [2] && renamed.deleted.is_empty());
        // Nothing taken away and nothing moved: only pages added.
        let added = plan_of(&model_of(&[0, 1, 2, 3, 4]), &base, &index, &mut work).unwrap();
        assert!(added.any() && !added.reordered && added.deleted.is_empty());
        assert_eq!(added.order.last(), Some(&Slot::New(4)));
        assert_eq!(added.base_sheets.len(), 4, "the sheets of the file's pages are kept for the size of a page added");
        assert!(moved.base_sheets.len() == 4 && same.base_sheets.is_empty());
    }

    #[test]
    fn pages_that_cannot_be_told_apart_are_refused_when_they_change_and_not_otherwise() {
        let (base, index) = file_of(3);
        let mut work = WriteWork::default();
        // Two pages of the model of one ID, with a page taken away: refused in words.
        let twice = model_of(&[1, 1]);
        assert!(matches!(plan_of(&twice, &base, &index, &mut work), Err(WriteError::Unsupported(w)) if w.contains("cannot be told apart")));
        // Two entries of the index of one ID, and a model that changes the pages: the same.
        let (twin_base, twin_index) = {
            let mut base = Document::empty();
            let mut index = ShapeIndex::default();
            for k in 0..3 {
                base.pages.push(Arc::new(Page::new(5, &format!("p{k}"))));
                index.push(PageShapes::new(PageSource::Page(k)).with_page_id(5));
            }
            (base, index)
        };
        assert!(
            matches!(plan_of(&model_of(&[5]), &twin_base, &twin_index, &mut work), Err(WriteError::Unsupported(w)) if w.contains("cannot be told apart"))
        );
        // The same IDs in the same order are matched by place, twins or not, and saved as they are.
        let unchanged = plan_of(&twin_base, &twin_base, &twin_index, &mut work).unwrap();
        assert!(!unchanged.any());
        // And an entry of the index that says no ID is no index to match pages by, when the pages changed.
        let mut nameless = ShapeIndex::default();
        nameless.push(PageShapes::new(PageSource::Page(0)));
        let (one_base, _) = file_of(1);
        assert!(matches!(plan_of(&model_of(&[9]), &one_base, &nameless, &mut work), Err(WriteError::Mismatch(_))));
    }

    #[test]
    fn a_drawing_keeps_a_page_and_a_page_that_is_not_a_background_does_not_follow_one() {
        let (base, index) = file_of(2);
        let mut work = WriteWork::default();
        let words = |r: Result<Structure, WriteError>| match r {
            Err(WriteError::Unsupported(w)) => w,
            other => panic!("{other:?}"),
        };
        assert!(words(plan_of(&model_of(&[]), &base, &index, &mut work)).contains("no page"));
        // A foreground page put after a page that is a background: the file reads its pages first.
        let mut doc = model_of(&[0, 5]);
        Arc::make_mut(doc.pages.get_mut(0).unwrap()).background = true;
        let background_first = plan_of(&doc, &base, &index, &mut work);
        assert!(background_first.is_err(), "{background_first:?}");
    }

    #[test]
    fn the_places_the_elements_have_after_the_save_follow_the_order_with_the_pages_added_after_those_the_file_has() {
        let st = Structure { order: vec![Slot::Old(2), Slot::New(1), Slot::Old(0)], deleted: vec![1], elements: 3, ..Structure::default() };
        // Elements 0 and 2 of the file move to places 2 and 0, element 1 is taken away, and the page added (the first after the file's three) is put at place 1.
        assert_eq!(ordinals(&st, 3).unwrap(), [2, usize::MAX, 0, 1]);
        assert!(matches!(ordinals(&st, 4), Err(WriteError::Mismatch(_))), "a part of other pages than the file was read with");
        assert_eq!(ordinals(&Structure::default(), 3).unwrap(), [0, 1, 2], "no change: each element where it is");
    }

    #[test]
    fn the_plan_of_pages_costs_steps_in_proportion_to_their_number_at_the_most_pages_a_drawing_holds() {
        let steps = |n: usize| {
            let (base, index) = file_of(n);
            let ids: Vec<u32> = (0..u32::try_from(n).unwrap()).rev().collect();
            let mut work = WriteWork::default();
            let st = plan_of(&model_of(&ids), &base, &index, &mut work).unwrap();
            assert!(st.reordered && st.deleted.is_empty());
            let old: Vec<usize> = st.order.iter().filter_map(|s| if let Slot::Old(k) = s { Some(*k) } else { None }).collect();
            assert_eq!(old.len(), n);
            // The pages that stay are a longest run that keeps its order: one page of a reversed drawing, and the rest move.
            assert_eq!(super::super::plan::longest_run(&old).len(), 1);
            ordinals(&st, n).unwrap();
            work.page_steps
        };
        let (half, full) = (steps(500), steps(1_000));
        assert_eq!(half, 2 * 500, "each page of the model and of the index once");
        assert_eq!(full, 2 * half, "in proportion to the pages");
        // One page more than the most is refused before any page is looked at.
        let (base, index) = file_of(1);
        let mut work = WriteWork::default();
        let too_many: Vec<u32> = (0..1_001).collect();
        assert!(matches!(plan_of(&model_of(&too_many), &base, &index, &mut work), Err(WriteError::Limit { max: 1_000, .. })));
        assert_eq!(work.page_steps, 0);
    }
}
