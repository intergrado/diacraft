//! The central directory of a ZIP archive: the end record, the ZIP64 records and the rows, read here from the public PKWARE
//! APPNOTE and. No ZIP library is used, and none of its text is in this code.
//!
//! The rules that keep this reader's cost a function of the file and nothing else:
//!
//! * **Offsets are from byte 0.** An archive with bytes in front of it (a stub, a prefix) has offsets that reach nothing, and is
//!   refused: `zip_corrupt` when the end record is found but its offsets do not reach their signatures. No attempt is made to guess
//!   a shift.
//! * **The end record is found by one backward scan** of the last 65,557 bytes (the record's 22 and the longest comment): the last
//!   signature whose own comment length ends exactly at the end of the file; failing that the last signature
//!   whose comment ends inside the file, with the bytes after it counted and ignored (the caller says so in a warning). A record whose
//!   comment runs past the end of the file does not fit. There is no second try. A file in which no record fits is `not_zip`, or
//!   `zip_corrupt` if it starts with a local header. If a comment holds bytes that look like an end record, they do not count unless
//!   their own comment length reaches the end of the file; one that does *is* the end record, and decides (the same is true of an
//!   appended second directory: see the crate documentation).
//! * **Before a row is read, the count is checked against the bytes**: a count whose rows, at 46 bytes at least, cannot fit in the
//!   size the record gives the directory is `zip_corrupt`. So a flipped bit in a count is damage, and a count that does fit but is
//!   more than the limit is `too_many_parts` (`zipread.rs`).
//! * **Rows are walked once, in order**, never past the directory, never more than the count. A row of more than 65,535 bytes is
//!   `zip_corrupt`. Nothing of a row is stored but what the caller asks for: the name as written (refused before it is copied if it
//!   is more than three times the name limit), the name a Unicode path field gives, and numbers. The field's name is copied as the row
//!   holds it, at most what is left of the row's 65,535 bytes, before any judge sees it: its length is the judge's to refuse (the opc
//!   judge does, after its rules for the name as written, so that the order of its refusals is the one it always had), and the copy is
//!   dropped with the row. Extra fields are read for the ZIP64 sizes and offset and for the Unicode path, and dropped.
//! * **The walk has to use the directory up.** What the count leaves of the directory must not start with a row (`zip_corrupt`): a
//!   count below the rows is damage, and not a package with parts gone. The digital signature record (`PK\x05\x05`), and any bytes
//!   that are not a row, may follow the last row and are not read.
//! * **The end records and the first of each extra field.** The ZIP64 record is read when any field of the plain record is its "look
//!   elsewhere" value (0xFFFF or 0xFFFFFFFF), and then only for the values the plain record leaves to it; one the plain record gives
//!   itself stands. When it is read, *its* disk fields decide whether the archive is a single disk, and the plain record's disk
//!   numbers and the locator's disk of the ZIP64 record and number of disks are not read (labelled: harmless, because every offset is
//!   from byte 0 and checked against the signature it must reach, so an archive that spans disks finds no record where it says one
//!   is). Of the ZIP64 and Unicode path fields of a row the first of each counts.
//! * **The name as written is given with the name a Unicode path field gives.** A row whose field counts (version 1, with the checksum
//!   of the name as written) has the field's name, and its name as written beside it ([`crate::zip::Entry::written`]), so that the
//!   caller's judge can hold the name as written to the same rules: any reader that does not read the field sees that name, and a field
//!   must not make a hostile name into a harmless one. Whether the entry is a folder is said by the name as written alone.

use std::cmp::Ordering;

use crate::ContainerError;
use crate::work::Work;

const ROW_SIGNATURE: &[u8] = b"PK\x01\x02";
const END_SIGNATURE: &[u8] = b"PK\x05\x06";
const LOCATOR_SIGNATURE: &[u8] = b"PK\x06\x07";
const END64_SIGNATURE: &[u8] = b"PK\x06\x06";
/// The signature of a local file header.
pub(crate) const LOCAL_SIGNATURE: &[u8] = b"PK\x03\x04";

const END_LEN: usize = 22;
const LOCATOR_LEN: usize = 20;
const END64_LEN: usize = 56;
/// The fixed part of a row of the directory: a row of fewer bytes cannot exist.
const ROW_FIXED_LEN: usize = 46;
const LOCAL_FIXED_LEN: usize = 30;
/// The longest comment an end record can have: the length is 16 bits.
const MAX_COMMENT: usize = 65_535;
/// A central directory file header must not exceed this many bytes, name, extra field and comment included.
const MAX_ROW_BYTES: usize = 65_535;
/// The ZIP64 "look elsewhere" values.
const SENTINEL16: u16 = 0xFFFF;
const SENTINEL32: u32 = 0xFFFF_FFFF;
/// General purpose flag bit 11: the name is UTF-8.
pub(crate) const FLAG_UTF8_NAME: u16 = 0x0800;
const EXTRA_ZIP64: u16 = 0x0001;
const EXTRA_UNICODE_PATH: u16 = 0x7075;

pub(crate) fn corrupt(detail: impl Into<String>) -> ContainerError {
    ContainerError::ZipCorrupt { detail: detail.into() }
}

fn cut() -> ContainerError {
    corrupt("a record of the archive is cut short")
}

/// Little-endian fields read in order from a slice, none past its end.
struct Fields<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Fields<'a> {
    fn new(bytes: &'a [u8], at: usize) -> Self {
        Fields { bytes, at }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], ContainerError> {
        let end = self.at.checked_add(n).ok_or_else(cut)?;
        let taken = self.bytes.get(self.at..end).ok_or_else(cut)?;
        self.at = end;
        Ok(taken)
    }

    fn u16(&mut self) -> Result<u16, ContainerError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().map_err(|_| cut())?))
    }

    fn u32(&mut self) -> Result<u32, ContainerError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| cut())?))
    }

    fn u64(&mut self) -> Result<u64, ContainerError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().map_err(|_| cut())?))
    }
}

fn offset(value: u64, what: &str) -> Result<usize, ContainerError> {
    usize::try_from(value).map_err(|_| corrupt(format!("{what} is past anything this machine can address")))
}

/// Where the directory is, and how many rows the end record says it has.
pub(crate) struct Plan {
    /// Rows the directory is said to hold (the ZIP64 count when the record says so).
    pub count: u64,
    /// Offset of the first row, from byte 0.
    pub start: usize,
    /// Offset just past the last byte the directory is said to have.
    pub end: usize,
    /// Bytes after the end record and its comment, which are ignored: 0 for a record that ends the file.
    pub after: usize,
    /// Where the plain end record is, from byte 0, and how many bytes of comment it has.
    pub end_record: usize,
    pub comment: usize,
    /// Where the ZIP64 end record is, when it was read: its bytes and its locator's run from here to the plain end record.
    pub zip64_record: Option<usize>,
}

/// The end record, and how many bytes follow it and its comment.
struct EndRecord {
    at: usize,
    after: usize,
}

/// The end record, by one backward scan of the final 65,557 bytes: the last place where the signature stands and the
/// record's own comment length ends exactly at the end of the file; and if there is none, the last place where the signature stands and
/// the comment ends inside the file, with the bytes after it counted and ignored. A record whose comment runs past the end of the file
/// does not fit. The scan does not stop at a record that only fits, since one that ends the file may be before it and wins; and it makes no
/// second try, whatever the record says, so the cost is that of one pass over a window of a fixed size (`Work::end_scan` counts it).
fn find_end_record(bytes: &[u8], work: &mut Work) -> Option<EndRecord> {
    let last = bytes.len().checked_sub(END_LEN)?;
    let first = last.saturating_sub(MAX_COMMENT);
    let mut fits: Option<EndRecord> = None;
    for at in (first..=last).rev() {
        work.end_scan = work.end_scan.saturating_add(1);
        if bytes.get(at..at + END_SIGNATURE.len()) != Some(END_SIGNATURE) {
            continue;
        }
        let Some(comment) = bytes.get(at + 20..at + 22).and_then(|c| <[u8; 2]>::try_from(c).ok()).map(u16::from_le_bytes) else { continue };
        match (at + END_LEN + usize::from(comment)).cmp(&bytes.len()) {
            Ordering::Equal => return Some(EndRecord { at, after: 0 }),
            Ordering::Less if fits.is_none() => fits = Some(EndRecord { at, after: bytes.len() - (at + END_LEN + usize::from(comment)) }),
            _ => {}
        }
    }
    fits
}

/// Finds the end record and, through it and the ZIP64 records, the directory. Nothing of the directory is read yet.
pub(crate) fn locate(bytes: &[u8], work: &mut Work) -> Result<Plan, ContainerError> {
    let Some(EndRecord { at, after }) = find_end_record(bytes, work) else {
        return Err(if bytes.starts_with(LOCAL_SIGNATURE) { corrupt("the archive has no end record") } else { ContainerError::NotZip });
    };
    let mut record = Fields::new(bytes, at + END_SIGNATURE.len());
    let (disk, directory_disk, on_disk, total) = (record.u16()?, record.u16()?, record.u16()?, record.u16()?);
    let (size, start) = (record.u32()?, record.u32()?);
    let comment = usize::from(record.u16()?);
    let (mut count, mut size, mut start) = (u64::from(total), u64::from(size), u64::from(start));
    let mut boundary = at;
    let mut zip64_record = None;
    let mut single_disk = disk == 0 && directory_disk == 0 && on_disk == total;
    let wants_zip64 = [disk, directory_disk, on_disk, total].contains(&SENTINEL16) || size == u64::from(SENTINEL32) || start == u64::from(SENTINEL32);
    let locator = at.checked_sub(LOCATOR_LEN).filter(|&from| bytes.get(from..from + LOCATOR_SIGNATURE.len()) == Some(LOCATOR_SIGNATURE));
    if let (true, Some(locator)) = (wants_zip64, locator) {
        let mut l = Fields::new(bytes, locator + LOCATOR_SIGNATURE.len());
        l.u32()?; // the disk of the ZIP64 record
        let record = offset(l.u64()?, "the offset of the ZIP64 end record")?;
        if record.checked_add(END64_LEN).is_none_or(|stop| stop > locator) {
            return Err(corrupt("the ZIP64 end record is not before its locator"));
        }
        if bytes.get(record..record + END64_SIGNATURE.len()) != Some(END64_SIGNATURE) {
            return Err(corrupt("there is no ZIP64 end record where its locator says"));
        }
        let mut z = Fields::new(bytes, record + END64_SIGNATURE.len());
        let remaining = z.u64()?;
        z.take(4)?; // the versions
        let (z_disk, z_directory_disk, z_on_disk, z_total, z_size, z_start) = (z.u32()?, z.u32()?, z.u64()?, z.u64()?, z.u64()?, z.u64()?);
        if remaining < 44 {
            return Err(corrupt("the ZIP64 end record is too short to be one"));
        }
        // A value the plain record gives as "look elsewhere" is read here; one it gives for itself stands.
        if total == SENTINEL16 {
            count = z_total;
        }
        if size == u64::from(SENTINEL32) {
            size = z_size;
        }
        if start == u64::from(SENTINEL32) {
            start = z_start;
        }
        single_disk = z_disk == 0 && z_directory_disk == 0 && z_on_disk == z_total;
        boundary = record;
        zip64_record = Some(record);
    }
    if !single_disk {
        return Err(corrupt("the archive is not a single-disk archive"));
    }
    let start = offset(start, "the offset of the directory")?;
    let end = start.checked_add(offset(size, "the size of the directory")?).filter(|&e| e <= boundary);
    let Some(end) = end else { return Err(corrupt("the directory does not end before the end record")) };
    // Before anything is read, a count has to fit: at 46 bytes at least a row.
    if u128::from(count) * ROW_FIXED_LEN as u128 > (end - start) as u128 {
        return Err(corrupt(format!("the end record counts {count} rows, which do not fit in the {} bytes it gives the directory", end - start)));
    }
    Ok(Plan { count, start, end, after, end_record: at, comment, zip64_record })
}

/// One row of the directory, as much of it as is kept.
pub(crate) struct Row {
    /// The name: the Unicode path field's when the field counts, else the name as written, decoded (UTF-8, or code page 437).
    pub name: String,
    /// The name as written, decoded, when a Unicode path field gave `name`; `None` when `name` is the name as written.
    pub written: Option<String>,
    /// The name *as written* ends in a slash: the entry is a folder, whatever a Unicode path field says (I2).
    pub folder: bool,
    pub flags: u16,
    pub method: u16,
    pub crc: u32,
    pub compressed: u64,
    pub declared: u64,
    /// Offset of the entry's local header, from byte 0.
    pub local_offset: usize,
    /// The row's "version made by" and "version needed to extract" (APPNOTE 4.4.2, 4.4.3).
    pub made_by: u16,
    pub needed: u16,
    /// The MS-DOS time and date of the row.
    pub time: u16,
    pub date: u16,
    /// The internal and external file attributes (APPNOTE 4.4.14, 4.4.15).
    pub internal: u16,
    pub external: u32,
    /// Where the row is, from byte 0, and how many bytes it takes (its fixed part, name, extra field and comment).
    pub row_offset: usize,
    pub row_len: usize,
}

/// The rows, in order. Each call reads one row, and the walk ends after exactly `count` of them or at the first error.
pub(crate) struct Rows<'a, 'w> {
    directory: &'a [u8],
    at: usize,
    left: u64,
    name_cap: usize,
    work: &'w mut Work,
}

impl<'a, 'w> Rows<'a, 'w> {
    /// `name_cap` is the name limit in bytes: a name more than three times as long as written is refused before it is copied (a
    /// name may be shorter once its percent-escapes are decoded, and longer once code page 437 is, by up to three times).
    pub(crate) fn new(bytes: &'a [u8], plan: &Plan, name_cap: usize, work: &'w mut Work) -> Self {
        Rows { directory: bytes.get(..plan.end).unwrap_or_default(), at: plan.start, left: plan.count, name_cap, work }
    }

    fn row(&mut self) -> Result<Row, ContainerError> {
        self.work.rows = self.work.rows.saturating_add(1);
        let row_offset = self.at;
        let mut f = Fields::new(self.directory, self.at);
        if f.take(ROW_SIGNATURE.len())? != ROW_SIGNATURE {
            return Err(corrupt("there is no central directory row where the end record says there is one"));
        }
        let (made_by, needed) = (f.u16()?, f.u16()?);
        let flags = f.u16()?;
        let method = f.u16()?;
        let (time, date) = (f.u16()?, f.u16()?);
        let crc = f.u32()?;
        let (compressed32, declared32) = (f.u32()?, f.u32()?);
        let (name_len, extra_len, comment_len) = (usize::from(f.u16()?), usize::from(f.u16()?), usize::from(f.u16()?));
        let disk = f.u16()?;
        let (internal, external) = (f.u16()?, f.u32()?);
        let local32 = f.u32()?;
        if ROW_FIXED_LEN + name_len + extra_len + comment_len > MAX_ROW_BYTES {
            return Err(corrupt("a row of the directory is longer than 65,535 bytes"));
        }
        let raw_name = f.take(name_len)?;
        let extra = f.take(extra_len)?;
        f.take(comment_len)?;
        let too_long = |raw: &[u8]| ContainerError::InvalidPartName {
            name: String::from_utf8_lossy(raw.get(..64).unwrap_or(raw)).into_owned(),
            reason: "longer than the limit",
        };
        if name_len > self.name_cap.saturating_mul(3) {
            return Err(too_long(raw_name));
        }
        let (zip64, unicode) = extra_fields(extra, self.work);
        let mut zip64 = zip64.map(|body| Fields::new(body, 0));
        let mut wide = |value: u32| -> Result<u64, ContainerError> {
            if value != SENTINEL32 {
                return Ok(u64::from(value));
            }
            zip64
                .as_mut()
                .ok_or_else(|| corrupt("a row leaves a value to its ZIP64 field, and has none"))?
                .u64()
                .map_err(|_| corrupt("a ZIP64 field is too short for the values its row leaves to it"))
        };
        let declared = wide(declared32)?;
        let compressed = wide(compressed32)?;
        let local = wide(local32)?;
        if disk != 0 && !(disk == SENTINEL16 && zip64.as_mut().is_some_and(|z| z.u32().is_ok_and(|d| d == 0))) {
            return Err(corrupt("a row says its entry is on another disk"));
        }
        let name = decode_name(raw_name, flags, unicode, self.work)?;
        self.at = f.at;
        Ok(Row {
            name: name.text,
            written: name.written,
            folder: name.folder,
            flags,
            method,
            crc,
            compressed,
            declared,
            local_offset: offset(local, "the offset of a local header")?,
            made_by,
            needed,
            time,
            date,
            internal,
            external,
            row_offset,
            row_len: f.at.saturating_sub(row_offset),
        })
    }
}

impl Rows<'_, '_> {
    /// After the last row the count allows: what is left of the directory must not be another row. A count below the rows the directory
    /// holds (one flipped bit, a count that wrapped past 65,535) would otherwise read as a package with parts gone, and a later save
    /// would lose them. What may follow the rows is what the format puts there or nothing at all: the digital signature record
    /// (`PK\x05\x05`) and bytes that are not a row are not read and not refused; four bytes that start a row are.
    pub(crate) fn finish(&self) -> Result<(), ContainerError> {
        let rest = self.directory.get(self.at..).unwrap_or_default();
        if rest.starts_with(ROW_SIGNATURE) {
            return Err(corrupt("the directory holds more rows than the end record counts"));
        }
        Ok(())
    }

    /// Where the walk is: after the last row read, once the count is met.
    pub(crate) fn position(&self) -> usize {
        self.at
    }
}

impl Iterator for Rows<'_, '_> {
    type Item = Result<Row, ContainerError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        let row = self.row();
        if row.is_err() {
            self.left = 0;
        }
        Some(row)
    }
}

/// The ZIP64 field's body and the Unicode path field's body, the first of each, from an extra field area. Nothing is kept: a record
/// that runs past the area, or bytes too few to be a record, end the scan. Each record walked is counted (`Work::extra_records`).
fn extra_fields<'a>(extra: &'a [u8], work: &mut Work) -> (Option<&'a [u8]>, Option<&'a [u8]>) {
    let (mut zip64, mut unicode) = (None, None);
    let mut rest = extra;
    while let Ok((id, body, after)) = extra_record(rest) {
        work.extra_records = work.extra_records.saturating_add(1);
        match id {
            EXTRA_ZIP64 => zip64 = zip64.or(Some(body)),
            EXTRA_UNICODE_PATH => unicode = unicode.or(Some(body)),
            _ => {}
        }
        rest = after;
    }
    (zip64, unicode)
}

/// One record of an extra field area: its id, its body and what follows it.
fn extra_record(area: &[u8]) -> Result<(u16, &[u8], &[u8]), ContainerError> {
    let mut f = Fields::new(area, 0);
    let (id, size) = (f.u16()?, usize::from(f.u16()?));
    let body = f.take(size)?;
    Ok((id, body, area.get(f.at..).unwrap_or_default()))
}

/// The name of a row, and the name as written when a Unicode path field replaced it.
struct Name {
    text: String,
    written: Option<String>,
    folder: bool,
}

/// The name of a row. The Unicode path field (Info-ZIP's `up`) counts when it says it is for this name, by the checksum of the name as
/// written; then flag bit 11 says UTF-8; else the bytes are UTF-8 if they are, and code page 437 if they are not (the ZIP format's own
/// default). Whether the entry is a folder is the name as written's to say (it ends in a slash), whatever the field says: the caller's
/// judge holds the field to agree with it.
fn decode_name(raw: &[u8], flags: u16, unicode: Option<&[u8]>, work: &mut Work) -> Result<Name, ContainerError> {
    Work::add(&mut work.name_bytes, raw.len());
    let folder = raw.last() == Some(&b'/');
    let written = match std::str::from_utf8(raw) {
        Ok(text) => text.to_owned(),
        Err(_) if flags & FLAG_UTF8_NAME != 0 => return Err(corrupt("a name the row flags as UTF-8 is not")),
        Err(_) => raw.iter().map(|&b| cp437(b)).collect(),
    };
    if let Some(body) = unicode {
        let mut f = Fields::new(body, 0);
        if let (Ok(1), Ok(crc)) = (f.take(1).map(|v| v.first().copied().unwrap_or(0)), f.u32())
            && crc == crc32fast::hash(raw)
            && let Some(text) = body.get(f.at..).and_then(|b| std::str::from_utf8(b).ok())
        {
            Work::add(&mut work.name_bytes, text.len());
            return Ok(Name { text: text.to_owned(), written: Some(written), folder });
        }
    }
    Ok(Name { text: written, written: None, folder })
}

/// A name as written, decoded as the reader decodes one with no Unicode path field: UTF-8 when the bytes are, else code page 437.
pub(crate) fn decode_written(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(text) => text.to_owned(),
        Err(_) => raw.iter().map(|&b| cp437(b)).collect(),
    }
}

/// Code page 437 for the bytes from 0x80 up (the ZIP format's name encoding when flag bit 11 is not set); below that, ASCII.
fn cp437(byte: u8) -> char {
    const HIGH: [char; 128] = [
        'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢',
        '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡', '╢', '╖',
        '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙',
        '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐', '▀', 'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩',
        '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{a0}',
    ];
    match byte.checked_sub(0x80).and_then(|i| HIGH.get(usize::from(i))) {
        Some(&c) => c,
        None => char::from(byte),
    }
}

/// What an entry's local header says of where its data is: the offset of the data (after the header's own name and extra field), and
/// the length of that extra field.
pub(crate) struct LocalHeader {
    pub data: usize,
    pub extra_len: usize,
}

/// The local header at `local_offset`, for the lengths of its own name and extra field, which say where the data starts. Nothing else of
/// the local header is read or believed (the central row decides the name, method, sizes and checksum: see `zipread.rs`).
pub(crate) fn local_header(bytes: &[u8], local_offset: usize, work: &mut Work) -> Result<LocalHeader, ContainerError> {
    work.local_headers = work.local_headers.saturating_add(1);
    let mut f = Fields::new(bytes, local_offset);
    if f.take(LOCAL_SIGNATURE.len())? != LOCAL_SIGNATURE {
        return Err(corrupt("there is no local header where the directory says there is one"));
    }
    f.take(LOCAL_FIXED_LEN - LOCAL_SIGNATURE.len() - 4)?; // versions, flags, method, time, date, checksum, sizes
    let (name_len, extra_len) = (usize::from(f.u16()?), usize::from(f.u16()?));
    let data = f.at.checked_add(name_len).and_then(|n| n.checked_add(extra_len)).ok_or_else(cut)?;
    Ok(LocalHeader { data, extra_len })
}

/// Every field of a local header, as written: what a copy of an entry compares with its row before it keeps the header's bytes.
pub(crate) struct LocalFields<'a> {
    pub flags: u16,
    pub method: u16,
    pub crc: u32,
    pub compressed: u32,
    pub size: u32,
    pub name: &'a [u8],
    /// The local header's extra field, as written.
    pub extra: &'a [u8],
    /// Where the entry's data starts, from byte 0: after the header's name and extra field.
    pub data: usize,
}

/// The local header at `local_offset`, every field of it. Nothing is counted: the reader has read this header already.
pub(crate) fn local_fields(bytes: &[u8], local_offset: usize) -> Result<LocalFields<'_>, ContainerError> {
    let mut f = Fields::new(bytes, local_offset);
    if f.take(LOCAL_SIGNATURE.len())? != LOCAL_SIGNATURE {
        return Err(corrupt("there is no local header where the directory says there is one"));
    }
    f.take(2)?; // version needed
    let (flags, method) = (f.u16()?, f.u16()?);
    f.take(4)?; // time and date
    let (crc, compressed, size) = (f.u32()?, f.u32()?, f.u32()?);
    let (name_len, extra_len) = (usize::from(f.u16()?), usize::from(f.u16()?));
    let name = f.take(name_len)?;
    let extra = f.take(extra_len)?;
    Ok(LocalFields { flags, method, crc, compressed, size, name, extra, data: f.at })
}

/// The fields of a central row that a copy of its entry needs, as written: the row's own bytes, and what is checked before they are kept.
pub(crate) struct RowFields<'a> {
    /// The whole row: its fixed part, name, extra field and comment.
    pub bytes: &'a [u8],
    pub flags: u16,
    pub method: u16,
    pub crc: u32,
    pub compressed32: u32,
    pub size32: u32,
    pub disk: u16,
    pub local32: u32,
    /// The name as written.
    pub name: &'a [u8],
    /// The row's extra field and its comment, as written.
    pub extra: &'a [u8],
    pub comment: &'a [u8],
}

/// Where in a central row its local header's offset is written (APPNOTE 4.3.12): the one field a copy changes.
pub(crate) const ROW_OFFSET_FIELD: usize = 42;

/// The central row of `row_len` bytes at `row_offset`, read again from the file. A row that is not one, or does not fill `row_len` exactly,
/// is `zip_corrupt`.
pub(crate) fn row_fields(bytes: &[u8], row_offset: usize, row_len: usize) -> Result<RowFields<'_>, ContainerError> {
    let row = row_offset.checked_add(row_len).and_then(|end| bytes.get(row_offset..end)).ok_or_else(cut)?;
    let mut f = Fields::new(row, 0);
    if f.take(ROW_SIGNATURE.len())? != ROW_SIGNATURE {
        return Err(corrupt("there is no central directory row where the entry says its row is"));
    }
    f.take(4)?; // made by, needed
    let (flags, method) = (f.u16()?, f.u16()?);
    f.take(4)?; // time and date
    let (crc, compressed32, size32) = (f.u32()?, f.u32()?, f.u32()?);
    let (name_len, extra_len, comment_len) = (usize::from(f.u16()?), usize::from(f.u16()?), usize::from(f.u16()?));
    let disk = f.u16()?;
    f.take(6)?; // internal and external attributes
    let local32 = f.u32()?;
    let name = f.take(name_len)?;
    let extra = f.take(extra_len)?;
    let comment = f.take(comment_len)?;
    if f.at != row.len() {
        return Err(corrupt("a row of the directory is not as long as the entry says"));
    }
    Ok(RowFields { bytes: row, flags, method, crc, compressed32, size32, disk, local32, name, extra, comment })
}

/// The row's extra field as a header written anew keeps it: its ZIP64 records left out (this crate writes no ZIP64). Its Unicode path
/// records are kept: the reader has checked them against the name as written, which is the name the header keeps.
pub(crate) fn row_extra_kept(area: &[u8], work: &mut Work) -> Vec<u8> {
    without(area, &[EXTRA_ZIP64], work)
}

/// A local header's extra field as a header written anew keeps it: its ZIP64 records left out, and its Unicode path records too. The reader
/// never reads a local Unicode path, and one that named nothing in the file (its checksum is of the local name, which may not be the
/// row's) could name something once the header holds the row's name; the row's own field, which was checked, says what the name is.
pub(crate) fn local_extra_kept(area: &[u8], work: &mut Work) -> Vec<u8> {
    without(area, &[EXTRA_ZIP64, EXTRA_UNICODE_PATH], work)
}

/// An extra field area with the records of `ids` left out and every other record kept, in order. The records are walked once; bytes at the
/// end too few to be a record, or a record that runs past the area, end the walk and are left out, as the reader leaves them unread.
fn without(area: &[u8], ids: &[u16], work: &mut Work) -> Vec<u8> {
    let mut kept = Vec::with_capacity(area.len());
    let mut rest = area;
    while let Ok((id, _, after)) = extra_record(rest) {
        work.local_extra_records = work.local_extra_records.saturating_add(1);
        // A record is kept or left out whole, by its id: its header and body are the bytes before what follows it.
        if !ids.contains(&id) {
            kept.extend_from_slice(rest.get(..rest.len().saturating_sub(after.len())).unwrap_or_default());
        }
        rest = after;
    }
    kept
}

/// The body of the first ZIP64 record of an extra field area (APPNOTE 4.5.3), by the walk the reader makes of an area: a record that runs
/// past the area, or bytes too few to be one, end it. The reader has walked the rows' areas, and a local header's is walked only by a copy,
/// so each record this walk looks at is counted (`Work::local_extra_records`).
pub(crate) fn zip64_record<'a>(area: &'a [u8], work: &mut Work) -> Option<&'a [u8]> {
    let mut rest = area;
    while let Ok((id, body, after)) = extra_record(rest) {
        work.local_extra_records = work.local_extra_records.saturating_add(1);
        if id == EXTRA_ZIP64 {
            return Some(body);
        }
        rest = after;
    }
    None
}

/// The two sizes a local header's ZIP64 record gives, the uncompressed size first (APPNOTE 4.5.3: in a local header the record holds both).
/// `None` when the record is too short for both.
pub(crate) fn zip64_sizes(body: &[u8]) -> Option<(u64, u64)> {
    let mut f = Fields::new(body, 0);
    Some((f.u64().ok()?, f.u64().ok()?))
}

/// The signature a data descriptor may start with (APPNOTE 4.3.9.3).
const DESCRIPTOR_SIGNATURE: u32 = 0x0807_4B50;

/// The length of the data descriptor at `at` (APPNOTE 4.3.9), when one is there **in the form its local header announces** that states this
/// checksum and these sizes and ends at or before `limit`. The form is the local header's to say (APPNOTE 4.3.9.2): sizes of 8 bytes each when
/// the header holds a ZIP64 record (`wide`), of 4 bytes when it does not; a streaming reader, which walks the file from local header to local
/// header, reads the descriptor so and then looks for the next header. The signature is optional (4.3.9.3), so the form with it is tried
/// first and then the form without. `None` when neither matches. The bytes are compared and nothing else is read, so a descriptor is kept
/// only when it says what the row says, in the form the header says it has.
pub(crate) fn descriptor_len(bytes: &[u8], at: usize, limit: usize, crc: u32, compressed: u64, size: u64, wide: bool) -> Option<usize> {
    for signed in [true, false] {
        let mut f = Fields::new(bytes, at);
        if signed && f.u32().ok() != Some(DESCRIPTOR_SIGNATURE) {
            continue;
        }
        let Ok(stated_crc) = f.u32() else { continue };
        let sizes = if wide { f.u64().and_then(|c| Ok((c, f.u64()?))) } else { f.u32().and_then(|c| Ok((u64::from(c), u64::from(f.u32()?)))) };
        if stated_crc == crc && sizes.ok() == Some((compressed, size)) && f.at <= limit {
            return Some(f.at.saturating_sub(at));
        }
    }
    None
}
