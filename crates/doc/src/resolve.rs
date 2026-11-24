//! Resolution: where a cell's formula and value come from when the sheet does not hold them.
//!
//! Nothing is evaluated here, and nothing is copied. [`Resolver::resolve`] walks the steps R32 gives, for the class the
//! catalogue gives the cell, and answers with the cell it found and where: the formula that is to run, the stored value that
//! stands until it has, the sheet it runs on (always the inheritor) and how `Sheet.N` in it is to be read.
//!
//! The steps, for a shape, in order, stopping at the first cell that ends the search (an expression, `No Formula`, or a value and no
//! `F`; `Inh` does not):
//!
//! 1. the shape's own sheet;
//! 2. the master shape it inherits from: its master, for an instance of a master with one top-level shape, and the shape named by
//!    `MasterShape` in the master of its nearest ancestor holding `Master`, for a member (one level: a master shape's own `Master`
//!    is not followed);
//! 3. the style: for a cell with a family, the style the sheet's own link of that family names, else the master shape's, and then
//!    from style to style along that family's link (at most [`MAX_STYLE_STEPS`]); for a cell with no family that a style may hold,
//!    the root style;
//! 4. the default, by the kind of value, or the printed formula of the few cells that read one.
//!
//! A page sheet, a master's page sheet and the document sheet have no master, and walk their own sheet, then the root style. A style
//! walks itself, then its own links. A row or section deleted (`Del`) at any step ends the walk with nothing: R28 says a deleted row
//! stops its values being inherited, and a master shape's own row reads the same as its instance's.
//!
//! **What a read costs.** A [`Resolver`] builds what it needs the first time it needs it and keeps it: the map of masters, styles or pages
//! by `ID`, where each shape of a master is (as a parent and a place for each shape, not a path for each: a path is up to 200 numbers),
//! the same for the members of each instance, the one live shape of each master, the index of a large section's rows and of a sheet's
//! many sections, and whether each style takes part for each family. So a read is a walk of at most [`MAX_STYLE_STEPS`] styles, and the
//! work done once per document, master, instance, sheet or style shows in [`Resolver::stats`] and does not grow with the number of reads.
//! (The first two rounds of found each of these done again for every read, or kept for every shape.)
//!
//! A write of a cell changes none of the maps by `ID` or of shapes, so [`Document::write_batch`] hands them from the resolver that judged
//! one write to the one that judges the next ([`Carried`]); what depends on the cells is built again.

use std::cell::{Cell as StdCell, OnceCell, RefCell};
use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use crate::catalogue::{CellClass, Family, Keying, cell_info, keying, section_class};
use crate::cell::Cell;
use crate::document::Document;
use crate::name::{CellName, SectionName};
use crate::part::{Master, Page, Style};
use crate::shape::{MAX_SHAPE_DEPTH, Shape, ShapeList};
use crate::shared::SharedVec;
use crate::sheet::{CellAddr, Lookup, Row, RowKey, Section, SectionKey, Sheet};
use crate::sheet_ref::{Owner, ShapeRef, SheetRef, Visit, chain_in, shape_in, walk};
use crate::value::ValueKind;

/// Most styles a walk along a style link visits. Real files reach the root in 5 steps at most.
pub const MAX_STYLE_STEPS: usize = 64;

/// Whether a style whose `EnableLineProps`, `EnableFillProps` or `EnableTextProps` is 0 is skipped for that family's cells even when
/// it holds them. Chooses yes, as leaves open; flip it here.
const STYLE_FLAG_SKIPS_STYLE: bool = true;

/// Sections a sheet may hold before a lookup of a section by key uses an index instead of a scan. Real sheets hold about ten.
const SECTION_SCAN_LIMIT: usize = 16;

/// Where a formula or a value was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// The sheet asked.
    Local,
    /// The master shape the sheet inherits from: its master, for an instance, and the shape it stands for, for a member.
    Master(ShapeRef),
    /// A style, reached along the link of a family, or the root style for a cell with no family.
    Style { id: u32, family: Option<Family> },
    /// Nowhere: the default.
    Default,
}

/// A source, and the cell found there.
#[derive(Clone, Debug, PartialEq)]
pub struct Located<'a> {
    pub source: Source,
    /// The cell itself; `None` when the source is [`Source::Default`].
    pub cell: Option<&'a Cell>,
}

impl Located<'_> {
    fn nowhere() -> Self {
        Located { source: Source::Default, cell: None }
    }
}

/// What a cell found nowhere is, and the formula a few cells read instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefaultSpec {
    pub kind: ValueKind,
    /// The printed formula some cells read in place of the default: the six `Text Transform` cells that print one and the
    /// two picture size cells. `calc` evaluates it on the sheet asked.
    pub formula: Option<&'static str>,
}

/// Things that went wrong with a link on the way, which a caller reports once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Warnings(u8);

impl Warnings {
    /// A `Master` that names no master: the shape resolved as if it had none.
    pub const DANGLING_MASTER: Warnings = Warnings(1);
    /// A `MasterShape` that names no shape of the master, or a member with no instance above it.
    pub const DANGLING_MASTER_SHAPE: Warnings = Warnings(2);
    /// A style link to a style that does not exist.
    pub const DANGLING_STYLE: Warnings = Warnings(4);
    /// A style chain that comes back to a style it has been through.
    pub const STYLE_CYCLE: Warnings = Warnings(8);
    /// A style chain longer than [`MAX_STYLE_STEPS`].
    pub const STYLE_CHAIN_TOO_LONG: Warnings = Warnings(16);

    /// Whether all of `other` is in this.
    pub fn contains(self, other: Warnings) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether there are none.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    fn add(&mut self, other: Warnings) {
        self.0 |= other.0;
    }
}

/// How a formula found on a master names the inheritor's own shapes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rebind {
    /// The instance: the nearest shape, the one asked or above it, that holds `Master`.
    pub instance: ShapeRef,
    /// The `ID` of the master the instance names.
    pub master: u32,
}

/// The master shape a shape inherits from, and the instance that gives it its master.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MasterStep {
    /// The shape of the master part (`Owner::Master`).
    pub shape: ShapeRef,
    /// The instance: the shape itself, for an instance, and for a member the nearest ancestor that holds `Master`.
    pub instance: ShapeRef,
    /// The `ID` of the master.
    pub master: u32,
}

/// The answer to a resolution. It says where things came from and leaves evaluating to `calc`.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved<'a> {
    /// Where the formula is: the first source with an expression, `No Formula` or a value and no `F`. A formula found
    /// here is evaluated on [`Resolved::on`], not on the sheet it was found on. `Source::Default` when none ends the search.
    pub formula: Located<'a>,
    /// Where the stored value stands: the nearest cell on the way to the formula that holds a `V`. A local cell with a `V`,
    /// `Inh` or not, has the last word until an evaluation.
    pub value: Located<'a>,
    /// The sheet the formula is evaluated on: the sheet asked, the inheritor.
    pub on: SheetRef,
    /// Set when the formula was found on the master: how `Sheet.N` in it is to be read ([`Resolver::rebind`]).
    pub rebind: Option<Rebind>,
    /// What the cell is when found nowhere, and the formula some cells read instead.
    pub default: DefaultSpec,
    /// Whether the catalogue lists the cell. When it does not (a cell of a batch not yet catalogued, or a name no batch knows), its class
    /// is a guess (`style-any`) and its default kind is a plain number: a text cell reads 0 and not the empty string, and a cell a later
    /// batch puts in a family would read its style. A caller that must not guess checks this.
    pub listed: bool,
    pub warnings: Warnings,
}

impl Resolved<'_> {
    /// The kind of value a cell with no `V` reads as: by its unit when the cell found has one, else the
    /// default of the cell's own type.
    pub fn kind_if_no_value(&self) -> ValueKind {
        self.formula.cell.and_then(|c| c.unit.as_deref()).and_then(ValueKind::from_unit).unwrap_or(self.default.kind)
    }
}

/// A row of a section after merging cell by cell.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectiveRow<'a> {
    pub key: RowKey,
    /// The row type `T`: the highest-precedence row's that has one.
    pub row_type: Option<Arc<str>>,
    /// Every cell any contributing row holds, resolved as the whole sheet resolves it, in name order.
    pub cells: Vec<(CellName, Resolved<'a>)>,
}

/// A member of a shape that has members, after the instance and its master are put together (MS-VSDX lets an
/// instance leave members out, and an omitted member is read by R6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Member {
    /// A shape of the page part: a member the instance lists, or one added locally.
    Local(ShapeRef),
    /// A shape of the master part that the instance leaves out. It has no sheet of the instance's: what it would read is the master
    /// shape's own cells, evaluated on a shape the caller makes for it. (No file in the census omits one: every instance of a grouped
    /// master lists all its descendants.)
    Omitted(ShapeRef),
}

/// What a [`Resolver`] has built so far: each count is a piece of work done once, which a read of a cell never does again.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResolverStats {
    /// Maps of masters, styles or pages by `ID`.
    pub id_maps: u64,
    /// Maps of a master's shapes by `ID`.
    pub master_path_maps: u64,
    /// Maps of an instance's members by `MasterShape`.
    pub member_maps: u64,
    /// Walks of a style chain to read an `Enable...Props` flag.
    pub flag_walks: u64,
    /// Scans of a master's top-level shapes for its one live shape.
    pub single_scans: u64,
    /// Indexes of the sections of a sheet that has many.
    pub section_indexes: u64,
    /// Numbers (`u32`) held by the maps of shape paths: what the memory those maps take is counted in.
    pub path_slots: u64,
}

/// What `master_link` found.
enum MasterLink {
    /// The shape inherits from nothing (no `Master`, no `MasterShape`, or an instance of a master with other than one shape).
    None,
    /// It inherits from this master shape.
    Found(MasterStep),
    /// A link that leads nowhere: the shape resolves as if unlinked.
    Dangling(Warnings),
}

/// Where a cell is looked for, in order, after the sheet's own: what R32 says for the class of the cell and the kind of sheet.
struct Plan<'a> {
    /// The master step and the sheet of the master shape.
    master: Option<(MasterStep, &'a Sheet)>,
    /// The style chain.
    styles: Option<StylePlan>,
    /// The root style.
    root: bool,
}

struct StylePlan {
    family: Family,
    start: Option<u32>,
    check_flags: bool,
}

/// The state of one resolution: what has been found, and the warnings met.
struct Walk<'a> {
    default: DefaultSpec,
    warnings: Warnings,
    /// The first cell on the way that holds a `V`.
    first_value: Option<Located<'a>>,
    /// The cell that ended the search.
    stopper: Option<Located<'a>>,
    rebind: Option<Rebind>,
    listed: bool,
    /// Whether the walk has met a cell at all.
    met_a_cell: bool,
}

impl<'a> Walk<'a> {
    /// Notes a cell on the way; true when it ends the search.
    fn visit(&mut self, source: Source, cell: &'a Cell) -> bool {
        self.met_a_cell = true;
        if self.first_value.is_none() && cell.value.is_some() {
            self.first_value = Some(Located { source: source.clone(), cell: Some(cell) });
        }
        if cell.ends_search() {
            self.stopper = Some(Located { source, cell: Some(cell) });
            return true;
        }
        false
    }

    /// A row or section deleted (`Del`) at a step after the sheet's own ends the walk (nothing below it is
    /// inherited). What the walk found before it stands (a cell of the sheet's own that holds a `V` has the last word, `Inh` or
    /// not): the answer is what was found, and `None` when it found no cell at all.
    fn ended_by_a_deletion(self, on: &SheetRef) -> Option<Resolved<'a>> {
        self.met_a_cell.then(|| self.finish(on.clone()))
    }

    fn finish(self, on: SheetRef) -> Resolved<'a> {
        Resolved {
            formula: self.stopper.unwrap_or_else(Located::nowhere),
            value: self.first_value.unwrap_or_else(Located::nowhere),
            on,
            rebind: self.rebind,
            default: self.default,
            listed: self.listed,
            warnings: self.warnings,
        }
    }
}

/// No parent: the entry is a top-level shape.
const NO_PARENT: u32 = u32::MAX;

/// Where one shape of a tree is: the entry of the shape it is in and its place in that shape's list.
#[derive(Clone, Copy)]
struct PathEntry {
    parent: u32,
    at: u32,
}

/// The first shape with each `ID` in a tree of shapes (in document order), kept as each shape's parent and place, and not as a path for
/// each: a path is up to [`MAX_SHAPE_DEPTH`] numbers, and a million shapes 200 levels deep made the maps 858 MB. A path is built, from
/// the entry of the shape, when one is asked for.
#[derive(Default)]
struct ShapePaths {
    entries: Vec<PathEntry>,
    by_id: HashMap<u32, u32>,
}

impl ShapePaths {
    /// The path to the first shape with `id`: the place of each shape from the top level down.
    fn path(&self, id: u32) -> Option<Vec<u32>> {
        let mut entry = *self.by_id.get(&id)?;
        let mut path = Vec::new();
        loop {
            let found = self.entries.get(usize::try_from(entry).ok()?)?;
            path.push(found.at);
            if found.parent == NO_PARENT {
                break;
            }
            // Parents are always earlier entries, so a walk up ends; the depth bound is for a map the model did not build.
            if path.len() > MAX_SHAPE_DEPTH {
                return None;
            }
            entry = found.parent;
        }
        path.reverse();
        Some(path)
    }

    /// The numbers held, for [`ResolverStats::path_slots`]: two for each entry and one for each `ID`.
    fn slots(&self) -> u64 {
        (self.entries.len() as u64).saturating_mul(2).saturating_add(self.by_id.len() as u64)
    }
}

/// Maps a tree of shapes: `visit` says for each shape whether to go into it (a skipped shape and what is under it are not entered) and the
/// `ID` it is found by, if any. The first shape found by an `ID` is the one the map gives.
fn map_shapes(top: &ShapeList, mut visit: impl FnMut(&Shape) -> (Visit, Option<u32>)) -> ShapePaths {
    let mut paths = ShapePaths::default();
    // The entry of the shape at each level above the one being visited.
    let mut above: Vec<u32> = Vec::new();
    walk(top, |shape, path| {
        let (what, key) = visit(shape);
        above.truncate(path.len().saturating_sub(1));
        if what == Visit::Skip {
            return Visit::Skip;
        }
        let me = match u32::try_from(paths.entries.len()) {
            Ok(me) if me != NO_PARENT => me,
            _ => return Visit::Skip,
        };
        paths.entries.push(PathEntry { parent: above.last().copied().unwrap_or(NO_PARENT), at: path.last().copied().unwrap_or(0) });
        if let Some(id) = key {
            paths.by_id.entry(id).or_insert(me);
        }
        above.push(me);
        what
    });
    paths
}

/// A style chain, one style at a time: from `next` along the links of a family, never more than [`MAX_STYLE_STEPS`], ending at a link
/// that is none, a style that does not exist and a style it has been through (each of the last two a warning). With `check_flags`
/// a style that does not take part for the family is passed over and the chain goes on.
struct StyleChain<'r, 'a> {
    resolver: &'r Resolver<'a>,
    next: Option<u32>,
    seen: Vec<u32>,
    family: Family,
    check_flags: bool,
    warnings: Warnings,
}

impl<'a> Iterator for StyleChain<'_, 'a> {
    type Item = &'a Style;

    fn next(&mut self) -> Option<&'a Style> {
        loop {
            let id = self.next?;
            if self.seen.len() >= MAX_STYLE_STEPS {
                self.warnings.add(Warnings::STYLE_CHAIN_TOO_LONG);
                self.next = None;
                return None;
            }
            if self.seen.contains(&id) {
                self.warnings.add(Warnings::STYLE_CYCLE);
                self.next = None;
                return None;
            }
            self.seen.push(id);
            let Some(style) = self.resolver.style(id) else {
                self.warnings.add(Warnings::DANGLING_STYLE);
                self.next = None;
                return None;
            };
            self.next = style.sheet.links.get(self.family);
            if !self.check_flags || self.resolver.style_enabled(style, self.family) {
                return Some(style);
            }
        }
    }
}

/// What a [`Resolver`] has built that stays true while the cells of the document are written: the maps by `ID` of pages, masters and
/// styles, the maps of a master's and an instance's shapes, and a master's one live shape. A write of a cell adds and removes none of
/// those, so the resolver that judges the next write of a batch starts from them ([`Resolver::resume`]) and does not build them again. What
/// depends on the cells themselves (the index of a section's rows and of a sheet's sections, whether a style takes part) is not here.
///
/// A holder must give it back only to a resolver of a document whose pages, masters, styles and shapes are those it was built for.
#[derive(Default)]
pub(crate) struct Carried {
    masters: OnceCell<HashMap<u32, usize>>,
    styles: OnceCell<HashMap<u32, usize>>,
    pages: OnceCell<HashMap<u32, usize>>,
    /// Per master `ID`, where each shape of the master is.
    master_shapes: RefCell<HashMap<u32, Rc<ShapePaths>>>,
    /// Per instance, where its first member that stands for each master shape is (below the instance).
    member_maps: RefCell<HashMap<ShapeRef, Rc<ShapePaths>>>,
    /// Per master `ID`, the position of its one live top-level shape, if it has exactly one.
    singles: RefCell<HashMap<u32, Option<usize>>>,
    stats: StdCell<ResolverStats>,
}

/// A sheet's sections by key, for a sheet with many (a hostile file may hold 1,024): the position of the first section with each name
/// and `IX`, and the positions of all those with a name.
struct SectionIndex {
    by_key: HashMap<(SectionName, Option<u32>), usize>,
    by_name: HashMap<SectionName, Vec<usize>>,
}

/// What `style_enabled` knows of a style for a family: the answer, and how far down the style's own chain the flag that gave it was (0 for
/// the style's own). A walk looks at [`MAX_STYLE_STEPS`] styles at most, so an answer is only used by a walk that would have reached it.
#[derive(Clone, Copy)]
struct FlagMemo {
    answer: bool,
    /// `None` when the walk from this style ran out of steps without finding a flag: no flag is within reach, so the style takes part.
    reach: Option<usize>,
}

/// Resolves cells of one document. It holds the lookups that make a read cheap on a large document, so a caller that reads many
/// cells keeps one; [`resolve`] builds one for each call. It borrows the document, which cannot change while it exists. It is not `Sync`.
pub struct Resolver<'a> {
    doc: &'a Document,
    carried: Carried,
    /// Per sheet with many sections (by address), its sections by key.
    section_index: RefCell<HashMap<usize, Rc<SectionIndex>>>,
    /// Per style and family, whether the style takes part.
    flags: RefCell<HashMap<(u32, Family), FlagMemo>>,
}

impl<'a> Resolver<'a> {
    /// A resolver for `doc`. It builds nothing until a read needs it.
    pub fn new(doc: &'a Document) -> Resolver<'a> {
        Resolver::resume(doc, Carried::default())
    }

    /// A resolver for `doc` that starts from what an earlier one built for a document with the same pages, masters, styles and shapes
    /// (the same document after cells were written).
    pub(crate) fn resume(doc: &'a Document, carried: Carried) -> Resolver<'a> {
        Resolver { doc, carried, section_index: RefCell::new(HashMap::new()), flags: RefCell::new(HashMap::new()) }
    }

    /// What this resolver built that a write of a cell does not undo, to hand to the next one.
    pub(crate) fn into_carried(self) -> Carried {
        self.carried
    }

    /// The document.
    pub fn document(&self) -> &'a Document {
        self.doc
    }

    /// What has been built so far. Each count is work done once, whatever the number of reads since.
    #[doc(hidden)]
    pub fn stats(&self) -> ResolverStats {
        self.carried.stats.get()
    }

    fn count(&self, bump: impl FnOnce(&mut ResolverStats)) {
        crate::work::resolver_built();
        let mut stats = self.carried.stats.get();
        bump(&mut stats);
        self.carried.stats.set(stats);
    }

    // ---- looking things up ---------------------------------------------------------------------------------------------

    /// A map of a list by `ID`, built the first time it is asked: the first with an `ID` wins, as it does when a hostile file repeats one.
    fn id_map<'s, T>(
        &'s self,
        cell: &'s OnceCell<HashMap<u32, usize>>,
        items: &SharedVec<Arc<T>>,
        id: impl Fn(&T) -> u32,
    ) -> &'s HashMap<u32, usize> {
        cell.get_or_init(|| {
            self.count(|s| s.id_maps += 1);
            let mut map = HashMap::with_capacity(items.len());
            for (at, item) in items.iter().enumerate() {
                map.entry(id(item)).or_insert(at);
            }
            map
        })
    }

    /// The page with `id`, by the resolver's map of pages by `ID` (built once), where `Document::page` scans.
    pub fn page(&self, id: u32) -> Option<&'a Arc<Page>> {
        let at = *self.id_map(&self.carried.pages, &self.doc.pages, |p| p.id).get(&id)?;
        self.doc.pages.get(at)
    }

    /// The master with `id`, by the resolver's map (`Document::master` scans).
    pub fn master(&self, id: u32) -> Option<&'a Arc<Master>> {
        let at = *self.id_map(&self.carried.masters, &self.doc.masters, |m| m.id).get(&id)?;
        self.doc.masters.get(at)
    }

    /// The style with `id`, by the resolver's map (`Document::style` scans).
    pub fn style(&self, id: u32) -> Option<&'a Arc<Style>> {
        let at = *self.id_map(&self.carried.styles, &self.doc.styles, |s| s.id).get(&id)?;
        self.doc.styles.get(at)
    }

    /// The top-level shapes of a page or a master.
    pub fn shapes(&self, owner: Owner) -> Option<&'a ShapeList> {
        match owner {
            Owner::Page(id) => self.page(id).map(|p| &p.shapes),
            Owner::Master(id) => self.master(id).map(|m| &m.shapes),
        }
    }

    /// The shape at `at`, through the maps of pages and masters.
    pub fn shape(&self, at: &ShapeRef) -> Option<&'a Arc<Shape>> {
        shape_in(self.shapes(at.owner)?, &at.path)
    }

    /// The sheet a reference names.
    pub fn sheet(&self, at: &SheetRef) -> Option<&'a Sheet> {
        match at {
            SheetRef::Document => Some(&self.doc.sheet),
            SheetRef::Style(id) => self.style(*id).map(|s| &*s.sheet),
            SheetRef::Page(id) => self.page(*id).map(|p| &*p.sheet),
            SheetRef::Master(id) => self.master(*id).map(|m| &*m.sheet),
            SheetRef::Shape(shape) => self.shape(shape).map(|s| &*s.sheet),
        }
    }

    /// What `sheet` itself holds at `addr`, finding a section of a sheet that has many, and a row of a large section, by an index. The same
    /// answer as [`Sheet::lookup`], without a scan of every row of a large section for each call.
    pub fn lookup(&self, sheet: &'a Sheet, addr: &CellAddr) -> Lookup<'a> {
        sheet.lookup_with(addr, |sheet, key| self.find_section(sheet, key), |section, key| self.find_row(section, key))
    }

    /// The index of the sections of `sheet`, built the first time it is asked, for a sheet that has more than [`SECTION_SCAN_LIMIT`].
    fn sections_of(&self, sheet: &'a Sheet) -> Option<Rc<SectionIndex>> {
        if sheet.sections.len() <= SECTION_SCAN_LIMIT {
            return None;
        }
        // The sheet's address names it for as long as this resolver lives: the document cannot change under it.
        let id = std::ptr::from_ref(sheet) as usize;
        let mut indexes = self.section_index.borrow_mut();
        Some(Rc::clone(indexes.entry(id).or_insert_with(|| {
            self.count(|s| s.section_indexes += 1);
            let mut by_key = HashMap::with_capacity(sheet.sections.len());
            let mut by_name: HashMap<SectionName, Vec<usize>> = HashMap::new();
            for (at, section) in sheet.sections.iter().enumerate() {
                // The first section with a key wins, as in a scan.
                by_key.entry((section.name.clone(), section.ix)).or_insert(at);
                by_name.entry(section.name.clone()).or_default().push(at);
            }
            Rc::new(SectionIndex { by_key, by_name })
        })))
    }

    /// The first section of `sheet` with this key.
    fn find_section(&self, sheet: &'a Sheet, key: &SectionKey) -> Option<&'a Arc<Section>> {
        let Some(index) = self.sections_of(sheet) else {
            return sheet.section(key);
        };
        let at = index.by_key.get(&(key.name.clone(), key.ix))?;
        sheet.sections.get(*at)
    }

    /// The sections of `sheet` called `name`, with their places, in file order.
    fn sections_named(&self, sheet: &'a Sheet, name: &str) -> Vec<(usize, &'a Arc<Section>)> {
        match self.sections_of(sheet) {
            None => sheet.sections.iter().enumerate().filter(|(_, s)| s.name.as_str() == name).collect(),
            Some(index) => {
                let places = index.by_name.get(name).map(Vec::as_slice).unwrap_or_default();
                places.iter().filter_map(|at| sheet.sections.get(*at).map(|s| (*at, s))).collect()
            }
        }
    }

    /// How many sections of `sheet` are called `name`: a look at the index of a sheet that has one (a count of the places it holds), else at the few
    /// sections the sheet has.
    fn sections_counted(&self, sheet: &'a Sheet, name: &str) -> usize {
        match self.sections_of(sheet) {
            None => sheet.sections.iter().filter(|s| s.name.as_str() == name).count(),
            Some(index) => index.by_name.get(name).map_or(0, Vec::len),
        }
    }

    fn find_row(&self, section: &'a Section, key: &RowKey) -> Option<&'a Arc<Row>> {
        // A section of more than `ROW_SCAN_LIMIT` rows holds its own table of them (`Section::row`), built once however many resolvers ask.
        section.row(key)
    }

    // ---- masters -------------------------------------------------------------------------------------------------------

    /// The position of the one top-level shape of a master that has exactly one that is not deleted, found once
    /// for each master: a master of a million shapes with the first of them deleted costs a scan, not one for each read.
    fn single_shape(&self, master: &Master) -> Option<usize> {
        *self.carried.singles.borrow_mut().entry(master.id).or_insert_with(|| {
            self.count(|s| s.single_scans += 1);
            let mut live = master.shapes.iter().enumerate().filter(|(_, s)| !s.del).map(|(at, _)| at);
            let only = live.next()?;
            live.next().is_none().then_some(only)
        })
    }

    /// The path to the first shape with `shape_id` in the master, by a map built the first time the master is asked.
    fn master_shape_path(&self, master: &'a Master, shape_id: u32) -> Option<Vec<u32>> {
        let map = {
            let mut maps = self.carried.master_shapes.borrow_mut();
            Rc::clone(maps.entry(master.id).or_insert_with(|| {
                let paths = shape_paths(&master.shapes);
                self.count(|s| {
                    s.master_path_maps += 1;
                    s.path_slots += paths.slots();
                });
                Rc::new(paths)
            }))
        };
        map.path(shape_id)
    }

    /// The master shape the shape at `at` inherits from, if it does (masters).
    fn master_link(&self, at: &ShapeRef) -> MasterLink {
        // A shape in a master part takes no master step: a master is one level.
        if !matches!(at.owner, Owner::Page(_)) {
            return MasterLink::None;
        }
        let Some(list) = self.shapes(at.owner) else {
            return MasterLink::None;
        };
        let Some(chain) = chain_in(list, &at.path) else {
            return MasterLink::None;
        };
        let Some(shape) = chain.last() else {
            return MasterLink::None;
        };
        if let Some(master_id) = shape.master {
            // An instance: its master's one top-level shape. `MasterShape` is ignored beside `Master`.
            let Some(master) = self.master(master_id) else {
                return MasterLink::Dangling(Warnings::DANGLING_MASTER);
            };
            return match self.single_shape(master).and_then(|top| u32::try_from(top).ok()) {
                Some(top) => {
                    MasterLink::Found(MasterStep { shape: ShapeRef::top(Owner::Master(master_id), top), instance: at.clone(), master: master_id })
                }
                None => MasterLink::None,
            };
        }
        let Some(wanted) = shape.master_shape else {
            return MasterLink::None;
        };
        // A member: the shape it stands for in the master of the nearest ancestor that holds `Master`.
        let ancestors = chain.len().saturating_sub(1);
        let Some(up) = chain.iter().take(ancestors).rposition(|s| s.master.is_some()) else {
            return MasterLink::Dangling(Warnings::DANGLING_MASTER_SHAPE);
        };
        let Some(master_id) = chain.get(up).and_then(|s| s.master) else {
            return MasterLink::Dangling(Warnings::DANGLING_MASTER_SHAPE);
        };
        let Some(master) = self.master(master_id) else {
            return MasterLink::Dangling(Warnings::DANGLING_MASTER);
        };
        let Some(path) = self.master_shape_path(master, wanted) else {
            return MasterLink::Dangling(Warnings::DANGLING_MASTER_SHAPE);
        };
        let instance = ShapeRef { owner: at.owner, path: at.path.iter().take(up + 1).copied().collect() };
        MasterLink::Found(MasterStep { shape: ShapeRef { owner: Owner::Master(master_id), path }, instance, master: master_id })
    }

    /// The master shape the shape at `at` inherits from: `Ok(None)` for a shape that inherits from none (no
    /// `Master`, no `MasterShape`, a shape in a master part, an instance of a master that does not have exactly one shape), `Err` with the
    /// warning for a link that leads nowhere, in which case the shape resolves as if it had none. This is the lookup a reader of a
    /// shape's geometry, text or members needs to find what the shape is an instance of.
    pub fn master_step(&self, at: &ShapeRef) -> Result<Option<MasterStep>, Warnings> {
        match self.master_link(at) {
            MasterLink::None => Ok(None),
            MasterLink::Found(step) => Ok(Some(step)),
            MasterLink::Dangling(w) => Err(w),
        }
    }

    /// The shape `Sheet.<id>` names in a formula that was found on the master, for the inheritor: the
    /// instance itself for the master's one top-level shape, and otherwise the member of the instance that stands for it, by
    /// `MasterShape`. A nested shape with its own `Master` is another scope, and is not entered. `None` for a name the master does not
    /// hold, or one nothing in the instance stands for (a member removed with `Del`, or left out). The members of an instance are mapped
    /// once for the instance, however many names are asked.
    pub fn rebind(&self, rebind: &Rebind, id: u32) -> Option<ShapeRef> {
        let master = self.master(rebind.master)?;
        let path = self.master_shape_path(master, id)?;
        if let [top] = path.as_slice()
            && self.single_shape(master).and_then(|only| u32::try_from(only).ok()) == Some(*top)
        {
            return Some(rebind.instance.clone());
        }
        let members = self.member_paths(&rebind.instance)?;
        let below = members.path(id)?;
        let mut full = rebind.instance.path.clone();
        full.extend(below);
        (full.len() <= MAX_SHAPE_DEPTH).then_some(ShapeRef { owner: rebind.instance.owner, path: full })
    }

    /// The path, below `instance`, of the first member that stands for each master shape: not a deleted member and nothing under one, and
    /// not a shape with its own `Master` or anything under it.
    fn member_paths(&self, instance: &ShapeRef) -> Option<Rc<ShapePaths>> {
        if let Some(known) = self.carried.member_maps.borrow().get(instance) {
            return Some(Rc::clone(known));
        }
        let shape = self.shape(instance)?;
        let paths = shape_paths_of_members(&shape.children);
        self.count(|s| {
            s.member_maps += 1;
            s.path_slots += paths.slots();
        });
        let map = Rc::new(paths);
        self.carried.member_maps.borrow_mut().insert(instance.clone(), Rc::clone(&map));
        Some(map)
    }

    // ---- styles --------------------------------------------------------------------------------------------------------

    /// Whether a style takes part for a family: its `Enable...Props` flag is not 0. A flag a style
    /// does not hold is read along that style's own link of the family, over at most [`MAX_STYLE_STEPS`] styles from it; no flag within
    /// reach is on. The answer is kept for the style and for every style the walk passed on the way that it holds for, so a chain is read
    /// once. What is kept never depends on who asked first: an answer is used by a walk only if the flag it came from is within that
    /// walk's own reach, and a walk that ran out of steps keeps its answer for its first style only (a style after it has more styles to
    /// look at than the walk did).
    fn style_enabled(&self, style: &'a Style, family: Family) -> bool {
        if !STYLE_FLAG_SKIPS_STYLE {
            return true;
        }
        let memoised = self.flags.borrow().get(&(style.id, family)).copied();
        if let Some(known) = memoised {
            return known.answer;
        }
        self.count(|s| s.flag_walks += 1);
        let flag = match family {
            Family::Line => "EnableLineProps",
            Family::Fill => "EnableFillProps",
            Family::Text => "EnableTextProps",
        };
        let mut at = style;
        let mut visited = vec![style.id];
        // The answer and the position in the walk (0 for `style`) of the style that gave it. `None` for a walk that ran out of steps.
        let mut found: Option<(bool, usize)> = None;
        for position in 0..MAX_STYLE_STEPS {
            if let Some(cell) = at.sheet.cell(flag) {
                match cell.value.as_ref().and_then(|v| v.number()) {
                    Some(n) => {
                        found = Some((n != 0.0, position));
                        break;
                    }
                    // No number: an `Inh` flag reads on; any other cell with nothing to read leaves the style in.
                    None if cell.ends_search() => {
                        found = Some((true, position));
                        break;
                    }
                    None => {}
                }
            }
            let next = at.sheet.links.get(family).filter(|next| !visited.contains(next));
            let Some(next) = next else {
                // The chain ends (no link, or a cycle): no flag on it, so the style takes part.
                found = Some((true, position));
                break;
            };
            // Copied out, so that the memo is not borrowed while it is written below.
            let memoised = self.flags.borrow().get(&(next, family)).copied();
            if let Some(known) = memoised {
                match known.reach {
                    // The flag the next style found is within this walk's reach.
                    Some(reach) if position + 1 + reach < MAX_STYLE_STEPS => {
                        found = Some((known.answer, position + 1 + reach));
                        break;
                    }
                    // It is beyond it, or it was not found at all: this walk runs out of steps before it.
                    _ => {
                        self.flags.borrow_mut().insert((style.id, family), FlagMemo { answer: true, reach: None });
                        return true;
                    }
                }
            }
            let Some(next_style) = self.style(next) else {
                found = Some((true, position));
                break;
            };
            visited.push(next);
            at = next_style;
        }
        let mut memo = self.flags.borrow_mut();
        match found {
            Some((answer, end)) => {
                for (position, id) in visited.into_iter().enumerate() {
                    memo.insert((id, family), FlagMemo { answer, reach: Some(end.saturating_sub(position)) });
                }
                answer
            }
            None => {
                memo.insert((style.id, family), FlagMemo { answer: true, reach: None });
                true
            }
        }
    }

    fn style_chain(&self, start: Option<u32>, family: Family, check_flags: bool) -> StyleChain<'_, 'a> {
        StyleChain { resolver: self, next: start, seen: Vec::new(), family, check_flags, warnings: Warnings::default() }
    }

    // ---- the steps -----------------------------------------------------------------------------------------------------

    /// What is looked for after the sheet's own, for a cell of this class on this kind of sheet: the one place the order of
    /// is written, for a cell and for a whole section.
    fn plan(&self, sheet: &SheetRef, local: &'a Sheet, class: CellClass, warnings: &mut Warnings) -> Plan<'a> {
        match sheet {
            SheetRef::Shape(at) => {
                // A class with no master step (`page-sheet`) takes none.
                let master = (class != CellClass::PageSheet)
                    .then(|| match self.master_link(at) {
                        MasterLink::Found(step) => self.shape(&step.shape).map(|s| (step, &*s.sheet)),
                        MasterLink::Dangling(w) => {
                            warnings.add(w);
                            None
                        }
                        MasterLink::None => None,
                    })
                    .flatten();
                let styles = class.family().map(|family| StylePlan {
                    family,
                    // This sheet's own link, or if it has none the master shape's.
                    start: local.links.get(family).or_else(|| master.as_ref().and_then(|(_, s)| s.links.get(family))),
                    check_flags: true,
                });
                let root = matches!(class, CellClass::StyleAny | CellClass::Unlisted | CellClass::PageSheet);
                Plan { master, styles, root }
            }
            SheetRef::Style(id) => {
                let styles = match class {
                    CellClass::FillFamily | CellClass::LineFamily | CellClass::TextFamily => {
                        class.family().map(|family| StylePlan { family, start: local.links.get(family), check_flags: true })
                    }
                    CellClass::StyleFlag(family) => Some(StylePlan { family, start: local.links.get(family), check_flags: false }),
                    _ => None,
                };
                let root = *id != 0 && matches!(class, CellClass::StyleAny | CellClass::Unlisted | CellClass::PageSheet);
                Plan { master: None, styles, root }
            }
            // A page sheet, a master's page sheet and the document sheet: no master, and the links are not walked (
            // `page-sheet`, of the page batch; E6 leaves the document sheet open and it is read the same way).
            SheetRef::Page(_) | SheetRef::Master(_) | SheetRef::Document => Plan { master: None, styles: None, root: true },
        }
    }

    /// The sheets a cell of this class is looked for on, in order, nearest first: the sheet itself, its master shape, the style chain and
    /// the root. What `effective_rows` and `effective_sections` merge.
    fn layers(&self, sheet: &SheetRef, class: CellClass) -> Option<Vec<(Source, &'a Sheet)>> {
        let local = self.sheet(sheet)?;
        let mut warnings = Warnings::default();
        let plan = self.plan(sheet, local, class, &mut warnings);
        let mut layers = vec![(Source::Local, local)];
        if let Some((step, master_sheet)) = plan.master {
            layers.push((Source::Master(step.shape), master_sheet));
        }
        if let Some(styles) = plan.styles {
            for style in self.style_chain(styles.start, styles.family, styles.check_flags) {
                layers.push((Source::Style { id: style.id, family: Some(styles.family) }, &*style.sheet));
            }
        }
        if plan.root
            && let Some(root) = self.style(0)
        {
            layers.push((Source::Style { id: 0, family: None }, &*root.sheet));
        }
        Some(layers)
    }

    // ---- resolving -----------------------------------------------------------------------------------------------------

    /// Where the cell at `addr` of the sheet `sheet` gets its formula and value. `None` when the
    /// sheet does not exist, or when the section or row the address is in is deleted at any step (`Del`): nothing in it is
    /// inherited either.
    pub fn resolve(&self, sheet: &SheetRef, addr: &CellAddr) -> Option<Resolved<'a>> {
        self.resolve_from(sheet, addr, false)
    }

    /// Whether the cell at `addr` would be found somewhere if the sheet held no cell of its own there: the master, a style.
    pub fn inherits(&self, sheet: &SheetRef, addr: &CellAddr) -> bool {
        self.resolve_from(sheet, addr, true).is_some_and(|r| r.formula.source != Source::Default)
    }

    fn resolve_from(&self, sheet: &SheetRef, addr: &CellAddr, skip_local: bool) -> Option<Resolved<'a>> {
        let local = self.sheet(sheet)?;
        let info = cell_info(addr.section.as_ref().map(|k| k.name.as_str()), addr.cell.as_str());
        let mut walk = Walk {
            default: DefaultSpec { kind: info.kind, formula: info.default_formula },
            warnings: Warnings::default(),
            first_value: None,
            stopper: None,
            rebind: None,
            listed: info.listed,
            met_a_cell: false,
        };
        match self.lookup(local, addr) {
            Lookup::Deleted => return None,
            Lookup::Found(cell) if !skip_local && walk.visit(Source::Local, cell) => return Some(walk.finish(sheet.clone())),
            _ => {}
        }
        let plan = self.plan(sheet, local, info.class, &mut walk.warnings);
        if let Some((step, master_sheet)) = plan.master {
            match self.lookup(master_sheet, addr) {
                Lookup::Deleted => return walk.ended_by_a_deletion(sheet),
                Lookup::Found(cell) => {
                    let (instance, master) = (step.instance, step.master);
                    if walk.visit(Source::Master(step.shape), cell) {
                        walk.rebind = Some(Rebind { instance, master });
                        return Some(walk.finish(sheet.clone()));
                    }
                }
                Lookup::Missing => {}
            }
        }
        if let Some(styles) = plan.styles {
            let mut chain = self.style_chain(styles.start, styles.family, styles.check_flags);
            for style in &mut chain {
                match self.lookup(&style.sheet, addr) {
                    Lookup::Deleted => return walk.ended_by_a_deletion(sheet),
                    Lookup::Found(cell) => {
                        if walk.visit(Source::Style { id: style.id, family: Some(styles.family) }, cell) {
                            return Some(walk.finish(sheet.clone()));
                        }
                    }
                    Lookup::Missing => {}
                }
            }
            walk.warnings.add(chain.warnings);
        }
        if plan.root
            && let Some(root) = self.style(0)
        {
            match self.lookup(&root.sheet, addr) {
                Lookup::Deleted => return walk.ended_by_a_deletion(sheet),
                Lookup::Found(cell) => {
                    walk.visit(Source::Style { id: 0, family: None }, cell);
                }
                Lookup::Missing => {}
            }
        }
        Some(walk.finish(sheet.clone()))
    }

    // ---- sections and rows ---------------------------------------------------------------------------------------------

    /// The sections of the name `name` that a sheet has once its master's and its styles' are merged in (sections
    /// merge as rows and cells do), as keys: for `Geometry`, one for each `IX` any of them holds, in `IX` order, and for any other name
    /// at most one. A section deleted (`Del`) at a step is gone, whatever a step below it holds. `None` if the sheet does not exist.
    ///
    /// A section is found by name and exact `IX`: a local `Geometry` 3 is another section than the master's `Geometry` 0.
    pub fn effective_sections(&self, sheet: &SheetRef, name: &str) -> Option<Vec<SectionKey>> {
        let layers = self.layers(sheet, section_class(name))?;
        Some(self.merge_sections(&layers, name))
    }

    /// [`Resolver::effective_sections`] for a sheet that holds at most `max_sections` sections called `name`, counted over the sheet, its master and the styles
    /// before any is merged: `Err(sections)` with the count when it holds more, and then nothing is merged (a caller that lists the `Geometry` sections
    /// of a shape for a pointer's move must not pay for the eight thousand an instance of one master inherits, for each of a thousand guides). The count costs a
    /// look at each layer, not at each section. `None` if the sheet does not exist.
    pub fn effective_sections_within(&self, sheet: &SheetRef, name: &str, max_sections: usize) -> Option<Result<Vec<SectionKey>, usize>> {
        let layers = self.layers(sheet, section_class(name))?;
        let mut raw = 0usize;
        for (_, layer) in &layers {
            raw = raw.saturating_add(self.sections_counted(layer, name));
        }
        if raw > max_sections {
            return Some(Err(raw));
        }
        Some(Ok(self.merge_sections(&layers, name)))
    }

    /// The sections called `name` that the `layers` (nearest first) leave once merged, as keys.
    fn merge_sections(&self, layers: &[(Source, &'a Sheet)], name: &str) -> Vec<SectionKey> {
        let mut blocked: HashSet<Option<u32>> = HashSet::new();
        // The lowest layer each `IX` is in, and the section's place in it: the order a master's sections come in before the local ones.
        let mut found: Vec<(Option<u32>, (usize, usize))> = Vec::new();
        let mut seen_at: HashMap<Option<u32>, usize> = HashMap::new();
        for (depth, (_, layer)) in layers.iter().enumerate() {
            let mut here: HashSet<Option<u32>> = HashSet::new();
            let named = self.sections_named(layer, name);
            crate::work::sections_merge(named.len());
            for (place, section) in named {
                // The first section with an `IX` in a sheet is the one that counts, as in a lookup.
                if !here.insert(section.ix) || blocked.contains(&section.ix) {
                    continue;
                }
                if section.del {
                    blocked.insert(section.ix);
                    continue;
                }
                match seen_at.get(&section.ix) {
                    Some(at) => {
                        if let Some(entry) = found.get_mut(*at) {
                            entry.1 = (depth, place);
                        }
                    }
                    None => {
                        seen_at.insert(section.ix, found.len());
                        found.push((section.ix, (depth, place)));
                    }
                }
            }
        }
        if name == "Geometry" {
            found.sort_by_key(|(ix, _)| *ix);
        } else {
            found.sort_by_key(|(_, (depth, place))| (std::cmp::Reverse(*depth), *place));
        }
        found.into_iter().map(|(ix, _)| SectionKey { name: SectionName::new(name), ix }).collect()
    }

    /// The names of the sections a sheet has once its master's and its styles' are merged in: those of the sheet, of its master shape, of
    /// the root and of the style chains, each only if some section of the name is not deleted. In the order they are first met, the
    /// sheet's own first. `None` if the sheet does not exist.
    pub fn effective_section_names(&self, sheet: &SheetRef) -> Option<Vec<SectionName>> {
        // The sheets a section is looked for on depend on the class of its name: the four that matter, and others as they come up.
        let mut layers_of: HashMap<CellClass, Vec<(Source, &'a Sheet)>> = HashMap::new();
        let mut seen: HashSet<SectionName> = HashSet::new();
        let mut names: Vec<SectionName> = Vec::new();
        for class in [CellClass::Unlisted, CellClass::TextFamily, CellClass::FillFamily, CellClass::LineFamily] {
            let layers = self.layers(sheet, class)?;
            for (_, layer) in &layers {
                for section in &layer.sections {
                    if seen.insert(section.name.clone()) {
                        names.push(section.name.clone());
                    }
                }
            }
            layers_of.insert(class, layers);
        }
        let mut kept = Vec::with_capacity(names.len());
        for name in names {
            let class = section_class(name.as_str());
            let layers = match layers_of.entry(class) {
                Entry::Occupied(known) => known.into_mut(),
                Entry::Vacant(slot) => slot.insert(self.layers(sheet, class)?),
            };
            if !self.merge_sections(layers, name.as_str()).is_empty() {
                kept.push(name);
            }
        }
        Some(kept)
    }

    /// The rows of a section after merging cell by cell: the rows of the sheet, its master shape and the style
    /// chain (for a section of cells in a family) and the root, each row holding every cell any of them holds, resolved as
    /// [`Resolver::resolve`] does. A row deleted (`Del`) at a step is gone, whatever a step below it holds, and so is every row of a
    /// section deleted at a step. A section keyed by index is in index order; any other has the rows of the lowest step first. `None`
    /// if the sheet does not exist.
    pub fn effective_rows(&self, sheet: &SheetRef, section: &SectionKey) -> Option<Vec<EffectiveRow<'a>>> {
        let layers = self.layers(sheet, section_class(section.name.as_str()))?;
        Some(self.merge_rows(sheet, section, &layers))
    }

    /// [`Resolver::effective_rows`] for a section of at most `max_rows` rows, counted over the sheet, its master and the styles before any is merged:
    /// `Err(rows)` with the count when it has more, and then nothing is merged or resolved (a caller that reads a section for a pointer's
    /// move must not pay for a hundred thousand inherited rows each time). `None` if the sheet does not exist.
    pub fn effective_rows_within(&self, sheet: &SheetRef, section: &SectionKey, max_rows: usize) -> Option<Result<Vec<EffectiveRow<'a>>, usize>> {
        let layers = self.layers(sheet, section_class(section.name.as_str()))?;
        let raw = self.rows_counted(&layers, section);
        if raw > max_rows {
            return Some(Err(raw));
        }
        Some(Ok(self.merge_rows(sheet, section, &layers)))
    }

    /// The type `T` of one row, as [`Resolver::effective_rows`] gives it (the nearest row that has one; none for a row deleted at a step, which is gone
    /// whatever a step below it holds, and none for a row or a section that is not there) **without merging the section**: a look at each layer and
    /// the row's place in it. A caller that asks it for many rows of one section pays for them, not for the section each time.
    pub fn row_type(&self, sheet: &SheetRef, section: &SectionKey, row: &RowKey) -> Option<Arc<str>> {
        let layers = self.layers(sheet, section_class(section.name.as_str()))?;
        for (_, layer) in &layers {
            let Some(found) = self.find_section(layer, section) else { continue };
            if found.del {
                return None;
            }
            if let Some(held) = found.row(row) {
                if held.del {
                    return None;
                }
                if held.row_type.is_some() {
                    return held.row_type.clone();
                }
            }
        }
        None
    }

    /// How many rows a section holds over the sheet, its master and the styles, before any is merged: the rows [`Resolver::effective_rows`] would walk, which
    /// is what a caller that keeps a budget of rows charges for a read (the merged rows are no more than that, and may be fewer). `None` if the sheet does not
    /// exist. It costs a look at each layer, not at each row.
    pub fn rows_held(&self, sheet: &SheetRef, section: &SectionKey) -> Option<usize> {
        let layers = self.layers(sheet, section_class(section.name.as_str()))?;
        Some(self.rows_counted(&layers, section))
    }

    fn rows_counted(&self, layers: &[(Source, &'a Sheet)], section: &SectionKey) -> usize {
        let mut raw = 0usize;
        for (_, layer) in layers {
            let Some(found) = self.find_section(layer, section) else { continue };
            if found.del {
                break;
            }
            raw = raw.saturating_add(found.rows.len());
        }
        raw
    }

    fn merge_rows(&self, sheet: &SheetRef, section: &SectionKey, layers: &[(Source, &'a Sheet)]) -> Vec<EffectiveRow<'a>> {
        let name = section.name.as_str();
        struct Merged {
            key: RowKey,
            row_type: Option<Arc<str>>,
            cells: BTreeSet<CellName>,
            /// The lowest layer the row is in and its place there.
            rank: (usize, usize),
        }
        let mut blocked: HashSet<RowKey> = HashSet::new();
        let mut at: HashMap<RowKey, usize> = HashMap::new();
        let mut merged: Vec<Merged> = Vec::new();
        'layers: for (depth, (_, layer)) in layers.iter().enumerate() {
            let Some(found) = self.find_section(layer, section) else {
                continue;
            };
            if found.del {
                break 'layers;
            }
            crate::work::rows_merge(found.rows.len());
            let mut here: HashSet<RowKey> = HashSet::new();
            for (place, row) in found.rows.iter().enumerate() {
                let Some(key) = row.key(name) else {
                    continue;
                };
                // The first row with a key in a section is the one that counts, as in a lookup.
                if !here.insert(key.clone()) || blocked.contains(&key) {
                    continue;
                }
                if row.del {
                    blocked.insert(key);
                    continue;
                }
                let slot = *at.entry(key.clone()).or_insert_with(|| {
                    merged.push(Merged { key, row_type: None, cells: BTreeSet::new(), rank: (depth, place) });
                    merged.len().saturating_sub(1)
                });
                if let Some(m) = merged.get_mut(slot) {
                    m.cells.extend(row.cells.keys().cloned());
                    // The nearest row that has a `T` gives it; the lowest row a row is in gives its place.
                    if m.row_type.is_none() {
                        m.row_type = row.row_type.clone();
                    }
                    m.rank = (depth, place);
                }
            }
        }
        if keying(name) == Keying::Indexed {
            merged.sort_by(|a, b| a.key.cmp(&b.key));
        } else {
            merged.sort_by_key(|m| (std::cmp::Reverse(m.rank.0), m.rank.1));
        }
        let mut rows = Vec::with_capacity(merged.len());
        for m in merged {
            let mut cells = Vec::with_capacity(m.cells.len());
            for cell in m.cells {
                let addr = CellAddr { section: Some(section.clone()), row: Some(m.key.clone()), cell: cell.clone() };
                if let Some(resolved) = self.resolve(sheet, &addr) {
                    cells.push((cell, resolved));
                }
            }
            rows.push(EffectiveRow { key: m.key, row_type: m.row_type, cells });
        }
        rows
    }

    /// The section `section` as each sheet a cell of it is looked for on holds it, nearest first: the sheet, its master shape, the style chain
    /// and the root, the layers [`Resolver::effective_rows`] merges, with no cell resolved. A layer that holds no such
    /// section adds nothing, and a layer whose section is deleted (`Del`) ends the list, as it ends a lookup. `None` if the sheet does
    /// not exist. A caller that wants the names of the rows (a lookup that ignores letter case) lists them from these, once.
    pub fn section_layers(&self, sheet: &SheetRef, section: &SectionKey) -> Option<Vec<&'a Arc<Section>>> {
        let layers = self.layers(sheet, section_class(section.name.as_str()))?;
        let mut found = Vec::new();
        for (_, layer) in layers {
            let Some(here) = self.find_section(layer, section) else { continue };
            if here.del {
                break;
            }
            found.push(here);
        }
        Some(found)
    }

    /// The members of a shape once its master is put together with it: for an
    /// instance or a member, the shapes of the master's matching shape (an instance of a master with one shape) or of the master itself
    /// (an instance of a master with several), each as the member of the page that stands for it by `MasterShape`, or [`Member::Omitted`]
    /// when the instance leaves it out; and the shapes the page added, which stand for nothing. A member with `Del` is gone, and so is a
    /// master shape it stands for.
    ///
    /// **The order is the page's.** The display order is the document's, so the shapes the page lists come in file order, those
    /// standing for a master shape and those it added alike. An omitted shape has no place of its own in the file: it follows the member
    /// that stands for the nearest master shape before it that the page has (a member with `Del` keeps its place for this), and leads the
    /// list, in the master's order, when no master shape before it is there. For a shape with no master it is its own members, in file
    /// order. `None` if there is no such shape.
    pub fn effective_members(&self, parent: &ShapeRef) -> Option<Vec<Member>> {
        let shape = self.shape(parent)?;
        let own = |i: usize| u32::try_from(i).ok().map(|i| parent.child(i));
        let master_children = self.master_children(parent, shape);
        // Omitted shapes by the member they follow (`None`: they lead), in the master's order.
        let mut after: HashMap<Option<usize>, Vec<ShapeRef>> = HashMap::new();
        if !master_children.is_empty() {
            // The first member that stands for each master shape. A member with its own `Master` stands for nothing of this master.
            let mut standing: HashMap<u32, usize> = HashMap::new();
            for (i, child) in shape.children.iter().enumerate() {
                if child.master.is_none()
                    && let Some(id) = child.master_shape
                {
                    standing.entry(id).or_insert(i);
                }
            }
            let mut anchor: Option<usize> = None;
            for master_child in master_children {
                let Some(master_shape) = self.shape(&master_child) else {
                    continue;
                };
                if master_shape.del {
                    continue;
                }
                match standing.get(&master_shape.id) {
                    Some(&i) => anchor = Some(i),
                    None => after.entry(anchor).or_default().push(master_child),
                }
            }
        }
        let mut members = Vec::new();
        let omitted = |members: &mut Vec<Member>, key: Option<usize>, after: &mut HashMap<Option<usize>, Vec<ShapeRef>>| {
            members.extend(after.remove(&key).into_iter().flatten().map(Member::Omitted));
        };
        omitted(&mut members, None, &mut after);
        for (i, child) in shape.children.iter().enumerate() {
            if !child.del {
                members.push(Member::Local(own(i)?));
            }
            omitted(&mut members, Some(i), &mut after);
        }
        Some(members)
    }

    /// The shapes of the master that stand for the members of the shape at `at`: its master shape's own children, or for an instance of a
    /// master with several top-level shapes (a group) those shapes.
    fn master_children(&self, at: &ShapeRef, shape: &Shape) -> Vec<ShapeRef> {
        match self.master_link(at) {
            MasterLink::Found(step) => {
                let count = self.shape(&step.shape).map_or(0, |s| s.children.len());
                (0..count).filter_map(|i| u32::try_from(i).ok()).map(|i| step.shape.child(i)).collect()
            }
            MasterLink::None => {
                let Some(master_id) = shape.master.filter(|_| matches!(at.owner, Owner::Page(_))) else {
                    return Vec::new();
                };
                let count = self.master(master_id).map_or(0, |m| m.shapes.len());
                (0..count).filter_map(|i| u32::try_from(i).ok()).map(|i| ShapeRef::top(Owner::Master(master_id), i)).collect()
            }
            MasterLink::Dangling(_) => Vec::new(),
        }
    }
}

/// Every shape of a master part by `ID`.
fn shape_paths(top: &ShapeList) -> ShapePaths {
    map_shapes(top, |shape| (Visit::Descend, Some(shape.id)))
}

/// The first member that stands for each master shape, in document order: a shape deleted (`Del`) and a shape with its own `Master` are
/// not looked at, nor anything under them.
fn shape_paths_of_members(members: &ShapeList) -> ShapePaths {
    map_shapes(members, |shape| if shape.del || shape.master.is_some() { (Visit::Skip, None) } else { (Visit::Descend, shape.master_shape) })
}

/// Resolves one cell directly under a sheet. A convenience that builds a [`Resolver`] for the call: a caller
/// that reads many cells keeps one.
pub fn resolve<'a>(doc: &'a Document, sheet: SheetRef, cell: &CellName) -> Option<Resolved<'a>> {
    Resolver::new(doc).resolve(&sheet, &CellAddr { section: None, row: None, cell: cell.clone() })
}
