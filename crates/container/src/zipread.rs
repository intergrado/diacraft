//! The container: a ZIP archive read into entries, bounded by what it really holds and not by what its headers say.
//!
//! `zipdir.rs` finds the central directory and walks its rows; this module asks the caller's [`Judge`] about each row, lays the entries
//! out, and inflates them itself (`flate2`), so that every limit (which applies
//! to ODF packages too) holds for the bytes that come out and a header that lies cannot move it:
//!
//! 1. **Names and headers first, for every entry, before a byte is inflated.** The judge says, row by row in the order of the directory,
//!    whether the row is kept, under what name, with what cap, and whether it counts as a part; then this module checks the part count,
//!    encryption and method, the ratio of each entry by its header, its size against its cap, and the running total of the sizes.
//!    The first entry that breaks a rule ends the read. The count itself was checked before a row was read, and what it leaves of the
//!    directory after the last row is checked (it must not be another row): both in `zipdir.rs`. Then the judge sees the whole list.
//! 2. **Then the layout**: every entry's local header is found where its row says, and no two entries, and no entry and the directory,
//!    share a byte. That is one sort of at most as many ranges as there are rows, dropped rows included.
//! 3. **Then each entry inflated through a reader that stops at a limit**, when the caller asks ([`Archive::inflate`]). The limit for an
//!    entry is the least of its cap, what its ratio allows (`max_ratio` times its compressed size), and what is left of the package total,
//!    computed from the bytes inflated so far. An entry that reaches it is refused as what it is (`zip_ratio_exceeded`, `part_too_large`
//!    or `package_too_large`, in that order when two are the same number), from the bytes, not the header. An entry the judge does
//!    not count (OPC's content types stream) is held to its cap and its ratio and not to the package total, and the cap is the judge's:
//!    this crate does not clamp it to the part limit, so a judge that gives an uncounted entry a cap bounds it by that cap alone.
//! 4. **Then the entry's own claims are checked against what came out**: the size the row states, and the checksum.
//!
//! So the most that is ever inflated is the package limit, and that bound holds however the headers lie; one more entry is read only as
//! far as the limit and then dropped. The total of the compressed sizes cannot be more than the file (entries that overlap are refused),
//! so the output is also at most `max_ratio` times the file. Every buffer for inflated bytes is grown with `try_reserve`, so memory that
//! runs out is [`ContainerError::OutOfMemory`] and not an abort.
//!
//! **A labelled deviation from R2: the central directory wins.** R2 says local and central headers must agree. This
//! reader does not refuse a disagreement. The central row decides the name, the flags, the method, the sizes and the checksum; the
//! local header is read for one thing, the lengths of its own name and extra field, which say where the data starts (and the second of
//! them is given, [`Entry::local_extra_len`], since an ODF package's first entry must have none); a data descriptor (flag bit 3) is never
//! read; a local name such as `../evil.xml` is never used for anything. That is safe because every byte an entry yields is bounded by the
//! central row's own sizes and checked against its checksum, and because no name from a local header exists to reach a file system.

use std::io::{ErrorKind, Read};
use std::sync::Arc;

use crate::ContainerError;
use crate::limits::Limits;
use crate::work::Work;
use crate::zipdir::{self, Plan, Rows, corrupt};

/// General purpose flag bits that make an entry encrypted (R2 says encryption is illegal): bit 0 (encrypted), bit 6 (strong
/// encryption) and bit 13 (the local header values are masked, which only the encryption of the directory does).
const FLAGS_ENCRYPTED: u16 = 0x0001 | 0x0040 | 0x2000;

/// The size of the buffer every entry is read through.
const SCRATCH_BYTES: usize = 64 * 1024;

/// How an entry's bytes are kept: the two methods a package may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    /// Method 0: the bytes as they are.
    Stored,
    /// Method 8: raw deflate.
    Deflated,
}

impl Method {
    /// The method's number in a ZIP header: 0 or 8.
    pub const fn number(self) -> u16 {
        match self {
            Method::Stored => 0,
            Method::Deflated => 8,
        }
    }

    /// The method with this number, or `None` for any method but 0 and 8.
    pub const fn from_number(number: u16) -> Option<Method> {
        match number {
            0 => Some(Method::Stored),
            8 => Some(Method::Deflated),
            _ => None,
        }
    }
}

/// One row of the central directory: what a [`Judge`] is shown, and, for a row it keeps, what the [`Archive`] holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The name. As the row gives it (a Unicode path field's when the field counts, else the name as written, UTF-8 or code page 437)
    /// to the judge; in the archive, the name the judge kept the entry under.
    pub name: String,
    /// The name as written, when a Unicode path field gave `name`, so that the judge can hold it to its rules too: a reader that ignores
    /// the field sees it. Given to the judge and not kept: `None` in the archive's entries.
    pub written: Option<String>,
    /// The name as written ends in a slash: a folder, which has no data (a folder whose row states a size is `zip_corrupt`).
    pub folder: bool,
    /// The general purpose bit flags of the row.
    pub flags: u16,
    /// The method number of the row. A file the archive keeps has 0 or 8 ([`Entry::compression`]); a folder may have any.
    pub method: u16,
    /// The CRC-32 the row states.
    pub crc: u32,
    /// The compressed size the row states (the ZIP64 field's when the row says so).
    pub compressed: u64,
    /// The uncompressed size the row states.
    pub size: u64,
    /// Where the entry's local header is, from byte 0.
    pub local_offset: usize,
    /// Where the entry's data starts, from byte 0: after its local header's name and extra field. 0 until the layout is read, so 0 for
    /// the judge.
    pub data_offset: usize,
    /// The length of the local header's extra field. 0 until the layout is read.
    pub local_extra_len: usize,
    /// The most bytes the entry may inflate to, as the judge said. 0 for the judge, and for a folder.
    pub cap: u64,
    /// Whether the entry counts toward the part limit and the package total, as the judge said. `false` for the judge, and for a folder.
    pub counted: bool,
    /// The row's "version made by" (APPNOTE 4.4.2): the system and the version of the writer.
    pub version_made_by: u16,
    /// The row's "version needed to extract" (APPNOTE 4.4.3).
    pub version_needed: u16,
    /// The row's MS-DOS time and date (APPNOTE 4.4.6), which a copy keeps and nothing else reads.
    pub time: u16,
    pub date: u16,
    /// The row's internal and external file attributes (APPNOTE 4.4.14, 4.4.15): the system's own bits, kept by a copy.
    pub internal_attributes: u16,
    pub external_attributes: u32,
    /// Where the entry's row of the central directory is, from byte 0, and how many bytes it takes (its fixed part, name, extra field
    /// and comment): what [`ArchiveWriter::copy`](crate::zip::ArchiveWriter::copy) writes again with only the offset changed.
    pub row_offset: usize,
    pub row_len: usize,
}

impl Entry {
    /// The name as written: [`Entry::written`] when a Unicode path field gave the name, else [`Entry::name`].
    pub fn written_name(&self) -> &str {
        self.written.as_deref().unwrap_or(&self.name)
    }

    /// The method, when it is one a package may use.
    pub fn compression(&self) -> Option<Method> {
        Method::from_number(self.method)
    }

    /// Stored, with no extra field in its local header: what asks of an ODF package's `mimetype`, whose name must
    /// then start at byte 30 and its bytes right after it. Whether it is the first entry is [`Entry::local_offset`] 0.
    pub fn is_plain_stored(&self) -> bool {
        self.compression() == Some(Method::Stored) && self.local_extra_len == 0
    }

    /// The extra field and the comment of the entry's central row, as written, read again from `bytes`, the file the entry was read from
    /// (for a comparison of two archives' headers). `None` when `bytes` hold no row there.
    pub fn row_extra_and_comment<'b>(&self, bytes: &'b [u8]) -> Option<(&'b [u8], &'b [u8])> {
        zipdir::row_fields(bytes, self.row_offset, self.row_len).ok().map(|row| (row.extra, row.comment))
    }

    /// The extra field of the entry's local header, as written, from `bytes`, the file the entry was read from. `None` when `bytes` do not
    /// reach it.
    pub fn local_extra<'b>(&self, bytes: &'b [u8]) -> Option<&'b [u8]> {
        bytes.get(self.data_offset.checked_sub(self.local_extra_len)?..self.data_offset)
    }
}

/// What a [`Judge`] makes of one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Keep the entry: under `rename` when it is given (the caller's own form of the name), else under its name; inflate it to at most
    /// `cap` bytes; and count it toward the part limit and the package total when `counted`. For a folder only `rename` is used: a
    /// folder has no data and counts toward nothing but the rows.
    Keep { rename: Option<String>, cap: u64, counted: bool },
    /// Do not keep the entry. Its bytes are still part of the layout, so it may not overlap another; nothing else of it is checked.
    Drop,
}

/// The caller's rules for the rows of an archive: which are kept, under what name, and what makes the whole list refused. The archive's
/// own rules (the count, the layout, encryption, the method, the ratio, the caps, the checksums) are this crate's and hold whatever a
/// judge says.
pub trait Judge {
    /// The error a read gives. A [`ContainerError`] of the archive becomes one of these.
    type Error: From<ContainerError>;

    /// One row, as it is read: before the next row is read, and before anything is inflated. `kept` holds the entries kept so far, in
    /// order, under the names they were kept by; the row, if kept, will be `kept.len()`.
    fn entry(&mut self, entry: &Entry, kept: &[Entry]) -> Result<Verdict, Self::Error>;

    /// After the last row, and before the layout is read: every entry kept. What the judge's own work over the list costs it adds to
    /// `work` (as [`Plain`] does in [`Work::name_comparisons`]).
    fn listed(&mut self, kept: &[Entry], work: &mut Work) -> Result<(), Self::Error> {
        let _ = (kept, work); // A judge that has nothing to say of the whole list says nothing, and costs nothing.
        Ok(())
    }
}

/// The judge [`Archive::read`] uses, for a package whose names are plain names compared exactly. Every row is kept,
/// folders included, under the name it has, with the part limit as its cap and counted. Refused, for the name a row has and for its
/// name as written when a Unicode path field replaced it: a leading slash, a backslash, a drive letter or a `..` segment
/// (`path_traversal`); an empty name, and one longer than the name limit (`invalid_part_name`); a Unicode path that is a folder where the
/// name as written is not, or the other way (`invalid_part_name`); and two rows with one name, by either name (`duplicate_part`).
#[derive(Debug, Clone)]
pub struct Plain {
    max_name: usize,
    cap: u64,
    /// The names as written that a Unicode path field replaced, with the index of their row among those kept.
    written: Vec<(String, usize)>,
}

impl Plain {
    /// The judge for one read within these limits (it keeps what the rows of that read wrote, for the duplicate rule).
    pub fn new(limits: &Limits) -> Plain {
        Plain { max_name: limits.max_part_name_bytes, cap: limits.max_part_bytes, written: Vec::new() }
    }
}

/// A name a plain package may hold.
fn check_plain_name(name: &str, max_name: usize) -> Result<(), ContainerError> {
    let bytes = name.as_bytes();
    let drive = matches!((bytes.first(), bytes.get(1)), (Some(c), Some(b':')) if c.is_ascii_alphabetic());
    let bare = name.strip_suffix('/').unwrap_or(name);
    if name.starts_with('/') || name.contains('\\') || drive || bare.split('/').any(|segment| segment == "..") {
        return Err(ContainerError::PathTraversal { name: name.to_owned() });
    }
    if bare.is_empty() {
        return Err(ContainerError::InvalidPartName { name: name.to_owned(), reason: "empty" });
    }
    if bare.len() > max_name {
        return Err(ContainerError::InvalidPartName { name: name.chars().take(64).collect(), reason: "longer than the limit" });
    }
    Ok(())
}

impl Judge for Plain {
    type Error = ContainerError;

    fn entry(&mut self, entry: &Entry, kept: &[Entry]) -> Result<Verdict, ContainerError> {
        if let Some(written) = &entry.written {
            check_plain_name(written, self.max_name)?;
            if entry.name.ends_with('/') != entry.folder {
                return Err(ContainerError::InvalidPartName {
                    name: entry.name.chars().take(64).collect(),
                    reason: "a Unicode path that is a folder where the name as written is not, or the other way",
                });
            }
        }
        check_plain_name(&entry.name, self.max_name)?;
        if let Some(written) = &entry.written {
            self.written.push((written.clone(), kept.len()));
        }
        Ok(Verdict::Keep { rename: None, cap: self.cap, counted: true })
    }

    /// Two rows with one name, exactly, by the names kept or the names as written: one sort and one pass, each comparison of two names
    /// counted in [`Work::name_comparisons`].
    fn listed(&mut self, kept: &[Entry], work: &mut Work) -> Result<(), ContainerError> {
        let mut names: Vec<(&str, usize)> = kept.iter().enumerate().map(|(i, e)| (e.name.as_str(), i)).collect();
        names.extend(self.written.iter().map(|(name, owner)| (name.as_str(), *owner)));
        let mut comparisons = 0u64;
        names.sort_unstable_by(|a, b| {
            comparisons = comparisons.saturating_add(1);
            a.cmp(b)
        });
        let mut found = None;
        for pair in names.windows(2) {
            comparisons = comparisons.saturating_add(1);
            if let [(first, a), (second, b)] = pair
                && first == second
                && a != b
            {
                found = Some(ContainerError::DuplicatePart { first: (*first).to_owned(), second: (*second).to_owned() });
                break;
            }
        }
        work.name_comparisons = work.name_comparisons.saturating_add(comparisons);
        found.map_or(Ok(()), Err)
    }
}

/// A ZIP archive whose rows are read and judged, whose layout is checked, and whose entries are inflated one by one when asked.
///
/// It borrows the bytes of the file: nothing is copied until an entry is inflated.
pub struct Archive<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry>,
    /// Whether each entry has been inflated once, so that a second inflate of it is not added to the total again.
    read: Vec<bool>,
    rows: usize,
    after: usize,
    /// Where the central directory starts: no entry's bytes, and no data descriptor a copy keeps, reach past it.
    directory_start: usize,
    /// Where every row's local header is, kept or dropped, in the order of the file: no data descriptor a copy keeps reaches the next one.
    starts: Vec<usize>,
    /// What lies after the entries, for an account of what a copy leaves out (`zipwrite.rs`).
    tail: Tail,
    max_ratio: u64,
    max_package_bytes: u64,
    /// The bytes of the counted entries inflated so far.
    total: u64,
    scratch: Vec<u8>,
}

impl<'a> Archive<'a> {
    /// Reads the archive in `bytes` within `limits`, with the [`Plain`] judge. Nothing is inflated yet.
    pub fn read(bytes: &'a [u8], limits: &Limits) -> Result<Archive<'a>, ContainerError> {
        Archive::read_with(bytes, limits, &mut Plain::new(limits), &mut Work::default())
    }

    /// Reads the archive in `bytes` within `limits`, with `judge` for the caller's rules, and adds what it cost to `work` (whether it
    /// reads or is refused). The first rule broken ends the read; nothing is inflated yet.
    pub fn read_with<J: Judge>(bytes: &'a [u8], limits: &Limits, judge: &mut J, work: &mut Work) -> Result<Archive<'a>, J::Error> {
        let plan = zipdir::locate(bytes, work)?;
        // Rows are parts, folders and what the caller does not count, so a count of more than twice the part limit (and a few) is refused
        // whatever the mix. The count has already been shown to fit in the directory, so this is a limit and not damage.
        let most_entries = u64::try_from(limits.max_parts.saturating_mul(2).saturating_add(16)).unwrap_or(u64::MAX);
        if plan.count > most_entries {
            return Err(ContainerError::TooManyParts { limit: limits.max_parts }.into());
        }
        let Listing { mut entries, locals, rows_end } = list_entries(bytes, &plan, limits, judge, work)?;
        let starts = check_layout(bytes, &locals, plan.start, work)?;
        for entry in &mut entries {
            let local = zipdir::local_header(bytes, entry.local_offset, work)?;
            entry.data_offset = local.data;
            entry.local_extra_len = local.extra_len;
        }
        let tail = Tail { rows_end, directory_end: plan.end, zip64_record: plan.zip64_record, end_record: plan.end_record, comment: plan.comment };
        Ok(Archive {
            bytes,
            read: vec![false; entries.len()],
            entries,
            rows: locals.len(),
            after: plan.after,
            directory_start: plan.start,
            starts,
            tail,
            max_ratio: limits.max_ratio,
            max_package_bytes: limits.max_package_bytes,
            total: 0,
            scratch: Vec::new(),
        })
    }

    /// The entries kept, in the order of the central directory (which means nothing), each under the name it was kept by.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entries kept, given up: for a caller that has inflated what it wants and keeps the names.
    pub fn into_entries(self) -> Vec<Entry> {
        self.entries
    }

    /// The index of the first entry kept under exactly `name`.
    pub fn position(&self, name: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.name == name)
    }

    /// Rows of the central directory: every entry, kept or dropped, folders included.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Bytes after the end record and its comment, which were not read: 0 for a record that ends the file. A caller
    /// says so in a warning.
    pub fn bytes_after_end_record(&self) -> usize {
        self.after
    }

    /// The bytes of the file, for a copy of an entry as it is (`zipwrite.rs`).
    pub(crate) fn source(&self) -> &'a [u8] {
        self.bytes
    }

    /// Where the central directory starts, from byte 0.
    pub(crate) fn directory_start(&self) -> usize {
        self.directory_start
    }

    /// Where the bytes after an entry whose local header is at `local_offset` must end: at the next local header in the file, of any row,
    /// kept or dropped, or at the directory when there is none. One binary search of the starts the layout sorted.
    pub(crate) fn room_after(&self, local_offset: usize) -> usize {
        let next = self.starts.partition_point(|&start| start <= local_offset);
        self.starts.get(next).copied().unwrap_or(self.directory_start).min(self.directory_start)
    }

    /// What lies after the entries: the end of the rows, the directory, the end records and the comment.
    pub(crate) fn tail(&self) -> &Tail {
        &self.tail
    }

    /// The bytes of the entry at `index`, inflated within its limit and checked against its row's size and checksum (see the module
    /// documentation), and what that cost added to `work`. A folder's are none. A counted entry is added to the package total the first
    /// time it is inflated, and each inflate of it is held to what was left of the total before that. An index past the entries is
    /// `zip_corrupt`.
    pub fn inflate(&mut self, index: usize, work: &mut Work) -> Result<Vec<u8>, ContainerError> {
        let Some(entry) = self.entries.get(index) else { return Err(corrupt(format!("the archive keeps no entry {index}"))) };
        if entry.folder {
            return Ok(Vec::new());
        }
        let first_read = !self.read.get(index).copied().unwrap_or(false);
        let left = match (entry.counted, first_read) {
            (true, true) => self.max_package_bytes.saturating_sub(self.total),
            (true, false) => self.max_package_bytes.saturating_sub(self.total).saturating_add(entry.size),
            (false, _) => u64::MAX,
        };
        let data = inflate_entry(self.bytes, entry, (self.max_ratio, self.max_package_bytes), left, &mut self.scratch, work)?;
        if entry.counted && first_read {
            self.total = self.total.saturating_add(data.len() as u64);
        }
        if let Some(read) = self.read.get_mut(index) {
            *read = true;
        }
        Ok(data)
    }

    /// [`Archive::inflate`] for each of `indices`, in the order given, **on up to `threads` threads**: the same bytes, the same error and the same counts in `work` as inflating them one after
    /// another gives, at any number of threads. The limit of each entry is what [`Archive::inflate`] would hold it to, worked out beforehand from the sizes the directory states: an
    /// entry that comes out as long as its row says leaves the total of the entries before it what the sum of their sizes is, so the limit of an entry does not wait for the ones before it to be
    /// inflated. **The first entry in the order that fails is the error**, whatever order they finished in; what the entries after it cost is not counted in `work`, as they are not read by the
    /// one-at-a-time read, and their bytes are let go.
    ///
    /// **A thread reads an entry no further than the size its row states** (or its limit, when that is less): the directory was checked against the ratio, the part cap and the package total by
    /// those sizes, so what the threads hold between them is at most what a read of the package holds, and **an entry that says less than it holds cannot make every thread hold the limit's worth**
    ///. An entry that holds more than it says is read again in its turn, on the calling thread, as [`Archive::inflate`] reads
    /// it, to find the error it is (past a limit, and which, or only not the size it says) and what that costs: the error and the counts are the one-at-a-time read's. The runs that are in flight
    /// and not yet taken are held to a quarter of a gigabyte of the sizes their rows state, and each worker inflates through a buffer of its own (the same `try_reserve`s as the
    /// one-at-a-time read: memory that runs out is [`ContainerError::OutOfMemory`]).
    ///
    /// The entries are read one at a time (no thread is made) when `threads` is 1, when `indices` hold fewer than two deflated files that are worth a thread ([`PARALLEL_MIN_BYTES`]) or an entry that
    /// was inflated before, and on the web.
    pub fn inflate_many(&mut self, indices: &[usize], threads: usize, work: &mut Work) -> Result<Vec<Vec<u8>>, ContainerError> {
        self.inflate_each(indices, threads, &|_| false, work).into_iter().collect()
    }

    /// [`Archive::inflate_many`] for a caller that **goes on past some errors**, as one that inflates each entry with [`Archive::inflate`] and matches on the error does: the result of each entry
    /// in the order given, **ending with the first error that `tolerate` says no to** (that error is the last result), or with every entry. An error that is tolerated is that entry's result and the
    /// entries after it are read as they are when it is not there: a read that fails is no part of the package total, as in the one-at-a-time read. The bytes, the errors and the counts in `work`
    /// are what that loop gives at any number of threads.
    pub fn inflate_each(
        &mut self,
        indices: &[usize],
        threads: usize,
        tolerate: &(dyn Fn(&ContainerError) -> bool + Sync),
        work: &mut Work,
    ) -> Vec<Result<Vec<u8>, ContainerError>> {
        let mut out: Vec<Result<Vec<u8>, ContainerError>> = Vec::new();
        if out.try_reserve_exact(indices.len()).is_err() {
            return vec![Err(ContainerError::OutOfMemory { what: "the entries of the archive".to_owned() })];
        }
        let (jobs, worth) = self.plan_inflate(indices);
        let Some(jobs) = jobs.filter(|_| threads > 1 && !cfg!(target_arch = "wasm32") && worth >= 2) else {
            for &index in indices {
                match self.inflate(index, work) {
                    Ok(data) => out.push(Ok(data)),
                    Err(e) => {
                        let ends = !tolerate(&e);
                        out.push(Err(e));
                        if ends {
                            break;
                        }
                    }
                }
            }
            return out;
        };
        let (bytes, max_ratio, max_package) = (self.bytes, self.max_ratio, self.max_package_bytes);
        let entries = &self.entries;
        // Entries are given to the workers in runs of about a megabyte, in order, each run with one buffer: a package of thousands of small entries is a few tasks and not thousands.
        let mut runs: Vec<(&[InflateJob], usize)> = Vec::new();
        let (mut from, mut size) = (0usize, 0u64);
        for (at, job) in jobs.iter().enumerate() {
            size = size.saturating_add(job.size);
            if size >= RUN_BYTES {
                runs.push(run_of(jobs.get(from..=at).unwrap_or(&[])));
                (from, size) = (at + 1, 0);
            }
        }
        if from < jobs.len() {
            runs.push(run_of(jobs.get(from..).unwrap_or(&[])));
        }
        // Whether a piece ends the read: an error that is not tolerated, or a thread that failed. An entry that holds more than its row says is read again in its turn, and may be tolerated.
        let ends =
            |piece: &Option<Piece>| piece.as_ref().is_none_or(|p| p.iter().any(|read| matches!(read, Probed::Read(Err(e), _) if !tolerate(e))));
        let pieces: Vec<Option<Piece>> = crate::par::lane(threads, |lane: &crate::par::Lane<'_, '_, Piece>| {
            let mut pieces: Vec<Option<Piece>> = Vec::with_capacity(runs.len());
            let take = |pieces: &mut Vec<Option<Piece>>| -> Option<bool> {
                let piece = lane.take()?.ok();
                let stop = ends(&piece);
                pieces.push(piece);
                Some(stop)
            };
            let mut ended = false;
            for &(run, charge) in &runs {
                // **What the runs in flight and not taken may hold is the bytes the rows state for them**, which are what the entries are read up to (below) and, summed over the
                // package, at most the package's own limit that the directory was checked against: no more is in flight than a read one at a time ends with, and the window keeps
                // the most that is read ahead of the entry in hand to a quarter of a gigabyte.
                while !ended && lane.pending() > 0 && lane.charged().saturating_add(charge) > INFLATE_WINDOW_BYTES {
                    ended = take(&mut pieces).is_none_or(|stop| stop);
                }
                if ended {
                    break;
                }
                lane.submit(charge, move || {
                    let mut scratch: Vec<u8> = Vec::new();
                    let mut piece: Piece = Vec::with_capacity(run.len());
                    for job in run {
                        let mut mine = Work::default();
                        let read = match entries.get(job.index) {
                            Some(entry) if entry.folder => Probed::Read(Ok(Vec::new()), mine),
                            Some(entry) => {
                                let bounds = Bounds { max_ratio, max_package, left: job.left };
                                match inflate_core(bytes, entry, bounds, true, &mut scratch, &mut mine) {
                                    Ok(data) => Probed::Read(Ok(data), mine),
                                    Err(Stop::Error(e)) => Probed::Read(Err(e), mine),
                                    Err(Stop::Overrun) => Probed::Overrun,
                                }
                            }
                            None => Probed::Read(Err(corrupt(format!("the archive keeps no entry {}", job.index))), mine),
                        };
                        // The entries after one that is refused are not read: the one-at-a-time read ends there. (An entry that holds more than its row says is read again in its turn, and
                        // the caller may go on past its error: the run goes on.)
                        let stop = matches!(&read, Probed::Read(Err(e), _) if !tolerate(e));
                        piece.push(read);
                        if stop {
                            break;
                        }
                    }
                    piece
                });
            }
            while !ended {
                ended = take(&mut pieces).is_none_or(|stop| stop);
            }
            pieces
        });
        self.settle_pieces(indices, &jobs, pieces, tolerate, work)
    }

    /// The results of the pieces the workers made, **in the order of the entries**, with the counts of each in `work`, ending with the first error that `tolerate` says no to; and **the archive
    /// counts what was read, whichever way this ends** (a failure, a thread that failed, pieces that are not the entries given): a caller that tolerates an error and reads on is held to the package
    /// total the entries read before it came to.
    fn settle_pieces(
        &mut self,
        indices: &[usize],
        jobs: &[InflateJob],
        pieces: Vec<Option<Piece>>,
        tolerate: &(dyn Fn(&ContainerError) -> bool + Sync),
        work: &mut Work,
    ) -> Vec<Result<Vec<u8>, ContainerError>> {
        let (bytes, max_ratio, max_package) = (self.bytes, self.max_ratio, self.max_package_bytes);
        let mut out: Vec<Result<Vec<u8>, ContainerError>> = Vec::with_capacity(indices.len());
        let (mut scratch, mut total, mut at) = (Vec::new(), self.total, 0usize);
        'settle: for piece in pieces {
            let Some(piece) = piece else {
                // A thread that failed is no property of the file: it is the resource failure it is, which no caller goes on past (a reader skips what is `zip_corrupt`).
                out.push(Err(ContainerError::OutOfMemory { what: "the threads that inflate the entries of the archive".to_owned() }));
                break 'settle;
            };
            for read in piece {
                let Some(job) = jobs.get(at) else {
                    out.push(Err(ContainerError::OutOfMemory { what: "the entries the threads inflated, which are not the ones given".to_owned() }));
                    break 'settle;
                };
                at += 1;
                let result = match read {
                    Probed::Read(result, mine) => {
                        add_work(work, &mine);
                        result
                    }
                    // The entry holds more than its row says. The one-at-a-time read finds out what that is (past a limit, and which, or only not the size it says) by reading it to its limit, and so
                    // does this, in its turn, on this thread, with what is left of the package total at this entry: the error is its, and the work is what it counts.
                    Probed::Overrun => match self.entries.get(job.index) {
                        Some(entry) => {
                            let left = if entry.counted { max_package.saturating_sub(total) } else { u64::MAX };
                            match inflate_core(bytes, entry, Bounds { max_ratio, max_package, left }, false, &mut scratch, work) {
                                Ok(_) | Err(Stop::Overrun) => Err(corrupt(format!("{} holds more than it says", entry.name.escape_debug()))),
                                Err(Stop::Error(e)) => Err(e),
                            }
                        }
                        None => Err(corrupt(format!("the archive keeps no entry {}", job.index))),
                    },
                };
                match result {
                    Ok(data) => {
                        if self.entries.get(job.index).is_some_and(|e| e.counted) {
                            total = total.saturating_add(data.len() as u64);
                        }
                        out.push(Ok(data));
                    }
                    Err(e) => {
                        let stop = !tolerate(&e);
                        out.push(Err(e));
                        if stop {
                            break 'settle;
                        }
                    }
                }
            }
        }
        self.mark_read(indices, &out);
        out
    }

    /// The archive counts the entries that were read, as the one-at-a-time read does: each is added to the package total once, and a read that failed is not.
    fn mark_read(&mut self, indices: &[usize], results: &[Result<Vec<u8>, ContainerError>]) {
        for (&index, result) in indices.iter().zip(results) {
            let Ok(data) = result else { continue };
            if self.entries.get(index).is_some_and(|e| e.counted) {
                self.total = self.total.saturating_add(data.len() as u64);
            }
            if let Some(read) = self.read.get_mut(index) {
                *read = true;
            }
        }
    }

    /// What [`Archive::inflate_many`] needs of each entry, worked out from the directory: the limit it is held to, in the order given. `None` when an entry was inflated before or an index
    /// names no entry (the caller reads one at a time, and gets the error that way). And how many of them are files worth a thread.
    fn plan_inflate(&self, indices: &[usize]) -> (Option<Vec<InflateJob>>, usize) {
        let mut jobs = Vec::with_capacity(indices.len());
        let (mut total, mut worth) = (self.total, 0usize);
        for &index in indices {
            let Some(entry) = self.entries.get(index) else { return (None, 0) };
            if self.read.get(index).copied().unwrap_or(false) {
                return (None, 0);
            }
            if entry.folder {
                jobs.push(InflateJob { index, left: 0, size: 0 });
                continue;
            }
            let left = if entry.counted { self.max_package_bytes.saturating_sub(total) } else { u64::MAX };
            if entry.counted {
                total = total.saturating_add(entry.size);
            }
            // **Only an entry that is deflated is worth a thread**: a stored one is copied and summed at the speed of memory, and the threads cost more than they save (measured: eight stored
            // pictures of 16 MiB were read in 368 ms on one thread and 476 on four).
            if entry.size >= PARALLEL_MIN_BYTES && entry.compression() == Some(Method::Deflated) {
                worth += 1;
            }
            jobs.push(InflateJob { index, left, size: entry.size });
        }
        (Some(jobs), worth)
    }
}

/// What a thread made of one job of [`Archive::inflate_into_many`]: whether the buffer was filled, the buffer, and what it cost.
type Filled = (Result<(), ContainerError>, Arc<[u8]>, Work);

/// A buffer for [`Archive::inflate_into_many`] to fill with an entry, of exactly the size the entry states and held by no one else.
pub struct IntoJob {
    pub index: usize,
    pub buffer: Arc<[u8]>,
}

/// What [`Archive::inflate_into_many`] made of one entry: the buffer filled, or the error, and what it cost.
pub struct IntoDone {
    pub index: usize,
    pub result: Result<Arc<[u8]>, ContainerError>,
    pub work: Work,
}

/// The body of [`Archive::inflate_into`]: `entry` inflated into `out` (of the size it states), held to `left` bytes of the package total, with every check of the row. It counts nothing.
fn inflate_into_entry(
    source: &[u8],
    entry: &Entry,
    (max_ratio, max_package): (u64, u64),
    left: u64,
    out: &mut [u8],
    work: &mut Work,
) -> Result<(), ContainerError> {
    let name = entry.name.as_str();
    let wrong_size = || corrupt(format!("the buffer for {} is not the {} bytes the archive states", name.escape_debug(), entry.size));
    if entry.folder {
        return if out.is_empty() { Ok(()) } else { Err(wrong_size()) };
    }
    if u64::try_from(out.len()).ok() != Some(entry.size) {
        return Err(wrong_size());
    }
    let ratio_limit = max_ratio.saturating_mul(entry.compressed);
    let limit = entry.cap.min(ratio_limit).min(left);
    if entry.size > limit {
        // Refused before a byte is read: what `inflate` would find after reading one past the limit, in the same order of classes.
        return Err(if ratio_limit <= entry.cap && ratio_limit <= left {
            ContainerError::ZipRatioExceeded { name: name.to_owned(), limit: max_ratio }
        } else if entry.cap <= left {
            ContainerError::PartTooLarge { name: name.to_owned(), limit: entry.cap }
        } else {
            ContainerError::PackageTooLarge { limit: max_package }
        });
    }
    let crc = if entry.size == 0 && entry.compressed == 0 {
        0
    } else {
        let start = entry.data_offset;
        let raw = usize::try_from(entry.compressed).ok().and_then(|c| start.checked_add(c)).and_then(|end| source.get(start..end));
        let Some(raw) = raw else { return Err(corrupt(format!("the data of {} is not in the file", name.escape_debug()))) };
        match entry.compression() {
            Some(Method::Stored) => fill(raw, out, name, work)?,
            Some(Method::Deflated) => fill(flate2::read::DeflateDecoder::new(raw), out, name, work)?,
            None => return Err(ContainerError::UnsupportedCompression { name: name.to_owned(), method: entry.method.to_string() }),
        }
        crc32fast::hash(out)
    };
    if crc != entry.crc {
        return Err(corrupt(format!("the checksum of {} is not the one the archive states", name.escape_debug())));
    }
    Ok(())
}

/// A run of entries and the bytes their rows state, as the charge of the run.
fn run_of(run: &[InflateJob]) -> (&[InflateJob], usize) {
    let claimed = run.iter().fold(0u64, |sum, job| sum.saturating_add(job.size));
    (run, usize::try_from(claimed).unwrap_or(usize::MAX))
}

/// An entry [`Archive::inflate_many`] gives to a worker: its place in the archive, the room the package total leaves it, and the size its row states.
struct InflateJob {
    index: usize,
    left: u64,
    size: u64,
}

/// What a worker made of one entry: its bytes or the error it was refused with, and what it cost; or that it holds more than its row says.
enum Probed {
    Read(Result<Vec<u8>, ContainerError>, Work),
    Overrun,
}

/// What a worker made of a run of entries, in order, up to the first that was not read.
type Piece = Vec<Probed>;

/// The limits an entry is read within: the ratio and the package total of the archive, and what is left of the total for this entry.
#[derive(Clone, Copy)]
struct Bounds {
    max_ratio: u64,
    max_package: u64,
    left: u64,
}

/// Why [`inflate_core`] did not give an entry's bytes.
enum Stop {
    Error(ContainerError),
    /// The entry holds more than its row says (only when asked to read no further than that).
    Overrun,
}

/// The smallest deflated entry (by the size its row states) that is worth a thread of its own: below this, inflating takes less than starting a thread.
pub const PARALLEL_MIN_BYTES: u64 = 64 * 1024;

/// About how many bytes of entries one task of [`Archive::inflate_many`] inflates, so that a package of many small entries is a few tasks.
const RUN_BYTES: u64 = 1024 * 1024;

/// The most bytes that the runs of [`Archive::inflate_many`] that are in flight and not yet taken may state for their entries: a quarter of a gigabyte. A run larger than this is read alone.
const INFLATE_WINDOW_BYTES: usize = 256 * 1024 * 1024;

/// Adds the counts of `other` to `work`: every field that is a count of the work an entry's inflate does.
fn add_work(work: &mut Work, other: &Work) {
    work.inflated = work.inflated.saturating_add(other.inflated);
}

/// The bytes of `entry` inflated within its limit and checked against its row: the body of [`Archive::inflate`], which holds the entry to `left` bytes of the package total, and keeps the
/// running total itself.
fn inflate_entry(
    source: &[u8],
    entry: &Entry,
    (max_ratio, max_package): (u64, u64),
    left: u64,
    scratch: &mut Vec<u8>,
    work: &mut Work,
) -> Result<Vec<u8>, ContainerError> {
    match inflate_core(source, entry, Bounds { max_ratio, max_package, left }, false, scratch, work) {
        Ok(data) => Ok(data),
        Err(Stop::Error(e)) => Err(e),
        // Only a read that stops at the row's size can overrun it.
        Err(Stop::Overrun) => Err(corrupt(format!("{} holds more than it says", entry.name.escape_debug()))),
    }
}

/// [`inflate_entry`]'s work. **`probe` reads the entry no further than the size its row states** (and the limit, when that is less): **a thread that reads an entry for another to use holds no
/// more of it than its row says**, which the directory checked against the ratio, the part cap and the package total, and not what the entry's bytes make of the limit, which an entry that says
/// less than it holds could use to hold the limit's worth on every thread. An entry that holds no more than its row says is the bytes and the checks of the one-at-a-time read; one that holds
/// more is [`Stop::Overrun`], and the caller reads it as the one-at-a-time read does, to find which error it is.
fn inflate_core(source: &[u8], entry: &Entry, bounds: Bounds, probe: bool, scratch: &mut Vec<u8>, work: &mut Work) -> Result<Vec<u8>, Stop> {
    let Bounds { max_ratio, max_package, left } = bounds;
    let name = entry.name.as_str();
    let ratio_limit = max_ratio.saturating_mul(entry.compressed);
    let limit = entry.cap.min(ratio_limit).min(left);
    let read_limit = if probe { limit.min(entry.size) } else { limit };
    let (data, crc) = if entry.size == 0 && entry.compressed == 0 {
        // An entry with no data at all is an empty part, whichever method it names: some writers give an empty entry no stream,
        // where others give it two bytes. Its checksum is still checked, below, and an empty one is 0.
        (Vec::new(), 0)
    } else {
        let start = entry.data_offset;
        let raw = usize::try_from(entry.compressed).ok().and_then(|c| start.checked_add(c)).and_then(|end| source.get(start..end));
        let Some(raw) = raw else { return Err(Stop::Error(corrupt(format!("the data of {} is not in the file", name.escape_debug())))) };
        if scratch.is_empty() {
            *scratch = vec![0u8; SCRATCH_BYTES];
        }
        let read = match entry.compression() {
            Some(Method::Stored) => read_bounded(raw, read_limit, entry.size, name, scratch, work),
            Some(Method::Deflated) => read_bounded(flate2::read::DeflateDecoder::new(raw), read_limit, entry.size, name, scratch, work),
            None => Err(ContainerError::UnsupportedCompression { name: name.to_owned(), method: entry.method.to_string() }),
        };
        read.map_err(Stop::Error)?
    };
    let length = data.len() as u64;
    if length > read_limit {
        if probe {
            return Err(Stop::Overrun);
        }
        // The same order as the checks of the headers: the ratio, then the part, then the package.
        return Err(Stop::Error(if ratio_limit <= entry.cap && ratio_limit <= left {
            ContainerError::ZipRatioExceeded { name: name.to_owned(), limit: max_ratio }
        } else if entry.cap <= left {
            ContainerError::PartTooLarge { name: name.to_owned(), limit: entry.cap }
        } else {
            ContainerError::PackageTooLarge { limit: max_package }
        }));
    }
    if length != entry.size {
        return Err(Stop::Error(corrupt(format!("{} says it is {} bytes and is {length}", name.escape_debug(), entry.size))));
    }
    if crc != entry.crc {
        return Err(Stop::Error(corrupt(format!("the checksum of {} is not the one the archive states", name.escape_debug()))));
    }
    Ok(data)
}

impl Archive<'_> {
    /// [`Archive::inflate`], into a buffer the caller made of **exactly the size the archive states for the entry**, so that the bytes are made once: a caller that wants them in
    /// an allocation of its own (an `Arc<[u8]>`) makes it of that size and gives it here, where `inflate` makes a `Vec` that a conversion then copies, which holds an entry twice
    /// while it is converted. A buffer of another length is `zip_corrupt` (and nothing is read); every limit, check and count of `inflate` holds, and a stream that
    /// is shorter or longer than its size or whose checksum is not the entry's is `zip_corrupt` as there. What is in `out` after an error is not the entry's bytes.
    pub fn inflate_into(&mut self, index: usize, out: &mut [u8], work: &mut Work) -> Result<(), ContainerError> {
        let Some(entry) = self.entries.get(index) else { return Err(corrupt(format!("the archive keeps no entry {index}"))) };
        let first_read = !self.read.get(index).copied().unwrap_or(false);
        let left = self.left_for(entry, first_read);
        inflate_into_entry(self.bytes, entry, (self.max_ratio, self.max_package_bytes), left, out, work)?;
        self.count_read(index);
        Ok(())
    }

    /// What is left of the package total for `entry`, as [`Archive::inflate`] holds it to.
    fn left_for(&self, entry: &Entry, first_read: bool) -> u64 {
        match (entry.counted, first_read) {
            (true, true) => self.max_package_bytes.saturating_sub(self.total),
            (true, false) => self.max_package_bytes.saturating_sub(self.total).saturating_add(entry.size),
            (false, _) => u64::MAX,
        }
    }

    /// Counts the entry at `index` as read, **as [`Archive::inflate_into`] counts a read that passed**: added to the package total once, however many times it is read. For a caller that had the
    /// entry read by [`Archive::inflate_into_many`], which counts nothing, and counts each in the order it uses them in, as it would have by reading them one at a time.
    pub fn count_read(&mut self, index: usize) {
        let first_read = !self.read.get(index).copied().unwrap_or(false);
        if first_read && self.entries.get(index).is_some_and(|e| e.counted) {
            let size = self.entries.get(index).map_or(0, |e| e.size);
            self.total = self.total.saturating_add(size);
        }
        if let Some(read) = self.read.get_mut(index) {
            *read = true;
        }
    }

    /// [`Archive::inflate_into`] for each of `jobs`, **on up to `threads` threads**, into the buffers the caller made (each of the size its entry states: nothing is allocated for the bytes here, so
    /// what the threads hold is what the caller made and no more). The results are in the order of the jobs, **whatever the order they finished in, and none ends the others**; each is the
    /// buffer filled or the error that `inflate_into` gives, with what it cost. **The archive counts nothing** (the package total and the entries read are the caller's to count, with
    /// [`Archive::count_read`], in the order it uses the entries in): each entry is held to what is left of the total now, which the directory's check of the sizes makes at least its size, so no
    /// order the caller counts them in changes what an entry may inflate to. A thread that fails is the error of its entry (`out_of_memory`, which no reader skips).
    pub fn inflate_into_many(&self, jobs: Vec<IntoJob>, threads: usize) -> Vec<IntoDone> {
        let (bytes, limits) = (self.bytes, (self.max_ratio, self.max_package_bytes));
        let entries = &self.entries;
        let lefts: Vec<u64> = jobs
            .iter()
            .map(|job| match entries.get(job.index) {
                Some(entry) => self.left_for(entry, !self.read.get(job.index).copied().unwrap_or(false)),
                None => 0,
            })
            .collect();
        let indices: Vec<usize> = jobs.iter().map(|job| job.index).collect();
        let workers = if cfg!(target_arch = "wasm32") { 0 } else { threads };
        let results: Vec<Result<Filled, crate::par::Failed>> = crate::par::lane(workers, |lane: &crate::par::Lane<'_, '_, Filled>| {
            for (job, left) in jobs.into_iter().zip(lefts) {
                let charge = job.buffer.len();
                lane.submit(charge, move || {
                    let IntoJob { index, mut buffer } = job;
                    let mut work = Work::default();
                    let result = match (entries.get(index), Arc::get_mut(&mut buffer)) {
                        (Some(entry), Some(out)) => inflate_into_entry(bytes, entry, limits, left, out, &mut work),
                        (None, _) => Err(corrupt(format!("the archive keeps no entry {index}"))),
                        (Some(_), None) => Err(corrupt("a buffer that was just made is shared".to_owned())),
                    };
                    (result, buffer, work)
                });
            }
            let mut results = Vec::new();
            while let Some(result) = lane.take() {
                results.push(result);
            }
            results
        });
        indices
            .into_iter()
            .zip(results)
            .map(|(index, result)| match result {
                Ok((Ok(()), buffer, work)) => IntoDone { index, result: Ok(buffer), work },
                Ok((Err(e), _, work)) => IntoDone { index, result: Err(e), work },
                Err(_) => IntoDone {
                    index,
                    result: Err(ContainerError::OutOfMemory { what: "the threads that inflate the entries of the archive".to_owned() }),
                    work: Work::default(),
                },
            })
            .collect()
    }

    /// Whether the entry at `index` holds the bytes `known`, **checked without a copy of them being made**: for a caller that has the bytes an entry should hold
    /// already (a writer that reads back the file it made from them), so that the check of the file does not hold them a second time.
    ///
    /// `Ok(false)`, with nothing read, when the row states another size or another checksum than those of `known` (the caller inflates the entry, as it would have). `Ok(true)` when the row
    /// states the size and the checksum of `known` and the entry's data, read through the archive's buffer and kept nowhere, comes to that size and that checksum: every limit, check and count
    /// of [`Archive::inflate`] holds (the entry is held to its cap, its ratio and the package total, and counted toward the total as an inflate counts it). An entry that is damaged is the
    /// error `inflate` gives for it. A folder holds nothing: `Ok(false)`.
    pub fn holds(&mut self, index: usize, known: &[u8], work: &mut Work) -> Result<bool, ContainerError> {
        let Some(entry) = self.entries.get(index) else { return Err(corrupt(format!("the archive keeps no entry {index}"))) };
        if entry.folder || u64::try_from(known.len()).ok() != Some(entry.size) || crc32fast::hash(known) != entry.crc {
            return Ok(false);
        }
        let first_read = !self.read.get(index).copied().unwrap_or(false);
        let name = entry.name.as_str();
        let left = match (entry.counted, first_read) {
            (true, true) => self.max_package_bytes.saturating_sub(self.total),
            (true, false) => self.max_package_bytes.saturating_sub(self.total).saturating_add(entry.size),
            (false, _) => u64::MAX,
        };
        let ratio_limit = self.max_ratio.saturating_mul(entry.compressed);
        let limit = entry.cap.min(ratio_limit).min(left);
        let (length, crc) = if entry.size == 0 && entry.compressed == 0 {
            (0, 0)
        } else {
            let start = entry.data_offset;
            let raw = usize::try_from(entry.compressed).ok().and_then(|c| start.checked_add(c)).and_then(|end| self.bytes.get(start..end));
            let Some(raw) = raw else { return Err(corrupt(format!("the data of {} is not in the file", name.escape_debug()))) };
            if self.scratch.is_empty() {
                self.scratch = vec![0u8; SCRATCH_BYTES];
            }
            match entry.compression() {
                Some(Method::Stored) => stream_bounded(raw, limit, name, &mut self.scratch, work)?,
                Some(Method::Deflated) => stream_bounded(flate2::read::DeflateDecoder::new(raw), limit, name, &mut self.scratch, work)?,
                None => return Err(ContainerError::UnsupportedCompression { name: name.to_owned(), method: entry.method.to_string() }),
            }
        };
        if length > limit {
            // The same order as the checks of the headers and of `inflate`: the ratio, then the part, then the package.
            return Err(if ratio_limit <= entry.cap && ratio_limit <= left {
                ContainerError::ZipRatioExceeded { name: name.to_owned(), limit: self.max_ratio }
            } else if entry.cap <= left {
                ContainerError::PartTooLarge { name: name.to_owned(), limit: entry.cap }
            } else {
                ContainerError::PackageTooLarge { limit: self.max_package_bytes }
            });
        }
        if length != entry.size {
            return Err(corrupt(format!("{} says it is {} bytes and is {length}", name.escape_debug(), entry.size)));
        }
        if crc != entry.crc {
            return Err(corrupt(format!("the checksum of {} is not the one the archive states", name.escape_debug())));
        }
        if entry.counted && first_read {
            self.total = self.total.saturating_add(length);
        }
        if let Some(read) = self.read.get_mut(index) {
            *read = true;
        }
        Ok(true)
    }
}

/// [`read_bounded`] keeping nothing: reads `source` to its end, or to one byte past `limit`, through `chunk`, and gives how many bytes came out and their CRC-32.
fn stream_bounded(mut source: impl Read, limit: u64, name: &str, chunk: &mut [u8], work: &mut Work) -> Result<(u64, u32), ContainerError> {
    let mut hasher = crc32fast::Hasher::new();
    let stop = limit.saturating_add(1);
    let mut length = 0u64;
    while length < stop {
        let want = usize::try_from(stop - length).unwrap_or(usize::MAX).min(chunk.len());
        let read = match source.read(chunk.get_mut(..want).unwrap_or_default()) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(corrupt(format!("{} cannot be read: {e}", name.escape_debug()))),
        };
        let got = chunk.get(..read).unwrap_or_default();
        hasher.update(got);
        Work::add(&mut work.inflated, got.len());
        length += got.len() as u64;
    }
    Ok((length, hasher.finalize()))
}

/// Reads `source` into `out`, which is the size the entry states: damage if it ends short, or has one more byte (the byte past is counted, as `read_bounded` counts it).
fn fill(mut source: impl Read, out: &mut [u8], name: &str, work: &mut Work) -> Result<(), ContainerError> {
    let mut filled = 0usize;
    while filled < out.len() {
        let Some(rest) = out.get_mut(filled..) else { break };
        match source.read(rest) {
            Ok(0) => break,
            Ok(n) => {
                filled += n;
                Work::add(&mut work.inflated, n);
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(corrupt(format!("{} cannot be read: {e}", name.escape_debug()))),
        }
    }
    let mut probe = [0u8; 1];
    let more = loop {
        match source.read(&mut probe) {
            Ok(n) => break n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(corrupt(format!("{} cannot be read: {e}", name.escape_debug()))),
        }
    };
    if more > 0 {
        Work::add(&mut work.inflated, more);
        return Err(corrupt(format!("{} says it is {} bytes and is more", name.escape_debug(), out.len())));
    }
    if filled != out.len() {
        return Err(corrupt(format!("{} says it is {} bytes and is {filled}", name.escape_debug(), out.len())));
    }
    Ok(())
}

impl std::fmt::Debug for Archive<'_> {
    /// The entries and the counts, not the bytes of the file.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Archive").field("entries", &self.entries).field("rows", &self.rows).field("after", &self.after).finish()
    }
}

/// How many rows the directory of the archive in `bytes` has, **by the rules [`Archive::read_with`] reads it by** (the end record that fits the file, and the ZIP64 count when the
/// record says so): nothing of the directory is read, and one backward scan of at most 65,557 bytes is made. A caller that sizes what the read will hold by the rows (a budget of
/// memory) asks this and not a rule of its own, since a file can hold a second end record where a rule of its own would look. The count has been shown to fit in the directory it
/// gives (46 bytes a row at least), so what is charged for it is what the read would make. The error is the one the read would give.
pub fn directory_rows(bytes: &[u8]) -> Result<u64, ContainerError> {
    zipdir::locate(bytes, &mut Work::default()).map(|plan| plan.count)
}

/// What lies in an archive after its entries, from byte 0: what a copy of every entry (`zipwrite.rs`) writes anew or leaves out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tail {
    /// Where the last row the count allows ends; the directory's own bytes run on to `directory_end`.
    pub rows_end: usize,
    pub directory_end: usize,
    /// The ZIP64 end record, when it was read; its locator follows it, and the plain end record follows that.
    pub zip64_record: Option<usize>,
    pub end_record: usize,
    /// The bytes of the end record's comment.
    pub comment: usize,
}

/// What pass 1 gives: the entries kept, where every row's local header is and how many bytes its data takes (dropped rows and folders
/// included), for the layout, and where the last row ends.
struct Listing {
    entries: Vec<Entry>,
    locals: Vec<(usize, u64)>,
    rows_end: usize,
}

/// Pass 1: every row's name and headers, judged before anything is inflated. Gives the entries kept and where every row's entry is.
fn list_entries<J: Judge>(bytes: &[u8], plan: &Plan, limits: &Limits, judge: &mut J, work: &mut Work) -> Result<Listing, J::Error> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut locals: Vec<(usize, u64)> = Vec::new();
    let (mut parts, mut declared_total) = (0usize, 0u64);
    let mut rows = Rows::new(bytes, plan, limits.max_part_name_bytes, work);
    for row in &mut rows {
        let row = row?;
        locals.push((row.local_offset, row.compressed));
        let entry = Entry {
            name: row.name,
            written: row.written,
            folder: row.folder,
            flags: row.flags,
            method: row.method,
            crc: row.crc,
            compressed: row.compressed,
            size: row.declared,
            local_offset: row.local_offset,
            data_offset: 0,
            local_extra_len: 0,
            cap: 0,
            counted: false,
            version_made_by: row.made_by,
            version_needed: row.needed,
            time: row.time,
            date: row.date,
            internal_attributes: row.internal,
            external_attributes: row.external,
            row_offset: row.row_offset,
            row_len: row.row_len,
        };
        let verdict = judge.entry(&entry, &entries)?;
        if entry.folder {
            // A folder has no data. A flipped bit turns `logo` into `log/`, and its data would be dropped; an empty deflate stream (two
            // bytes) is no data, so it is the uncompressed size that says.
            if entry.size != 0 {
                return Err(
                    corrupt(format!("the folder {} holds {} bytes of data, and a folder has none", entry.name.escape_debug(), entry.size)).into()
                );
            }
            if let Verdict::Keep { rename, .. } = verdict {
                entries.push(Entry { name: rename.unwrap_or(entry.name), written: None, ..entry });
            }
            continue;
        }
        let Verdict::Keep { rename, cap, counted } = verdict else { continue };
        if counted {
            parts += 1;
            if parts > limits.max_parts {
                return Err(ContainerError::TooManyParts { limit: limits.max_parts }.into());
            }
        }
        if entry.flags & FLAGS_ENCRYPTED != 0 {
            return Err(ContainerError::EncryptedPackage { name: entry.name }.into());
        }
        if entry.compression().is_none() {
            return Err(ContainerError::UnsupportedCompression { name: entry.name, method: entry.method.to_string() }.into());
        }
        let (declared, compressed) = (entry.size, entry.compressed);
        // The ratio first (a claim of 2 GiB from 1 MiB is a ratio, though it is also over the part limit), then the sizes.
        if u128::from(declared) > u128::from(limits.max_ratio) * u128::from(compressed) {
            return Err(ContainerError::ZipRatioExceeded { name: entry.name, limit: limits.max_ratio }.into());
        }
        if declared > cap {
            return Err(ContainerError::PartTooLarge { name: entry.name, limit: cap }.into());
        }
        if counted {
            declared_total = declared_total.saturating_add(declared);
            if declared_total > limits.max_package_bytes {
                return Err(ContainerError::PackageTooLarge { limit: limits.max_package_bytes }.into());
            }
        }
        entries.push(Entry { name: rename.unwrap_or(entry.name), written: None, cap, counted, ..entry });
    }
    // The count was met; what is left of the directory must not be more rows (a count below the truth is damage, not a smaller package).
    rows.finish()?;
    let rows_end = rows.position();
    judge.listed(&entries, work)?;
    Ok(Listing { entries, locals, rows_end })
}

/// Pass 2: where every row's entry sits, local header and data, and that no two share a byte and none reaches the directory. A sort
/// of one range per row, so the time is that of a sort, however the rows are placed. Gives where each local header is, in the order of
/// the file.
fn check_layout(bytes: &[u8], locals: &[(usize, u64)], directory_start: usize, work: &mut Work) -> Result<Vec<usize>, ContainerError> {
    let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(locals.len());
    for &(local_offset, compressed) in locals {
        let data = zipdir::local_header(bytes, local_offset, work)?.data;
        let end = usize::try_from(compressed).ok().and_then(|c| data.checked_add(c)).filter(|&end| end <= directory_start);
        let Some(end) = end else { return Err(corrupt("an entry's data runs into the central directory or past the file")) };
        ranges.push((local_offset, end));
    }
    ranges.sort_unstable();
    for pair in ranges.windows(2) {
        if let [(_, earlier_end), (later_start, _)] = pair
            && earlier_end > later_start
        {
            return Err(corrupt("entries overlap: they share bytes of the file"));
        }
    }
    Ok(ranges.into_iter().map(|(start, _)| start).collect())
}

/// Reads `source` to its end, or to one byte past `limit` (so that the caller can tell a part at the limit from one over it), and
/// gives the bytes and their CRC-32, reading through `chunk`. The buffer is reserved from the entry's own claim of its size, which has
/// passed the ratio and size checks and so is bounded, and grows past it only if the entry holds more than it said; every growth is a
/// `try_reserve`.
fn read_bounded(
    mut source: impl Read,
    limit: u64,
    claimed: u64,
    name: &str,
    chunk: &mut [u8],
    work: &mut Work,
) -> Result<(Vec<u8>, u32), ContainerError> {
    let out_of_memory = || ContainerError::OutOfMemory { what: name.to_owned() };
    let mut data: Vec<u8> = Vec::new();
    let first = usize::try_from(claimed.min(limit)).map_err(|_| out_of_memory())?;
    data.try_reserve_exact(first).map_err(|_| out_of_memory())?;
    let mut hasher = crc32fast::Hasher::new();
    let stop = limit.saturating_add(1);
    let mut length = 0u64;
    while length < stop {
        let want = usize::try_from(stop - length).unwrap_or(usize::MAX).min(chunk.len());
        let read = match source.read(chunk.get_mut(..want).unwrap_or_default()) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(corrupt(format!("{} cannot be read: {e}", name.escape_debug()))),
        };
        let got = chunk.get(..read).unwrap_or_default();
        data.try_reserve(got.len()).map_err(|_| out_of_memory())?;
        data.extend_from_slice(got);
        hasher.update(got);
        Work::add(&mut work.inflated, got.len());
        length += got.len() as u64;
    }
    Ok((data, hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zip::ArchiveWriter;

    fn archive(count: usize) -> Vec<u8> {
        let mut writer = ArchiveWriter::new();
        for k in 0..count {
            writer.add(&format!("p{k}.bin"), &vec![k as u8; 1_000 + k], Method::Stored).unwrap();
        }
        writer.finish().unwrap()
    }

    fn piece_of(parts: &[Vec<u8>]) -> Option<Piece> {
        Some(parts.iter().map(|data| Probed::Read(Ok(data.clone()), Work::default())).collect())
    }

    /// The jobs a plan makes for entries `0..count`, and the sums of what is read of them.
    fn settle(file: &[u8], count: usize, pieces: Vec<Option<Piece>>) -> (Archive<'_>, Vec<Result<Vec<u8>, ContainerError>>) {
        let mut archive = Archive::read(file, &crate::Limits::default()).unwrap();
        let indices: Vec<usize> = (0..count).collect();
        let (jobs, _) = archive.plan_inflate(&indices);
        let jobs = jobs.unwrap();
        // A caller that goes on past `zip_corrupt`, as the readers do.
        let tolerate = |e: &ContainerError| matches!(e, ContainerError::ZipCorrupt { .. });
        let out = archive.settle_pieces(&indices, &jobs, pieces, &tolerate, &mut Work::default());
        (archive, out)
    }

    #[test]
    fn a_thread_that_failed_ends_the_results_with_an_error_no_caller_goes_on_past_and_what_was_read_before_it_is_counted() {
        let file = archive(5);
        let first = piece_of(&[vec![0u8; 1_000], vec![1u8; 1_001]]);
        let (archive, out) = settle(&file, 5, vec![first, None, piece_of(&[vec![2u8; 1_002]])]);
        assert_eq!(out.len(), 3, "the results end at the thread that failed");
        assert!(out[0].is_ok() && out[1].is_ok());
        let error = out[2].as_ref().unwrap_err();
        assert_eq!(error.class(), "out_of_memory", "a thread that failed is not damage in the file, which a reader would skip: {error}");
        assert_eq!(archive.total, 2_001, "the package total counts the entries read before the failure");
        assert!(archive.read[0] && archive.read[1] && !archive.read[2]);
    }

    #[test]
    fn pieces_that_are_not_the_entries_given_end_the_results_alike_and_count_what_was_read() {
        let file = archive(5);
        // Three entries given and a piece that holds four reads: the fourth is no entry given.
        let (archive, out) = settle(&file, 3, vec![piece_of(&[vec![0u8; 1_000], vec![1u8; 1_001], vec![2u8; 1_002], vec![3u8; 1_003]])]);
        assert_eq!(out.len(), 4);
        assert_eq!(out[3].as_ref().unwrap_err().class(), "out_of_memory");
        assert_eq!(archive.total, 3_003);
    }

    #[test]
    fn a_tolerated_error_after_entries_that_were_read_leaves_them_counted_and_the_read_goes_on() {
        let file = archive(5);
        let damaged: Option<Piece> = Some(vec![Probed::Read(Err(corrupt("an entry that is damaged".to_owned())), Work::default())]);
        let mut pieces = vec![piece_of(&[vec![0u8; 1_000], vec![1u8; 1_001]]), damaged];
        pieces.push(piece_of(&[vec![3u8; 1_003]]));
        let (archive, out) = settle(&file, 4, pieces);
        assert_eq!(out.len(), 4, "the read went on past the damaged entry");
        assert_eq!(archive.total, 1_000 + 1_001 + 1_003, "the damaged entry is no part of the total, the others are");
    }
}
