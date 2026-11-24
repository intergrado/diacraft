//! Equal sheets and sections are one allocation (the design's 2.3: "shared pointers are not part of the value").
//!
//! A drawing of a thousand rectangles of one size holds a thousand sheets that differ in two cells and a thousand sections of path rows that do not differ at all, and
//! the model has always been able to share them (a section is behind an `Arc` in its sheet, a sheet in its shape, and a write makes its own copy of what it changes). The
//! `.odg` reader shares what it builds; this decoder built a copy for every shape: 18.7 KB for a rectangle where the reader's own peak is 5.7 KB. So a decode **keeps one
//! copy of each sheet and each section it reads, and gives every later equal one that copy**.
//!
//! **What "equal" is.** A unit is a `sheet` or a `Section` element, and what it decodes to is a function of the text of its subtree and of what the names in it mean where
//! it stands (the namespace each prefix is bound to). The **stream** of a unit is that function's input, written out: every name, attribute, text and node of the subtree,
//! in order, with a length before each string and a mark where each element ends, and then the binding, in the scope the unit is in, of every prefix its names use. Two
//! units are one if and only if their streams are the same bytes. **No hash is the proof of that.** The key of a unit is a 128-bit hash of its stream (two SipHash 1-3
//! with random keys made for each decode), and it only **finds the candidates**: a unit that has the key of a unit read before is compared with it, byte for byte, and
//! is the same unit when the bytes are the same and a different one when they are not (a file that makes two units collide, which on a target whose hasher keys are not
//! secret it can, gets two units and not one). A key holds at most [`BUCKET_MAX`] different units, so that a file made of collisions costs a few comparisons for a unit
//! and not as many as there are units. **A unit that is the same as one read before is not decoded again.** It costs the budget what decoding it cost the items (the
//! time the parse took, and the comparison that proved it the same, which is as long as the unit and no longer, whatever is skipped), a reference
//! ([`SHARED_REF_BYTES`]) in bytes, and the count of unknown content it had (so that the note of what was lost says what it said), and its place in the model is the copy.
//!
//! **What is charged once.** The first unit of a stream is decoded and charged as it always was, and its entry in the table costs [`SHARE_ENTRY_BYTES`] and the bytes of
//! its stream (which the entry holds, for the comparison of the units that follow it, up to [`SHEET_STREAM_BYTES`] for the sheets and [`SECTION_STREAM_BYTES`] for the
//! sections and the sheets together: a unit past them is not entered, and every unit equal to it is decoded as it is). A later equal unit is a reference. **What an edit
//! unshares is charged by the edit** (the engine's `Session::edit_if`), and not by the open: the model a decode builds is what it holds.
//!
//! **Two modes.** An open ([`Sharing::Keep`]) keeps the copy in the table, and a hit is that `Arc`. The save's check ([`Sharing::Count`], `check_decodes`) makes the
//! same charges and keeps nothing of the units but their streams and what they cost, so that what a save holds beside its drawing is the drawing's one part and no copy of
//! what the open will keep, and a hit is a placeholder that nothing reads. The two spend the budget by the same steps, which is what makes a save that passes a package an
//! open takes. [`Sharing::Off`] shares nothing (the memory tests of the prices measure the units one by one).
//!
//! **Where the table's entries are**. The table is let go when the decode ends, and **what it leaves in the memory the model was built in is paid for by every edit
//! after the open**: an edit works over the whole page, and the edits after a `.diacraft` open cost 1.3 to 1.5 times those after a `.vsdx` open of the same drawing, with the same
//! instructions and twice the misses of the translation of addresses (the profile is in `crates/engine/tests/it/native_speed.rs`); a decode that entered nothing in its table, or
//! shared nothing, did not have that cost. A key that held a `Vec` of its units and a unit that held a `Box` of its stream were two small allocations for each unit between the
//! model's own, all let go together at the end. **No unit is an allocation of its own now.** A [`Table`] is a map from a key to the head of its list, the units in one array that
//! grows by doubling (each with the place of the next unit under its key), and the streams in chunks that a stream is made in place in (the first as long as the first stream, each
//! next twice as long as the last, to a mebibyte), so that what the table leaves is a few large holes. **What the table's own memory costs the model** is charged before it is made:
//! [`SHARE_ENTRY_BYTES`] for each entry, which holds the most the array and the map take for one (both at the size after a doubling), its stream, and the room of a chunk beyond the
//! stream that makes the chunk.

use std::collections::{BTreeSet, HashMap};
use std::hash::{BuildHasher, DefaultHasher, Hasher, RandomState};
use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::stamp::Stamp;
use diacraft_doc::{Section, Sheet};

use super::decode::Ours;
use crate::ns::Scope;

/// What a reference to a unit that is shared costs the model, in bytes: the pointer in the list that holds it, and the room of the list.
pub const SHARED_REF_BYTES: u64 = 16;

/// What the entry of a unit in the table costs the model, in bytes, beside the bytes of its stream: the most it takes of the two arrays that hold it, each just after a doubling. Its
/// slot is 40 bytes and the array of slots at most twice as long as it is full, so 80; the map of keys holds a bucket of 32 bytes and a byte of control for it, and is at least seven
/// sixteenths full when it has just doubled, so 76: 156, and 160 with room for the part of a doubling that is not yet used.
pub const SHARE_ENTRY_BYTES: u64 = 160;

/// The most bytes of streams the entries of sheets and sections hold between them, once a sheet is entered: the sheets stop at [`SHEET_STREAM_BYTES`] and the sections,
/// which are what repeats, have the rest. 64 MiB, a tenth of the model.
pub const SECTION_STREAM_BYTES: u64 = 64 << 20;

/// The most bytes of streams the entries hold when a sheet is entered: 32 MiB. A sheet is what differs from shape to shape, and the sections it holds are what repeat.
pub const SHEET_STREAM_BYTES: u64 = 32 << 20;

/// How many different units one key holds. A unit past it is not entered, and a unit equal to it is decoded as it is.
pub const BUCKET_MAX: usize = 4;

/// What a decode does with the units it has read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sharing {
    /// Shares nothing.
    Off,
    /// Keeps the copy of each unit, and gives it to every equal one (an open).
    Keep,
    /// Keeps the stream and the cost of each unit and nothing of the unit (the save's check).
    Count,
}

/// What a decode knows of a unit it has read beside its stream: what reading it cost the items, what unknown content it counted and, when the decode keeps units, its copy.
pub(super) struct Made<T> {
    pub(super) items: u64,
    pub(super) unknown: u64,
    pub(super) value: Option<Arc<T>>,
}

/// A unit as a test gives it to a table: [`Made`] and its stream as a box. A decode gives the table the unit and has it make the stream in place ([`Share::enter_walked`]).
#[cfg(test)]
pub(super) struct Entry<T> {
    pub(super) items: u64,
    pub(super) unknown: u64,
    pub(super) value: Option<Arc<T>>,
    /// The stream of the unit: what a later unit is compared with to be proved the same. The table keeps it in its own chunks, and not as an allocation of its own.
    pub(super) stream: Box<[u8]>,
}

/// An entry as the table keeps it: the cost, the copy, the place of the stream in the table's chunks and the place of the next unit under the same key.
pub(super) struct Slot<T> {
    pub(super) items: u64,
    pub(super) unknown: u64,
    pub(super) value: Option<Arc<T>>,
    /// The next unit under the key (the one entered before this one), or [`NO_UNIT`].
    next: u32,
    chunk: u32,
    start: u32,
    len: u32,
}

/// The end of a list of units.
const NO_UNIT: u32 = u32::MAX;

/// The most bytes of one chunk of streams, unless a stream is longer: a table that holds a few units holds a few kilobytes (its first chunk is as long as its first stream), and a
/// table of millions of them holds chunks that are each one allocation of their own, so that no unit is a small allocation between the model's.
const LAST_CHUNK_BYTES: usize = 1 << 20;

/// The units of one kind a decode has read: **no unit is an allocation of its own** (found that a table of a box and a `Vec` for each unit spread the model it
/// was beside between small allocations, which a walk of the model, and every edit after the read, paid for). The head of the list of the units under a key is in a map, the units
/// are in one array that grows by doubling, each with the place of the next unit under its key, and the streams are in chunks that are large allocations (up to
/// [`LAST_CHUNK_BYTES`]). Each key holds the different units that have it, at most [`BUCKET_MAX`].
pub(super) struct Table<T> {
    heads: HashMap<u128, u32>,
    slots: Vec<Slot<T>>,
    chunks: Vec<Vec<u8>>,
}

/// The bytes `len` from `start` of the chunk `chunk` of `chunks`, or `None` when the chunk is not there or does not hold them all ([`Table::stream_of`]).
pub(crate) fn stream_in(chunks: &[Vec<u8>], chunk: u32, start: u32, len: u32) -> Option<&[u8]> {
    let start = usize::try_from(start).ok()?;
    let end = start.checked_add(usize::try_from(len).ok()?)?;
    chunks.get(usize::try_from(chunk).ok()?)?.get(start..end)
}

/// [`Table::new_chunk_for`] of a table whose last chunk has this `(capacity, length)`, or none.
pub(crate) fn chunk_for(last: Option<(usize, usize)>, len: usize) -> Option<usize> {
    match last {
        Some((capacity, used)) if capacity.saturating_sub(used) >= len => None,
        Some((capacity, _)) => Some(capacity.saturating_mul(2).min(LAST_CHUNK_BYTES).max(len)),
        None => Some(len),
    }
}

impl<T> Table<T> {
    fn new() -> Table<T> {
        Table { heads: HashMap::new(), slots: Vec::new(), chunks: Vec::new() }
    }

    /// The bytes of the stream a slot holds.
    fn stream_of(&self, slot: &Slot<T>) -> Option<&[u8]> {
        stream_in(&self.chunks, slot.chunk, slot.start, slot.len)
    }

    /// The units under `key`, the latest entered first. A list is at most [`BUCKET_MAX`] long, and the walk never follows more.
    fn under(&self, key: u128) -> impl Iterator<Item = &Slot<T>> {
        let mut at = self.heads.get(&key).copied().unwrap_or(NO_UNIT);
        let mut left = BUCKET_MAX;
        std::iter::from_fn(move || {
            left = left.checked_sub(1)?;
            let slot = self.slots.get(usize::try_from(at).ok()?)?;
            at = slot.next;
            Some(slot)
        })
    }

    /// The entry of the key whose stream is one `same` says yes to.
    pub(super) fn find(&self, key: u128, same: impl Fn(&[u8]) -> bool) -> Option<&Slot<T>> {
        self.under(key).find(|slot| self.stream_of(slot).is_some_and(&same))
    }

    /// Whether the key can hold another unit.
    pub(super) fn has_room(&self, key: u128) -> bool {
        self.under(key).count() < BUCKET_MAX && u32::try_from(self.slots.len()).is_ok_and(|n| n < NO_UNIT)
    }

    /// The length of the chunk that a stream of `len` bytes needs made for it, or `None` when it fits at the end of the last one: the first chunk is as long as the stream, each next
    /// is twice as long as the last (to [`LAST_CHUNK_BYTES`]) and never shorter than the stream.
    fn new_chunk_for(&self, len: usize) -> Option<usize> {
        chunk_for(self.chunks.last().map(|last| (last.capacity(), last.len())), len)
    }

    /// What a stream of `len` bytes makes beyond itself, in bytes: the room of the chunk made for it, less the stream, and nothing when it fits in the last chunk.
    pub(super) fn chunk_slack(&self, len: usize) -> usize {
        self.new_chunk_for(len).map_or(0, |size| size.saturating_sub(len))
    }

    /// Makes the room for a stream of `len` bytes at the end of the last chunk, in a new chunk if it does not fit there. `None` when the chunk cannot be had.
    fn room_for(&mut self, len: usize) -> Option<()> {
        if let Some(size) = self.new_chunk_for(len) {
            let mut chunk: Vec<u8> = Vec::new();
            chunk.try_reserve_exact(size).ok()?;
            self.chunks.push(chunk);
        }
        Some(())
    }

    /// Enters a unit under its key, if the key has room for another, **its stream of `len` bytes made by `fill` at the end of a chunk**: the stream is no allocation of its own, not even
    /// for the moment it is made in (a box made and let go for each unit was a hole in the memory the model is being built in). `false`, and nothing entered, when the key has no room, the
    /// room for the stream cannot be had, or `fill` did not make `len` bytes.
    pub(super) fn put_with(&mut self, key: u128, len: usize, made: Made<T>, fill: impl FnOnce(&mut Vec<u8>)) -> bool {
        if !self.has_room(key) {
            return false;
        }
        let (Ok(stream_len), Ok(at)) = (u32::try_from(len), u32::try_from(self.slots.len())) else { return false };
        if self.room_for(len).is_none() {
            return false;
        }
        let Ok(chunk_at) = u32::try_from(self.chunks.len().saturating_sub(1)) else { return false };
        let Some(chunk) = self.chunks.last_mut() else { return false };
        let Ok(start) = u32::try_from(chunk.len()) else { return false };
        fill(chunk);
        if chunk.len() != usize::try_from(start).unwrap_or(usize::MAX).saturating_add(len) {
            chunk.truncate(usize::try_from(start).unwrap_or(0));
            return false;
        }
        let next = self.heads.insert(key, at).unwrap_or(NO_UNIT);
        self.slots.push(Slot { items: made.items, unknown: made.unknown, value: made.value, next, chunk: chunk_at, start, len: stream_len });
        true
    }

    /// [`Table::put_with`] with the stream given as a box.
    #[cfg(test)]
    pub(super) fn put(&mut self, key: u128, entry: Entry<T>) -> bool {
        let made = Made { items: entry.items, unknown: entry.unknown, value: entry.value };
        self.put_with(key, entry.stream.len(), made, |chunk| chunk.extend_from_slice(&entry.stream))
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.slots.len()
    }

    /// How many chunks the streams are in.
    #[cfg(test)]
    pub(super) fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// The bytes the chunks have room for, and the bytes of streams they hold.
    #[cfg(test)]
    pub(super) fn chunk_bytes(&self) -> (usize, usize) {
        (self.chunks.iter().map(Vec::capacity).sum(), self.chunks.iter().map(Vec::len).sum())
    }

    /// How many entries hold the copy of their unit.
    #[cfg(test)]
    pub(super) fn values_kept(&self) -> usize {
        self.slots.iter().filter(|e| e.value.is_some()).count()
    }
}

/// The units of a part that its `def` attributes name, **by name, one name for a sheet or a section** (the format's 3.19): a `same` takes the copy
/// ([`super::vocab::el::SAME`]). A name holds the copy when the decode keeps units and nothing when it only checks.
#[derive(Default)]
pub(super) struct Defs {
    map: HashMap<String, AnyDef>,
    /// The names in the order they were given, for the names of a unit that is not read to be taken back.
    order: Vec<String>,
}

/// A named unit of either kind.
pub(super) enum AnyDef {
    Sheet(Def<Sheet>),
    Section(Def<Section>),
}

/// A named unit: its copy (when the decode keeps units), what reading it cost the items and what unknown content it counted, which each `same` that names it costs
/// again, as a unit that an equal one is the same as ([`Entry`]), and its fingerprint ([`super::unitfp`]), which a `same` inside another unit is.
pub(super) struct Def<T> {
    pub(super) value: Option<Arc<T>>,
    pub(super) items: u64,
    pub(super) unknown: u64,
    pub(super) fp: Stamp,
}

impl AnyDef {
    fn fp(&self) -> Stamp {
        match self {
            AnyDef::Sheet(d) => d.fp,
            AnyDef::Section(d) => d.fp,
        }
    }
}

impl Defs {
    pub(super) fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }

    /// How many names are given.
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.map.len()
    }

    pub(super) fn get(&self, name: &str) -> Option<&AnyDef> {
        self.map.get(name)
    }

    /// The fingerprint of the unit a name names.
    pub(super) fn fingerprint_of(&self, name: &str) -> Option<Stamp> {
        self.map.get(name).map(AnyDef::fp)
    }

    /// Gives `name` to a unit, if no unit of the part has it: whether it was given.
    pub(super) fn insert(&mut self, name: String, def: AnyDef) -> bool {
        if self.map.contains_key(&name) {
            return false;
        }
        self.order.push(name.clone());
        self.map.insert(name, def);
        true
    }

    /// Where the names are now, for [`Defs::rollback`].
    pub(super) fn mark(&self) -> usize {
        self.order.len()
    }

    /// Takes back the names given since `mark`: a unit that is not read gives none.
    pub(super) fn rollback(&mut self, mark: usize) {
        while self.order.len() > mark {
            if let Some(name) = self.order.pop() {
                self.map.remove(&name);
            }
        }
    }
}

/// What a decode shares: its mode, its keys and its tables.
pub(super) struct Share {
    pub(super) mode: Sharing,
    keyer: Keyer,
    pub(super) sections: Table<Section>,
    pub(super) sheets: Table<Sheet>,
    /// The bytes of the streams the entries hold.
    streams: u64,
    empty_sheet: Arc<Sheet>,
    empty_section: Arc<Section>,
}

impl Share {
    pub(super) fn new(mode: Sharing) -> Share {
        Share {
            mode,
            keyer: Keyer { a: RandomState::new(), b: RandomState::new(), collide: false },
            sections: Table::new(),
            sheets: Table::new(),
            streams: 0,
            empty_sheet: Arc::new(Sheet::default()),
            empty_section: Arc::new(Section::new("", None)),
        }
    }

    /// A share whose every unit has the one key: what a target whose hasher keys no one keeps secret can be made to have.
    #[cfg(test)]
    pub(super) fn colliding(mode: Sharing) -> Share {
        let mut share = Share::new(mode);
        share.keyer.collide = true;
        share
    }

    /// The key of the unit `root`, in the scope `sc` inside it.
    #[cfg(test)]
    pub(super) fn key(&self, root: &El, sc: &Scope<Ours>) -> u128 {
        self.keyer.key_len(root, sc).0
    }

    /// The key of the unit `root`, in the scope `sc` inside it, the bytes of its stream, and whether it is relative to its part.
    pub(super) fn key_len(&self, root: &El, sc: &Scope<Ours>) -> (u128, u64, bool) {
        self.keyer.key_len(root, sc)
    }

    /// The stream of the unit `root`, in the scope `sc` inside it, of `len` bytes (what [`Share::key_len`] said).
    #[cfg(test)]
    pub(super) fn stream(&self, root: &El, sc: &Scope<Ours>, len: u64) -> Box<[u8]> {
        let mut out = Recording(Vec::with_capacity(usize::try_from(len).unwrap_or(0)));
        walk(root, sc, &mut out);
        out.0.into_boxed_slice()
    }

    /// The entry of the unit `root` in its table, if a unit with its key **and its stream** was read before.
    pub(super) fn find<T: Unit>(&self, key: u128, root: &El, sc: &Scope<Ours>) -> Option<&Slot<T>> {
        T::table_of(self).find(key, |stream| same_stream(root, sc, stream))
    }

    /// Whether a unit whose stream is `len` bytes can be entered: its key holds room for another unit, and the streams held are not past what this kind keeps.
    pub(super) fn has_room<T: Unit>(&self, key: u128, len: u64) -> bool {
        T::table_of(self).has_room(key) && self.streams.saturating_add(len) <= T::STREAM_ROOM
    }

    /// What entering a unit of kind `T` whose stream is `len` bytes makes in the table beyond the stream itself, in bytes (the room of a chunk it has to make): to be charged first.
    pub(super) fn chunk_slack<T: Unit>(&self, len: u64) -> u64 {
        let slack = usize::try_from(len).map_or(0, |len| T::table_of(self).chunk_slack(len));
        u64::try_from(slack).unwrap_or(u64::MAX)
    }

    /// Enters a unit read, whose stream of `len` bytes (what [`Share::key_len`] said) is made in the table from `root` in the scope `sc`: [`Share::has_room`] said it could be.
    pub(super) fn enter_walked<T: Unit>(&mut self, key: u128, len: u64, (root, sc): (&El, &Scope<Ours>), made: Made<T>) {
        let Ok(bytes) = usize::try_from(len) else { return };
        if T::table(self).put_with(key, bytes, made, |chunk| walk(root, sc, &mut Appending(chunk))) {
            self.streams = self.streams.saturating_add(len);
        }
    }

    /// Enters a unit read, whose stream is `entry.stream`.
    #[cfg(test)]
    pub(super) fn enter<T: Unit>(&mut self, key: u128, entry: Entry<T>) {
        let len = u64::try_from(entry.stream.len()).unwrap_or(u64::MAX);
        if T::table(self).put(key, entry) {
            self.streams = self.streams.saturating_add(len);
        }
    }

    /// The sheet of a shape that has none of its own: the one empty sheet, when a decode shares, and one made for it when it does not.
    pub(super) fn default_sheet(&self) -> Arc<Sheet> {
        match self.mode {
            Sharing::Off => Arc::new(Sheet::default()),
            Sharing::Keep | Sharing::Count => Arc::clone(&self.empty_sheet),
        }
    }
}

/// A kind of unit: where its table is, what stands in for a copy a check does not keep, and how much stream its entries may hold.
pub(super) trait Unit: Sized {
    /// The most bytes of streams the entries hold between all kinds when a unit of this kind is entered.
    const STREAM_ROOM: u64;
    fn table(share: &mut Share) -> &mut Table<Self>;
    fn table_of(share: &Share) -> &Table<Self>;
    fn placeholder(share: &Share) -> Arc<Self>;
    /// A named unit of this kind, as a unit of either kind.
    fn named(def: Def<Self>) -> AnyDef;
    /// The unit of this kind that a name names, if it names one of this kind.
    fn of_kind(def: &AnyDef) -> Option<&Def<Self>>;
}

impl Unit for Sheet {
    const STREAM_ROOM: u64 = SHEET_STREAM_BYTES;
    fn table(share: &mut Share) -> &mut Table<Sheet> {
        &mut share.sheets
    }
    fn table_of(share: &Share) -> &Table<Sheet> {
        &share.sheets
    }
    fn placeholder(share: &Share) -> Arc<Sheet> {
        Arc::clone(&share.empty_sheet)
    }
    fn named(def: Def<Sheet>) -> AnyDef {
        AnyDef::Sheet(def)
    }
    fn of_kind(def: &AnyDef) -> Option<&Def<Sheet>> {
        match def {
            AnyDef::Sheet(d) => Some(d),
            AnyDef::Section(_) => None,
        }
    }
}

impl Unit for Section {
    const STREAM_ROOM: u64 = SECTION_STREAM_BYTES;
    fn table(share: &mut Share) -> &mut Table<Section> {
        &mut share.sections
    }
    fn table_of(share: &Share) -> &Table<Section> {
        &share.sections
    }
    fn placeholder(share: &Share) -> Arc<Section> {
        Arc::clone(&share.empty_section)
    }
    fn named(def: Def<Section>) -> AnyDef {
        AnyDef::Section(def)
    }
    fn of_kind(def: &AnyDef) -> Option<&Def<Section>> {
        match def {
            AnyDef::Section(d) => Some(d),
            AnyDef::Sheet(_) => None,
        }
    }
}

/// The two keys of a decode.
struct Keyer {
    a: RandomState,
    b: RandomState,
    /// Every unit has the one key (a test of what a collision does).
    collide: bool,
}

/// Where the stream of a unit goes: into the hashers that make its key, into a buffer, or against the bytes of a stream that was kept.
trait Sink {
    fn put(&mut self, bytes: &[u8]);
    /// The subtree says something that means what it means only where it stands in its part (a name it gives, or a `same` that names one): see [`walk`].
    fn relative(&mut self) {}
    /// Whether the rest of the stream is of no use (it has differed from the one it is compared with).
    fn done(&self) -> bool {
        false
    }
}

/// The key of a stream, and its length.
struct Hashing {
    a: DefaultHasher,
    b: DefaultHasher,
    len: u64,
    relative: bool,
}

impl Sink for Hashing {
    fn put(&mut self, bytes: &[u8]) {
        self.a.write(bytes);
        self.b.write(bytes);
        self.len = self.len.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
    }

    fn relative(&mut self) {
        self.relative = true;
    }
}

/// A stream kept.
#[cfg(test)]
struct Recording(Vec<u8>);

#[cfg(test)]
impl Sink for Recording {
    fn put(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }
}

/// A stream made at the end of a vector that has the room for it (a chunk of a table).
struct Appending<'a>(&'a mut Vec<u8>);

impl Sink for Appending<'_> {
    fn put(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }
}

/// A stream compared with one kept: it is the same if every byte is, and nothing more is said or left.
struct Comparing<'c> {
    rest: &'c [u8],
    same: bool,
}

impl Sink for Comparing<'_> {
    fn put(&mut self, bytes: &[u8]) {
        match self.rest.strip_prefix(bytes) {
            Some(rest) if self.same => self.rest = rest,
            _ => self.same = false,
        }
    }

    fn done(&self) -> bool {
        !self.same
    }
}

fn tag<S: Sink>(out: &mut S, tag: u8) {
    out.put(&[tag]);
}

/// A string, with its length before it, so that where one ends and the next begins is part of the stream.
fn text<S: Sink>(out: &mut S, mark: u8, text: &str) {
    tag(out, mark);
    out.put(&u64::try_from(text.len()).unwrap_or(u64::MAX).to_le_bytes());
    out.put(text.as_bytes());
}

/// An element's name and attributes, and the prefixes they use that are bound outside the element (an attribute with no prefix is in no namespace). **A name a unit
/// gives (`def`) and a `same` that names one mean what they mean where they stand in their part** (the part's names are the part's own, and the next part gives the
/// same names to other units), so a subtree that holds one is said to be relative to its part, and no unit of the table is the same as it.
fn open<'a, S: Sink>(out: &mut S, e: &'a El, prefixes: &mut BTreeSet<&'a str>) {
    let local = e.name.rsplit_once(':').map_or(e.name.as_str(), |(_, l)| l);
    if local == super::vocab::el::SAME
        || (local == super::vocab::el::LONG && e.attrs.iter().any(|(k, v)| k == super::vocab::at::N_LONG && v == super::vocab::at::DEF))
        || e.attrs.iter().any(|(k, _)| k == super::vocab::at::DEF || k.strip_suffix(super::vocab::at::B64_SUFFIX) == Some(super::vocab::at::DEF))
    {
        out.relative();
    }
    text(out, OPEN, &e.name);
    prefixes.insert(e.name.split_once(':').map_or("", |(p, _)| p));
    out.put(&u64::try_from(e.attrs.len()).unwrap_or(u64::MAX).to_le_bytes());
    for (k, v) in &e.attrs {
        text(out, ATTR, k);
        text(out, ATTR, v);
        if k != "xmlns"
            && !k.starts_with("xmlns:")
            && let Some((p, _)) = k.split_once(':')
            && p != "xml"
        {
            prefixes.insert(p);
        }
    }
}

const OPEN: u8 = 1;
const ATTR: u8 = 2;
const CLOSE: u8 = 3;
const TEXT: u8 = 4;
const CDATA: u8 = 5;
const COMMENT: u8 = 6;
const PI_TARGET: u8 = 7;
const PI_DATA: u8 = 8;
const PREFIX: u8 = 9;
const BOUND: u8 = 10;
const UNBOUND: u8 = 11;

/// The stream of `root`: its subtree, in order, and the namespace each prefix its names use is bound to in `sc`. Not recursive: the stack is at most as deep as the
/// tree, which the container holds to its ceiling.
fn walk<S: Sink>(root: &El, sc: &Scope<Ours>, out: &mut S) {
    // The prefixes the names of the subtree use, sorted and once each; "" is the default namespace, which a name with no prefix is in.
    let mut prefixes: BTreeSet<&str> = BTreeSet::new();
    open(out, root, &mut prefixes);
    let mut stack: Vec<(&El, usize)> = vec![(root, 0)];
    while let Some((e, at)) = stack.pop() {
        if out.done() {
            return;
        }
        let Some(node) = e.children.get(at) else {
            tag(out, CLOSE);
            continue;
        };
        stack.push((e, at + 1));
        match node {
            Node::El(child) => {
                open(out, child, &mut prefixes);
                stack.push((child, 0));
            }
            Node::Text(t) => text(out, TEXT, t),
            Node::CData(t) => text(out, CDATA, t),
            Node::Comment(t) => text(out, COMMENT, t),
            Node::Pi { target, data } => {
                text(out, PI_TARGET, target);
                text(out, PI_DATA, data);
            }
        }
    }
    for prefix in prefixes {
        text(out, PREFIX, prefix);
        match sc.bound(prefix) {
            Some(uri) => text(out, BOUND, &uri),
            None => tag(out, UNBOUND),
        }
    }
}

/// Whether the stream of `root` is `kept`, byte for byte.
fn same_stream(root: &El, sc: &Scope<Ours>, kept: &[u8]) -> bool {
    let mut out = Comparing { rest: kept, same: true };
    walk(root, sc, &mut out);
    out.same && out.rest.is_empty()
}

impl Keyer {
    /// The key of `root`, a hash of its stream, the stream's length, and whether the subtree is relative to its part (see [`open`]).
    fn key_len(&self, root: &El, sc: &Scope<Ours>) -> (u128, u64, bool) {
        let mut out = Hashing { a: self.a.build_hasher(), b: self.b.build_hasher(), len: 0, relative: false };
        walk(root, sc, &mut out);
        let key = if self.collide { 0 } else { (u128::from(out.a.finish()) << 64) | u128::from(out.b.finish()) };
        (key, out.len, out.relative)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(name: &str, attrs: &[(&str, &str)], children: Vec<Node>) -> El {
        El { name: name.to_owned(), attrs: attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(), children }
    }

    /// The scope inside a root that declares `prefix` as `uri` (and the codec's own namespace as the default).
    fn scope(prefix: &str, uri: &str) -> Scope<Ours> {
        let root = el("r", &[("xmlns", super::super::URN), (&format!("xmlns:{prefix}"), uri)], vec![]);
        Scope::<Ours>::empty().enter(&root)
    }

    fn key_of(share: &Share, unit: &El) -> u128 {
        share.key(unit, &scope("q", "urn:q"))
    }

    #[test]
    fn a_key_is_the_same_for_the_same_subtree_and_a_hash_of_every_part_of_it() {
        let share = Share::new(Sharing::Keep);
        let cell = |attrs: &[(&str, &str)]| Node::El(el("Cell", attrs, vec![]));
        let unit = |children: Vec<Node>| el("Section", &[("N", "a")], children);
        let base = unit(vec![cell(&[("N", "ab"), ("V", "c")])]);
        assert_eq!(key_of(&share, &base), key_of(&share, &base.clone()), "the same subtree, the same key");
        let others = [
            unit(vec![cell(&[("N", "a"), ("V", "bc")])]),
            unit(vec![cell(&[("V", "c"), ("N", "ab")])]),
            unit(vec![cell(&[("N", "ab"), ("V", "c"), ("U", "")])]),
            unit(vec![cell(&[("N", "ab"), ("V", "c")]), cell(&[("N", "ab"), ("V", "c")])]),
            unit(vec![Node::El(el("Cell", &[("N", "ab"), ("V", "c")], vec![Node::Text(" ".to_owned())]))]),
            unit(vec![Node::El(el("Cell", &[("N", "ab"), ("V", "c")], vec![Node::Comment(" ".to_owned())]))]),
            unit(vec![Node::El(el("Cell", &[("N", "ab"), ("V", "c")], vec![Node::CData(" ".to_owned())]))]),
            unit(vec![Node::El(el("Cell", &[("N", "ab"), ("V", "c")], vec![Node::Pi { target: "t".to_owned(), data: String::new() }]))]),
            unit(vec![Node::El(el("Cell", &[("N", "ab"), ("V", "c")], vec![Node::Pi { target: String::new(), data: "t".to_owned() }]))]),
            unit(vec![Node::El(el("Cell", &[("N", "ab"), ("V", "c")], vec![Node::El(el("extra", &[], vec![]))]))]),
            el("Section", &[("N", "b")], vec![cell(&[("N", "ab"), ("V", "c")])]),
            el("Row", &[("N", "a")], vec![cell(&[("N", "ab"), ("V", "c")])]),
            unit(vec![]),
        ];
        let mut keys: Vec<u128> = others.iter().map(|o| key_of(&share, o)).collect();
        keys.push(key_of(&share, &base));
        let distinct: std::collections::HashSet<u128> = keys.iter().copied().collect();
        assert_eq!(distinct.len(), keys.len(), "every difference of the text is a difference of the key");
    }

    #[test]
    fn a_key_says_what_the_prefixes_of_a_subtree_are_bound_to_where_it_stands() {
        let share = Share::new(Sharing::Keep);
        let unit = el("Section", &[("N", "a")], vec![Node::El(el("q:x", &[("q:a", "1")], vec![]))]);
        let one = share.key(&unit, &scope("q", "urn:one"));
        let two = share.key(&unit, &scope("q", "urn:two"));
        assert_ne!(one, two, "the same text under two bindings of a prefix is two units");
        assert_eq!(one, share.key(&unit, &scope("q", "urn:one")), "and under one binding it is one");
        // A prefix the subtree does not use is no part of its key; the default namespace, which its unprefixed names are in, is.
        let plain = el("Section", &[("N", "a")], vec![]);
        assert_eq!(share.key(&plain, &scope("z", "urn:1")), share.key(&plain, &scope("z", "urn:2")), "a prefix no name of the subtree uses");
        let under = |uri: &str| {
            let root = el("r", &[("xmlns", uri)], vec![]);
            share.key(&plain, &Scope::<Ours>::empty().enter(&root))
        };
        assert_ne!(under("urn:a"), under("urn:b"), "an unprefixed name is in the default namespace of where it stands");
        assert_eq!(under("urn:a"), under("urn:a"));
    }

    #[test]
    fn a_prefix_that_only_an_attribute_uses_is_bound_in_the_key_and_the_declarations_and_the_xml_prefix_are_not_names() {
        let share = Share::new(Sharing::Keep);
        // A kept attribute of a prefix is written back with the declaration of its prefix, so what the prefix is bound to is part of what the unit is.
        let unit = el("Cell", &[("N", "a"), ("z:note", "1")], vec![]);
        assert_ne!(share.key(&unit, &scope("z", "urn:one")), share.key(&unit, &scope("z", "urn:two")));
        // A declaration is not a use of its prefix, and `xml` is bound by no one: units that differ only in what the scope binds to prefixes they do not use are one.
        let declares = el("Cell", &[("N", "a"), ("xmlns:z", "urn:inside"), ("xml:space", "preserve")], vec![]);
        let under = |prefix: &str, uri: &str| share.key(&declares, &scope(prefix, uri));
        assert_eq!(under("q", "urn:one"), under("q", "urn:two"));
    }

    #[test]
    fn a_key_is_a_hash_of_a_hundred_and_twenty_eight_bits_and_both_halves_are_the_subtree() {
        let share = Share::new(Sharing::Keep);
        for i in 0..32 {
            let key = key_of(&share, &el("Section", &[("N", &format!("s{i}"))], vec![]));
            assert_ne!(key >> 64, 0, "{i}: the high half");
            assert_ne!(key & u128::from(u64::MAX), 0, "{i}: the low half");
            assert_ne!(key >> 64, key & u128::from(u64::MAX), "{i}: the two halves are two hashes");
        }
    }

    #[test]
    fn the_placeholder_of_a_sheet_that_a_check_gives_for_a_unit_it_keeps_nothing_of_is_the_one_empty_sheet() {
        for mode in [Sharing::Keep, Sharing::Count] {
            let share = Share::new(mode);
            assert!(Arc::ptr_eq(&<Sheet as Unit>::placeholder(&share), &share.default_sheet()));
            assert!(Arc::ptr_eq(&<Section as Unit>::placeholder(&share), &<Section as Unit>::placeholder(&share)));
            assert_eq!(*<Section as Unit>::placeholder(&share), Section::new("", None));
        }
    }

    #[test]
    fn a_key_made_by_another_decode_is_another_key_of_the_same_subtree() {
        // The keys of a decode are its own: no file can make two units collide by a hash it can compute.
        let (a, b) = (Share::new(Sharing::Keep), Share::new(Sharing::Keep));
        let unit = el("Section", &[("N", "a")], vec![]);
        assert_ne!(key_of(&a, &unit), key_of(&b, &unit));
    }

    #[test]
    fn the_empty_sheet_of_a_shape_is_one_copy_when_a_decode_shares_and_a_new_one_when_it_does_not() {
        for mode in [Sharing::Keep, Sharing::Count] {
            let share = Share::new(mode);
            assert!(Arc::ptr_eq(&share.default_sheet(), &share.default_sheet()), "{mode:?}");
        }
        let off = Share::new(Sharing::Off);
        assert!(!Arc::ptr_eq(&off.default_sheet(), &off.default_sheet()));
        assert_eq!(*off.default_sheet(), Sheet::default());
    }

    fn entry(items: u64, stream: &[u8]) -> Entry<Sheet> {
        Entry { items, unknown: 2, value: Some(Arc::new(Sheet::default())), stream: stream.into() }
    }

    #[test]
    fn a_table_gives_back_what_it_was_given_by_its_key_and_its_stream_and_nothing_else() {
        let mut share = Share::new(Sharing::Keep);
        let any = |_: &[u8]| true;
        assert!(share.sheets.find(7, any).is_none());
        assert!(share.sheets.put(7, entry(5, b"one")));
        let hit = share.sheets.find(7, any).expect("an entry");
        assert_eq!((hit.items, hit.unknown), (5, 2));
        assert!(share.sheets.find(8, any).is_none());
        assert!(share.sections.find(7, any).is_none(), "the tables of the kinds are apart");
        assert_eq!(share.sheets.len(), 1);
        // A key whose stream is not the one asked for has no entry, and the next of the key is found beside the first.
        assert!(share.sheets.find(7, |stream| stream == b"two").is_none());
        assert!(share.sheets.put(7, entry(6, b"two")));
        assert_eq!(share.sheets.find(7, |stream| stream == b"two").expect("the second").items, 6);
        assert_eq!(share.sheets.find(7, |stream| stream == b"one").expect("the first").items, 5);
        assert_eq!(share.sheets.len(), 2);
    }

    #[test]
    fn the_names_of_a_part_are_one_map_for_both_kinds_given_once_and_taken_back_with_the_unit_that_gave_them() {
        let fp = Stamp(7);
        let sheet = |items| AnyDef::Sheet(Def { value: None, items, unknown: 0, fp });
        let section = |items| AnyDef::Section(Def { value: None, items, unknown: 0, fp });
        let mut defs = Defs::default();
        assert_eq!(defs.len(), 0);
        assert!(defs.insert("a".to_owned(), sheet(1)));
        assert!(!defs.insert("a".to_owned(), section(2)), "a name is given once, whatever the kinds");
        assert!(defs.insert("b".to_owned(), section(3)));
        assert_eq!(defs.len(), 2);
        // What a name names is of one kind.
        let kind =
            |name: &str| defs.get(name).map(|d| (<Sheet as Unit>::of_kind(d).map(|d| d.items), <Section as Unit>::of_kind(d).map(|d| d.items)));
        assert_eq!(kind("a"), Some((Some(1), None)));
        assert_eq!(kind("b"), Some((None, Some(3))));
        assert_eq!(kind("c"), None);
        // The names of a unit that is not read are taken back, and only those.
        let mark = defs.mark();
        assert!(defs.insert("c".to_owned(), sheet(4)) && defs.insert("d".to_owned(), sheet(5)));
        assert_eq!(defs.len(), 4);
        defs.rollback(mark);
        assert_eq!((defs.len(), defs.get("c").is_none(), defs.get("a").is_some()), (2, true, true));
        assert!(defs.insert("c".to_owned(), sheet(6)), "a name that was taken back can be given");
        assert_eq!(defs.fingerprint_of("c"), Some(fp));
        defs.clear();
        assert_eq!(defs.len(), 0);
        assert_eq!(defs.mark(), 0);
    }

    #[test]
    fn a_key_holds_at_most_a_few_different_units_and_a_unit_past_them_is_not_entered() {
        let mut share = Share::new(Sharing::Keep);
        for i in 0..BUCKET_MAX {
            assert!(share.sheets.has_room(1), "{i}");
            assert!(share.sheets.put(1, entry(i as u64, &[i as u8])), "{i}");
        }
        assert!(!share.sheets.has_room(1));
        assert!(!share.sheets.put(1, entry(99, b"more")));
        assert_eq!(share.sheets.len(), BUCKET_MAX);
        assert!(share.sheets.has_room(2), "another key has room of its own");
    }

    #[test]
    fn the_stream_of_a_unit_is_the_same_bytes_for_the_same_unit_and_is_no_other_unit_s() {
        let share = Share::new(Sharing::Keep);
        let unit = el("Section", &[("N", "a")], vec![Node::El(el("Cell", &[("N", "x"), ("V", "1")], vec![]))]);
        let sc = scope("q", "urn:q");
        let (key, len, _) = share.key_len(&unit, &sc);
        let stream = share.stream(&unit, &sc, len);
        assert_eq!(u64::try_from(stream.len()).unwrap(), len, "the length that was counted is the length that is made");
        assert!(same_stream(&unit, &sc, &stream));
        assert!(same_stream(&unit.clone(), &sc, &stream));
        // One byte more or one byte less in what is kept, and one byte different in the unit, are all different units.
        let mut longer = stream.to_vec();
        longer.push(0);
        assert!(!same_stream(&unit, &sc, &longer));
        assert!(!same_stream(&unit, &sc, &stream[..stream.len() - 1]));
        let other = el("Section", &[("N", "a")], vec![Node::El(el("Cell", &[("N", "x"), ("V", "2")], vec![]))]);
        assert!(!same_stream(&other, &sc, &stream));
        assert_ne!(share.key(&other, &sc), key);
        // And what a prefix is bound to is in the stream.
        let used = el("Section", &[("N", "a")], vec![Node::El(el("q:x", &[], vec![]))]);
        let (_, n, _) = share.key_len(&used, &scope("q", "urn:one"));
        let kept = share.stream(&used, &scope("q", "urn:one"), n);
        assert!(same_stream(&used, &scope("q", "urn:one"), &kept));
        assert!(!same_stream(&used, &scope("q", "urn:two"), &kept));
    }

    #[test]
    fn a_key_that_every_unit_has_still_tells_two_units_apart_by_their_streams() {
        // What a target whose hasher keys are no secret can be made to do: two different units under one key are two units, and the same unit twice is one.
        let share = Share::colliding(Sharing::Keep);
        let sc = scope("q", "urn:q");
        let a = el("Section", &[("N", "a")], vec![]);
        let b = el("Section", &[("N", "b")], vec![]);
        assert_eq!(share.key(&a, &sc), share.key(&b, &sc), "the test's keys all collide");
        let mut share = share;
        let (key, len, _) = share.key_len(&a, &sc);
        let stream = share.stream(&a, &sc, len);
        share.enter::<Sheet>(key, Entry { items: 1, unknown: 0, value: Some(Arc::new(Sheet::default())), stream });
        assert!(share.find::<Sheet>(key, &a, &sc).is_some(), "the same unit is found");
        assert!(share.find::<Sheet>(key, &b, &sc).is_none(), "a different unit under the same key is not");
    }

    #[test]
    fn the_streams_an_entry_holds_are_held_to_what_a_kind_keeps() {
        let mut share = Share::new(Sharing::Keep);
        let key = 3;
        // Under the sheets' room a sheet is entered, and past it, with the streams held, it is not; a section has the room that the sheets leave, up to its own.
        assert!(share.has_room::<Sheet>(key, SHEET_STREAM_BYTES));
        assert!(!share.has_room::<Sheet>(key, SHEET_STREAM_BYTES + 1));
        assert!(share.has_room::<Section>(key, SECTION_STREAM_BYTES));
        assert!(!share.has_room::<Section>(key, SECTION_STREAM_BYTES + 1));
        share.enter::<Sheet>(key, entry(1, &[0u8; 1000]));
        assert!(share.has_room::<Sheet>(9, SHEET_STREAM_BYTES - 1000));
        assert!(!share.has_room::<Sheet>(9, SHEET_STREAM_BYTES - 999));
        assert!(share.has_room::<Section>(9, SECTION_STREAM_BYTES - 1000));
        assert!(!share.has_room::<Section>(9, SECTION_STREAM_BYTES - 999));
    }

    #[test]
    fn the_key_and_the_stream_of_a_unit_have_each_string_after_its_length_and_each_element_closed() {
        // Two subtrees that differ only in where a string ends, or where an element ends, are two units, in their keys and in their streams. A stream that left out the
        // length of a string, or the close of an element, would write the same bytes for both.
        let share = Share::new(Sharing::Keep);
        let sc = scope("q", "urn:q");
        let cell = |n: &str| Node::El(el("Cell", &[("N", n)], vec![]));
        let row = |children: Vec<Node>| Node::El(el("Row", &[], children));
        // A cell closed with its row, and a cell after it.
        let one = el("Section", &[], vec![row(vec![cell("a")]), cell("b")]);
        let two = el("Section", &[], vec![row(vec![cell("a"), cell("b")])]);
        // Where the text of the attributes ends.
        let ab = el("Section", &[("a", "bc"), ("d", "e")], vec![]);
        let abc = el("Section", &[("a", "b"), ("cd", "e")], vec![]);
        // Text, and a name that holds the bytes a separator would be.
        let tab = el("Section", &[], vec![Node::Text("a\tb".to_owned())]);
        let tabs = el("Section", &[], vec![Node::Text("a".to_owned()), Node::Text("b".to_owned())]);
        for (x, y) in [(&one, &two), (&ab, &abc), (&tab, &tabs)] {
            let ((kx, lx, _), (ky, ly, _)) = (share.key_len(x, &sc), share.key_len(y, &sc));
            assert_ne!(kx, ky, "{x:?} and {y:?}");
            let (sx, sy) = (share.stream(x, &sc, lx), share.stream(y, &sc, ly));
            assert_ne!(sx, sy, "{x:?} and {y:?}");
            assert!(!same_stream(y, &sc, &sx) && !same_stream(x, &sc, &sy));
        }
    }

    #[test]
    fn namespace_uris_that_hold_a_tab_or_a_line_feed_are_bound_by_their_length_and_not_by_a_separator() {
        // A binding written as `prefix TAB uri LF` would write the same text for these pairs of scopes.
        let share = Share::new(Sharing::Keep);
        let unit = el("Section", &[], vec![Node::El(el("q:a", &[("r:b", "1")], vec![]))]);
        let scope2 = |q: &str, r: &str| {
            let root = el("r", &[("xmlns", super::super::URN), ("xmlns:q", q), ("xmlns:r", r)], vec![]);
            Scope::<Ours>::empty().enter(&root)
        };
        let pairs = [
            (scope2("a", "b"), scope2("a\tb", "")),
            (scope2("a\nr\tb", ""), scope2("a", "b")),
            (scope2("u\tv", "w"), scope2("u", "v\tw")),
            (scope2("u\nv", "w"), scope2("u", "v\nw")),
            (scope2("", "a\nq\tb"), scope2("a", "b")),
        ];
        for (x, y) in &pairs {
            let ((kx, lx, _), (ky, ly, _)) = (share.key_len(&unit, x), share.key_len(&unit, y));
            assert_ne!(kx, ky);
            let (sx, sy) = (share.stream(&unit, x, lx), share.stream(&unit, y, ly));
            assert_ne!(sx, sy);
            assert!(!same_stream(&unit, y, &sx));
        }
    }

    #[test]
    fn a_table_keeps_its_units_in_a_few_chunks_and_no_unit_has_an_allocation_of_its_own() {
        // Three thousand different units of a hundred bytes and a little more: the streams are in chunks (the first as long as the first stream, each next twice as long as the last),
        // so there are a dozen of them and not three thousand, and each stream is where it was put, whole.
        let mut share = Share::new(Sharing::Keep);
        let stream = |i: u32| vec![(i % 251) as u8; 100 + (i % 7) as usize];
        for i in 0..3_000u32 {
            assert!(share.sheets.put(u128::from(i), entry(u64::from(i), &stream(i))), "{i}");
        }
        assert_eq!(share.sheets.len(), 3_000);
        assert!(share.sheets.chunk_count() <= 14, "{} chunks", share.sheets.chunk_count());
        let (room, held) = share.sheets.chunk_bytes();
        assert_eq!(held, (0..3_000u32).map(|i| stream(i).len()).sum::<usize>(), "every stream is in a chunk, and nothing else is");
        assert!(room < 3 * held, "the chunks have room for less than three times what they hold ({room} for {held})");
        for i in 0..3_000u32 {
            let want = stream(i);
            let hit = share.sheets.find(u128::from(i), |s| s == want.as_slice()).expect("the unit");
            assert_eq!(hit.items, u64::from(i), "{i}");
            assert!(share.sheets.find(u128::from(i), |s| s != want.as_slice()).is_none(), "{i}: no other stream is under the key");
        }
    }

    #[test]
    fn a_chunk_is_as_long_as_its_first_stream_then_twice_the_last_to_a_mebibyte_and_what_it_has_beyond_its_stream_is_told_before_it_is_made() {
        let mut share = Share::new(Sharing::Keep);
        let table = |share: &Share| (share.sheets.chunk_count(), share.sheets.chunk_bytes());
        // The first stream makes a chunk as long as itself: nothing beyond it.
        assert_eq!(share.sheets.chunk_slack(100), 0);
        assert!(share.sheets.put(1, entry(0, &[1u8; 100])));
        assert_eq!(table(&share), (1, (100, 100)));
        // A second does not fit in it, and makes a chunk twice as long as the last: a hundred bytes beyond itself.
        assert_eq!(share.sheets.chunk_slack(100), 100);
        assert!(share.sheets.put(2, entry(0, &[2u8; 100])));
        assert_eq!(table(&share), (2, (300, 200)));
        // The hundred bytes left in it take a third, and tell of nothing beyond it.
        assert_eq!(share.sheets.chunk_slack(100), 0);
        assert!(share.sheets.put(3, entry(0, &[3u8; 100])));
        assert_eq!(table(&share), (2, (300, 300)));
        // A fourth makes a chunk of 400 (three hundred beyond itself); one that is longer than twice the last makes a chunk as long as itself.
        assert_eq!(share.sheets.chunk_slack(100), 300);
        assert_eq!(share.sheets.chunk_slack(900), 0);
        assert!(share.sheets.put(4, entry(0, &[4u8; 900])));
        assert_eq!(table(&share), (3, (1_200, 1_200)));
        // The chunks do not grow past a mebibyte, unless a stream is longer: then it is a chunk of its own, as long as it.
        let mebibyte = 1usize << 20;
        let mut key = 10;
        while share.sheets.chunk_bytes().0 < 4 * mebibyte {
            assert!(share.sheets.put(key, entry(0, &vec![5u8; 300_000])));
            key += 1;
        }
        let last = share.sheets.chunk_bytes().0;
        assert_eq!(share.sheets.chunk_slack(3 * mebibyte), 0, "a stream longer than the chunks is the length of its chunk");
        assert!(share.sheets.chunk_slack(100) <= mebibyte, "a chunk is at most a mebibyte beyond its stream");
        assert_eq!(last, share.sheets.chunk_bytes().0);
    }

    #[test]
    fn a_stream_the_table_cannot_make_whole_is_not_entered_and_leaves_the_others_as_they_were() {
        let mut share = Share::new(Sharing::Keep);
        let made = |items| Made { items, unknown: 0, value: None };
        assert!(share.sheets.put_with(1, 4, made(1), |chunk| chunk.extend_from_slice(b"abcd")));
        // A stream that is made shorter or longer than it said is no entry, and what it wrote is taken back.
        assert!(!share.sheets.put_with(2, 4, made(2), |chunk| chunk.extend_from_slice(b"ab")));
        assert!(!share.sheets.put_with(3, 4, made(3), |chunk| chunk.extend_from_slice(b"abcdef")));
        assert_eq!(share.sheets.len(), 1);
        assert!(share.sheets.find(2, |_| true).is_none() && share.sheets.find(3, |_| true).is_none());
        assert!(share.sheets.put_with(4, 3, made(4), |chunk| chunk.extend_from_slice(b"xyz")));
        assert_eq!(share.sheets.find(1, |s| s == b"abcd").expect("the first").items, 1);
        assert_eq!(share.sheets.find(4, |s| s == b"xyz").expect("the last").items, 4);
        assert_eq!(share.sheets.chunk_bytes().1, 7, "the bytes of the streams that failed are not held");
    }
}
