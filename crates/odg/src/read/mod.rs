//! Reading a drawing into the ShapeSheet model.
//!
//! # The order of a read
//!
//! 1. **The package** (`Package::read_kept`): the archive and the manifest, with the trees of `content.xml` and `styles.xml` left to
//!    the steps that convert them, each parsed once (`Package::parse_tree`, which looks it through for macros as `Package::read` does), so
//!    that a read never holds both. Whatever the package refuses is the answer. A package with no `content.xml` is `no_drawing_content`.
//!    **Every file is inflated**, each other file charged to the media budget before it is inflated, so that the model can keep it (step 6).
//! 2. **`meta.xml` and `settings.xml`**, small parts parsed here under [`Limits::max_small_part_bytes`]: the generator, and the drawing scale
//!    And what the model keeps of them (`keep.rs`: the title and the properties, the fields kept whole, the settings tree).
//! 3. **`styles.xml`**, its tree parsed now: the font faces, the common and default styles and their dashes, markers,
//!    gradients and list styles, its automatic styles and page layouts, the layer set, and the master pages. Then the root style (the default
//!    styles over DiaCraft's new-drawing root), the style sheets, and the background pages of the master pages
//!    that hold shapes. The tree is dropped. Each style's property values are parsed once, as the style is read
//!    (`typed.rs`), and the names it holds (a dash, a gradient, a marker, the styles its DiaCraft links name) are looked up once, when the
//!    part that holds the style has been read (`StyleSet::resolve`): a run, a paragraph, a list item or a shape that shows the style reads
//!    what was found, whatever the length of the strings the style holds.
//! 4. **`content.xml`**, its tree parsed likewise: its font faces and automatic styles, then each `draw:page` with its shapes. The tree is
//!    dropped.
//!
//! **The `.odg` data of the model**: beside the cells, the reader fills the `odf` slots with what no cell reads and with what cells only approximate: the
//! document's named definitions, layer set and other style elements (`odfdoc.rs`), a style sheet's own data (`styles.rs`), a page's master page and layout (`pages.rs`), and a
//! shape's held values with their stamps, units and style name, a glue point on its row, a picture's attributes and a text's paragraphs (`held.rs`, `text.rs`). Each is charged
//! before it is made, and a style's values are made once and shared by every shape that names it.
//! 5. **The document**: pages in document order with `ID`s 0, 1, 2, ..., the background pages after them, the styles, and the
//!    [`ShapeIndex`], checked once more against the model's caps.
//! 6. **What the model keeps beside the drawing** (`keep.rs`): the font faces, the roots of the parts, `mimetype`, the manifest's rows, and every package entry no
//!    field reads as a part of the model, its bytes the package's own. Each is charged before it is made.
//!
//! A part that is not XML the container accepts, or whose root is not its fixed one, refuses the file. Every cap is checked before what
//! it counts is stored; nothing is truncated. What the model has no slot for is counted in the report (`kept`) and stays in the file's
//! bytes, which the caller keeps.

mod alt;
pub(crate) mod cells;
pub(crate) mod comments;
mod connectors;
mod custom;
mod equations;
mod flat;
pub(crate) mod geometry;
mod gradient;
mod held;
pub(crate) use self::held::{End as HeldEnd, Value as HeldValue};
mod keep;
pub(crate) mod layers;
mod links;
pub(crate) mod names;
mod odfdoc;
mod outputs;
mod pages;
mod preset;
pub(crate) mod props;
pub(crate) mod readback;
mod shapes;
pub use shapes::TEXT_OBJECT_ROW;
mod sheets;
mod spelled;
mod styles;
mod text;
mod texts;
mod typed;
pub(crate) mod units;
mod unread;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use diacraft_container::xml::{self, El};
use diacraft_doc::{Document, Value};

use crate::error::{LimitKind, Missing, OdgError};
use crate::index::ShapeIndex;
use crate::limits::Limits;
use crate::package::Package;
use crate::report::{ReadReport, shown};
use styles::Tab;

pub(crate) use connectors::glue_formula;
pub(crate) use connectors::glue_row_beside;
pub(crate) use connectors::{Boxed, Place};
/// For the Kani harnesses of the crate (`crate::kani_proofs`).
#[cfg(kani)]
pub(crate) use connectors::{glue_row, own_glue_row};
pub(crate) use custom::reads_no_glue_points;
pub(crate) use shapes::text_turn_beside_flips;
pub(crate) use shapes::{Affine, FrameRead, frame_read, transform_map, turn_beside_mirrors};
pub(crate) use shapes::{Kindof, glue_box};

/// The items a cell costs the document's total ([`Limits::max_items`]): what the VSDX reader charges for the same cell's markup (a `Cell`
/// element and its `N`, `V` and `U` attributes, four XML items). At [`ITEM_BYTES`] an item that is 256 bytes: a cell's entry in its map
/// (its name and the cell, 104 bytes) in a node at least half full, and its value's text.
pub(crate) const ITEMS_PER_CELL: usize = 4;

/// The bytes of the model's memory one item stands for, where a thing is charged by its size (text, names, rows, shapes):
/// see `limits.rs` for what the item cap then bounds.
pub(crate) const ITEM_BYTES: usize = 64;

/// What the model's parts cost in items ([`ITEM_BYTES`] each), from the sizes of their own types: a map of cells allocates a node of eleven
/// entries for its first cell (the standard library's `BTreeMap` node holds eleven), and each part behind an `Arc` carries two counts.
pub(crate) mod cost {
    use std::mem::size_of;

    use diacraft_doc::{Cell, CellName, Row, Section, Shape, Sheet};

    use super::ITEM_BYTES;

    /// A map's first node of cells.
    const NODE: usize = 16 + 11 * (size_of::<CellName>() + size_of::<Cell>());
    /// The counts in front of what an `Arc` holds.
    const ARC: usize = 16;

    /// A row and its first node of cells: a row of up to eleven cells holds no more (its cells are charged on their own besides).
    pub(crate) const ROW: usize = (ARC + size_of::<Row>() + NODE).div_ceil(ITEM_BYTES);
    /// A section and its place in its sheet.
    pub(crate) const SECTION: usize = (ARC + size_of::<Section>() + 8).div_ceil(ITEM_BYTES);
    /// The node of cells of a section that holds cells of its own (a `Geometry` section's `NoFill`).
    pub(crate) const SECTION_CELLS: usize = NODE.div_ceil(ITEM_BYTES);
    /// The extension slot of a shape that holds `.odg` data (`Shape::ext`): the allocation behind its pointer, rounded up to whole items. The attributes and
    /// the nodes it holds are charged on their own, where they are made.
    pub(crate) const EXT_SHAPE: usize = (ARC + size_of::<diacraft_doc::ExtData<diacraft_doc::OdfShape>>()).div_ceil(ITEM_BYTES);
    /// The extension slot of a page that holds `.odg` data (`Page::ext`).
    pub(crate) const EXT_PAGE: usize = (ARC + size_of::<diacraft_doc::ExtData<diacraft_doc::OdfPage>>()).div_ceil(ITEM_BYTES);
    /// The extension slot of a style sheet that holds `.odg` data (`Style::ext`).
    pub(crate) const EXT_STYLE: usize = (ARC + size_of::<diacraft_doc::ExtData<diacraft_doc::OdfStyle>>()).div_ceil(ITEM_BYTES);
    /// The extension slot of the document (`Document::ext`): one, whatever it holds.
    pub(crate) const EXT_DOC: usize = (ARC + size_of::<diacraft_doc::ExtData<diacraft_doc::OdfDoc>>()).div_ceil(ITEM_BYTES);
    /// The extension slot of a `Connection` row that holds a glue point's attributes (`Row::ext`).
    pub(crate) const EXT_ROW: usize = (ARC + size_of::<diacraft_doc::ExtData<diacraft_doc::OdfRow>>()).div_ceil(ITEM_BYTES);
    /// The extension slot of a text that holds the file's paragraphs (`Text::ext`).
    pub(crate) const EXT_TEXT: usize = (ARC + size_of::<diacraft_doc::ExtData<diacraft_doc::OdfText>>()).div_ceil(ITEM_BYTES);
    /// The extension slot of a picture's `ForeignData` that holds the `draw:image` attributes.
    pub(crate) const EXT_FOREIGN: usize = (ARC + size_of::<diacraft_doc::ExtData<diacraft_doc::Kept>>()).div_ceil(ITEM_BYTES);
    /// One held value of a shape (`OdfShape::held`), in whole items: the list is made exactly as long as it is (`shrink_to_fit`), so that `n` values are `n * 40 + 16` bytes (the
    /// allocation's own) and `n` items cover them.
    pub(crate) const HELD: usize = size_of::<diacraft_doc::Held>().div_ceil(ITEM_BYTES);
    /// One paragraph of a text's `OdfText` (`OdfParagraph`), in whole items.
    pub(crate) const TEXT_PARAGRAPH: usize = size_of::<diacraft_doc::OdfParagraph>().div_ceil(ITEM_BYTES);
    /// One level of the lists round a paragraph (`OdfListLevel`), in whole items.
    pub(crate) const TEXT_LEVEL: usize = size_of::<diacraft_doc::OdfListLevel>().div_ceil(ITEM_BYTES);
    /// One run of a paragraph (`OdfRun`), in whole items.
    pub(crate) const TEXT_RUN: usize = size_of::<diacraft_doc::OdfRun>().div_ceil(ITEM_BYTES);
    /// A shape and its sheet, each an allocation of its own behind an `Arc` and each rounded up to whole items, and the sheet's first
    /// node of cells.
    pub(crate) const SHAPE: usize =
        (ARC + size_of::<Shape>()).div_ceil(ITEM_BYTES) + (ARC + size_of::<Sheet>()).div_ceil(ITEM_BYTES) + NODE.div_ceil(ITEM_BYTES);

    /// What text of `n` bytes is charged, in bytes: **nine eighths of its length, rounded up**, the eighth that the law of the memory tests holds
    /// for the size classes of the app's allocator beside what the model keeps (`diacraft_sheetxml::model::cost::text_bytes`, the codec's). Text charged by its length
    /// alone fell short of the law by it on a kind made mostly of text: a paragraph of 64 KiB was charged 0.991 of what the law asks, a kept text of 1 MiB 0.894.
    pub(crate) const fn text(n: usize) -> usize {
        n.saturating_add(n.div_ceil(8))
    }

    // ---- what a kind keeps, by the law of the memory tests ----------------------------------------------------------------------------------------------

    /// What the allocators give an allocation of `n` bytes **by the law the memory tests hold a model to** (`tests/it/memory_counted.rs`; widened by
    /// to the allocator the app ships with): the bytes, an eighth of them for the size classes of mimalloc, and the most glibc adds to one (24).
    pub(crate) const fn held(n: usize) -> usize {
        n.saturating_add(n / 8).saturating_add(24)
    }

    /// What an entry of `size` bytes holds in a list that grows by doubling, **at its emptiest** (room for twice what it holds, at a count one past a power of two),
    /// with the eighth of the law: the list is one allocation for all its entries, so no entry pays the 24 of one.
    pub(crate) const fn slot(size: usize) -> usize {
        let room = size.saturating_mul(2);
        room.saturating_add(room / 8)
    }

    /// The items that pay for `need` bytes (counted by [`held`] and [`slot`]) **a quarter above**, in whole items: the rule the memory prices are raised by when a
    /// count finds one short.
    pub(crate) const fn priced(need: usize) -> usize {
        need.saturating_add(need / 4).div_ceil(ITEM_BYTES)
    }

    /// A page, beside its cells, its attributes and its extension slot (charged where they are made): the page and its sheet, each behind an `Arc`, its two names and
    /// the names of its master page and layout (`Arc<str>`s, whose bytes are charged where they are read), its place in the document's list of pages and its place in
    /// the index of the read ([`crate::index::PageShapes`], which the engine holds beside the model). Counted, an empty page kept 3,016 bytes in 20
    /// allocations and was charged 44 items: 2,816 bytes, where the law asks 3,875.
    pub(crate) const PAGE: usize = priced(
        held(ARC + size_of::<diacraft_doc::Page>())
            + held(ARC + size_of::<Sheet>())
            + 4 * held(ARC)
            + slot(size_of::<std::sync::Arc<diacraft_doc::Page>>())
            + slot(size_of::<crate::index::PageShapes>()),
    );

    /// A part of a text (`Text::parts`: a run of characters, a marker, a field) beside the bytes of its characters: its place in the list of parts, which grows by
    /// doubling, and the `Arc<str>` of a run of characters. It was one item; counted, a paragraph of a span (a run of characters, a marker and a run of
    /// characters more) kept 640 bytes in 3 allocations and was charged 10 items, where the law asks 792.
    pub(crate) const TEXT_PART: usize = priced(slot(size_of::<diacraft_doc::TextPart>()) + held(ARC));

    /// A node of markup kept whole in a list of kept nodes (`Passthrough`), beside the items of the markup itself (`shapes::element_cost`): its place in the list,
    /// which grows by doubling. It was not charged; counted, a kept hyperlink's wrapper round a rectangle kept 752 bytes in 17 allocations and was
    /// charged 18 items, where the law asks 1,254.
    pub(crate) const KEPT_SLOT: usize = priced(slot(size_of::<diacraft_doc::Placed>()));

    /// The marker section `User.DiaCraftRoute` of a connector, beside its one cell: the section and its row behind their `Arc`s, the list of its rows (room for four at
    /// the first), the row's node of cells, its name and its place in the sheet's list of sections. It was charged a section and a row (24 items); counted
    /// A connector kept 9,616 bytes in 49 allocations and was charged 187 items, where the law asks 11,995.
    pub(crate) const ROUTE_MARKER: usize = priced(
        held(ARC + size_of::<Section>())
            + held(4 * size_of::<std::sync::Arc<Row>>())
            + held(ARC + size_of::<Row>())
            + held(NODE)
            + held(ARC + "DiaCraftRoute".len())
            + slot(size_of::<std::sync::Arc<Section>>()),
    );

    /// A row of alt text (`User.visAltText` or `User.visAltDescription`, `alt.rs`) beside its two cells and its text (charged where they are made): the row behind its `Arc`, its
    /// node of cells, its name and its place in the section's list of rows.
    pub(crate) const ALT_ROW: usize =
        priced(held(ARC + size_of::<Row>()) + held(NODE) + held(ARC + "visAltDescription".len()) + slot(size_of::<std::sync::Arc<Row>>()));

    /// The `User` section a shape that had none is given for its alt text: the section behind its `Arc`, the list of its rows (room for four at the first) and its place in
    /// the sheet's list of sections.
    pub(crate) const ALT_SECTION: usize =
        priced(held(ARC + size_of::<Section>()) + held(4 * size_of::<std::sync::Arc<Row>>()) + slot(size_of::<std::sync::Arc<Section>>()));

    /// A `Connect` row of a connector's glued end: its place in the page's list of connects, which grows by doubling, and its two cell names (`BeginX` or `EndX`,
    /// and `PinX` or `Connections.X<n>`, at most 24 bytes each). It was four items; counted, a glued connector's two ends kept 400 bytes in 4
    /// allocations and were charged 8 items, where the law asks 546.
    pub(crate) const CONNECT: usize = priced(slot(size_of::<diacraft_doc::Connect>()) + 2 * held(ARC + 24));
}

/// Reads the drawing in `bytes` within `limits`. See the module documentation.
pub fn read(bytes: &[u8], limits: &Limits) -> Result<(Document, ReadReport), OdgError> {
    let (result, report) = read_partial(bytes, limits);
    result.map(|doc| (doc, report))
}

/// As [`read`], and the report is given whether or not the drawing was read: a refused file reports what the reader knew when it refused it.
pub fn read_partial(bytes: &[u8], limits: &Limits) -> (Result<Document, OdgError>, ReadReport) {
    let mut reader = Reader::new(limits);
    let result = reader.read_drawing(bytes);
    let report = reader.finish();
    match result {
        Ok(doc) => match doc.check_limits() {
            Ok(()) => {
                // What the model holds, counted from the model, for the save's gate to compare the caller's model with later.
                let mut report = report;
                report.index.set_figures(crate::figures::of(&doc));
                (Ok(doc), report)
            }
            Err(e) => (Err(OdgError::Model(e)), report),
        },
        Err(e) => (Err(e), report),
    }
}

/// Everything one read keeps: the caps and what is left of each, the report, and the formulas parsed so far.
pub(crate) struct Reader<'l> {
    pub limits: &'l Limits,
    pub report: ReadReport,
    items_left: usize,
    cells_left: usize,
    shapes_left: usize,
    text_left: usize,
    pub media_left: u64,
    /// What is left of the bytes DiaCraft's entries of one file may hold (`ext::items::MAX_CARRIED_BYTES`).
    pub(crate) carried_left: u64,
    pub forms: cells::Forms,
    /// Each style's text shown in cells, by a hash of the text, so that two places holding equal texts share one value
    /// ([`Reader::shared_value`]); the values by where their text is held are in each [`texts::StyleTexts`].
    style_texts_by_hash: HashMap<u64, Vec<Value>>,
    /// Each style's tab stops, by a hash of the stops, so that two styles holding equal stops share one list ([`Reader::shared_tabs`]).
    tab_lists_by_hash: HashMap<u64, Vec<Rc<[Tab]>>>,
    /// Debug builds: the address of the text of each value `shared_value` made longer than [`cells::SHORT_TEXT`], the only long texts a
    /// cell of a text row may hold (`text::key_of` names such a value by where it is held, which is sound only for a value that stays put
    /// for the whole read).
    #[cfg(debug_assertions)]
    shared_long: HashSet<usize>,
    /// Warning classes given once per document.
    once: HashSet<&'static str>,
    /// Style names already warned of as naming no style (one warning per name).
    missing: HashSet<u64>,
    /// The sheet reader the fragments of DiaCraft's extension are read through, with the budgets of this read.
    pub(crate) ext: crate::ext::payload::ReadState,
    /// DiaCraft's document file's entries for groups, read before the pages so that a group is given its data as it is built.
    pub(crate) groups: crate::ext::docfile::Groups,
    /// Style names already warned of as held twice by one container (one warning per name), by a hash of the name.
    repeated: HashSet<u64>,
    /// The `Layer` section a page with no layer set of its own was given (the document's), found at the first such page: `Some(None)` for a document whose layer set
    /// holds no row, `None` while no page has been read without a layer set of its own.
    document_layers: Option<Option<std::sync::Arc<diacraft_doc::Section>>>,
    /// The font faces the two parts' `office:font-face-decls` declared, held as the document's font table (`keep.rs`).
    pub(crate) faces: keep::Faces,
    /// What the document's containers hold that no field reads (`unread.rs`).
    pub(crate) unread: unread::Unread,
    /// The comments the pages' annotations gave, and what they have taken of the model's caps so far (`comments.rs`).
    pub(crate) gather: comments::Gather,
}

/// The hash a list of tab stops is found by (`Reader::shared_tabs`): its length, and each stop's position, unit and alignment. Every field
/// of a stop takes part, so that two lists that differ in one are, but for a collision, found apart; a collision is then settled by comparing
/// the lists, which is what makes the list shared by identity the list that was asked for.
fn tab_hash(tabs: &[Tab]) -> u64 {
    use std::hash::{Hash, Hasher};
    #[cfg(test)]
    if tests::COLLIDE.with(std::cell::Cell::get) {
        return 0;
    }
    let mut hasher = std::hash::DefaultHasher::new();
    tabs.len().hash(&mut hasher);
    for tab in tabs {
        // `-0` and `0` are equal positions: one bit pattern for both.
        (tab.position.inches + 0.0).to_bits().hash(&mut hasher);
        (tab.position.unit as u8).hash(&mut hasher);
        tab.alignment.hash(&mut hasher);
    }
    hasher.finish()
}

impl<'l> Reader<'l> {
    pub(crate) fn new(limits: &'l Limits) -> Reader<'l> {
        Reader {
            limits,
            report: ReadReport::default(),
            items_left: limits.max_items,
            cells_left: limits.max_cells,
            shapes_left: limits.max_shapes,
            text_left: limits.max_text_bytes,
            media_left: limits.max_media_bytes,
            carried_left: crate::ext::items::MAX_CARRIED_BYTES,
            forms: cells::Forms::with_budget(limits.max_items.saturating_mul(ITEM_BYTES) / cells::FORMULA_NODE_BYTES),
            style_texts_by_hash: HashMap::new(),
            tab_lists_by_hash: HashMap::new(),
            #[cfg(debug_assertions)]
            shared_long: HashSet::new(),
            once: HashSet::new(),
            missing: HashSet::new(),
            ext: crate::ext::payload::ReadState::new(diacraft_sheetxml::Caps {
                cells: limits.max_cells,
                items: limits.max_items,
                formula_nodes: limits.max_items.saturating_mul(ITEM_BYTES) / cells::FORMULA_NODE_BYTES,
            }),
            groups: crate::ext::docfile::Groups::default(),
            repeated: HashSet::new(),
            document_layers: None,
            faces: keep::Faces::default(),
            unread: unread::Unread::default(),
            gather: comments::Gather::default(),
        }
    }

    fn finish(mut self) -> ReadReport {
        // One word for the document about the shapes drawn as boxes for want of a kind this reader knows: the box is all that is shown of
        // them, and a person is told, with how many (the renderer cannot tell such a shape from a rectangle).
        let unknown = self.report.boxed_shapes;
        if unknown > 0 {
            self.warn("unknown_shape_type", || {
                if unknown == 1 {
                    "1 custom shape is of a kind DiaCraft does not know and is drawn as a box".to_owned()
                } else {
                    format!("{unknown} custom shapes are of a kind DiaCraft does not know and are drawn as boxes")
                }
            });
        }
        // What the drawing does not hold: a line with its count for each class, the first of the warnings, so that the open says them whatever else it says.
        let lines = self.report.not_kept_note();
        if !lines.is_empty() {
            let count = self.report.warning_counts.entry(crate::report::NOT_KEPT_WARNING).or_insert(0);
            *count = count.saturating_add(lines.len() as u64);
            let shown = lines.len();
            self.report.warnings.splice(0..0, lines);
            self.report.warnings.truncate(self.limits.max_report_entries.max(shown));
        }
        let cost = self.report.work.cost();
        self.report.index.set_read_cost(cost);
        self.report
    }

    /// `n` more of `class` that the drawing does not hold ([`ReadReport::not_kept`]): counted where the read meets them, and said once, in the note.
    pub(crate) fn not_kept(&mut self, class: &'static str, n: u64) {
        let count = self.report.not_kept.entry(class).or_insert(0);
        *count = count.saturating_add(n);
    }

    /// A warning of `class`: counted, and named in words while the list has room.
    pub(crate) fn warn(&mut self, class: &'static str, words: impl FnOnce() -> String) {
        let count = self.report.warning_counts.entry(class).or_insert(0);
        *count = count.saturating_add(1);
        if self.report.warnings.len() < self.limits.max_report_entries {
            self.report.warnings.push(words());
        }
    }

    /// `n` warnings of `class` at once (a shape's whole list of stale items, or of fingerprints that are none): all counted, and named in words once per document
    /// whatever `n` is, so that a drawing of 100,000 stale items has one line of words and not 100,000.
    pub(crate) fn warn_counted(&mut self, class: &'static str, n: u64, words: impl FnOnce() -> String) {
        let count = self.report.warning_counts.entry(class).or_insert(0);
        *count = count.saturating_add(n);
        if self.once.insert(class) && self.report.warnings.len() < self.limits.max_report_entries {
            self.report.warnings.push(words());
        }
    }

    /// A warning of `class` given at most once per document.
    pub(crate) fn warn_once(&mut self, class: &'static str, words: impl FnOnce() -> String) {
        if self.once.insert(class) {
            self.warn(class, words);
        }
    }

    /// Whether `name` is a style name not yet warned of as naming no style; it is noted, as a hash and not a copy, so that names nothing
    /// holds cost eight bytes each however long they are (two names of one hash share one warning).
    pub(crate) fn first_missing(&mut self, name: &str) -> bool {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        name.hash(&mut hasher);
        self.missing.insert(hasher.finish())
    }

    /// Whether `name` is a style name not yet warned of as held twice by a container; noted as a hash, as [`Reader::first_missing`] notes its names.
    pub(crate) fn first_repeated(&mut self, name: &str) -> bool {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        name.hash(&mut hasher);
        self.repeated.insert(hasher.finish())
    }

    /// One more thing of `kind` kept without a cell or a slot in the model.
    pub(crate) fn keep(&mut self, kind: &'static str) {
        self.keep_n(kind, 1);
    }

    /// `n` more things of `kind` kept without a slot in the model.
    pub(crate) fn keep_n(&mut self, kind: &'static str, n: u64) {
        let count = self.report.kept.entry(kind).or_insert(0);
        *count = count.saturating_add(n);
    }

    /// Takes `n` items from the document's total, before what they pay for is stored.
    pub(crate) fn take_items(&mut self, n: usize) -> Result<(), OdgError> {
        // The formulas parsed since the last charge are paid for with these items: a cell's tree costs its nodes, whatever the text's length.
        let (formula_bytes, formula_nodes) = self.forms.settle();
        self.report.work.formula_nodes = self.report.work.formula_nodes.saturating_add(formula_nodes);
        // A tree the document's budget of nodes could not hold was not built: the document is past its items, as the charge would say.
        if self.forms.over_budget() {
            return Err(OdgError::Limit { kind: LimitKind::Items, max: self.limits.max_items as u64 });
        }
        let n = n.saturating_add(formula_bytes.div_ceil(ITEM_BYTES));
        match self.items_left.checked_sub(n) {
            Some(left) => {
                self.items_left = left;
                self.report.work.items = self.report.work.items.saturating_add(n as u64);
                Ok(())
            }
            None => Err(OdgError::Limit { kind: LimitKind::Items, max: self.limits.max_items as u64 }),
        }
    }

    /// Takes the items text of `n` bytes stands for ([`ITEM_BYTES`] each), before it is stored: text, names and other strings, by their length, **at nine eighths of it**
    /// ([`cost::text`]).
    pub(crate) fn take_bytes(&mut self, n: usize) -> Result<(), OdgError> {
        self.take_items(cost::text(n).div_ceil(ITEM_BYTES))
    }

    /// Takes the items `n` bytes stand for as they are: what is not text the model keeps by its length (a decode's spend, priced by the codec already; the backstop of a
    /// part's bytes; the base64 of bytes the model keeps decoded, three quarters of it; the work of a fingerprint or of a compiled expression).
    pub(crate) fn take_model_bytes(&mut self, n: usize) -> Result<(), OdgError> {
        self.take_items(n.div_ceil(ITEM_BYTES))
    }

    /// The one value a style's text is shown by (a font's family, a bullet's characters), made when the text is first shown: it is hashed
    /// once, an equal text held in another place gives the value it already has, and a new one is charged by its bytes before it is made.
    /// [`texts::StyleTexts::value`] finds it again by where the text is held, so that a family named by many styles is one copy and not one
    /// for each conversion.
    fn shared_value(&mut self, text: &str) -> Result<Value, OdgError> {
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::hash::DefaultHasher::new();
            text.hash(&mut hasher);
            hasher.finish()
        };
        self.take_items(1)?;
        let same = self.style_texts_by_hash.get(&hash).and_then(|list| list.iter().find(|v| v.text() == text)).cloned();
        match same {
            Some(value) => Ok(value),
            None => {
                self.take_bytes(text.len())?;
                let value = Value::from_text(text);
                #[cfg(debug_assertions)]
                if text.len() > cells::SHORT_TEXT {
                    self.shared_long.insert(value.text().as_ptr() as usize);
                }
                self.style_texts_by_hash.entry(hash).or_default().push(value.clone());
                Ok(value)
            }
        }
    }

    /// Debug builds: asserts that `text`, a cell's text longer than [`cells::SHORT_TEXT`], is a value [`Reader::shared_value`] made, which
    /// the reader holds for the whole read, so that the address a row's key names it by is never freed and never taken by another text. A
    /// new kind of cell that holds a long text of any other making fails here, in every test that reads one.
    #[cfg(debug_assertions)]
    pub(crate) fn assert_shared_long(&self, text: &str) {
        debug_assert!(
            self.shared_long.contains(&(text.as_ptr() as usize)),
            "a cell of a text row holds a text of {} bytes that is not a shared value",
            text.len()
        );
    }

    /// The one list a style's tab stops are shared as, made when the style is read: a list equal to one an earlier style holds is that list,
    /// found by a hash of its stops and then by comparing them, so equal lists are one list and two lists that are not the same list are
    /// different ones. A shape-style pair then compares the lists it shows by identity (`sheets::same_tabs`) and reads no stop, however many
    /// stops (up to `styles::MOST_TABS`) the styles hold; the walk is made here, for each style read, and its stops are counted
    /// ([`crate::ReadWork::tab_stops_walked`]).
    pub(crate) fn shared_tabs(&mut self, tabs: Vec<Tab>) -> Rc<[Tab]> {
        let hash = tab_hash(&tabs);
        // The hash walks the stops once, and each list of the same hash and length is walked once to compare it.
        let mut walked = tabs.len();
        let mut same = None;
        for list in self.tab_lists_by_hash.get(&hash).map_or(&[][..], Vec::as_slice) {
            if list.len() != tabs.len() {
                continue;
            }
            walked = walked.saturating_add(tabs.len());
            if **list == *tabs {
                same = Some(Rc::clone(list));
                break;
            }
        }
        self.report.work.tab_stops_walked = self.report.work.tab_stops_walked.saturating_add(walked as u64);
        match same {
            Some(list) => list,
            None => {
                let made: Rc<[Tab]> = Rc::from(tabs);
                self.tab_lists_by_hash.entry(hash).or_default().push(Rc::clone(&made));
                made
            }
        }
    }

    /// Takes `n` bytes of text from the document's total ([`Limits::max_text_bytes`]), and the items they stand for, before the text is made.
    pub(crate) fn take_text(&mut self, n: usize) -> Result<(), OdgError> {
        match self.text_left.checked_sub(n) {
            Some(left) => {
                self.text_left = left;
                self.report.work.text_bytes = self.report.work.text_bytes.saturating_add(n as u64);
                self.take_bytes(n)
            }
            None => Err(OdgError::Limit { kind: LimitKind::TextBytes, max: self.limits.max_text_bytes as u64 }),
        }
    }

    /// Takes `n` cells from the document's total, and [`ITEMS_PER_CELL`] items for each, before they are stored.
    pub(crate) fn take_cells(&mut self, n: usize) -> Result<(), OdgError> {
        match self.cells_left.checked_sub(n) {
            Some(left) => {
                self.cells_left = left;
                self.report.stats.cells = self.report.stats.cells.saturating_add(n);
                self.take_items(n.saturating_mul(ITEMS_PER_CELL))
            }
            None => Err(OdgError::Limit { kind: LimitKind::Cells, max: self.limits.max_cells as u64 }),
        }
    }

    /// What is left of the items a read may take: the room a decode of something the codec reads is given to spend, before it builds anything.
    pub(crate) fn items_room(&self) -> usize {
        self.items_left
    }

    /// Counts `n` cells against the document's cells and the figures of the read, and takes no items: for the cells of a sheet the codec decoded, which charged its own
    /// items and bytes ([`Reader::take_cells`] takes [`ITEMS_PER_CELL`] besides).
    pub(crate) fn count_cells(&mut self, n: usize) -> Result<(), OdgError> {
        match self.cells_left.checked_sub(n) {
            Some(left) => {
                self.cells_left = left;
                self.report.stats.cells = self.report.stats.cells.saturating_add(n);
                Ok(())
            }
            None => Err(OdgError::Limit { kind: LimitKind::Cells, max: self.limits.max_cells as u64 }),
        }
    }

    /// Counts `n` shapes against the document's shapes, and takes no items (see [`Reader::count_cells`]).
    pub(crate) fn count_shapes(&mut self, n: usize) -> Result<(), OdgError> {
        match self.shapes_left.checked_sub(n) {
            Some(left) => {
                self.shapes_left = left;
                Ok(())
            }
            None => Err(OdgError::Limit { kind: LimitKind::Shapes, max: self.limits.max_shapes as u64 }),
        }
    }

    /// Takes one shape from the document's total, before it is built.
    pub(crate) fn take_shape(&mut self) -> Result<(), OdgError> {
        match self.shapes_left.checked_sub(1) {
            Some(left) => {
                self.shapes_left = left;
                self.take_items(cost::SHAPE)
            }
            None => Err(OdgError::Limit { kind: LimitKind::Shapes, max: self.limits.max_shapes as u64 }),
        }
    }

    pub(crate) fn saw_element(&mut self) {
        self.report.work.elements = self.report.work.elements.saturating_add(1);
    }

    /// `n` elements and attributes a decode of the codec read, counted as the tree they were (the figure a save's memory estimate goes by).
    pub(crate) fn saw_elements(&mut self, n: u64) {
        self.report.work.elements = self.report.work.elements.saturating_add(n);
    }

    pub(crate) fn saw_attrs(&mut self, n: usize) {
        self.report.work.attributes = self.report.work.attributes.saturating_add(n as u64);
    }

    pub(crate) fn style_step(&mut self) {
        self.report.work.style_steps = self.report.work.style_steps.saturating_add(1);
    }

    pub(crate) fn path_units(&mut self, n: usize) {
        self.report.work.path_units = self.report.work.path_units.saturating_add(n as u64);
    }

    pub(crate) fn text_units(&mut self, n: usize) {
        self.report.work.text_units = self.report.work.text_units.saturating_add(n as u64);
    }

    /// `n` bytes of a style name hashed to look a list style or a font face up by name.
    pub(crate) fn name_bytes(&mut self, n: usize) {
        self.report.work.name_bytes = self.report.work.name_bytes.saturating_add(n as u64);
    }

    /// `n` more style values parsed.
    pub(crate) fn value_parses(&mut self, n: u64) {
        self.report.work.value_parses = self.report.work.value_parses.saturating_add(n);
    }

    /// `n` levels of a list style visited to choose the one a list's depth names.
    pub(crate) fn list_levels(&mut self, n: u64) {
        self.report.work.list_levels = self.report.work.list_levels.saturating_add(n);
    }

    /// `n` bytes of two cells' texts compared one by one.
    pub(crate) fn compared_bytes(&mut self, n: usize) {
        self.report.work.compared_bytes = self.report.work.compared_bytes.saturating_add(n as u64);
    }

    /// `n` entries of a cell list visited by the search for a cell of the same name.
    pub(crate) fn cells_scanned(&mut self, n: usize) {
        self.report.work.cells_scanned = self.report.work.cells_scanned.saturating_add(n as u64);
    }

    /// A length read in `px`: warns once per document.
    pub(crate) fn saw_px(&mut self, length: units::Length) {
        if length.unit == units::LenUnit::Px {
            self.warn_once("length_in_px", || "a length is written in px, which is read as 1/96 inch".to_owned());
        }
    }

    /// The whole read (steps 1 to 5).
    fn read_drawing(&mut self, bytes: &[u8]) -> Result<Document, OdgError> {
        // A flat drawing is one XML document, read as the package that holds the same elements (`read/flat.rs`).
        if crate::Form::of(bytes) == crate::Form::Flat {
            return self.read_flat(bytes);
        }
        let mut work = diacraft_container::Work::default();
        // The two trees are parsed one at a time, each when it is converted, so that a read never holds both (`Package::read_deferred`).
        let package = Package::read_kept(bytes, self.limits, &mut work);
        self.report.work.package = work;
        let mut package = package?;
        self.report.variant = package.variant();
        self.report.signed = package.signed();
        for w in package.warnings() {
            let words = w.to_string();
            self.warn(w.class(), || words);
        }
        // A file nothing reads that is damaged is left out of the model's parts, and said: a damaged entry does not stop a read.
        let damaged = package.damaged().len();
        if damaged > 0 {
            let first = package.damaged().first().map(|name| shown(name)).unwrap_or_default();
            self.warn_counted("part_not_kept", damaged as u64, || {
                format!("{damaged} files of the package are damaged and were not kept (the first is \"{first}\"): they are left out of the model and are in the file's bytes as they were")
            });
        }
        if package.part("content.xml").is_none() {
            return Err(OdgError::NoDrawingContent { missing: Missing::ContentPart });
        }
        // Step 2. What the read keeps of the document's own parts (`keep.rs`): `meta.xml` into fields, `settings.xml` whole, and the roots.
        let mut held = keep::Held::default();
        if let Some(meta) = self.small_part(&package, "meta.xml")? {
            self.report.generator = pages::generator(self, &meta)?;
            held.meta = keep::meta(self, &meta)?;
            if held.meta.is_some() {
                held.root(self, "meta.xml", &meta.attrs)?;
            }
        }
        let scale = match self.small_part(&package, "settings.xml")? {
            Some(settings) => {
                let scale = pages::scale(self, &settings)?;
                held.settings = Some(keep::settings(self, &settings)?);
                held.root(self, "settings.xml", &settings.attrs)?;
                scale
            }
            None => None,
        };
        // Step 3. The common styles are all `styles.xml`'s, so the style sheets are complete before the content part is read.
        let mut set = styles::StyleSet::default();
        let mut res = styles::Resolver::default();
        // Shared by the master pages' shapes and the pages': the shared sections and the pictures.
        let mut shared = shapes::Shared::default();
        let styles_tree = self.tree(&mut package, "styles.xml");
        let (sheets, masters) = match styles_tree {
            None => (sheets::build(self, &mut set, &mut res)?, Vec::new()),
            Some(Err(error)) => return Err(error),
            Some(Ok(tree)) => {
                let mut names = names::Names::default();
                let mark = names.enter(&tree);
                if !names.is(&tree, names::Ns::Office, "document-styles") {
                    return Err(OdgError::UnexpectedRoot { part: "styles.xml" });
                }
                held.root(self, "styles.xml", &tree.attrs)?;
                let masters = pages::read_styles_part(self, &mut names, &tree, &mut set, None)?;
                // The part is read, so every dash, gradient and marker a name may mean is known: the names are looked up, once.
                set.resolve(self, 0);
                let sheets = sheets::build(self, &mut set, &mut res)?;
                let out = {
                    let mut cx = shapes::Cx {
                        set: &set,
                        res: &mut res,
                        sheets: &sheets,
                        package: Some(&package),
                        file: styles::File::Styles,
                        styles: HashMap::new(),
                        shared: &mut shared,
                        texts: texts::StyleTexts::of(&set),
                        legacy: crate::write::is_legacy(names.attrs(&tree).peek(names::Ns::Office, "version")),
                    };
                    pages::read_master_pages(self, &mut names, &masters, &mut cx, scale.as_ref())?
                };
                // The automatic styles of this part that no master page's shape named.
                set.hold_unused(self, &mut names, &tree, styles::File::Styles, None)?;
                names.leave(mark);
                (sheets, out)
            }
        };
        // Step 4.
        let tree = match self.tree(&mut package, "content.xml") {
            Some(Ok(tree)) => tree,
            Some(Err(error)) => return Err(error),
            None => return Err(OdgError::NoDrawingContent { missing: Missing::ContentPart }),
        };
        // Both trees are looked through for macros now.
        self.report.macros = package.has_macros();
        let mut names = names::Names::default();
        let mark = names.enter(&tree);
        if !names.is(&tree, names::Ns::Office, "document-content") {
            return Err(OdgError::UnexpectedRoot { part: "content.xml" });
        }
        held.root(self, "content.xml", &tree.attrs)?;
        self.report.office_version = names.attrs(&tree).peek(names::Ns::Office, "version").map(shown);
        let first = set.list.len();
        pages::read_content_styles(self, &mut names, &tree, &mut set)?;
        set.resolve(self, first);
        res.prepare(self, &set);
        // DiaCraft's document file is read ahead of the pages (a group's data is put on the group as it is built); what else it holds goes over the document after.
        let mut file = crate::ext::docfile::parse(self, &package)?;
        if let Some(root) = &file {
            self.groups = crate::ext::docfile::read_groups(self, root)?;
        }
        let pages = {
            let mut cx = shapes::Cx {
                set: &set,
                res: &mut res,
                sheets: &sheets,
                package: Some(&package),
                file: styles::File::Content,
                styles: HashMap::new(),
                shared: &mut shared,
                texts: texts::StyleTexts::of(&set),
                legacy: crate::write::is_legacy(names.attrs(&tree).peek(names::Ns::Office, "version")),
            };
            pages::read_content_pages(self, &mut names, &tree, &mut cx, &masters, scale.as_ref(), None)?
        };
        // The automatic styles of this part that no shape, paragraph or run named.
        set.hold_unused(self, &mut names, &tree, styles::File::Content, None)?;
        names.leave(mark);
        drop(tree);
        // Step 5.
        let mut doc = Document::empty();
        let mut index = ShapeIndex::default();
        pages::assemble(self, &mut doc, &mut index, pages, masters)?;
        doc.styles = diacraft_doc::SharedVec::from_vec(sheets.into_styles());
        self.report.stats.styles = doc.styles.len();
        // The document's own ODF data: the definitions, the layer set and the style elements kept whole.
        let fill_entries = odfdoc::finish(self, &mut doc, std::mem::take(&mut set.doc), Some(&package))?;
        // DiaCraft's document file, over what the markup gave.
        if self.groups.unclaimed() > 0 {
            self.warn("diacraft_group_unknown", || {
                "DiaCraft's document file holds data for a page the drawing does not have by that name".to_owned()
            });
        }
        if self.groups.left_over() > 0 {
            self.warn("diacraft_group_unclaimed", || {
                "DiaCraft's document file holds data for a group that its page does not have by that name (it was renamed or taken away in another program): that data is not read".to_owned()
            });
        }
        self.report.native = crate::ext::docfile::apply(self, &mut doc, file.as_mut(), Some(&package))?;
        // What a page the writer adds reads as its `Layer` rows: it has no layer set of its own, so the document's (a page of the file may have one of its own,
        // which the writer's page-added markup does not). The first page that has none gave it (read and charged); a file whose pages all have one is read for it here.
        self.report.native.new_page_layers = match self.document_layers.take() {
            Some(rows) => rows,
            None => {
                let mut bare = diacraft_doc::Sheet::default();
                layers::layer_rows(self, &mut bare, set.layers.as_deref())?;
                bare.sections.pop()
            }
        };
        // The document file may give a page an `ID` of its own: the index goes by the `ID`s the model has.
        for (at, page) in doc.pages.iter().enumerate() {
            index.set_page_id(at, page.id);
        }
        // The pages' annotations are the document's comments, now that every page has its `ID`.
        comments::finish(self, &mut doc)?;
        // Tier 3: the links to a master that went stale keep the shapes' own copies of what the master gave them.
        links::keep_copies(self, &mut doc, &index)?;
        // The font faces, `meta.xml`, `settings.xml`, the roots, the manifest's rows and the entries nothing else reads (`keep.rs`).
        // (A fill image's picture is one of the definitions' bytes, and no part beside.)
        let mut pictures: HashSet<&str> = shared.picture_entries().collect();
        pictures.extend(fill_entries.iter().map(String::as_str));
        keep::apply(self, &mut doc, held, Some(&package), &pictures)?;
        // The document's own items and its parts', now that its title and properties are read.
        if let Some(root) = &file {
            crate::ext::items::read_late(self, &mut doc, root, Some(&package))?;
        }
        self.report.index = index;
        Ok(doc)
    }

    /// The tree of `name`, one of the package's two tree parts, parsed now and counted.
    fn tree(&mut self, package: &mut Package, name: &str) -> Option<Result<El, OdgError>> {
        let mut work = diacraft_container::Work::default();
        // The reader's trees have a tighter item cap than the package layer's (`Limits::max_read_xml_nodes`).
        let mut limits = self.limits.clone();
        limits.container.xml.max_nodes = limits.container.xml.max_nodes.min(limits.max_read_xml_nodes);
        let tree = package.parse_tree(name, &limits, &mut work);
        self.report.work.package.xml_events = self.report.work.package.xml_events.saturating_add(work.xml_events);
        tree
    }

    /// The tree of a small part (`meta.xml`, `settings.xml`), parsed here: `None` when the package has none, or one larger than
    /// [`Limits::max_small_part_bytes`], which is left unread with a warning.
    fn small_part(&mut self, package: &Package, name: &'static str) -> Result<Option<El>, OdgError> {
        let Some(bytes) = package.part(name) else { return Ok(None) };
        let most = self.limits.max_small_part_bytes;
        if bytes.len() as u64 > most {
            self.warn("part_left_unread", || format!("{name} is larger than {most} bytes and was not read"));
            return Ok(None);
        }
        self.report.work.small_part_bytes = self.report.work.small_part_bytes.saturating_add(bytes.len() as u64);
        let mut work = diacraft_container::Work::default();
        let tree =
            xml::parse_counted(bytes, &self.limits.container.xml, &mut work).map_err(|error| OdgError::Xml { part: name.to_owned(), error })?;
        self.report.work.package.xml_events = self.report.work.package.xml_events.saturating_add(work.xml_events);
        Ok(Some(tree))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    thread_local! {
        /// Test hook: every list of tab stops hashes to 0, so that lists found by the same hash are told apart by comparing them alone.
        pub(super) static COLLIDE: Cell<bool> = const { Cell::new(false) };
    }

    use super::Reader;
    use super::styles::Tab;
    use super::units::{LenUnit, Length};
    use crate::limits::Limits;

    /// A tab stop at `position` in `unit`, in inches as the reader reads one.
    fn tab(position: f64, unit: LenUnit, alignment: u8) -> Tab {
        Tab { position: Length { inches: position, unit }, alignment }
    }

    /// `warn_counted` counts every warning it is given, names a class once whatever the count, and names no class once the report holds its most entries (a
    /// drawing of 100,000 stale items has one line of words).
    #[test]
    fn warnings_counted_at_once_are_all_counted_and_named_once_within_the_report_cap() {
        let limits = Limits { max_report_entries: 2, ..Limits::default() };
        let mut r = Reader::new(&limits);
        r.warn_counted("first", 100_000, || "one".to_owned());
        r.warn_counted("first", 5, || "again".to_owned());
        assert_eq!(r.report.warning_counts.get("first"), Some(&100_005));
        assert_eq!(r.report.warnings, ["one"], "named once");
        r.warn_counted("second", 1, || "two".to_owned());
        assert_eq!(r.report.warnings, ["one", "two"], "the report has room for two");
        r.warn_counted("third", 7, || "three".to_owned());
        assert_eq!(r.report.warnings.len(), 2, "and no more");
        assert_eq!(r.report.warning_counts.get("third"), Some(&7), "but a class past the cap is counted");
        r.warn_counted("max", u64::MAX, || "x".to_owned());
        r.warn_counted("max", 1, || "y".to_owned());
        assert_eq!(r.report.warning_counts.get("max"), Some(&u64::MAX), "saturating");
    }

    #[test]
    fn equal_tab_lists_are_one_list_and_lists_that_differ_are_not() {
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        let list = || vec![tab(1.0, LenUnit::Cm, 0), tab(2.0, LenUnit::Cm, 2)];
        let first = r.shared_tabs(list());
        assert_eq!(r.report.work.tab_stops_walked, 2, "the hash of two stops");
        let again = r.shared_tabs(list());
        assert!(Rc::ptr_eq(&first, &again));
        assert_eq!(r.report.work.tab_stops_walked, 2 + 2 + 2, "the hash, and the walk that found the first list equal");
        // A stop more, a stop fewer, another place, unit, alignment or order: another list each.
        let others = [
            vec![tab(1.0, LenUnit::Cm, 0)],
            vec![tab(1.0, LenUnit::Cm, 0), tab(2.0, LenUnit::Cm, 2), tab(3.0, LenUnit::Cm, 2)],
            vec![tab(1.0, LenUnit::Cm, 0), tab(2.5, LenUnit::Cm, 2)],
            vec![tab(1.0, LenUnit::Mm, 0), tab(2.0, LenUnit::Cm, 2)],
            vec![tab(1.0, LenUnit::Cm, 0), tab(2.0, LenUnit::Cm, 1)],
            vec![tab(2.0, LenUnit::Cm, 2), tab(1.0, LenUnit::Cm, 0)],
            Vec::new(),
        ];
        let mut seen = vec![first];
        for other in others {
            let made = r.shared_tabs(other.clone());
            assert!(seen.iter().all(|s| !Rc::ptr_eq(s, &made)), "{other:?}");
            assert_eq!(*made, *other);
            // Asked again, it is the same list it was.
            assert!(Rc::ptr_eq(&made, &r.shared_tabs(other)));
            seen.push(made);
        }
    }

    #[test]
    fn minus_zero_and_zero_are_one_position() {
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        let (zero, minus) = (r.shared_tabs(vec![tab(0.0, LenUnit::Cm, 0)]), r.shared_tabs(vec![tab(-0.0, LenUnit::Cm, 0)]));
        assert!(Rc::ptr_eq(&zero, &minus));
    }

    #[test]
    fn a_list_is_hashed_once_and_walked_again_only_against_a_list_of_its_own_hash() {
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        let of = |n: usize, last: f64| -> Vec<Tab> { (0..n).map(|k| tab(if k + 1 == n { last } else { k as f64 }, LenUnit::In, 0)).collect() };
        r.shared_tabs(of(100, 1.0));
        assert_eq!(r.report.work.tab_stops_walked, 100);
        // Lists of another length, and a list of the same length that differs in its last stop, are each hashed and found by no one: no
        // list has their hash, so none is walked to compare.
        r.shared_tabs(of(101, 1.0));
        r.shared_tabs(of(100, 2.0));
        assert_eq!(r.report.work.tab_stops_walked, 100 + 101 + 100);
        // Asked again, the first is hashed and walked once against its own copy.
        r.shared_tabs(of(100, 1.0));
        assert_eq!(r.report.work.tab_stops_walked, 100 + 101 + 100 + 100 + 100);
    }

    /// Each field of a stop takes part in the hash: lists that differ in one, and only in it, hash apart.
    #[test]
    fn every_field_of_a_tab_stop_and_the_length_of_the_list_is_hashed() {
        use super::tab_hash;
        let base = vec![tab(1.0, LenUnit::Cm, 0), tab(2.0, LenUnit::Cm, 2)];
        let h = tab_hash(&base);
        assert_eq!(h, tab_hash(&base.clone()), "the hash is a function of the list");
        let others = [
            ("position", vec![tab(1.5, LenUnit::Cm, 0), tab(2.0, LenUnit::Cm, 2)]),
            ("unit", vec![tab(1.0, LenUnit::Mm, 0), tab(2.0, LenUnit::Cm, 2)]),
            ("alignment", vec![tab(1.0, LenUnit::Cm, 1), tab(2.0, LenUnit::Cm, 2)]),
            ("alignment of the last", vec![tab(1.0, LenUnit::Cm, 0), tab(2.0, LenUnit::Cm, 3)]),
            ("length", vec![tab(1.0, LenUnit::Cm, 0)]),
            ("order", vec![tab(2.0, LenUnit::Cm, 2), tab(1.0, LenUnit::Cm, 0)]),
        ];
        for (field, list) in others {
            assert_ne!(h, tab_hash(&list), "{field}");
        }
        // `-0` and `0` are one position.
        assert_eq!(tab_hash(&[tab(0.0, LenUnit::Cm, 0)]), tab_hash(&[tab(-0.0, LenUnit::Cm, 0)]));
    }

    /// With every list hashing to one value, lists are found apart by the comparison that follows a hash hit, and the lists of another length
    /// are not walked.
    #[test]
    fn lists_that_collide_are_told_apart_by_comparing_them() {
        COLLIDE.with(|c| c.set(true));
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        let a = r.shared_tabs(vec![tab(1.0, LenUnit::Cm, 0), tab(2.0, LenUnit::Cm, 2)]);
        assert_eq!(r.report.work.tab_stops_walked, 2, "the hash of two stops");
        // One alignment apart: the same hash, not the same list.
        let b = r.shared_tabs(vec![tab(1.0, LenUnit::Cm, 0), tab(2.0, LenUnit::Cm, 1)]);
        assert!(!Rc::ptr_eq(&a, &b));
        assert_eq!(b[1].alignment, 1);
        assert_eq!(r.report.work.tab_stops_walked, 2 + 2 + 2, "hashed, and walked against the one list of its length");
        // A list of another length shares the hash and is passed over without a walk.
        let c = r.shared_tabs(vec![tab(1.0, LenUnit::Cm, 0)]);
        assert!(!Rc::ptr_eq(&a, &c) && !Rc::ptr_eq(&b, &c));
        assert_eq!(c.len(), 1);
        assert_eq!(r.report.work.tab_stops_walked, 6 + 1, "hashed only: the two lists of two stops are not walked");
        // Each is found again as itself, whichever the order it is asked in.
        assert!(Rc::ptr_eq(&b, &r.shared_tabs(vec![tab(1.0, LenUnit::Cm, 0), tab(2.0, LenUnit::Cm, 1)])));
        assert!(Rc::ptr_eq(&a, &r.shared_tabs(vec![tab(1.0, LenUnit::Cm, 0), tab(2.0, LenUnit::Cm, 2)])));
        assert!(Rc::ptr_eq(&c, &r.shared_tabs(vec![tab(1.0, LenUnit::Cm, 0)])));
        // The position and the unit are told apart by the comparison too.
        let d = r.shared_tabs(vec![tab(1.0, LenUnit::Cm, 0), tab(2.5, LenUnit::Cm, 2)]);
        let e = r.shared_tabs(vec![tab(1.0, LenUnit::Cm, 0), tab(2.0, LenUnit::In, 2)]);
        assert!(!Rc::ptr_eq(&a, &d) && !Rc::ptr_eq(&a, &e) && !Rc::ptr_eq(&d, &e));
        COLLIDE.with(|c| c.set(false));
    }

    /// The formulas parsed since the last charge are paid for, exactly, with the next items taken: each node at 128 bytes and the text twice
    /// (it is kept for sharing), in whole items, once.
    #[test]
    fn the_formulas_parsed_are_charged_by_their_nodes_with_the_next_items_taken() {
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        r.forms.cell("Width*0.5", 1.0, None);
        let nodes = diacraft_formula::parse("Width*0.5").node_count();
        assert!(r.take_items(3).is_ok());
        assert_eq!(r.report.work.items, 3 + ((2 * 9 + nodes * 128) as u64).div_ceil(64));
        assert_eq!(r.report.work.formula_nodes, nodes as u64);
        // Paid once, and a text already parsed costs nothing more.
        r.forms.cell("Width*0.5", 1.0, None);
        assert!(r.take_items(1).is_ok());
        assert_eq!(r.report.work.items, 3 + ((2 * 9 + nodes * 128) as u64).div_ceil(64) + 1);
        assert_eq!(r.report.work.formula_nodes, nodes as u64);
        // Text of whole items: nodes alone are 2 items each, and a byte more is one more item.
        let mut r = Reader::new(&limits);
        r.forms.cell("1+1", 1.0, None);
        assert!(r.take_items(0).is_ok());
        assert_eq!(r.report.work.items, (2 * 3 + 3 * 128_u64).div_ceil(64));
        assert_eq!(r.report.work.items, 7);
    }

    /// A document's budget of nodes is its items at 64 bytes over 128 bytes a node: a tree that fits is built, one past it is not, and the
    /// next charge refuses the document, and every one after.
    #[test]
    fn a_tree_past_the_budget_of_nodes_refuses_the_next_charge() {
        let limits = Limits { max_items: 2_000, ..Limits::default() };
        // A chain of `n` ones is 2n - 1 nodes.
        let chain = |n: usize| "1+".repeat(n - 1) + "1";
        let mut r = Reader::new(&limits);
        r.forms.cell(&chain(500), 1.0, None);
        assert!(!r.forms.over_budget(), "999 of 1,000 nodes are built");
        assert_eq!(r.forms.settle().1, 999);
        let mut r = Reader::new(&limits);
        r.forms.cell(&chain(501), 1.0, None);
        assert!(r.forms.over_budget(), "1,001 nodes are not");
        assert!(matches!(r.take_items(0), Err(crate::error::OdgError::Limit { kind: crate::error::LimitKind::Items, .. })));
        assert!(r.take_items(0).is_err(), "and the document stays refused");
    }
}
