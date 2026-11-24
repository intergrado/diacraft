//! Reading sheets: the `Cell`, `Section`, `Row` and `Trigger` elements that every `PageSheet`, `Shape`, `StyleSheet` and `DocumentSheet` holds
//! And a cell from the attributes alone.
//!
//! A cell is read as the file gave it: `F`, `V`, `U` and `E` as the strings they are (`FormulaSlot::read` parses `F` against the
//! document's budget and keeps its text whether or not it parses), any other attribute in `raw`, and the text and children inside it in
//! `extra`. A cell directly under a sheet, under a section, or in a row is told apart by where it is read.
//!
//! **Keys.** A section is `Section N`, and only `Geometry` has an `IX`: an absent one is the number of `Geometry` sections
//! before it, and `IX` on any other section is kept as an attribute and is no key. A row is keyed by `N` in a section keyed by name and
//! by `IX` in one keyed by index (`doc::keying`); an absent `IX` in a section keyed by index, or keyed either way, is the number of
//! rows before it. Nothing is renumbered: a section whose first row is 2 holds rows 2 and on.
//!
//! **What a hostile sheet does.** A cell with no `N` or an empty one is kept as an unknown element and warned of; a name over
//! `MAX_NAME_BYTES`, more cells in one place, more rows in a section or more sections in a sheet than the model allows is refused before
//! the item is stored. A second cell of one name in one place is kept as an unknown element (the first is the one found), and a
//! repeated row key or section key keeps both, in file order, with a warning: a lookup finds the first, as everywhere in the model.
//!
//! **What a read costs.** The budgets of the document (cells, items, formula nodes) are in the [`SheetReader`], charged before what they pay
//! for is stored, and the tallies the report wants (the most cells in one place, the most rows in a section) are kept beside them. The
//! reader is one value for a whole document: every sheet of every part is read through it, so that the budgets are summed over the document.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{
    Cell, CellExtra, CellName, Cells, FormulaSlot, INHERIT_MARKER, Keying, MAX_CELLS_PER_CONTAINER, MAX_NAME_BYTES, MAX_ROWS_PER_SECTION,
    MAX_SECTIONS_PER_SHEET, NO_FORMULA_MARKER, Passthrough, RawAttrs, RawNode, Row, RowKey, Section, Sheet, Value, keying,
};

use crate::diag::Diagnostics;
use crate::limits::{Caps, LimitError, LimitKind};
use crate::model::BUCKET_MAX;
use crate::model::byvalue;
use crate::model::cost::push_bytes;
use crate::ns::{Scope, Vocabulary};
use crate::passthrough::{canonical_u32, element_cost, is_blank, keep, raw_element, raw_node, read_flag, read_u32};
use crate::price::{self, FORMULA_BYTES, FORMULA_NODE_BYTES, POINTER_BYTES, ROW_BYTES, SECTION_BYTES};

/// Most distinct strings the interner keeps. A file has a few dozen units and error words; a hostile one could have a million different `U`.
const INTERNED_MOST: usize = 4096;

/// The longest word, in bytes, that the table of repeated words takes: a unit, an error value, a type is a few bytes, and a long string is not one that repeats.
const INTERNED_LONGEST: usize = 32;

/// What reading a sheet has met of its sections so far: the numbers a later section needs from the earlier ones, kept as counters so that a
/// sheet of 8,192 sections is not 8,192 scans of the sections before it (a sheet of 1,024, with 7,500 shapes like it, took 7 seconds).
#[derive(Default)]
pub struct SheetTally {
    /// The bytes charged for what the sheet's children built and the sheet still holds (their cells, and their sections' places in it, net of what was given back for a section
    /// or a cell that was shared): what a sheet that turns out to be a copy of another gives back ([`SheetReader::share_sheet`]).
    bytes: u64,
    /// `Geometry` sections so far.
    geometry: usize,
    /// The `IX` of the last `Geometry` section.
    last_geometry_ix: Option<u32>,
    /// The names of the sections other than `Geometry` so far.
    names: HashSet<String>,
}

impl SheetTally {
    fn note(&mut self, section: &Section) {
        if section.name == "Geometry" {
            self.geometry += 1;
            self.last_geometry_ix = section.ix;
        } else {
            self.names.insert(section.name.as_str().to_owned());
        }
    }
}

/// Where a reading is and what it says: the part (a name for messages, never text from the file) and the [`Diagnostics`] its warnings go to.
pub struct Cx<'a, D: Diagnostics> {
    /// Where the warnings go.
    pub notes: &'a mut D,
    /// The part being read, for messages. It is escaped where a message holds it.
    pub part: &'a str,
}

impl<'a, D: Diagnostics> Cx<'a, D> {
    /// A context for reading `part`, with warnings to `notes`.
    pub fn new(notes: &'a mut D, part: &'a str) -> Cx<'a, D> {
        Cx { notes, part }
    }
}

/// What a sheet element's child turned out to be.
pub enum Took {
    /// A cell or a section, now in the sheet: it counts among the element's typed children.
    Typed,
    /// Kept as an unknown element, so it does not count: a `Trigger`, or a cell or section that could not be put in the sheet.
    Kept,
    /// Not a child of a sheet: the caller's own element.
    No,
}

/// The three style links of a sheet element, and the attributes that go to `raw`. `LineStyle`, `FillStyle` and `TextStyle` are typed
/// when they are the text of their number; an empty link is no link (`None`) and is kept as the attribute it was, with any other text.
/// Whether `name` is one of the three, and was read as one.
pub fn style_link(sheet: &mut Sheet, name: &str, value: &str) -> bool {
    let (id, needs_raw) = read_u32(value);
    let slot = match name {
        "LineStyle" => &mut sheet.links.line,
        "FillStyle" => &mut sheet.links.fill,
        "TextStyle" => &mut sheet.links.text,
        _ => return false,
    };
    *slot = id;
    if needs_raw {
        sheet.raw.push(name, value);
    }
    true
}

/// A section's name when it is one of the 17 predefined, `<other section>` for any other: the name of a section a
/// file defines is content.
fn section_word(name: &str) -> &'static str {
    diacraft_doc::SECTION_NAMES.iter().find(|n| **n == name).copied().unwrap_or("<other section>")
}

/// What reading the sheets of a document came to, for a report: the budgets spent and the largest figures met.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SheetStats {
    /// Cells read (a cell kept as an unknown element is counted, one that was refused is not).
    pub cells: usize,
    /// XML items taken from the document's budget.
    pub items: usize,
    /// Nodes of the formulas parsed.
    pub formula_nodes: usize,
    /// Cells that hold a formula of their own.
    pub formula_cells: usize,
    /// The most cells found directly under one sheet, section or row.
    pub max_cells_in_one_place: usize,
    /// The most sections found in one sheet.
    pub max_sections_in_a_sheet: usize,
    /// The most rows found in one section.
    pub max_rows_in_a_section: usize,
    /// Bytes of memory charged to the byte budget and still held (`SheetReader::with_bytes`; counted when there is no budget too).
    pub bytes: u64,
    /// The most bytes the byte budget was charged at any moment: what a sample-file run compares with the budget.
    pub bytes_peak: u64,
}

/// The work ahead of a read that has bytes of the budget set aside for the memory it holds ([`SheetReader::set_aside`]).
pub trait LookaheadHook: Send + Sync {
    /// **Ends the work and lets go of everything it holds**, and returns when that is done: the threads have finished what they were doing, and every tree they made and the read has not taken
    /// is dropped. Called before the bytes set aside go back to the budget, so that the budget is never paid out for memory that is still held.
    fn drain(&self);

    /// What is set aside changed to `set_aside` bytes: for a test that holds the memory the work holds to it.
    fn changed(&self, _set_aside: u64) {}
}

/// The budgets of one document's reading, and the tallies beside them. One value is made for a document and every sheet of it is read through
/// it. See the module documentation.
pub struct SheetReader {
    caps: Caps,
    /// Formula nodes the document may still parse.
    formula_budget: usize,
    formula_nodes_used: usize,
    cells_left: usize,
    cells_read: usize,
    items_left: usize,
    /// The bytes of memory the document may still hold (`u64::MAX` when no byte budget was asked for), what is charged now, and the most that was.
    bytes_left: u64,
    bytes_spent: u64,
    bytes_peak: u64,
    /// The bytes of the budget set aside for work done ahead of the read ([`SheetReader::set_aside`]): not in `bytes_left`, and not charged, so that no figure of the report moves.
    lookahead: u64,
    /// A charge took what was set aside back, since [`SheetReader::lookahead_taken_back`] was last asked.
    lookahead_taken: bool,
    /// The work ahead of the read that what is set aside stands for, which is told to finish and let go of its memory before the set-aside goes back to the budget.
    hook: Option<Arc<dyn LookaheadHook>>,
    formula_cells: usize,
    max_cells_in_one_place: usize,
    max_sections_in_a_sheet: usize,
    max_rows_in_a_section: usize,
    /// The section of a sheet being read, for a refusal to say where it was.
    current_section: &'static str,
    /// Small strings that repeat in every file (`U`, `E`), kept once each: at most [`INTERNED_MOST`] of them.
    interned: HashMap<String, Arc<str>>,
    /// The sections and sheets read so far, when the reader shares what is equal ([`SheetReader::with_sharing`]).
    shared: Option<Shared>,
}

/// The units a sharing reader has met, by the key of each: at most [`BUCKET_MAX`] different units under one key.
///
/// **The table keeps its entries out of the model's way** (B2): every entry of it is in one of a few arrays that grow by doubling, and no entry is an
/// allocation of its own. A list of units under a key, and the pointers of a sheet's sections, made as small allocations between the shapes' own, left holes between the model's
/// objects when the table was dropped at the end of the read, and a drawing of distinct shapes read with the table was edited half again as slowly as the same read without it
/// (the model was the same size, and the work of the calculation was the same: the objects were farther apart).
struct Shared {
    sections: Units<Arc<Section>>,
    sheets: Units<SharedSheet>,
    /// What the key of a unit is held to: all of it, unless a test makes every key agree ([`SheetReader::with_colliding_keys`]).
    key_mask: u64,
    /// The addresses of the sections of every sheet in the table, one after the other: a [`SharedSheet`] names its own.
    pointers: Vec<usize>,
    /// The addresses of the sections of the sheet in hand, to be compared with the table's and, if it is entered, taken into `pointers`.
    scratch: Vec<usize>,
}

impl Default for Shared {
    fn default() -> Shared {
        Shared { sections: Units::default(), sheets: Units::default(), key_mask: u64::MAX, pointers: Vec::new(), scratch: Vec::new() }
    }
}

/// A sheet a sharing reader has met, and where in [`Shared`]'s `pointers` the sections it holds are, each by the address of its allocation.
struct SharedSheet {
    sheet: Arc<Sheet>,
    start: usize,
    len: usize,
}

/// The units of one kind a sharing reader has met: the head of the list of the units under a key, and the units themselves in one array, each with the place of the next
/// unit under its key. A list is at most [`BUCKET_MAX`] long (a longer one is never made).
struct Units<T> {
    heads: HashMap<u64, u32>,
    entries: Vec<(u32, T)>,
}

impl<T> Default for Units<T> {
    fn default() -> Units<T> {
        Units { heads: HashMap::new(), entries: Vec::new() }
    }
}

/// The end of a list of units.
const NO_UNIT: u32 = u32::MAX;

impl<T> Units<T> {
    /// The units under `key`, the latest entered first.
    fn under(&self, key: u64) -> impl Iterator<Item = &T> {
        let mut at = self.heads.get(&key).copied().unwrap_or(NO_UNIT);
        std::iter::from_fn(move || {
            let (next, unit) = self.entries.get(usize::try_from(at).ok()?)?;
            at = *next;
            Some(unit)
        })
    }

    /// Whether `key` has room for one more unit.
    fn has_room(&self, key: u64) -> bool {
        self.under(key).take(BUCKET_MAX).count() < BUCKET_MAX && u32::try_from(self.entries.len()).is_ok_and(|n| n < NO_UNIT)
    }

    /// Enters `unit` under `key`, in front of the units that are there.
    fn enter(&mut self, key: u64, unit: T) {
        let Ok(at) = u32::try_from(self.entries.len()) else { return };
        if at == NO_UNIT {
            return;
        }
        let next = self.heads.insert(key, at).unwrap_or(NO_UNIT);
        self.entries.push((next, unit));
    }
}

/// What an entry of the table of a sharing reader costs the model: the key, the list of units under it and the pointer to the unit.
pub const SHARING_ENTRY_BYTES: u64 = 96;

impl SheetReader {
    /// A reader with the whole of the budgets in `caps` left.
    #[must_use]
    pub fn new(caps: Caps) -> SheetReader {
        SheetReader {
            caps,
            formula_budget: caps.formula_nodes,
            formula_nodes_used: 0,
            cells_left: caps.cells,
            cells_read: 0,
            items_left: caps.items,
            bytes_left: u64::MAX,
            bytes_spent: 0,
            bytes_peak: 0,
            lookahead: 0,
            lookahead_taken: false,
            hook: None,
            formula_cells: 0,
            max_cells_in_one_place: 0,
            max_sections_in_a_sheet: 0,
            max_rows_in_a_section: 0,
            current_section: "",
            interned: HashMap::new(),
            shared: None,
        }
    }

    /// This reader, **sharing what is equal**: a section that is equal (by `==`, never by a hash alone) to one it has read is that one's allocation, and a sheet that is equal to one it has
    /// read ([`SheetReader::share_sheet`]) is too. What the copies were charged is given back, so that what the reader has charged is what the model holds. A drawing of a hundred thousand
    /// shapes that hold one path holds one path. A reader that does not ask builds every unit. The table of what was met costs bytes of its own, taken from the budget only while it has
    /// them: a file that fits is read.
    #[must_use]
    pub fn with_sharing(mut self) -> SheetReader {
        self.shared = Some(Shared::default());
        self
    }

    /// This reader, **sharing, with every key the same**: what finds the candidates for a unit finds all of them, so that only the comparison (`==`) and the room of a key
    /// ([`BUCKET_MAX`] units) decide what is shared. It is for the tests that say a key never proves two units one.
    #[doc(hidden)]
    #[must_use]
    pub fn with_colliding_keys(mut self) -> SheetReader {
        self.shared = Some(Shared { key_mask: 0, ..Shared::default() });
        self
    }

    /// This reader, with `bytes` of memory the document may hold, charged by the real size of what is built ([`crate::price`]) before it is built. Without it
    /// the byte budget is none: the prices are still summed, for the report.
    #[must_use]
    pub fn with_bytes(mut self, bytes: u64) -> SheetReader {
        self.bytes_left = bytes;
        self
    }

    /// Takes the price `n` of something the model holds from the byte budget, with the margin the prices are given ([`price::MODEL_MARGIN_EIGHTHS`] eighths of
    /// them over): called before what it pays for is allocated. Nothing is taken when the budget cannot pay for all of it.
    pub fn take_bytes(&mut self, n: u64) -> Result<(), LimitError> {
        self.take_exact(price::with_margin(n))
    }

    /// Takes `n` bytes from the byte budget as they are: for what is counted from the thing itself (the tree of a part, a picture's bytes) and needs no margin.
    pub fn take_exact(&mut self, n: u64) -> Result<(), LimitError> {
        match self.bytes_left.checked_sub(n) {
            Some(left) => {
                // A reader with no byte budget has all of `u64::MAX`, and keeps it: what it is charged is counted, and nothing is ever refused.
                if self.bytes_left != u64::MAX {
                    self.bytes_left = left;
                }
                self.bytes_spent = self.bytes_spent.saturating_add(n);
                self.bytes_peak = self.bytes_peak.max(self.bytes_spent);
                Ok(())
            }
            // What was set aside for work ahead of the read is the read's to take back: a charge that the budget would pay without it is paid, and the work ahead is told.
            None if self.lookahead > 0 => {
                self.settle_lookahead();
                self.take_exact(n)
            }
            // The cap, as a person is told it: what the budget was, and not what is left.
            None => Err(LimitError { kind: LimitKind::Bytes, max: self.bytes_left.saturating_add(self.bytes_spent) }),
        }
    }

    /// Gives back `n` bytes that [`SheetReader::take_bytes`] took for something that is gone (the tree of a part, once the part is read). More than was
    /// taken is not given.
    pub fn give_bytes(&mut self, n: u64) {
        let n = n.min(self.bytes_spent);
        self.bytes_spent -= n;
        if self.bytes_left != u64::MAX {
            self.bytes_left = self.bytes_left.saturating_add(n);
        }
    }

    /// Bytes the document may still hold beside what is set aside for work ahead of the read (`u64::MAX` when there is no byte budget): **what the budget can pay now**, with the memory of the work
    /// ahead counted as held. The figure a read that does no work ahead has is [`SheetReader::bytes_left_settled`].
    #[must_use]
    pub fn bytes_left(&self) -> u64 {
        self.bytes_left
    }

    /// Bytes the document may still hold **once the work ahead of the read is over and what is set aside is back**: the figure a read that does no work ahead has, for deciding what that read
    /// would decide. **It is not memory that is free while the work is not over**: a read that allocates by it asks [`SheetReader::settle_lookahead`] first.
    #[must_use]
    pub fn bytes_left_settled(&self) -> u64 {
        if self.bytes_left == u64::MAX { self.bytes_left } else { self.bytes_left.saturating_add(self.lookahead) }
    }

    /// Installs the work that what is set aside stands for ([`LookaheadHook`]).
    pub fn set_lookahead_hook(&mut self, hook: Arc<dyn LookaheadHook>) {
        self.hook = Some(hook);
    }

    /// Sets `n` bytes of the byte budget aside for the memory that work done ahead of the read holds while it is held (the trees of parts parsed on other threads before the read's
    /// turn for them). **They are not charged** (no count of the report moves), and the budget does not pay them out: **a charge that the rest of the budget cannot pay ends the work ahead and
    /// waits for it to let go of its memory** ([`SheetReader::settle_lookahead`]), and only then takes them back, so that work ahead never makes a refusal that a read that does none would
    /// not make (the refusal, when there is one, is the same charge's, with the same words) and the budget is never paid out for memory that is still held. `false`, and nothing set aside,
    /// when the budget has not `n` beyond what is set aside already; always `true` with no byte budget.
    pub fn set_aside(&mut self, n: u64) -> bool {
        if self.bytes_left == u64::MAX {
            return true;
        }
        match self.bytes_left.checked_sub(n) {
            Some(left) => {
                self.bytes_left = left;
                self.lookahead = self.lookahead.saturating_add(n);
                self.changed();
                true
            }
            None => false,
        }
    }

    /// Gives `n` of the bytes set aside back to the budget (the work ahead was taken, or let go of): never more than is set aside. **The caller says so only for memory that is gone, or that it has
    /// charged in its place.**
    pub fn set_aside_back(&mut self, n: u64) {
        let n = n.min(self.lookahead);
        self.lookahead -= n;
        self.bytes_left = self.bytes_left.saturating_add(n);
        self.changed();
    }

    /// [`SheetReader::give_bytes`] for memory that is not free yet: the figures of the report are what they are when it is, and the bytes **are set aside until the caller gives them back**
    /// ([`SheetReader::set_aside_back`]) when the memory has been let go of (a tree that another thread is dropping).
    pub fn give_bytes_later(&mut self, n: u64) {
        let n = n.min(self.bytes_spent);
        self.bytes_spent -= n;
        if self.bytes_left != u64::MAX {
            self.lookahead = self.lookahead.saturating_add(n);
            self.changed();
        }
    }

    /// The bytes set aside now.
    #[must_use]
    pub fn lookahead(&self) -> u64 {
        self.lookahead
    }

    /// **Ends the work ahead of the read and takes what is set aside back**: the work lets go of everything it holds first ([`LookaheadHook::drain`]), so the budget gets the bytes back only when
    /// the memory they stand for is gone. A charge that the rest of the budget cannot pay does this; so does a read that is to allocate by what the budget has left and has the work ahead holding some of it.
    pub fn settle_lookahead(&mut self) {
        if self.lookahead == 0 {
            return;
        }
        if let Some(hook) = &self.hook {
            hook.drain();
        }
        self.bytes_left = self.bytes_left.saturating_add(self.lookahead);
        self.lookahead = 0;
        self.lookahead_taken = true;
        self.changed();
    }

    fn changed(&self) {
        if let Some(hook) = &self.hook {
            hook.changed(self.lookahead);
        }
    }

    /// Whether a charge or a call to [`SheetReader::settle_lookahead`] took what was set aside back since this was last asked: the work ahead is over.
    pub fn lookahead_taken_back(&mut self) -> bool {
        std::mem::take(&mut self.lookahead_taken)
    }

    // --------------------------------------------------------------------------------------------------------------- budgets

    /// Takes one cell from the document's total. Called before the cell is stored.
    pub fn take_cell(&mut self) -> Result<(), LimitError> {
        if self.cells_left == 0 {
            return Err(LimitError { kind: LimitKind::Cells, max: self.caps.cells as u64 });
        }
        self.cells_left -= 1;
        self.cells_read += 1;
        Ok(())
    }

    /// Takes `n` items from the document's total: called before what they pay for is stored. A file past the total is refused whole.
    pub fn take_items(&mut self, n: usize) -> Result<(), LimitError> {
        match self.items_left.checked_sub(n) {
            Some(left) => {
                self.items_left = left;
                Ok(())
            }
            None => Err(LimitError { kind: LimitKind::Items, max: self.caps.items as u64 }),
        }
    }

    /// Charges the cell budget again for `n` cells already read once: a part that two rows name is read once and paid for by each of them.
    /// Nothing is taken when the budget cannot pay for all `n`.
    pub fn charge_cells_again(&mut self, n: usize) -> Result<(), LimitError> {
        self.cells_left = self.cells_left.checked_sub(n).ok_or(LimitError { kind: LimitKind::Cells, max: self.caps.cells as u64 })?;
        self.cells_read = self.cells_read.saturating_add(n);
        Ok(())
    }

    /// Cells the document may still hold.
    #[must_use]
    pub fn cells_left(&self) -> usize {
        self.cells_left
    }

    /// Items the document may still keep.
    #[must_use]
    pub fn items_left(&self) -> usize {
        self.items_left
    }

    /// What was spent and met so far.
    #[must_use]
    pub fn stats(&self) -> SheetStats {
        SheetStats {
            cells: self.cells_read,
            items: self.caps.items.saturating_sub(self.items_left),
            formula_nodes: self.formula_nodes_used,
            formula_cells: self.formula_cells,
            max_cells_in_one_place: self.max_cells_in_one_place,
            max_sections_in_a_sheet: self.max_sections_in_a_sheet,
            max_rows_in_a_section: self.max_rows_in_a_section,
            bytes: self.bytes_spent,
            bytes_peak: self.bytes_peak,
        }
    }

    /// The section of a sheet being read: one of the 17 predefined sections' names, `<other section>`, or empty for a cell directly under a
    /// sheet or no sheet at all. After a refusal, the section it was in.
    #[must_use]
    pub fn current_section(&self) -> &'static str {
        self.current_section
    }

    /// `text` as an `Arc<str>`, shared with every other equal short string read so far.
    fn intern(&mut self, text: &str) -> Arc<str> {
        if let Some(found) = self.interned.get(text) {
            return Arc::clone(found);
        }
        let made: Arc<str> = Arc::from(text);
        if self.has_room_for(text) {
            self.interned.insert(text.to_owned(), Arc::clone(&made));
        }
        made
    }

    /// Whether the table of repeated words takes `text` as a new entry: it is not full ([`INTERNED_MOST`]) and the word is short ([`INTERNED_LONGEST`] bytes at the most).
    /// The one place that says so, for the table and for the price of the table, which must agree on every word.
    fn has_room_for(&self, text: &str) -> bool {
        self.interned.len() < INTERNED_MOST && text.len() <= INTERNED_LONGEST
    }

    /// What `text` costs as a string that repeats in every file (`U`, `E`, `T`): nothing when it is held already, else its own and its entry in the table.
    fn intern_bytes(&self, text: &str) -> u64 {
        if self.interned.contains_key(text) {
            return 0;
        }
        let entry = if self.has_room_for(text) { price::INTERN_ENTRY_BYTES } else { 0 };
        price::arc_str(text.len()).saturating_add(entry)
    }

    // ------------------------------------------------------------------------------------------------------------ passthrough

    /// Keeps an element and everything in it, whole, as an unknown element of a passthrough, after `typed` typed children. Its items are
    /// taken from the total first.
    pub fn pass_element(&mut self, into: &mut Passthrough, typed: usize, el: &El) -> Result<(), LimitError> {
        self.take_items(element_cost(el))?;
        self.take_bytes(price::kept_element_bytes(into.len(), el))?;
        keep(into, typed, RawNode::Element(raw_element(el)));
        Ok(())
    }

    /// Keeps a node that is not an element (text, a comment, an instruction) as an unknown node, unless it is blank text between elements,
    /// which is layout and is not kept.
    pub fn pass_other(&mut self, into: &mut Passthrough, typed: usize, node: &Node) -> Result<(), LimitError> {
        if matches!(node, Node::Text(t) | Node::CData(t) if is_blank(t)) {
            return Ok(());
        }
        self.take_items(1)?;
        self.take_bytes(price::kept_other_bytes(into.len(), node))?;
        keep(into, typed, raw_node(node));
        Ok(())
    }

    // ----------------------------------------------------------------------------------------------------------------- sheets

    /// Reads a `PageSheet` or a `DocumentSheet`: an element that is only a sheet. Its attributes are the links, and every other one in `raw`
    /// (`UniqueID`, and a document sheet's `Name`, `NameU`, `IsCustomName`, `IsCustomNameU`); its unknown children are in the sheet's own
    /// passthrough.
    ///
    /// This is also how a sheet is read from elements under a namespace: `sc` is the scope inside `el`, entered with a [`Vocabulary`] that
    /// reads that namespace, and the children of `el` in it are the cells and sections of the sheet.
    pub fn read_sheet_only<V: Vocabulary, D: Diagnostics>(&mut self, cx: &mut Cx<'_, D>, el: &El, sc: &Scope<V>) -> Result<Sheet, LimitError> {
        self.take_items(1 + el.attrs.len())?;
        // Every attribute is priced as one kept: the links that are typed cost nothing, and a sheet has a few attributes.
        let mut bytes = 0u64;
        for (kept, (k, v)) in el.attrs.iter().enumerate() {
            bytes = bytes.saturating_add(price::raw_attr_bytes(kept, k.len(), v.len()));
        }
        self.take_bytes(bytes)?;
        let mut sheet = Sheet::default();
        for (k, v) in &el.attrs {
            if !style_link(&mut sheet, k, v) {
                sheet.raw.push(k, v);
            }
        }
        let mut typed = 0usize;
        let mut tally = SheetTally::default();
        for node in &el.children {
            match node {
                Node::El(child) => {
                    let csc = sc.enter(child);
                    if csc.is_ours(child) {
                        match self.sheet_child(cx, &mut sheet, &mut tally, child, &csc, typed)? {
                            Took::Typed => {
                                typed += 1;
                                continue;
                            }
                            Took::Kept => continue,
                            Took::No => {}
                        }
                    }
                    self.pass_element(&mut sheet.passthrough, typed, child)?;
                }
                other => self.pass_other(&mut sheet.passthrough, typed, other)?,
            }
        }
        self.note_sheet(&sheet);
        Ok(sheet)
    }

    /// The allocation a section just read is to be: the first equal one this reader has met, when it shares (`==` decides; a key only finds the candidates), and then what the
    /// section was charged is given back, but for its place in its sheet's list; or itself. `spent` is what the reader had charged before it began to read the section, and
    /// `before` the sections the sheet held before it.
    fn share_section(&mut self, section: Section, spent: u64, before: usize) -> Arc<Section> {
        let Some(shared) = self.shared.as_mut() else { return Arc::new(section) };
        let key = byvalue::section_key(&section) & shared.key_mask;
        let found = shared.sections.under(key).find(|s| ***s == section).map(Arc::clone);
        if let Some(found) = found {
            let cost = self.bytes_spent.saturating_sub(spent);
            // Its place in its sheet's list stays charged: that list is the sheet's.
            let slot = price::with_margin(push_bytes(before, POINTER_BYTES));
            self.give_bytes(cost.saturating_sub(slot));
            return found;
        }
        let arc = Arc::new(section);
        self.enter_section(key, &arc);
        arc
    }

    /// Enters a section that was not met, in the table of the units, when the table has room and the budget has the bytes of its entry (the table never refuses a read).
    fn enter_section(&mut self, key: u64, section: &Arc<Section>) {
        let room = self.shared.as_ref().is_some_and(|s| s.sections.has_room(key));
        if room
            && self.take_bytes(SHARING_ENTRY_BYTES).is_ok()
            && let Some(shared) = self.shared.as_mut()
        {
            shared.sections.enter(key, Arc::clone(section));
        }
    }

    /// The allocation a finished sheet is to be: the first equal one this reader has met, when it shares, and then what the sheet's children were charged is given back (`tally`
    /// holds it: the cells it holds and the places of its sections); or the sheet itself, which is then the one the later equal sheets are. A sheet is equal to another when
    /// its links, cells, attributes, kept nodes and slot are, and each of its sections is the very allocation the other holds in the same place (sections that are shared are
    /// compared by pointer, and a section that was not shared with its twin makes the sheets two).
    pub fn share_sheet(&mut self, sheet: Sheet, tally: &SheetTally) -> Arc<Sheet> {
        let Some(shared) = self.shared.as_mut() else { return Arc::new(sheet) };
        shared.scratch.clear();
        shared.scratch.extend(sheet.sections.iter().map(|s| Arc::as_ptr(s).addr()));
        let key = byvalue::sheet_key(&sheet, &shared.scratch) & shared.key_mask;
        let found = shared
            .sheets
            .under(key)
            .find(|other| {
                let theirs = other.start.checked_add(other.len).and_then(|end| shared.pointers.get(other.start..end)).unwrap_or(&[]);
                byvalue::equal_sheets(&other.sheet, theirs, &sheet, &shared.scratch)
            })
            .map(|other| Arc::clone(&other.sheet));
        if let Some(found) = found {
            self.give_bytes(tally.bytes);
            return found;
        }
        let arc = Arc::new(sheet);
        let room = shared.sheets.has_room(key);
        if room
            && self.take_bytes(SHARING_ENTRY_BYTES).is_ok()
            && let Some(shared) = self.shared.as_mut()
        {
            let (start, len) = (shared.pointers.len(), shared.scratch.len());
            shared.pointers.extend_from_slice(&shared.scratch);
            shared.sheets.enter(key, SharedSheet { sheet: Arc::clone(&arc), start, len });
        }
        arc
    }

    /// Takes the sizes of a finished sheet for the report.
    pub fn note_sheet(&mut self, sheet: &Sheet) {
        self.max_cells_in_one_place = self.max_cells_in_one_place.max(sheet.cells.len());
        self.max_sections_in_a_sheet = self.max_sections_in_a_sheet.max(sheet.sections.len());
    }

    /// Reads a child of a sheet element if it is a `Cell`, a `Section` or a `Trigger`, putting it in `sheet`. `typed` is the number of
    /// typed children of the element so far, for where a kept node goes. The caller has checked that `el` is in a namespace it reads.
    pub fn sheet_child<V: Vocabulary, D: Diagnostics>(
        &mut self,
        cx: &mut Cx<'_, D>,
        sheet: &mut Sheet,
        tally: &mut SheetTally,
        el: &El,
        sc: &Scope<V>,
        typed: usize,
    ) -> Result<Took, LimitError> {
        let spent = self.bytes_spent;
        let took = self.sheet_child_inner(cx, sheet, tally, el, sc, typed)?;
        // What the child left charged, net of what was given back for a section that was shared: the sheet's own, which a copy of the sheet gives back.
        tally.bytes = tally.bytes.saturating_add(self.bytes_spent.saturating_sub(spent));
        Ok(took)
    }

    fn sheet_child_inner<V: Vocabulary, D: Diagnostics>(
        &mut self,
        cx: &mut Cx<'_, D>,
        sheet: &mut Sheet,
        tally: &mut SheetTally,
        el: &El,
        sc: &Scope<V>,
        typed: usize,
    ) -> Result<Took, LimitError> {
        match el.local() {
            "Cell" => {
                let (cells, passthrough) = (&mut sheet.cells, &mut sheet.passthrough);
                Ok(if self.put_cell(cx, cells, passthrough, typed, el)? { Took::Typed } else { Took::Kept })
            }
            "Trigger" => {
                self.pass_element(&mut sheet.passthrough, typed, el)?;
                Ok(Took::Kept)
            }
            "Section" => {
                if sheet.sections.len() >= MAX_SECTIONS_PER_SHEET {
                    return Err(LimitError { kind: LimitKind::SectionsInASheet, max: MAX_SECTIONS_PER_SHEET as u64 });
                }
                let (spent, before) = (self.bytes_spent, sheet.sections.len());
                match self.read_section(cx, el, sc, tally, before)? {
                    Some(section) => {
                        tally.note(&section);
                        let section = self.share_section(section, spent, before);
                        sheet.sections.push(section);
                        Ok(Took::Typed)
                    }
                    None => {
                        self.pass_element(&mut sheet.passthrough, typed, el)?;
                        Ok(Took::Kept)
                    }
                }
            }
            _ => Ok(Took::No),
        }
    }

    // ------------------------------------------------------------------------------------------------------------------ cells

    /// The name a cell called `name` is stored under in `cells`, or `None` when it may not be stored. A cell that cannot be (no name, or a
    /// name already taken) is warned of and charged to the cell budget, and it is `Ok(None)`: the caller keeps what it was read from. A name
    /// over the model's cap, and a place with no room left, are the error. `Ok(Some)` charges nothing yet.
    fn vet_cell<D: Diagnostics>(&mut self, cx: &mut Cx<'_, D>, cells: &Cells, name: Option<&str>) -> Result<Option<CellName>, LimitError> {
        let part = cx.part;
        let Some(name) = name.filter(|n| !n.is_empty()) else {
            cx.notes.warn("cell_without_name", || format!("a cell in {} has no name and is kept as it is", part.escape_debug()));
            self.take_cell()?;
            return Ok(None);
        };
        if name.len() > MAX_NAME_BYTES {
            return Err(LimitError { kind: LimitKind::NameBytes, max: MAX_NAME_BYTES as u64 });
        }
        let key = CellName::new(name);
        if cells.contains_key(&key) {
            cx.notes.warn("duplicate_cell", || format!("a cell is named twice in one place in {}; the first is the one read", part.escape_debug()));
            self.take_cell()?;
            return Ok(None);
        }
        if cells.len() >= MAX_CELLS_PER_CONTAINER {
            return Err(LimitError { kind: LimitKind::CellsInOnePlace, max: MAX_CELLS_PER_CONTAINER as u64 });
        }
        Ok(Some(key))
    }

    /// Puts a `Cell` element in `cells`, or keeps it whole in `passthrough` when it cannot go there: no name, or a name already taken. Whether
    /// it went in `cells`.
    fn put_cell<D: Diagnostics>(
        &mut self,
        cx: &mut Cx<'_, D>,
        cells: &mut Cells,
        passthrough: &mut Passthrough,
        typed: usize,
        el: &El,
    ) -> Result<bool, LimitError> {
        let Some(key) = self.vet_cell(cx, cells, el.attr("N"))? else {
            self.pass_element(passthrough, typed, el)?;
            return Ok(false);
        };
        self.take_cell()?;
        self.take_items(1 + el.attrs.len())?;
        self.take_bytes(self.cell_bytes(&key, cells.len(), el.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str()))))?;
        let cell = self.read_cell(cx, el)?;
        cells.insert(key, cell);
        Ok(true)
    }

    /// Puts the cell `name`, given as attributes and with no content, in `cells`: the form a cell takes where it is no element of its own.
    /// `attrs` are the attributes of the cell as an element would have them but for `N` (`F`, `V`, `U`, `E` and any other, which is kept in
    /// `raw`); a pair named `N` is ignored, as in the element.
    ///
    /// A cell that is stored costs what the element would: one cell, and one item for the element, one for its name and one for each attribute
    /// but `N` (a pair named `N` is ignored and costs nothing). A cell with no name, or a name `cells` has, is warned of and **not** stored
    /// (`Ok(false)`: the first of a name is the one read), and costs the one cell and no item, since nothing of it is kept (an element is
    /// kept whole, and costs its nodes, which a set of pairs is not). A name past `MAX_NAME_BYTES`, or more cells in `cells` than the model allows
    /// in one place, is the error.
    pub fn put_cell_attrs<D: Diagnostics>(
        &mut self,
        cx: &mut Cx<'_, D>,
        cells: &mut Cells,
        name: &str,
        attrs: &[(&str, &str)],
    ) -> Result<bool, LimitError> {
        let Some(key) = self.vet_cell(cx, cells, Some(name))? else {
            return Ok(false);
        };
        self.take_cell()?;
        let wanted = attrs.iter().filter(|(k, _)| *k != "N").count();
        self.take_items(2usize.saturating_add(wanted))?;
        self.take_bytes(self.cell_bytes(&key, cells.len(), attrs.iter().copied()))?;
        let cell = self.cell_from_attrs(cx.notes, attrs.iter().copied())?;
        cells.insert(key, cell);
        Ok(true)
    }

    /// A `Cell` element: its attributes, and what is inside it.
    fn read_cell<D: Diagnostics>(&mut self, cx: &mut Cx<'_, D>, el: &El) -> Result<Cell, LimitError> {
        let mut cell = self.cell_from_attrs(cx.notes, el.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;
        cell.extra = self.cell_extra(el)?;
        Ok(cell)
    }

    /// The cell its attributes say: `V`, `U`, `E` and `F` are its value, unit, error and formula, `N` is ignored, any other goes to `raw`.
    fn cell_from_attrs<'a, D: Diagnostics>(&mut self, notes: &mut D, attrs: impl Iterator<Item = (&'a str, &'a str)>) -> Result<Cell, LimitError> {
        let mut cell = Cell::default();
        let mut raw = RawAttrs::new();
        let mut f = None;
        for (k, v) in attrs {
            match k {
                "N" => {}
                "F" => f = Some(v),
                "V" => cell.value = Some(Value::from_text(v)),
                "U" => cell.unit = Some(self.intern(v)),
                "E" => cell.error = Some(self.intern(v)),
                _ => raw.push(k, v),
            }
        }
        cell.raw = raw;
        if let Some(f) = f {
            // The nodes of the tree are charged to the byte budget once they are made, so the parser is given no more than the byte budget can pay for: a
            // formula it cannot afford is kept as text, as one past the node budget is.
            let node = price::formula_node_bytes();
            // The nodes are allocated by what the budget can pay. A formula has at most a node for each byte of its text, so the parse is held by neither the budget beside the memory of the work ahead
            // nor the one without it when that has the nodes for the text: otherwise the work ahead is over first, and the parse is held by what the budget has.
            let beside = usize::try_from(self.bytes_left / node).unwrap_or(usize::MAX);
            if self.lookahead > 0 && beside < self.formula_budget.min(f.len().saturating_add(1)) {
                self.settle_lookahead();
            }
            let affordable = usize::try_from(self.bytes_left / node).unwrap_or(usize::MAX);
            let before = self.formula_budget.min(affordable);
            let mut budget = before;
            cell.formula = FormulaSlot::read(f, &mut budget);
            let used = before.saturating_sub(budget);
            self.formula_budget = self.formula_budget.saturating_sub(used);
            self.formula_nodes_used = self.formula_nodes_used.saturating_add(used);
            self.take_bytes((used as u64).saturating_mul(FORMULA_NODE_BYTES))?;
            if let FormulaSlot::Local(formula) = &cell.formula {
                self.formula_cells += 1;
                if let Err(error) = &formula.ast {
                    notes.formula_error(error.kind.name());
                }
            }
        }
        Ok(cell)
    }

    /// What a cell costs the byte budget, besides the nodes of its formula (charged when it is parsed): its place in the nodes of its map, its name when the
    /// catalogue does not list it, and every string it keeps (`V`, the text of `F`, `U` and `E` where they are new, and the attributes that go to `raw`).
    fn cell_bytes<'a>(&self, key: &CellName, have: usize, attrs: impl Iterator<Item = (&'a str, &'a str)>) -> u64 {
        let mut bytes = price::cell_node_bytes(have);
        if key.is_allocated() {
            bytes = bytes.saturating_add(price::arc_str(key.as_str().len()));
        }
        let mut raw = 0usize;
        for (k, v) in attrs {
            let more = match k {
                "N" => 0,
                "F" if v == INHERIT_MARKER || v == NO_FORMULA_MARKER => 0,
                // The formula, its text, and the strings and names its tree holds, which are copies of parts of the text: no more than the text.
                "F" => FORMULA_BYTES.saturating_add(price::arc_str(v.len())).saturating_add(price::heap(v.len())),
                "V" => price::arc_str(v.len()),
                "U" | "E" => self.intern_bytes(v),
                _ => {
                    raw += 1;
                    price::raw_attr_bytes(raw - 1, k.len(), v.len())
                }
            };
            bytes = bytes.saturating_add(more);
        }
        bytes
    }

    /// The text and the other nodes inside a cell. A cell that holds text and nothing else keeps it as `text`; a cell that holds anything
    /// else (an element such as `RefBy`, an instruction such as `<?NewValue V?>`, a comment) keeps all of its content in order in the
    /// passthrough, text included, so that the order of a mixed content is not lost.
    fn cell_extra(&mut self, el: &El) -> Result<Option<Arc<CellExtra>>, LimitError> {
        if el.children.is_empty() {
            return Ok(None);
        }
        let only_text = el.children.iter().all(|n| matches!(n, Node::Text(_) | Node::CData(_)));
        let mut extra = CellExtra::default();
        if only_text {
            self.take_items(1)?;
            let len =
                el.children.iter().map(|n| if let Node::Text(t) | Node::CData(t) = n { t.len() } else { 0 }).fold(0usize, usize::saturating_add);
            // The text is put together in a string, and made an `Arc<str>` of its own: both are held at the moment it is made.
            self.take_bytes(price::CELL_EXTRA_BYTES.saturating_add(price::arc_str(len)).saturating_add(price::heap(len)))?;
            let mut text = String::new();
            for node in &el.children {
                if let Node::Text(t) | Node::CData(t) = node {
                    text.push_str(t);
                }
            }
            extra.text = Some(Arc::from(text));
        } else {
            self.take_bytes(price::CELL_EXTRA_BYTES)?;
            for node in &el.children {
                self.take_items(match node {
                    Node::El(child) => element_cost(child),
                    _ => 1,
                })?;
                self.take_bytes(match node {
                    Node::El(child) => price::kept_element_bytes(extra.passthrough.len(), child),
                    other => price::kept_other_bytes(extra.passthrough.len(), other),
                })?;
                extra.passthrough.push(0, raw_node(node));
            }
        }
        Ok(Some(Arc::new(extra)))
    }

    // ------------------------------------------------------------------------------------------------------- sections and rows

    /// A `Section` element, or `None` when it has no name (then the caller keeps it as it is). `tally` is what the sheet's earlier sections left.
    fn read_section<V: Vocabulary, D: Diagnostics>(
        &mut self,
        cx: &mut Cx<'_, D>,
        el: &El,
        sc: &Scope<V>,
        tally: &SheetTally,
        sections_before: usize,
    ) -> Result<Option<Section>, LimitError> {
        let part = cx.part;
        let Some(name) = el.attr("N").filter(|n| !n.is_empty()) else {
            cx.notes.warn("section_without_name", || format!("a section in {} has no name and is kept as it is", part.escape_debug()));
            return Ok(None);
        };
        if name.len() > MAX_NAME_BYTES {
            return Err(LimitError { kind: LimitKind::NameBytes, max: MAX_NAME_BYTES as u64 });
        }
        self.current_section = section_word(name);
        self.take_items(1 + el.attrs.len())?;
        let is_geometry = name == "Geometry";
        let mut section = Section::new(name, None);
        // The section, its place in its sheet's list, its name where the catalogue does not list it, the name noted in the sheet's tally, and what it keeps of
        // its attributes (`N` is the name, a `Geometry` `IX` and a `Del` that read as they are written are typed).
        let mut bytes = SECTION_BYTES.saturating_add(push_bytes(sections_before, POINTER_BYTES));
        if section.name.is_allocated() {
            bytes = bytes.saturating_add(price::arc_str(name.len()));
        }
        if !is_geometry {
            bytes = bytes.saturating_add(price::SEEN_ENTRY_BYTES).saturating_add(price::heap(name.len()));
        }
        let mut kept = 0usize;
        for (k, v) in &el.attrs {
            let typed = match k.as_str() {
                "N" => true,
                "IX" => is_geometry && canonical_u32(v).is_some(),
                "Del" => v == "1",
                _ => false,
            };
            if !typed {
                bytes = bytes.saturating_add(price::raw_attr_bytes(kept, k.len(), v.len()));
                kept += 1;
            }
        }
        self.take_bytes(bytes)?;
        for (k, v) in &el.attrs {
            match k.as_str() {
                "N" => {}
                "IX" if is_geometry => {
                    let (ix, needs_raw) = read_u32(v);
                    section.ix = ix;
                    if needs_raw {
                        section.raw.push(k, v);
                    }
                }
                "Del" => {
                    let (del, needs_raw) = read_flag(v);
                    section.del = del;
                    if needs_raw {
                        section.raw.push(k, v);
                    }
                }
                _ => section.raw.push(k, v),
            }
        }
        if is_geometry {
            let previous = tally.last_geometry_ix;
            let ix = match section.ix {
                Some(ix) => ix,
                None => u32::try_from(tally.geometry).unwrap_or(u32::MAX),
            };
            section.ix = Some(ix);
            if previous.is_some_and(|p| ix <= p) {
                cx.notes.warn("geometry_ix_not_rising", || format!("the Geometry sections in {} do not number upward", part.escape_debug()));
            }
        } else if tally.names.contains(section.name.as_str()) {
            cx.notes.warn("duplicate_section", || {
                format!("a section is named twice in one sheet in {}; the first is the one found", part.escape_debug())
            });
        }
        let keying = keying(name);
        let mut seen: HashSet<RowKey> = HashSet::new();
        let mut typed = 0usize;
        for node in &el.children {
            let Node::El(child) = node else {
                self.pass_other(&mut section.passthrough, typed, node)?;
                continue;
            };
            let csc = sc.enter(child);
            if !csc.is_ours(child) {
                self.pass_element(&mut section.passthrough, typed, child)?;
                continue;
            }
            match child.local() {
                "Cell" => {
                    let (cells, passthrough) = (&mut section.cells, &mut section.passthrough);
                    if self.put_cell(cx, cells, passthrough, typed, child)? {
                        typed += 1;
                    }
                }
                "Row" => {
                    if section.rows.len() >= MAX_ROWS_PER_SECTION {
                        return Err(LimitError { kind: LimitKind::RowsInASection, max: MAX_ROWS_PER_SECTION as u64 });
                    }
                    // Its place in the list of its section, and the key noted for the check of repeated keys.
                    self.take_bytes(push_bytes(section.rows.len(), POINTER_BYTES).saturating_add(price::SEEN_ENTRY_BYTES))?;
                    let row = self.read_row(cx, child, &csc, keying, section.rows.len())?;
                    if let Some(key) = row.key(name)
                        && !seen.insert(key)
                    {
                        cx.notes.warn("duplicate_row_key", || {
                            format!("two rows of one section have one key in {}; the first is the one found", part.escape_debug())
                        });
                    }
                    section.rows.push(Arc::new(row));
                    typed += 1;
                }
                _ => self.pass_element(&mut section.passthrough, typed, child)?,
            }
        }
        self.current_section = "";
        self.max_rows_in_a_section = self.max_rows_in_a_section.max(section.rows.len());
        self.max_cells_in_one_place = self.max_cells_in_one_place.max(section.cells.len());
        Ok(Some(section))
    }

    /// A `Row` element. `before` is how many rows the section holds already.
    fn read_row<V: Vocabulary, D: Diagnostics>(
        &mut self,
        cx: &mut Cx<'_, D>,
        el: &El,
        sc: &Scope<V>,
        keying: Keying,
        before: usize,
    ) -> Result<Row, LimitError> {
        let part = cx.part;
        self.take_items(1 + el.attrs.len())?;
        // The row, and every string it keeps: its names, its type where it is new, and the attributes that go to `raw` (a number or a flag that reads as it is
        // written is typed).
        let mut bytes = ROW_BYTES;
        let mut kept = 0usize;
        for (k, v) in &el.attrs {
            let more = match k.as_str() {
                "N" if !v.is_empty() => price::arc_str(v.len()),
                "LocalName" => price::arc_str(v.len()),
                "T" => self.intern_bytes(v),
                "IX" if canonical_u32(v).is_some() => 0,
                "Del" if v == "1" => 0,
                _ => {
                    kept += 1;
                    price::raw_attr_bytes(kept - 1, k.len(), v.len())
                }
            };
            bytes = bytes.saturating_add(more);
        }
        self.take_bytes(bytes)?;
        let mut row = Row::default();
        for (k, v) in &el.attrs {
            match k.as_str() {
                "N" => {
                    if v.len() > MAX_NAME_BYTES {
                        return Err(LimitError { kind: LimitKind::NameBytes, max: MAX_NAME_BYTES as u64 });
                    }
                    // An empty name is no name (`N=""` used to refuse the whole file as a model limit): the attribute is kept as it was.
                    if v.is_empty() {
                        row.raw.push(k, v);
                    } else {
                        row.name = Some(Arc::from(v.as_str()));
                    }
                }
                "LocalName" => row.local_name = Some(Arc::from(v.as_str())),
                "T" => row.row_type = Some(self.intern(v)),
                "IX" => {
                    let (ix, needs_raw) = read_u32(v);
                    row.ix = ix;
                    if needs_raw {
                        row.raw.push(k, v);
                    }
                }
                "Del" => {
                    let (del, needs_raw) = read_flag(v);
                    row.del = del;
                    if needs_raw {
                        row.raw.push(k, v);
                    }
                }
                _ => row.raw.push(k, v),
            }
        }
        // A row of a section keyed by name that has no `N` (3,274 rows in 36 real files have only a `LocalName`) cannot be
        // found by name. It is read as it is, kept, and warned of; the `LocalName` is not made its name.
        if keying == Keying::Named && row.name.is_none() {
            cx.notes.warn("row_without_name", || {
                format!(
                    "a row of a section keyed by name in {} has no N{}; it is kept and cannot be found by name",
                    part.escape_debug(),
                    if row.local_name.is_some() { " (only a LocalName)" } else { "" }
                )
            });
        }
        // An absent `IX` is the count of earlier rows, in a section keyed by index, or keyed either way when the row has no name.
        let wants_ix = match keying {
            Keying::Indexed => true,
            Keying::Either => row.name.is_none(),
            Keying::Named => false,
        };
        if wants_ix && row.ix.is_none() && !el.attrs.iter().any(|(k, _)| k == "IX") {
            row.ix = Some(u32::try_from(before).unwrap_or(u32::MAX));
        }
        let mut typed = 0usize;
        for node in &el.children {
            let Node::El(child) = node else {
                self.pass_other(&mut row.passthrough, typed, node)?;
                continue;
            };
            let csc = sc.enter(child);
            if csc.is_ours(child) && child.is("Cell") {
                let (cells, passthrough) = (&mut row.cells, &mut row.passthrough);
                if self.put_cell(cx, cells, passthrough, typed, child)? {
                    typed += 1;
                }
            } else {
                self.pass_element(&mut row.passthrough, typed, child)?;
            }
        }
        self.max_cells_in_one_place = self.max_cells_in_one_place.max(row.cells.len());
        Ok(row)
    }
}
