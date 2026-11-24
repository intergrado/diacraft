//! Reading a `.diacraft` package into the model (the public spec's sections 2, 3.16 and 6; the plan's).
//!
//! **Every byte is hostile, and one budget covers the open** (6.2): the file's length is checked before anything is read, the entries and their sizes by the archive's
//! directory before anything is inflated, the sizes the blob records state before a blob is fetched, the items of every part before it is decoded, and the model it builds
//! by the codec's [`Budget`], each pool of which is sized from [`Limits::max_memory`] ([`pools`]). **A part at a time**: the manifest, the document part, each page part
//! and each master part is inflated, decoded and let go before the next is asked for ([`PartSource`]); **a page or a master part is given to the codec as its bytes and read as a stream of
//! the children of its root**, so that what is held at once is the document part's tree and the bytes of the part in hand and one child's tree: a page of any
//! number of shapes is read, and what bounds a child is the codec's (4 million items, of which what is beyond a tree's 2 million is held back from the model's pool).
//! **Never an abort**: a size that does not fit is refused in words, and memory that runs out is the container's `out_of_memory`.
//!
//! **What is refused, and what is only lost.** A file is damaged (2.10) when its ZIP structure breaks the rules, its `mimetype` or manifest is not as the format says,
//! an entry is unlisted, listed twice or missing, the document part or a part its lists name is missing or breaks the vocabulary, or a limit is passed. **Damage confined
//! to blobs does not refuse the file**: a blob entry that is missing, whose checksum fails, or whose size or stamp is not what its record states is a lost blob, and the
//! picture it showed is a picture that could not be loaded, with one note in words that counts them and names none. Nothing of a blob is ever parsed or opened: the bytes
//! of a macro project or an embedded package are inflated once, to their own size, and held as bytes.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use diacraft_container::ContainerError;
use diacraft_container::Work;
use diacraft_container::xml::{El, XmlLimits, parse_counted};
use diacraft_container::zip::Archive;
use diacraft_container::zip::IntoJob;
use diacraft_container::zip::Method;
use diacraft_doc::{Document, Format, Part, PartRole, RawAttrs};
use diacraft_sheetxml::model::{
    BlobId, BlobSource, Budget, CodecError, DecodeNote, PartLists, PartSource, Stamp, decode_from, part_lists, stamp_of_bytes,
};

use crate::decoded::{BlobRecord, Decoded, EntryRecord, ExtraRecord, PartRecord, Print, blobs_named_bytes, items_of, print_as_current};
use crate::error::NativeError;
use crate::kind::{FORMAT, Kind, Sniffed, sniff_bytes};
use crate::limits::{Limits, Pools, pools};
use crate::manifest::{self, Manifest, Row};
use crate::names::{DOCUMENT, MANIFEST, MIMETYPE, NativeJudge, PREVIEW, STATE, role};
use crate::state::{self, State, highest_ids};

/// What a read lost or did not understand, for the note a person is shown. Each is a count, and **none names anything from the file** (2.10, 5.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadNote {
    /// Pictures whose bytes are lost: the shapes show none.
    PictureLost {
        /// How many (one for each blob).
        count: u64,
    },
    /// Themes whose bytes are lost: each is left out.
    ThemeLost {
        /// How many.
        count: u64,
    },
    /// Parts of the drawing held as bytes whose bytes are lost: each is left out.
    PartLost {
        /// How many.
        count: u64,
    },
    /// Elements and attributes of a newer version that the model had no place to keep: not read, and not written again.
    Unknown {
        /// How many.
        count: u64,
    },
    /// Entries of the package that no record names and that were left out, since the file's minor version is not a newer one.
    UnusedBlob {
        /// How many.
        count: u64,
    },
}

impl fmt::Display for ReadNote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (count, one, many) = match self {
            ReadNote::PictureLost { count } => (*count, "picture could not be loaded", "pictures could not be loaded"),
            ReadNote::ThemeLost { count } => {
                (*count, "theme was left out because its bytes are lost", "themes were left out because their bytes are lost")
            }
            ReadNote::PartLost { count } => {
                (*count, "part of the file was left out because its bytes are lost", "parts of the file were left out because their bytes are lost")
            }
            ReadNote::Unknown { count } => (
                *count,
                "item that a newer DiaCraft added was not read and is not kept",
                "items that a newer DiaCraft added were not read and are not kept",
            ),
            ReadNote::UnusedBlob { count } => {
                (*count, "entry that nothing in the file uses was left out", "entries that nothing in the file uses were left out")
            }
        };
        if count == 1 { write!(f, "1 {one}") } else { write!(f, "{count} {many}") }
    }
}

/// What a read made besides the model: the kind and version of the file, its state, what a write can copy from, what was lost, and what the read cost.
#[derive(Clone, Debug)]
pub struct Opened {
    /// The kind the file's `mimetype` and manifest agree on.
    pub kind: Kind,
    /// The version the manifest states, `major.minor`: the major is 1, and the minor may be higher than [`FORMAT`]'s.
    pub format: (u32, u32),
    /// The ledger and the content stamp (2.7), with every mark raised to the highest shape `id` of its page.
    pub state: State,
    /// What a write copies from: give it back to [`write`](crate::write) with the bytes it was made from.
    pub decoded: Decoded,
    /// What was lost or left out, each counted once and naming nothing.
    pub notes: Vec<ReadNote>,
    /// How many parts of a role this version does not know are kept as bytes in the model (5.2).
    pub kept_parts: usize,
    /// What the read cost, in the container's units.
    pub work: Work,
    /// How many blobs threads inflated before the decode asked for them: none at one thread. Nothing else of the read depends on it.
    pub blobs_ahead: usize,
}

/// The state a read of the package shares between the part source and the blob source: the archive, the cost, the limits of a parse, and the first refusal
/// either of them met (a source cannot return a [`NativeError`], so it keeps it here, and the read returns it in place of the codec's).
struct Package<'a> {
    archive: Archive<'a>,
    work: Work,
    xml: XmlLimits,
    part_cap: u64,
    parsed_items: u64,
    items_cap: u64,
    index: HashMap<String, usize>,
    failure: Option<NativeError>,
    /// The stamp of the bytes of the document part as they were inflated, taken when it was read (a write compares it with the part it would make, beside the checksum).
    document_stamp: Option<Stamp>,
    /// What those bytes are with the earlier namespace read as the current one, when they hold it.
    document_as_current: Option<Print>,
}

impl Package<'_> {
    /// Keeps the first refusal, and gives the codec's own word for it.
    fn fail(&mut self, e: NativeError, part: &str) -> CodecError {
        if self.failure.is_none() {
            self.failure = Some(e);
        }
        CodecError::Malformed { part: part.to_owned(), why: "a part the package could not give" }
    }

    /// The tree of the XML part held by the entry named `name`: inflated to its stated size (at most the part cap), parsed at the limits of one tree, and the bytes let
    /// go. **The items of every tree read are summed** and the read stops at the first that passes what an open may read: a drawing of many pages is time, and
    /// each tree is bounded as the budget is, but the sum is what bounds the time.
    fn tree(&mut self, name: &str) -> Result<El, NativeError> {
        self.tree_of(name, false)
    }

    /// [`Package::tree`], and `stamped` takes the stamp of the part's bytes first (the document part's).
    fn tree_of(&mut self, name: &str, stamped: bool) -> Result<El, NativeError> {
        let index = *self.index.get(name).ok_or_else(|| NativeError::damaged("part_missing"))?;
        let size = self.archive.entries().get(index).map(|e| e.size).ok_or_else(|| NativeError::damaged("part_missing"))?;
        if size > self.part_cap {
            return Err(NativeError::TooLarge("a part of the drawing is larger than the memory of this open leaves room for"));
        }
        let bytes = self.archive.inflate(index, &mut self.work)?;
        if stamped {
            self.document_stamp = Some(stamp_of_bytes(&bytes));
            self.document_as_current = print_as_current(&bytes);
        }
        let parsed = parse_counted(&bytes, &self.xml, &mut self.work);
        drop(bytes);
        let tree = parsed.map_err(|error| ContainerError::Xml { part: name.chars().take(64).collect(), error })?;
        self.parsed_items = self.parsed_items.saturating_add(items_of(&tree));
        if self.parsed_items > self.items_cap {
            return Err(CodecError::Limit { what: "items in the drawing", max: self.items_cap }.into());
        }
        Ok(tree)
    }

    /// The inflated bytes of the XML part held by the entry named `name`, to be read **as a stream of the children of its root** by the codec (no tree of a
    /// page or a master part is made, so that a page of any number of shapes is read, and the items of each child, of the part and of the drawing are bounded by the codec's own
    /// limits and budget). Inflated to its stated size, at most the part cap.
    fn bytes(&mut self, name: &str) -> Result<Vec<u8>, NativeError> {
        let index = *self.index.get(name).ok_or_else(|| NativeError::damaged("part_missing"))?;
        let size = self.archive.entries().get(index).map(|e| e.size).ok_or_else(|| NativeError::damaged("part_missing"))?;
        if size > self.part_cap {
            return Err(NativeError::TooLarge("a part of the drawing is larger than the memory of this open leaves room for"));
        }
        Ok(self.archive.inflate(index, &mut self.work)?)
    }

    /// The bytes of the entry at `index`, **made once**: in an allocation of the size the entry states, that [`Archive::inflate_into`] fills. A `Vec` that is converted to an
    /// `Arc<[u8]>` holds the entry twice while it is copied. **The size is bounded before this is called**: the archive checked every entry's size against the ratio, the part cap and
    /// the package total when it was opened (`Archive::read_with`), and the pool was charged for it (the blob records' sum, or `charge_blob` of an unknown part), so no size
    /// an entry states can ask for more than those allow. And **an allocation that cannot be had is a refusal and no abort**: see [`room_for`].
    fn inflate_arc(&mut self, index: usize) -> Result<Arc<[u8]>, ContainerError> {
        let size = self.archive.entries().get(index).map(|e| e.size).unwrap_or(0);
        let len = usize::try_from(size).map_err(|_| ContainerError::OutOfMemory { what: "a part of the package".to_owned() })?;
        room_for(len)?;
        // One allocation, of exactly `len` bytes (a run of a known length collected into an `Arc` is made in place), zeroed and then filled.
        let mut bytes: Arc<[u8]> = std::iter::repeat_n(0u8, len).collect();
        let Some(buffer) = Arc::get_mut(&mut bytes) else {
            return Err(ContainerError::ZipCorrupt { detail: "a buffer that was just made is shared".to_owned() });
        };
        self.archive.inflate_into(index, buffer, &mut self.work)?;
        Ok(bytes)
    }
}

/// What an `Arc<[u8]>` takes besides its bytes: its two counts, and a margin for the allocator's rounding.
const ARC_HEADER_BYTES: usize = 64;

/// Whether the allocation of an `Arc<[u8]>` of `len` bytes can be had, **asked of the allocator before the allocation that cannot say no is made**. Stable Rust has no way to make
/// such an `Arc` that can fail (`Arc::try_new_zeroed_slice` is not stable, and its result becomes bytes only through `unsafe`, which this workspace forbids), and an `Arc` made from a
/// `Vec` copies it, which holds the bytes twice (M1). So a `Vec` of the size is reserved with `try_reserve_exact`, which answers `Err` where the address space or the memory is short,
/// and is let go at once; the `Arc` is made right after it, of the same size. Another thread could take the room between the two (the allocation then aborts, as it always did): a
/// window of a few instructions, where the whole read was exposed before.
fn room_for(len: usize) -> Result<(), ContainerError> {
    let out_of_memory = || ContainerError::OutOfMemory { what: "a part of the package".to_owned() };
    let wanted = len.checked_add(ARC_HEADER_BYTES).ok_or_else(out_of_memory)?;
    let mut probe: Vec<u8> = Vec::new();
    probe.try_reserve_exact(wanted).map_err(|_| out_of_memory())?;
    // The block is made and not used: `black_box` keeps the compiler from taking the pair of allocation and release out, as it may for an unused one, which would answer
    // `Ok` for room that was never asked for.
    std::hint::black_box(&probe);
    Ok(())
}

/// The parts of a drawing, asked for one at a time by the decode, and the blobs each names. **A page or a master part is given as its bytes**, which the codec reads as a stream of
/// the children of the part's root, one child's tree at a time: the part is never a tree, and the one held is the bytes of the part in hand.
struct Parts<'c, 'a> {
    package: &'c RefCell<Package<'a>>,
    pages: Vec<Option<Vec<u32>>>,
    masters: Vec<Option<Vec<u32>>>,
    /// The bytes of the part in hand, let go when the next is asked for.
    held: Vec<u8>,
}

impl Parts<'_, '_> {
    /// The part named `name`, inflated: its bytes are kept to be given, and the blob numbers it names are found by a scan of the text, which the part's bytes are dropped after if
    /// they were a text of their own (UTF-16).
    fn one(&mut self, name: &str) -> Result<Option<Vec<u32>>, CodecError> {
        self.held = Vec::new();
        let mut package = self.package.borrow_mut();
        match package.bytes(name) {
            Ok(bytes) => {
                let named = blobs_named_bytes(&bytes);
                self.held = bytes;
                Ok(named)
            }
            Err(e) => Err(package.fail(e, "part")),
        }
    }

    /// What a source that has only bytes says to a request for a tree: never asked, since the bytes are given.
    fn no_tree() -> CodecError {
        CodecError::Malformed { part: "part".to_owned(), why: "a part is read as bytes, not as a tree" }
    }
}

impl PartSource for Parts<'_, '_> {
    type Tree = El;

    fn page(&mut self, _index: usize, _name: &str) -> Result<El, CodecError> {
        Err(Self::no_tree())
    }

    fn master(&mut self, _index: usize, _name: &str) -> Result<El, CodecError> {
        Err(Self::no_tree())
    }

    fn page_bytes(&mut self, _index: usize, name: &str) -> Result<Option<Cow<'_, [u8]>>, CodecError> {
        let named = self.one(name)?;
        self.pages.push(named);
        Ok(Some(Cow::Borrowed(self.held.as_slice())))
    }

    fn master_bytes(&mut self, _index: usize, name: &str) -> Result<Option<Cow<'_, [u8]>>, CodecError> {
        let named = self.one(name)?;
        self.masters.push(named);
        Ok(Some(Cow::Borrowed(self.held.as_slice())))
    }
}

/// What the document part says of a blob and where its entry is: `None` for an entry that is missing, or is no blob of the manifest's.
#[derive(Clone, Copy)]
struct Record {
    entry: Option<usize>,
    size: u64,
}

/// The blobs of a package, fetched when the decode asks for one: an entry is inflated only when it is named, and only when the size it states is the record's. A blob
/// that is missing, whose checksum fails or whose size is not the record's is lost (`None`); one that passes a limit or runs out of memory is a refusal, kept in the
/// package for the read to return.
struct Blobs<'c, 'a> {
    package: &'c RefCell<Package<'a>>,
    records: HashMap<u32, Record>,
    loaded: RefCell<HashMap<u32, Arc<[u8]>>>,
    /// The blobs that threads inflated before the decode asked for them, by number: what each came to, to be given when it is asked for, **as if it had been inflated then**.
    prefetched: RefCell<HashMap<u32, Prefetched>>,
    /// How many blobs were inflated ahead.
    ahead: usize,
}

/// A blob inflated ahead of the decode's request: the buffer filled or the error, and what it cost. The package counts neither until the blob is asked for.
struct Prefetched {
    result: Result<Arc<[u8]>, ContainerError>,
    work: Work,
}

/// The smallest blob worth a thread: a smaller one is inflated in less time than it takes to hand over, and **a blob that is stored is copied at the speed of memory** and is never prefetched.
const PREFETCH_MIN_BYTES: u64 = 64 * 1024;

impl Blobs<'_, '_> {
    /// Inflates the blobs the decode will ask for, **on threads, before it asks**: each into the buffer it is made into when asked for (`Package::inflate_arc`: exactly the size the
    /// directory states, which the pool was charged for with the sum of the records' before anything was inflated, so what the threads hold is what the pool was charged for and no more). The
    /// blobs are given to the decode when it asks for them as if inflated then: **their cost is counted, their errors are met, and the package total counts them, in the order the decode asks**, so the
    /// document, the notes and the refusal are those of the read that inflates each when it is asked for. A blob that no record names once, that is small, or that no buffer can be made for is left
    /// to be inflated when asked for. A blob that is never asked for is let go with the read and costs nothing that is counted.
    fn prefetch(&self, threads: usize) -> usize {
        let package = self.package.borrow_mut();
        let mut named: HashMap<usize, Vec<u32>> = HashMap::new();
        for (id, record) in &self.records {
            // The size the directory states is the record's (the check `get` makes), and the blob is worth a thread.
            let Some(index) = record.entry else { continue };
            let deflated = package.archive.entries().get(index).is_some_and(|e| e.compression() == Some(Method::Deflated));
            if deflated && record.size >= PREFETCH_MIN_BYTES && package.archive.entries().get(index).map(|e| e.size) == Some(record.size) {
                named.entry(index).or_default().push(*id);
            }
        }
        // In the order of the archive; an entry that two records name is read when asked for, so that each record's buffer is its own.
        let mut wanted: Vec<(usize, u32)> = named.into_iter().filter_map(|(index, ids)| (ids.len() == 1).then(|| (index, ids[0]))).collect();
        wanted.sort_unstable();
        let (mut jobs, mut ids) = (Vec::new(), Vec::new());
        for (index, id) in wanted {
            let Some(len) = package.archive.entries().get(index).and_then(|e| usize::try_from(e.size).ok()) else { continue };
            if room_for(len).is_err() {
                // The memory is short: the blobs from here on are inflated when asked for, and the refusal, if there is one, is theirs.
                break;
            }
            let buffer: Arc<[u8]> = std::iter::repeat_n(0u8, len).collect();
            jobs.push(IntoJob { index, buffer });
            ids.push(id);
        }
        if jobs.len() < 2 {
            return 0;
        }
        let done = package.archive.inflate_into_many(jobs, threads);
        let mut prefetched = self.prefetched.borrow_mut();
        for (id, done) in ids.into_iter().zip(done) {
            prefetched.insert(id, Prefetched { result: done.result, work: done.work });
        }
        prefetched.len()
    }
}

impl BlobSource for Blobs<'_, '_> {
    fn get(&self, id: BlobId) -> Option<Arc<[u8]>> {
        if let Some(held) = self.loaded.borrow().get(&id.0) {
            return Some(Arc::clone(held));
        }
        let record = self.records.get(&id.0)?;
        let index = record.entry?;
        let mut package = self.package.borrow_mut();
        // The size the directory states, which the archive checked against what inflates: when it is not what the record says, the entry is not this blob. Nothing is
        // inflated for it, so a record that understates cannot make the read hold more than the pool was charged.
        if package.archive.entries().get(index).map(|e| e.size) != Some(record.size) {
            return None;
        }
        // The blob a thread inflated for this request: what it cost is counted now, and the package counts it read now (`inflate_into` counts a read that passed), as the read of it now would.
        let inflated = match self.prefetched.borrow_mut().remove(&id.0) {
            Some(done) => {
                package.work.inflated = package.work.inflated.saturating_add(done.work.inflated);
                if done.result.is_ok() {
                    package.archive.count_read(index);
                }
                done.result
            }
            None => package.inflate_arc(index),
        };
        match inflated {
            Ok(held) => {
                self.loaded.borrow_mut().insert(id.0, Arc::clone(&held));
                Some(held)
            }
            // A checksum or a size that is not the entry's own: a lost blob.
            Err(ContainerError::ZipCorrupt { .. }) => None,
            Err(other) => {
                if package.failure.is_none() {
                    package.failure = Some(NativeError::Container(other));
                }
                None
            }
        }
    }
}

/// The entry record of `index` in the archive: its name, checksum and size as the archive states them (and the read verified).
fn entry_record(archive: &Archive<'_>, index: usize) -> Option<EntryRecord> {
    archive.entries().get(index).map(|e| EntryRecord { name: e.name.clone(), crc: e.crc, size: e.size })
}

/// Reads a `.diacraft` package from `bytes` within `limits`: the model, and what the read learned of the file (the kind and version, the state, what was lost, and what a
/// write copies from). A file that is no `.diacraft` file is [`NativeError::NotNative`]; a major version this build does not know is refused in words that name it;
/// damage and the limits are refused in words (see the module's documentation).
pub fn read(bytes: &[u8], limits: &Limits) -> Result<(Document, Opened), NativeError> {
    let file_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if file_len > limits.max_file_bytes {
        return Err(NativeError::TooLarge("the file is larger than a DiaCraft file may be"));
    }
    let sniffed_kind = match sniff_bytes(bytes) {
        Sniffed::Kind(kind) => kind,
        Sniffed::UnknownKind => return Err(NativeError::damaged("kind_unknown")),
        Sniffed::No => return Err(NativeError::NotNative),
    };
    let pools = pools(limits, bytes)?;
    let mut work = Work::default();
    let mut judge = NativeJudge::new(&limits.container, pools.part_bytes);
    let archive = Archive::read_with(bytes, &limits.container, &mut judge, &mut work)?;
    let index: HashMap<String, usize> = archive.entries().iter().enumerate().map(|(i, e)| (e.name.clone(), i)).collect();
    let xml = manifest::xml_limits(&limits.container, pools.tree_items);
    let mut package = Package {
        archive,
        work,
        xml,
        part_cap: pools.part_bytes,
        parsed_items: 0,
        items_cap: pools.items,
        index,
        failure: None,
        document_stamp: None,
        document_as_current: None,
    };
    let mut budget = pools.budget(&limits.container);
    let manifest = read_manifest(&mut package, sniffed_kind, &mut budget, limits)?;
    let mut plan = Plan::of(&manifest, &package, &mut budget)?;
    // The document part is charged by the decode, element by element, as every part is: only the manifest and the state are no part of the model.
    let document_tree = package.tree_of(DOCUMENT, true)?;
    let (document_stamp, document_as_current) = (package.document_stamp, package.document_as_current);
    let lists = part_lists(&document_tree)?;
    plan.check_lists(&lists)?;
    let records = plan.records(&lists, &package, &pools)?;
    let package = RefCell::new(package);
    let mut blobs = Blobs { package: &package, records, loaded: RefCell::new(HashMap::new()), prefetched: RefCell::new(HashMap::new()), ahead: 0 };
    let threads = if cfg!(target_arch = "wasm32") { 1 } else { diacraft_container::spare::threads_beside(limits.threads, limits.max_memory) };
    if threads > 1 {
        blobs.ahead = blobs.prefetch(threads);
    }
    let mut parts = Parts { package: &package, pages: Vec::new(), masters: Vec::new(), held: Vec::new() };
    let mut notes: Vec<DecodeNote> = Vec::new();
    let decoded = decode_from(&document_tree, &mut parts, &blobs, &mut budget, &mut notes);
    drop(document_tree);
    // A part or a blob the sources could not give is the reason the codec gives for refusing: it is the refusal.
    let failure = package.borrow_mut().failure.take();
    let mut doc = match (decoded, failure) {
        (_, Some(failure)) => return Err(failure),
        (Err(e), None) => return Err(e.into()),
        (Ok(doc), None) => doc,
    };
    // **A blob that is lost, of any kind, is let go here.** A blob whose size or stamp is not its record's is dropped by the codec, which notes it by its number when it is a picture
    // and by its name when it is a theme or a part (`PictureLost`, `ThemeLost`, `PartLost`), so the notes cannot tell which of the blobs fetched is the lost one. The bytes tell: every
    // place in the codec that keeps a blob keeps the allocation the source gave it, so a blob the model holds is held by the model and the source both, and one that only the source
    // holds is one the codec threw away. It is freed before the parts of unknown roles are read, and a later save has nothing of it to copy (`Decoded.blobs`, below).
    blobs.loaded.borrow_mut().retain(|_, bytes| Arc::strong_count(bytes) > 1);
    let mut package = package.borrow_mut();
    let (extras, lost_extras) = plan.read_extras(&mut package, &mut budget)?;
    let kept_parts = extras.len();
    let mut kept: Vec<ExtraRecord> = Vec::with_capacity(extras.len());
    for extra in extras {
        // The role word and the other attributes of the row stay with the part, so that a save writes the row as it was (5.2).
        let row = std::iter::once(("role", extra.row.role.as_str())).chain(extra.row.extra.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        let part = Part {
            format: Format::Native,
            name: Arc::from(extra.row.path.as_str()),
            media_type: Arc::from(extra.row.media_type.as_deref().unwrap_or("")),
            role: PartRole::Other,
            manifest: RawAttrs::from_pairs(row),
            rels: Arc::new(Vec::new()),
            bytes: Arc::clone(&extra.bytes),
        };
        doc.parts.push(Arc::new(part));
        kept.push(ExtraRecord { entry: extra.entry, bytes: extra.bytes });
    }
    doc.check_limits().map_err(|e| NativeError::Codec(CodecError::from(e)))?;
    let highest = highest_ids(&doc);
    let page_ids: HashSet<u32> = doc.pages.iter().map(|p| p.id).collect();
    let state = match plan.state {
        Some(index) => {
            let name = package.archive.entries().get(index).map(|e| e.name.clone()).ok_or_else(|| NativeError::damaged("state_malformed"))?;
            let tree = package.tree(&name)?;
            budget.charge_items(items_of(&tree))?;
            state::parse(&tree, &page_ids, &highest)?
        }
        // No state part is an empty state: and a mark is never below the highest shape `id` of its page.
        None => State { ledger: state::Ledger(highest.iter().filter(|(_, m)| **m > 0).map(|(p, m)| (*p, *m)).collect()), content_stamp: None },
    };
    // What a write copies from.
    let mut remembered = Decoded {
        document: entry_record(&package.archive, plan.document),
        document_stamp,
        document_as_current,
        preview: plan.preview.and_then(|i| entry_record(&package.archive, i)),
        extras: kept,
        ..Decoded::default()
    };
    for (k, (page, (id, name))) in doc.pages.iter().zip(&lists.pages).enumerate() {
        let entry = package.index.get(name).and_then(|i| entry_record(&package.archive, *i)).ok_or_else(|| NativeError::damaged("part_missing"))?;
        remembered.pages.push(PartRecord { id: *id, entry, object: Arc::clone(page), blobs: parts.pages.get(k).cloned().flatten() });
    }
    for (k, (master, (id, name))) in doc.masters.iter().zip(&lists.masters).enumerate() {
        let entry = package.index.get(name).and_then(|i| entry_record(&package.archive, *i)).ok_or_else(|| NativeError::damaged("part_missing"))?;
        remembered.masters.push(PartRecord { id: *id, entry, object: Arc::clone(master), blobs: parts.masters.get(k).cloned().flatten() });
    }
    // The blobs the model holds, as they were read: a lost blob is not among them (it was let go above).
    for rec in &lists.blobs {
        let held = blobs.loaded.borrow().get(&rec.id.0).cloned();
        let entry = blobs.records.get(&rec.id.0).and_then(|r| r.entry).and_then(|i| entry_record(&package.archive, i));
        if let (Some(bytes), Some(entry)) = (held, entry) {
            remembered.blobs.insert(rec.id.0, BlobRecord { entry, bytes });
        }
    }
    let opened = Opened {
        kind: manifest.kind,
        format: manifest.format,
        state,
        decoded: remembered,
        notes: read_notes(&notes, plan.unused_blobs, lost_extras),
        kept_parts,
        work: package.work,
        blobs_ahead: blobs.ahead,
    };
    Ok((doc, opened))
}

/// The manifest: found, inflated, parsed and charged to the budget; its kind is the `mimetype`'s; and the `mimetype` entry is the first, stored, with no extra field.
fn read_manifest(package: &mut Package<'_>, kind: Kind, budget: &mut Budget, limits: &Limits) -> Result<Manifest, NativeError> {
    // The media type: the first entry, stored and with no extra field, and what it holds is the kind's own media type (2.3).
    let first = package.index.get(MIMETYPE).copied().ok_or_else(|| NativeError::damaged("mimetype_bad"))?;
    let plain = package.archive.entries().get(first).is_some_and(|e| e.local_offset == 0 && e.is_plain_stored());
    if !plain {
        return Err(NativeError::damaged("mimetype_bad"));
    }
    let media = package.archive.inflate(first, &mut package.work)?;
    if media != kind.media_type().as_bytes() {
        return Err(NativeError::damaged("mimetype_bad"));
    }
    if !package.index.contains_key(MANIFEST) {
        return Err(NativeError::damaged("manifest_missing"));
    }
    let tree = package.tree(MANIFEST)?;
    budget.charge_items(items_of(&tree))?;
    let manifest = manifest::parse(&tree, limits.container.max_parts, budget)?;
    if manifest.kind != kind {
        return Err(NativeError::damaged("kind_mismatch"));
    }
    Ok(manifest)
}

/// An entry of a role this version does not know, read: its row (the manifest's own), its entry and its bytes, in the one allocation the model holds them in.
struct Extra<'m> {
    row: &'m Row,
    entry: EntryRecord,
    bytes: Arc<[u8]>,
}

/// What a row of the manifest costs the plan: its place in the table of rows and in the set of its role, and the entries it makes of the lists it builds.
const PLAN_ROW_BYTES: u64 = 160;

/// What the manifest says about the entries, checked against the archive: which entry is which part, and which hold bytes this version keeps. **It holds the rows of the
/// manifest by their place and not copies of them** (a row is a few strings; fifty thousand of them copied again are what the budget never saw).
struct Plan<'m> {
    manifest: &'m Manifest,
    /// The index of `document.xml`.
    document: usize,
    /// The index of `state.xml`, when there is one.
    state: Option<usize>,
    /// The index of `preview.png`, when there is one.
    preview: Option<usize>,
    /// The place of each row, by its path.
    rows: HashMap<&'m str, usize>,
    /// Names of the entries listed as pages and as masters.
    pages: HashSet<&'m str>,
    masters: HashSet<&'m str>,
    /// Rows of unknown roles, and (of a newer minor version) of blobs that no record names, in the order listed: the place of the row, and of its entry, which exists.
    extras: Vec<(usize, usize)>,
    /// Entries of the role `blob` that exist and that the document part will be checked to name, by the place of their row: filled by [`Plan::records`].
    blob_rows: Vec<(usize, usize)>,
    /// How many entries of the role `blob` no record named and that were left out.
    unused_blobs: u64,
    /// The file's minor version is higher than this build's: an entry no record names is kept.
    newer_minor: bool,
}

impl<'m> Plan<'m> {
    /// Checks the manifest against the archive (2.4): every entry other than `mimetype` and `manifest.xml` is listed exactly once, and every listed entry exists (a
    /// listed blob that is missing is a lost blob and no damage); a part of a role this version does not know that the file requires is refused naming the role; the
    /// document, state and preview are at their own names, and **an entry of those names has the role**. What the plan holds is charged to the budget first.
    fn of(manifest: &'m Manifest, package: &Package<'_>, budget: &mut Budget) -> Result<Plan<'m>, NativeError> {
        let rows_len = u64::try_from(manifest.rows.len()).unwrap_or(u64::MAX);
        budget.charge_bytes(rows_len.saturating_mul(PLAN_ROW_BYTES))?;
        let mut rows: HashMap<&str, usize> = HashMap::with_capacity(manifest.rows.len());
        for (place, row) in manifest.rows.iter().enumerate() {
            if row.path == MIMETYPE || row.path == MANIFEST {
                return Err(NativeError::damaged("manifest_row"));
            }
            if rows.insert(row.path.as_str(), place).is_some() {
                return Err(NativeError::damaged("entry_listed_twice"));
            }
        }
        // Every entry of the package is listed.
        for entry in package.archive.entries() {
            if entry.name != MIMETYPE && entry.name != MANIFEST && !rows.contains_key(entry.name.as_str()) {
                return Err(NativeError::damaged("entry_unlisted"));
            }
        }
        let (mut document, mut state, mut preview) = (None, None, None);
        let (mut pages, mut masters) = (HashSet::new(), HashSet::new());
        let mut extras: Vec<(usize, usize)> = Vec::new();
        let mut blob_rows = Vec::new();
        for (place, row) in manifest.rows.iter().enumerate() {
            // The entries that are the package's own have their roles, whatever else a row says of them.
            let own = match row.path.as_str() {
                DOCUMENT => Some(role::DOCUMENT),
                STATE => Some(role::STATE),
                PREVIEW => Some(role::PREVIEW),
                _ => None,
            };
            if own.is_some_and(|want| row.role != want) {
                return Err(NativeError::damaged("manifest_row"));
            }
            let at = package.index.get(row.path.as_str()).copied();
            let known = role::is_known(&row.role);
            if !known && row.required {
                return Err(NativeError::required_role(&row.role));
            }
            let Some(at) = at else {
                // A listed blob that is not there is a lost blob; anything else listed is missing.
                if row.role == role::BLOB {
                    continue;
                }
                return Err(NativeError::damaged("entry_missing"));
            };
            match row.role.as_str() {
                role::DOCUMENT => {
                    if row.path != DOCUMENT || document.replace(at).is_some() {
                        return Err(NativeError::damaged("manifest_row"));
                    }
                }
                role::STATE => {
                    if row.path != STATE || state.replace(at).is_some() {
                        return Err(NativeError::damaged("manifest_row"));
                    }
                }
                role::PREVIEW => {
                    if row.path != PREVIEW || preview.replace(at).is_some() {
                        return Err(NativeError::damaged("manifest_row"));
                    }
                }
                role::PAGE => {
                    pages.insert(row.path.as_str());
                }
                role::MASTER => {
                    masters.insert(row.path.as_str());
                }
                role::BLOB => blob_rows.push((place, at)),
                _ => extras.push((place, at)),
            }
        }
        let document = document.ok_or_else(|| NativeError::damaged("document_missing"))?;
        Ok(Plan {
            manifest,
            document,
            state,
            preview,
            rows,
            pages,
            masters,
            extras,
            blob_rows,
            unused_blobs: 0,
            newer_minor: manifest.format.1 > FORMAT.1,
        })
    }

    /// The lists of the document part against the manifest (3.3, 2.10): **each page and master part is named once**, by one list; each name is an entry of the role; and each page
    /// and master entry is named. The lists are exact.
    fn check_lists(&self, lists: &PartLists) -> Result<(), NativeError> {
        let mut seen: HashSet<&str> = HashSet::with_capacity(lists.pages.len().saturating_add(lists.masters.len()));
        for (_, name) in lists.pages.iter().chain(&lists.masters) {
            if !seen.insert(name.as_str()) {
                return Err(NativeError::damaged("part_listed_twice"));
            }
        }
        let named_pages: HashSet<&str> = lists.pages.iter().map(|(_, n)| n.as_str()).collect();
        let named_masters: HashSet<&str> = lists.masters.iter().map(|(_, n)| n.as_str()).collect();
        if named_pages.iter().any(|n| !self.pages.contains(*n)) || named_masters.iter().any(|n| !self.masters.contains(*n)) {
            return Err(NativeError::damaged("part_missing"));
        }
        if self.pages.iter().any(|n| !named_pages.contains(*n)) || self.masters.iter().any(|n| !named_masters.contains(*n)) {
            return Err(NativeError::damaged("part_unlisted"));
        }
        Ok(())
    }

    /// The records of the document part against the entries, and the check that the blobs they state fit the pool: **the sizes the records state are added before
    /// a blob is fetched** (3.16). An entry that holds a blob no record names is left out, or kept as an extra part when the file is of a newer minor version (2.6).
    fn records(&mut self, lists: &PartLists, package: &Package<'_>, pools: &Pools) -> Result<HashMap<u32, Record>, NativeError> {
        let mut records: HashMap<u32, Record> = HashMap::with_capacity(lists.blobs.len());
        let mut named: HashSet<&str> = HashSet::with_capacity(lists.blobs.len());
        let mut sum = 0u64;
        for rec in &lists.blobs {
            sum = sum.saturating_add(rec.size);
            // The entry a record names, when the manifest lists it as a blob.
            let entry = match self.rows.get(rec.path.as_str()).and_then(|place| self.manifest.rows.get(*place)) {
                Some(row) if row.role == role::BLOB => package.index.get(&rec.path).copied(),
                _ => None,
            };
            if entry.is_some() {
                named.insert(rec.path.as_str());
            }
            records.insert(rec.id.0, Record { entry, size: rec.size });
        }
        if sum > pools.blob_bytes {
            return Err(CodecError::Limit { what: "bytes of pictures and parts in the drawing", max: pools.blob_bytes }.into());
        }
        for (place, at) in std::mem::take(&mut self.blob_rows) {
            let Some(row) = self.manifest.rows.get(place) else { continue };
            if named.contains(row.path.as_str()) {
                continue;
            }
            if self.newer_minor {
                self.extras.push((place, at));
            } else {
                self.unused_blobs = self.unused_blobs.saturating_add(1);
            }
        }
        Ok(records)
    }

    /// The parts of a role this version does not know, read as bytes: charged to the blob pool by the size the entry states, before it is inflated (a package part is
    /// held in memory beside the blobs), and inflated once into the allocation the model holds. They are never parsed. **A blob of a newer minor that no record names and
    /// whose checksum fails is a lost blob** (2.10), left out and counted; any other part that fails its checksum is damage.
    fn read_extras(&self, package: &mut Package<'_>, budget: &mut Budget) -> Result<(Vec<Extra<'m>>, u64), NativeError> {
        let mut out = Vec::with_capacity(self.extras.len());
        let mut lost = 0u64;
        for (place, at) in &self.extras {
            let (Some(row), Some(entry)) = (self.manifest.rows.get(*place), entry_record(&package.archive, *at)) else { continue };
            budget.charge_blob(entry.size)?;
            budget.charge_bytes(u64::try_from(row.path.len().saturating_add(row.role.len())).unwrap_or(u64::MAX).saturating_add(256))?;
            match package.inflate_arc(*at) {
                Ok(bytes) => out.push(Extra { row, entry, bytes }),
                Err(ContainerError::ZipCorrupt { .. }) if row.role == role::BLOB => lost = lost.saturating_add(1),
                Err(e) => return Err(e.into()),
            }
        }
        Ok((out, lost))
    }
}

/// The notes of a read: what the codec counted, each kind once and in a fixed order, and the entries left out. A part lost is a part of the codec's and a blob of a newer minor
/// that failed its checksum.
fn read_notes(notes: &[DecodeNote], unused: u64, lost_extras: u64) -> Vec<ReadNote> {
    let (mut pictures, mut themes, mut parts, mut unknown) = (HashSet::new(), 0u64, lost_extras, 0u64);
    for note in notes {
        match note {
            DecodeNote::PictureLost { blob } => {
                pictures.insert(blob.0);
            }
            DecodeNote::ThemeLost { .. } => themes = themes.saturating_add(1),
            DecodeNote::PartLost { .. } => parts = parts.saturating_add(1),
            DecodeNote::Unknown { count } => unknown = unknown.saturating_add(*count),
        }
    }
    let mut out = Vec::new();
    let lost = u64::try_from(pictures.len()).unwrap_or(u64::MAX);
    if lost > 0 {
        out.push(ReadNote::PictureLost { count: lost });
    }
    if themes > 0 {
        out.push(ReadNote::ThemeLost { count: themes });
    }
    if parts > 0 {
        out.push(ReadNote::PartLost { count: parts });
    }
    if unknown > 0 {
        out.push(ReadNote::Unknown { count: unknown });
    }
    if unused > 0 {
        out.push(ReadNote::UnusedBlob { count: unused });
    }
    out
}
