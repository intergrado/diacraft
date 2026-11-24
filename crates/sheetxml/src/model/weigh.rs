//! **What a model holds in memory, counted by one walk three ways**: the bytes of the allocations the documents of a session hold
//! between them, each allocation once.
//!
//! * **[`held_bytes_of`]** counts the documents it is given together, with one set of what has been counted: an allocation that more than one place holds (a page, a chunk of a
//!   list of shapes, a shape, a sheet, a section, a row, a text, a string) is counted once, however many documents and places hold it. A drawing and every undo step of it are
//!   one such count, and what they share, which is nearly all of it, is counted once.
//! * **[`grown`]** counts what an edit **added**: the allocations of the document it made that are not the very allocations the document it started from holds in the same
//!   places. It walks what the edit copied and not the drawing (the lists, the chunks, the pages, the shapes and the sheets that the two documents share are the same pointer
//!   in both, and a walk stops at the first), so that an edit of one shape of a million costs a chunk of them.
//! * **[`Released`]** counts what **dropping** documents frees: it lets go of each reference a dropped document holds, and an allocation is freed when the last holder lets go
//!   of it, which then lets go of what it holds. It touches what the dropped documents hold that the others do not, as [`grown`] touches what an edit copied, and it does not
//!   change the documents (a command's snapshot still holds them, and the count is of what the session would hold without it).
//!
//! The three charge each allocation **the same price**, so that a count kept by adding what [`grown`] says and taking away what [`Released`] says is the count [`held_bytes_of`]
//! gives, as long as every allocation the edits make is one [`grown`] meets: the price of an element is what the open charges it ([`element_bytes`]), a map of cells its nodes
//! and each cell its own, and the **characters** of the strings the model holds (the name, value, unit, error and formula of a cell, and the parts of a text) are charged once
//! for each string allocation, for a copy of a cell shares its strings. **What an object keeps without a field is counted too**: the list of its raw
//! attributes and the list of its kept nodes, each once however many copies of the object hold it, with the characters of their strings, and a connect at what an open charges its element.
//! It is a floor, as it always was: what a file's open charges besides (the elements' own attributes, the slots of the extension) is not counted, so that a drawing it refuses is one
//! the open refuses.
//!
//! It allocates only the addresses of what is shared or what a release has met, and it recurses on nothing: the shapes of a group are a stack of lists.

use std::cell::Cell as Counter;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{
    Cells, Connects, Document, FormulaSlot, Passthrough, RawAttrs, RawNode, Section, Shape, ShapeList, SharedVec, Sheet, Text, TextPart,
};

use super::cost::{ATTR_PAIR_BYTES, LIST_HEADER_BYTES, PLACED_BYTES, RAW_NODE_BYTES, cells_bytes, element_bytes, vec_bytes};
use super::vocab::el;

thread_local! {
    static WALKS: Counter<u64> = const { Counter::new(0) };
    static TOUCHED: Counter<u64> = const { Counter::new(0) };
}

/// How many allocations the calling thread's walks have looked at, of every kind ([`held_bytes_of`], [`grown`] and [`Released::release`]): the tests hold the work of an edit to a count of
/// this (what it changed, and not the drawing), as they hold it to [`whole_walks`] for the count made whole. A release that walks a model that shares nothing with the edit moves it by
/// the model.
#[must_use]
pub fn allocations_walked() -> u64 {
    TOUCHED.with(Counter::get)
}

/// The address of the allocation behind a pointer to it: what a count by address looks a thing up by.
trait Addr {
    fn addr(&self) -> usize;
}

impl<X: ?Sized> Addr for Arc<X> {
    fn addr(&self) -> usize {
        Arc::as_ptr(self).cast::<u8>().addr()
    }
}

/// How many times the calling thread has counted documents **whole** ([`held_bytes_of`]): the tests hold the work an edit does to a count of this, which a time does not
/// tell from the load of the machine. An edit that keeps the count it had ([`grown`], [`Released`]) walks what it changed and does not move this.
#[must_use]
pub fn whole_walks() -> u64 {
    WALKS.with(Counter::get)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Way {
    Held,
    Grown,
    Released,
}

/// The price of each kind of allocation, looked up once and not for each of the millions a walk may meet.
struct Prices {
    sheet: u64,
    section: u64,
    row: u64,
    cell: u64,
    shape: u64,
    text: u64,
    connect: u64,
    extra: u64,
    page: u64,
    master: u64,
    style: u64,
}

struct Weigh {
    way: Way,
    price: Prices,
    total: u64,
    /// How much of `total` is **characters** (the strings of the cells and the texts, and the names of the cells): the rest is structure, the elements and the nodes of the maps.
    chars: u64,
    most: u64,
    /// What has been counted (`Held`, `Grown`), by address: only an allocation that more than one place holds is put in, for one that has a single holder cannot be met twice.
    seen: HashSet<usize>,
    /// What a release has met, by address, with the references to it that are still held (`Released`).
    left: HashMap<usize, usize>,
    /// Whether an allocation that another place holds was counted as new (`Grown`): it may be one that was counted already (what a copy of a cell shares with a cell elsewhere, a
    /// sheet that a new shape shares with one that is held), so that the count is then a figure that may be more than the model holds.
    doubtful: bool,
    /// How many of the bytes counted were in what was **doubtful** (the outermost allocation that another place holds, with all that was counted under it): the most by which the
    /// count may be more than what the model holds.
    doubt_bytes: u64,
    /// How deep inside a doubtful allocation the walk is: its bytes are added once, at the outermost.
    doubt_open: u32,
    /// What the documents the edit started from hold **at the places it changed** (`Grown`): the addresses of the allocations of the chunks of a list that are not the chunks the new
    /// list has in the same place. A shape that an edit put in another place (a z-order change, a delete that shifted the others, a group that took some) is among them and is **not
    /// new**: the document the edit started from holds it, and nothing of it is walked.
    moved: HashSet<usize>,
}

fn len(s: &str) -> u64 {
    u64::try_from(s.len()).unwrap_or(u64::MAX)
}

impl Weigh {
    fn new(way: Way, most: u64) -> Weigh {
        Weigh {
            way,
            price: Prices {
                sheet: element_bytes(el::SHEET),
                section: element_bytes(el::SECTION),
                row: element_bytes(el::ROW),
                cell: element_bytes(el::CELL),
                shape: element_bytes(el::SHAPE),
                text: element_bytes(el::TEXT),
                connect: element_bytes(el::CONNECT),
                extra: element_bytes(el::EXTRA),
                page: element_bytes(el::PAGE),
                master: element_bytes(el::MASTER),
                style: element_bytes(el::STYLE),
            },
            total: 0,
            chars: 0,
            most,
            seen: HashSet::new(),
            left: HashMap::new(),
            doubtful: false,
            doubt_bytes: 0,
            doubt_open: 0,
            moved: HashSet::new(),
        }
    }

    fn add(&mut self, n: u64) {
        self.total = self.total.saturating_add(n);
    }

    /// Counts characters: bytes of a string that a copy of a place shares and that are held once, which an edit may make by the megabyte where it adds no element.
    fn add_chars(&mut self, n: u64) {
        self.add(n);
        self.chars = self.chars.saturating_add(n);
    }

    fn over(&self) -> bool {
        self.total > self.most
    }

    /// Whether `a` is to be counted, with what it holds: **new to the count**. `old` is the allocation in the same place of the document the edit started from (`Grown`), and is
    /// never counted again. A release (`Released`) lets go of one reference to `a`, and counts it when that was the last.
    fn take<T: ?Sized>(&mut self, a: &Arc<T>, old: Option<&Arc<T>>) -> bool {
        TOUCHED.with(|c| c.set(c.get().saturating_add(1)));
        let count = Arc::strong_count(a);
        let at = Arc::as_ptr(a).cast::<u8>().addr();
        match self.way {
            Way::Released => {
                if count <= 1 {
                    return true;
                }
                let rest = self.left.entry(at).or_insert(count);
                *rest = rest.saturating_sub(1);
                *rest == 0
            }
            Way::Held | Way::Grown => {
                if old.is_some_and(|o| Arc::ptr_eq(o, a)) {
                    return false;
                }
                // What the document the edit started from holds in a place the edit changed is not new, wherever the edit put it.
                if self.way == Way::Grown && self.moved.contains(&at) {
                    return false;
                }
                if count <= 1 {
                    return true;
                }
                let first = self.seen.insert(at);
                if first && self.way == Way::Grown {
                    self.doubtful = true;
                }
                first
            }
        }
    }

    /// Runs `f` as the walk of an allocation that another place holds (`shared`), and counts what it added as doubtful when it is the outermost.
    fn doubting(&mut self, shared: bool, f: impl FnOnce(&mut Weigh)) {
        if shared && self.way == Way::Grown && self.doubt_open == 0 {
            let before = self.total;
            self.doubt_open = 1;
            f(self);
            self.doubt_open = 0;
            self.doubt_bytes = self.doubt_bytes.saturating_add(self.total.saturating_sub(before));
        } else {
            f(self);
        }
    }

    /// The characters of a string allocation, once.
    fn string(&mut self, s: &Arc<str>, old: Option<&Arc<str>>) {
        if self.take(s, old) {
            self.add_chars(len(s));
            if self.way == Way::Grown && self.doubt_open == 0 && Arc::strong_count(s) > 1 {
                self.doubt_bytes = self.doubt_bytes.saturating_add(len(s));
            }
        }
    }

    /// The attributes an object keeps without a field (`RawAttrs`): the list, once, and the characters of each name and value. A copy of an object holds the same list, and an edit
    /// of one attribute copies it; the list is the unit, as a sheet is. (The save's floor counts what an open charges for them at the least: the room of the list and the characters.)
    fn raw(&mut self, raw: &RawAttrs, old: Option<&RawAttrs>) {
        let Some(list) = raw.shared_list() else { return };
        let old_list = old.and_then(RawAttrs::shared_list);
        if !self.take(list, old_list) {
            return;
        }
        self.add(vec_bytes(list.len(), ATTR_PAIR_BYTES).saturating_add(LIST_HEADER_BYTES));
        for (i, (name, value)) in list.iter().enumerate() {
            let was = old_list.and_then(|o| o.get(i));
            self.string(name, was.map(|p| &p.0));
            self.string(value, was.map(|p| &p.1));
        }
    }

    /// The nodes an object keeps without a field (`Passthrough`, kept content): the list once, and each node with what it holds, by a stack of its own (an element may hold elements,
    /// to the depth the codec keeps, and nothing here recurses on it).
    fn passthrough(&mut self, kept: &Passthrough, old: Option<&Passthrough>) {
        let Some(list) = kept.shared_list() else { return };
        if !self.take(list, old.and_then(Passthrough::shared_list)) {
            return;
        }
        self.add(vec_bytes(list.len(), PLACED_BYTES).saturating_add(LIST_HEADER_BYTES));
        let mut stack: Vec<&RawNode> = list.iter().map(|placed| &placed.node).collect();
        while let Some(node) = stack.pop() {
            if self.over() {
                return;
            }
            match node {
                RawNode::Text(s) | RawNode::Comment(s) => self.string(s, None),
                RawNode::Pi { target, data } => {
                    self.string(target, None);
                    self.string(data, None);
                }
                RawNode::Element(e) => {
                    self.string(&e.name, None);
                    self.raw(&e.attrs, None);
                    self.add(vec_bytes(e.children.len(), RAW_NODE_BYTES));
                    stack.extend(e.children.iter());
                }
            }
        }
    }

    /// The connects of a page or a master: the list once, a connect at what an open charges its element, and the characters of the cell names it holds and its attributes.
    fn connects(&mut self, connects: &Connects, old: Option<&Connects>) {
        let Some(list) = connects.shared_list() else { return };
        let old_list = old.and_then(Connects::shared_list);
        if !self.take(list, old_list) {
            return;
        }
        self.add(LIST_HEADER_BYTES);
        for (i, connect) in list.iter().enumerate() {
            let was = old_list.and_then(|o| o.get(i));
            self.add(self.price.connect);
            if let Some(cell) = &connect.from_cell {
                self.string(cell, was.and_then(|c| c.from_cell.as_ref()));
            }
            if let Some(cell) = &connect.to_cell {
                self.string(cell, was.and_then(|c| c.to_cell.as_ref()));
            }
            self.raw(&connect.raw, was.map(|c| &c.raw));
            if self.over() {
                return;
            }
        }
    }

    /// The cells of a map: its nodes, each cell's own bytes, and the characters of each name and of what it holds.
    fn cells(&mut self, cells: &Cells, old: Option<&Cells>) {
        self.add(cells_bytes(cells.len()));
        self.add(u64::try_from(cells.len()).unwrap_or(u64::MAX).saturating_mul(self.price.cell));
        for (name, cell) in cells {
            self.add_chars(len(name.as_str()));
            let same = old.and_then(|o| o.get(name));
            if let Some(v) = &cell.value {
                self.string(v.shared_text(), same.and_then(|o| o.value.as_ref()).map(diacraft_doc::Value::shared_text));
            }
            if let Some(unit) = &cell.unit {
                self.string(unit, same.and_then(|o| o.unit.as_ref()));
            }
            if let Some(error) = &cell.error {
                self.string(error, same.and_then(|o| o.error.as_ref()));
            }
            if let FormulaSlot::Local(f) = &cell.formula {
                let was = match same.map(|o| &o.formula) {
                    Some(FormulaSlot::Local(was)) => Some(was),
                    _ => None,
                };
                if self.take(f, was) {
                    self.add_chars(len(&f.source));
                }
            }
            self.raw(&cell.raw, same.map(|o| &o.raw));
            if let Some(extra) = &cell.extra {
                let was = same.and_then(|o| o.extra.as_ref());
                if self.take(extra, was) {
                    self.add(self.price.extra);
                    if let Some(text) = &extra.text {
                        self.string(text, was.and_then(|o| o.text.as_ref()));
                    }
                    self.passthrough(&extra.passthrough, was.map(|o| &o.passthrough));
                }
            }
            if self.over() {
                return;
            }
        }
    }

    fn sheet(&mut self, s: &Arc<Sheet>, old: Option<&Arc<Sheet>>) {
        let shared = Arc::strong_count(s) > 1;
        self.doubting(shared, |w| w.sheet_walk(s, old));
    }

    fn sheet_walk(&mut self, s: &Arc<Sheet>, old: Option<&Arc<Sheet>>) {
        if !self.take(s, old) {
            return;
        }
        self.add(self.price.sheet);
        self.cells(&s.cells, old.map(|o| &o.cells));
        self.raw(&s.raw, old.map(|o| &o.raw));
        self.passthrough(&s.passthrough, old.map(|o| &o.passthrough));
        for (i, section) in s.sections.iter().enumerate() {
            let old_section = old.and_then(|o| o.sections.get(i));
            // A section that another place holds (the group and the shapes of an `.odg` that share one) is the doubt of the count when it is met as new, as a sheet is.
            let shared = Arc::strong_count(section) > 1;
            self.doubting(shared, |w| w.section_walk(section, old_section));
            if self.over() {
                return;
            }
        }
    }

    fn section_walk(&mut self, section: &Arc<Section>, old_section: Option<&Arc<Section>>) {
        if !self.take(section, old_section) {
            return;
        }
        self.add(self.price.section);
        self.cells(&section.cells, old_section.map(|o| &o.cells));
        self.raw(&section.raw, old_section.map(|o| &o.raw));
        self.passthrough(&section.passthrough, old_section.map(|o| &o.passthrough));
        for (j, row) in section.rows.iter().enumerate() {
            let old_row = old_section.and_then(|o| o.rows.get(j));
            let shared = Arc::strong_count(row) > 1;
            self.doubting(shared, |w| {
                if w.take(row, old_row) {
                    w.add(w.price.row);
                    w.cells(&row.cells, old_row.map(|o| &o.cells));
                    w.raw(&row.raw, old_row.map(|o| &o.raw));
                    w.passthrough(&row.passthrough, old_row.map(|o| &o.passthrough));
                }
            });
            if self.over() {
                return;
            }
        }
    }

    fn text(&mut self, t: &Arc<Text>, old: Option<&Arc<Text>>) {
        let shared = Arc::strong_count(t) > 1;
        self.doubting(shared, |w| w.text_walk(t, old));
    }

    fn text_walk(&mut self, t: &Arc<Text>, old: Option<&Arc<Text>>) {
        if !self.take(t, old) {
            return;
        }
        self.add(self.price.text);
        for (i, part) in t.parts.iter().enumerate() {
            if let TextPart::Chars(s) | TextPart::Fld { text: s, .. } = part {
                let was = match old.and_then(|o| o.parts.get(i)) {
                    Some(TextPart::Chars(was) | TextPart::Fld { text: was, .. }) => Some(was),
                    _ => None,
                };
                self.string(s, was);
            }
        }
    }

    /// A list of shared chunks: the pointer a clone of the list shares, the chunks, then each element with the element in the same place of `old`. A list, a chunk or an
    /// element that is the one `old` has in the same place, or that was counted, is passed.
    fn list<'a, T, F>(&mut self, list: &'a SharedVec<T>, old: Option<&'a SharedVec<T>>, mut each: F)
    where
        T: Addr,
        F: FnMut(&mut Weigh, &'a T, Option<&'a T>),
    {
        let Some(top) = list.top_arc() else { return };
        let old_top = old.and_then(SharedVec::top_arc);
        if !self.take(top, old_top) {
            return;
        }
        if self.way == Way::Grown
            && let Some(old_top) = old_top
        {
            // What the list held in the places the edit changed (the chunks that are not the ones the new list has there, and those past its end): an allocation of it that the edit
            // put somewhere else is not new.
            for (ci, old_chunk) in old_top.iter().enumerate() {
                if top.get(ci).is_some_and(|c| Arc::ptr_eq(c, old_chunk)) {
                    continue;
                }
                for item in old_chunk.iter() {
                    TOUCHED.with(|c| c.set(c.get().saturating_add(1)));
                    self.moved.insert(item.addr());
                }
            }
        }
        for (ci, chunk) in top.iter().enumerate() {
            let old_chunk = old_top.and_then(|t| t.get(ci));
            if !self.take(chunk, old_chunk) {
                continue;
            }
            for (si, item) in chunk.iter().enumerate() {
                if self.over() {
                    return;
                }
                each(self, item, old_chunk.and_then(|c| c.get(si)));
            }
        }
    }

    /// One document, against the one `old` it was made from, if any.
    fn document<'a>(&mut self, doc: &'a Document, old: Option<&'a Document>) {
        self.sheet(&doc.sheet, old.map(|o| &o.sheet));
        self.list(&doc.styles, old.map(|o| &o.styles), |w, style, was| {
            if w.take(style, was) {
                w.add(w.price.style);
                w.sheet(&style.sheet, was.map(|s| &s.sheet));
                w.raw(&style.raw, was.map(|s| &s.raw));
                w.passthrough(&style.passthrough, was.map(|s| &s.passthrough));
            }
        });
        // The shapes of every page and master, and of every group in them: a stack of lists, so that nothing recurses on the depth of a drawing.
        let mut stack: Vec<(&'a ShapeList, Option<&'a ShapeList>)> = Vec::new();
        self.list(&doc.pages, old.map(|o| &o.pages), |w, page, was| {
            if w.take(page, was) {
                w.add(w.price.page);
                w.sheet(&page.sheet, was.map(|p| &p.sheet));
                w.raw(&page.raw, was.map(|p| &p.raw));
                w.passthrough(&page.passthrough, was.map(|p| &p.passthrough));
                w.connects(&page.connects, was.map(|p| &p.connects));
                stack.push((&page.shapes, was.map(|p| &p.shapes)));
            }
        });
        self.list(&doc.masters, old.map(|o| &o.masters), |w, master, was| {
            if w.take(master, was) {
                w.add(w.price.master);
                w.sheet(&master.sheet, was.map(|m| &m.sheet));
                w.raw(&master.raw, was.map(|m| &m.raw));
                w.passthrough(&master.passthrough, was.map(|m| &m.passthrough));
                w.connects(&master.connects, was.map(|m| &m.connects));
                stack.push((&master.shapes, was.map(|m| &m.shapes)));
            }
        });
        while let Some((shapes, was)) = stack.pop() {
            if self.over() {
                return;
            }
            self.list(shapes, was, |w, shape: &'a Arc<Shape>, was: Option<&'a Arc<Shape>>| {
                if !w.take(shape, was) {
                    return;
                }
                let shared = Arc::strong_count(shape) > 1;
                w.doubting(shared, |w| {
                    w.add(w.price.shape);
                    w.sheet(&shape.sheet, was.map(|s| &s.sheet));
                    if let Some(text) = &shape.text {
                        w.text(text, was.and_then(|s| s.text.as_ref()));
                    }
                    w.raw(&shape.raw, was.map(|s| &s.raw));
                    w.passthrough(&shape.passthrough, was.map(|s| &s.passthrough));
                });
                if !shape.children.is_empty() {
                    stack.push((&shape.children, was.map(|s| &s.children)));
                }
            });
        }
    }
}

/// The bytes the `docs` hold **between them**, each allocation once, and **stopped at the first count past `most`** (the count is then more than `most` and not the whole). The same
/// document given twice is counted once. This walks all of what is not shared with an earlier one: it is the count a ledger starts from and is made again from, and it is counted
/// in [`whole_walks`].
pub fn held_bytes_of<'a>(docs: impl IntoIterator<Item = &'a Document>, most: u64) -> u64 {
    WALKS.with(|c| c.set(c.get().saturating_add(1)));
    let mut w = Weigh::new(Way::Held, most);
    let mut done: HashSet<usize> = HashSet::new();
    for doc in docs {
        if !done.insert(std::ptr::from_ref(doc).addr()) {
            continue;
        }
        w.document(doc, None);
        if w.over() {
            break;
        }
    }
    w.total
}

/// What an edit added to the model: its bytes, and whether some of what it counted may have been counted already.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Added {
    /// The bytes of the allocations of `next` that are not the ones `before` holds in the same places, each once, stopped at the first count past the figure asked for.
    pub bytes: u64,
    /// How many of those bytes are characters (the strings of the cells and the texts, and the names of the cells); the rest are structure.
    pub chars: u64,
    /// Whether an allocation that another place holds was counted as new: a shape that moved to another place of its list, or a string that a cell shares with a cell elsewhere.
    /// The bytes are then a figure that may be more than the model holds, and a count kept from them is made again when it passes the figure it is held to.
    pub doubtful: bool,
    /// How many of the bytes were of what is doubtful: the most that the bytes are more than what the model holds more.
    pub doubt_bytes: u64,
}

/// What the edit that made `next` of `before` **added** to the model, at the least and as a figure that never counts a free ([`Added`]), stopped at the first count past
/// `most`. A walk of what the edit copied and not of the drawing (see the [module](self)).
pub fn grown(before: &Document, next: &Document, most: u64) -> Added {
    let mut w = Weigh::new(Way::Grown, most);
    w.document(next, Some(before));
    Added { bytes: w.total, chars: w.chars, doubtful: w.doubtful, doubt_bytes: w.doubt_bytes }
}

/// What letting go of a document freed: its bytes, and how many of them are characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Freed {
    pub bytes: u64,
    pub chars: u64,
}

/// What dropping documents frees: [`Released::release`] lets go of the references one document holds, and says the bytes that this freed on top of what the ones before it did.
/// An allocation that another document, or another holder, still holds is not freed. **The documents are not changed**, so a count made here is of what the session holds once
/// they are dropped, and a plan of which to drop is made before any is.
pub struct Released {
    w: Weigh,
}

impl Released {
    /// A release that has let go of nothing.
    pub fn new() -> Released {
        Released { w: Weigh::new(Way::Released, u64::MAX) }
    }

    /// Lets go of what `doc` holds, and gives what this freed: the bytes, and how many are characters. A document is to be let go of once.
    pub fn release(&mut self, doc: &Document) -> Freed {
        let (before, chars) = (self.w.total, self.w.chars);
        self.w.document(doc, None);
        Freed { bytes: self.w.total.saturating_sub(before), chars: self.w.chars.saturating_sub(chars) }
    }
}

impl Default for Released {
    fn default() -> Self {
        Released::new()
    }
}
