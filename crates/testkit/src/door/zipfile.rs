//! ZIP archives taken apart and put together again, with or without lies.
//!
//! [`Archive::parse`] reads an honest archive (one made by a writer of this project: the entries are where the central directory says, with true sizes) into its entries, each as
//! name, data and whether it was deflated. [`Archive::honest`] writes them again with true headers. [`raws_of`] and [`write_raw`] make the archive a lying writer would: every field
//! of a local header and of a central record is held apart, so that a mutator changes one of them (a size, an offset, a checksum, a name, a method, a flag) and the two records, or the
//! record and the data, no longer agree. The format is PKWARE's APPNOTE, which is public.

use std::io::{Read, Write};

/// The most bytes one entry may inflate to when an archive is taken apart (a seed is small; this stops a bad seed).
const MAX_ENTRY: u64 = 64 << 20;

/// One entry: its name as bytes, its data, and whether it is written deflated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: Vec<u8>,
    pub data: Vec<u8>,
    pub deflate: bool,
}

impl Entry {
    /// An entry of `name` holding `data`.
    pub fn new(name: &str, data: impl Into<Vec<u8>>, deflate: bool) -> Entry {
        Entry { name: name.as_bytes().to_vec(), data: data.into(), deflate }
    }

    /// The name as text, with anything that is not UTF-8 replaced.
    pub fn name_text(&self) -> String {
        String::from_utf8_lossy(&self.name).into_owned()
    }

    /// Whether the entry holds XML text a mutator may rewrite: a name that ends in `.xml` or `.rels`, or a manifest.
    pub fn is_xml(&self) -> bool {
        let name = self.name_text();
        name.ends_with(".xml") || name.ends_with(".rels")
    }
}

/// The entries of an archive in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Archive {
    pub entries: Vec<Entry>,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn inflate(raw: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(raw).take(MAX_ENTRY + 1).read_to_end(&mut out).ok()?;
    (out.len() as u64 <= MAX_ENTRY).then_some(out)
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    // Writing into memory cannot fail.
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_default()
}

impl Archive {
    /// The entries of an honest archive, or `None` when `bytes` is not one this function can read (it reads only what the project's own writers make).
    pub fn parse(bytes: &[u8]) -> Option<Archive> {
        let eocd = (0..bytes.len().checked_sub(21)?).rev().find(|&i| u32_at(bytes, i) == Some(0x0605_4B50))?;
        let count = usize::from(u16_at(bytes, eocd + 10)?);
        let mut at = usize::try_from(u32_at(bytes, eocd + 16)?).ok()?;
        let mut entries = Vec::new();
        for _ in 0..count {
            if u32_at(bytes, at)? != 0x0201_4B50 {
                return None;
            }
            let method = u16_at(bytes, at + 10)?;
            let csize = usize::try_from(u32_at(bytes, at + 20)?).ok()?;
            let (nlen, xlen, clen) =
                (usize::from(u16_at(bytes, at + 28)?), usize::from(u16_at(bytes, at + 30)?), usize::from(u16_at(bytes, at + 32)?));
            let local = usize::try_from(u32_at(bytes, at + 42)?).ok()?;
            let name = bytes.get(at + 46..at + 46 + nlen)?.to_vec();
            at += 46 + nlen + xlen + clen;
            if u32_at(bytes, local)? != 0x0403_4B50 {
                return None;
            }
            let start = local + 30 + usize::from(u16_at(bytes, local + 26)?) + usize::from(u16_at(bytes, local + 28)?);
            let raw = bytes.get(start..start.checked_add(csize)?)?;
            let data = if method == 8 { inflate(raw)? } else { raw.to_vec() };
            entries.push(Entry { name, data, deflate: method == 8 });
        }
        Some(Archive { entries })
    }

    /// The archive written with true headers, in order.
    pub fn honest(&self) -> Vec<u8> {
        write_raw(&raws_of(self), &Tail::default())
    }

    /// The index of the first entry named `name`.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.name == name.as_bytes())
    }
}

/// One entry as a lying writer holds it: the local header's fields and the central record's apart, and the bytes that follow the local header.
#[derive(Clone, Debug)]
pub struct Raw {
    pub name: Vec<u8>,
    pub central_name: Vec<u8>,
    pub method: u16,
    pub central_method: u16,
    pub flags: u16,
    pub central_flags: u16,
    pub crc: u32,
    pub central_crc: u32,
    pub compressed: u32,
    pub central_compressed: u32,
    pub size: u32,
    pub central_size: u32,
    pub extra: Vec<u8>,
    pub central_extra: Vec<u8>,
    pub payload: Vec<u8>,
    /// The offset the central record states, when it is not the true one.
    pub offset: Option<u32>,
    /// False for an entry the central directory does not list.
    pub listed: bool,
}

fn len32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The honest header fields of `archive`'s entries.
pub fn raws_of(archive: &Archive) -> Vec<Raw> {
    archive
        .entries
        .iter()
        .map(|e| {
            let payload = if e.deflate { deflate(&e.data) } else { e.data.clone() };
            let method = if e.deflate { 8 } else { 0 };
            let crc = crc32fast::hash(&e.data);
            Raw {
                name: e.name.clone(),
                central_name: e.name.clone(),
                method,
                central_method: method,
                flags: 0,
                central_flags: 0,
                crc,
                central_crc: crc,
                compressed: len32(payload.len()),
                central_compressed: len32(payload.len()),
                size: len32(e.data.len()),
                central_size: len32(e.data.len()),
                extra: Vec::new(),
                central_extra: Vec::new(),
                payload,
                offset: None,
                listed: true,
            }
        })
        .collect()
}

/// What surrounds the entries: junk before the first header, the end record's fields when they are not the true ones, its comment and junk after it.
#[derive(Clone, Debug, Default)]
pub struct Tail {
    pub prefix: Vec<u8>,
    pub count: Option<u16>,
    pub directory_size: Option<u32>,
    pub directory_offset: Option<u32>,
    pub comment: Vec<u8>,
    pub comment_length: Option<u16>,
    pub suffix: Vec<u8>,
}

fn put16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn len16(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

/// An archive of `raws` as written, local headers and data first, then the central directory, then the end record, each field as the [`Raw`] and the [`Tail`] hold it.
pub fn write_raw(raws: &[Raw], tail: &Tail) -> Vec<u8> {
    let mut out = tail.prefix.clone();
    let mut offsets = Vec::with_capacity(raws.len());
    for r in raws {
        offsets.push(len32(out.len()));
        put32(&mut out, 0x0403_4B50);
        put16(&mut out, 20);
        put16(&mut out, r.flags);
        put16(&mut out, r.method);
        put16(&mut out, 0);
        put16(&mut out, 0x21);
        put32(&mut out, r.crc);
        put32(&mut out, r.compressed);
        put32(&mut out, r.size);
        put16(&mut out, len16(r.name.len()));
        put16(&mut out, len16(r.extra.len()));
        out.extend_from_slice(&r.name);
        out.extend_from_slice(&r.extra);
        out.extend_from_slice(&r.payload);
    }
    let directory = len32(out.len());
    let mut listed = 0u16;
    for (r, true_offset) in raws.iter().zip(&offsets) {
        if !r.listed {
            continue;
        }
        listed = listed.saturating_add(1);
        put32(&mut out, 0x0201_4B50);
        put16(&mut out, 45);
        put16(&mut out, 20);
        put16(&mut out, r.central_flags);
        put16(&mut out, r.central_method);
        put16(&mut out, 0);
        put16(&mut out, 0x21);
        put32(&mut out, r.central_crc);
        put32(&mut out, r.central_compressed);
        put32(&mut out, r.central_size);
        put16(&mut out, len16(r.central_name.len()));
        put16(&mut out, len16(r.central_extra.len()));
        put16(&mut out, 0);
        put16(&mut out, 0);
        put16(&mut out, 0);
        put32(&mut out, 0);
        put32(&mut out, r.offset.unwrap_or(*true_offset));
        out.extend_from_slice(&r.central_name);
        out.extend_from_slice(&r.central_extra);
    }
    let size = len32(out.len()) - directory;
    put32(&mut out, 0x0605_4B50);
    put16(&mut out, 0);
    put16(&mut out, 0);
    put16(&mut out, tail.count.unwrap_or(listed));
    put16(&mut out, tail.count.unwrap_or(listed));
    put32(&mut out, tail.directory_size.unwrap_or(size));
    put32(&mut out, tail.directory_offset.unwrap_or(directory));
    put16(&mut out, tail.comment_length.unwrap_or_else(|| len16(tail.comment.len())));
    out.extend_from_slice(&tail.comment);
    out.extend_from_slice(&tail.suffix);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Archive {
        Archive {
            entries: vec![
                Entry::new("mimetype", b"application/x-test".to_vec(), false),
                Entry::new("a/b.xml", b"<a>".repeat(200), true),
                Entry::new("empty", Vec::new(), true),
            ],
        }
    }

    #[test]
    fn an_archive_is_taken_apart_and_written_again_with_the_same_entries() {
        let archive = sample();
        let bytes = archive.honest();
        assert_eq!(Archive::parse(&bytes), Some(archive.clone()));
        assert_eq!(Archive::parse(&bytes).map(|a| a.honest()), Some(bytes), "the same bytes on a second writing");
        assert_eq!(archive.find("a/b.xml"), Some(1));
        assert_eq!(archive.find("none"), None);
        assert!(archive.entries.get(1).is_some_and(Entry::is_xml) && !archive.entries.first().is_some_and(Entry::is_xml));
    }

    #[test]
    fn what_this_writer_makes_is_read_by_the_projects_container_reader() {
        let limits = diacraft_container::Limits::default();
        let bytes = sample().honest();
        let read = diacraft_container::zip::Archive::read(&bytes, &limits).map(|a| a.entries().len());
        assert_eq!(read, Ok(3));
        // A lie the reader can see is refused in its words: a size, a checksum, a name that differs from the local header's.
        let archive = sample();
        let refused = |change: &dyn Fn(&mut Vec<Raw>)| {
            let mut raws = raws_of(&archive);
            change(&mut raws);
            let bytes = write_raw(&raws, &Tail::default());
            let mut zip = diacraft_container::zip::Archive::read(&bytes, &limits).ok()?;
            let mut work = diacraft_container::Work::default();
            (0..3).try_for_each(|i| zip.inflate(i, &mut work).map(drop)).err().map(|e| e.class())
        };
        assert_eq!(refused(&|_| {}), None);
        assert_eq!(refused(&|r| r[1].central_crc ^= 1), Some("zip_corrupt"));
        assert_eq!(refused(&|r| r[1].central_size += 1), Some("zip_corrupt"));
    }

    #[test]
    fn a_lie_changes_the_field_and_leaves_the_rest() {
        let archive = sample();
        let honest = archive.honest();
        let mut raws = raws_of(&archive);
        raws[1].central_size = 7;
        raws[2].listed = false;
        let lying = write_raw(&raws, &Tail { count: Some(9), comment: b"hi".to_vec(), ..Tail::default() });
        assert_ne!(lying, honest);
        assert!(lying.ends_with(b"hi"));
        assert_eq!(u16_at(&lying, lying.len() - 2 - 22 + 10), Some(9), "the end record's count is the lie");
        assert_eq!(Archive::parse(&lying).map(|a| a.entries.len()), None, "nine records are claimed and two are there");
    }
}
