//! A streaming walk of an archive, as a reader that never reads the central directory goes (the ODF plan's harness calls it
//! `stream_walk`; this one is built here from the public PKWARE APPNOTE): from byte 0, each local header, its data, and the data
//! descriptor its flags announce (bit 3) in the form its local header announces (4.3.9.2: sizes of 8 bytes when the local header holds a
//! ZIP64 record, of 4 when it does not; the signature optional, 4.3.9.3), and then the next local header, until the directory's first row.
//!
//! The data of an entry is as long as its local header says (a ZIP64 record's sizes when the header leaves them to it), except a deflated
//! entry with flag bit 3, which is inflated to the end of its stream, since its header says nothing. A stored entry with flag bit 3 is
//! read as long as its header says, which is right for the zero-length entries these tests build (a streaming reader cannot know more). A
//! descriptor must state the checksum and the sizes of what was read. The first entry that breaks any of this ends the walk. What the walk
//! holds is the layout a streaming reader follows, not what an entry holds: the checksum a local header states is not checked (a copy
//! writes the row's there when it writes the headers anew, the central header winning).

use flate2::{Decompress, FlushDecompress, Status};

/// The most bytes one entry is inflated to: these tests' entries are far smaller.
const MOST_INFLATED: usize = 16 << 20;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at.checked_add(8)?)?.try_into().ok()?))
}

/// The body of the first ZIP64 record (id 1) of an extra field.
fn zip64(mut extra: &[u8]) -> Option<&[u8]> {
    while let (Some(id), Some(size)) = (u16_at(extra, 0), u16_at(extra, 2)) {
        let body = extra.get(4..4 + usize::from(size))?;
        if id == 1 {
            return Some(body);
        }
        extra = extra.get(4 + usize::from(size)..)?;
    }
    None
}

/// Where a walk broke: the entry, counted from 1, its local header's offset, and what was wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broke {
    pub entry: usize,
    pub at: usize,
    pub what: &'static str,
}

/// The names of the entries a streaming reader reads from `bytes`, in order, or where it broke.
pub fn walk(bytes: &[u8]) -> Result<Vec<Vec<u8>>, Broke> {
    let mut names = Vec::new();
    let mut at = 0usize;
    loop {
        let entry = names.len() + 1;
        let broke = |what| Broke { entry, at, what };
        match bytes.get(at..at + 4) {
            Some(b"PK\x03\x04") => {}
            Some(b"PK\x01\x02") | Some(b"PK\x05\x06") => return Ok(names),
            _ => return Err(broke("no local header where the last entry ended")),
        }
        let cut = || broke("a local header cut short");
        let (flags, method) = (u16_at(bytes, at + 6).ok_or_else(cut)?, u16_at(bytes, at + 8).ok_or_else(cut)?);
        let (mut compressed, size) = (u64::from(u32_at(bytes, at + 18).ok_or_else(cut)?), u32_at(bytes, at + 22).ok_or_else(cut)?);
        let (name_len, extra_len) = (usize::from(u16_at(bytes, at + 26).ok_or_else(cut)?), usize::from(u16_at(bytes, at + 28).ok_or_else(cut)?));
        let name = bytes.get(at + 30..at + 30 + name_len).ok_or_else(cut)?.to_vec();
        let extra = bytes.get(at + 30 + name_len..at + 30 + name_len + extra_len).ok_or_else(cut)?;
        let record = zip64(extra);
        if compressed == 0xFFFF_FFFF || size == 0xFFFF_FFFF {
            // The record holds both sizes, the uncompressed one first (APPNOTE 4.5.3).
            let record = record.ok_or_else(|| broke("sizes left to a ZIP64 record that is not there"))?;
            compressed = u64_at(record, 8).ok_or_else(|| broke("a ZIP64 record too short for both sizes"))?;
        }
        let data = at + 30 + name_len + extra_len;
        let described = flags & 0x0008 != 0;
        let (read, out) = if described && method == 8 {
            inflate(bytes.get(data..).unwrap_or_default()).ok_or_else(|| broke("a deflate stream that does not end"))?
        } else {
            let length = usize::try_from(compressed).map_err(|_| cut())?;
            let raw = bytes.get(data..data + length).ok_or_else(|| broke("data past the end of the file"))?;
            (length, raw.to_vec())
        };
        let mut next = data + read;
        if described {
            if bytes.get(next..next + 4) == Some(b"PK\x07\x08") {
                next += 4;
            }
            let stated_crc = u32_at(bytes, next).ok_or_else(cut)?;
            let (stated_compressed, stated_size) = if record.is_some() {
                let sizes = (u64_at(bytes, next + 4).ok_or_else(cut)?, u64_at(bytes, next + 12).ok_or_else(cut)?);
                next += 20;
                sizes
            } else {
                let sizes = (u64::from(u32_at(bytes, next + 4).ok_or_else(cut)?), u64::from(u32_at(bytes, next + 8).ok_or_else(cut)?));
                next += 12;
                sizes
            };
            let inflated = if method == 8 { out.len() as u64 } else { read as u64 };
            if (stated_crc, stated_compressed, stated_size) != (crc32fast::hash(&out), read as u64, inflated) {
                return Err(broke("a data descriptor that does not state what was read"));
            }
        }
        names.push(name);
        at = next;
    }
}

/// The deflate stream at the start of `input`, inflated to its end: how many bytes of `input` it took, and what came out.
fn inflate(input: &[u8]) -> Option<(usize, Vec<u8>)> {
    let mut inflater = Decompress::new(false);
    let mut out = Vec::with_capacity(4096);
    loop {
        if out.len() == out.capacity() {
            if out.len() >= MOST_INFLATED {
                return None;
            }
            out.reserve(4096);
        }
        let before = (inflater.total_in(), inflater.total_out());
        let rest = input.get(usize::try_from(inflater.total_in()).ok()?..)?;
        match inflater.decompress_vec(rest, &mut out, FlushDecompress::None).ok()? {
            Status::StreamEnd => return Some((usize::try_from(inflater.total_in()).ok()?, out)),
            _ if (inflater.total_in(), inflater.total_out()) == before => return None,
            _ => {}
        }
    }
}
