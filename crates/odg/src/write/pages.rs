//! A page's size, margins and orientation:
//! they live in the page layout of the page's master page, not in the page.
//!
//! **Which of the three is edited**. A page whose master page and layout no other page uses has its layout edited where it is, in
//! the places of its attributes (the page's own size, never its width and height swapped for landscape). A page whose master page, or whose
//! layout, another page uses too gets **a copy of both**: the master page under its name followed by `_` and the lowest number from 2 not in
//! use, with the display name followed by a space and that number, and a layout under the next free name in its prefix (`PM1` after `PM0`); the
//! copy holds the master page's children as they are, the edit is made in the copy of the layout, and the page names the copy
//! (`draw:master-page-name`). The first master page and layout, and the other pages, are untouched.
//!
//! **Which pages share a layout** is counted through every master page that names it: the pages that reach a layout are the pages of each master
//! page that names it (a layout two master pages name serves the pages of both). A page that moves to a copy is taken from the count of the master
//! page it left and of the layout that master page names, and from no other: a page of another master page that names the same layout still
//! reaches it (taking it from the layout's count by the master pages that name it let a layout two master pages share be
//! edited in place under a page that was not edited).
//!
//! **What a save leaves unnamed is taken out**: a master page that a page named before the save and none names after. One that nothing named
//! before stays, as an orphaned style that was an orphan before does. A layout is never left unnamed by these edits: the last page that reaches a
//! layout is edited in place, and its master page keeps naming it. **A page taken away leaves its master page and layout in the file: a project rule against R13**
//! Whose sentence this section reads as the resize's own.
//!
//! **A background page** (a master page that holds shapes, which the model has as a background page) shares its layout with the page that names
//! it: when that page is resized the layout is edited only if the model's background page has the same new size (and the shapes of the background
//! page are written against it); otherwise the save is refused in words, and nothing changes in the file that the model does not hold.
//! A split of such a master page is refused: its copy would be another background page the model lacks.
//!
//! **What is refused, in words:** the cases above, and a page whose master page or layout the file lacks.
//!
//! **A resized page's shapes** are placed against the page's height: the comparison finds each shape's `svg:y` changed when the height is, and
//! the rest of the save writes it.

use std::collections::{HashMap, HashSet};

use super::attrs::TagEdit;
use super::compare::{Diff, PageDiff};
use super::frame::Editor;
use super::parts::Source;
use super::scan::{Ask, Fam, LayoutDef, PartKind, ScanPage, scan_part_asking};
use super::splice::{Splicer, escape_value};
use super::structure::Slot;
use super::stylemap::{CellVal, Op, PageEdit, page_ops};
use super::styles::{Ctx, Names};
use diacraft_doc::Document;

use super::{WriteError, WriteWork};
use crate::index::PageSource;
use crate::limits::Limits;
use crate::read::names::Ns;

/// The master page a page's edit writes into: a copy of its master page and layout, or the layout where it is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PagePlan {
    /// The page's place in the content part's scan: its `draw:page` element before the save. `None` for a page the save adds, which is written with the
    /// master page its plan makes.
    pub scan_page: Option<usize>,
    /// The place of the page's element among those of the part made (what the check of the bytes made looks at).
    pub made_page: usize,
    /// The page's master page (by name) and the layout it names.
    pub master: String,
    pub layout: String,
    /// The copies to make: the master page's new name and display name, and the new layout's name.
    pub split: Option<Split>,
    /// What is written into the layout (the copy's, or the one the page has).
    pub ops: Vec<Op>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Split {
    pub master: String,
    pub display: String,
    pub layout: String,
}

fn refuse(words: &'static str) -> WriteError {
    WriteError::Unsupported(words)
}

/// The words of a size edit of a background page the page that names its master page does not make with it.
const BACKGROUND_ALONE: &str = "a change of the size of a background page (a master page that holds shapes) that the page which names it does not make too: they share the layout, so both change size or neither does";

/// The words of a resize of a page whose background page the model does not resize with it.
const BACKGROUND_STAYS: &str = "a page resized whose master page holds shapes (a background page the model keeps at its size): the layout is the background page's too, which would change size with it";

/// The words of a new page for which the file has no master page without shapes to take the size from.
const NO_MASTER: &str = "a new page in a file whose pages all name a master page that holds shapes (a background page): the new page needs a master page with no shapes to take its size from";

/// What the pages' size edits come to: where each is written, and the master pages the save leaves unnamed.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PagesPlan {
    pub plans: Vec<PagePlan>,
    /// The master pages (by their place among the master pages of `styles.xml`, and name) that no page names after the save, taken out.
    pub unnamed_masters: Vec<(usize, String)>,
    /// The master page each page the save adds names, by the page's place in the model, with the place of its element among those of the part made.
    pub new_masters: HashMap<usize, (usize, String)>,
}

/// What a copy of a master page needs named: the names in use and a cursor for each master page that only moves on.
struct Namer {
    taken: HashSet<String>,
    suffixes: HashMap<String, HashSet<u64>>,
    cursor: HashMap<String, u64>,
    names: Names,
}

impl Namer {
    /// The names of the master pages and layouts of `styles.xml`: found once, so that a name is the lowest number from 2 that is not in use by a cursor that only moves
    /// on, and not by a walk from 2 for each split (a part of many `Default_2` to `Default_k` made each split walk k names). Each name looked at and each number tried is a
    /// step, and the steps are capped.
    fn of(styles: &super::scan::Scan, work: &mut WriteWork) -> Result<Namer, WriteError> {
        let taken: HashSet<String> = styles.pages.iter().filter_map(|m| m.name.clone()).collect();
        let mut suffixes: HashMap<String, HashSet<u64>> = HashMap::new();
        for name in taken.iter() {
            work.master_name_tries += 1;
            if !work.step() {
                work.check_steps()?;
            }
            if let Some((base, n)) = name.rsplit_once('_').and_then(|(b, n)| Some((b, n.parse::<u64>().ok()?))) {
                suffixes.entry(base.to_owned()).or_default().insert(n);
            }
        }
        let mut names = Names::default();
        names.add_all(styles.layouts.iter().map(|l| (Fam::Layout, l.name.as_str())).chain(styles.defs.iter().map(|d| (d.family, d.name.as_str()))));
        Ok(Namer { taken, suffixes, cursor: HashMap::new(), names })
    }

    /// The copy of the master page `master` (shown as `master_page`) that the next split makes: its name (the lowest number from 2 not in use after the name), its
    /// display name and the new layout's name.
    fn copy(&mut self, master: &str, master_page: &ScanPage, work: &mut WriteWork) -> Result<Split, WriteError> {
        let next = self.cursor.entry(master.to_owned()).or_insert(2);
        let none = HashSet::new();
        let used = self.suffixes.get(master).unwrap_or(&none);
        while used.contains(next) {
            work.master_name_tries += 1;
            if !work.step() {
                work.check_steps()?;
            }
            *next += 1;
        }
        work.master_name_tries += 1;
        if !work.step() {
            work.check_steps()?;
        }
        let new_master = (*next, format!("{master}_{next}"));
        *next += 1;
        if !self.taken.insert(new_master.1.clone()) {
            return Err(refuse("master pages whose names the copy of a master page would repeat"));
        }
        let display = format!("{} {}", master_page.display.as_deref().unwrap_or(master), new_master.0);
        Ok(Split { master: new_master.1, display, layout: self.names.next(Fam::Layout) })
    }
}

/// The page layout's cells of a page as text, in the order of the cells: what two pages that a master page serves have in common. `Err` for a cell that holds a formula,
/// which no layout says.
fn layout_key(sheet: &diacraft_doc::Sheet) -> Result<String, &'static str> {
    let mut key = String::new();
    for name in super::stylemap::PAGE_CELLS {
        match sheet.cell(name) {
            Some(cell) => {
                let v = CellVal::of(cell)?;
                key.push_str(&format!("{name}={}|{:?}|{:?};", v.text, v.number.map(f64::to_bits), v.unit));
            }
            None => key.push_str(&format!("{name}=;")),
        }
    }
    Ok(key)
}

/// Works out, for each page whose size was edited and for each page the model added, what is written where. Scans both parts of the original: the pages' master pages in
/// `content.xml`, the master pages and layouts in `styles.xml`.
///
/// **A page the model adds names a master page**: one a page of the drawing already has, as the save leaves it, when that page has the same
/// layout cells (a page copied from another, which is the common case, names the other's master page); else a copy of a master page with no shapes and its layout, made as R13 says
/// for a page resized, with the layout edited to the new page's size. The copy is made from the master page of the nearest page before the new one that has one with no shapes (else
/// the first), and what it is edited by is the new page's cells against that page's in the file; a copy made for one new page is the one another new page of the same size names.
pub(crate) fn plan(source: &mut Source<'_, '_>, diff: &Diff, doc: &Document, limits: &Limits, work: &mut WriteWork) -> Result<PagesPlan, WriteError> {
    let edited: Vec<&PageDiff> =
        diff.pages.iter().filter(|p| p.held_at.is_some() && p.size.is_some() && matches!(p.source, PageSource::Page(_))).collect();
    let added: Vec<&PageDiff> = diff.pages.iter().filter(|p| p.held_at.is_none()).collect();
    // The background pages whose size was edited, by the place of their master page: each must be matched by the page that names it.
    let mut background: HashMap<usize, &PageDiff> = HashMap::new();
    for p in &diff.pages {
        if let (PageSource::MasterPage(k), Some(_)) = (p.source, &p.size) {
            background.insert(k, p);
        }
    }
    if edited.is_empty() && added.is_empty() {
        if !background.is_empty() {
            return Err(refuse(BACKGROUND_ALONE));
        }
        return Ok(PagesPlan::default());
    }
    let ask = Ask { styles: true, ..Ask::default() };
    let mut scanned = Vec::new();
    for (name, kind) in [("content.xml", PartKind::Content), ("styles.xml", PartKind::Styles)] {
        let bytes = source.part(name, work)?.ok_or(refuse("a page whose master page is in a part the package lacks"))?;
        if diacraft_container::xml_scan::is_utf16(&bytes) {
            return Err(refuse("a change to a part written in UTF-16"));
        }
        scanned.push(scan_part_asking(&bytes, kind, &ask, limits, work)?);
    }
    let (Some(styles), Some(content)) = (scanned.pop(), scanned.pop()) else {
        return Err(refuse("a page whose master page is in a part the package lacks"));
    };
    // Where each element of the file is after the save.
    let mut made_page: HashMap<Slot, usize> = HashMap::new();
    made_page
        .try_reserve(diff.structure.order.len())
        .map_err(|_| WriteError::Limit { what: "memory for the pages of the drawing", max: diff.structure.order.len() as u64 })?;
    for (place, slot) in diff.structure.order.iter().enumerate() {
        made_page.insert(*slot, place);
    }
    work.page_steps = work.page_steps.saturating_add(diff.structure.order.len() as u64);
    work.check_steps()?;
    // The pages the save takes away name no master page after it.
    let deleted: HashSet<usize> = diff.structure.deleted.iter().copied().collect();
    let mut users: HashMap<&str, usize> = HashMap::new();
    for (i, page) in content.pages.iter().enumerate() {
        if deleted.contains(&i) {
            continue;
        }
        if let Some(m) = page.master.as_deref() {
            *users.entry(m).or_insert(0) += 1;
        }
    }
    // **A project rule against R13**: a master page or layout that only a page taken away named is **kept**. R13 says a master page or layout that no
    // page names after a save is removed, which is written for a resize; here the count with the pages taken away already out is what the master pages were "named by
    // before", so a master page only a deleted page named reads as one nothing named before, an orphaned style that stays as it was. (A row of the check list is owed
    // for what the application does.)
    let before_users = users.clone();
    // The pages that reach each layout, through every master page that names it.
    let mut layout_pages: HashMap<&str, usize> = HashMap::new();
    let mut by_name: HashMap<&str, usize> = HashMap::new();
    for (i, m) in styles.pages.iter().enumerate() {
        if let Some(l) = m.layout.as_deref() {
            let reach = m.name.as_deref().and_then(|n| users.get(n)).copied().unwrap_or(0);
            *layout_pages.entry(l).or_insert(0) += reach;
        }
        if let Some(n) = m.name.as_deref() {
            by_name.entry(n).or_insert(i);
        }
    }
    let mut namer = Namer::of(&styles, work)?;
    // A layout is looked up by name, and the layouts' count by the same.
    let layouts: HashMap<&str, &LayoutDef> = styles.layouts.iter().map(|l| (l.name.as_str(), l)).collect();
    let mut plans = Vec::new();
    let mut paired: HashSet<usize> = HashSet::new();
    for page in edited {
        let (PageSource::Page(scan_at) | PageSource::MasterPage(scan_at)) = page.source;
        let scan_page = content.pages.get(scan_at).ok_or(WriteError::Mismatch("the part holds fewer pages than the file was read with"))?;
        let master = scan_page.master.clone().ok_or(refuse("a page that names no master page"))?;
        let at = *by_name.get(master.as_str()).ok_or(refuse("a page whose master page the file lacks"))?;
        let master_page: &ScanPage = styles.pages.get(at).ok_or(refuse("a page whose master page the file lacks"))?;
        let layout = master_page.layout.clone().ok_or(refuse("a master page that names no page layout"))?;
        if !layouts.contains_key(layout.as_str()) {
            return Err(refuse("a master page whose page layout the file lacks"));
        }
        let size = page.size.as_ref().ok_or(WriteError::Mismatch("a page plan for a page whose size was not edited"))?;
        let ops = page_ops(&size.changes).map_err(WriteError::Unsupported)?;
        // Another page reaches the layout, by this master page or by another that names it.
        let shared = layout_pages.get(layout.as_str()).copied().unwrap_or(0) > 1;
        if !master_page.shapes.is_empty() && !shared {
            // A background page: the layout is edited in place only if the model's background page was made the same size.
            let same = background.get(&at).is_some_and(|bg| bg.size.as_ref().is_some_and(|b| same_new_size(size, b)));
            if !same {
                return Err(refuse(BACKGROUND_STAYS));
            }
            paired.insert(at);
        }
        let split = if shared {
            if !master_page.shapes.is_empty() {
                return Err(refuse("a page whose master page holds shapes and is used by other pages (its copy would be another background page)"));
            }
            let split = namer.copy(&master, master_page, work)?;
            // The page leaves this master page and, with it, this master page's way to the layout; the pages of the others still reach it.
            if let Some(n) = users.get_mut(master.as_str()) {
                *n = n.saturating_sub(1);
            }
            if let Some(n) = layout_pages.get_mut(layout.as_str()) {
                *n = n.saturating_sub(1);
            }
            Some(split)
        } else {
            None
        };
        plans.push(PagePlan {
            scan_page: Some(scan_at),
            made_page: made_page
                .get(&Slot::Old(scan_at))
                .copied()
                .ok_or(WriteError::Mismatch("a page that was edited is not among the pages the save leaves"))?,
            master,
            layout,
            split,
            ops,
        });
    }
    if background.keys().any(|k| !paired.contains(k)) {
        return Err(refuse(BACKGROUND_ALONE));
    }
    let new_masters = if added.is_empty() {
        HashMap::new()
    } else {
        plan_added(&added, diff, doc, &content, &styles, (&by_name, &layouts), &mut namer, &mut plans, &made_page, work)?
    };
    // What the save leaves unnamed: a master page a page named and none names now, shapeless.
    let mut unnamed_masters = Vec::new();
    for (i, m) in styles.pages.iter().enumerate() {
        let Some(name) = m.name.as_deref() else { continue };
        let named_before = before_users.get(name).copied().unwrap_or(0) > 0;
        let named_now = users.get(name).copied().unwrap_or(0) > 0;
        if named_before && !named_now && m.shapes.is_empty() && by_name.get(name) == Some(&i) {
            unnamed_masters.push((i, name.to_owned()));
        }
    }
    Ok(PagesPlan { plans, unnamed_masters, new_masters })
}

/// The master page each page the model added names (`plan`): by the layout cells of the pages that stay, or a copy.
#[allow(clippy::too_many_arguments)]
fn plan_added(
    added: &[&PageDiff],
    diff: &Diff,
    doc: &Document,
    content: &super::scan::Scan,
    styles: &super::scan::Scan,
    (by_name, layouts): (&HashMap<&str, usize>, &HashMap<&str, &LayoutDef>),
    namer: &mut Namer,
    plans: &mut Vec<PagePlan>,
    made_page: &HashMap<Slot, usize>,
    work: &mut WriteWork,
) -> Result<HashMap<usize, (usize, String)>, WriteError> {
    let shapeless = |master: &str| by_name.get(master).and_then(|at| styles.pages.get(*at)).is_some_and(|m| m.shapes.is_empty());
    // The master page each page of the file that stays has after the save, and the layout cells it has: what a new page of the same cells names.
    let mut by_cells: HashMap<String, String> = HashMap::new();
    // The plans of the pages of the file, by the place of the page's element: each page is one look.
    // (The master page each has after the save, and whether its layout is edited where it is.)
    let planned: HashMap<usize, (String, bool)> = plans
        .iter()
        .filter_map(|pl| Some((pl.scan_page?, (pl.split.as_ref().map_or(&pl.master, |s| &s.master).clone(), pl.split.is_none()))))
        .collect();
    for p in diff.pages.iter().filter(|p| p.held_at.is_some()) {
        let PageSource::Page(k) = p.source else { continue };
        work.page_steps = work.page_steps.saturating_add(1);
        let Some(scan) = content.pages.get(k) else { continue };
        let Some(model) = doc.pages.get(p.page_at) else { continue };
        let Some(master) = planned.get(&k).map(|(m, _)| m).or(scan.master.as_ref()) else {
            continue;
        };
        // A copy this save makes of a master page is one of a master page with no shapes (a page whose master page holds shapes is refused when it is planned): it is
        // not in the file yet, so the file's master pages are not asked about it.
        let copy = planned.get(&k).is_some_and(|(_, in_place)| !*in_place);
        if model.back_page.is_none()
            && (copy || shapeless(master))
            && let Ok(key) = layout_key(&model.sheet)
        {
            by_cells.entry(key).or_insert_with(|| master.clone());
        }
    }
    work.check_steps()?;
    // The page of the file that stays and has a master page with no shapes, before each page of the model (the template of a copy), and the first of them.
    let mut template_before: Vec<Option<usize>> = Vec::new();
    template_before
        .try_reserve_exact(doc.pages.len())
        .map_err(|_| WriteError::Limit { what: "memory for the pages of the drawing", max: doc.pages.len() as u64 })?;
    let mut last: Option<usize> = None;
    let mut first: Option<usize> = None;
    for (at, p) in diff.pages.iter().enumerate() {
        template_before.push(last);
        if p.held_at.is_some()
            && matches!(p.source, PageSource::Page(_))
            && let PageSource::Page(k) = p.source
            && content.pages.get(k).and_then(|s| s.master.as_deref()).is_some_and(shapeless)
        {
            last = Some(at);
            first.get_or_insert(at);
        }
    }
    // The first page the save takes away that has a master page with no shapes: the template when no page stays that has one.
    let taken: Option<(usize, usize)> = diff
        .structure
        .deleted
        .iter()
        .zip(&diff.structure.deleted_held)
        .find(|(k, _)| content.pages.get(**k).and_then(|s| s.master.as_deref()).is_some_and(shapeless))
        .map(|(k, h)| (*k, *h));
    let mut out: HashMap<usize, (usize, String)> = HashMap::new();
    for page in added {
        work.page_steps = work.page_steps.saturating_add(1);
        let made = made_page
            .get(&Slot::New(page.page_at))
            .copied()
            .ok_or(WriteError::Mismatch("a page that was added is not among the pages the save leaves"))?;
        let model = doc.pages.get(page.page_at).ok_or(WriteError::Mismatch("a page the plan names is not in the drawing"))?;
        let key = layout_key(&model.sheet).map_err(WriteError::Unsupported)?;
        if let Some(name) = by_cells.get(&key) {
            out.insert(page.page_at, (made, name.clone()));
            continue;
        }
        // A copy, of the master page of the nearest page before the new one, or else of the first, that has one with no shapes; else of one a page the save takes away
        // has (which stays in the file: the master pages are not taken out with the page).
        let (k, h) = match template_before.get(page.page_at).copied().flatten().or(first) {
            Some(template) => {
                let t = diff.pages.get(template).ok_or(WriteError::Mismatch("a page the plan names is not in the drawing"))?;
                let (PageSource::Page(k), Some(h)) = (t.source, t.held_at) else { return Err(refuse(NO_MASTER)) };
                (k, h)
            }
            None => taken.ok_or(refuse(NO_MASTER))?,
        };
        let scan = content.pages.get(k).ok_or(WriteError::Mismatch("the part holds fewer pages than the file was read with"))?;
        let master = scan.master.clone().ok_or(refuse(NO_MASTER))?;
        let at = *by_name.get(master.as_str()).ok_or(refuse("a page whose master page the file lacks"))?;
        let master_page = styles.pages.get(at).ok_or(refuse("a page whose master page the file lacks"))?;
        let layout = master_page.layout.clone().ok_or(refuse("a master page that names no page layout"))?;
        if !layouts.contains_key(layout.as_str()) {
            return Err(refuse("a master page whose page layout the file lacks"));
        }
        // What the layout of that master page has, the file's, against what the new page has.
        let base_sheet = diff.structure.base_sheets.get(h).ok_or(WriteError::Mismatch("the plan has no sheet for a page of the file"))?;
        let template_page = diacraft_doc::Page { sheet: std::sync::Arc::clone(base_sheet), ..diacraft_doc::Page::new(0, "") };
        let edit = super::compare::page_edit(model, &template_page, &mut work.cells_compared)?;
        let ops = match &edit {
            Some(e) => page_ops(&e.changes).map_err(WriteError::Unsupported)?,
            None => Vec::new(),
        };
        // The master page of the file is the template's own, as the file has it, when this save does not touch the page: a page that is resized has its master page's layout
        // edited where it is, or has a copy made, and a layout that a page on another master page edits in place is no longer the size the template's page had (the two
        // master pages name one layout). The template page that stays as it is still names its master page, so no master page is left unnamed by the join.
        if edit.is_none() && !planned.contains_key(&k) {
            by_cells.insert(key, master.clone());
            out.insert(page.page_at, (made, master));
            continue;
        }
        let split = namer.copy(&master, master_page, work)?;
        by_cells.insert(key, split.master.clone());
        out.insert(page.page_at, (made, split.master.clone()));
        plans.push(PagePlan { scan_page: None, made_page: made, master, layout, split: Some(split), ops });
    }
    work.check_steps()?;
    Ok(out)
}

/// Whether two size edits make the same cells the same: the model's page and its background page, resized together.
fn same_new_size(a: &PageEdit, b: &PageEdit) -> bool {
    a.changes.len() == b.changes.len()
        && a.changes
            .iter()
            .all(|(cell, v)| b.changes.iter().any(|(other, w)| other == cell && v.as_ref().map(|c| c.number) == w.as_ref().map(|c| c.number)))
}

impl Editor<'_, '_> {
    /// Points the `draw:page` at `page` to the master page `name`.
    pub(crate) fn set_master(&mut self, page: &ScanPage, name: &str) -> Result<(), WriteError> {
        let mut tag = TagEdit::untracked(self.bytes, page.open, page.scope, self.work)?;
        self.work.charge_attribute(self.limits)?;
        match tag.find(&mut self.scopes, Ns::Draw, "master-page-name", self.work) {
            Some(at) => tag.set(&mut self.splicer, at, name),
            None => tag.add(&mut self.splicer, &mut self.scopes, self.work, Ns::Draw, "master-page-name", name),
        }
    }

    /// Writes the plans into `styles.xml`: for each, the layout where it is, or the copies of the master page and the layout; and takes out the master
    /// pages the save leaves unnamed. Gives the places of the master pages (by their order among the part's) that a copy was put
    /// after and those taken out, which shift those after them.
    pub(crate) fn write_pages(
        &mut self,
        pages: &PagesPlan,
        masters: &[ScanPage],
        layouts: &[LayoutDef],
    ) -> Result<(Vec<usize>, Vec<usize>), WriteError> {
        let plans = &pages.plans;
        let mut inserted = Vec::new();
        // The layouts and the master pages are indexed by name once, and each plan is one look in each, charged as a step.
        let layout_at: HashMap<&str, usize> = layouts.iter().enumerate().rev().map(|(i, l)| (l.name.as_str(), i)).collect();
        let master_at: HashMap<&str, usize> = masters.iter().enumerate().rev().filter_map(|(i, m)| Some((m.name.as_deref()?, i))).collect();
        for plan in plans {
            if !self.work.step() || !self.work.step() {
                self.work.check_steps()?;
            }
            let layout = layout_at
                .get(plan.layout.as_str())
                .and_then(|i| layouts.get(*i))
                .ok_or(WriteError::Mismatch("a layout the plan names is not in the part"))?;
            let props = layout.props.ok_or(refuse("a page layout with no properties element"))?;
            let mut wrote = Vec::new();
            let Some(split) = &plan.split else {
                let source = self.bytes;
                let mut tag = TagEdit::untracked(source, props.open, props.scope, self.work)?;
                let mut ctx = Ctx { scopes: &mut self.scopes, work: &mut *self.work, limits: self.limits, unit: self.unit };
                ctx.write_ops(&mut tag, &mut self.splicer, &plan.ops, &mut wrote)?;
                continue;
            };
            // The layout's copy, with the edit made in it, put after the layout.
            let unit = self.bytes.get(layout.lead..layout.end).ok_or(WriteError::Mismatch("a layout that is not in the part"))?;
            let shift = |p: (usize, usize)| (p.0.saturating_sub(layout.lead), p.1.saturating_sub(layout.lead));
            let mut sub = Splicer::new(unit);
            let name_at = shift(layout.name_value);
            sub.replace(name_at.0, name_at.1, escape_value(&split.layout, b'"'));
            let mut tag = TagEdit::untracked(unit, shift(props.open), props.scope, self.work)?;
            self.ctx().write_ops(&mut tag, &mut sub, &plan.ops, &mut wrote)?;
            let copy = String::from_utf8(sub.apply(self.work, self.limits)?).map_err(|_| WriteError::Mismatch("a layout that is not text"))?;
            self.splicer.insert(layout.end, copy);
            // The master page's copy, naming the new layout, put after the master page.
            let at = *master_at.get(plan.master.as_str()).ok_or(WriteError::Mismatch("a master page the plan names is not in the part"))?;
            let master = masters.get(at).ok_or(WriteError::Mismatch("a master page the plan names is not in the part"))?;
            let unit = self.bytes.get(master.lead..master.end).ok_or(WriteError::Mismatch("a master page that is not in the part"))?;
            let shift = |p: (usize, usize)| (p.0.saturating_sub(master.lead), p.1.saturating_sub(master.lead));
            let mut sub = Splicer::new(unit);
            let mut tag = TagEdit::untracked(unit, shift(master.open), master.scope, self.work)?;
            for (local, value) in
                [("name", split.master.as_str()), ("display-name", split.display.as_str()), ("page-layout-name", split.layout.as_str())]
            {
                self.work.charge_attribute(self.limits)?;
                match tag.find(&mut self.scopes, Ns::Style, local, self.work) {
                    Some(found) => tag.set(&mut sub, found, value)?,
                    None => tag.add(&mut sub, &mut self.scopes, self.work, Ns::Style, local, value)?,
                }
            }
            let copy = String::from_utf8(sub.apply(self.work, self.limits)?).map_err(|_| WriteError::Mismatch("a master page that is not text"))?;
            self.splicer.insert(master.end, copy);
            inserted.push(at);
        }
        let mut removed = Vec::new();
        for (i, _) in &pages.unnamed_masters {
            let master = masters.get(*i).ok_or(WriteError::Mismatch("a master page the plan takes out is not in the part"))?;
            self.splicer.remove(master.lead, master.end);
            removed.push(*i);
        }
        Ok((inserted, removed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_says_where_the_edit_goes_and_a_split_names_its_copies() {
        let split = Split { master: "Default_2".into(), display: "Default 2".into(), layout: "PM2".into() };
        let plan = PagePlan {
            scan_page: Some(1),
            made_page: 1,
            master: "Default".into(),
            layout: "PM1".into(),
            split: Some(split.clone()),
            ops: Vec::new(),
        };
        assert_eq!(plan.split.as_ref(), Some(&split));
        assert_eq!(refuse("x"), WriteError::Unsupported("x"));
    }

    fn named(name: &str) -> ScanPage {
        ScanPage { name: Some(name.to_owned()), display: Some(name.to_owned()), ..ScanPage::default() }
    }

    /// The copy of a master page is named for the lowest number from 2 that no master page has after its name, each copy goes on from the one before, and the layout of
    /// each is the next free name in its prefix.
    #[test]
    fn the_name_of_a_copy_of_a_master_page_is_the_lowest_number_from_2_in_no_name_and_each_copy_goes_on() {
        let mut scan = super::super::scan::Scan::default();
        for name in ["Default", "Default_2", "Default_4", "Other_3"] {
            scan.pages.push(named(name));
        }
        let mut work = WriteWork::default();
        let mut namer = Namer::of(&scan, &mut work).unwrap();
        assert_eq!(work.master_name_tries, 4, "each name looked at once");
        let master = named("Default");
        let first = namer.copy("Default", &master, &mut work).unwrap();
        assert_eq!((first.master.as_str(), first.display.as_str()), ("Default_3", "Default 3"));
        let second = namer.copy("Default", &master, &mut work).unwrap();
        assert_eq!((second.master.as_str(), second.display.as_str()), ("Default_5", "Default 5"), "4 is in use, and the cursor goes on from 3");
        assert_ne!(first.layout, second.layout);
        assert!(first.layout.starts_with("PM") && second.layout.starts_with("PM"));
        // Another master page's copies are numbered apart, from 2 again, and a display name that is none is the name.
        let other = namer.copy("Other", &ScanPage { name: Some("Other".into()), ..ScanPage::default() }, &mut work).unwrap();
        assert_eq!((other.master.as_str(), other.display.as_str()), ("Other_2", "Other 2"));
        // A copy whose name a master page already has, under no number the names found hold, is refused.
        let mut scan = super::super::scan::Scan::default();
        scan.pages.push(named("Default"));
        let mut namer = Namer::of(&scan, &mut work).unwrap();
        namer.taken.insert("Default_2".to_owned());
        assert!(matches!(namer.copy("Default", &named("Default"), &mut work), Err(WriteError::Unsupported(w)) if w.contains("repeat")));
    }

    /// The layout cells of two pages make one key when each cell is the same number in the same unit, and two when one is another number, another unit or missing; a cell with
    /// a formula has none.
    #[test]
    fn two_pages_have_one_layout_key_only_when_their_seven_cells_are_the_same() {
        use diacraft_doc::{Cell, CellName, Sheet, Value};
        let sheet = |cells: &[(&str, &str, Option<&str>)]| {
            let mut s = Sheet::default();
            for (name, value, unit) in cells {
                let mut cell = Cell::constant(Value::from_text(value));
                if let Some(unit) = unit {
                    cell = cell.with_unit(unit);
                }
                s.cells.insert(CellName::new(name), cell);
            }
            s
        };
        let base = [("PageWidth", "11", Some("IN")), ("PageHeight", "8.5", Some("IN")), ("PageLeftMargin", "0", Some("IN"))];
        let key = |cells: &[(&str, &str, Option<&str>)]| layout_key(&sheet(cells)).unwrap();
        assert_eq!(key(&base), key(&base));
        assert_ne!(key(&base), key(&[("PageWidth", "12", Some("IN")), base[1], base[2]]), "another number");
        assert_ne!(key(&base), key(&[("PageWidth", "11", Some("CM")), base[1], base[2]]), "another unit");
        assert_ne!(key(&base), key(&[base[0], base[1]]), "a margin missing");
        assert_ne!(key(&base), key(&[base[0], base[1], ("PageRightMargin", "0", Some("IN"))]), "another cell");
        assert_eq!(key(&base), key(&[base[2], base[0], base[1]]), "the cells are taken in the order of the layout, whatever order they were put in");
        // A cell that is not a layout cell is no part of the key.
        assert_eq!(key(&base), key(&[base[0], base[1], base[2], ("DrawingScale", "5", None)]));
        let mut formula = sheet(&base);
        formula.cells.insert(CellName::new("PageWidth"), Cell::read(Some("1+10"), Some("11"), Some("IN"), None, &mut 100));
        assert!(layout_key(&formula).is_err(), "a formula is no layout's");
    }
}
