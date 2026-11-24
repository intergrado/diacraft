//! Writing a model as a `.diacraft` package (the public spec's sections 2 and 6.3; the plan's).
//!
//! **No file is written that an open refuses** (6.3): the codec's save checks every part and the package as an open would ([`save_parts`]), and this adds what the
//! package holds beside the codec's parts: the entries of parts of unknown roles, the preview, the manifest and the state. A refusal is in words and names what is large.
//!
//! **One fixed order, one fixed level, and the same bytes for the same model** (2.9): the entries are written in the order `mimetype`, `manifest.xml`, `document.xml`,
//! the pages in page order, the masters in master order, the blobs by number, `state.xml`, `preview.png`, and the parts of unknown roles in the order they were read; every
//! entry is dated 1980-01-01, deflated at [`DEFLATE_LEVEL`] when it is deflated, and written as XML in the canonical form; no hash-map order reaches the output.
//!
//! **What is copied as it is.** Given the bytes and the [`Decoded`] of the last read or write, a part whose model object is still the very allocation that was decoded,
//! and every blob of which still maps to the very allocation it was, is copied entry for entry, compressed bytes and all, and not written again: a save that changed
//! nothing writes the file it read, and a save that changed a page rewrites that page's part and no other. A part is copied **only** when it can be said to mean what it
//! meant: blobs are numbered by first use, so a part that names a blob by number is copied only when that number is still that blob (Review Focus 1).
//!
//! **Threads** ([`write_threads`]). With more than one, the entries are deflated on workers while the next part is made, and written in the order they were given, so **the package is
//! the same bytes at any number of threads**; the codec makes the parts of its first pass together and holds the ones that fit for the second ([`SaveThreads`]). Each entry in flight is
//! charged to the same meter as before, before it is made (the bytes it holds besides those of its deflated copy), and one that the meter cannot afford beside the ones in flight waits until
//! they are written, and is made as a single thread makes it when none is.
//!
//! **Memory.** What this crate makes itself is charged to [`Limits::max_memory`] before it is made: the serialized bytes of a part and the deflated copy of it, the package
//! as it grows, and the directory of the archive. The model and the codec's trees are the codec's (`save_parts_bytes`, which makes a page and a master a child at a time and hands over their bytes, and the engine's count of what a session holds).

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use diacraft_container::par::{self, Failed, Lane};
use diacraft_container::zip::{Archive, ArchiveWriter, Entry, Method, Pack, Packed, pack};
use diacraft_container::{ContainerError, Work};
use diacraft_doc::{Document, Format, Part, PartRole, SharedVec};
use diacraft_sheetxml::model::{
    BlobId, BlobSink, BlobSource, CodecError, PartLists, PartRole as SavedRole, SaveThreads, SavedOwned, Stamp, save_parts_owned, stamp_of_bytes,
};

use crate::blobs::{BlobTable, stored};
use crate::decoded::{BlobRecord, Decoded, EntryRecord, ExtraRecord, PartRecord, Print, blobs_named_bytes};
use crate::error::NativeError;
use crate::kind::{DEFLATE_LEVEL, Kind};
use crate::limits::{Limits, directory_bytes, part_cap};
use crate::manifest::{self, Row};
use crate::names::{DOCUMENT, MANIFEST, MIMETYPE, NativeJudge, PREVIEW, STATE, role, valid_entry_name};
use crate::state::{self, State, highest_ids};

/// The most bytes of a preview picture: it is at most 256 pixels on its longer side, and this is a few times the largest such picture.
const PREVIEW_MAX: usize = 4 << 20;
/// The first bytes of every PNG picture.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
/// The most bytes of `written-by`: free text for people.
const WRITTEN_BY_MAX: usize = 256;
/// The most bytes of a word in a manifest row (a role, a media type): what a reader accepts.
const WORD_MAX: usize = crate::manifest::WORD_MAX;

/// What a write is asked to make beyond the model.
#[derive(Clone, Debug)]
pub struct WriteOptions {
    /// The kind of file: the `mimetype` entry and the manifest's `kind` (1.2).
    pub kind: Kind,
    /// Who wrote the file (`DiaCraft 0.2.0`): free text for people, cut to 256 bytes and with anything XML forbids replaced.
    pub written_by: String,
    /// The preview picture, a PNG of the first page at most 256 pixels on its longer side (2.8), or `None`. A write whose model is unchanged since the file it is
    /// given as [`Previous`] copies the preview that file had.
    pub preview: Option<Vec<u8>>,
    /// What the write is held to: the entries, the file's length, and the memory.
    pub limits: Limits,
}

impl Default for WriteOptions {
    fn default() -> WriteOptions {
        WriteOptions { kind: Kind::Drawing, written_by: String::new(), preview: None, limits: Limits::default() }
    }
}

/// The package a model was last read from or written as, and what the read or write learned of it, for a write to copy from. See [`Decoded`].
#[derive(Clone, Copy, Debug)]
pub struct Previous<'a> {
    /// The bytes of the package.
    pub bytes: &'a [u8],
    /// What was learned of them.
    pub decoded: &'a Decoded,
}

/// A package, and what a later write copies from it.
#[derive(Clone, Debug)]
pub struct Written {
    /// The package.
    pub bytes: Vec<u8>,
    /// What a later write copies from: the model's objects as they were written, the entries that hold them, and the blobs.
    pub decoded: Decoded,
    /// How many entries were copied as they were: pages, masters, blobs, parts of unknown roles and the preview.
    pub copied: usize,
}

/// The bytes of an entry that is on its way to be written: a part the codec made and handed over, bytes the model holds (shared, never copied), or a few of this crate's own.
enum Data {
    Owned(Vec<u8>),
    Shared(Arc<[u8]>),
}

impl Data {
    fn bytes(&self) -> &[u8] {
        match self {
            Data::Owned(bytes) => bytes,
            Data::Shared(bytes) => bytes,
        }
    }
}

/// What an entry is given as: lent for the call, given with its bytes, or shared with the model.
enum Input<'d> {
    Lent(&'d [u8]),
    Owned(Vec<u8>),
    Shared(Arc<[u8]>),
}

impl Input<'_> {
    fn bytes(&self) -> &[u8] {
        match self {
            Input::Lent(bytes) => bytes,
            Input::Owned(bytes) => bytes,
            Input::Shared(bytes) => bytes,
        }
    }
}

/// What a write has of an entry in the lane, to be written in its place: an entry compressed (or checksummed) apart, or one of the previous package copied as it is.
enum Done {
    Packed { name: String, data: Data, how: Pack, packed: Result<Packed, ContainerError>, charged: u64 },
    Copy { index: usize, name: String, charged: u64 },
}

/// The lane a write puts its entries through.
type Pipe<'l, 's, 'e> = &'l Lane<'s, 'e, Done>;

/// The smallest entry that is given to a worker: a smaller one is compressed where it is given, since the time of a hand-over is more than its compression.
const OFFLOAD_FROM: usize = 32 * 1024;

/// The most bytes of entries that may be in the lane at once, beside what the meter says: the lane never holds more than a few parts, however much the meter has.
const FLIGHT_BYTES: usize = 256 << 20;

/// A table shared between the codec's save, which numbers and reads blobs through it, and the writer, which reads it when the first part is handed over.
#[derive(Clone)]
struct SharedTable(Rc<RefCell<BlobTable>>);

impl BlobSink for SharedTable {
    fn put(&mut self, bytes: &Arc<[u8]>, media_type: &str) -> Result<BlobId, CodecError> {
        self.0.borrow_mut().put(bytes, media_type)
    }
}

impl BlobSource for SharedTable {
    fn get(&self, id: BlobId) -> Option<Arc<[u8]>> {
        self.0.borrow().get(id)
    }
}

/// The memory this crate may still make: taken before a thing is made, and given back when it is let go.
struct Meter {
    left: u64,
}

impl Meter {
    fn take(&mut self, n: u64) -> Result<(), NativeError> {
        self.left = self.left.checked_sub(n).ok_or(NativeError::TooLarge(
            "the file needs more memory to save than is free: the drawing and the other files that are open leave no room for it",
        ))?;
        Ok(())
    }

    fn give(&mut self, n: u64) {
        self.left = self.left.saturating_add(n);
    }
}

fn big(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// A part of an unknown role, as the package writes it: its row and its bytes, as held in the model.
struct Native {
    row: Row,
    bytes: Arc<[u8]>,
}

/// The package a write copies from: its bytes and directory, and the names of its entries.
struct Prior<'p> {
    bytes: &'p [u8],
    archive: Archive<'p>,
    index: HashMap<String, usize>,
    decoded: &'p Decoded,
    /// The decoded page parts by the address of the model object, and the masters.
    pages: HashMap<usize, usize>,
    masters: HashMap<usize, usize>,
    /// The parts of unknown roles it remembers, by name.
    extras: HashMap<&'p str, &'p ExtraRecord>,
}

/// The method an entry is wanted in to be copied: the one the writer would pick for it, or any (a part of an unknown role is stored as it was, 2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wanted {
    Stored,
    Deflated,
    Any,
}

impl Prior<'_> {
    /// The index of the entry `record` describes, when the previous package holds it as it was read: the same name, size and checksum.
    fn verified(&self, record: &EntryRecord) -> Option<usize> {
        let index = *self.index.get(&record.name)?;
        let entry = self.archive.entries().get(index)?;
        (entry.crc == record.crc && entry.size == record.size).then_some(index)
    }

    /// The index of the entry `record` describes **when it can be copied into this writer's package as it is**: the previous package holds it as it was read, **its headers are
    /// exactly the ones this writer writes** ([`headers_are_ours`]: a copy keeps them byte for byte, so another tool's date, extra fields, comment and attributes would come
    /// into a file the format says has none, 2.1), and its method is the wanted one (2.2, 2.6).
    fn copyable(&self, record: &EntryRecord, wanted: Wanted) -> Option<usize> {
        let index = self.verified(record)?;
        let method = headers_are_ours(self.bytes, self.archive.entries().get(index)?)?;
        match (wanted, method) {
            (Wanted::Any, _) | (Wanted::Stored, Method::Stored) | (Wanted::Deflated, Method::Deflated) => Some(index),
            _ => None,
        }
    }

    /// The method the previous package holds the entry `record` describes in, when it holds it as it was read.
    fn method_of(&self, record: &EntryRecord) -> Option<Method> {
        self.archive.entries().get(self.verified(record)?)?.compression()
    }
}

/// Whether the headers of `entry` are exactly those this writer writes for a file of its name and method: the local header, byte for byte (the fixed date of 1980, no extra
/// field, no data descriptor, the UTF-8 flag where the name needs it, the version of its method), and the row of the directory in every field it holds (no extra field, no
/// comment, no attribute). Gives its method. **An entry that another tool zipped is not this**, and is written afresh.
fn headers_are_ours(bytes: &[u8], entry: &Entry) -> Option<Method> {
    let method = entry.compression()?;
    let version: u16 = if method == Method::Stored { 10 } else { 20 };
    let name = entry.name.as_bytes();
    let flags: u16 = if entry.name.is_ascii() { 0 } else { 0x0800 };
    let row = entry.flags == flags
        && entry.version_made_by == version
        && entry.version_needed == version
        && entry.time == 0
        && entry.date == 0x0021
        && entry.internal_attributes == 0
        && entry.external_attributes == 0
        && entry.row_len == 46usize.checked_add(name.len())?
        && entry.local_extra_len == 0
        && entry.written.is_none()
        && !entry.folder;
    if !row || entry.data_offset != entry.local_offset.checked_add(30)?.checked_add(name.len())? {
        return None;
    }
    let mut want: Vec<u8> = Vec::with_capacity(30 + name.len());
    want.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
    want.extend_from_slice(&version.to_le_bytes());
    want.extend_from_slice(&flags.to_le_bytes());
    want.extend_from_slice(&method.number().to_le_bytes());
    want.extend_from_slice(&0u16.to_le_bytes());
    want.extend_from_slice(&0x0021u16.to_le_bytes());
    want.extend_from_slice(&entry.crc.to_le_bytes());
    want.extend_from_slice(&u32::try_from(entry.compressed).ok()?.to_le_bytes());
    want.extend_from_slice(&u32::try_from(entry.size).ok()?.to_le_bytes());
    want.extend_from_slice(&u16::try_from(name.len()).ok()?.to_le_bytes());
    want.extend_from_slice(&0u16.to_le_bytes());
    want.extend_from_slice(name);
    let local = bytes.get(entry.local_offset..entry.data_offset)?;
    (local == want.as_slice() && entry.row_extra_and_comment(bytes) == Some((&[][..], &[][..]))).then_some(method)
}

/// The addresses of the objects of a list, each with the index it stands at.
fn addresses<T>(records: &[PartRecord<T>]) -> HashMap<usize, usize> {
    records.iter().enumerate().map(|(i, r)| (Arc::as_ptr(&r.object).addr(), i)).collect()
}

/// A model written, a part at a time, as a package: the writer holds what the codec hands over, and keeps what the later steps need.
struct Emitter<'a, 'p, 'l, 's, 'e> {
    writer: ArchiveWriter,
    /// The lane the entries are compressed in, when the write has threads: **an entry given to it is written in its place by [`Emitter::commit`]**, and none is written by other means while any is in it.
    pipe: Option<Pipe<'l, 's, 'e>>,
    meter: Meter,
    options: &'a WriteOptions,
    doc: &'a Document,
    table: SharedTable,
    natives: Vec<Native>,
    state: Vec<u8>,
    prior: Option<Prior<'p>>,
    lists: Option<PartLists>,
    /// For each page and master, in order: the entry of the previous package to copy it from, or `None` to write it.
    page_copy: Vec<Option<usize>>,
    master_copy: Vec<Option<usize>>,
    pages_done: usize,
    masters_done: usize,
    page_blobs: Vec<Option<Vec<u32>>>,
    master_blobs: Vec<Option<Vec<u32>>>,
    preview: Preview,
    /// The bytes of the previous package's preview when it is written again ([`Preview::Reread`]): read before the manifest is made, since the manifest lists the entry.
    kept_preview: Vec<u8>,
    /// What this write has made the container do (the open of the previous package, and the read of its preview).
    work: Work,
    /// The stamp of the document part this write made, for the next write to know it by.
    document_stamp: Option<Stamp>,
    copied: usize,
    /// What the package holds inflated so far, and what it is known to be going to hold besides (the blobs, the state, the preview and the parts of unknown roles,
    /// which the lists give before they are written): together held to the package cap.
    inflated: u64,
    ahead: u64,
    failure: Option<NativeError>,
}

/// What the package does about its preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Preview {
    /// None is written.
    None,
    /// The picture the options carry.
    Given,
    /// The previous package's, copied from the entry at this index.
    Copy(usize),
    /// The previous package's, **written again** from the bytes held in [`Emitter::kept_preview`]: its entry is not one to copy as it is, since another tool zipped it.
    Reread,
}

impl Emitter<'_, '_, '_, '_, '_> {
    fn fail(&mut self, e: NativeError) -> CodecError {
        if self.failure.is_none() {
            self.failure = Some(e);
        }
        CodecError::Malformed { part: "package".to_owned(), why: "the package could not be written" }
    }

    /// A part the codec handed over **as its bytes** (a page or a master is made a child at a time and never as a tree, so that a page of any number of shapes is
    /// written), which are the bytes the package stores.
    fn emit(&mut self, part: SavedOwned<'_>) -> Result<(), CodecError> {
        let done = match part.role {
            SavedRole::Document => match part.lists {
                Some(lists) => self.document(&part.bytes, lists),
                None => Err(NativeError::Unwritable("the codec handed over a document part with no lists")),
            },
            SavedRole::Page { .. } => self.page(part.name, part.bytes),
            SavedRole::Master { .. } => self.master(part.name, part.bytes),
        };
        done.map_err(|e| self.fail(e))
    }

    /// Adds `bytes` as the entry `name`, deflated at the package's level, charging the deflated copy and the entry's growth of the package.
    fn add_xml(&mut self, name: &str, bytes: &[u8], ahead: bool) -> Result<(), NativeError> {
        self.add_entry(name, Input::Lent(bytes), Pack::Level(DEFLATE_LEVEL), ahead)
    }

    /// Adds `bytes` as the entry `name`, stored.
    fn add_stored(&mut self, name: &str, bytes: &[u8], ahead: bool) -> Result<(), NativeError> {
        self.add_entry(name, Input::Lent(bytes), Pack::Stored, ahead)
    }

    /// Adds the entry `name` of `data`, packed as `how` says (stored, or deflated at the package's level). **With a lane it is compressed on a worker** and written, in its place, when
    /// the entries before it are: what it holds beside the bytes of its copy is charged to the meter before it is given, an entry the meter cannot afford beside the ones in the lane waits until
    /// they are written, and one that it cannot afford alone is made as it always was, which refuses it in words.
    fn add_entry(&mut self, name: &str, data: Input<'_>, how: Pack, ahead: bool) -> Result<(), NativeError> {
        let len = big(data.bytes().len());
        self.written(len, ahead)?;
        // The deflated copy is at most the bytes and a few more, and the package grows by what it came to and the row of the directory.
        let worst = len.saturating_add(256).saturating_add(big(name.len()).saturating_mul(2));
        if let Some(pipe) = self.pipe {
            // What the entry holds while it waits to be written: its bytes, unless the model holds them already.
            let held = if matches!(data, Input::Shared(_)) { 0 } else { len };
            let charge = worst.saturating_add(held);
            let size = data.bytes().len();
            loop {
                while let Some(done) = pipe.try_take() {
                    self.commit(done)?;
                }
                if pipe.charged().saturating_add(size) <= FLIGHT_BYTES && self.meter.take(charge).is_ok() {
                    break;
                }
                if pipe.pending() == 0 {
                    // Nothing is in the lane to be given back: the entry is made as it was before there were threads.
                    return self.add_now(name, data.bytes(), how, worst);
                }
                let done = pipe.take();
                if let Some(done) = done {
                    self.commit(done)?;
                }
            }
            let data = match data {
                Input::Lent(bytes) => Data::Owned(bytes.to_vec()),
                Input::Owned(bytes) => Data::Owned(bytes),
                Input::Shared(bytes) => Data::Shared(bytes),
            };
            let name = name.to_owned();
            let make = move || {
                let packed = pack(&name, data.bytes(), how);
                // The bytes of an entry that was deflated are no use any more: they are let go here and not when the entry is written.
                let data = match &packed {
                    Ok(p) if p.method() == Method::Deflated => Data::Owned(Vec::new()),
                    _ => data,
                };
                Done::Packed { name, data, how, packed, charged: charge }
            };
            if size < OFFLOAD_FROM {
                pipe.submit_done(size, make());
            } else {
                pipe.submit(size, make);
            }
            return Ok(());
        }
        self.add_now(name, data.bytes(), how, worst)
    }

    /// The entry written at once, by this thread: what is charged is the deflated copy.
    fn add_now(&mut self, name: &str, bytes: &[u8], how: Pack, worst: u64) -> Result<(), NativeError> {
        self.meter.take(worst)?;
        let before = self.writer.written_bytes();
        match how {
            Pack::Stored => self.writer.add(name, bytes, Method::Stored)?,
            _ => self.writer.add_at_level(name, bytes, DEFLATE_LEVEL)?,
        }
        self.grew(before, name, worst)
    }

    /// An entry of the lane written, in its place: the writer takes what a worker made of it, and what was charged for it is given back and what it took of the package is charged.
    fn commit(&mut self, result: Result<Done, Failed>) -> Result<(), NativeError> {
        let done = result
            .map_err(|_| NativeError::Unwritable("a worker thread of the save stopped while it compressed an entry, so the file was not written"))?;
        match done {
            Done::Packed { name, data, how, packed, charged } => {
                let before = self.writer.written_bytes();
                self.writer.add_packed(&name, data.bytes(), how, packed)?;
                drop(data);
                self.grew(before, &name, charged)
            }
            Done::Copy { index, name, charged } => {
                let before = self.writer.written_bytes();
                if let Some(prior) = &self.prior {
                    self.writer.copy(&prior.archive, index)?;
                }
                self.copied += 1;
                self.grew(before, &name, charged)
            }
        }
    }

    /// Writes every entry that is in the lane.
    fn drain(&mut self) -> Result<(), NativeError> {
        let Some(pipe) = self.pipe else { return Ok(()) };
        while let Some(done) = pipe.take() {
            self.commit(done)?;
        }
        Ok(())
    }

    /// What an entry written took of the package: what was charged beforehand is given back, and what the package grew by stays charged.
    fn grew(&mut self, before: usize, name: &str, charged: u64) -> Result<(), NativeError> {
        let grew = big(self.writer.written_bytes().saturating_sub(before)).saturating_add(46).saturating_add(big(name.len()));
        self.meter.give(charged);
        self.meter.take(grew)?;
        if big(self.writer.written_bytes()) > self.options.limits.max_file_bytes {
            return Err(NativeError::TooLarge("the package would be larger than a DiaCraft file may be"));
        }
        Ok(())
    }

    /// An entry of `len` inflated bytes is about to be written: `ahead` says it is one the lists gave before (and is no more than they said, so the total does not
    /// move), and the total, written and still to come, is held to the package cap.
    fn written(&mut self, len: u64, ahead: bool) -> Result<(), NativeError> {
        if ahead {
            self.ahead = self.ahead.saturating_sub(len);
        }
        self.inflated = self.inflated.saturating_add(len);
        self.check_inflated()
    }

    fn check_inflated(&self) -> Result<(), NativeError> {
        if self.inflated.saturating_add(self.ahead) > self.options.limits.container.max_package_bytes {
            return Err(NativeError::TooLarge("the package would hold more bytes, inflated, than a DiaCraft file may"));
        }
        Ok(())
    }

    /// Copies the entry at `index` of the previous package as it is, charging what it takes of the package.
    fn copy(&mut self, index: usize, ahead: bool) -> Result<(), NativeError> {
        let Some(prior) = &self.prior else { return Ok(()) };
        let Some(entry) = prior.archive.entries().get(index) else { return Ok(()) };
        let (size, compressed, name) = (entry.size, entry.compressed, entry.name.clone());
        self.written(size, ahead)?;
        let worst = compressed.saturating_add(256).saturating_add(big(name.len()).saturating_mul(2));
        if let Some(pipe) = self.pipe {
            // Given in its place in the lane, so that the entries before it are written first.
            loop {
                while let Some(done) = pipe.try_take() {
                    self.commit(done)?;
                }
                if self.meter.take(worst).is_ok() {
                    pipe.submit_done(0, Done::Copy { index, name, charged: worst });
                    return Ok(());
                }
                if pipe.pending() == 0 {
                    break;
                }
                let done = pipe.take();
                if let Some(done) = done {
                    self.commit(done)?;
                }
            }
        }
        self.meter.take(worst)?;
        let before = self.writer.written_bytes();
        if let Some(prior) = &self.prior {
            self.writer.copy(&prior.archive, index)?;
        }
        self.copied += 1;
        self.grew(before, &name, worst)
    }

    /// The document part, the first handed over: `lists` are what it lists (the codec read them from the part it made), every decision of what to copy is made, and the first
    /// entries are written.
    fn document(&mut self, document: &[u8], lists: &PartLists) -> Result<(), NativeError> {
        let lists = lists.clone();
        let counted = 5usize
            .saturating_add(lists.pages.len())
            .saturating_add(lists.masters.len())
            .saturating_add(lists.blobs.len())
            .saturating_add(self.natives.len());
        if counted > self.options.limits.container.max_parts {
            return Err(NativeError::TooLarge("the package would have more entries than a DiaCraft file may"));
        }
        self.check_names(&lists)?;
        // What the package holds besides the parts, inflated: the blobs and the parts of unknown roles are known now.
        let known: u64 = lists
            .blobs
            .iter()
            .map(|b| b.size)
            .chain(self.natives.iter().map(|n| big(n.bytes.len())))
            .chain(self.options.preview.iter().map(|p| big(p.len())))
            .fold(big(self.state.len()), u64::saturating_add);
        self.ahead = known;
        self.check_inflated()?;
        let stamp = stamp_of_bytes(document);
        self.document_stamp = Some(stamp);
        self.decide(&lists, document, stamp);
        // The manifest lists every entry, in the order they are written.
        let rows = self.rows(&lists);
        let manifest = manifest::write(self.options.kind, &written_by(&self.options.written_by), &rows)?;
        let kind = self.options.kind.media_type();
        self.add_stored(MIMETYPE, kind.as_bytes(), false)?;
        self.add_xml(MANIFEST, &manifest, false)?;
        self.add_xml(DOCUMENT, document, false)?;
        self.lists = Some(lists);
        Ok(())
    }

    /// The names of the entries the codec's parts and the blobs have, and the names the entries of the package that are fixed: none of the parts of unknown roles
    /// takes one (**a part is never written over another**, and a writer that cannot keep one does not write the file as if it had none, 5.2).
    fn check_names(&self, lists: &PartLists) -> Result<(), NativeError> {
        let mut taken: HashSet<&str> = [MIMETYPE, MANIFEST, DOCUMENT, STATE, PREVIEW].into_iter().collect();
        taken.extend(lists.pages.iter().map(|(_, n)| n.as_str()));
        taken.extend(lists.masters.iter().map(|(_, n)| n.as_str()));
        taken.extend(lists.blobs.iter().map(|b| b.path.as_str()));
        for native in &self.natives {
            if !taken.insert(native.row.path.as_str()) {
                return Err(NativeError::Unwritable("a part the drawing holds has the name of an entry of the package, or of another part"));
            }
        }
        Ok(())
    }

    /// Which pages and masters are copied from the previous package, and what is done about the preview.
    fn decide(&mut self, lists: &PartLists, document: &[u8], stamp: Stamp) {
        let table = self.table.0.borrow();
        // For each page and master: whether the model object is the one decoded (and its blobs the allocations they were), which is what *unchanged* means; and the entry to copy,
        // which is also asked of the headers of the entry and its method (it is `None` for a package another tool zipped, whose pages are as they were and are written again).
        let (mut pages, mut masters): (Vec<Option<usize>>, Vec<Option<usize>>) = (Vec::new(), Vec::new());
        let mut as_read = true;
        for (page, (id, name)) in self.doc.pages.iter().zip(&lists.pages) {
            let record = self
                .prior
                .as_ref()
                .and_then(|p| Some((p, unchanged_record(p, &p.pages, &p.decoded.pages, (Arc::as_ptr(page).addr(), *id), name, &table)?)));
            as_read &= record.is_some();
            pages.push(record.and_then(|(p, r)| p.copyable(&r.entry, Wanted::Deflated)));
        }
        for (master, (id, name)) in self.doc.masters.iter().zip(&lists.masters) {
            let record = self
                .prior
                .as_ref()
                .and_then(|p| Some((p, unchanged_record(p, &p.masters, &p.decoded.masters, (Arc::as_ptr(master).addr(), *id), name, &table)?)));
            as_read &= record.is_some();
            masters.push(record.and_then(|(p, r)| p.copyable(&r.entry, Wanted::Deflated)));
        }
        drop(table);
        // The model is unchanged when the document part is the bytes it was (its size and checksum, and its stamp, which a checksum made to collide does not share) and every
        // page and master is as it was read, whoever zipped the package. **A part that held the namespace of the builds before the id moved to `diacraft.app` is the bytes it was
        // when it is those bytes with the current namespace in its place**, which is what this write makes of a drawing that did not change: the file keeps its picture.
        let made = Print { stamp, size: big(document.len()), crc: crc32fast::hash(document) };
        let unchanged = as_read
            && self.prior.as_ref().is_some_and(|p| {
                p.decoded.document.as_ref().is_some_and(|d| {
                    let as_read = p.decoded.document_stamp.map(|stamp| Print { stamp, size: d.size, crc: d.crc });
                    (as_read == Some(made) || p.decoded.document_as_current == Some(made)) && p.verified(d).is_some()
                })
            });
        self.preview = if self.options.preview.is_some() {
            Preview::Given
        } else if unchanged {
            self.previous_preview()
        } else {
            Preview::None
        };
        self.page_copy = pages;
        self.master_copy = masters;
    }

    /// What is done about the preview of the previous package for a write whose drawing is what it read (2.8: "copies the preview as it was"): the entry is **copied** when it is one to
    /// copy; and when another tool zipped it (its headers are not this writer's, so nothing of that package is copied, B1) its bytes are **read and written again**, stored, so that a
    /// file does not lose its picture to its first save. A preview that is no PNG, is larger than a preview may be, or does not inflate to what its entry states is left out, as a
    /// writer MAY (2.8): the save is made without it.
    fn previous_preview(&mut self) -> Preview {
        let Some(prior) = self.prior.as_mut() else { return Preview::None };
        let decoded = prior.decoded;
        let Some(record) = decoded.preview.as_ref() else { return Preview::None };
        if let Some(index) = prior.copyable(record, Wanted::Stored) {
            return Preview::Copy(index);
        }
        let Some(index) = prior.verified(record) else { return Preview::None };
        let size = prior.archive.entries().get(index).map_or(u64::MAX, |e| e.size);
        // A preview is a few kilobytes: when the memory cannot spare it either, the save goes on without one.
        if size > big(PREVIEW_MAX) || self.meter.take(size).is_err() {
            return Preview::None;
        }
        // **Into a buffer of the size that was charged**, made once and fallibly: a stream that holds more than the entry states (a crafted entry states 1 KB and inflates to what the
        // container's own limits allow) fails at the first byte past the buffer, and nothing is allocated that the meter did not see.
        let len = usize::try_from(size).unwrap_or(usize::MAX);
        let mut bytes: Vec<u8> = Vec::new();
        let filled = bytes.try_reserve_exact(len).is_ok() && {
            bytes.resize(len, 0);
            prior.archive.inflate_into(index, &mut bytes, &mut self.work).is_ok()
        };
        match filled {
            true if bytes.starts_with(&PNG_SIGNATURE) => {
                self.kept_preview = bytes;
                Preview::Reread
            }
            // Not a PNG, or bytes that do not inflate to what the entry states (a checksum that fails): the preview is left out, which 2.8 allows, and the save is made.
            _ => {
                self.meter.give(size);
                Preview::None
            }
        }
    }

    /// The rows of the manifest, in the order the entries are written.
    fn rows(&self, lists: &PartLists) -> Vec<Row> {
        let mut rows: Vec<Row> = Vec::new();
        rows.push(Row::known(DOCUMENT, role::DOCUMENT, true, None));
        rows.extend(lists.pages.iter().map(|(_, name)| Row::known(name, role::PAGE, true, None)));
        rows.extend(lists.masters.iter().map(|(_, name)| Row::known(name, role::MASTER, true, None)));
        rows.extend(lists.blobs.iter().map(|b| Row::known(&b.path, role::BLOB, false, Some(row_media_type(&b.media_type)))));
        rows.push(Row::known(STATE, role::STATE, false, None));
        if self.preview != Preview::None {
            rows.push(Row::known(PREVIEW, role::PREVIEW, false, None));
        }
        rows.extend(self.natives.iter().map(|n| n.row.clone()));
        rows
    }

    fn page(&mut self, name: &str, bytes: Vec<u8>) -> Result<(), NativeError> {
        let at = self.pages_done;
        self.pages_done += 1;
        let copy = self.page_copy.get(at).copied().flatten();
        let blobs = match copy {
            Some(index) => {
                self.copy(index, false)?;
                self.prior_blobs(at, true)
            }
            None => {
                let blobs = blobs_named_bytes(&bytes);
                self.add_entry(name, Input::Owned(bytes), Pack::Level(DEFLATE_LEVEL), false)?;
                blobs
            }
        };
        self.page_blobs.push(blobs);
        Ok(())
    }

    fn master(&mut self, name: &str, bytes: Vec<u8>) -> Result<(), NativeError> {
        let at = self.masters_done;
        self.masters_done += 1;
        let copy = self.master_copy.get(at).copied().flatten();
        let blobs = match copy {
            Some(index) => {
                self.copy(index, false)?;
                self.prior_blobs(at, false)
            }
            None => {
                let blobs = blobs_named_bytes(&bytes);
                self.add_entry(name, Input::Owned(bytes), Pack::Level(DEFLATE_LEVEL), false)?;
                blobs
            }
        };
        self.master_blobs.push(blobs);
        Ok(())
    }

    /// The blobs the part copied for the page (or master) at `at` names, as they were remembered.
    fn prior_blobs(&self, at: usize, page: bool) -> Option<Vec<u32>> {
        let prior = self.prior.as_ref()?;
        let object =
            if page { self.doc.pages.get(at).map(|p| Arc::as_ptr(p).addr()) } else { self.doc.masters.get(at).map(|m| Arc::as_ptr(m).addr()) }?;
        if page {
            prior.pages.get(&object).and_then(|j| prior.decoded.pages.get(*j)).and_then(|r| r.blobs.clone())
        } else {
            prior.masters.get(&object).and_then(|j| prior.decoded.masters.get(*j)).and_then(|r| r.blobs.clone())
        }
    }

    /// The entries after the parts: the blobs, the state, the preview and the parts of unknown roles.
    fn finish(&mut self) -> Result<(), NativeError> {
        let Some(lists) = self.lists.clone() else { return Err(NativeError::Unwritable("the codec handed over no document part")) };
        for rec in &lists.blobs {
            let bytes = self.table.0.borrow().arc(rec.id.0).cloned().ok_or(NativeError::Unwritable("a blob the document part lists is not held"))?;
            // The method this writer picks for the bytes, which is the method a copy must be in.
            let wanted = if stored(&bytes) { Wanted::Stored } else { Wanted::Deflated };
            let copy = self.prior.as_ref().and_then(|p| {
                let held = p.decoded.blobs.get(&rec.id.0)?;
                (Arc::ptr_eq(&held.bytes, &bytes) && held.entry.name == rec.path).then(|| p.copyable(&held.entry, wanted)).flatten()
            });
            match copy {
                Some(index) => self.copy(index, true)?,
                None if stored(&bytes) => self.add_entry(&rec.path, Input::Shared(Arc::clone(&bytes)), Pack::Stored, true)?,
                None => self.add_entry(&rec.path, Input::Shared(Arc::clone(&bytes)), Pack::Level(DEFLATE_LEVEL), true)?,
            }
        }
        let state = std::mem::take(&mut self.state);
        self.add_xml(STATE, &state, true)?;
        match self.preview {
            Preview::None => {}
            Preview::Given => {
                let options = self.options;
                if let Some(bytes) = &options.preview {
                    self.add_stored(PREVIEW, bytes, true)?;
                }
            }
            Preview::Copy(index) => self.copy(index, false)?,
            Preview::Reread => {
                let bytes = std::mem::take(&mut self.kept_preview);
                let len = big(bytes.len());
                let done = self.add_stored(PREVIEW, &bytes, false);
                drop(bytes);
                self.meter.give(len);
                done?;
            }
        }
        for k in 0..self.natives.len() {
            let Some(native) = self.natives.get(k) else { continue };
            let (path, bytes) = (native.row.path.clone(), Arc::clone(&native.bytes));
            // A part is copied when it is the part that was read and its headers are this writer's; written afresh, **it keeps the method it had** (2.2), which only the
            // previous package knows, and the writer's own rule is for a part that has none.
            let (copy, had) = self.prior.as_ref().map_or((None, None), |p| match p.extras.get(path.as_str()) {
                Some(held) if Arc::ptr_eq(&held.bytes, &bytes) => (p.copyable(&held.entry, Wanted::Any), p.method_of(&held.entry)),
                _ => (None, None),
            });
            match copy {
                Some(index) => self.copy(index, true)?,
                None => match had.unwrap_or(if stored(&bytes) { Method::Stored } else { Method::Deflated }) {
                    Method::Stored => self.add_entry(&path, Input::Shared(Arc::clone(&bytes)), Pack::Stored, true)?,
                    Method::Deflated => self.add_entry(&path, Input::Shared(Arc::clone(&bytes)), Pack::Level(DEFLATE_LEVEL), true)?,
                },
            }
        }
        Ok(())
    }
}

/// The record of the page or master at `address` that is written under `name`, **when it is as it was read**: its model object is the one decoded, it was held in an entry of that
/// name that the previous package still holds as it was read, its root `id` is the one the list gives, and every blob it names is still the very allocation it was, at the same
/// number. Whoever zipped the package, and in whatever method: that is a question of the entry's bytes and not of its headers, which only a copy as it is asks of ([`Prior::copyable`]).
fn unchanged_record<'r, T>(
    prior: &Prior<'_>,
    by_address: &HashMap<usize, usize>,
    records: &'r [PartRecord<T>],
    (address, id): (usize, u32),
    name: &str,
    table: &BlobTable,
) -> Option<&'r PartRecord<T>> {
    let record = records.get(*by_address.get(&address)?)?;
    // The root of the part names its `id`, which the list gives: a part is copied only under the `id` it was read with.
    if record.entry.name != name || record.id != id {
        return None;
    }
    prior.verified(&record.entry)?;
    for id in record.blobs.as_ref()? {
        let held = prior.decoded.blobs.get(id)?;
        if !Arc::ptr_eq(&held.bytes, table.arc(*id)?) {
            return None;
        }
    }
    Some(record)
}

/// The media type a manifest row says of a blob: the record's own when a row can hold it (at most 1,024 bytes, and no character XML forbids), and `application/octet-stream`
/// otherwise. The row is informative (2.4): a reader finds a blob by the record of the document part, which holds the media type of the first use whatever it is, so that a
/// model whose picture says a very long type, or one with a control character in it, is written and comes back as it was.
fn row_media_type(media_type: &str) -> &str {
    if media_type.len() <= WORD_MAX && diacraft_container::xml::first_bad_char(media_type).is_none() {
        media_type
    } else {
        "application/octet-stream"
    }
}

/// The text of `written-by`: cut to 256 bytes, with each character XML forbids replaced.
fn written_by(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let c = if diacraft_container::xml::xml_char(c) && c != '\r' && c != '\n' && c != '\t' { c } else { '\u{FFFD}' };
        if out.len() + c.len_utf8() > WRITTEN_BY_MAX {
            break;
        }
        out.push(c);
    }
    out
}

/// The default role word of a part that holds none of its own (a part made by a reader of another format): the spec's word for the role.
fn role_word(role: PartRole) -> &'static str {
    match role {
        PartRole::Thumbnail => "thumbnail",
        PartRole::MacroProject => "macro-project",
        PartRole::MacroSignature => "macro-signature",
        PartRole::MacroData => "macro-data",
        PartRole::Embedding => "embedding",
        PartRole::ObjectFile => "object-file",
        PartRole::ObjectReplacement => "object-replacement",
        PartRole::Signature => "signature",
        PartRole::CustomXml => "custom-xml",
        PartRole::Font => "font",
        PartRole::Other => "other",
    }
}

/// The part of unknown role as the package writes it, or why it cannot be.
fn native_of(part: &Part, limits: &Limits) -> Result<Native, NativeError> {
    let name: &str = &part.name;
    if !valid_entry_name(name, limits.container.max_part_name_bytes) {
        return Err(NativeError::Unwritable("a part the drawing holds has a name that no entry of a package may have"));
    }
    if big(part.bytes.len()) > limits.container.max_part_bytes {
        return Err(NativeError::TooLarge("a part the drawing holds is larger than an entry of a package may be"));
    }
    let word = part.manifest.get("role").unwrap_or_else(|| role_word(part.role));
    // The words of the row are what a reader accepts of one: short, and made of what XML can hold.
    let words = [word, &part.media_type];
    if words.iter().any(|w| w.len() > WORD_MAX || diacraft_container::xml::first_bad_char(w).is_some()) {
        return Err(NativeError::Unwritable("a part the drawing holds has a role or a media type that a manifest row cannot hold"));
    }
    // A role this version reads as itself would be read as that, and the entry would mean what it does not (a blob that no record names is the one exception: it is
    // what a later version's blob is, and it is handled as it was read).
    if role::is_known(word) && word != role::BLOB {
        return Err(NativeError::Unwritable("a part the drawing holds has the role of a part this version reads as its own"));
    }
    let extra: Vec<(String, String)> = part
        .manifest
        .iter()
        .filter(|(k, _)| !matches!(*k, "role" | "path" | "required" | "media-type"))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    let media_type = (!part.media_type.is_empty()).then(|| part.media_type.to_string());
    Ok(Native { row: Row { path: name.to_owned(), role: word.to_owned(), required: false, media_type, extra }, bytes: Arc::clone(&part.bytes) })
}

/// Writes `doc` as a `.diacraft` package. See the [module](self) for what is written, in what order, and what is copied from `previous`.
///
/// `state` is the ledger and content stamp the drawing's session holds (2.7): the ledger is written raised to the highest shape `id` of each page, and no row for a page
/// the drawing lacks. A drawing that holds parts of a newer version's roles (parts of [`Format::Native`]) writes each at its own path with its manifest row (5.2).
pub fn write(doc: &Document, state: &State, previous: Option<&Previous<'_>>, options: &WriteOptions) -> Result<Written, NativeError> {
    write_threads(doc, state, previous, options, 1)
}

/// [`write`] **on up to `threads` threads**: the entries are deflated on workers while the next part is made, and written in the order they were given; the codec makes the
/// parts of its first pass together and holds the ones that fit for its second. **The package is the same bytes at any number of threads**, and so is every refusal. 1 is the
/// single-threaded write, which makes no thread.
pub fn write_threads(
    doc: &Document,
    state: &State,
    previous: Option<&Previous<'_>>,
    options: &WriteOptions,
    threads: usize,
) -> Result<Written, NativeError> {
    if threads <= 1 {
        return write_in(doc, state, previous, options, None, 1);
    }
    // The thread that makes the parts is busy: the lane has one worker fewer than the count.
    par::lane(threads.saturating_sub(1).max(1), |lane: &Lane<'_, '_, Done>| write_in(doc, state, previous, options, Some(lane), threads))
}

/// The most bytes of parts that the codec's first pass may hold for its second (half of it) and make together (the other half), as a part of what the meter has: none unless the meter has a gibibyte, so that a
/// save that is within a few percent of what the process may hold is the save it always was.
pub(crate) fn keep_of(left: u64) -> u64 {
    if left >= 1 << 30 { (left / 8).min(384 << 20) } else { 0 }
}

fn write_in<'s, 'e>(
    doc: &Document,
    state: &State,
    previous: Option<&Previous<'_>>,
    options: &WriteOptions,
    pipe: Option<Pipe<'_, 's, 'e>>,
    threads: usize,
) -> Result<Written, NativeError> {
    let limits = &options.limits;
    if let Some(preview) = &options.preview {
        if !preview.starts_with(&PNG_SIGNATURE) {
            return Err(NativeError::Unwritable("the preview is not a PNG picture"));
        }
        if preview.len() > PREVIEW_MAX {
            return Err(NativeError::TooLarge("the preview picture is larger than a preview may be"));
        }
    }
    let mut natives: Vec<Native> = Vec::new();
    for part in doc.parts.iter().filter(|p| p.format == Format::Native) {
        natives.push(native_of(part, limits)?);
    }
    // The parts of unknown roles are written at their own paths and not as `part` elements of the document part (3.14): the codec never sees them.
    let working: Cow<'_, Document> = if natives.is_empty() {
        Cow::Borrowed(doc)
    } else {
        let mut copy = doc.clone();
        copy.parts = SharedVec::from_vec(doc.parts.iter().filter(|p| p.format != Format::Native).cloned().collect());
        Cow::Owned(copy)
    };
    let mut meter = Meter { left: limits.max_memory };
    let mut work = Work::default();
    let prior = open_prior(previous, limits, &mut meter, &mut work)?;
    let page_ids: Vec<u32> = doc.pages.iter().map(|p| p.id).collect();
    let state_bytes = state::write(state, &page_ids, &highest_ids(doc))?;
    let table = SharedTable(Rc::new(RefCell::new(BlobTable::new())));
    let mut emitter = Emitter {
        writer: ArchiveWriter::new().tight(),
        pipe,
        meter,
        options,
        doc: &working,
        table: table.clone(),
        natives,
        state: state_bytes,
        prior,
        lists: None,
        page_copy: Vec::new(),
        master_copy: Vec::new(),
        pages_done: 0,
        masters_done: 0,
        page_blobs: Vec::new(),
        master_blobs: Vec::new(),
        preview: Preview::None,
        kept_preview: Vec::new(),
        work,
        document_stamp: None,
        copied: 0,
        inflated: 0,
        ahead: 0,
        failure: None,
    };
    let mut sink = table;
    // What the codec may hold of parts beside the one in hand is taken from the meter for as long as it holds it.
    let keep = if pipe.is_some() { keep_of(emitter.meter.left) } else { 0 };
    let keep = if keep > 0 && emitter.meter.take(keep).is_ok() { keep } else { 0 };
    let saved = save_parts_owned(&working, &mut sink, &limits.container, SaveThreads { threads, keep }, &mut |part| emitter.emit(part));
    emitter.meter.give(keep);
    if let Some(failure) = emitter.failure.take() {
        return Err(failure);
    }
    saved?;
    emitter.finish()?;
    emitter.drain()?;
    let (copied, lists, page_blobs, master_blobs) =
        (emitter.copied, emitter.lists.clone(), std::mem::take(&mut emitter.page_blobs), std::mem::take(&mut emitter.master_blobs));
    let natives = std::mem::take(&mut emitter.natives);
    let preview = emitter.preview;
    let document_stamp = emitter.document_stamp;
    let bytes = emitter.writer.finish()?;
    if big(bytes.len()) > limits.max_file_bytes {
        return Err(NativeError::TooLarge("the package would be larger than a DiaCraft file may be"));
    }
    let lists = lists.ok_or(NativeError::Unwritable("the codec handed over no document part"))?;
    let made = Made { document_stamp, lists: &lists, named: (page_blobs, master_blobs), natives: &natives, preview: preview != Preview::None };
    let decoded = remember(&bytes, &working, &sink.0.borrow(), made, limits)?;
    Ok(Written { bytes, decoded, copied })
}

/// The previous package's directory, when there is one that is whole: a package that cannot be read is no package to copy from, and the write goes on without it.
fn open_prior<'p>(previous: Option<&Previous<'p>>, limits: &Limits, meter: &mut Meter, work: &mut Work) -> Result<Option<Prior<'p>>, NativeError> {
    let Some(previous) = previous else { return Ok(None) };
    let len = big(previous.bytes.len());
    if len > limits.max_file_bytes {
        return Ok(None);
    }
    meter.take(directory_bytes(previous.bytes, &limits.container))?;
    let mut judge = NativeJudge::new(&limits.container, part_cap(&limits.container));
    let Ok(archive) = Archive::read_with(previous.bytes, &limits.container, &mut judge, work) else { return Ok(None) };
    let index: HashMap<String, usize> = archive.entries().iter().enumerate().map(|(i, e)| (e.name.clone(), i)).collect();
    let extras: HashMap<&str, &ExtraRecord> = previous.decoded.extras.iter().map(|e| (e.entry.name.as_str(), e)).collect();
    Ok(Some(Prior {
        bytes: previous.bytes,
        index,
        pages: addresses(&previous.decoded.pages),
        masters: addresses(&previous.decoded.masters),
        decoded: previous.decoded,
        extras,
        archive,
    }))
}

/// The blob numbers each part names, in the order of the parts: `None` for a part that names more than a package has blobs.
type Named = Vec<Option<Vec<u32>>>;

/// What a write made, besides the bytes: the lists, the blobs the parts named, the parts of unknown roles, and whether a preview was written.
struct Made<'a> {
    document_stamp: Option<Stamp>,
    lists: &'a PartLists,
    named: (Named, Named),
    natives: &'a [Native],
    preview: bool,
}

/// What the package just written says of itself, for the next write to copy from: read from its own directory.
fn remember(bytes: &[u8], doc: &Document, table: &BlobTable, made: Made<'_>, limits: &Limits) -> Result<Decoded, NativeError> {
    let Made { document_stamp, lists, named, natives, preview } = made;
    let mut judge = NativeJudge::new(&limits.container, part_cap(&limits.container));
    let archive = Archive::read_with(bytes, &limits.container, &mut judge, &mut Work::default())?;
    let entries: HashMap<&str, EntryRecord> =
        archive.entries().iter().map(|e| (e.name.as_str(), EntryRecord { name: e.name.clone(), crc: e.crc, size: e.size })).collect();
    let entry = |name: &str| entries.get(name).cloned().ok_or(NativeError::Unwritable("an entry the package should hold is not in its directory"));
    let mut out = Decoded { document: Some(entry(DOCUMENT)?), document_stamp, ..Decoded::default() };
    for ((page, (id, name)), blobs) in doc.pages.iter().zip(&lists.pages).zip(named.0) {
        out.pages.push(PartRecord { id: *id, entry: entry(name)?, object: Arc::clone(page), blobs });
    }
    for ((master, (id, name)), blobs) in doc.masters.iter().zip(&lists.masters).zip(named.1) {
        out.masters.push(PartRecord { id: *id, entry: entry(name)?, object: Arc::clone(master), blobs });
    }
    for rec in &lists.blobs {
        if let Some(held) = table.arc(rec.id.0) {
            out.blobs.insert(rec.id.0, BlobRecord { entry: entry(&rec.path)?, bytes: Arc::clone(held) });
        }
    }
    for native in natives {
        out.extras.push(ExtraRecord { entry: entry(&native.row.path)?, bytes: Arc::clone(&native.bytes) });
    }
    if preview {
        out.preview = Some(entry(PREVIEW)?);
    }
    Ok(out)
}
