//! The memory the engine holds beside the drawing an `.odg` save is given, **measured**, never priced by a ratio.
//!
//! The writer's gate (`diacraft_odg::write`, `Limits::max_write_memory`) counts what the save holds: the model it is given, the file read again beside it, the
//! package it makes and what the model has grown by since the read. It cannot see what the **engine** holds beside the model:
//!
//! * the **undo and redo steps**: earlier versions of the drawing, which share with it what no edit since has changed and hold what has (an edit of every shape
//!   of a drawing of 350,000 leaves a second whole drawing in the history);
//! * the **pages the last save was given** (the writer's index holds them, and the next save compares the model with them): the copy of the drawing with its new
//!   shapes flattened, so they hold the flattened shapes and, after an undo, shapes the model has since dropped;
//! * the **shapes the last save flattened**, with the model's shape each was made from (the next save reuses a copy of a shape that has not changed);
//! * the **copies this save makes**: a shape replaced in the copy of the drawing is still in the model.
//!
//! **What is counted.** Every allocation is reached from a version of the drawing through shared pointers, so what a version holds that nothing nearer holds is the
//! set of chunks of shape lists, shapes and pages it reaches that the drawing and the versions counted before it do not (the pointers are the identity: a chunk or a
//! shape two versions share is one allocation, counted once). The walk starts from the drawing, marking what it holds; then each other version is walked,
//! skipping at once any page, chunk or shape already marked, and **the members of a group are walked as the shapes of a page are** (a group that was edited is the
//! members it holds, not one shape: the first version of this counted a group as one shape, and a drawing of 175,000 members in groups edited four times aborted the
//! save). Each shape that is new to the walk is **priced by what it holds** ([`shape_bytes`]: its cells, rows, sections, text and the attributes it keeps), not by a
//! share of the model's cost over its shapes (which, after a delete of 3,990 of 4,000 shapes, priced each of the 4,000 the history holds at 4,000 times what it is).
//!
//! **What it costs.** One look at each shape, chunk and page of the drawing, then at each shape and chunk that is *new* to the walk, and at nothing that is shared:
//! at most the shapes the process holds, since each costs a real allocation, and the walk stops as soon as the sum passes the memory a save may hold.

use std::collections::{HashMap, HashSet};
use std::mem::size_of;
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellName, Connect, Document, Ext, ExtData, FormulaSlot, Kept, Master, OdfRow, OdfShape, Page, Row, Section, Shape, ShapeList, SharedVec,
    Sheet, Style, Text, TextPart, Theme, VsdxRest,
};

use crate::history::History;

/// The counts in front of what an `Arc` holds.
const ARC: u64 = 16;

/// The attributes and the nodes a [`Kept`] holds, as the walk counts them (the top level of each, as it counts `raw` and `passthrough`).
fn kept_nodes(kept: &Kept) -> u64 {
    kept.raw.iter().count() as u64 + kept.kept.iter().count() as u64
}

/// What an extension slot holds in memory: none for a slot that holds nothing, and for one that does the allocation behind its pointer and a node for
/// each attribute and element of each side, the VSDX elements and a newer build's content among them. `own` is the nodes the ODF side holds besides its kept markup.
fn slot_bytes<T: Default + Clone + PartialEq>(ext: &Ext<T>, odf_nodes: impl Fn(&T) -> u64) -> u64 {
    ext.get().map_or(0, |d: &ExtData<T>| {
        let vsdx = |rest: &VsdxRest| rest.0.iter().map(|(_, k)| 1 + kept_nodes(k)).fold(0u64, u64::saturating_add);
        let nodes = odf_nodes(&d.odf).saturating_add(vsdx(&d.vsdx)).saturating_add(kept_nodes(&d.native));
        (ARC + size_of::<ExtData<T>>() as u64).saturating_add(nodes.saturating_mul(NODE_BYTES))
    })
}

/// What a map of cells costs for each cell besides the cell itself: the node's share of the entry.
const MAP_ENTRY: u64 = 24;

/// What a parsed formula costs for each byte of its text: the tree is about 25 times the text (the reader's own measure).
const FORMULA_TREE: u64 = 25;

/// A chunk of a shape list: the allocation of up to [`diacraft_doc::CHUNK`] pointers, and its pointer in the list above it.
const CHUNK_BYTES: u64 = 16 + 8 * diacraft_doc::CHUNK as u64 + 8;

/// What a kept attribute or node costs besides its text: an allocation of its own.
const NODE_BYTES: u64 = 96;

/// What prices shapes: the formulas it has priced already, by the pointer each is shared by. A formula is behind an `Arc` and the reader gives every cell that has
/// the same text one (`cells::Forms`), so a drawing of 350,000 shapes whose cells hold `Width*0.5` holds that tree once; a formula nobody else shares is priced
/// in full, at the tree's cost (about 25 times its text, the reader's own measure), whatever the hostile text.
#[derive(Default)]
pub(crate) struct Sizer {
    formulas: HashSet<usize>,
    /// The sections whose formulas [`Sizer::share`] has marked, by pointer: a section the copies of a shape share is marked once.
    shared: HashSet<usize>,
    /// The sections the pricing of the versions an edit replaced ([`Sizer::sheet_beside`] with a next version) has priced, by pointer: a section the copies of a shape
    /// share is one allocation, priced once (ten thousand connectors that shared a route of 4,000 rows, routed again, priced it ten thousand times).
    priced: HashSet<usize>,
}

// The rows the pricing of this thread looked at (priced, or passed over to mark the formulas a new version holds still), for the tests that hold an edit's
// pricing to the rows it changed. Not kept outside tests.
#[cfg(test)]
thread_local! {
    pub(crate) static ROWS_LOOKED_AT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn note_rows(n: usize) {
    ROWS_LOOKED_AT.with(|c| c.set(c.get().saturating_add(n as u64)));
}

#[cfg(not(test))]
fn note_rows(_: usize) {}

impl Sizer {
    /// The bytes a cell holds: itself, its name's entry in its map, its text, and its formula's tree unless that tree is one already priced.
    fn cell(&mut self, name: &CellName, cell: &Cell) -> u64 {
        let formula = match &cell.formula {
            FormulaSlot::Local(f) if self.formulas.insert(Arc::as_ptr(f) as usize) => {
                (f.source.len() as u64).saturating_mul(FORMULA_TREE).saturating_add(NODE_BYTES)
            }
            _ => 0,
        };
        let value = cell.value.as_ref().map_or(0, |v| v.text().len() as u64);
        let unit = cell.unit.as_ref().map_or(0, |u| u.len() as u64);
        (size_of::<CellName>() as u64)
            .saturating_add(size_of::<Cell>() as u64)
            .saturating_add(MAP_ENTRY)
            .saturating_add(name.as_str().len() as u64)
            .saturating_add(formula)
            .saturating_add(value)
            .saturating_add(unit)
    }

    fn row(&mut self, row: &Row) -> u64 {
        let mut bytes = ARC + size_of::<Row>() as u64;
        for t in [&row.name, &row.local_name, &row.row_type] {
            bytes = bytes.saturating_add(t.as_ref().map_or(0, |t| t.len() as u64));
        }
        for (name, cell) in row.cells.iter() {
            bytes = bytes.saturating_add(self.cell(name, cell));
        }
        bytes = bytes.saturating_add(slot_bytes(&row.ext, |o: &OdfRow| kept_nodes(&o.kept) + u64::from(o.held.is_some())));
        bytes.saturating_add((row.raw.iter().count() as u64 + row.passthrough.iter().count() as u64).saturating_mul(NODE_BYTES))
    }

    fn section(&mut self, section: &Section) -> u64 {
        note_rows(section.rows.len());
        let mut bytes = ARC + size_of::<Section>() as u64 + section.name.as_str().len() as u64;
        for (name, cell) in section.cells.iter() {
            bytes = bytes.saturating_add(self.cell(name, cell));
        }
        for row in section.rows.iter() {
            bytes = bytes.saturating_add(self.row(row));
        }
        bytes = bytes.saturating_add(slot_bytes(&section.ext, kept_nodes));
        bytes.saturating_add((section.raw.iter().count() as u64 + section.passthrough.iter().count() as u64).saturating_mul(NODE_BYTES))
    }

    fn sheet(&mut self, sheet: &Sheet) -> u64 {
        self.sheet_beside(sheet, None)
    }

    /// [`Sizer::sheet`] of a version of a shape that the next one replaced, whose sheet is `kept`: less the sections `kept` holds behind the same pointer (the next version
    /// holds those, and dropping this one frees none of them), and less a section this pricing has priced already (the copies of a shape share their sections, and a section
    /// is one allocation however many hold it). A move of ten thousand connectors that shared a route of 4,000 rows priced the route for each of them, forty
    /// million rows for a step that holds their cells, and so charged the step thousands of times what it holds. With no `kept`, every section is priced, as [`Sizer::sheet`]
    /// prices a sheet.
    fn sheet_beside(&mut self, sheet: &Sheet, kept: Option<&Sheet>) -> u64 {
        let mut bytes = ARC + size_of::<Sheet>() as u64;
        for (name, cell) in sheet.cells.iter() {
            bytes = bytes.saturating_add(self.cell(name, cell));
        }
        let held: HashSet<usize> = kept.map(|k| k.sections.iter().map(|section| Arc::as_ptr(section) as usize).collect()).unwrap_or_default();
        for section in sheet.sections.iter() {
            let at = Arc::as_ptr(section) as usize;
            if kept.is_some() && (held.contains(&at) || !self.priced.insert(at)) {
                continue;
            }
            bytes = bytes.saturating_add(self.section(section));
        }
        bytes = bytes.saturating_add(slot_bytes(&sheet.ext, kept_nodes));
        bytes.saturating_add((sheet.raw.iter().count() as u64 + sheet.passthrough.iter().count() as u64).saturating_mul(NODE_BYTES))
    }

    /// What one shape holds in memory, **its own and not its members'**: the shape, its sheet with every cell, row and section, its text, its data and the
    /// attributes and nodes it keeps.
    pub(crate) fn shape(&mut self, shape: &Shape) -> u64 {
        self.shape_beside(shape, None)
    }

    /// [`Sizer::shape`], less the sections the sheet `kept` (the next version's) holds behind the same pointer ([`Sizer::sheet_beside`]).
    fn shape_beside(&mut self, shape: &Shape, kept: Option<&Sheet>) -> u64 {
        let mut bytes = ARC + size_of::<Shape>() as u64;
        for t in [&shape.name, &shape.name_u] {
            bytes = bytes.saturating_add(t.as_ref().map_or(0, |t| t.len() as u64));
        }
        for t in [&shape.data1, &shape.data2, &shape.data3] {
            bytes = bytes.saturating_add(t.as_ref().map_or(0, |t| t.len() as u64));
        }
        bytes = bytes.saturating_add(self.sheet_beside(&shape.sheet, kept));
        bytes = bytes.saturating_add(shape.text.as_ref().map_or(0, |t| text_bytes(t)));
        bytes = bytes.saturating_add(shape.foreign.as_ref().map_or(0, |_| NODE_BYTES));
        // The slot of the shape's `.odg` and VSDX data: the markup the `.odg` reader keeps for the shape is there and not in `raw`.
        bytes = bytes.saturating_add(slot_bytes(&shape.ext, |o: &OdfShape| kept_nodes(&o.kept) + o.held.len() as u64 + o.units.len() as u64));
        bytes.saturating_add((shape.raw.iter().count() as u64 + shape.passthrough.iter().count() as u64).saturating_mul(NODE_BYTES))
    }
}

/// What the text of a shape holds: **each part is a [`TextPart`] of its own** (`size_of::<TextPart>()` bytes in the vector, whatever the part says), and a text
/// edit clones the whole vector. A run of characters and a field also hold their text behind an `Arc`; a kept node is an allocation of its own. (A part was priced
/// at the bytes of its characters, one byte for a one-letter run: 100,000 such runs in a 9 KB file were 100,000 parts of 48 bytes and a history that said 48 MiB
/// held 2.7 GB.)
fn text_bytes(text: &Text) -> u64 {
    text.parts
        .iter()
        .map(|p| {
            (size_of::<TextPart>() as u64).saturating_add(match p {
                TextPart::Chars(c) => ARC + c.len() as u64,
                TextPart::Fld { text, .. } => ARC + text.len() as u64,
                TextPart::Raw(_) | TextPart::OdfRaw(_) | TextPart::NativeRaw(_) => NODE_BYTES,
                _ => 0,
            })
        })
        .fold(ARC, u64::saturating_add)
}

/// What the walk has marked: the pages, the chunks and the shapes it has met, by their pointers, and the lists of connects it has priced.
#[derive(Default)]
struct Seen {
    pages: HashSet<usize>,
    chunks: HashSet<usize>,
    shapes: HashSet<usize>,
    connects: Vec<diacraft_doc::Connects>,
}

/// The walk: what the versions hold that the ones before them do not.
struct Walk {
    seen: Seen,
    sizer: Sizer,
    /// The bytes found so far, and the most the walk goes on for (it stops, with the sum, past it).
    bytes: u64,
    stop: u64,
    /// The units the walk went through (shapes, chunks and pages looked at), for a test of what it costs.
    steps: u64,
}

impl Walk {
    fn over(&self) -> bool {
        self.bytes > self.stop
    }

    fn add(&mut self, bytes: u64) {
        self.bytes = self.bytes.saturating_add(bytes);
    }

    /// Walks a list of shapes and what is in the groups in it, marking each chunk and shape on the way. With `price` unset nothing is priced (the drawing's own
    /// shapes are marked, not counted).
    fn list(&mut self, top: &ShapeList, price: bool) {
        let mut stack: Vec<&ShapeList> = vec![top];
        while let Some(list) = stack.pop() {
            if self.over() {
                return;
            }
            for chunk in list.chunk_arcs() {
                self.steps = self.steps.saturating_add(1);
                if !self.seen.chunks.insert(Arc::as_ptr(chunk) as usize) {
                    continue;
                }
                if price {
                    self.add(CHUNK_BYTES);
                }
                for shape in chunk.iter() {
                    self.steps = self.steps.saturating_add(1);
                    if !self.seen.shapes.insert(Arc::as_ptr(shape) as usize) {
                        continue;
                    }
                    if price {
                        let bytes = self.sizer.shape(shape);
                        self.add(bytes);
                    }
                    if !shape.children.is_empty() {
                        stack.push(&shape.children);
                    }
                }
            }
        }
    }

    /// A page: its shapes and its connects, unless the page is one already walked.
    fn page(&mut self, page: &Arc<Page>, price: bool) {
        self.steps = self.steps.saturating_add(1);
        if !self.seen.pages.insert(Arc::as_ptr(page) as usize) {
            return;
        }
        if price {
            self.add(ARC + size_of::<Page>() as u64);
            if !page.connects.is_empty() && !self.seen.connects.iter().any(|c| c.shares_with(&page.connects)) {
                self.add((page.connects.len() as u64).saturating_mul(size_of::<Connect>() as u64 + NODE_BYTES));
                self.seen.connects.push(page.connects.clone());
            }
        } else if !page.connects.is_empty() {
            self.seen.connects.push(page.connects.clone());
        }
        self.list(&page.shapes, price);
    }

    fn document(&mut self, doc: &Document, price: bool) {
        for page in doc.pages.iter() {
            self.page(page, price);
            if self.over() {
                return;
            }
        }
    }
}

/// What the engine holds beside what the save is given: **`given` is the drawing the writer is handed** (the copy, with the shapes flattened, or the model itself
/// when no shape needed it), whose shapes the writer's gate already counts, so the walk marks them and counts none of them. What is then priced is what the engine
/// holds that `given` does not: the `model` when it is not the drawing given (the shapes the copy replaced are still in it), the undo and redo steps of `history`,
/// the `saved` pages the writer's index holds and the `kept` shapes (the model's shapes the last save flattened). The sum, or the sum so far when it is past `stop`
/// (the walk ends there: it is already over). `steps` gets what the walk went through.
pub(crate) fn held_beside(
    given: &Document,
    model: &Arc<Document>,
    history: Option<&History>,
    saved: &[Arc<Page>],
    kept: &[Arc<Shape>],
    stop: u64,
    steps: &mut u64,
) -> u64 {
    let mut walk = Walk { seen: Seen::default(), sizer: Sizer::default(), bytes: 0, stop, steps: 0 };
    walk.document(given, false);
    if !std::ptr::eq(given, &**model) {
        walk.document(model, true);
    }
    if let Some(history) = history {
        for step in history.undo_documents().chain(history.redo_documents()) {
            if walk.over() {
                break;
            }
            if Arc::ptr_eq(step, model) {
                continue;
            }
            walk.document(step, true);
        }
    }
    if !walk.over() {
        for page in saved {
            walk.page(page, true);
        }
    }
    for shape in kept {
        if walk.over() {
            break;
        }
        walk.steps = walk.steps.saturating_add(1);
        if walk.seen.shapes.insert(Arc::as_ptr(shape) as usize) {
            let bytes = walk.sizer.shape(shape);
            walk.add(bytes);
            if !shape.children.is_empty() {
                walk.list(&shape.children, true);
            }
        }
    }
    *steps = walk.steps;
    walk.bytes
}

// ---- what an edit leaves in the history -------------------------------------------------------------------------------------------------

impl Sizer {
    /// Marks the formulas of `sheet` as priced: they are shared with a shape that holds them still, so what holds the same trees besides is not charged for them.
    fn share(&mut self, sheet: &Sheet) {
        let mut mark = |cell: &Cell| {
            if let FormulaSlot::Local(f) = &cell.formula {
                self.formulas.insert(Arc::as_ptr(f) as usize);
            }
        };
        for cell in sheet.cells.values() {
            mark(cell);
        }
        for section in &sheet.sections {
            // A section met before was marked then: the copies of a shape share their sections, and are marked once.
            if !self.shared.insert(Arc::as_ptr(section) as usize) {
                continue;
            }
            note_rows(section.rows.len());
            for cell in section.cells.values() {
                mark(cell);
            }
            for row in section.rows.iter() {
                for cell in row.cells.values() {
                    mark(cell);
                }
            }
        }
    }
}

/// What a version of the drawing holds that the next version does not (the history's byte budget): the allocations the old version keeps alive
/// and the new one has let go of or replaced, priced as [`held_beside`] prices them (by what the shapes hold). A step of the history is charged this much:
/// the version it keeps is all that an edit of a few shapes of a drawing of thousands holds beside the drawing. The versions are compared as the edit made one
/// of the other: a page, a chunk of a list of shapes, a shape, behind the same pointer in both is shared and costs nothing, a shape that was replaced costs what it
/// holds (less the formulas the new one holds still), a shape that is gone costs what it and its members hold, and a list that changed length is paired by `ID`.
/// **What else is priced** (at the structural caps a page's sheet alone is 0.76 MB, which a `page.size` leaves behind in each step): the **page's
/// sheet** and the **document's sheet** when an edit replaced them (the cells they hold, and the sections the new sheet does not share), the **style list**, the
/// **masters**, the **themes** (their bytes), the colour and font tables, the title and the properties, each when a step replaced it. **Not priced:** the parts of the
/// package (macro projects, the thumbnail), the elements the model keeps as they were read, and the VSDX header: no command rewrites them.
/// **What it costs:** a look at the chunks of the lists the edit changed and at each shape in a chunk that is not shared, no more than the edit wrote; a list of
/// styles, masters or themes that was replaced is looked at whole once (65,536 pointers at the most).
pub(crate) fn unshared_bytes(old: &Document, new: &Document) -> u64 {
    let mut diff = Unshared { sizer: Sizer::default(), bytes: 0 };
    diff.document(old, new);
    for page in old.pages.iter() {
        match new.pages.iter().find(|p| p.id == page.id) {
            Some(same) if Arc::ptr_eq(page, same) => {}
            Some(other) => diff.page(page, other),
            None => {
                let mut walk = Walk { seen: Seen::default(), sizer: Sizer::default(), bytes: 0, stop: u64::MAX, steps: 0 };
                walk.page(page, true);
                diff.bytes = diff.bytes.saturating_add(walk.bytes);
            }
        }
    }
    diff.bytes
}

/// The walk of [`unshared_bytes`].
struct Unshared {
    sizer: Sizer,
    bytes: u64,
}

impl Unshared {
    fn add(&mut self, bytes: u64) {
        self.bytes = self.bytes.saturating_add(bytes);
    }

    /// What the document holds besides its pages that `new` does not: its sheet, styles, masters, themes, tables, title and properties, each when replaced.
    fn document(&mut self, old: &Document, new: &Document) {
        self.sheet(&old.sheet, &new.sheet);
        // The replaced items are paired with the new ones by `ID` (a theme by its part name), in one map made once for the list.
        let gone = self.replaced(&old.styles, &new.styles);
        if !gone.is_empty() {
            let by_id: HashMap<u32, &Arc<Style>> = new.styles.iter().map(|n| (n.id, n)).collect();
            for style in gone {
                self.style(style, by_id.get(&style.id).copied());
            }
        }
        let gone = self.replaced(&old.masters, &new.masters);
        if !gone.is_empty() {
            let by_id: HashMap<u32, &Arc<Master>> = new.masters.iter().map(|n| (n.id, n)).collect();
            for master in gone {
                self.master(master, by_id.get(&master.id).copied());
            }
        }
        let gone = self.replaced(&old.themes, &new.themes);
        if !gone.is_empty() {
            let by_part: HashMap<&str, &Arc<Theme>> = new.themes.iter().map(|n| (n.part.as_ref(), n)).collect();
            for theme in gone {
                self.theme(theme, by_part.get(theme.part.as_ref()).copied());
            }
        }
        if !Arc::ptr_eq(&old.colors, &new.colors) {
            let rows = old.colors.iter().fold(0_u64, |sum, c| sum.saturating_add(size_of::<diacraft_doc::ColorEntry>() as u64 + c.rgb.len() as u64));
            self.add(ARC.saturating_add(rows));
        }
        if !Arc::ptr_eq(&old.fonts, &new.fonts) {
            let rows = old
                .fonts
                .iter()
                .fold(0_u64, |sum, f| sum.saturating_add(size_of::<diacraft_doc::FaceName>() as u64 + f.name_u.len() as u64 + NODE_BYTES));
            self.add(ARC.saturating_add(rows));
        }
        if !Arc::ptr_eq(&old.title, &new.title) {
            self.add(ARC + old.title.len() as u64);
        }
        if !Arc::ptr_eq(&old.props, &new.props) {
            self.add(
                ARC + size_of::<diacraft_doc::DocProps>() as u64
                    + (old.props.custom.len() as u64).saturating_mul(size_of::<diacraft_doc::CustomProp>() as u64 + NODE_BYTES),
            );
        }
    }

    /// The items of `old` that `new` does not hold behind the same pointer, after charging the chunks of `old` that `new` does not share; nothing for two lists
    /// that are one list. The charge for each item is the caller's.
    fn replaced<'a, T>(&mut self, old: &'a SharedVec<Arc<T>>, new: &SharedVec<Arc<T>>) -> Vec<&'a Arc<T>> {
        if old.is_same_list_as(new) {
            return Vec::new();
        }
        let new_chunks = new.chunk_arcs();
        let kept: HashSet<usize> = new.iter().map(|item| Arc::as_ptr(item) as usize).collect();
        let (mut open, mut gone) = (0_u64, Vec::new());
        for (i, chunk) in old.chunk_arcs().iter().enumerate() {
            if new_chunks.get(i).is_some_and(|other| Arc::ptr_eq(chunk, other)) {
                continue;
            }
            open += 1;
            gone.extend(chunk.iter().filter(|item| !kept.contains(&(Arc::as_ptr(item) as usize))));
        }
        self.add(open.saturating_mul(CHUNK_BYTES));
        gone
    }

    /// A sheet `old` that `new` replaced: its own cells (a copy of the map is made at each edit), the vector of pointers to its sections, and the sections the new
    /// sheet does not hold behind the same pointer (the others are shared), less the formulas the new sheet holds still. A sheet behind one pointer in both costs nothing.
    fn sheet(&mut self, old: &Arc<Sheet>, new: &Arc<Sheet>) {
        if Arc::ptr_eq(old, new) {
            return;
        }
        self.sizer.share(new);
        let mut bytes = ARC + size_of::<Sheet>() as u64;
        for (name, cell) in old.cells.iter() {
            bytes = bytes.saturating_add(self.sizer.cell(name, cell));
        }
        bytes = bytes.saturating_add((old.sections.len() as u64).saturating_mul(size_of::<Arc<Section>>() as u64));
        let kept: HashSet<usize> = new.sections.iter().map(|section| Arc::as_ptr(section) as usize).collect();
        for section in old.sections.iter().filter(|section| !kept.contains(&(Arc::as_ptr(section) as usize))) {
            bytes = bytes.saturating_add(self.sizer.section(section));
        }
        bytes = bytes.saturating_add((old.raw.iter().count() as u64 + old.passthrough.iter().count() as u64).saturating_mul(NODE_BYTES));
        self.add(bytes);
    }

    /// A sheet nothing of the next version shares: all of it.
    fn whole_sheet(&mut self, sheet: &Arc<Sheet>) {
        let bytes = self.sizer.sheet(sheet);
        self.add(bytes);
    }

    /// A style `old` the next version replaced by `new` (found by `ID`), or took away.
    fn style(&mut self, old: &Style, new: Option<&Arc<Style>>) {
        let mut bytes = ARC + size_of::<Style>() as u64;
        for t in [&old.name, &old.name_u] {
            bytes = bytes.saturating_add(t.as_ref().map_or(0, |t| t.len() as u64));
        }
        self.add(bytes.saturating_add((old.raw.iter().count() as u64 + old.passthrough.iter().count() as u64).saturating_mul(NODE_BYTES)));
        match new {
            Some(new) => self.sheet(&old.sheet, &new.sheet),
            None => self.whole_sheet(&old.sheet),
        }
    }

    /// A master `old` the next version replaced by `new` (found by `ID`), or took away: its sheet, its shapes and its connects.
    fn master(&mut self, old: &Master, new: Option<&Arc<Master>>) {
        let mut bytes = ARC + size_of::<Master>() as u64;
        for t in [&old.name, &old.name_u, &old.unique_id, &old.rel_id, &old.icon, &old.part_name] {
            bytes = bytes.saturating_add(t.as_ref().map_or(0, |t| t.len() as u64));
        }
        self.add(bytes.saturating_add((old.raw.iter().count() as u64 + old.passthrough.iter().count() as u64).saturating_mul(NODE_BYTES)));
        let shared = new.is_some_and(|n| old.connects.shares_with(&n.connects));
        if !old.connects.is_empty() && !shared {
            self.add((old.connects.len() as u64).saturating_mul(size_of::<Connect>() as u64 + NODE_BYTES));
        }
        match new {
            Some(new) => {
                self.sheet(&old.sheet, &new.sheet);
                self.lists(&old.shapes, &new.shapes, 0);
            }
            None => {
                self.whole_sheet(&old.sheet);
                self.whole(&old.shapes);
            }
        }
    }

    /// A theme `old` the next version replaced by `new` (found by part name), or took away: its bytes unless the new one holds the same.
    fn theme(&mut self, old: &Theme, new: Option<&Arc<Theme>>) {
        let mut bytes = ARC + size_of::<Theme>() as u64 + old.part.len() as u64;
        for t in [&old.major_font, &old.minor_font] {
            bytes = bytes.saturating_add(t.as_ref().map_or(0, |t| t.len() as u64));
        }
        if !new.is_some_and(|n| Arc::ptr_eq(&n.bytes, &old.bytes)) {
            bytes = bytes.saturating_add(ARC + old.bytes.len() as u64);
        }
        self.add(bytes);
    }

    fn page(&mut self, old: &Page, new: &Page) {
        self.add(ARC + size_of::<Page>() as u64);
        self.sheet(&old.sheet, &new.sheet);
        if !old.connects.is_empty() && !old.connects.shares_with(&new.connects) {
            self.add((old.connects.len() as u64).saturating_mul(size_of::<Connect>() as u64 + NODE_BYTES));
        }
        self.lists(&old.shapes, &new.shapes, 0);
    }

    /// Everything in `list` and in the groups in it, which nothing else holds.
    fn whole(&mut self, list: &ShapeList) {
        let mut stack: Vec<&ShapeList> = vec![list];
        while let Some(list) = stack.pop() {
            for chunk in list.chunk_arcs() {
                self.add(CHUNK_BYTES);
                for shape in chunk.iter() {
                    let bytes = self.sizer.shape(shape);
                    self.add(bytes);
                    if !shape.children.is_empty() {
                        stack.push(&shape.children);
                    }
                }
            }
        }
    }

    /// A shape `old` that `new` replaced: what `old` holds of its own that `new` does not (the sections `new` holds behind the same pointer are its too), and what
    /// is in its members.
    fn pair(&mut self, old: &Shape, new: &Shape, depth: usize) {
        self.sizer.share(&new.sheet);
        let bytes = self.sizer.shape_beside(old, Some(&new.sheet));
        self.add(bytes);
        if old.children.is_empty() {
            return;
        }
        if new.children.is_empty() || depth >= diacraft_doc::MAX_SHAPE_DEPTH {
            self.whole(&old.children);
        } else {
            self.lists(&old.children, &new.children, depth + 1);
        }
    }

    fn lists(&mut self, old: &ShapeList, new: &ShapeList, depth: usize) {
        if old.is_same_list_as(new) {
            return;
        }
        let new_chunks = new.chunk_arcs();
        // The chunks of `old` that `new` does not hold at the same place are allocations `old` keeps; the shapes in the others are shared.
        let open: Vec<usize> =
            old.chunk_arcs().iter().enumerate().filter(|(i, a)| !new_chunks.get(*i).is_some_and(|b| Arc::ptr_eq(a, b))).map(|(i, _)| i).collect();
        self.add((open.len() as u64).saturating_mul(CHUNK_BYTES));
        let by_place = old.len() == new.len()
            && open.iter().all(|i| match (old.chunk_arcs().get(*i), new_chunks.get(*i)) {
                (Some(a), Some(b)) => a.len() == b.len() && a.iter().zip(b.iter()).all(|(o, n)| Arc::ptr_eq(o, n) || o.id == n.id),
                _ => false,
            });
        if by_place {
            for i in &open {
                let (Some(a), Some(b)) = (old.chunk_arcs().get(*i), new_chunks.get(*i)) else { continue };
                for (o, n) in a.iter().zip(b.iter()) {
                    if !Arc::ptr_eq(o, n) {
                        self.pair(o, n, depth);
                    }
                }
            }
            return;
        }
        let mut index: std::collections::HashMap<u32, usize> = std::collections::HashMap::with_capacity(new.len());
        let mut twice = false;
        for (k, shape) in new.iter().enumerate() {
            twice |= index.insert(shape.id, k).is_some();
        }
        for i in &open {
            let Some(chunk) = old.chunk_arcs().get(*i) else { continue };
            for o in chunk.iter() {
                // An `ID` twice pairs nothing for certain: what is priced is the whole of the old shape.
                match if twice { None } else { index.get(&o.id).and_then(|k| new.get(*k)) } {
                    Some(n) if Arc::ptr_eq(o, n) => {}
                    Some(n) => self.pair(o, n, depth),
                    None => {
                        let bytes = self.sizer.shape(o);
                        self.add(bytes);
                        if !o.children.is_empty() {
                            self.whole(&o.children);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn walked_steps(current: &Arc<Document>, history: Option<&History>, saved: &[Arc<Page>]) -> (u64, u64) {
    let mut steps = 0;
    let bytes = held_beside(current, current, history, saved, &[], u64::MAX, &mut steps);
    (bytes, steps)
}

#[cfg(test)]
mod unshared_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_doc::{CellAddr, CellEdit, CellWrite, Owner, SheetRef, Value, WriteOrigin};
    use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

    use super::*;

    /// A shape with a handful of cells, one a formula.
    fn shape(id: u32) -> ShapeBuilder {
        ShapeBuilder::new(id).cell_u("PinX", "1", "IN").cell_u("PinY", "2", "IN").cell_u("Width", "3", "IN").cell_u("Height", "1", "IN").formula(
            "LocPinX",
            "Width*0.5",
            "1.5",
        )
    }

    /// `after` with the parts of the document that are not pages (its sheet, styles, masters, themes, tables, title and properties) the very ones `before` holds,
    /// as two versions of one drawing hold them: documents the builder makes apart have each their own, which the charge would count.
    fn with_the_parts_of(before: &Document, mut after: Document) -> Document {
        after.sheet = Arc::clone(&before.sheet);
        after.styles = before.styles.clone();
        after.masters = before.masters.clone();
        after.themes = before.themes.clone();
        after.colors = Arc::clone(&before.colors);
        after.fonts = Arc::clone(&before.fonts);
        after.title = Arc::clone(&before.title);
        after.props = Arc::clone(&before.props);
        after
    }

    fn page_of(n: u32) -> Document {
        let mut page = PageBuilder::new();
        for id in 1..=n {
            page = page.shape(shape(id));
        }
        DocBuilder::new().page(1, page).build()
    }

    /// `doc` with `PinX` of each shape in `ids` written.
    fn written(doc: &Document, ids: &[u32], value: &str) -> Document {
        let mut after = doc.clone();
        for id in ids {
            let at = after.find_shape(Owner::Page(1), *id).unwrap();
            let write = CellWrite {
                addr: CellAddr::direct("PinX"),
                edit: CellEdit::Cell(diacraft_doc::Cell::constant(Value::from_text(value)).with_unit("IN")),
                origin: WriteOrigin::Drawing,
            };
            after.write_batch(vec![(SheetRef::Shape(at), write)]).unwrap();
        }
        after
    }

    /// What the version of shape `id` in `doc` holds by itself, priced as the walk prices it (its own formula trees included).
    fn own(doc: &Document, id: u32) -> u64 {
        let at = doc.find_shape(Owner::Page(1), id).unwrap();
        Sizer::default().shape(doc.shape(&at).unwrap())
    }

    /// The tree of the formula `Width*0.5`, which a shape's new version holds still when only a constant was written.
    fn formula_tree() -> u64 {
        ("Width*0.5".len() as u64) * FORMULA_TREE + NODE_BYTES
    }

    /// The page, and the chunk its shapes are in: what is held whenever a shape of the page was written.
    const PAGE_AND_CHUNK: u64 = ARC + size_of::<Page>() as u64 + CHUNK_BYTES;

    #[test]
    fn a_version_that_the_next_shares_everything_with_holds_nothing() {
        let doc = page_of(10);
        assert_eq!(unshared_bytes(&doc, &doc.clone()), 0);
        assert_eq!(unshared_bytes(&doc, &doc), 0);
    }

    #[test]
    fn an_edit_of_one_shape_leaves_the_chunk_and_the_old_shape_less_the_formulas_the_new_one_holds_still() {
        let before = page_of(10);
        let after = written(&before, &[4], "7");
        let held = unshared_bytes(&before, &after);
        // The old shape's cells are its own, and the tree of `Width*0.5` is held by the new shape's cell still, so it is not charged again.
        assert_eq!(held, PAGE_AND_CHUNK + own(&before, 4) - formula_tree(), "page and chunk, and the shape without the shared tree");
    }

    #[test]
    fn what_an_edit_rewrites_is_charged_a_shape_at_a_time_and_nothing_more() {
        let before = page_of(100);
        let ids: Vec<u32> = (1..=100).collect();
        let one = unshared_bytes(&before, &written(&before, &[1], "7"));
        let all = unshared_bytes(&before, &written(&before, &ids, "7"));
        let per_shape = own(&before, 1) - formula_tree();
        // Two chunks of 64 hold the hundred; every one of them and every shape is the old version's.
        assert_eq!(all, ARC + size_of::<Page>() as u64 + 2 * CHUNK_BYTES + 100 * per_shape);
        assert!(all > 50 * one && one > per_shape);
    }

    #[test]
    fn a_shape_taken_out_is_charged_with_all_it_holds_and_the_chunks_after_it_are_the_old_versions() {
        let before = page_of(70);
        let mut after = before.clone();
        let at = after.find_shape(Owner::Page(1), 3).unwrap();
        let page = Arc::make_mut(after.pages.get_mut(0).unwrap());
        page.shapes.remove(usize::try_from(at.path[0]).unwrap()).unwrap();
        // Both chunks are laid out again, the shapes in them are shared by `ID` but for the one that went.
        assert_eq!(unshared_bytes(&before, &after), ARC + size_of::<Page>() as u64 + 2 * CHUNK_BYTES + own(&before, 3));
    }

    #[test]
    fn a_shape_added_costs_the_old_version_the_chunk_and_no_shape() {
        let before = page_of(10);
        let mut after = before.clone();
        let page = Arc::make_mut(after.pages.get_mut(0).unwrap());
        let extra = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(99).cell_u("PinX", "1", "IN"))).build();
        page.shapes.push(Arc::clone(extra.page(1).unwrap().shapes.get(0).unwrap()));
        assert_eq!(unshared_bytes(&before, &after), PAGE_AND_CHUNK);
    }

    #[test]
    fn the_members_of_a_group_are_charged_one_by_one_and_the_group_for_its_own_cells() {
        let built = |x: &str| {
            let mut group = ShapeBuilder::new(50).group().cell_u("Width", "9", "IN");
            for id in 1..=5 {
                group = group.child(shape(id).cell_u("PinX", if id == 3 { x } else { "1" }, "IN"));
            }
            DocBuilder::new().page(1, PageBuilder::new().shape(group)).build()
        };
        let (before, after) = (built("1"), built("2"));
        // The two documents were built apart, so nothing is shared by pointer: the group, its chunk and every member are the old version's, less the formula trees
        // the new members hold with the same text (each tree is its own allocation here, so they are all charged).
        let total: u64 = [50, 1, 2, 3, 4, 5].iter().map(|id| own(&before, *id)).sum();
        let after = with_the_parts_of(&before, after);
        let sheet = Sizer::default().sheet(&before.page(1).unwrap().sheet);
        assert_eq!(unshared_bytes(&before, &after), PAGE_AND_CHUNK + CHUNK_BYTES + total + sheet);
    }

    #[test]
    fn a_group_that_lost_all_its_members_is_charged_for_each_of_them_whole() {
        let mut group = ShapeBuilder::new(50).group().cell_u("Width", "9", "IN");
        for id in 1..=5 {
            group = group.child(shape(id).cell_u("PinX", "1", "IN"));
        }
        let before = DocBuilder::new().page(1, PageBuilder::new().shape(group)).build();
        let mut after = before.clone();
        let page = Arc::make_mut(after.pages.get_mut(0).unwrap());
        Arc::make_mut(page.shapes.get_mut(0).unwrap()).children = diacraft_doc::ShapeList::new();
        // The group is the same shape (its cells are shared with its new version, which holds the same ones: priced as before), and its members are no part of the
        // new version at all: the chunk they stood in, and each of them.
        let total: u64 = [50, 1, 2, 3, 4, 5].iter().map(|id| own(&before, *id)).sum();
        assert_eq!(unshared_bytes(&before, &after), PAGE_AND_CHUNK + CHUNK_BYTES + total);
    }

    #[test]
    fn an_id_twice_pairs_nothing_for_certain_and_the_old_shapes_are_charged_whole() {
        let twice = |value: &str| {
            let page = PageBuilder::new().shape(shape(1).cell_u("PinX", value, "IN")).shape(shape(1).cell_u("PinX", value, "IN"));
            DocBuilder::new().page(1, page).build()
        };
        let (before, after) = (twice("1"), twice("2"));
        let both: u64 = before.pages.get(0).unwrap().shapes.iter().map(|s| Sizer::default().shape(s)).sum();
        // The two documents were built apart: the sheet of the page is the old version's too.
        let after = with_the_parts_of(&before, after);
        let sheet = Sizer::default().sheet(&before.page(1).unwrap().sheet);
        assert_eq!(unshared_bytes(&before, &after), ARC + size_of::<Page>() as u64 + CHUNK_BYTES + both + sheet);
    }

    /// A page of `n` shapes with five `Connect` rows, which the clones of the document share.
    fn page_with_connects(n: u32) -> Document {
        let mut doc = page_of(n);
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        page.connects = diacraft_doc::Connects::from_vec(vec![Connect::default(); 5]);
        doc
    }

    #[test]
    fn the_connects_of_a_page_are_charged_when_the_next_version_has_another_list_of_them_and_not_when_it_shares_it() {
        let before = page_with_connects(10);
        let shared = written(&before, &[4], "7");
        assert!(before.page(1).unwrap().connects.shares_with(&shared.page(1).unwrap().connects));
        let held = unshared_bytes(&before, &shared);
        assert_eq!(held, PAGE_AND_CHUNK + own(&before, 4) - formula_tree(), "the connects the next version shares cost nothing");
        let mut copied = shared.clone();
        let page = Arc::make_mut(copied.pages.get_mut(0).unwrap());
        page.connects = diacraft_doc::Connects::from_vec(before.page(1).unwrap().connects.iter().cloned().collect());
        assert_eq!(
            unshared_bytes(&before, &copied),
            held + 5 * (size_of::<Connect>() as u64 + NODE_BYTES),
            "a list of its own costs a row of it each"
        );
        // A page with no connects costs none of them, whatever the next version has.
        let none = page_of(10);
        assert_eq!(unshared_bytes(&none, &written(&none, &[4], "7")), PAGE_AND_CHUNK + own(&none, 4) - formula_tree());
    }

    #[test]
    fn a_group_that_lost_a_group_is_charged_for_the_members_of_the_one_it_lost_too() {
        let inner = ShapeBuilder::new(60).group().child(shape(1)).child(shape(2));
        let outer = ShapeBuilder::new(50).group().child(inner).child(shape(3));
        let before = DocBuilder::new().page(1, PageBuilder::new().shape(outer)).build();
        let mut after = before.clone();
        let page = Arc::make_mut(after.pages.get_mut(0).unwrap());
        Arc::make_mut(page.shapes.get_mut(0).unwrap()).children = diacraft_doc::ShapeList::new();
        // The two lists of members (the outer group's and the inner group's), and each shape in them.
        let total: u64 = [50, 60, 1, 2, 3].iter().map(|id| own(&before, *id)).sum();
        assert_eq!(unshared_bytes(&before, &after), PAGE_AND_CHUNK + 2 * CHUNK_BYTES + total);
    }

    #[test]
    fn a_group_taken_out_of_a_longer_list_is_charged_with_its_members_and_the_shapes_that_stay_are_shared() {
        let group = ShapeBuilder::new(50).group().child(shape(1)).child(shape(2));
        let before = DocBuilder::new().page(1, PageBuilder::new().shape(shape(10)).shape(group).shape(shape(11))).build();
        let mut after = before.clone();
        let page = Arc::make_mut(after.pages.get_mut(0).unwrap());
        page.shapes.remove(1).unwrap();
        let gone: u64 = [50, 1, 2].iter().map(|id| own(&before, *id)).sum();
        assert_eq!(
            unshared_bytes(&before, &after),
            PAGE_AND_CHUNK + CHUNK_BYTES + gone,
            "the chunk of the group's members, and each of them, besides the group"
        );
    }

    #[test]
    fn a_shape_replaced_in_a_list_that_changed_length_is_charged_and_the_ones_it_shares_are_not() {
        let before = page_of(3);
        let mut after = written(&before, &[2], "7");
        let page = Arc::make_mut(after.pages.get_mut(0).unwrap());
        let extra = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(99).cell_u("PinX", "1", "IN"))).build();
        page.shapes.push(Arc::clone(extra.page(1).unwrap().shapes.get(0).unwrap()));
        assert_eq!(
            unshared_bytes(&before, &after),
            PAGE_AND_CHUNK + own(&before, 2) - formula_tree(),
            "shape 2 only: 1 and 3 are the same allocations"
        );
    }

    #[test]
    fn an_id_twice_in_a_list_that_changed_length_charges_both_old_shapes_whole() {
        let twice = DocBuilder::new().page(1, PageBuilder::new().shape(shape(1)).shape(shape(1))).build();
        let mut after = twice.clone();
        let extra = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(99).cell_u("PinX", "1", "IN"))).build();
        Arc::make_mut(after.pages.get_mut(0).unwrap()).shapes.push(Arc::clone(extra.page(1).unwrap().shapes.get(0).unwrap()));
        let both: u64 = twice.pages.get(0).unwrap().shapes.iter().map(|s| Sizer::default().shape(s)).sum();
        // Both of them are the very allocations the next version holds, and an `ID` twice pairs nothing for certain: they are charged all the same.
        assert_eq!(unshared_bytes(&twice, &after), PAGE_AND_CHUNK + both);
    }

    #[test]
    fn two_lists_as_long_with_an_id_twice_are_paired_by_place_and_a_reordered_list_by_id() {
        let twice = DocBuilder::new().page(1, PageBuilder::new().shape(shape(1)).shape(shape(1))).build();
        let after = written(&twice, &[1], "7");
        // By place: the first is the one that was written (charged less the tree its new version holds still), and the second is the same allocation.
        let first = Sizer::default().shape(twice.pages.get(0).unwrap().shapes.get(0).unwrap());
        assert_eq!(unshared_bytes(&twice, &after), PAGE_AND_CHUNK + first - formula_tree());
        // The same two shapes the other way round: paired by `ID`, each is the very shape the next version holds, and only the chunk is the old version's.
        let before = page_of(2);
        let mut reordered = before.clone();
        let page = Arc::make_mut(reordered.pages.get_mut(0).unwrap());
        let (a, b) = (Arc::clone(page.shapes.get(0).unwrap()), Arc::clone(page.shapes.get(1).unwrap()));
        page.shapes = diacraft_doc::ShapeList::from_vec(vec![b, a]);
        assert_eq!(unshared_bytes(&before, &reordered), PAGE_AND_CHUNK);
    }

    #[test]
    fn an_edit_of_one_member_of_a_group_charges_that_member_and_the_group_and_not_the_other_members() {
        let mut group = ShapeBuilder::new(50).group().cell_u("Width", "9", "IN");
        for id in 1..=5 {
            group = group.child(shape(id));
        }
        let before = DocBuilder::new().page(1, PageBuilder::new().shape(group)).build();
        let after = written(&before, &[3], "7");
        // The group's copy and the chunk it is in, the chunk of its members, and the one member that was written (less the tree its new version holds still).
        assert_eq!(unshared_bytes(&before, &after), PAGE_AND_CHUNK + CHUNK_BYTES + own(&before, 50) + own(&before, 3) - formula_tree());
    }

    /// A page of `n` shapes that hold one `Geometry` section of `rows` rows between them: one allocation, as the copies of a shape hold it (a paste, a file's copies).
    fn copies_sharing_a_section(n: u32, rows: u32) -> (Document, Arc<Section>) {
        let mut doc = page_of(n);
        let mut section = Section::new("Geometry", Some(0));
        for k in 1..=rows {
            section.rows.push(Arc::new(Row { ix: Some(k), ..Row::default() }));
        }
        let section = Arc::new(section);
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        for at in 0..page.shapes.len() {
            let shape = page.shapes.get_mut(at).map(Arc::make_mut).unwrap();
            Arc::make_mut(&mut shape.sheet).sections.push(Arc::clone(&section));
        }
        (doc, section)
    }

    /// A cell written leaves a shape's sections shared with its next version, which holds them still: dropping the version it replaced frees none of
    /// them, and the step is not charged for them (a move of ten thousand connectors charged each its route of 4,000 rows). Its cells are charged as before.
    #[test]
    fn a_section_the_next_version_of_a_shape_holds_still_is_not_charged_to_the_version_it_replaced() {
        let (before, section) = copies_sharing_a_section(10, 100);
        let after = written(&before, &[4], "7");
        let rows = Sizer::default().section(&section);
        assert_eq!(unshared_bytes(&before, &after), PAGE_AND_CHUNK + own(&before, 4) - formula_tree() - rows, "the shape less its section");
    }

    /// A section that the versions an edit replaced share between them is one allocation, and is charged once, not once for each version (each copy given
    /// a section of its own in place of the shared one, as routing connectors again gives each a route of its own).
    #[test]
    fn a_section_the_replaced_versions_of_many_shapes_share_is_charged_once() {
        let (before, section) = copies_sharing_a_section(10, 100);
        let mut after = before.clone();
        let page = Arc::make_mut(after.pages.get_mut(0).unwrap());
        for at in 0..page.shapes.len() {
            let shape = page.shapes.get_mut(at).map(Arc::make_mut).unwrap();
            let sheet = Arc::make_mut(&mut shape.sheet);
            let own = Section::clone(sheet.sections.last().unwrap());
            *sheet.sections.last_mut().unwrap() = Arc::new(own);
        }
        let rows = Sizer::default().section(&section);
        let one = own(&before, 1) - formula_tree() - rows;
        ROWS_LOOKED_AT.with(|c| c.set(0));
        assert_eq!(unshared_bytes(&before, &after), PAGE_AND_CHUNK + 10 * one + rows, "each shape less the section, and the section once");
        // The section priced once, and the ten new ones looked at once each for the formulas they hold.
        assert_eq!(ROWS_LOOKED_AT.with(std::cell::Cell::get), 100 + 10 * 100);
    }

    /// The sections the next versions share are looked at once for the formulas they hold, however many versions hold them.
    #[test]
    fn the_formulas_of_a_section_many_next_versions_share_are_looked_at_once() {
        let (before, _) = copies_sharing_a_section(10, 100);
        let ids: Vec<u32> = (1..=10).collect();
        let after = written(&before, &ids, "7");
        ROWS_LOOKED_AT.with(|c| c.set(0));
        unshared_bytes(&before, &after);
        assert_eq!(ROWS_LOOKED_AT.with(std::cell::Cell::get), 100, "the shared section, once");
    }

    fn text_of(parts: Vec<TextPart>) -> Text {
        Text { parts, ..Text::default() }
    }

    #[test]
    fn a_text_is_priced_a_part_at_a_time_each_part_whole() {
        let part = size_of::<TextPart>() as u64;
        assert_eq!(text_bytes(&text_of(Vec::new())), ARC);
        assert_eq!(
            text_bytes(&text_of(vec![TextPart::Chars(Arc::from("abc"))])),
            ARC + part + ARC + 3,
            "a run: the part, and its text behind an Arc"
        );
        assert_eq!(text_bytes(&text_of(vec![TextPart::Fld { ix: 1, text: Arc::from("12") }])), ARC + part + ARC + 2, "a field: the same");
        assert_eq!(
            text_bytes(&text_of(vec![TextPart::Cp(0), TextPart::Pp(1), TextPart::Tp(2)])),
            ARC + 3 * part,
            "a marker is the part and nothing more"
        );
        let node = || diacraft_doc::RawNode::Text(Arc::from("kept"));
        assert_eq!(
            text_bytes(&text_of(vec![TextPart::Raw(node()), TextPart::OdfRaw(Box::new(node())), TextPart::NativeRaw(Box::new(node()))])),
            ARC + 3 * (part + NODE_BYTES),
            "a kept node is the part and an allocation of its own"
        );
        let adjacent: Vec<TextPart> = (0..1_000).map(|_| TextPart::Chars(Arc::from("a"))).collect();
        assert_eq!(
            text_bytes(&text_of(adjacent)),
            ARC + 1_000 * (part + ARC + 1),
            "a thousand one-letter runs are a thousand parts, not a thousand bytes"
        );
    }

    #[test]
    fn an_edit_of_a_text_of_a_hundred_thousand_parts_charges_the_vector_the_step_keeps() {
        let mut before = page_of(3);
        let page = Arc::make_mut(before.pages.get_mut(0).unwrap());
        let adjacent: Vec<TextPart> = (0..100_000).map(|_| TextPart::Chars(Arc::from("a"))).collect();
        Arc::make_mut(page.shapes.get_mut(1).unwrap()).text = Some(Arc::new(text_of(adjacent)));
        let mut after = before.clone();
        let page = Arc::make_mut(after.pages.get_mut(0).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(1).unwrap());
        let mut parts = shape.text.as_ref().unwrap().parts.clone();
        parts[0] = TextPart::Chars(Arc::from("b"));
        shape.text = Some(Arc::new(text_of(parts)));
        let held = unshared_bytes(&before, &after);
        let vector = 100_000 * size_of::<TextPart>() as u64;
        // The chunk and the page, and the old shape with its text: a vector of 100,000 parts and the run behind each (less the formula its new version holds).
        assert_eq!(held, PAGE_AND_CHUNK + own(&before, 2) - formula_tree());
        assert!(held > vector, "{held} bytes charged for a vector of {vector}");
    }

    /// `n` cells of constants called `User0`, `User1`, ... in a sheet.
    fn sheet_of(n: u32) -> Sheet {
        let mut sheet = Sheet::default();
        for k in 0..n {
            sheet.cells.insert(CellName::new(&format!("User{k}")), Cell::constant(Value::from_text("1")));
        }
        sheet
    }

    /// `sheet` with the cell `User0` written: a copy of the map, as an edit makes it.
    fn written_sheet(sheet: &Arc<Sheet>) -> Arc<Sheet> {
        let mut again = Arc::clone(sheet);
        Arc::make_mut(&mut again).cells.insert(CellName::new("User0"), Cell::constant(Value::from_text("2")));
        again
    }

    #[test]
    fn the_sheet_of_a_page_an_edit_replaced_is_charged_and_one_behind_the_same_pointer_is_not() {
        let mut before = page_of(10);
        Arc::make_mut(before.pages.get_mut(0).unwrap()).sheet = Arc::new(sheet_of(4_000));
        // A shape edited: the page is replaced, its sheet is the same allocation, and the sheet costs nothing.
        let shape_edit = written(&before, &[4], "7");
        assert_eq!(unshared_bytes(&before, &shape_edit), PAGE_AND_CHUNK + own(&before, 4) - formula_tree());
        // A page size written: the sheet's map of cells is copied, and the old copy is the step's, a cell of it at a time.
        let mut after = before.clone();
        let page = Arc::make_mut(after.pages.get_mut(0).unwrap());
        page.sheet = written_sheet(&page.sheet);
        let sheet = Sizer::default().sheet(&before.page(1).unwrap().sheet);
        assert_eq!(unshared_bytes(&before, &after), ARC + size_of::<Page>() as u64 + sheet);
        assert!(sheet >= 4_000 * size_of::<Cell>() as u64, "{sheet} bytes for 4,000 cells");
    }

    #[test]
    fn the_sheet_of_the_document_an_edit_replaced_is_charged_and_the_sections_the_new_sheet_shares_are_not() {
        let mut before = page_of(2);
        before.sheet = Arc::new(sheet_of(500));
        let after = Document { sheet: written_sheet(&before.sheet), ..before.clone() };
        assert_eq!(unshared_bytes(&before, &after), Sizer::default().sheet(&before.sheet));
        // A section both sheets hold behind the one pointer is the new sheet's too: the old sheet is charged its cells and its vector of pointers.
        let mut section = Section::new("Scratch", None);
        section.rows.push(Arc::new(Row { ix: Some(0), ..Row::default() }));
        let section = Arc::new(section);
        let mut with = sheet_of(10);
        with.sections.push(Arc::clone(&section));
        before.sheet = Arc::new(with);
        let after = Document { sheet: written_sheet(&before.sheet), ..before.clone() };
        let pointer = size_of::<Arc<Section>>() as u64;
        assert_eq!(unshared_bytes(&before, &after), Sizer::default().sheet(&before.sheet) - Sizer::default().section(&section) + pointer);
        // And one the new sheet does not hold is charged whole, with the pointer to it.
        let mut without = written_sheet(&before.sheet);
        Arc::make_mut(&mut without).sections.clear();
        let after = Document { sheet: without, ..before.clone() };
        assert_eq!(unshared_bytes(&before, &after), Sizer::default().sheet(&before.sheet) + pointer);
    }

    /// A style with `id` and a sheet of `cells` cells.
    fn style_of(id: u32, cells: u32) -> Arc<Style> {
        Arc::new(Style { id, sheet: Arc::new(sheet_of(cells)), ..Style::default() })
    }

    #[test]
    fn a_style_an_edit_replaced_or_took_away_is_charged_with_its_sheet_and_a_list_that_is_the_same_list_costs_nothing() {
        let mut before = page_of(2);
        for id in 1..=3 {
            before.styles.push(style_of(id, 100));
        }
        let own_style = ARC + size_of::<Style>() as u64;
        assert_eq!(unshared_bytes(&before, &before.clone()), 0);
        let mut after = before.clone();
        let at = after.styles.iter().position(|s| s.id == 2).unwrap();
        let style = Arc::make_mut(after.styles.get_mut(at).unwrap());
        style.sheet = written_sheet(&style.sheet);
        let old = Sizer::default().sheet(&before.styles.get(at).unwrap().sheet);
        assert_eq!(
            unshared_bytes(&before, &after),
            CHUNK_BYTES + own_style + old,
            "the chunk, the style and the old copy of its sheet; the other styles are shared"
        );
        // Taken away: the whole of it.
        let mut gone = before.clone();
        gone.styles.remove(at).unwrap();
        assert_eq!(unshared_bytes(&before, &gone), CHUNK_BYTES + own_style + old);
        // The same sheet in a style that was only renamed: the sheet is shared and costs nothing.
        let mut renamed = before.clone();
        Arc::make_mut(renamed.styles.get_mut(at).unwrap()).name = Some(Arc::from("Other"));
        assert_eq!(unshared_bytes(&before, &renamed), CHUNK_BYTES + own_style);
    }

    #[test]
    fn a_master_and_a_theme_an_edit_replaced_are_charged_and_the_bytes_of_a_theme_the_new_one_shares_are_not() {
        let mut before = page_of(2);
        before.masters.push(Arc::new(Master { id: 7, sheet: Arc::new(sheet_of(50)), ..Master::default() }));
        let mut after = before.clone();
        let master = Arc::make_mut(after.masters.get_mut(0).unwrap());
        master.sheet = written_sheet(&master.sheet);
        let sheet = Sizer::default().sheet(&before.masters.get(0).unwrap().sheet);
        assert_eq!(unshared_bytes(&before, &after), CHUNK_BYTES + ARC + size_of::<Master>() as u64 + sheet);
        // The connects of a master: not charged when the new one shares the list, charged when it holds another, and when the master is gone.
        let mut before = page_of(2);
        before.masters.push(Arc::new(Master { id: 8, connects: diacraft_doc::Connects::from_vec(vec![Connect::default(); 5]), ..Master::default() }));
        let own = CHUNK_BYTES + ARC + size_of::<Master>() as u64;
        let mut renamed = before.clone();
        Arc::make_mut(renamed.masters.get_mut(0).unwrap()).name = Some(Arc::from("Other"));
        assert_eq!(unshared_bytes(&before, &renamed), own, "the same list of connects: not charged");
        let mut other = before.clone();
        Arc::make_mut(other.masters.get_mut(0).unwrap()).connects = diacraft_doc::Connects::from_vec(vec![Connect::default(); 5]);
        let rows = 5 * (size_of::<Connect>() as u64 + NODE_BYTES);
        assert_eq!(unshared_bytes(&before, &other), own + rows, "another list: a row each");
        let mut gone = before.clone();
        gone.masters.remove(0).unwrap();
        assert_eq!(
            unshared_bytes(&before, &gone),
            own + Sizer::default().sheet(&before.masters.get(0).unwrap().sheet) + rows,
            "a master taken away: all of it"
        );
        let mut before = page_of(2);
        let bytes: Arc<[u8]> = Arc::from(vec![0_u8; 10_000]);
        before.themes.push(Arc::new(Theme { part: Arc::from("theme1.xml"), bytes: Arc::clone(&bytes), ..Theme::default() }));
        let own = ARC + size_of::<Theme>() as u64 + "theme1.xml".len() as u64;
        // The part replaced with the same bytes: the struct only. With other bytes: the old ones too.
        let mut same = before.clone();
        Arc::make_mut(same.themes.get_mut(0).unwrap()).major_font = None;
        assert_eq!(unshared_bytes(&before, &same), CHUNK_BYTES + own);
        let mut other = before.clone();
        Arc::make_mut(other.themes.get_mut(0).unwrap()).bytes = Arc::from(vec![1_u8; 10_000]);
        assert_eq!(unshared_bytes(&before, &other), CHUNK_BYTES + own + ARC + 10_000);
    }

    #[test]
    fn the_tables_the_title_and_the_properties_are_charged_when_replaced() {
        let mut before = page_of(2);
        before.colors = Arc::new((0..10).map(|ix| diacraft_doc::ColorEntry { ix, rgb: Arc::from("#000000") }).collect());
        before.title = Arc::from("a title");
        let mut after = before.clone();
        after.colors = Arc::new(Vec::new());
        assert_eq!(unshared_bytes(&before, &after), ARC + 10 * (size_of::<diacraft_doc::ColorEntry>() as u64 + 7));
        let face = |name: &str| diacraft_doc::FaceName { name_u: Arc::from(name), raw: diacraft_doc::RawAttrs::new(), ext: Default::default() };
        before.fonts = Arc::new(vec![face("Arial"), face("Calibri")]);
        let mut after = before.clone();
        after.fonts = Arc::new(Vec::new());
        assert_eq!(unshared_bytes(&before, &after), ARC + 2 * size_of::<diacraft_doc::FaceName>() as u64 + (5 + 7) + 2 * NODE_BYTES);
        let mut after = before.clone();
        after.title = Arc::from("another");
        assert_eq!(unshared_bytes(&before, &after), ARC + "a title".len() as u64);
        let mut after = before.clone();
        after.props = Arc::new(diacraft_doc::DocProps { subject: Some(Arc::from("s")), ..diacraft_doc::DocProps::default() });
        assert_eq!(unshared_bytes(&before, &after), ARC + size_of::<diacraft_doc::DocProps>() as u64);
    }

    #[test]
    fn a_page_the_next_version_lacks_is_charged_whole() {
        let before = page_of(5);
        let after = DocBuilder::new().page(2, PageBuilder::new()).build();
        let shapes: u64 = before.pages.get(0).unwrap().shapes.iter().map(|s| Sizer::default().shape(s)).sum();
        let after = with_the_parts_of(&before, after);
        assert_eq!(unshared_bytes(&before, &after), ARC + size_of::<Page>() as u64 + CHUNK_BYTES + shapes);
    }
}
