//! The catalogue of faces a [`FontDb`](crate::FontDb) knows: what each is called and where its bytes are, read cheaply, with
//! the face itself parsed the first time it is wanted.
//!
//! A face comes from the binary (the optional craft-fonts faces and the bundled ones), from memory a caller added, or from a file
//! under a system font folder. Reading a folder costs a few kilobytes a file, since only the `name`, `OS/2` and `head` tables are
//! read to catalogue it, one read for each. Every size the files can make us read or allocate is capped here, and the folder scan
//! has a time budget of its own (see [`system`](crate::system)).
//!
//! **What is held in memory.** A file is read whole only when a face of it is wanted to draw with, once for all the faces of a
//! collection, and is then held for the life of the database; the most a database holds is [`MAX_HELD_BYTES`]. Looking for a face
//! that has some character (a fallback) never loads a file: each face's `cmap` table alone is read once, into a short list of
//! ranges ([`Coverage`]), and at most [`MAX_INDEX_BYTES`] of `cmap` tables are read in a database's life.
//!
//! The way of reading a face's names out of a font file without loading the file is adapted from DeckCraft's
//! `crates/fonts/src/fontdb.rs` (`docs/provenance.md`).

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Mutex, PoisonError};

use skrifa::Font;
#[cfg(not(target_arch = "wasm32"))]
use skrifa::Tag;
#[cfg(not(target_arch = "wasm32"))]
use skrifa::font::Blob;
use skrifa::font::Format;
use skrifa::raw::TableProvider;

use crate::face::{Face, FontBytes, contained, declared_style, face_names, face_upem, legacy_family};
use crate::standin::{GenericClass, class_of_name, key};

/// Where a face came from, which is also its priority when two faces answer to one name: the nearer the front, the sooner it is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    /// Added by the caller at run time, such as a face the document embeds.
    Added,
    /// The craft-fonts faces built into the binary, when `CRAFT_FONTS_DIR` was set at build time.
    Craft,
    /// Installed on the machine.
    System,
    /// Compiled into the binary always.
    Bundled,
}

/// The most faces one file or collection adds, and the most faces a database holds.
pub(crate) const MAX_FACES_PER_FILE: u32 = 64;
pub(crate) const MAX_ENTRIES: usize = 60_000;
/// The largest font file read for a face, in bytes (the biggest CJK collections are some hundreds of megabytes).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const MAX_FONT_FILE: u64 = 512 << 20;
/// The most bytes of font files a database holds in memory at once, whole, for faces it draws with. A file that would take it past
/// this is not loaded and the face is skipped; the text it would have drawn falls to another face.
pub(crate) const MAX_HELD_BYTES: u64 = 1 << 30;
/// The most bytes of `cmap` tables read in a database's life, to find which face has a character.
pub(crate) const MAX_INDEX_BYTES: u64 = 512 << 20;
/// The most ranges of code points a database keeps from all the `cmap` tables it has read (32 MiB of them): a hostile `cmap` can make
/// a few bytes into many ranges, and what is kept must be bounded by more than what is read.
pub(crate) const MAX_INDEX_RANGES: u64 = 4 << 20;
/// The largest `cmap` table read for one face.
#[cfg(not(target_arch = "wasm32"))]
const MAX_CMAP_TABLE: u32 = 8 << 20;
/// The most mappings read from one face's `cmap`, and the most ranges kept from them.
const MAX_INDEXED_MAPPINGS: usize = 0x11_0000 + 4096;
const MAX_COVERAGE_RANGES: usize = 16_384;
/// The largest `name` table read for cataloguing, the largest `OS/2` and `head`, and the most tables a font directory may claim.
#[cfg(not(target_arch = "wasm32"))]
const MAX_NAME_TABLE: u32 = 1 << 20;
#[cfg(not(target_arch = "wasm32"))]
const MAX_SMALL_TABLE: u32 = 1 << 12;
#[cfg(not(target_arch = "wasm32"))]
const MAX_TABLES: usize = 512;
/// The most bytes read from the disk to catalogue one file, whatever it claims: a collection of 64 faces that each name a megabyte.
#[cfg(not(target_arch = "wasm32"))]
const MAX_CATALOGUE_BYTES_PER_FILE: u64 = 16 << 20;

/// The two counters that bound what one database keeps, shared by all its entries.
#[derive(Debug)]
pub(crate) struct Budgets {
    held: AtomicU64,
    index: AtomicU64,
    ranges: AtomicU64,
    ranges_cap: u64,
    // Only a file has bytes to hold or tables to read, so on `wasm32` these two are never looked at.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    held_cap: u64,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    index_cap: u64,
}

impl Default for Budgets {
    fn default() -> Budgets {
        Budgets::with_caps(MAX_HELD_BYTES, MAX_INDEX_BYTES)
    }
}

impl Budgets {
    /// Add `n` to `counter` if the total stays within `cap`.
    fn take(counter: &AtomicU64, cap: u64, n: u64) -> bool {
        let mut now = counter.load(Ordering::Relaxed);
        loop {
            let Some(next) = now.checked_add(n).filter(|t| *t <= cap) else { return false };
            match counter.compare_exchange_weak(now, next, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => return true,
                Err(seen) => now = seen,
            }
        }
    }

    /// Keep `n` more ranges, if that stays within [`MAX_INDEX_RANGES`].
    fn take_ranges(&self, n: u64) -> bool {
        Budgets::take(&self.ranges, self.ranges_cap, n)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn give_back(counter: &AtomicU64, n: u64) {
        let mut now = counter.load(Ordering::Relaxed);
        while let Err(seen) = counter.compare_exchange_weak(now, now.saturating_sub(n), Ordering::Relaxed, Ordering::Relaxed) {
            now = seen;
        }
    }
}

impl Budgets {
    pub fn with_caps(held_cap: u64, index_cap: u64) -> Budgets {
        Budgets { held: AtomicU64::new(0), index: AtomicU64::new(0), ranges: AtomicU64::new(0), ranges_cap: MAX_INDEX_RANGES, held_cap, index_cap }
    }

    #[cfg(feature = "test-support")]
    pub fn with_ranges_cap(mut self, ranges_cap: u64) -> Budgets {
        self.ranges_cap = ranges_cap;
        self
    }

    /// Bytes of whole font files held so far.
    pub fn held(&self) -> u64 {
        self.held.load(Ordering::Relaxed)
    }

    /// Bytes of `cmap` tables read so far.
    pub fn indexed(&self) -> u64 {
        self.index.load(Ordering::Relaxed)
    }
}

/// A font file on disk that several faces may share: read whole the first time any of them is wanted, and the bytes then held once.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct FileCell {
    path: PathBuf,
    size: u64,
    state: Mutex<FileState>,
}

#[cfg(not(target_arch = "wasm32"))]
enum FileState {
    Unread,
    Held(Arc<Vec<u8>>),
    Failed,
}

/// What asking a [`FileCell`] for its bytes gave.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) enum FileRead {
    Bytes(Arc<Vec<u8>>),
    /// The file can't be read, or isn't a font file: decided once.
    Failed,
    /// Holding it would pass [`MAX_HELD_BYTES`]. Not decided: a later ask may find room.
    OverBudget,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileCell {
    pub fn new(path: PathBuf, size: u64) -> FileCell {
        FileCell { path, size, state: Mutex::new(FileState::Unread) }
    }

    pub fn bytes(&self, budgets: &Budgets) -> FileRead {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match &*state {
            FileState::Held(b) => return FileRead::Bytes(b.clone()),
            FileState::Failed => return FileRead::Failed,
            FileState::Unread => {}
        }
        if self.size > MAX_FONT_FILE {
            *state = FileState::Failed;
            return FileRead::Failed;
        }
        if !Budgets::take(&budgets.held, budgets.held_cap, self.size) {
            return FileRead::OverBudget;
        }
        match read_font_file(&self.path) {
            Some(b) => {
                // The file may have changed size since it was catalogued: what was read is what is held.
                let read = b.len() as u64;
                if read > self.size {
                    if !Budgets::take(&budgets.held, budgets.held_cap, read - self.size) {
                        Budgets::give_back(&budgets.held, self.size);
                        return FileRead::OverBudget;
                    }
                } else {
                    Budgets::give_back(&budgets.held, self.size - read);
                }
                let b = Arc::new(b);
                *state = FileState::Held(b.clone());
                FileRead::Bytes(b)
            }
            None => {
                Budgets::give_back(&budgets.held, self.size);
                *state = FileState::Failed;
                FileRead::Failed
            }
        }
    }
}

#[derive(Clone)]
pub(crate) enum Source {
    Static {
        bytes: &'static [u8],
        index: u32,
    },
    Shared {
        bytes: Arc<Vec<u8>>,
        index: u32,
    },
    /// A font file read when a face of it is first wanted to draw with. There are no files to read on `wasm32`.
    #[cfg(not(target_arch = "wasm32"))]
    File {
        file: Arc<FileCell>,
        index: u32,
        /// Where this face's table directory starts in the file: 0 for a plain font, a collection's offset for a member.
        dir_start: u32,
        /// The glyph count its `maxp` table states (at most 65,535), which the cmap alone has to be read against.
        glyphs: u16,
    },
}

/// What cataloguing learns about a face.
#[derive(Clone, Debug)]
pub(crate) struct Info {
    pub family: String,
    pub legacy: Option<String>,
    pub style: String,
    pub weight: u16,
    pub bold: bool,
    pub italic: bool,
    /// `OS/2` `ulUnicodeRange1` to `4`; all 0 when the face doesn't say.
    pub ranges: [u32; 4],
}

/// The code points a face maps, as sorted, disjoint, inclusive ranges: what a search for a fallback face reads without loading the
/// face. A font that maps more ranges than [`MAX_COVERAGE_RANGES`] has the rest left out.
#[derive(Debug, Default)]
pub(crate) struct Coverage {
    ranges: Vec<(u32, u32)>,
}

impl Coverage {
    pub fn contains(&self, c: char) -> bool {
        let cp = c as u32;
        let i = self.ranges.partition_point(|(lo, _)| *lo <= cp);
        i.checked_sub(1).and_then(|j| self.ranges.get(j)).is_some_and(|(_, hi)| *hi >= cp)
    }

    /// The ranges of the code points the font's `cmap` maps.
    fn of_font(font: &Font) -> Coverage {
        let mut cps: Vec<u32> = font.charmap().unicodes().take(MAX_INDEXED_MAPPINGS).map(|(cp, _)| cp).collect();
        cps.sort_unstable();
        cps.dedup();
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        for cp in cps {
            match ranges.last_mut() {
                Some((_, hi)) if hi.checked_add(1) == Some(cp) => *hi = cp,
                _ => {
                    if ranges.len() >= MAX_COVERAGE_RANGES {
                        break;
                    }
                    ranges.push((cp, cp));
                }
            }
        }
        Coverage { ranges }
    }
}

/// What asking an entry which characters it maps gave.
pub(crate) enum Cov<'a> {
    Known(&'a Coverage),
    /// The face's `cmap` can't be read: it covers nothing. Decided once.
    Unreadable,
    /// The database has read as many `cmap` bytes as it may. Not decided. (Only a file has `cmap` bytes to read.)
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    OverBudget,
}

pub(crate) struct Entry {
    pub keys: Vec<String>,
    pub info: Info,
    pub origin: Origin,
    pub source: Source,
    /// The class the family's name says it is, if it says.
    pub class: Option<GenericClass>,
    face: OnceLock<Option<Face>>,
    coverage: OnceLock<Option<Coverage>>,
}

impl Entry {
    pub fn new(info: Info, origin: Origin, source: Source) -> Entry {
        let mut keys = vec![key(&info.family)];
        keys.extend(info.legacy.as_deref().map(key));
        keys.retain(|k| !k.is_empty());
        keys.dedup();
        let class = keys.first().and_then(|k| class_of_name(k));
        Entry { keys, info, origin, source, class, face: OnceLock::new(), coverage: OnceLock::new() }
    }

    /// The face, parsed the first time it is asked for. `None` if the bytes can't be read or don't parse, which is decided once, or
    /// if the database already holds [`MAX_HELD_BYTES`] of files, which is not.
    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
    pub fn face(&self, budgets: &Budgets) -> Option<&Face> {
        if let Some(f) = self.face.get() {
            return f.as_ref();
        }
        let (bytes, index) = match &self.source {
            Source::Static { bytes, index } => (FontBytes::Static(bytes), *index),
            Source::Shared { bytes, index } => (FontBytes::Shared(bytes.clone()), *index),
            #[cfg(not(target_arch = "wasm32"))]
            Source::File { file, index, .. } => match file.bytes(budgets) {
                FileRead::Bytes(b) => (FontBytes::Shared(b), *index),
                FileRead::Failed => return self.face.get_or_init(|| None).as_ref(),
                FileRead::OverBudget => return None,
            },
        };
        self.face
            .get_or_init(|| {
                let face = Face::new(bytes, index);
                if face.is_none() {
                    log::warn!("font face {} {} could not be read and is skipped", self.info.family, self.info.style);
                }
                face
            })
            .as_ref()
    }

    /// Was this face tried and found unreadable, for good?
    pub fn face_failed(&self) -> bool {
        matches!(self.face.get(), Some(None))
    }

    /// Which code points the face maps, found from its `cmap` table alone the first time it is asked for.
    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
    pub fn coverage(&self, budgets: &Budgets) -> Cov<'_> {
        fn known(c: &Option<Coverage>) -> Cov<'_> {
            c.as_ref().map_or(Cov::Unreadable, Cov::Known)
        }
        if let Some(c) = self.coverage.get() {
            return known(c);
        }
        let built: Option<Coverage> = match &self.source {
            Source::Static { bytes, index } => coverage_of_bytes(FontBytes::Static(bytes), *index),
            Source::Shared { bytes, index } => coverage_of_bytes(FontBytes::Shared(bytes.clone()), *index),
            #[cfg(not(target_arch = "wasm32"))]
            Source::File { file, dir_start, glyphs, .. } => match file_cmap(&file.path, *dir_start, budgets) {
                CmapRead::Table(table) => {
                    // The reader limits a character map by the glyph count in `maxp` and takes a font with none for a font of no glyphs,
                    // which maps nothing. Only the `cmap` is read here, and the glyph count is the one the catalogue read from the font's
                    // own `maxp`, so a map is cut where it is cut when the whole font is read.
                    let tables = vec![(*b"cmap", table), (*b"maxp", maxp_of(*glyphs))];
                    contained(|| partial_font(tables).map(|f| Coverage::of_font(&f))).flatten()
                }
                CmapRead::Missing => None,
                CmapRead::OverBudget => return Cov::OverBudget,
            },
        };
        // What is kept is bounded as well as what is read.
        if let Some(c) = &built
            && !budgets.take_ranges(c.ranges.len() as u64)
        {
            return Cov::OverBudget;
        }
        known(self.coverage.get_or_init(|| built))
    }
}

/// The coverage of a face whose bytes are all in memory.
fn coverage_of_bytes(bytes: FontBytes, index: u32) -> Option<Coverage> {
    contained(|| Font::new(bytes.blob(), index).map(|f| Coverage::of_font(&f))).flatten()
}

/// A `maxp` table of version 0.5 that states `glyphs` glyphs: all the reader needs of one to limit a character map.
#[cfg(not(target_arch = "wasm32"))]
fn maxp_of(glyphs: u16) -> Vec<u8> {
    let [hi, lo] = glyphs.to_be_bytes();
    vec![0x00, 0x00, 0x50, 0x00, hi, lo]
}

/// A font file read whole, if it is a regular file of a size a font can have.
#[cfg(not(target_arch = "wasm32"))]
fn read_font_file(path: &std::path::Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() < 12 || meta.len() > MAX_FONT_FILE {
        return None;
    }
    std::fs::read(path).ok()
}

#[cfg(not(target_arch = "wasm32"))]
enum CmapRead {
    Table(Vec<u8>),
    /// No `cmap` in the face's directory, or one too big or past the end of the file.
    Missing,
    OverBudget,
}

/// Read a face's `cmap` table alone out of a file: the directory at `dir_start`, then the table, by bounded reads.
#[cfg(not(target_arch = "wasm32"))]
fn file_cmap(path: &std::path::Path, dir_start: u32, budgets: &Budgets) -> CmapRead {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else { return CmapRead::Missing };
    let Ok(meta) = file.metadata() else { return CmapRead::Missing };
    let size = meta.len();
    let mut read_at = |offset: u64, len: usize| -> Option<Vec<u8>> {
        if offset.checked_add(len as u64)? > size {
            return None;
        }
        let mut buf = vec![0; len];
        file.seek(SeekFrom::Start(offset)).ok()?;
        file.read_exact(&mut buf).ok()?;
        Some(buf)
    };
    let Some(count) = read_at(u64::from(dir_start), 12).and_then(|d| d.get(4..6).and_then(|b| b.try_into().ok()).map(u16::from_be_bytes)) else {
        return CmapRead::Missing;
    };
    let count = usize::from(count);
    if count == 0 || count > MAX_TABLES {
        return CmapRead::Missing;
    }
    let Some(records) = read_at(u64::from(dir_start) + 12, count * 16) else { return CmapRead::Missing };
    let found = records.as_chunks::<16>().0.iter().find(|rec| rec.get(0..4) == Some(b"cmap".as_slice()));
    let Some(rec) = found else { return CmapRead::Missing };
    let be32 = |at: usize| rec.get(at..at + 4).and_then(|s| s.try_into().ok()).map(u32::from_be_bytes);
    let (Some(offset), Some(len)) = (be32(8), be32(12)) else { return CmapRead::Missing };
    if len == 0 || len > MAX_CMAP_TABLE {
        return CmapRead::Missing;
    }
    if !Budgets::take(&budgets.index, budgets.index_cap, u64::from(len)) {
        return CmapRead::OverBudget;
    }
    match read_at(u64::from(offset), len as usize) {
        Some(t) => CmapRead::Table(t),
        None => CmapRead::Missing,
    }
}

/// What a parsed face says about itself, for the catalogue.
pub(crate) fn info_of(font: &Font) -> Option<Info> {
    let (family, style) = face_names(font)?;
    let legacy = legacy_family(font, &family);
    let (weight, italic) = declared_style(font, &style);
    let ranges =
        font.tables().os2().map_or([0; 4], |o| [o.ul_unicode_range_1(), o.ul_unicode_range_2(), o.ul_unicode_range_3(), o.ul_unicode_range_4()]);
    Some(Info { family, legacy, style, weight, bold: weight >= 600, italic, ranges })
}

/// Entries for the faces in `data`, a font or a collection, from the given origin. Faces whose tables don't parse are counted in
/// the second number and left out.
pub(crate) fn entries_from_bytes(data: FontBytes, origin: Origin) -> (Vec<Entry>, usize) {
    let (slice, shared): (&[u8], _) = match &data {
        FontBytes::Static(b) => (b, None),
        FontBytes::Shared(b) => (b, Some(b.clone())),
    };
    let indices = contained(|| face_indices(slice)).unwrap_or_default();
    if indices.is_empty() {
        return (Vec::new(), 1);
    }
    let mut rejected = 0;
    let mut out = Vec::new();
    for index in indices {
        let info = contained(|| {
            let font = Font::new(data.blob(), index)?;
            face_upem(&font)?;
            info_of(&font)
        });
        let info = info.flatten();
        let Some(info) = info else {
            rejected += 1;
            continue;
        };
        let source = match (&shared, &data) {
            (Some(bytes), _) => Source::Shared { bytes: bytes.clone(), index },
            (None, FontBytes::Static(bytes)) => Source::Static { bytes, index },
            (None, FontBytes::Shared(bytes)) => Source::Shared { bytes: bytes.clone(), index },
        };
        out.push(Entry::new(info, origin, source));
    }
    (out, rejected)
}

/// The indices of the distinct faces in a font file or collection, at most [`MAX_FACES_PER_FILE`]. A collection may list one
/// face many times, which counts once. Bytes that are neither a font nor a collection give none.
fn face_indices(data: &[u8]) -> Vec<u32> {
    match Format::new(data) {
        Some(Format::Sfnt(_)) if !data.starts_with(b"ttcf") => vec![0],
        Some(Format::Sfnt(count)) => {
            let mut seen = std::collections::HashSet::new();
            let mut out = Vec::new();
            for i in 0..count.min(MAX_FACES_PER_FILE * 4) {
                // Two entries that point at the same table directory are one face.
                let start = 12usize.saturating_add((i as usize).saturating_mul(4));
                let dir = data.get(start..start.saturating_add(4)).and_then(|b| b.try_into().ok()).map(u32::from_be_bytes);
                if let Some(dir) = dir
                    && seen.insert(dir)
                {
                    out.push(i);
                    if out.len() as u32 >= MAX_FACES_PER_FILE {
                        break;
                    }
                }
            }
            out
        }
        // Not a font, or one the cataloguer has no use for: a Type 1 font or a bare CFF font has no `head` table to give a face.
        Some(Format::Type1 | Format::Cff(_)) | None => Vec::new(),
    }
}

/// The reads made to catalogue one file: each range is read once, and the total is capped.
#[cfg(not(target_arch = "wasm32"))]
struct FileReads {
    file: std::fs::File,
    size: u64,
    cache: std::collections::HashMap<(u64, usize), Option<Vec<u8>>>,
    spent: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileReads {
    /// `len` bytes at `offset`, or `None` if the file is too short, the read fails or the file's byte budget is spent. A range read
    /// before is not read again: the faces of a collection share tables, and a hostile one may name the same megabyte many times.
    fn at(&mut self, offset: u64, len: usize) -> Option<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        if let Some(hit) = self.cache.get(&(offset, len)) {
            return hit.clone();
        }
        let got = (|| {
            if offset.checked_add(len as u64)? > self.size || self.spent.checked_add(len as u64)? > MAX_CATALOGUE_BYTES_PER_FILE {
                return None;
            }
            self.spent += len as u64;
            crate::work::file_bytes(len as u64);
            let mut buf = vec![0; len];
            self.file.seek(SeekFrom::Start(offset)).ok()?;
            self.file.read_exact(&mut buf).ok()?;
            Some(buf)
        })();
        if self.cache.len() < 4096 {
            self.cache.insert((offset, len), got.clone());
        }
        got
    }
}

/// Entries for the faces of one font file, catalogued from its `name`, `OS/2` and `head` tables alone. The file is not kept in
/// memory: the face is read from it when first wanted. At most [`MAX_CATALOGUE_BYTES_PER_FILE`] of it is read.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn entries_from_file(path: &std::path::Path, origin: Origin) -> Vec<Entry> {
    let Ok(file) = std::fs::File::open(path) else { return Vec::new() };
    let Ok(meta) = file.metadata() else { return Vec::new() };
    if !meta.is_file() || meta.len() < 12 || meta.len() > MAX_FONT_FILE {
        return Vec::new();
    }
    let size = meta.len();
    let mut reads = FileReads { file, size, cache: std::collections::HashMap::new(), spent: 0 };
    let be32 = |b: &[u8], at: usize| b.get(at..at + 4).and_then(|s| s.try_into().ok()).map(u32::from_be_bytes);
    let Some(head) = reads.at(0, 12) else { return Vec::new() };
    // Each face as (its index in the collection, where its table directory starts). A collection may list one directory many times:
    // it is one face, and keeps the first index.
    let starts: Vec<(u32, u32)> = if head.starts_with(b"ttcf") {
        let n = be32(&head, 8).unwrap_or(0).min(MAX_FACES_PER_FILE * 4) as usize;
        let listed: Vec<u32> = reads.at(12, n * 4).map(|b| b.as_chunks::<4>().0.iter().map(|c| u32::from_be_bytes(*c)).collect()).unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        let distinct = listed.iter().enumerate().filter(|(_, s)| seen.insert(**s));
        distinct.map(|(i, s)| (i as u32, *s)).take(MAX_FACES_PER_FILE as usize).collect()
    } else {
        vec![(0, 0)]
    };
    let cell = Arc::new(FileCell::new(path.to_path_buf(), size));
    let mut out = Vec::new();
    for (index, start) in starts {
        let Some((info, glyphs)) = face_info_from_file(&mut reads, start) else { continue };
        out.push(Entry::new(info, origin, Source::File { file: cell.clone(), index, dir_start: start, glyphs }));
    }
    out
}

/// The catalogue entry of the face whose table directory starts at `start`, and the glyph count its `maxp` states: a font of the `name`,
/// `OS/2`, `head` and `maxp` tables (and the `MAXP` of a font that has one) is made of what is read from the file, and is asked what a
/// face is asked ([`face_upem`]) before it is listed, so that a font is listed if and only if the same bytes would be a face given to
/// [`FontDb::add_font_data`](crate::FontDb::add_font_data). Only the first record of each of those tags is read.
#[cfg(not(target_arch = "wasm32"))]
fn face_info_from_file(reads: &mut FileReads, start: u32) -> Option<(Info, u16)> {
    let dir = reads.at(u64::from(start), 12)?;
    let count = usize::from(u16::from_be_bytes(dir.get(4..6)?.try_into().ok()?));
    if count == 0 || count > MAX_TABLES {
        return None;
    }
    let records = reads.at(u64::from(start) + 12, count * 16)?;
    let mut has_outlines = false;
    let mut wanted: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    for rec in records.as_chunks::<16>().0 {
        let tag: [u8; 4] = rec.get(0..4)?.try_into().ok()?;
        let offset = u32::from_be_bytes(rec.get(8..12)?.try_into().ok()?);
        let len = u32::from_be_bytes(rec.get(12..16)?.try_into().ok()?);
        has_outlines |= matches!(&tag, b"glyf" | b"CFF " | b"CFF2");
        let cap = match &tag {
            b"name" => MAX_NAME_TABLE,
            b"OS/2" | b"head" | b"maxp" | b"MAXP" => MAX_SMALL_TABLE,
            _ => continue,
        };
        // One read for each tag: a directory that lists a tag again is a lie, and its first record is the one that counts.
        if len <= cap && !wanted.iter().any(|(t, _)| *t == tag) {
            wanted.push((tag, reads.at(u64::from(offset), len as usize)?));
        }
    }
    if !has_outlines || !wanted.iter().any(|(t, _)| t == b"name") {
        return None;
    }
    contained(|| {
        let font = partial_font(wanted)?;
        face_upem(&font)?;
        let glyphs = u16::try_from(font.num_glyphs()).unwrap_or(u16::MAX);
        Some((info_of(&font)?, glyphs))
    })
    .flatten()
}

/// A font that has just these tables: what cataloguing has read of a file, which is not the file. The reader asks for a table by its tag
/// when something first reads it, and a table that is not here is a table the font does not have.
#[cfg(not(target_arch = "wasm32"))]
fn partial_font(tables: Vec<([u8; 4], Vec<u8>)>) -> Option<Font> {
    let tables: Vec<([u8; 4], Arc<Vec<u8>>)> = tables.into_iter().map(|(tag, data)| (tag, Arc::new(data))).collect();
    let lookup: Arc<dyn Fn(Tag) -> Option<Blob> + Send + Sync> =
        Arc::new(move |tag: Tag| tables.iter().find(|(t, _)| *t == tag.to_be_bytes()).map(|(_, data)| Blob::Shared(data.clone())));
    Font::new(lookup, 0)
}

/// The `OS/2` Unicode range bit that covers `c`, for the blocks whose bits are usually right. They are only a hint: 15 faces on
/// one machine hold Han with the bit clear, so a face that doesn't set the bit is searched after the ones that do, never left out.
pub(crate) fn range_bit(c: char) -> Option<u32> {
    let cp = c as u32;
    Some(match cp {
        0x0370..=0x03FF => 7,
        0x0400..=0x052F => 9,
        0x0590..=0x05FF => 11,
        0x0600..=0x06FF => 13,
        0x0700..=0x074F => 71,
        0x0780..=0x07BF => 72,
        0x0900..=0x097F => 15,
        0x0E00..=0x0E7F => 24,
        0x0E80..=0x0EFF => 25,
        0x0F00..=0x0FFF => 70,
        0x1000..=0x109F => 74,
        0x1780..=0x17FF => 80,
        0x1800..=0x18AF => 81,
        0x3040..=0x309F => 49,
        0x30A0..=0x30FF => 50,
        0x3100..=0x312F => 51,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF => 59,
        0xA000..=0xA4CF => 83,
        0xAC00..=0xD7AF => 56,
        _ => return None,
    })
}

/// How well a face's declared `OS/2` ranges say it has a glyph for `c`: the order a search for a fallback looks in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Hint {
    /// The face sets the bit for `c`'s block, or `c` is in no block with a bit.
    Declared,
    /// The face's ranges are all zero: it doesn't say.
    Silent,
    /// The face declares ranges and the block's bit is clear.
    Denied,
}

pub(crate) fn hint(ranges: &[u32; 4], c: char) -> Hint {
    if ranges.iter().all(|r| *r == 0) {
        return Hint::Silent;
    }
    let Some(bit) = range_bit(c) else { return Hint::Declared };
    if ranges.get((bit / 32) as usize).is_none_or(|word| word & (1 << (bit % 32)) != 0) { Hint::Declared } else { Hint::Denied }
}
