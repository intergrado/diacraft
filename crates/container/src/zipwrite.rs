//! Writing a ZIP archive, from entries in the order they should have (PKWARE's APPNOTE).
//!
//! [`ArchiveWriter`] writes each entry as it is added, so nothing is held twice, and the central directory and the end record at
//! [`ArchiveWriter::finish`]. **What it promises**, each of which a test reads back with this crate's reader:
//!
//! * **The method asked for, and only stored and deflate**. [`ArchiveWriter::add`] writes the method it is given;
//!   [`ArchiveWriter::add_smallest`] deflates and stores the entry instead when deflate does not make it smaller. A part over 4 MiB is
//!   deflated at the fast level, since a person waits for a save and the size of the file is not content.
//! * **Local and central headers agree**: name, flags, method, version, time, date, checksum and both sizes. No data descriptor, no
//!   extra field, no comment, no encryption. So the first entry, if it is stored, has its name at byte 30 and its bytes right after the
//!   name, of `mimetype`.
//! * **Folders and names as given.** [`ArchiveWriter::add_directory`] writes a folder entry (a name that ends in a slash, no data, the
//!   MS-DOS directory attribute), as an ODF package may hold one. A name is written as its UTF-8 bytes, spaces and all, with flag
//!   bit 11 set when it is not ASCII; a name that would leave the package (a leading slash, a backslash, a drive letter, a `..` segment),
//!   an empty one, a file's that ends in a slash and a folder's that does not are refused, and two entries of one name are refused at
//!   [`ArchiveWriter::finish`]. The writer holds no reading limits, so what it writes is what this crate's reader takes back *within the
//!   limits the reader is given*: under [`crate::Limits::default`], [`crate::zip::Plain`] refuses a name of more than 1,024 bytes (any up to
//!   65,489 is written: a row of 46 bytes and its name, extra field and comment may take 65,535, and a longer one is
//!   refused here as `invalid_part_name`) as `invalid_part_name`, and more than 50,000 files (up to 65,535 are written) as `too_many_parts`,
//!   and a reader that wants them back passes larger limits. A caller that must read its own output with the defaults keeps within them;
//!   names are not capped here but by what a header holds.
//! * **The same entries give the same bytes.** The time is the first of January 1980, midnight, the earliest a ZIP can hold (times
//!   are not content).
//! * **No ZIP64.** The format needs it past 4 GiB or 65,535 entries; this crate's limits read at most 50,000 parts and 1 GiB of them, so
//!   an archive the writer cannot hold could not be read back: `package_too_large` or `too_many_parts`, not a file of the wrong shape.
//!
//! **An entry copied as it is** ([`ArchiveWriter::copy`]: an entry a save did not change keeps its compressed
//! bytes, method, flags, extra fields, dates and order, and is not deflated again). The entry comes from an [`Archive`] this crate read, so
//! its row and its layout have been checked; the copy reads the row and the local header again from the file and keeps them **byte for
//! byte** ([`Copied::Exact`]: the local header, the data, the data descriptor when the entry has one, and the central row with its extra
//! field and comment, of which only the local header's offset is written anew) when they can be kept as they are, and a reader that walks
//! the copy from local header to local header, as a streaming reader does, reads each entry as its row says:
//!
//! * the row gives the local header's offset in its own 32-bit field, the one field a copy writes anew (not in a ZIP64 field), and says
//!   the entry is on disk 0;
//! * the local header has the row's name, method and flags;
//! * when its flags do not say a data descriptor follows the data (bit 3), it states the checksum and the two sizes as the row's own fields
//!   state them, and sizes it leaves to a ZIP64 field (0xFFFFFFFF in both headers) are the ones its own ZIP64 record gives (both sizes,
//!   APPNOTE 4.5.3) and the row gives;
//! * when they do, a descriptor that states the row's checksum and sizes follows the data **in the form the local header announces**
//!   (APPNOTE 4.3.9.2): with sizes of 8 bytes when the local header holds a ZIP64 record, of 4 bytes when it does
//!   not, with or without its signature (4.3.9.3); and it ends by the next entry's local header, or the directory. So a zero-length entry
//!   whose local header holds a ZIP64 record keeps its 24-byte descriptor, though its first 16 bytes also read as a short one that states
//!   zeros; and a descriptor of the other form is not kept.
//!
//! Otherwise ([`Copied::Rewritten`]; the central header wins) the data is still copied as it is, compressed, never
//! inflated, under headers written from the row: its versions, flags (but the data descriptor's bit), method, time, date, checksum, sizes,
//! name as written and attributes, the row's extra field and comment, and the local header's extra field, each extra field with its ZIP64
//! records left out (this writer writes no ZIP64, so the sizes and the offset are in the headers' own fields), and the local one's Unicode
//! path records too (the reader never reads them, and one that named nothing under the local name the file had could name a path once the
//! header holds the row's name; the row's own, which the reader checked, is kept), so that local and central headers agree. Either way the name as written is held to the rules of every name this writer writes, and the duplicate rule at
//! [`ArchiveWriter::finish`] holds over copied names too (by the name as written, decoded as the reader decodes it, and by the name the
//! archive kept the entry under). **What a copy does not keep**, since it writes a well-formed archive, and what
//! [`copy_account`] counts, by why, without writing anything ([`CopyAccount`], [`Dropped`]): bytes no header announces (before the first
//! entry, between two, after the last, and between the directory and the end records: data descriptors written where no flag says one
//! follows are such bytes, and so is a descriptor of a form its local header does not announce), the directory's bytes after its last row,
//! the archive's ZIP64 end records (the archive is closed by a plain end record, which counts what this writer can write), the archive's
//! comment, bytes after the end record, and the order of a central directory that is not the order of the file (entries are written in
//! the order they are copied). A copy of every entry in the order of the file is the file, byte for byte, exactly when the account says
//! nothing was rewritten, reordered or left out.

use std::io::Write;

use flate2::Compression;
use flate2::write::DeflateEncoder;

use crate::ContainerError;
use crate::par::{self, Failed, Lane};
use crate::work::Work;
use crate::zipdir::{self, FLAG_UTF8_NAME, ROW_OFFSET_FIELD};
use crate::zipread::{Archive, Entry, Method};

/// An entry over this many bytes is deflated at the fast level.
const FAST_ABOVE: usize = 4 * 1024 * 1024;

/// The most entries a plain end record counts.
const MAX_ITEMS: usize = 65_535;

/// ZIP's earliest date, 1980-01-01, in DOS format; the time is midnight.
const DOS_DATE: u16 = 0x0021;

/// The MS-DOS attribute byte's directory bit, in the external attributes of a folder's row (the rows are made by MS-DOS, version 0).
const DOS_DIRECTORY: u32 = 0x10;

/// The largest the archive may grow: offsets and sizes are 32 bits.
const MAX_ARCHIVE: u64 = u32::MAX as u64;

/// General purpose flag bit 3: the checksum and the sizes follow the data, in a data descriptor.
const FLAG_DESCRIPTOR: u16 = 0x0008;

/// The ZIP64 "look elsewhere" value of a 32-bit field.
const SENTINEL32: u32 = 0xFFFF_FFFF;

/// An entry already written: its row of the central directory, whole, and the names the duplicate rule knows it by.
struct Row {
    /// The row's bytes, as the directory will hold them.
    central: Vec<u8>,
    /// The entry's name as the reader will decode it, and, for a copy, the name its archive kept it under when that is another.
    names: Vec<Vec<u8>>,
}

/// How [`ArchiveWriter::copy`] wrote an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Copied {
    /// Every byte as it was read: the local header, the data, the data descriptor when there is one, and the central row, of which only the
    /// offset of the local header is written anew.
    Exact,
    /// The data as it was read, still compressed, under headers written anew from the central row (see the module documentation).
    Rewritten,
}

/// The fields of a local header and of a central row that this writer writes.
struct Header<'n> {
    made_by: u16,
    needed: u16,
    flags: u16,
    method: u16,
    time: u16,
    date: u16,
    crc: u32,
    compressed: u32,
    size: u32,
    name: &'n [u8],
    internal: u16,
    external: u32,
    /// The local header's extra field, the row's, and the row's comment: none for an entry this writer makes.
    local_extra: &'n [u8],
    central_extra: &'n [u8],
    comment: &'n [u8],
}

impl Header<'_> {
    /// Whether every variable field fits its 16-bit length, and the row the 65,535 bytes a row may take: what this
    /// crate's reader takes back.
    fn fits(&self) -> bool {
        let short = |part: &[u8]| u16::try_from(part.len()).is_ok();
        let row = 46usize.saturating_add(self.name.len()).saturating_add(self.central_extra.len()).saturating_add(self.comment.len());
        short(self.name) && short(self.local_extra) && short(self.central_extra) && short(self.comment) && row <= 65_535
    }

    /// The local header (APPNOTE 4.3.7). The lengths were checked by [`Header::fits`] before the entry was written.
    fn local(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
        out.extend_from_slice(&self.needed.to_le_bytes());
        out.extend_from_slice(&self.flags.to_le_bytes());
        out.extend_from_slice(&self.method.to_le_bytes());
        out.extend_from_slice(&self.time.to_le_bytes());
        out.extend_from_slice(&self.date.to_le_bytes());
        out.extend_from_slice(&self.crc.to_le_bytes());
        out.extend_from_slice(&self.compressed.to_le_bytes());
        out.extend_from_slice(&self.size.to_le_bytes());
        out.extend_from_slice(&(self.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&(self.local_extra.len() as u16).to_le_bytes());
        out.extend_from_slice(self.name);
        out.extend_from_slice(self.local_extra);
    }

    /// The central row (APPNOTE 4.3.12) for a local header at `offset`.
    fn central(&self, offset: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity(46 + self.name.len() + self.central_extra.len() + self.comment.len());
        out.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
        out.extend_from_slice(&self.made_by.to_le_bytes());
        out.extend_from_slice(&self.needed.to_le_bytes());
        out.extend_from_slice(&self.flags.to_le_bytes());
        out.extend_from_slice(&self.method.to_le_bytes());
        out.extend_from_slice(&self.time.to_le_bytes());
        out.extend_from_slice(&self.date.to_le_bytes());
        out.extend_from_slice(&self.crc.to_le_bytes());
        out.extend_from_slice(&self.compressed.to_le_bytes());
        out.extend_from_slice(&self.size.to_le_bytes());
        out.extend_from_slice(&(self.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&(self.central_extra.len() as u16).to_le_bytes());
        out.extend_from_slice(&(self.comment.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // disk
        out.extend_from_slice(&self.internal.to_le_bytes());
        out.extend_from_slice(&self.external.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(self.name);
        out.extend_from_slice(self.central_extra);
        out.extend_from_slice(self.comment);
        out
    }
}

fn too_large() -> ContainerError {
    ContainerError::PackageTooLarge { limit: MAX_ARCHIVE }
}

fn out_of_memory() -> ContainerError {
    ContainerError::OutOfMemory { what: "the archive being written".to_owned() }
}

fn corrupt(detail: &str) -> ContainerError {
    ContainerError::ZipCorrupt { detail: detail.to_owned() }
}

/// The name rules of every entry this writer writes, for `name` as it will be written (`raw_len` bytes of it).
fn check_name(name: &str, raw_len: usize, folder: bool) -> Result<(), ContainerError> {
    let bytes = name.as_bytes();
    let bare = name.strip_suffix('/').unwrap_or(name);
    let drive = matches!((bytes.first(), bytes.get(1)), (Some(c), Some(b':')) if c.is_ascii_alphabetic());
    if name.starts_with('/') || name.contains('\\') || drive || bare.split('/').any(|segment| segment == "..") {
        return Err(ContainerError::PathTraversal { name: name.to_owned() });
    }
    let invalid = |reason: &'static str| ContainerError::InvalidPartName { name: name.chars().take(64).collect(), reason };
    if bare.is_empty() {
        return Err(invalid("empty"));
    }
    if name.ends_with('/') != folder {
        return Err(invalid(if folder {
            "a folder's name that does not end in a slash"
        } else {
            "a file's name that ends in a slash, as a folder's does"
        }));
    }
    if u16::try_from(raw_len).is_err() {
        return Err(invalid("longer than the 65,535 bytes a ZIP name can be"));
    }
    Ok(())
}

/// An archive being written. See the [module documentation](self).
#[derive(Default)]
pub struct ArchiveWriter {
    out: Vec<u8>,
    rows: Vec<Row>,
    /// Whether the output grows by an eighth ([`ArchiveWriter::tight`]) and not by what a `Vec` does by itself.
    tight: bool,
}

impl ArchiveWriter {
    /// An archive with no entry yet.
    pub fn new() -> ArchiveWriter {
        ArchiveWriter::default()
    }

    /// How many entries have been written.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether no entry has been written.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Writes the file `name` with `data`, by `method` exactly: deflated even when that is larger, which a save that keeps an entry's
    /// method wants.
    pub fn add(&mut self, name: &str, data: &[u8], method: Method) -> Result<(), ContainerError> {
        self.check(name, data.len(), false)?;
        let body = match method {
            Method::Stored => Body::Stored(data),
            Method::Deflated => Body::Deflated(deflate(data)?),
        };
        self.put(name, crc32fast::hash(data), data.len(), &body, false)
    }

    /// Writes the file `name` with `data` deflated at `level` (0 to 9), **at that level whatever the size of the entry**, and as deflate even where that
    /// is no smaller (a package of the same model gives the same bytes, the design's 1.6). [`ArchiveWriter::add`] deflates an entry over
    /// 4 MiB at the fast level, which makes the bytes depend on a size; this does not. A level past 9 is refused as `unsupported_compression`, and
    /// nothing is written then, nor for a name this writer would refuse.
    pub fn add_at_level(&mut self, name: &str, data: &[u8], level: u32) -> Result<(), ContainerError> {
        level_is_deflate(name, level)?;
        self.check(name, data.len(), false)?;
        let body = Body::Deflated(deflate_at(data, Compression::new(level), usize::try_from(u32::MAX).unwrap_or(usize::MAX))?);
        self.put(name, crc32fast::hash(data), data.len(), &body, false)
    }

    /// Writes the file `name` with `data` deflated, or stored when deflate does not make it smaller (an empty entry is stored). Gives the
    /// method written.
    pub fn add_smallest(&mut self, name: &str, data: &[u8]) -> Result<Method, ContainerError> {
        self.check(name, data.len(), false)?;
        let body = if data.is_empty() {
            Body::Stored(data)
        } else {
            let bytes = deflate(data)?;
            if bytes.len() < data.len() { Body::Deflated(bytes) } else { Body::Stored(data) }
        };
        self.put(name, crc32fast::hash(data), data.len(), &body, false)?;
        Ok(body.method())
    }

    /// Writes the file `name` as [`pack`] made it (which a worker may have done): what [`ArchiveWriter::add`], [`ArchiveWriter::add_smallest`] or
    /// [`ArchiveWriter::add_at_level`] does for `how`, with the same refusals in the same order, and the compression already made. Gives the method written. **`data` is the entry's
    /// bytes, which are written as they are when the entry is stored** (and are not looked at when it is deflated, so that a caller that let them go when they were compressed passes none);
    /// bytes that are not the length the entry was packed from are `zip_corrupt`.
    pub fn add_packed(&mut self, name: &str, data: &[u8], how: Pack, packed: Result<Packed, ContainerError>) -> Result<Method, ContainerError> {
        if let Pack::Level(level) = how {
            level_is_deflate(name, level)?;
        }
        self.check(name, packed.as_ref().map_or(data.len(), |p| p.size), false)?;
        let packed = packed?;
        let method = packed.method();
        let body = match packed.body {
            PackedBody::Stored if data.len() == packed.size => Body::Stored(data),
            PackedBody::Stored => return Err(corrupt("the bytes of an entry to write are not the ones it was packed from")),
            PackedBody::Deflated(bytes) => Body::Deflated(bytes),
        };
        self.put(name, packed.crc, packed.size, &body, false)?;
        Ok(method)
    }

    /// Writes every entry of `items`, in order, as [`ArchiveWriter::add`], [`ArchiveWriter::add_smallest`] or [`ArchiveWriter::add_at_level`] writes it for its [`Pack`], **and on
    /// more than one thread when `workers` allows**: the compression of each entry (one stream, never cut into pieces) is made on a worker, and the entries are written in the order they
    /// were given, so **the archive is byte for byte the one that adding them one after another makes**, at any number of threads. Gives the method each was written with.
    ///
    /// **Memory**: the bytes of compression that are in the hands of workers, and of workers' results not written yet, are held to [`Workers::window`], each entry charged by its size
    /// ([`Workers`]) before it is given to a worker; an entry that is more than the window is made alone, when nothing else is in flight, as a single thread makes it. A worker
    /// that panics is `zip_corrupt` ("an entry could not be compressed"), in the place of its entry, and a name or a size the writer refuses is refused in its place as it is when one adds
    /// entry after entry: the entries before it are written, the error is the first of the sequence, and nothing after it is.
    pub fn add_many(&mut self, items: &[Add<'_>], workers: Workers) -> Result<Vec<Method>, ContainerError> {
        let mut methods: Vec<Method> = Vec::new();
        methods.try_reserve_exact(items.len()).map_err(|_| out_of_memory())?;
        let large = items.iter().filter(|item| item.data.len() >= PARALLEL_FROM).count();
        if workers.threads <= 1 || large < 2 {
            for item in items {
                methods.push(self.add_one(item)?);
            }
            return Ok(methods);
        }
        // Workers when there is more than one entry to make that is worth a worker's start; the main thread, which would wait, makes the small ones and writes.
        par::lane(workers.threads, |lane: &Lane<'_, '_, Result<Packed, ContainerError>>| -> Result<(), ContainerError> {
            let mut written = 0usize;
            for item in items {
                let charge = item.charge();
                while lane.pending() > 0 && lane.charged().saturating_add(charge) > workers.window {
                    self.write_next(lane, items, &mut written, &mut methods)?;
                }
                let item = *item;
                if item.data.len() < PARALLEL_FROM {
                    lane.submit_done(charge, pack(item.name, item.data, item.how));
                } else {
                    lane.submit(charge, move || pack(item.name, item.data, item.how));
                }
            }
            while written < items.len() {
                self.write_next(lane, items, &mut written, &mut methods)?;
            }
            Ok(())
        })?;
        Ok(methods)
    }

    /// Writes one entry the way a single thread does.
    fn add_one(&mut self, item: &Add<'_>) -> Result<Method, ContainerError> {
        match item.how {
            Pack::Stored => self.add(item.name, item.data, Method::Stored).map(|()| Method::Stored),
            Pack::Deflated => self.add(item.name, item.data, Method::Deflated).map(|()| Method::Deflated),
            Pack::Smallest => self.add_smallest(item.name, item.data),
            Pack::Level(level) => self.add_at_level(item.name, item.data, level).map(|()| Method::Deflated),
            #[cfg(test)]
            Pack::Panics => Err(corrupt("an entry that panics is added")),
        }
    }

    /// Takes the result of the oldest entry given to `lane` that is not written yet, and writes it.
    fn write_next(
        &mut self,
        lane: &Lane<'_, '_, Result<Packed, ContainerError>>,
        items: &[Add<'_>],
        written: &mut usize,
        methods: &mut Vec<Method>,
    ) -> Result<(), ContainerError> {
        let Some(result) = lane.take() else { return Ok(()) };
        let Some(item) = items.get(*written) else { return Err(corrupt("a result of a compression that has no entry")) };
        *written += 1;
        let packed = result.unwrap_or_else(|Failed| Err(corrupt("an entry could not be compressed: the compressor stopped")));
        methods.push(self.add_packed(item.name, item.data, item.how, packed)?);
        Ok(())
    }

    /// Writes the folder `name`, which ends in a slash (`Configurations2/`): stored, with no data.
    pub fn add_directory(&mut self, name: &str) -> Result<(), ContainerError> {
        self.check(name, 0, true)?;
        self.put(name, crc32fast::hash(&[]), 0, &Body::Stored(&[]), true)
    }

    /// Writes the entry at `index` of `archive` as it is: its compressed bytes, never inflated, and its headers byte for byte when they can
    /// be kept, else headers written anew from its row (see the module documentation). Gives which. An index past the archive's entries,
    /// and a row or local header that is not where the entry says, are `zip_corrupt`; a name this writer would not write is refused as
    /// [`ArchiveWriter::add`] refuses it; and nothing is written then.
    pub fn copy(&mut self, archive: &Archive<'_>, index: usize) -> Result<Copied, ContainerError> {
        self.copy_counted(archive, index, &mut Work::default())
    }

    /// [`ArchiveWriter::copy`], adding the records of the extra fields the copy walked to `work` ([`Work::local_extra_records`]: the local
    /// header's, looked through for its ZIP64 record, and, for an entry written under headers made anew, the local header's and the row's
    /// again). The count is made whether the entry is copied or refused.
    pub fn copy_counted(&mut self, archive: &Archive<'_>, index: usize, work: &mut Work) -> Result<Copied, ContainerError> {
        let Some(entry) = archive.entries().get(index) else { return Err(corrupt("the archive holds no such entry to copy")) };
        let source = archive.source();
        let plan = plan(archive, entry, work)?;
        let (row, local, data_end) = (&plan.row, &plan.local, plan.data_end);
        let written = zipdir::decode_written(row.name);
        check_name(&written, row.name.len(), entry.folder)?;
        let size = usize::try_from(entry.size).map_err(|_| too_large())?;
        self.check_room(size)?;
        let data = source.get(local.data..data_end).ok_or_else(|| corrupt("the data of an entry to copy is not in the file"))?;
        let mut names = vec![written.into_bytes()];
        if entry.name.as_bytes() != names.first().map(Vec::as_slice).unwrap_or_default() {
            names.push(entry.name.as_bytes().to_vec());
        }
        let offset = u32::try_from(self.out.len()).map_err(|_| too_large())?;
        if plan.exact {
            let bytes = source.get(entry.local_offset..plan.end()).ok_or_else(|| corrupt("an entry to copy is not in the file"))?;
            let mut central = row.bytes.to_vec();
            central.get_mut(ROW_OFFSET_FIELD..ROW_OFFSET_FIELD + 4).ok_or_else(|| corrupt("a row too short"))?.copy_from_slice(&offset.to_le_bytes());
            self.grow(bytes.len())?;
            self.out.extend_from_slice(bytes);
            self.rows.push(Row { central, names });
            return Ok(Copied::Exact);
        }
        let (local_extra, central_extra) = (zipdir::local_extra_kept(local.extra, work), zipdir::row_extra_kept(row.extra, work));
        let header = Header {
            made_by: entry.version_made_by,
            needed: entry.version_needed,
            flags: entry.flags & !FLAG_DESCRIPTOR,
            method: entry.method,
            time: entry.time,
            date: entry.date,
            crc: entry.crc,
            compressed: u32::try_from(data.len()).map_err(|_| too_large())?,
            size: u32::try_from(size).map_err(|_| too_large())?,
            name: row.name,
            internal: entry.internal_attributes,
            external: entry.external_attributes,
            local_extra: &local_extra,
            central_extra: &central_extra,
            comment: row.comment,
        };
        self.write(&header, data, names)?;
        Ok(Copied::Rewritten)
    }

    /// Writes the file at `index` of `archive` with `data` in place of what it holds, under the entry's own headers: its name as written, its
    /// method (stored stays stored and deflated stays deflated, even where that is larger, "a changed entry keeps its
    /// method"), versions, flags but the data descriptor's bit, date and time, attributes, extra fields (ZIP64 records, and a local header's
    /// Unicode path records, left out as for a copy) and comment; only the checksum and the sizes are those of `data`. So a save that changes
    /// one part of a package changes that part's bytes and sizes and no other field of its headers. A folder, an index past the archive's
    /// entries, a row or local header that is not where the entry says, and a name this writer would not write are refused as
    /// [`ArchiveWriter::copy`] refuses them, and nothing is written then. The records of the extra fields walked are added to `work`.
    pub fn replace(&mut self, archive: &Archive<'_>, index: usize, data: &[u8], work: &mut Work) -> Result<(), ContainerError> {
        let Some(entry) = archive.entries().get(index) else { return Err(corrupt("the archive holds no such entry to replace")) };
        if entry.folder {
            return Err(corrupt("a folder holds no data to replace"));
        }
        let method = entry.compression().ok_or_else(|| corrupt("an entry to replace is neither stored nor deflated"))?;
        let plan = plan(archive, entry, work)?;
        let row = &plan.row;
        let written = zipdir::decode_written(row.name);
        check_name(&written, row.name.len(), false)?;
        self.check_room(data.len())?;
        let mut names = vec![written.into_bytes()];
        if entry.name.as_bytes() != names.first().map(Vec::as_slice).unwrap_or_default() {
            names.push(entry.name.as_bytes().to_vec());
        }
        let body = match method {
            Method::Stored => Body::Stored(data),
            Method::Deflated => Body::Deflated(deflate(data)?),
        };
        let bytes = body.bytes();
        let (local_extra, central_extra) = (zipdir::local_extra_kept(plan.local.extra, work), zipdir::row_extra_kept(row.extra, work));
        let header = Header {
            made_by: entry.version_made_by,
            needed: entry.version_needed,
            flags: entry.flags & !FLAG_DESCRIPTOR,
            method: method.number(),
            time: entry.time,
            date: entry.date,
            crc: crc32fast::hash(data),
            compressed: u32::try_from(bytes.len()).map_err(|_| too_large())?,
            size: u32::try_from(data.len()).map_err(|_| too_large())?,
            name: row.name,
            internal: entry.internal_attributes,
            external: entry.external_attributes,
            local_extra: &local_extra,
            central_extra: &central_extra,
            comment: row.comment,
        };
        self.write(&header, bytes, names)
    }

    /// The name rules, the count and the size, before anything is compressed.
    fn check(&self, name: &str, size: usize, folder: bool) -> Result<(), ContainerError> {
        check_name(name, name.len(), folder)?;
        self.check_room(size)
    }

    /// Room for one more entry of `size` uncompressed bytes: under the count a plain end record holds, and under 4 GiB.
    fn check_room(&self, size: usize) -> Result<(), ContainerError> {
        if self.rows.len() >= MAX_ITEMS {
            return Err(ContainerError::TooManyParts { limit: MAX_ITEMS });
        }
        if u32::try_from(size).is_err() {
            return Err(too_large());
        }
        Ok(())
    }

    /// The archive, with `more` bytes and the directory still to come, stays under 4 GiB; and the memory for them is had.
    fn grow(&mut self, more: usize) -> Result<(), ContainerError> {
        let after = self.out.len().checked_add(more).ok_or_else(too_large)?;
        if u32::try_from(after).is_err() {
            return Err(too_large());
        }
        self.rows.try_reserve(1).map_err(|_| out_of_memory())?;
        self.reserve_out(more)
    }

    /// This writer, with an output that grows **by an eighth** of what it holds at least (and by exactly what is needed when the eighth is not to be had), the growth
    /// of the buffer the encoder deflates into, and not by what a `Vec` does by itself, which can reserve twice what is held: a package of 500 MB then reserves 500 MB
    /// and a little, not a gigabyte of address space. The bytes written are the same. A writer made by [`ArchiveWriter::new`] keeps the growth it always had,
    /// since a caller's measured memory (the `.odg` writer's) rests on it.
    #[must_use]
    pub fn tight(mut self) -> ArchiveWriter {
        self.tight = true;
        self
    }

    /// Room for `more` bytes in the output: the growth of the `Vec` itself, or an eighth at a time for a [`tight`](ArchiveWriter::tight) writer.
    fn reserve_out(&mut self, more: usize) -> Result<(), ContainerError> {
        if !self.tight {
            return self.out.try_reserve(more).map_err(|_| out_of_memory());
        }
        if self.out.capacity().saturating_sub(self.out.len()) >= more {
            return Ok(());
        }
        let step = (self.out.len() / 8).max(more);
        if self.out.try_reserve_exact(step).is_ok() {
            return Ok(());
        }
        self.out.try_reserve_exact(more).map_err(|_| out_of_memory())
    }

    /// How many bytes the archive holds so far: the local headers and the data of the entries written, and not the directory, which [`ArchiveWriter::finish`]
    /// adds. A caller that holds a budget of memory counts what each entry took by the difference.
    pub fn written_bytes(&self) -> usize {
        self.out.len()
    }

    /// Writes an entry this writer made: its local header and data, and its row for the directory.
    fn put(&mut self, name: &str, crc: u32, size: usize, body: &Body<'_>, folder: bool) -> Result<(), ContainerError> {
        let method = body.method().number();
        let bytes = body.bytes();
        let header = Header {
            made_by: version_needed(method, folder),
            needed: version_needed(method, folder),
            flags: if name.is_ascii() { 0 } else { FLAG_UTF8_NAME },
            method,
            time: 0,
            date: DOS_DATE,
            crc,
            compressed: u32::try_from(bytes.len()).map_err(|_| too_large())?,
            size: u32::try_from(size).map_err(|_| too_large())?,
            name: name.as_bytes(),
            internal: 0,
            external: if folder { DOS_DIRECTORY } else { 0 },
            local_extra: &[],
            central_extra: &[],
            comment: &[],
        };
        self.write(&header, bytes, vec![name.as_bytes().to_vec()])
    }

    /// Writes the local header of `header` and `data` after it, and keeps its row.
    fn write(&mut self, header: &Header<'_>, data: &[u8], names: Vec<Vec<u8>>) -> Result<(), ContainerError> {
        if !header.fits() {
            let name = String::from_utf8_lossy(header.name.get(..64).unwrap_or(header.name)).into_owned();
            return Err(ContainerError::InvalidPartName { name, reason: "longer, with its extra fields and comment, than a ZIP header holds" });
        }
        let offset = u32::try_from(self.out.len()).map_err(|_| too_large())?;
        self.grow(30usize.saturating_add(header.name.len()).saturating_add(header.local_extra.len()).saturating_add(data.len()))?;
        header.local(&mut self.out);
        self.out.extend_from_slice(data);
        self.rows.push(Row { central: header.central(offset), names });
        Ok(())
    }

    /// The central directory and the end record: the archive is complete. Two entries of one name, compared exactly, are
    /// `duplicate_part`, and nothing is given.
    pub fn finish(mut self) -> Result<Vec<u8>, ContainerError> {
        let mut order: Vec<(&[u8], usize)> =
            self.rows.iter().enumerate().flat_map(|(owner, row)| row.names.iter().map(move |name| (name.as_slice(), owner))).collect();
        order.sort_unstable();
        let twice = order.windows(2).find_map(|pair| match pair {
            [(first, a), (second, b)] if first == second && a != b => Some(*first),
            _ => None,
        });
        if let Some(name) = twice {
            let name = String::from_utf8_lossy(name).into_owned();
            return Err(ContainerError::DuplicatePart { first: name.clone(), second: name });
        }
        let directory_start = u32::try_from(self.out.len()).map_err(|_| too_large())?;
        let directory_bytes: usize = self.rows.iter().map(|r| r.central.len()).fold(22usize, usize::saturating_add);
        let end = self.out.len().checked_add(directory_bytes).ok_or_else(too_large)?;
        if u32::try_from(end).is_err() {
            return Err(too_large());
        }
        self.reserve_out(directory_bytes)?;
        let count = u16::try_from(self.rows.len()).map_err(|_| ContainerError::TooManyParts { limit: MAX_ITEMS })?;
        let out = &mut self.out;
        for row in &self.rows {
            out.extend_from_slice(&row.central);
        }
        let directory_size = u32::try_from(out.len() - directory_start as usize).map_err(|_| too_large())?;
        out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&directory_size.to_le_bytes());
        out.extend_from_slice(&directory_start.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        Ok(self.out)
    }
}

/// How one entry of an archive is copied: its row and its local header as written, where its data ends, the data descriptor its local
/// header announces when that is there in the form announced, and whether the headers are kept byte for byte. [`ArchiveWriter::copy`]
/// follows it and [`copy_account`] counts it, so the two cannot tell different stories.
struct CopyPlan<'a> {
    row: zipdir::RowFields<'a>,
    local: zipdir::LocalFields<'a>,
    data_end: usize,
    /// The length of the data descriptor after the data: 0 when the local header announces none, or the one it announces is not there.
    descriptor: usize,
    /// The length of a data descriptor of the other form than the local header announces, at the end of the data, when bit 3 announces one and
    /// the announced form is not there: bytes the copy leaves out, counted as `descriptor_form`.
    wrong_form: usize,
    exact: bool,
}

impl CopyPlan<'_> {
    /// Where the entry's bytes end in the file: after its data, and after the data descriptor its local header announces when that is
    /// there. At most the next local header, or the directory (see [`plan`]).
    fn end(&self) -> usize {
        self.data_end.saturating_add(self.descriptor)
    }

    /// Where what the copy accounts for ends: [`CopyPlan::end`], or the end of a descriptor of the wrong form, which the account counts under
    /// a reason of its own.
    fn accounted_end(&self) -> usize {
        self.end().max(self.data_end.saturating_add(self.wrong_form))
    }
}

/// The plan of `entry`'s copy (see the module documentation): its headers are kept as they are when the offset, the one field written
/// anew, is the row's own, and the local header says what the row says, as a reader that walks the file from local header to local header
/// reads it. Its data, and a data descriptor, must end by the next local header in the file or the directory: the layout the reader checked
/// holds the data there, and a descriptor that ran on would be another entry's bytes.
fn plan<'a>(archive: &Archive<'a>, entry: &Entry, work: &mut Work) -> Result<CopyPlan<'a>, ContainerError> {
    let source = archive.source();
    let row = zipdir::row_fields(source, entry.row_offset, entry.row_len)?;
    let local = zipdir::local_fields(source, entry.local_offset)?;
    let room = archive.room_after(entry.local_offset);
    let data_end = usize::try_from(entry.compressed)
        .ok()
        .and_then(|c| local.data.checked_add(c))
        .filter(|&end| end <= room)
        .ok_or_else(|| corrupt("the data of an entry to copy is not where its headers say"))?;
    let local_zip64 = zipdir::zip64_record(local.extra, work);
    let announced = local.flags & FLAG_DESCRIPTOR != 0;
    // The descriptor's form is the local header's to say: sizes of 8 bytes with a ZIP64 record, of 4 without (APPNOTE 4.3.9.2). Trying the
    // short form first took the first 16 bytes of a zero-length entry's 24 for a descriptor, as both state zeros.
    let descriptor =
        if announced { zipdir::descriptor_len(source, data_end, room, row.crc, entry.compressed, entry.size, local_zip64.is_some()) } else { None };
    // A descriptor of the other form, where the announced one is not: told apart from bytes no flag announces.
    let wrong_form = if announced && descriptor.is_none() {
        zipdir::descriptor_len(source, data_end, room, row.crc, entry.compressed, entry.size, local_zip64.is_none()).unwrap_or(0)
    } else {
        0
    };
    let plain = row.local32 != SENTINEL32 && row.disk == 0;
    let agree = plain && local.name == row.name && local.method == row.method && local.flags == row.flags;
    let exact = agree
        && if announced {
            descriptor.is_some()
        } else {
            // Sizes left to a ZIP64 field (the 32-bit field 0xFFFFFFFF in both headers) are those the local header's own record gives,
            // both of them (APPNOTE 4.5.3), and the row's: a streaming reader takes them from the local record.
            let stated = (local.crc, local.compressed, local.size) == (row.crc, row.compressed32, row.size32);
            let wide = local.compressed == SENTINEL32 || local.size == SENTINEL32;
            stated && (!wide || local_zip64.and_then(zipdir::zip64_sizes) == Some((entry.size, entry.compressed)))
        };
    Ok(CopyPlan { row, local, data_end, descriptor: descriptor.unwrap_or(0), wrong_form, exact })
}

/// What a copy of every entry of an archive, one by one in the order the entries lie in the file (what an unedited save is),
/// keeps and what it leaves out, worked out from the archive without writing anything: [`copy_account`]. A copy
/// writes a well-formed archive, so what no header announces is not written, and this says how much of it there was
/// and why. **The copy is the file, byte for byte, exactly when [`CopyAccount::is_exact`]**; when it is not, the fields say why.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CopyAccount {
    /// Entries whose headers are kept byte for byte ([`Copied::Exact`]).
    pub exact: usize,
    /// Entries copied under headers written anew from their rows ([`Copied::Rewritten`]): each changes the file, and its data descriptor,
    /// if it had one, goes with its old headers.
    pub rewritten: usize,
    /// The central directory lists the entries in another order than they lie in the file; the copy's lists them as they lie.
    pub rows_reordered: bool,
    /// Rows the archive's judge did not keep ([`crate::zip::Verdict::Drop`]): a copy of the entries kept has none of them. Their entries'
    /// bytes are among [`Dropped::unannounced`]. None for the plain judge, which keeps every row.
    pub rows_not_kept: usize,
    /// Bytes of the file the copy does not write, by why.
    pub dropped: Dropped,
}

impl CopyAccount {
    /// Nothing is rewritten, reordered or left out: the copy is the file, byte for byte.
    pub fn is_exact(&self) -> bool {
        self.rewritten == 0 && !self.rows_reordered && self.rows_not_kept == 0 && self.dropped.total() == 0
    }
}

/// Bytes of an archive that a copy of its entries does not write, by why. Each is a count of bytes of the file read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Dropped {
    /// `unannounced`: bytes no header announces, before the first entry, between two entries, after the last before the directory, and
    /// between the directory and the end records: what follows an entry that is not its local header, name, extra field, data, or the data
    /// descriptor its flags announce in the form its local header announces (a descriptor written where no flag says one follows is such
    /// bytes).
    pub unannounced: u64,
    /// `descriptor_form`: a data descriptor of another form than its entry's local header announces (APPNOTE 4.3.9.2: sizes of 8 bytes when
    /// the local header holds a ZIP64 record, of 4 when it does not), where bit 3 announces one and the announced form is not there, and the
    /// descriptor of the other form is, stating the row's checksum and sizes..
    pub descriptor_form: u64,
    /// `directory_tail`: bytes of the directory after its last row, which the reader leaves unread (a digital signature record, APPNOTE
    /// 4.3.13, or anything that is not a row).
    pub directory_tail: u64,
    /// `zip64_end_records`: the ZIP64 end record and its locator, from the record to the plain end record; the copy's plain end record
    /// counts what they counted.
    pub zip64_end_records: u64,
    /// `archive_comment`: the end record's comment.
    pub archive_comment: u64,
    /// `after_end_record`: bytes after the end record and its comment.
    pub after_end_record: u64,
}

impl Dropped {
    /// Every byte left out.
    pub fn total(&self) -> u64 {
        self.reasons().iter().fold(0u64, |sum, (_, bytes)| sum.saturating_add(*bytes))
    }

    /// Each reason's word and its count of bytes, in a fixed order, zeros included.
    pub fn reasons(&self) -> [(&'static str, u64); 6] {
        [
            ("unannounced", self.unannounced),
            ("descriptor_form", self.descriptor_form),
            ("directory_tail", self.directory_tail),
            ("zip64_end_records", self.zip64_end_records),
            ("archive_comment", self.archive_comment),
            ("after_end_record", self.after_end_record),
        ]
    }
}

/// The bytes from `from` to `to`, none when `to` is not past `from`.
fn span(from: usize, to: usize) -> u64 {
    u64::try_from(to.saturating_sub(from)).unwrap_or(u64::MAX)
}

/// What a copy of every entry of `archive`, in the order the entries lie in the file, keeps and leaves out ([`CopyAccount`]): each entry
/// planned as [`ArchiveWriter::copy`] plans it, one sort of the entries, and no byte written or inflated. A row or local header that is not
/// where its entry says is `zip_corrupt`, as for [`ArchiveWriter::copy`].
pub fn copy_account(archive: &Archive<'_>) -> Result<CopyAccount, ContainerError> {
    copy_account_counted(archive, &mut Work::default())
}

/// [`copy_account`], adding the records of the local headers' extra fields it walked to `work` ([`Work::local_extra_records`]), whether the
/// account is made or the archive refused.
pub fn copy_account_counted(archive: &Archive<'_>, work: &mut Work) -> Result<CopyAccount, ContainerError> {
    let entries = archive.entries();
    let mut account = CopyAccount {
        rows_reordered: entries.windows(2).any(|pair| matches!(pair, [a, b] if a.local_offset > b.local_offset)),
        rows_not_kept: archive.rows().saturating_sub(entries.len()),
        ..CopyAccount::default()
    };
    let mut order: Vec<(usize, &Entry)> = entries.iter().map(|e| (e.local_offset, e)).collect();
    order.sort_unstable_by_key(|&(offset, _)| offset);
    let mut at = 0usize;
    for (offset, entry) in order {
        let plan = plan(archive, entry, work)?;
        account.dropped.unannounced = account.dropped.unannounced.saturating_add(span(at, offset));
        account.dropped.descriptor_form = account.dropped.descriptor_form.saturating_add(u64::try_from(plan.wrong_form).unwrap_or(u64::MAX));
        at = plan.accounted_end().max(at);
        if plan.exact {
            account.exact += 1;
        } else {
            account.rewritten += 1;
        }
    }
    let tail = archive.tail();
    let records = tail.zip64_record.unwrap_or(tail.end_record);
    let dropped = &mut account.dropped;
    dropped.unannounced = dropped.unannounced.saturating_add(span(at, archive.directory_start())).saturating_add(span(tail.directory_end, records));
    dropped.directory_tail = span(tail.rows_end, tail.directory_end);
    dropped.zip64_end_records = span(records, tail.end_record);
    dropped.archive_comment = u64::try_from(tail.comment).unwrap_or(u64::MAX);
    dropped.after_end_record = u64::try_from(archive.bytes_after_end_record()).unwrap_or(u64::MAX);
    Ok(account)
}

/// The bytes an entry's data is written as.
enum Body<'d> {
    Stored(&'d [u8]),
    Deflated(Vec<u8>),
}

impl Body<'_> {
    fn method(&self) -> Method {
        match self {
            Body::Stored(_) => Method::Stored,
            Body::Deflated(_) => Method::Deflated,
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            Body::Stored(data) => data,
            Body::Deflated(bytes) => bytes,
        }
    }
}

/// How an entry's bytes are written: what each of [`ArchiveWriter::add`] (with a method), [`ArchiveWriter::add_smallest`] and [`ArchiveWriter::add_at_level`] does with them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pack {
    /// Stored, as [`ArchiveWriter::add`] with [`Method::Stored`].
    Stored,
    /// Deflated, even where that is larger, at the default level and the fast one past 4 MiB, as [`ArchiveWriter::add`] with [`Method::Deflated`].
    Deflated,
    /// Deflated, or stored when deflate does not make it smaller, as [`ArchiveWriter::add_smallest`].
    Smallest,
    /// Deflated at this level (0 to 9) whatever the size, as [`ArchiveWriter::add_at_level`].
    Level(u32),
    /// A hook of the unit tests: an entry whose compression panics.
    #[cfg(test)]
    Panics,
}

/// An entry to write with [`ArchiveWriter::add_many`].
#[derive(Clone, Copy, Debug)]
pub struct Add<'a> {
    /// The name, as for [`ArchiveWriter::add`].
    pub name: &'a str,
    pub data: &'a [u8],
    pub how: Pack,
}

impl Add<'_> {
    /// The bytes that the compression of this entry may hold while it is in flight, charged before it is given to a worker: none for an entry that is stored (its checksum is all
    /// that is made), else the entry's size and an eighth (what the output buffer grows to at the most besides a stream that does not shrink) and a few kilobytes of the compressor's own.
    fn charge(&self) -> usize {
        match self.how {
            Pack::Stored => 0,
            _ => self.data.len().saturating_add(self.data.len() / 8).saturating_add(COMPRESSOR_BYTES),
        }
    }
}

/// How many threads [`ArchiveWriter::add_many`] may use, and how many bytes of compression it may have in flight at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Workers {
    /// The most threads: 1 is the single-threaded path, which makes no thread.
    pub threads: usize,
    /// The most bytes in flight, counted as [`Add`] charges an entry: an entry charged more than this is made when nothing else is in flight.
    pub window: usize,
}

impl Workers {
    /// The window of [`Workers::new`]: 128 MiB.
    pub const WINDOW: usize = 128 * 1024 * 1024;

    /// One thread: the entries are added one after another by the caller's thread.
    pub const ONE: Workers = Workers { threads: 1, window: 0 };

    /// `threads` threads and the default window.
    pub fn new(threads: usize) -> Workers {
        Workers { threads, window: Workers::WINDOW }
    }
}

/// The least size of an entry that [`ArchiveWriter::add_many`] gives to a worker: a smaller one is made by the caller's thread, where the time of a hand-over is more than its compression.
const PARALLEL_FROM: usize = 32 * 1024;

/// What a compressor holds of its own while it works, besides its output: its tables, in kibibytes, counted at the most of what the backend holds.
const COMPRESSOR_BYTES: usize = 512 * 1024;

/// An entry compressed and checksummed apart from an archive, by [`pack`], to be written with [`ArchiveWriter::add_packed`].
#[derive(Debug)]
pub struct Packed {
    body: PackedBody,
    crc: u32,
    /// The bytes the entry was packed from.
    size: usize,
}

#[derive(Debug)]
enum PackedBody {
    Stored,
    Deflated(Vec<u8>),
}

impl Packed {
    /// The method the entry is written with.
    pub fn method(&self) -> Method {
        match self.body {
            PackedBody::Stored => Method::Stored,
            PackedBody::Deflated(_) => Method::Deflated,
        }
    }
}

/// The level of [`Pack::Level`] is deflate's (0 to 9): a level past it is refused as `unsupported_compression`.
fn level_is_deflate(name: &str, level: u32) -> Result<(), ContainerError> {
    if level > 9 {
        return Err(ContainerError::UnsupportedCompression { name: name.chars().take(64).collect(), method: format!("deflate level {level}") });
    }
    Ok(())
}

/// What the compression of the entry `name` of `data`, for `how`, makes: the bytes deflated, or stored, and the checksum, with nothing written. **It does what [`ArchiveWriter::add`],
/// [`ArchiveWriter::add_smallest`] or [`ArchiveWriter::add_at_level`] does before it writes**, and may be called on any thread; [`ArchiveWriter::add_packed`] writes it.
pub fn pack(name: &str, data: &[u8], how: Pack) -> Result<Packed, ContainerError> {
    if let Pack::Level(level) = how {
        level_is_deflate(name, level)?;
    }
    check_name(name, name.len(), false)?;
    if u32::try_from(data.len()).is_err() {
        return Err(too_large());
    }
    let whole = usize::try_from(u32::MAX).unwrap_or(usize::MAX);
    let body = match how {
        Pack::Stored => PackedBody::Stored,
        Pack::Deflated => PackedBody::Deflated(deflate(data)?),
        Pack::Smallest if data.is_empty() => PackedBody::Stored,
        Pack::Smallest => {
            let bytes = deflate(data)?;
            if bytes.len() < data.len() { PackedBody::Deflated(bytes) } else { PackedBody::Stored }
        }
        Pack::Level(level) => PackedBody::Deflated(deflate_at(data, Compression::new(level), whole)?),
        #[cfg(test)]
        #[allow(clippy::panic)] // the hook of a unit test of a worker that panics
        Pack::Panics => panic!("an entry of a test that panics"),
    };
    Ok(Packed { body, crc: crc32fast::hash(data), size: data.len() })
}

/// APPNOTE 4.4.3.2: 2.0 for deflate and for a folder, 1.0 for a stored file.
fn version_needed(method: u16, folder: bool) -> u16 {
    if method == 8 || folder { 20 } else { 10 }
}

/// Why a [`Sink`] stopped taking bytes, kept where the caller can read it after the encoder has taken the sink and given an error for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stopped {
    No,
    TooLarge,
    NoMemory,
}

/// A buffer the encoder writes into that grows by `try_reserve` and not by the abort a `Vec` makes when memory is not had (a part of 254 MB deflated into a plain `Vec` aborted the process at its 256 MiB doubling).
/// It grows by an eighth of what it holds
/// at least, and by exactly what a write needs when the eighth is not to be had, so that an archive that fits is not refused for a doubling that
/// does not.
struct Sink {
    out: Vec<u8>,
    /// The most bytes it may hold: what a ZIP entry's size field holds.
    room: usize,
    stopped: std::rc::Rc<std::cell::Cell<Stopped>>,
    /// The unit tests' way to run out of memory, which a test cannot do for real: which reservations are refused.
    #[cfg(test)]
    refuse: Refuse,
}

/// Which reservations of a [`Sink`] the unit tests refuse.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refuse {
    Nothing,
    /// The reservation of the step, and not the one of the write's own size that follows it.
    TheStep,
    Everything,
}

impl Sink {
    fn new(room: usize) -> Sink {
        Sink {
            out: Vec::new(),
            room,
            stopped: std::rc::Rc::new(std::cell::Cell::new(Stopped::No)),
            #[cfg(test)]
            refuse: Refuse::Nothing,
        }
    }

    /// Makes room for `more` bytes beyond those held, exactly; `false` when the memory is not had. `step` says which of a write's two
    /// reservations this is.
    fn reserve(&mut self, more: usize, step: bool) -> bool {
        #[cfg(test)]
        if self.refuse == Refuse::Everything || (self.refuse == Refuse::TheStep && step) {
            return false;
        }
        let _ = step;
        self.out.try_reserve_exact(more).is_ok()
    }
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let stop = |why: Stopped, cell: &std::cell::Cell<Stopped>| {
            cell.set(why);
            Err(std::io::Error::from(std::io::ErrorKind::OutOfMemory))
        };
        if self.out.len().saturating_add(buf.len()) > self.room {
            return stop(Stopped::TooLarge, &self.stopped);
        }
        if self.out.capacity().saturating_sub(self.out.len()) < buf.len() {
            let step = (self.out.len() / 8).max(buf.len());
            if !self.reserve(step, true) && !self.reserve(buf.len(), false) {
                return stop(Stopped::NoMemory, &self.stopped);
            }
        }
        self.out.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// `data` deflated: at the default level, or the fast one past [`FAST_ABOVE`]. Memory that is not had is [`ContainerError::OutOfMemory`], and
/// an output of more than a ZIP entry's size field holds is refused as it grows.
fn deflate(data: &[u8]) -> Result<Vec<u8>, ContainerError> {
    deflate_within(data, usize::try_from(u32::MAX).unwrap_or(usize::MAX))
}

/// [`deflate`], refusing an output of more than `room` bytes as `package_too_large`.
fn deflate_within(data: &[u8], room: usize) -> Result<Vec<u8>, ContainerError> {
    deflate_into(data, Sink::new(room))
}

/// [`deflate_within`] into the sink given.
fn deflate_into(data: &[u8], sink: Sink) -> Result<Vec<u8>, ContainerError> {
    let level = if data.len() > FAST_ABOVE { Compression::fast() } else { Compression::default() };
    deflate_into_at(data, sink, level)
}

/// `data` deflated at exactly `level`, refusing an output of more than `room` bytes as `package_too_large`.
fn deflate_at(data: &[u8], level: Compression, room: usize) -> Result<Vec<u8>, ContainerError> {
    deflate_into_at(data, Sink::new(room), level)
}

/// `data` deflated at `level` into the sink given.
fn deflate_into_at(data: &[u8], sink: Sink, level: Compression) -> Result<Vec<u8>, ContainerError> {
    let stopped = std::rc::Rc::clone(&sink.stopped);
    let mut encoder = DeflateEncoder::new(sink, level);
    let written = encoder.write_all(data);
    let finished = encoder.finish();
    match (stopped.get(), written, finished) {
        (Stopped::TooLarge, ..) => Err(too_large()),
        (Stopped::NoMemory, ..) => Err(out_of_memory()),
        (Stopped::No, Ok(()), Ok(sink)) => Ok(sink.out),
        _ => Err(ContainerError::ZipCorrupt { detail: "an entry could not be compressed".to_owned() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bytes that deflate to about their own size.
    fn noise(len: usize) -> Vec<u8> {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (state >> 33) as u8
            })
            .collect()
    }

    #[test]
    fn an_output_past_the_room_is_refused_as_it_grows_and_one_within_it_is_given() {
        let data = noise(4_000);
        let whole = deflate(&data).map(|d| d.len()).unwrap_or(0);
        assert!(whole > 3_000, "noise does not deflate: {whole}");
        assert_eq!(deflate_within(&data, whole).map(|d| d.len()).ok(), Some(whole), "room for exactly the output");
        assert_eq!(deflate_within(&data, whole - 1).err().map(|e| e.class()), Some("package_too_large"), "one byte less");
        assert_eq!(deflate_within(&data, 100).err().map(|e| e.class()), Some("package_too_large"));
        // The sink's growth is the eighth of what it holds, and a write's own size when that is more.
        let mut sink = Sink::new(usize::MAX);
        assert_eq!(sink.write(&[1; 100]).ok(), Some(100));
        assert!(sink.out.capacity() >= 100 && sink.out.capacity() < 200, "{}", sink.out.capacity());
        assert_eq!(sink.write(&[2; 10]).ok(), Some(10));
        assert_eq!(sink.out.len(), 110);
        // Exactly the eighth of the 100 it held (12), by `try_reserve_exact`: the doubling a `Vec` makes by itself would hold 200.
        assert_eq!(sink.out.capacity(), 112);
        // A write that fits in what is free grows nothing; the first that does not grows it by the eighth of what it holds (14 of 112).
        assert_eq!(sink.write(&[3; 2]).ok(), Some(2));
        assert_eq!(sink.out.capacity(), 112, "a write that fits reserves nothing");
        assert_eq!(sink.write(&[4; 1]).ok(), Some(1));
        assert_eq!(sink.out.capacity(), 112 + 14);
        // And a write bigger than the eighth grows it by its own size.
        assert_eq!(sink.write(&[5; 100]).ok(), Some(100));
        assert_eq!(sink.out.len(), 213);
        assert_eq!(sink.out.capacity(), 213);
        assert_eq!(sink.stopped.get(), Stopped::No);
    }

    #[test]
    fn a_worker_that_panics_is_an_error_in_the_place_of_its_entry_and_nothing_after_it_is_written() {
        // The compressor of one entry panics on a worker; the writer says so as an error of its own (zip_corrupt, in words), the entry before it is written, and no thread of
        // the process is left behind (the scope joined them, or this test would not end).
        let big = noise(200_000);
        let items = [
            Add { name: "a.bin", data: &big, how: Pack::Smallest },
            Add { name: "b.bin", data: &big, how: Pack::Panics },
            Add { name: "c.bin", data: &big, how: Pack::Smallest },
            Add { name: "d.bin", data: &big, how: Pack::Smallest },
        ];
        for threads in [2, 7, 31] {
            let mut writer = ArchiveWriter::new();
            let error = writer.add_many(&items, Workers::new(threads)).err();
            assert!(
                matches!(&error, Some(ContainerError::ZipCorrupt { detail }) if detail.contains("could not be compressed")),
                "{threads} threads: {error:?}"
            );
            assert_eq!(writer.len(), 1, "{threads} threads: the entry before it is written and none after it");
        }
    }

    #[test]
    fn memory_that_is_not_had_is_an_error_of_its_own_and_the_exact_reservation_is_the_one_that_is_tried_second() {
        let data = noise(4_000);
        let into = |refuse: Refuse| {
            let mut sink = Sink::new(usize::MAX);
            sink.refuse = refuse;
            deflate_into(&data, sink)
        };
        assert!(into(Refuse::Nothing).is_ok());
        assert_eq!(into(Refuse::Everything).err().map(|e| e.class()), Some("out_of_memory"), "no memory: an error and not an abort");
        // The step refused and the write's own size granted: the sink takes what the write needs and goes on.
        assert_eq!(into(Refuse::TheStep).ok(), into(Refuse::Nothing).ok(), "the output is the same");
    }
}
