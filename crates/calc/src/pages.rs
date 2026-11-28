//! The cells of other pages that read an edit (gap F40; for the rest).
//!
//! **The gap.** The graph of the readers of a page's cells ([`crate::ReaderIndex`], `index.rs`) is made for one page, the page an edit is on, so a cell of page 2 that names a cell of
//! page 1 (`Pages[Page-1]!Sheet.10!Width`) is found by no write to the cell it reads, and kept the value it had until its own page was worked out. A graph of every page would fix that, and
//! is not affordable: a document of a hundred pages of twelve thousand shapes would walk and classify a million and a half formulas to answer the first edit.
//!
//! **What is kept instead** is [`PageLinks`]: for each page, the sheets that hold a formula that names a page (a `Pages[..]` before a reference), and **which page names they use**
//! ([`Designator`]), found by a walk that looks at the text of each formula and at the tree of those that may matter, a step each. It holds no cell and **resolves nothing**: what a
//! `Pages[Site-2]!Sheet.9!EndY` reaches is decided when it is asked, against the document as it is then (a shape added before the one it names moves its place, a page renamed leaves the
//! name, and a reader is never left reading where a shape was). A document of one page has none for its pages, and one whose formulas name no page holds an empty list for each page and
//! costs a pointer comparison for each page to a commit that finds the pages as they were.
//!
//! **What else the listing holds**, so that no formula that can read a page is left out:
//!
//! * **the sheets no page holds**: the document's sheet, each style's, each master's page sheet and the shapes of each master. A cell of those that names a page is a reader as any is, and
//!   it is worked out when the cell it reads changes. They are kept by the allocation of what they were made for, put right for the master or the style an edit replaced and no other;
//! * **the formulas an instance inherits** from a master shape that names a page: the instance's sheet is listed with the names its master shape holds, so the instance reads the page
//!   through the formula it inherits as it reads one of its own. **Each instance is charged for each name it inherits** (a place in the index of names for each: the names are shared, the places are
//!   not), so that the memory of a listing is what it is charged, and a master that names thousands of pages with thousands of instances ends the commit as out of memory before the index is built. This is looked at only for a drawing in which some master shape names a page, and a page's listing made before the
//!   masters changed in that way is made again ([`LinkGlobal::epoch`]);
//! * **the formulas that read at run time** (`EVALTEXT`, `EVALCELL`): their text does not say which page they read, so a sheet that holds one is read as **naming every page** ([`Named::wild`]),
//!   and its cells that read at run time are worked out once, after the first change to a cell of any page ([`PageLinks::wild_readers`]). The other calls whose reads the text does not name
//!   (`LISTSHEETREF`, `CONTAINERSHEETREF` and the like) read the shapes of their own page, which the rule of the page already follows;
//! * **the formulas that read the list of pages** (`PAGENAME()`, `PAGENUMBER()`, `PAGECOUNT()`, `BKGPAGENAME()`): no name in their text, and no change to a cell reaches them, but a page
//!   renamed, added, taken away, moved or made a background does ([`Named::list`], [`PageNames`]). A page that is **only renamed** gives another reading to `PAGENAME()` and `BKGPAGENAME()` and not to
//!   `PAGENUMBER()` or `PAGECOUNT()`, which read the order of the pages ([`ListReads`]), so the value one of those holds is kept.
//!
//! **What else it holds**, so that the last of the cells that read what a commit worked out is found:
//!
//! * **the sheets that name a sheet no page holds** (`TheDoc!`, `Styles!Name!`, `Masters[Name]!`: [`Unpaged`], and [`Target`] for what a change is read through): a cell of the document's sheet, of a
//!   style or of a master (or its shapes) that a commit worked out is read by those sheets **on every page**, found by the listing as the cells that name a page are, and classified for the commit
//!   ([`CrossMap::make`]) when first asked. A sheet of the commit's own page is answered by that page's graph and left out of the answer ([`PageLinks::naming_unpaged`]);
//! * **the formulas a sheet inherits from a style**: a cell a sheet holds as `Inh` whose formula the chain finds on a style is that formula, worked out on the sheet. [`StyleReach`] says which styles hold a
//!   formula the listing or the graph looks at (and which are based on one), and [`style_given`] asks a sheet for its inherited cells **only if a link of the sheet (or of its master shape) names such a
//!   style**, a step each, so a drawing whose styles hold none asks nothing of any sheet. The listing of a page made before a style changed is made again ([`LinkGlobal::epoch`]) when a style gave a sheet such a
//!   formula before the change or does after it, and a sheet given another link is classified again (`index::same_formulas`).
//!
//! **How it is kept up to date** ([`PageLinks::sync`]), whatever the edits since it was made: a page is looked at by pointer first, and a page that is another allocation than the one it was
//! made for is put right by what the edit replaced (the shapes of the page are walked beside the shapes the list was made for, skipping every list and every shape that is the same allocation, as the
//! reader index does, `index::replaced_in`); a sheet that was replaced and whose formulas are as they were (an edit that writes values: a move, a fill, the results of a recalculation) changes
//! nothing in it; a page whose shapes are not the same shapes in the same places (one added, taken out, moved) is made again whole, as the graph of a page is.
//!
//! **How it is used** ([`crate::affected`]): when a cell changes, the cells that read it on other pages are the readers in the sheets that name its page. Those sheets are classified when
//! first asked, for one commit, by the classification of a page's graph (`affected::classify_sheet`), and the entries that name a cell of the page are what [`CrossMap`] keeps. A formula the
//! listing does not name is a formula that names no page: **no formula that names no page reads another page** but the ones the list above says. When the pages themselves change, [`PageNames`]
//! says which names found another page (or none) and [`PageLinks::readers_naming`] the cells that name them, which [`commit_changed_across`] works out in the one commit.
//!
//! **Not followed.** A cell that reads a style's or a master's cell **by inheriting its value** (it holds no formula of the cell and takes the value from the style or the master shape, and a cell of its own
//! sheet reads it by its plain name) is worked out only when its own cell is: a chain that changes a style's or a master shape's cell reaches the sheets that name it (`Styles!`, `Masters[..]!`) and the cells
//! that inherit its formula, and not the cells of a sheet that merely read the value it passes down. A renamed master or style is not a change to the names a formula finds it by (a page's is, [`PageNames`]).
//! A string a cell holds and another reads **as a formula** (a text converted to a number) is read where it is found.

use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use diacraft_doc::{CellAddr, Document, FormulaSlot, Links, Master, Owner, Page, ShapeRef, Sheet, SheetRef, Source, Style};
use diacraft_formula::{Call, Expr, Formula, Index, Qualifier};

use crate::affected::{Found, Key, MasterCross, Reader, classify_sheet, page_of, reaches_beyond_own_sheet};
use crate::changes::{Changes, SectionFormulas, each_formula};
use crate::commit::{CommitReport, commit_changed_with};
use crate::eval::{Budget, EvalCtx, EvalOptions, EvalStats, Evaluator};
use crate::index::{ReaderIndex, Replaced, replaced_in, same_formulas};
use crate::recalc::{walk_master, walk_page};
use crate::rules::{self, KEY_BYTES};

/// How a formula names a page: `Pages[Site-2]` is the name `Site-2` (a page by its `NameU` or its `Name`), and `Pages[2]` the number 2 (a page by its `NameU`, which is the digits,
/// `names.rs`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Designator {
    number: bool,
    text: Arc<str>,
}

impl Designator {
    fn of(index: &Index) -> Designator {
        match index {
            Index::Name(name) => Designator { number: false, text: Arc::clone(name) },
            Index::Number(n) => Designator { number: true, text: Arc::from(n.to_string()) },
        }
    }

    /// The designators that can name `page`: its `NameU` and its `Name` as names, and its `NameU` as a number.
    fn naming(page: &Page) -> Vec<Designator> {
        let mut out: Vec<Designator> = Vec::with_capacity(3);
        for (number, text) in [(false, page.name_u.as_ref()), (false, Some(&page.name)), (true, page.name_u.as_ref())] {
            if let Some(text) = text {
                let one = Designator { number, text: Arc::clone(text) };
                if !out.contains(&one) {
                    out.push(one);
                }
            }
        }
        out
    }
}

/// Which of the sheets no page holds a formula names: the document's (`TheDoc!`), a style's (`Styles!Name!`) and a master's (`Masters[Name]!`). A change to a cell of one of those is read by the
/// sheets that name it, on every page, so the listing says which sheets do, as it says which name a page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Unpaged {
    doc: bool,
    styles: bool,
    masters: bool,
}

impl Unpaged {
    /// Every sheet no page holds.
    const ALL: Unpaged = Unpaged { doc: true, styles: true, masters: true };

    fn any(self) -> bool {
        self.doc || self.styles || self.masters
    }

    fn or(self, other: Unpaged) -> Unpaged {
        Unpaged { doc: self.doc || other.doc, styles: self.styles || other.styles, masters: self.masters || other.masters }
    }

    fn and(self, other: Unpaged) -> Unpaged {
        Unpaged { doc: self.doc && other.doc, styles: self.styles && other.styles, masters: self.masters && other.masters }
    }
}

/// The sheets a change to a cell is read through, by what a formula names them by: the sheets of one page (`Pages[..]`), the document's, the styles' or the masters'.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Target {
    Page(u32),
    Doc,
    Styles,
    Masters,
}

impl Target {
    /// The target a sheet is one of.
    pub(crate) fn of(sheet: &SheetRef) -> Target {
        match sheet {
            SheetRef::Page(id) => Target::Page(*id),
            SheetRef::Shape(at) => match at.owner {
                Owner::Page(id) => Target::Page(id),
                Owner::Master(_) => Target::Masters,
            },
            SheetRef::Document => Target::Doc,
            SheetRef::Style(_) => Target::Styles,
            SheetRef::Master(_) => Target::Masters,
        }
    }
}

/// What the formulas of a sheet say of the pages they read: the listing's entry for the sheet (and what one formula says, for a cell).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Named {
    /// The pages the formulas name, each once, in the order met. Shared by the instances that inherit them from a master shape.
    names: Arc<[Designator]>,
    /// A formula calls `EVALTEXT` or `EVALCELL`: it works a text out as a formula, which may name any page. The sheet is read as naming every page.
    wild: bool,
    /// A formula calls `PAGENAME`, `PAGENUMBER`, `PAGECOUNT` or `BKGPAGENAME`: it reads the list of pages with no name in its text, the names of the pages or their order.
    list: ListReads,
    /// A formula names the document's sheet, a style's or a master's.
    unpaged: Unpaged,
}

impl Named {
    fn is_empty(&self) -> bool {
        self.names.is_empty() && !self.wild && !self.list.any() && !self.unpaged.any()
    }

    /// What the entry is charged, as accounted: its place and a quarter of one for each name.
    fn bytes(&self) -> u64 {
        KEY_BYTES.saturating_add((self.names.len() as u64).saturating_mul(KEY_BYTES / 4))
    }

    /// What this says of the sheets no page holds that `gates` lets through.
    fn masked(self, gates: Unpaged) -> Named {
        Named { unpaged: self.unpaged.and(gates), ..self }
    }

    /// What this and `other` say together.
    fn with(&self, other: &Named) -> Named {
        let mut names: Vec<Designator> = self.names.to_vec();
        let mut known: HashSet<&Designator> = self.names.iter().collect();
        for one in other.names.iter() {
            if known.insert(one) {
                names.push(one.clone());
            }
        }
        Named { names: names.into(), wild: self.wild || other.wild, list: self.list.or(other.list), unpaged: self.unpaged.or(other.unpaged) }
    }
}

/// The calls that work a text out as a formula, whose reads are decided when they run.
const RUN_TIME_FUNCTIONS: [&str; 2] = ["EVALTEXT", "EVALCELL"];

/// The calls that read the names of the pages (`BKGPAGENAME`: the name of the background page a page shows).
const NAME_FUNCTIONS: [&str; 2] = ["PAGENAME", "BKGPAGENAME"];

/// The calls that read the order of the pages, their count, which are backgrounds and which background a page shows.
const ORDER_FUNCTIONS: [&str; 3] = ["PAGENUMBER", "PAGECOUNT", "BKGPAGENAME"];

/// What a formula reads of the list of pages, with no name of a page in its text: **the names of the pages** (`PAGENAME`, `BKGPAGENAME`) and **their order** (`PAGENUMBER`, `PAGECOUNT`, and
/// `BKGPAGENAME`, which reads which background a page shows). A page that is only renamed gives another reading to the names and to no order, so a formula that reads only the order is not worked out for it, and the
/// value it holds (a file's own cached value) is kept; a page added, taken away, moved or made a background changes both, as it always did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ListReads {
    names: bool,
    order: bool,
}

impl ListReads {
    /// Whether a formula reads the list at all.
    fn any(self) -> bool {
        self.names || self.order
    }

    /// What this and `other` read together.
    fn or(self, other: ListReads) -> ListReads {
        ListReads { names: self.names || other.names, order: self.order || other.order }
    }

    /// What a call reads, by its name.
    fn of(call: &Call) -> ListReads {
        ListReads { names: NAME_FUNCTIONS.iter().any(|f| call.is(f)), order: ORDER_FUNCTIONS.iter().any(|f| call.is(f)) }
    }

    /// Whether a change to the list, `changed` (what it gave another reading), gave a formula that reads `self` another reading.
    fn is_changed_by(self, changed: ListReads) -> bool {
        (self.names && changed.names) || (self.order && changed.order)
    }
}

/// What the formula one call or reference is part of says, from the qualifiers before it and the name of the call.
struct Reading {
    names: Vec<Designator>,
    wild: bool,
    list: ListReads,
    unpaged: Unpaged,
}

/// Adds the pages the qualifiers name, each once (`known` says which are in `out`: a formula may name thousands, and a search of the list for each would be quadratic), and the arguments of the
/// calls among them to `stack`.
fn note_qualifiers<'a>(qualifiers: &'a [Qualifier], stack: &mut Vec<&'a Expr>, out: &mut Reading, known: &mut HashSet<Designator>) {
    for q in qualifiers {
        match q {
            Qualifier::Path(segments) => {
                if let [one] = segments.as_slice() {
                    if one.name.eq_ignore_ascii_case("Pages")
                        && let Some(index) = &one.index
                    {
                        let named = Designator::of(index);
                        if known.insert(named.clone()) {
                            out.names.push(named);
                        }
                    } else if one.index.is_none() && one.name.eq_ignore_ascii_case("TheDoc") {
                        out.unpaged.doc = true;
                    } else if one.index.is_none() && one.name.eq_ignore_ascii_case("Styles") {
                        out.unpaged.styles = true;
                    } else if one.index.is_some() && one.name.eq_ignore_ascii_case("Masters") {
                        out.unpaged.masters = true;
                    }
                }
            }
            Qualifier::Call { args, .. } => stack.extend(args.iter().flatten()),
        }
    }
}

/// What a call says by its name.
fn note_call(call: &Call, out: &mut Reading) {
    out.wild |= RUN_TIME_FUNCTIONS.iter().any(|f| call.is(f));
    out.list = out.list.or(ListReads::of(call));
}

/// What a formula reads of the pages: every `Pages[..]` before a reference or a call, anywhere in the tree, and the calls that read at run time or the list of pages. Walked with a stack of
/// its own, so a tree of any depth is walked.
pub(crate) fn read_by(expr: &Expr) -> Named {
    let mut out = Reading { names: Vec::new(), wild: false, list: ListReads::default(), unpaged: Unpaged::default() };
    let mut known: HashSet<Designator> = HashSet::new();
    let mut stack: Vec<&Expr> = vec![expr];
    while let Some(e) = stack.pop() {
        match e {
            Expr::Ref(r) => note_qualifiers(&r.qualifiers, &mut stack, &mut out, &mut known),
            Expr::Call(c) => {
                note_qualifiers(&c.qualifiers, &mut stack, &mut out, &mut known);
                note_call(c, &mut out);
                stack.extend(c.args.iter().flatten());
            }
            Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
            Expr::Binary(_, a, b) => {
                stack.push(a);
                stack.push(b);
            }
            Expr::Number(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Color { .. } | Expr::Currency { .. } | Expr::Guid(_) => {}
        }
    }
    Named { names: out.names.into(), wild: out.wild, list: out.list, unpaged: out.unpaged }
}

/// Whether the text of a formula can say what the listing looks for, which a scan of its bytes tells without the tree: an index cannot be written without a `[`, the calls it looks for hold
/// `EVAL` or `PAGEN` or `PAGEC`, and the sheets no page holds are named by `TheDoc` or `Styles`, in any letter case, **when `look` says they are worth listing** (a text that names only
/// `TheDoc` is not looked at, and costs nothing, in a drawing whose document sheet reads nothing beyond itself: [`Inherits`]).
pub(crate) fn worth_a_look(source: &str, look: Unpaged) -> bool {
    let bytes = source.as_bytes();
    bytes.contains(&b'[')
        || bytes.windows(4).any(|w| w.eq_ignore_ascii_case(b"eval"))
        || bytes
            .windows(5)
            .any(|w| w.get(..4).is_some_and(|f| f.eq_ignore_ascii_case(b"page")) && matches!(w.get(4), Some(b'n' | b'N' | b'c' | b'C')))
        || ((look.doc || look.styles)
            && bytes.windows(6).any(|w| (look.doc && w.eq_ignore_ascii_case(b"thedoc")) || (look.styles && w.eq_ignore_ascii_case(b"styles"))))
}

/// **The formulas a sheet inherits from a style** (as the ones it inherits from a master shape are): a cell the sheet holds as `Inh` whose formula the chain finds on a style is that formula,
/// worked out on the sheet. The styles that hold a formula the listing or the graph looks at (one that names a page, the document's sheet, the styles or the masters, reads at run time or reads the
/// list of pages, or reads beyond its sheet), and the styles based on one, are what a sheet's links are looked at against: **a sheet no link of which names such a style is not asked for its
/// inherited cells at all**, so a drawing whose styles hold no such formula costs nothing here.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct StyleReach {
    ids: HashSet<u32>,
}

/// Whether a sheet holds a formula that the listing or the classification looks at: charged by the text of each formula and its nodes, and left as soon as one is found.
fn holds_a_formula_looked_at(ev: &mut Evaluator<'_, '_>, sheet: &Sheet) -> bool {
    let mut found = false;
    let mut text_bytes = 0_usize;
    each_formula(sheet, &mut SectionFormulas::default(), |_, formula| {
        let Some(formula) = formula else { return };
        text_bytes = text_bytes.saturating_add(formula.source.len());
        if found || ev.ctx.stats.budget_exhausted {
            return;
        }
        found = !scan_formula(ev, formula, Unpaged::ALL).is_empty() || reaches_beyond_own_sheet(ev, formula);
    });
    let _ = ev.steps(text_bytes.div_ceil(rules::UNITS_PER_STEP));
    found
}

impl StyleReach {
    /// The styles of the evaluator's document that hold a formula looked at, and those based on one (by any of their three links, which name the styles they are based on). A step for each
    /// style and for each one reached, its place in the set held memory.
    fn of(ev: &mut Evaluator<'_, '_>) -> StyleReach {
        let doc = ev.doc;
        let mut seen: HashSet<u32> = HashSet::new();
        let mut based_on: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut direct: Vec<u32> = Vec::new();
        for style in doc.styles.iter() {
            // A style whose `ID` an earlier style has is not addressable.
            if !seen.insert(style.id) {
                continue;
            }
            if !ev.steps(1) || !ev.ctx.hold_bytes(KEY_BYTES) {
                break;
            }
            let links = style.sheet.links;
            for parent in [links.line, links.fill, links.text].into_iter().flatten() {
                based_on.entry(parent).or_default().push(style.id);
            }
            if holds_a_formula_looked_at(ev, &style.sheet) {
                direct.push(style.id);
            }
        }
        let mut ids: HashSet<u32> = HashSet::new();
        let mut queue: VecDeque<u32> = VecDeque::new();
        for id in direct {
            if ids.insert(id) {
                queue.push_back(id);
            }
        }
        while let Some(id) = queue.pop_front() {
            if !ev.steps(1) {
                break;
            }
            for child in based_on.get(&id).into_iter().flatten() {
                if ids.insert(*child) {
                    queue.push_back(*child);
                }
            }
        }
        StyleReach { ids }
    }

    /// Whether no style gives a formula that is looked at.
    pub(crate) fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Whether `sheet` (whose cells are `data`) can be given a formula by such a style: one of its links names one, or, for a shape that is an instance, one of its master shape's links does (a shape
    /// with no link of a family has its master shape's), or the root style does (the cells with no family are read from it).
    pub(crate) fn gives(&self, ev: &mut Evaluator<'_, '_>, sheet: &SheetRef, data: &Sheet) -> bool {
        if self.ids.is_empty() {
            return false;
        }
        let named = |links: &Links| [links.line, links.fill, links.text].into_iter().flatten().any(|id| self.ids.contains(&id));
        if self.ids.contains(&0) || named(&data.links) {
            return true;
        }
        let SheetRef::Shape(at) = sheet else { return false };
        if !ev.steps(1) {
            return false;
        }
        let Ok(Some(step)) = ev.res.master_step(at) else { return false };
        ev.res.sheet(&SheetRef::Shape(step.shape)).is_some_and(|master| named(&master.links))
    }
}

impl Evaluator<'_, '_> {
    /// The styles that can give a sheet a formula that is looked at: found when first asked, for this evaluator.
    pub(crate) fn style_reach(&mut self) -> Rc<StyleReach> {
        if let Some(known) = &self.style_reach {
            return Rc::clone(known);
        }
        let made = Rc::new(StyleReach::of(self));
        self.style_reach = Some(Rc::clone(&made));
        made
    }
}

/// The formulas `sheet` gets from a style for the cells `inherited` (which it holds as `Inh`): each is resolved, a step for each, and kept when the chain finds a formula of its own on a style
/// (a formula found on a master shape is the instance's through the master shape, which has its own rule).
pub(crate) fn style_formulas(ev: &mut Evaluator<'_, '_>, sheet: &SheetRef, inherited: &[CellAddr]) -> Vec<(CellAddr, Arc<Formula>)> {
    let mut out: Vec<(CellAddr, Arc<Formula>)> = Vec::new();
    for addr in inherited {
        if !ev.steps(1) {
            break;
        }
        let Some(found) = ev.res.resolve(sheet, addr) else { continue };
        if !matches!(found.formula.source, Source::Style { .. }) {
            continue;
        }
        if let Some(cell) = found.formula.cell
            && let FormulaSlot::Local(formula) = &cell.formula
        {
            out.push((addr.clone(), Arc::clone(formula)));
        }
    }
    out
}

/// What one formula reads of the pages: nothing for nearly every formula, whose text is looked at for a `[` or a call's name and no more; a formula that may matter is walked, a step a node.
fn scan_formula(ev: &mut Evaluator<'_, '_>, formula: &Formula, look: Unpaged) -> Named {
    if ev.ctx.stats.budget_exhausted || !worth_a_look(&formula.source, look) {
        return Named::default();
    }
    let Ok(expr) = &formula.ast else { return Named::default() };
    if !ev.steps(expr.node_count()) {
        return Named::default();
    }
    read_by(expr)
}

/// What the formulas a sheet reads of the pages, put together: each name once, in the order met.
#[derive(Default)]
struct Gather {
    names: Vec<Designator>,
    known: HashSet<Designator>,
    wild: bool,
    list: ListReads,
    unpaged: Unpaged,
}

impl Gather {
    fn add(&mut self, one: &Named) {
        for d in one.names.iter() {
            if self.known.insert(d.clone()) {
                self.names.push(d.clone());
            }
        }
        self.wild |= one.wild;
        self.list = self.list.or(one.list);
        self.unpaged = self.unpaged.or(one.unpaged);
    }

    fn done(self) -> Named {
        Named { names: self.names.into(), wild: self.wild, list: self.list, unpaged: self.unpaged }
    }
}

/// The formulas a sheet gets from a style for the cells it holds as `Inh`, with their cells: none, and no look at the sheet's cells, unless a link of the sheet names a style that holds
/// a formula that is looked at ([`StyleReach::gives`]).
pub(crate) fn style_given(ev: &mut Evaluator<'_, '_>, sheet: &SheetRef, data: &Sheet, memo: &mut SectionFormulas) -> Vec<(CellAddr, Arc<Formula>)> {
    let reach = ev.style_reach();
    if reach.is_empty() || !reach.gives(ev, sheet, data) {
        return Vec::new();
    }
    let mut inherited: Vec<CellAddr> = Vec::new();
    each_formula(data, memo, |addr, formula| {
        if formula.is_none() {
            inherited.push(addr.clone());
        }
    });
    style_formulas(ev, sheet, &inherited)
}

/// What the formulas a sheet holds, and those it gets from a style, read of the pages, each once: nothing for nearly every sheet. **A step for each 64 bytes of formula text a sheet holds**, and a
/// formula that may matter is walked for what it reads, a step a node.
fn scan_sheet(ev: &mut Evaluator<'_, '_>, sheet: &SheetRef, data: &Sheet, memo: &mut SectionFormulas, look: Unpaged) -> Named {
    let mut gathered = Gather::default();
    let mut text_bytes = 0_usize;
    each_formula(data, memo, |_, formula| {
        let Some(formula) = formula else { return };
        text_bytes = text_bytes.saturating_add(formula.source.len());
        gathered.add(&scan_formula(ev, formula, look));
    });
    for (_, formula) in style_given(ev, sheet, data, memo) {
        text_bytes = text_bytes.saturating_add(formula.source.len());
        gathered.add(&scan_formula(ev, &formula, look));
    }
    // A budget this spends is read by the caller from the evaluator (`stats.budget_exhausted`), as for every walk of the page.
    let _ = ev.steps(text_bytes.div_ceil(rules::UNITS_PER_STEP));
    gathered.done()
}

/// The cells of a sheet whose formulas (held, or given by a style) read something the listing looks for, each with what it reads: found by the walk of [`scan_sheet`], for the readers of a change to the pages.
fn scan_cells(ev: &mut Evaluator<'_, '_>, sheet: &SheetRef, data: &Sheet, memo: &mut SectionFormulas) -> Vec<(CellAddr, Named)> {
    // What a change to the pages gave another reading is a name a page is found by, a call that reads at run time or the list of pages: the sheets no page holds are not looked for.
    let look = Unpaged::default();
    let mut out: Vec<(CellAddr, Named)> = Vec::new();
    let mut text_bytes = 0_usize;
    each_formula(data, memo, |addr, formula| {
        let Some(formula) = formula else { return };
        text_bytes = text_bytes.saturating_add(formula.source.len());
        let one = scan_formula(ev, formula, look);
        if !one.is_empty() {
            out.push((addr.clone(), one));
        }
    });
    for (addr, formula) in style_given(ev, sheet, data, memo) {
        text_bytes = text_bytes.saturating_add(formula.source.len());
        let one = scan_formula(ev, &formula, look);
        if !one.is_empty() {
            out.push((addr, one));
        }
    }
    let _ = ev.steps(text_bytes.div_ceil(rules::UNITS_PER_STEP));
    out
}

/// Where a sheet is in a walk of its page: the page's own sheet, then the shapes by their places.
fn walk_order(sheet: &SheetRef) -> (u8, &[u32]) {
    match sheet {
        SheetRef::Page(_) => (0, &[]),
        SheetRef::Shape(at) => (1, at.path.as_slice()),
        _ => (2, &[]),
    }
}

/// One sheet of the listing: what it reads of the pages, and what the entry was charged.
#[derive(Debug)]
struct Entry {
    sheet: SheetRef,
    named: Named,
    bytes: u64,
}

/// What a page's listing is made under: **the master shapes that name something**, by sheet (what an instance inherits), and **which of the sheets no page holds a chain of commits can change**
/// The sheets that name `TheDoc!` or `Styles!` are worth listing only when the document's sheet, or a style, holds a formula that reads beyond its own sheet: a cell of a sheet
/// whose formulas read nothing else changes only when a cell of that sheet is written, and a write to the document's sheet or to a style works every page out whole, so a drawing whose document
/// sheet and styles hold constants and formulas of their own cells lists none of the thousands of shapes that read them. The masters are always listed: a page reads a master it holds no instance
/// of through `Masters[..]!`, which no pass over the pages that hold an instance reaches. `None` for the sheets no page holds, whose entries are kept whole.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Inherits {
    shapes: HashMap<SheetRef, Named>,
    gates: Option<Unpaged>,
}

type MasterShapes = Inherits;

impl Inherits {
    fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    fn get(&self, sheet: &SheetRef) -> Option<&Named> {
        self.shapes.get(sheet)
    }
}

/// Whether a sheet holds a formula that reads beyond its own sheet (a qualified reference, a frame function, a call that reads at run time): charged by the text of each formula and its nodes, and
/// left as soon as one is found.
fn holds_a_formula_that_reads_beyond(ev: &mut Evaluator<'_, '_>, sheet: &Sheet) -> bool {
    let mut found = false;
    let mut text_bytes = 0_usize;
    each_formula(sheet, &mut SectionFormulas::default(), |_, formula| {
        let Some(formula) = formula else { return };
        text_bytes = text_bytes.saturating_add(formula.source.len());
        if !found && !ev.ctx.stats.budget_exhausted {
            found = reaches_beyond_own_sheet(ev, formula);
        }
    });
    let _ = ev.steps(text_bytes.div_ceil(rules::UNITS_PER_STEP));
    found
}

/// What an instance of a master shape inherits of the pages: what that shape names (nothing when no master shape names a page, which is looked up first, so that a drawing in which none does
/// asks no instance for its master).
fn inherited(ev: &mut Evaluator<'_, '_>, at: &ShapeRef, masters: &MasterShapes) -> Option<Named> {
    if masters.is_empty() || !ev.steps(1) {
        return None;
    }
    let Ok(Some(step)) = ev.res.master_step(at) else { return None };
    masters.get(&SheetRef::Shape(step.shape)).cloned()
}

/// The entry of a sheet of a page: what its formulas name, with what its master shape names if it is an instance.
fn entry_of(ev: &mut Evaluator<'_, '_>, sheet: &SheetRef, data: &Sheet, memo: &mut SectionFormulas, masters: &MasterShapes) -> Option<Entry> {
    let own = scan_sheet(ev, sheet, data, memo, masters.gates.unwrap_or(Unpaged::ALL));
    // **An instance is listed under every name it inherits**, one place each in the index of names ([`PageLinks::rebuild`]), so it is charged for each of them, as a sheet that names them itself is:
    // a master shape that names 200 pages and 500 instances of it are 100,000 places, and are charged as 100,000 (the names themselves are shared, the places are not).
    let named = match sheet {
        SheetRef::Shape(at) => match inherited(ev, at, masters) {
            Some(from_master) if own.is_empty() => from_master,
            Some(from_master) => own.with(&from_master),
            None => own,
        },
        _ => own,
    };
    let named = match masters.gates {
        Some(gates) => named.masked(gates),
        None => named,
    };
    if named.is_empty() {
        return None;
    }
    let bytes = named.bytes();
    Some(Entry { sheet: sheet.clone(), named, bytes })
}

/// What one page holds of the listing: the page it was made for (held, so that a pointer to it is none other's), and its sheets that name a page, in the order of a walk.
#[derive(Debug)]
struct LinkPage {
    page: Arc<Page>,
    sheets: Vec<Entry>,
    /// What the entries are charged, as accounted.
    bytes: u64,
    /// The listing of the master shapes the instances were looked at under ([`LinkGlobal::epoch`]).
    epoch: u64,
}

/// What putting a page's listing right came to.
enum Update {
    /// The sheets that name a page are the ones they were.
    Same,
    /// A sheet's formulas changed and the list was made again for it.
    Changed,
    /// The page is to be made again whole.
    Remake,
}

impl LinkPage {
    /// The listing of `page`, made by a walk of its sheets. `None` when the budget ran out on the way.
    fn make(ev: &mut Evaluator<'_, '_>, doc: &Document, page: &Arc<Page>, masters: &MasterShapes, epoch: u64) -> Option<LinkPage> {
        let mut found = LinkPage { page: Arc::clone(page), sheets: Vec::new(), bytes: 0, epoch };
        let mut memo = SectionFormulas::default();
        let mut over = false;
        walk_page(doc, page.id, &mut |sheet, data| {
            if over {
                return;
            }
            if let Some(entry) = entry_of(ev, sheet, data, &mut memo, masters) {
                if !ev.ctx.hold_bytes(entry.bytes) {
                    over = true;
                    return;
                }
                found.bytes = found.bytes.saturating_add(entry.bytes);
                found.sheets.push(entry);
            }
        });
        (!over && !ev.ctx.stats.budget_exhausted).then_some(found)
    }

    /// Puts the listing right for `page`, which is another allocation than the one it was made for: only the sheets the edits since replaced are looked at again. [`Update::Remake`]
    /// when it is to be made again whole (the shapes are not the same shapes in the same places) or the budget ran out.
    fn bring_up_to_date(&mut self, ev: &mut Evaluator<'_, '_>, page: &Arc<Page>, masters: &MasterShapes) -> Update {
        let owner = Owner::Page(page.id);
        let mut replaced: Vec<Replaced> = Vec::new();
        if !Arc::ptr_eq(&self.page.sheet, &page.sheet) {
            replaced.push(Replaced { sheet: SheetRef::Page(page.id), was: Arc::clone(&self.page.sheet), now: Arc::clone(&page.sheet) });
        }
        if !replaced_in(&self.page.shapes, &page.shapes, owner, &mut Vec::new(), &mut replaced, ev) {
            return Update::Remake;
        }
        let mut again: Vec<(SheetRef, Arc<Sheet>)> = Vec::new();
        for r in &replaced {
            match same_formulas(ev, &r.was, &r.now) {
                Some(true) => {}
                Some(false) => again.push((r.sheet.clone(), Arc::clone(&r.now))),
                None => return Update::Remake,
            }
        }
        let changed = !again.is_empty();
        if changed {
            let gone: HashSet<&SheetRef> = again.iter().map(|(sheet, _)| sheet).collect();
            if !ev.work(self.sheets.len().saturating_mul(8)) {
                return Update::Remake;
            }
            let mut released = 0_u64;
            self.sheets.retain(|entry| {
                let drop = gone.contains(&entry.sheet);
                if drop {
                    released = released.saturating_add(entry.bytes);
                }
                !drop
            });
            ev.ctx.release_bytes(released);
            self.bytes = self.bytes.saturating_sub(released);
            let mut memo = SectionFormulas::default();
            for (sheet, data) in again {
                let Some(entry) = entry_of(ev, &sheet, &data, &mut memo, masters) else { continue };
                if !ev.ctx.hold_bytes(entry.bytes) {
                    return Update::Remake;
                }
                self.bytes = self.bytes.saturating_add(entry.bytes);
                self.sheets.push(entry);
            }
            self.sheets.sort_by(|a, b| walk_order(&a.sheet).cmp(&walk_order(&b.sheet)));
        }
        self.page = Arc::clone(page);
        if ev.ctx.stats.budget_exhausted {
            Update::Remake
        } else if changed {
            Update::Changed
        } else {
            Update::Same
        }
    }
}

/// The listing of `page` put right from the one `kept` (made for another allocation of it), or made whole: with whether the sheets that name a page are others than before. `None` when the
/// budget ran out. A listing made under another listing of the master shapes is made again.
fn refresh(
    ev: &mut Evaluator<'_, '_>,
    doc: &Document,
    page: &Arc<Page>,
    kept: Option<LinkPage>,
    masters: &MasterShapes,
    epoch: u64,
) -> Option<(LinkPage, bool)> {
    let Some(mut old) = kept else { return LinkPage::make(ev, doc, page, masters, epoch).map(|made| (made, true)) };
    if old.epoch != epoch {
        ev.ctx.release_bytes(old.bytes);
        return LinkPage::make(ev, doc, page, masters, epoch).map(|made| (made, true));
    }
    match old.bring_up_to_date(ev, page, masters) {
        Update::Same => Some((old, false)),
        Update::Changed => Some((old, true)),
        Update::Remake => {
            ev.ctx.release_bytes(old.bytes);
            if ev.ctx.stats.budget_exhausted { None } else { LinkPage::make(ev, doc, page, masters, epoch).map(|made| (made, true)) }
        }
    }
}

/// What one part the pages do not hold (a style, a master) has in the listing, with the allocation it was made for.
#[derive(Debug)]
struct Part<T> {
    part: Arc<T>,
    sheets: Vec<Entry>,
    bytes: u64,
}

impl<T> Part<T> {
    fn new(part: &Arc<T>) -> Part<T> {
        Part { part: Arc::clone(part), sheets: Vec::new(), bytes: 0 }
    }

    /// Lists `sheet` with what it names, charged to the commit's memory. `false` when the memory ran out.
    fn add(&mut self, ev: &mut Evaluator<'_, '_>, entry: Option<Entry>) -> bool {
        let Some(entry) = entry else { return true };
        if !ev.ctx.hold_bytes(entry.bytes) {
            return false;
        }
        self.bytes = self.bytes.saturating_add(entry.bytes);
        self.sheets.push(entry);
        true
    }
}

/// The sheets no page holds, in the listing: the document's sheet, each style's, each master's page sheet and its shapes. See the module documentation.
#[derive(Debug, Default)]
struct LinkGlobal {
    sheet: Option<Part<Sheet>>,
    styles: HashMap<u32, Part<Style>>,
    masters: HashMap<u32, Part<Master>>,
    /// The master shapes that name something, for the instances that inherit from them, and which sheets no page holds a chain can change ([`Inherits`]).
    master_shapes: MasterShapes,
    /// Whether the document's sheet holds a formula that reads beyond its own sheet.
    doc_reads: bool,
    /// Whether some style held a formula the listing looks at ([`StyleReach`]) when the styles were last put right.
    styled: bool,
    /// Moves when [`LinkGlobal::master_shapes`] changes, or a style changes while a style gives a sheet a formula the listing looks at, so that the listing of a page made under another is made again.
    epoch: u64,
    /// What the entries are charged, as accounted.
    bytes: u64,
}

impl LinkGlobal {
    /// Brings the listing of the sheets no page holds to the state of `doc`, looking again only at what an edit replaced (by pointer). → whether anything is listed otherwise than it was;
    /// `None` when the budget ran out.
    fn sync(&mut self, ev: &mut Evaluator<'_, '_>, doc: &Document) -> Option<bool> {
        let mut changed = false;
        let none = MasterShapes::default();
        if !self.sheet.as_ref().is_some_and(|held| Arc::ptr_eq(&held.part, &doc.sheet)) {
            if let Some(old) = self.sheet.take() {
                ev.ctx.release_bytes(old.bytes);
            }
            let mut part = Part::new(&doc.sheet);
            let entry = entry_of(ev, &SheetRef::Document, &doc.sheet, &mut SectionFormulas::default(), &none);
            if !part.add(ev, entry) || ev.ctx.stats.budget_exhausted {
                return None;
            }
            self.doc_reads = holds_a_formula_that_reads_beyond(ev, &doc.sheet);
            self.sheet = Some(part);
            changed = true;
        }
        let mut seen: HashSet<u32> = HashSet::new();
        let mut styles_replaced = false;
        for style in doc.styles.iter() {
            // A style whose `ID` an earlier style has is not addressable.
            if !seen.insert(style.id) || self.styles.get(&style.id).is_some_and(|held| Arc::ptr_eq(&held.part, style)) {
                continue;
            }
            if let Some(old) = self.styles.remove(&style.id) {
                ev.ctx.release_bytes(old.bytes);
            }
            let mut part = Part::new(style);
            let entry = entry_of(ev, &SheetRef::Style(style.id), &style.sheet, &mut SectionFormulas::default(), &none);
            if !part.add(ev, entry) || ev.ctx.stats.budget_exhausted {
                return None;
            }
            self.styles.insert(style.id, part);
            changed = true;
            styles_replaced = true;
        }
        let before = self.styles.len();
        self.styles.retain(|id, gone| {
            let keep = seen.contains(id);
            if !keep {
                ev.ctx.release_bytes(gone.bytes);
            }
            keep
        });
        styles_replaced |= self.styles.len() != before;
        changed |= self.styles.len() != before;
        // **A formula a shape inherits from a style**: the listing of a page holds, for a shape, what the formulas it gets from a style name, which an edit of a style (or of what it is based on)
        // changes without the page's own sheets changing. A page's listing made under the styles as they were is made again, when a style gave a sheet a formula that is looked at before the edit
        // or does after it (a drawing whose styles hold none has nothing to put right).
        if styles_replaced {
            let now = !ev.style_reach().is_empty();
            if self.styled || now {
                self.epoch = self.epoch.saturating_add(1);
            }
            self.styled = now;
        }
        // **Which sheets no page holds a chain of commits can change**: a page's listing made under other gates is made again.
        let gates = Unpaged { doc: self.doc_reads, styles: self.styled, masters: true };
        if self.master_shapes.gates != Some(gates) {
            self.master_shapes.gates = Some(gates);
            self.epoch = self.epoch.saturating_add(1);
        }
        seen.clear();
        let mut masters_changed = false;
        for master in doc.masters.iter() {
            if !seen.insert(master.id) || self.masters.get(&master.id).is_some_and(|held| Arc::ptr_eq(&held.part, master)) {
                continue;
            }
            if let Some(old) = self.masters.remove(&master.id) {
                ev.ctx.release_bytes(old.bytes);
            }
            let mut part = Part::new(master);
            let mut ok = true;
            let mut memo = SectionFormulas::default();
            walk_master(master, &mut |sheet, data| {
                if ok {
                    let entry = entry_of(ev, sheet, data, &mut memo, &none);
                    ok = part.add(ev, entry);
                }
            });
            if !ok || ev.ctx.stats.budget_exhausted {
                return None;
            }
            self.masters.insert(master.id, part);
            masters_changed = true;
        }
        let before = self.masters.len();
        self.masters.retain(|id, gone| {
            let keep = seen.contains(id);
            if !keep {
                ev.ctx.release_bytes(gone.bytes);
            }
            keep
        });
        masters_changed |= self.masters.len() != before;
        if masters_changed {
            let shapes: HashMap<SheetRef, Named> = self
                .masters
                .values()
                .flat_map(|part| part.sheets.iter())
                .filter(|entry| matches!(entry.sheet, SheetRef::Shape(_)))
                .map(|entry| (entry.sheet.clone(), entry.named.clone()))
                .collect();
            if shapes != self.master_shapes.shapes {
                self.master_shapes.shapes = shapes;
                self.epoch = self.epoch.saturating_add(1);
            }
            changed = true;
        }
        self.bytes = self
            .sheet
            .iter()
            .map(|p| p.bytes)
            .chain(self.styles.values().map(|p| p.bytes))
            .chain(self.masters.values().map(|p| p.bytes))
            .fold(0, u64::saturating_add);
        Some(changed)
    }

    /// The entries of the sheets no page holds, in the order of the document: its sheet, the styles, the masters.
    fn each_entry<'a>(&'a self, doc: &'a Document, mut f: impl FnMut(&'a Entry)) {
        if let Some(part) = &self.sheet {
            part.sheets.iter().for_each(&mut f);
        }
        let mut seen: HashSet<u32> = HashSet::new();
        for style in doc.styles.iter() {
            if seen.insert(style.id)
                && let Some(part) = self.styles.get(&style.id)
            {
                part.sheets.iter().for_each(&mut f);
            }
        }
        seen.clear();
        for master in doc.masters.iter() {
            if seen.insert(master.id)
                && let Some(part) = self.masters.get(&master.id)
            {
                part.sheets.iter().for_each(&mut f);
            }
        }
    }
}

/// The sheets that name a page, for every page of a document and the sheets no page holds, kept from one edit to the next: see the module documentation.
#[derive(Debug, Default)]
pub(crate) struct PageLinks {
    pages: HashMap<u32, LinkPage>,
    global: LinkGlobal,
    /// The sheets that name each page name, in the order of the pages and a walk of each, then the sheets no page holds; rebuilt when a list changed.
    by_name: HashMap<Designator, Vec<SheetRef>>,
    /// The sheets that read at run time, which are read as naming every page.
    wild: Vec<SheetRef>,
    /// The sheets that read the list of pages.
    listy: Vec<SheetRef>,
    /// The sheets that name the document's sheet, a style's and a master's, in the order of the document.
    of_doc: Vec<SheetRef>,
    of_styles: Vec<SheetRef>,
    of_masters: Vec<SheetRef>,
    /// What the listing is charged, as accounted: every page's entries and the sheets no page holds.
    pub(crate) bytes: u64,
}

impl PageLinks {
    /// Lets the listing go.
    pub(crate) fn clear(&mut self) {
        *self = PageLinks::default();
    }

    /// Brings the listing to the state of `doc` and charges it to the commit's memory. A document of one page has none for its pages (no other page reads them), unless `every_page` says a
    /// change to the pages is being worked out, and a budget that runs out leaves none.
    pub(crate) fn sync(&mut self, ev: &mut Evaluator<'_, '_>, doc: &Document, every_page: bool) {
        if !ev.ctx.hold_bytes(self.bytes) {
            self.clear();
            return;
        }
        let Some(mut changed) = self.global.sync(ev, doc) else {
            self.clear();
            return;
        };
        if doc.pages.len() < 2 && !every_page {
            changed |= !self.pages.is_empty();
            self.pages.clear();
        } else {
            let mut seen: HashSet<u32> = HashSet::with_capacity(doc.pages.len());
            let (masters, epoch) = (&self.global.master_shapes, self.global.epoch);
            for page in doc.pages.iter() {
                // A page whose `ID` an earlier page has is not addressable: a reference reaches the first.
                if !seen.insert(page.id) || self.pages.get(&page.id).is_some_and(|old| Arc::ptr_eq(&old.page, page) && old.epoch == epoch) {
                    continue;
                }
                let kept = self.pages.remove(&page.id);
                let Some((entry, differs)) = refresh(ev, doc, page, kept, masters, epoch) else {
                    self.clear();
                    return;
                };
                changed |= differs;
                self.pages.insert(page.id, entry);
            }
            let before = self.pages.len();
            self.pages.retain(|id, gone| {
                let keep = seen.contains(id);
                if !keep {
                    ev.ctx.release_bytes(gone.bytes);
                }
                keep
            });
            changed |= self.pages.len() != before;
        }
        self.bytes = self.pages.values().map(|p| p.bytes).fold(self.global.bytes, u64::saturating_add);
        if changed {
            self.rebuild(doc);
        }
    }

    /// Lists the sheets that name each page name, page by page in the order of the document, and then the sheets no page holds. **Each place pushed here is one name of one entry**, which the
    /// entry's bytes were charged for when it was made ([`Named::bytes`]: a place and a quarter of one for each name), within the commit's memory and before this runs: a listing that passed
    /// the memory was given up by [`PageLinks::sync`] and is not built, and one that is kept is charged again by each commit that uses it.
    fn rebuild(&mut self, doc: &Document) {
        let mut by_name: HashMap<Designator, Vec<SheetRef>> = HashMap::new();
        let (mut wild, mut listy): (Vec<SheetRef>, Vec<SheetRef>) = (Vec::new(), Vec::new());
        let (mut of_doc, mut of_styles, mut of_masters): (Vec<SheetRef>, Vec<SheetRef>, Vec<SheetRef>) = (Vec::new(), Vec::new(), Vec::new());
        let mut list = |entry: &Entry| {
            for one in entry.named.names.iter() {
                by_name.entry(one.clone()).or_default().push(entry.sheet.clone());
            }
            if entry.named.wild {
                wild.push(entry.sheet.clone());
            }
            if entry.named.list.any() {
                listy.push(entry.sheet.clone());
            }
            if entry.named.unpaged.doc {
                of_doc.push(entry.sheet.clone());
            }
            if entry.named.unpaged.styles {
                of_styles.push(entry.sheet.clone());
            }
            if entry.named.unpaged.masters {
                of_masters.push(entry.sheet.clone());
            }
        };
        for page in doc.pages.iter() {
            let Some(entry) = self.pages.get(&page.id) else { continue };
            if !Arc::ptr_eq(&entry.page, page) {
                continue;
            }
            entry.sheets.iter().for_each(&mut list);
        }
        self.global.each_entry(doc, &mut list);
        self.by_name = by_name;
        self.wild = wild;
        self.listy = listy;
        self.of_doc = of_doc;
        self.of_styles = of_styles;
        self.of_masters = of_masters;
    }

    /// The sheets that name `page` and are not on it, each once, in the order of the document: the ones whose formulas may read a cell of it.
    pub(crate) fn naming(&self, page: &Page) -> Vec<SheetRef> {
        let mut out: Vec<SheetRef> = Vec::new();
        let mut known: HashSet<&SheetRef> = HashSet::new();
        for one in Designator::naming(page) {
            for sheet in self.by_name.get(&one).into_iter().flatten() {
                if page_of(sheet) != Some(page.id) && known.insert(sheet) {
                    out.push(sheet.clone());
                }
            }
        }
        out
    }

    /// The sheets that name the sheets `target` is (`TheDoc!`, `Styles!`, `Masters[..]!`) and are not on page `except`, each once, in the order of the document: the ones whose formulas may read a cell of
    /// them. The sheets of the page the commit is for are left out: its own graph answers them. A target that is a page has none here (`naming` says them).
    pub(crate) fn naming_unpaged(&self, target: Target, except: u32) -> Vec<SheetRef> {
        let all = match target {
            Target::Page(_) => return Vec::new(),
            Target::Doc => &self.of_doc,
            Target::Styles => &self.of_styles,
            Target::Masters => &self.of_masters,
        };
        // A page whose `ID` an earlier page has is listed once for each (`rebuild`): each sheet is given once.
        let mut known: HashSet<&SheetRef> = HashSet::new();
        all.iter().filter(|sheet| page_of(sheet) != Some(except) && known.insert(sheet)).cloned().collect()
    }

    /// The cells that read at run time (`EVALTEXT`, `EVALCELL`, and what else a sheet that holds one reads without naming it) in the sheets that read every page, in the order of the
    /// document: worked out once after a change to a cell of any page. A step for each cell of those sheets, as a classification of the sheets of a page costs.
    pub(crate) fn wild_readers(&self, ev: &mut Evaluator<'_, '_>, formulas: &mut SectionFormulas) -> Vec<Reader> {
        let mut gathered = Gathered::default();
        let mut memo = MasterCross::default();
        'sheets: for sheet in &self.wild {
            let Some(data) = ev.res.sheet(sheet) else { continue };
            let mut found = Found::default();
            classify_sheet(ev, sheet, data, &mut memo, formulas, &mut found);
            for (reader, _) in found.dynamic {
                if !gathered.add(ev, reader) {
                    break 'sheets;
                }
            }
        }
        gathered.out
    }

    /// The cells whose formulas name a page in `names`, read at run time, or (when `names` says the list of pages changed) read that list: the cells to work out after a change to the
    /// pages themselves, in the order of the document. A cell an instance inherits from a master shape is the instance's.
    pub(crate) fn readers_naming(&self, ev: &mut Evaluator<'_, '_>, names: &PageNames) -> Vec<Reader> {
        let wanted: HashSet<&Designator> = names.names.iter().collect();
        let mut sheets: Vec<&SheetRef> = Vec::new();
        let mut known: HashSet<&SheetRef> = HashSet::new();
        for one in &names.names {
            for sheet in self.by_name.get(one).into_iter().flatten() {
                if known.insert(sheet) {
                    sheets.push(sheet);
                }
            }
        }
        for sheet in self.wild.iter().chain(names.list.any().then_some(&self.listy).into_iter().flatten()) {
            if known.insert(sheet) {
                sheets.push(sheet);
            }
        }
        let matches = |read: &Named| read.wild || read.list.is_changed_by(names.list) || read.names.iter().any(|d| wanted.contains(d));
        let mut formulas = SectionFormulas::default();
        let mut shapes: HashMap<SheetRef, Arc<Vec<(CellAddr, Named)>>> = HashMap::new();
        // **Everything found is charged** to the commit, a step for each sheet and for each cell of a master shape looked at for an instance, and the memory of each cell kept; each cell is found
        // once, by a set made as the cells are met. The walk ends the moment the budget or the memory runs out.
        let mut gathered = Gathered::default();
        'sheets: for sheet in sheets {
            if !ev.steps(1) {
                break;
            }
            let Some(data) = ev.res.sheet(sheet) else { continue };
            for (addr, read) in scan_cells(ev, sheet, data, &mut formulas) {
                if matches(&read) && !gathered.add(ev, (sheet.clone(), addr)) {
                    break 'sheets;
                }
            }
            // An instance inherits the formulas of its master shape, which are its own cells to work out as far as they name what changed.
            let SheetRef::Shape(at) = sheet else { continue };
            if self.global.master_shapes.is_empty() || !ev.steps(1) {
                continue;
            }
            let Ok(Some(step)) = ev.res.master_step(at) else { continue };
            let master = SheetRef::Shape(step.shape);
            if self.global.master_shapes.get(&master).is_none() {
                continue;
            }
            let cells = match shapes.get(&master) {
                Some(known) => Arc::clone(known),
                None => {
                    let found = ev.res.sheet(&master).map(|data| scan_cells(ev, &master, data, &mut formulas)).unwrap_or_default();
                    let found = Arc::new(found);
                    shapes.insert(master, Arc::clone(&found));
                    found
                }
            };
            // A step for each cell of the master shape looked at, before it is.
            if !ev.steps(cells.len()) {
                break;
            }
            for (addr, _) in cells.iter().filter(|(_, read)| matches(read)) {
                if !gathered.add(ev, (sheet.clone(), addr.clone())) {
                    break 'sheets;
                }
            }
        }
        gathered.out
    }
}

/// The cells a walk gathers, each once and in the order found: a place in the list and one in the set that tells a cell met twice, charged together to the commit's memory as each is added.
#[derive(Default)]
struct Gathered {
    out: Vec<Reader>,
    seen: HashSet<Reader>,
}

impl Gathered {
    /// Adds `reader` unless it is held already. `false`, with nothing added, when the memory of the commit runs out: the walk ends.
    fn add(&mut self, ev: &mut Evaluator<'_, '_>, reader: Reader) -> bool {
        if self.seen.contains(&reader) {
            return true;
        }
        if !ev.ctx.hold_bytes(2 * KEY_BYTES) {
            return false;
        }
        self.seen.insert(reader.clone());
        self.out.push(reader);
        true
    }
}

/// What the sheets that name a page read of it, for one commit: the cells of the page, each with the cells of other pages that read it, and the readers that find no sheet.
#[derive(Debug, Default)]
pub(crate) struct CrossMap {
    /// The readers of a cell of the page, by the cell, as the page's own graph holds them.
    pub(crate) cross: HashMap<Key, Vec<Reader>>,
    /// The readers in those sheets whose reference found no sheet: a shape that was taken out may have been the one they named.
    pub(crate) dangling: Vec<Reader>,
    /// Every reader of either kind, in the order the sheets were classified (a set of readers in an order that does not depend on how a map is laid out).
    readers: Vec<Reader>,
}

impl CrossMap {
    /// Classifies `sheets` (the sheets that name `target`: a page, the document's sheet, the styles or the masters) and keeps what they read of it.
    pub(crate) fn make(ev: &mut Evaluator<'_, '_>, sheets: &[SheetRef], target: Target, formulas: &mut SectionFormulas) -> CrossMap {
        let mut map = CrossMap::default();
        let mut memo = MasterCross::default();
        for sheet in sheets {
            let Some(data) = ev.res.sheet(sheet) else { continue };
            let mut found = Found::default();
            classify_sheet(ev, sheet, data, &mut memo, formulas, &mut found);
            for (key, reader, _) in found.cross {
                if Target::of(&key.0) == target {
                    map.readers.push(reader.clone());
                    map.cross.entry(key).or_default().push(reader);
                }
            }
            for (reader, ..) in found.dangling {
                map.readers.push(reader.clone());
                map.dangling.push(reader);
            }
        }
        map
    }

    /// Every reader the map holds, each once, in the order they were found.
    pub(crate) fn every_reader(&self) -> Vec<Reader> {
        let mut known: HashSet<&Reader> = HashSet::new();
        let mut out: Vec<Reader> = Vec::new();
        for reader in &self.readers {
            if known.insert(reader) {
                out.push(reader.clone());
            }
        }
        out
    }
}

/// The page names an edit gave another reading: the names that found one page and find another or none (or found none and find one), and the names of the pages that were renamed, added or
/// taken away. With whether the list of pages changed (its order, its count, which are background pages), which `PAGENAME()`, `PAGENUMBER()` and the like read. **A command that changes the
/// pages makes it from the document before and the one after** ([`PageNames::between`]) and puts it in [`Changes::page_names`], and [`commit_changed_across`] works out the cells that name them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageNames {
    names: Vec<Designator>,
    list: ListReads,
}

/// Whether two pages are the same as far as a reader of the list of pages goes: the `ID`, the names, the type and the page they are shown over.
fn same_in_the_list(a: &Page, b: &Page) -> bool {
    a.id == b.id && a.name == b.name && a.name_u == b.name_u && a.background == b.background && a.back_page == b.back_page
}

/// The page each designator finds in `doc`: the first page in the order of the list that a name or a number says, as the evaluator finds it.
fn found_by_name(doc: &Document) -> HashMap<Designator, u32> {
    let mut table: HashMap<Designator, u32> = HashMap::with_capacity(doc.pages.len().saturating_mul(3));
    for page in doc.pages.iter() {
        for one in Designator::naming(page) {
            table.entry(one).or_insert(page.id);
        }
    }
    table
}

impl PageNames {
    /// Whether nothing was given another reading.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty() && !self.list.any()
    }

    /// Whether the names of the pages changed: what `PAGENAME()` and `BKGPAGENAME()` read.
    pub fn changed_names(&self) -> bool {
        self.list.names
    }

    /// Whether the order of the pages, their count, which are backgrounds or which background a page shows changed: what `PAGENUMBER()`, `PAGECOUNT()` and `BKGPAGENAME()` read. A page that is only renamed
    /// changes none of them.
    pub fn changed_order(&self) -> bool {
        self.list.order
    }

    /// What `after` gave another reading than `before`: nothing for two drawings whose lists of pages are the same pages in the same places, which costs a comparison by pointer of each
    /// page. At most [`diacraft_doc::MAX_PAGES`] pages are looked at in each, three names each.
    pub fn between(before: &Document, after: &Document) -> PageNames {
        let same = before.pages.is_same_list_as(&after.pages)
            || (before.pages.len() == after.pages.len()
                && before.pages.iter().zip(after.pages.iter()).all(|(a, b)| Arc::ptr_eq(a, b) || same_in_the_list(a, b)));
        if same {
            return PageNames::default();
        }
        let (was, now) = (found_by_name(before), found_by_name(after));
        let mut names: Vec<Designator> = Vec::new();
        names.extend(now.iter().filter(|(d, id)| was.get(*d) != Some(*id)).map(|(d, _)| d.clone()));
        names.extend(was.keys().filter(|d| !now.contains_key(*d)).cloned());
        // A page renamed keeps the names a formula reads it by, and its new name is read by `PAGENAME()`: the names it had and has are both given.
        let first_by_id = |doc: &Document| -> HashMap<u32, Arc<Page>> {
            let mut by_id: HashMap<u32, Arc<Page>> = HashMap::with_capacity(doc.pages.len());
            for page in doc.pages.iter() {
                by_id.entry(page.id).or_insert_with(|| Arc::clone(page));
            }
            by_id
        };
        let (before_by_id, after_by_id) = (first_by_id(before), first_by_id(after));
        for page in after.pages.iter() {
            match before_by_id.get(&page.id) {
                Some(old) if old.name == page.name && old.name_u == page.name_u => {}
                Some(old) => {
                    names.extend(Designator::naming(old));
                    names.extend(Designator::naming(page));
                }
                None => names.extend(Designator::naming(page)),
            }
        }
        for page in before.pages.iter().filter(|p| !after_by_id.contains_key(&p.id)) {
            names.extend(Designator::naming(page));
        }
        names.sort();
        names.dedup();
        // The order is another when the pages are not the same pages in the same places, background or not and showing the same background: a rename alone leaves it as it was.
        let reordered = before.pages.len() != after.pages.len()
            || before.pages.iter().zip(after.pages.iter()).any(|(a, b)| a.id != b.id || a.background != b.background || a.back_page != b.back_page);
        PageNames { names, list: ListReads { names: true, order: reordered } }
    }
}

#[cfg(test)]
mod tests;

/// Whether any sheet of the document names a master (`Masters[..]!`): brings the listing of the sheets that name a page to the state of `doc` and says whether a sheet of any page, or of the sheets no
/// page holds, reads a master. A call that wrote a cell of a master's shape asks it before it works the cells that read it out: **no page's graph is made and no cell worked out for the answer**, only
/// the listing, which the next commit finds kept. → the answer, `true` when the budget or the memory ran out (so that the commit that follows is made and refuses in words), and the steps
/// it took.
pub fn masters_are_named(index: &mut ReaderIndex, doc: &Document, budget: &Budget, options: EvalOptions) -> (bool, u64) {
    let mut ctx = EvalCtx::new(options, *budget);
    let named = {
        let mut ev = Evaluator::new(doc, &mut ctx);
        index.sync_links(&mut ev, doc);
        !index.links.of_masters.is_empty()
    };
    crate::work::stepped(ctx.stats.steps);
    (named || ctx.stats.budget_exhausted || ctx.stats.memory_exhausted, ctx.stats.steps)
}

/// [`commit_changed_with`] for an edit that may be read by cells of other pages: **what [`commit_changed_with`] gives for a change of cells, which already goes after the cells of other pages
/// that name them** (`affected.rs`, in the one commit), and, for a pass over the whole of a page (a style, a master or the document's sheet changed, and every cell of the page was worked
/// out: [`Changes::whole_page`]), **one more commit** for the cells of other pages that name the page and what reads them in turn, within what the first left of the budget. A cell that comes
/// out as it was passes nothing on, so a page that was worked out in the same call costs what it takes to find it is right, and a call that works every page out whole says so
/// ([`Changes::other_pages_worked`]) and has no commit added. The report takes what the second commit did.
///
/// **A change to the pages themselves** ([`Changes::page_names`]) is turned into the cells that name them first ([`PageLinks::readers_naming`], in the budget of the call), and they are worked out
/// in the same commit as the rest of the change: `page` is then the page of the first of them, so that the graph made for the commit is of a page that is read.
pub fn commit_changed_across(
    index: &mut ReaderIndex,
    doc: &mut Document,
    budget: &Budget,
    options: EvalOptions,
    page: u32,
    changes: &Changes,
) -> CommitReport {
    let mut probe = EvalStats::default();
    let mut named: Option<Changes> = None;
    let mut page = page;
    if !changes.page_names.is_empty() {
        let mut ctx = EvalCtx::new(options.clone(), *budget);
        let readers = {
            let mut ev = Evaluator::new(&*doc, &mut ctx);
            index.sync_links_for_pages(&mut ev, &*doc);
            index.links.readers_naming(&mut ev, &changes.page_names)
        };
        probe = ctx.stats;
        crate::work::stepped(probe.steps);
        if probe.budget_exhausted || probe.memory_exhausted {
            return CommitReport {
                budget_exhausted: probe.budget_exhausted,
                memory_exhausted: probe.memory_exhausted,
                stats: probe,
                ..CommitReport::default()
            };
        }
        let only_names = !changes.whole_page
            && changes.cells.is_empty()
            && changes.rows.is_empty()
            && changes.sections.is_empty()
            && changes.fresh.is_empty()
            && changes.removed.is_empty()
            && changes.reread.is_empty();
        let mut more = Changes { page_names: PageNames::default(), ..changes.clone() };
        more.reread.extend(readers);
        // Nothing reads what changed: there is nothing to work out, and no graph is made.
        if more.is_empty() {
            return CommitReport { stats: probe, ..CommitReport::default() };
        }
        if only_names && let Some(first) = more.reread.iter().find_map(|(sheet, _)| page_of(sheet)).filter(|first| doc.page(*first).is_some()) {
            page = first;
        }
        named = Some(more);
    }
    let changes = named.as_ref().unwrap_or(changes);
    let left = Budget { steps: budget.steps.saturating_sub(probe.steps), bytes: budget.bytes };
    let mut report = commit_changed_with(index, doc, &left, options.clone(), page, changes);
    report.stats.absorb(&probe);
    if !changes.whole_page || changes.other_pages_worked || report.budget_exhausted || report.memory_exhausted || doc.page(page).is_none() {
        return report;
    }
    let left = Budget { steps: budget.steps.saturating_sub(report.stats.steps), bytes: budget.bytes };
    let mut ctx = EvalCtx::new(options.clone(), left);
    let readers = {
        let mut ev = Evaluator::new(&*doc, &mut ctx);
        index.sync_links(&mut ev, &*doc);
        let sheets = doc.page(page).map(|p| index.links.naming(p)).unwrap_or_default();
        let mut formulas = SectionFormulas::default();
        let mut out = if sheets.is_empty() { Vec::new() } else { CrossMap::make(&mut ev, &sheets, Target::Page(page), &mut formulas).every_reader() };
        // The cells that read at run time are worked out after a pass over a page too.
        let wild = index.links.wild_readers(&mut ev, &mut formulas);
        let known: HashSet<Reader> = out.iter().cloned().collect();
        out.extend(wild.into_iter().filter(|reader| !known.contains(reader)));
        out
    };
    report.stats.absorb(&ctx.stats);
    report.budget_exhausted |= ctx.stats.budget_exhausted;
    report.memory_exhausted |= ctx.stats.memory_exhausted;
    if readers.is_empty() || ctx.stats.budget_exhausted {
        return report;
    }
    let left = Budget { steps: budget.steps.saturating_sub(report.stats.steps), bytes: budget.bytes };
    let more = commit_changed_with(index, doc, &left, options, page, &Changes { cells: readers, ..Changes::default() });
    // Every count of the second commit is added, the `SETF` writes of the calls it reached among them.
    report.absorb(&more);
    report
}
