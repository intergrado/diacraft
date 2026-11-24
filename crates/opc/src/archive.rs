//! The package's archive, read through the container crate with OPC's rules for its rows: which rows are parts, which is the content
//! types stream, and whether their names are part names.
//!
//! The archive itself (the end record, the ZIP64 records, the rows, the count, encryption, the method, the ratio and the caps, the layout,
//! and inflating within the limits) is `diacraft-container`'s, which asks this module's [`Opc`] judge about each row as it reads it and
//! about the whole list after the last row, before the layout is read. So the order of the checks, and the error a file with several
//! faults gives, is the order the reader always had:
//!
//! 1. **For each row, in order**: its name (refused as written and as a Unicode path field gives it, by the same rules), and a
//!    folder's data (a folder has none); a part's name as a part name, duplicates (over the names as written as well),
//!    the count of parts; then the container's encryption, method, ratio, cap and total. The content types stream is told by its
//!    name as written and is no part: it has its own cap and is not counted.
//! 2. **After the last row**: the directory is used up (the container's), folders past the first hundred are counted in one warning,
//!    the stream is there (`missing_content_types`), and no name is the path prefix of another.
//! 3. **Then the layout, then every entry inflated in order** (the container's).
//!
//! A folder is dropped, with a warning naming the first hundred: it is not a part, and its name, as an OPC folder name, is
//! still judged. Since a folder is told by the name *as written* ending in a slash, a Unicode path field cannot make a file a
//! folder and so drop it.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::hash_map::Entry as MapEntry;
use std::sync::Arc;

use diacraft_container::Work;
use diacraft_container::zip::{Archive, Entry, Judge, Verdict};

use crate::content_types::CONTENT_TYPES_ITEM;
use crate::error::{OpcError, Warning};
use crate::limits::Limits;
use crate::name::{check_folder_name, fold, from_item_name, is_rels_part};

/// The signature of a compound file (old Office files, and encrypted packages): R22 `ole_with_package_extension`.
const OLE_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// How many directory items are named in a [`Warning`] each; the rest are counted in one. The items are dropped either way: this
/// bounds what a directory of nothing but folders can make the reader keep.
const MAX_DIRECTORY_WARNINGS: usize = 100;

/// The bytes of a part: inflated by this read, or the caller's own, which the entry was checked to hold ([`read_archive`]'s `known`).
pub(crate) enum PartData {
    Own(Vec<u8>),
    Shared(Arc<[u8]>),
}

impl PartData {
    pub(crate) fn bytes(&self) -> &[u8] {
        match self {
            PartData::Own(bytes) => bytes,
            PartData::Shared(bytes) => bytes,
        }
    }
}

/// What a caller has of the bytes some parts should hold, by part name (canonical, with its slash): see [`crate::Package::read_knowing`].
pub(crate) type Known<'k> = &'k dyn Fn(&str) -> Option<Arc<[u8]>>;

/// Part names with their bytes.
pub(crate) type NamedParts = Vec<(String, PartData)>;

/// The inflated bytes of an archive: the parts in archive order, and the content types stream.
pub(crate) struct Raw {
    /// `(part name, bytes)`, part names in canonical form, in the order of the central directory.
    pub parts: NamedParts,
    pub content_types: Vec<u8>,
    pub warnings: Vec<Warning>,
    /// Rows of the central directory: parts, folders and the content types stream.
    pub rows: usize,
}

/// Reads `bytes` as the ZIP archive of a package within `limits`. **A part `known` gives bytes for is not inflated when its entry holds them** ([`Archive::holds`]: the row states their
/// size and checksum, and the entry's data, read and kept nowhere, comes to both): the part is those bytes, shared with the caller. Any other part, and one whose entry holds other
/// bytes, is inflated as always.
pub(crate) fn read_archive(bytes: &[u8], limits: &Limits, known: Option<Known<'_>>, threads: usize) -> Result<Raw, OpcError> {
    if bytes.starts_with(&OLE_SIGNATURE) {
        return Err(OpcError::OleWithPackageExtension);
    }
    let mut judge = Opc::new(limits);
    let mut work = Work::default();
    let mut archive = Archive::read_with(bytes, &limits.container(), &mut judge, &mut work)?;
    // The judge keeps one entry it does not count, the content types stream, and refuses a second as the row it is, with both names as
    // written. Were a second ever kept, it is refused here all the same, before anything is inflated: one stream is never read in place
    // of another.
    let mut streams = archive.entries().iter().filter(|e| !e.counted);
    if streams.next().is_none() {
        return Err(OpcError::MissingContentTypes);
    }
    if streams.next().is_some() {
        return Err(OpcError::DuplicatePart { first: CONTENT_TYPES_ITEM.to_owned(), second: CONTENT_TYPES_ITEM.to_owned() });
    }
    let mut warnings = Vec::new();
    if archive.bytes_after_end_record() > 0 {
        // Bytes after the end record and its comment are not read, and are said to be there.
        warnings.push(Warning::BytesAfterEndRecord { count: archive.bytes_after_end_record() });
    }
    // The table of names goes before a byte is inflated.
    warnings.extend(judge.into_warnings());
    let rows = archive.rows();
    let mut data: Vec<PartData> = Vec::with_capacity(archive.entries().len());
    if known.is_none() && threads > 1 {
        // Every entry in the order of the directory, on threads: the same bytes, the same counts and the same first error as one at a time.
        let indices: Vec<usize> = (0..archive.entries().len()).collect();
        data.extend(archive.inflate_many(&indices, threads, &mut work)?.into_iter().map(PartData::Own));
    } else {
        for index in 0..archive.entries().len() {
            // Only a part is looked up (the content types stream is not counted, and is always read).
            let shared = match (known, archive.entries().get(index)) {
                (Some(known), Some(entry)) if entry.counted && !entry.folder => known(&entry.name),
                _ => None,
            };
            let part = match shared {
                Some(bytes) if archive.holds(index, &bytes, &mut work)? => PartData::Shared(bytes),
                _ => PartData::Own(archive.inflate(index, &mut work)?),
            };
            data.push(part);
        }
    }
    let entries = archive.into_entries();
    let mut parts: NamedParts = Vec::with_capacity(entries.len().min(limits.max_parts));
    let mut content_types = Vec::new();
    for (entry, bytes) in entries.into_iter().zip(data) {
        // The judge counts every part and nothing else: the one entry kept and not counted (one, as checked above) is the content types stream.
        if entry.counted {
            parts.push((entry.name, bytes));
        } else {
            content_types = match bytes {
                PartData::Own(bytes) => bytes,
                PartData::Shared(bytes) => bytes.to_vec(),
            };
        }
    }
    Ok(Raw { parts, content_types, warnings, rows })
}

/// OPC's rules for the rows of the archive. See the module documentation.
struct Opc<'l> {
    limits: &'l Limits,
    /// Folded part name, as written or as a Unicode path field gives it, to the index of the entry that has it. A row with no field has
    /// one name and one key: the name is not kept twice.
    seen: HashMap<String, usize>,
    /// The name of the content types stream's row, once one is read.
    content_types_item: Option<String>,
    warnings: Vec<Warning>,
    directory_items: usize,
    unnamed_items: usize,
}

impl<'l> Opc<'l> {
    fn new(limits: &'l Limits) -> Self {
        Opc { limits, seen: HashMap::new(), content_types_item: None, warnings: Vec::new(), directory_items: 0, unnamed_items: 0 }
    }

    fn into_warnings(self) -> Vec<Warning> {
        self.warnings
    }
}

impl Judge for Opc<'_> {
    type Error = OpcError;

    fn entry(&mut self, row: &Entry, kept: &[Entry]) -> Result<Verdict, OpcError> {
        let name_cap = self.limits.max_part_name_bytes;
        let written = row.written_name();
        let (folder, stream) = (row.folder, written.eq_ignore_ascii_case(CONTENT_TYPES_ITEM));
        // A row whose name a Unicode path field gives: its part name as written, which is the name to a reader that ignores the field.
        let mut written_part = None;
        if row.written.is_some() {
            written_part = unicode_path(row, folder, stream, name_cap)?;
        }
        let item = &row.name;
        if folder {
            // A folder is an item whose name, as written, ends in a slash, and nothing else is (as written, so that a
            // Unicode path field cannot make a file a folder and so drop it). It is not a part; its name is still
            // judged, so that `../x/` is not waved through. The container refuses a folder that holds data.
            check_folder_name(item.strip_suffix('/').unwrap_or(item), name_cap)?;
            self.directory_items += 1;
            if self.directory_items <= MAX_DIRECTORY_WARNINGS {
                self.warnings.push(Warning::DirectoryItem { name: item.clone() });
            } else {
                self.unnamed_items += 1;
            }
            return Ok(Verdict::Drop);
        }
        if stream {
            if let Some(first) = self.content_types_item.replace(item.clone()) {
                return Err(OpcError::DuplicatePart { first, second: item.clone() });
            }
            // The content types stream is the reader's own and no part, so the part limit is not its limit, and it is not counted.
            return Ok(Verdict::Keep { rename: Some(CONTENT_TYPES_ITEM.to_owned()), cap: self.limits.max_package_xml_bytes, counted: false });
        }
        let name = from_item_name(item, name_cap)?;
        // V1 over the name the row has and, when a Unicode path field gave it another, over its name as written too: two rows with
        // one name as written are duplicates to a reader that ignores the field.
        claim(&mut self.seen, &name, kept.len(), kept)?;
        if let Some(written) = &written_part {
            claim(&mut self.seen, written, kept.len(), kept)?;
        }
        // A relationships part is both a part and one of the XML parts this crate parses itself.
        let cap = if is_rels_part(&name) { self.limits.max_part_bytes.min(self.limits.max_package_xml_bytes) } else { self.limits.max_part_bytes };
        Ok(Verdict::Keep { rename: Some(name), cap, counted: true })
    }

    /// The prefix check's comparisons are counted in this crate's own `work` (its test-support counters), as they always were, and not in
    /// the container's.
    fn listed(&mut self, kept: &[Entry], _work: &mut Work) -> Result<(), OpcError> {
        if self.unnamed_items > 0 {
            self.warnings.push(Warning::MoreDirectoryItems { count: self.unnamed_items });
        }
        if self.content_types_item.is_none() {
            return Err(OpcError::MissingContentTypes);
        }
        check_prefixes(&self.seen, kept)
    }
}

/// A row whose Unicode path field gives its name (I2, items 1 and 2 of round 3). The name as written still counts, because
/// it is the name any reader that does not read the field sees, and a field must not make a name that is refused as written into one that
/// is not:
///
/// * the name as written is judged as it would be without the field, by the same rules (`from_item_name`, so that a traversal, an
///   escape of a letter, an empty segment, a control character, the stream's name spelled in escapes and the rest are refused with
///   the same class), as a folder without its slash, and a stream's name is the stream's and needs no judgement;
/// * the field has to agree with the name as written on whether the entry is a folder and on whether it is the content types stream:
///   a folder is a name that ends in a slash, and the stream is that name, in either (`invalid_part_name` where they differ). A field
///   that gives a valid different part to a row that is neither is the one thing it is for;
/// * the name as written, as a part name, is given back, so that the duplicate and prefix rules can hold over it as well: two rows with
///   one name as written are duplicates to a reader that ignores the field. `None` for a folder and for the stream.
fn unicode_path(row: &Entry, folder: bool, stream: bool, name_cap: usize) -> Result<Option<String>, OpcError> {
    let written = row.written_name();
    let canonical = if stream {
        None
    } else if folder {
        check_folder_name(written.strip_suffix('/').unwrap_or(written), name_cap)?;
        None
    } else {
        Some(from_item_name(written, name_cap)?)
    };
    let text = &row.name;
    if text.ends_with('/') != folder || text.eq_ignore_ascii_case(CONTENT_TYPES_ITEM) != stream {
        return Err(OpcError::InvalidPartName {
            name: text.chars().take(64).collect(),
            reason: "a Unicode path that is not, as the name as written is not, a folder or the content types stream",
        });
    }
    if text.len() > name_cap.saturating_mul(3) {
        return Err(OpcError::InvalidPartName { name: text.chars().take(64).collect(), reason: "longer than the limit" });
    }
    Ok(canonical)
}

/// The order of the prefix check: byte by byte, with `/` before every other byte. A name and all the names under it (`a`, `a/b`,
/// `a/b/c`) are then neighbours, which a plain byte order does not give (`a-b` sorts between `a` and `a/b`). It compares bytes and not
/// segments (comparing `split('/')` of two names of 508 one-letter segments costs an iterator step for each, and was most of the 2
/// seconds that 50,000 such names took: review round 2, I3), and skips the bytes the two names share a block at a time, which a
/// byte-by-byte rank comparison does not (names that share a kilobyte are the worst case of a sort).
fn slash_first(a: &str, b: &str) -> Ordering {
    const BLOCK: usize = 64;
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (a_blocks, _) = a.as_chunks::<BLOCK>();
    let (b_blocks, _) = b.as_chunks::<BLOCK>();
    let shared = a_blocks.iter().zip(b_blocks).take_while(|(x, y)| x == y).count() * BLOCK;
    let rank = |byte: &u8| if *byte == b'/' { 0 } else { u16::from(*byte) + 1 };
    let (a_rest, b_rest) = (a.get(shared..).unwrap_or_default(), b.get(shared..).unwrap_or_default());
    // The bytes looked at: those the two names share, and those up to where they part (at most the shorter of what is left).
    crate::work::name_comparison(shared + a_rest.len().min(b_rest.len()));
    a_rest.iter().map(rank).cmp(b_rest.iter().map(rank))
}

/// How a name in the table of names is shown in an error: the part's own name when it is the name the table has (in the case it
/// arrived with), and the folded name otherwise (a name as written that a Unicode path field replaced is not kept: only its key is).
fn shown(key: &str, owner: usize, entries: &[Entry]) -> String {
    match entries.get(owner).filter(|e| e.counted).map(|e| e.name.as_str()) {
        Some(part) if fold(part) == key => part.to_owned(),
        _ => key.to_owned(),
    }
}

/// `name` (a part name, as the row has it or as written) for the row `owner`, which is the index its entry will have. The folded
/// name is the key. A key that another row holds is a duplicate. A row may hold one key twice: a name as written and the name a field
/// gives can be one name.
fn claim(seen: &mut HashMap<String, usize>, name: &str, owner: usize, entries: &[Entry]) -> Result<(), OpcError> {
    match seen.entry(fold(name)) {
        MapEntry::Vacant(slot) => {
            slot.insert(owner);
            Ok(())
        }
        MapEntry::Occupied(held) if *held.get() == owner => Ok(()),
        MapEntry::Occupied(held) => Err(OpcError::DuplicatePart { first: shown(held.key(), *held.get(), entries), second: name.to_owned() }),
    }
}

/// Whether `name` is under `prefix`: it starts with it and continues with a slash.
fn is_under(name: &str, prefix: &str) -> bool {
    crate::work::name_bytes_looked_at(prefix.len());
    name.len() > prefix.len() && name.starts_with(prefix) && name.as_bytes().get(prefix.len()) == Some(&b'/')
}

/// No name of one row is a path prefix of the name of another. The names are those of `seen` (as written, and as a field gives
/// them). They are sorted with `/` first ([`slash_first`]), so that a name is followed by every name under it, and walked once with the
/// names that are prefixes of the one in hand open: they all belong to one row, or the walk has already refused. That is a sort and one
/// pass, in time that follows the size of the names and not the square of the depth of each; and the same error on every run, whatever
/// order the archive lists them in. The names one row has are not in each other's way (each reading of the file has one name for it).
fn check_prefixes(seen: &HashMap<String, usize>, entries: &[Entry]) -> Result<(), OpcError> {
    let mut names: Vec<(&String, usize)> = seen.iter().map(|(key, &owner)| (key, owner)).collect();
    names.sort_unstable_by(|a, b| slash_first(a.0, b.0));
    let mut open: Vec<(&String, usize)> = Vec::new();
    for &(name, owner) in &names {
        while open.last().is_some_and(|(prefix, _)| !is_under(name, prefix)) {
            open.pop();
        }
        if let Some(&(prefix, held_by)) = open.last()
            && held_by != owner
        {
            return Err(OpcError::PartNamePrefix { prefix: shown(prefix, held_by, entries), part: shown(name, owner, entries) });
        }
        open.push((name, owner));
    }
    Ok(())
}
