//! A ZIP writer for tests, written by hand so that it can build what no honest writer would: a `mimetype` with an extra field in its local
//! header, an entry flagged encrypted, a size the header lies about, entries in any order. It is the container crate's test writer
//! (`crates/container/tests/it/zipbuild.rs`), cut to what these tests use. The format is PKWARE's APPNOTE, which is public.

use std::io::Write;

/// One archive entry as it sits in the file.
#[derive(Clone, Debug)]
pub struct Item {
    pub name: Vec<u8>,
    /// The name the local header holds when it is not the row's (the row and the entry's own name stay `name`).
    pub local_name: Option<Vec<u8>>,
    /// Compression method written in both headers: 0 stored, 8 deflate.
    pub method: u16,
    /// The bytes of the entry's data as they sit in the archive: compressed, when `method` is 8.
    pub data: Vec<u8>,
    /// The CRC-32 of the uncompressed bytes, as the headers state it.
    pub crc: u32,
    /// The uncompressed size, as the headers state it.
    pub size: u64,
    pub flags: u16,
    /// Bytes of the local header's extra field.
    pub local_extra: Vec<u8>,
    /// Bytes of the central row's extra field, and its comment.
    pub extra: Vec<u8>,
    pub comment: Vec<u8>,
    /// Bytes written right after the data: a data descriptor, or anything no header announces.
    pub after_data: Vec<u8>,
}

impl Item {
    /// An honest stored entry.
    pub fn stored(name: &str, content: &[u8]) -> Item {
        Item {
            name: name.as_bytes().to_vec(),
            local_name: None,
            method: 0,
            data: content.to_vec(),
            crc: crc32fast::hash(content),
            size: content.len() as u64,
            flags: 0,
            local_extra: Vec::new(),
            extra: Vec::new(),
            comment: Vec::new(),
            after_data: Vec::new(),
        }
    }

    /// An honest deflated entry.
    pub fn deflated(name: &str, content: &[u8]) -> Item {
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(content).expect("deflate into memory");
        let data = enc.finish().expect("deflate into memory");
        Item { method: 8, data, ..Item::stored(name, content) }
    }

    /// The same entry, with these flag bits also set.
    pub fn with_flags(mut self, bits: u16) -> Item {
        self.flags |= bits;
        self
    }

    /// The same entry, whose local header names it with these bytes while its row names it as it did.
    pub fn with_local_name(mut self, name: &[u8]) -> Item {
        self.local_name = Some(name.to_vec());
        self
    }

    /// The same entry, with these bytes in its local header's extra field.
    pub fn with_local_extra(mut self, extra: &[u8]) -> Item {
        self.local_extra.extend_from_slice(extra);
        self
    }
}

fn u16le(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// An archive of `items` in order, closed by a plain end record.
pub fn build(items: &[Item]) -> Vec<u8> {
    build_listed(items, false)
}

/// What an archive holds that no header announces, and a copy leaves out: bytes before the first entry (the offsets
/// count them), between the directory and the end record, an archive comment, and bytes after the end record.
#[derive(Clone, Default)]
pub struct Opts {
    pub prefix: Vec<u8>,
    pub before_end: Vec<u8>,
    pub comment: Vec<u8>,
    pub after_end: Vec<u8>,
}

/// An archive of `items` in order in the file, whose central directory lists them in the reverse order when `reversed`.
pub fn build_listed(items: &[Item], reversed: bool) -> Vec<u8> {
    build_with(items, reversed, &Opts::default())
}

/// [`build_listed`], laid out and closed as `opts` says.
pub fn build_with(items: &[Item], reversed: bool, opts: &Opts) -> Vec<u8> {
    let mut out = opts.prefix.clone();
    let mut rows: Vec<Vec<u8>> = Vec::new();
    for it in items {
        let mut central = Vec::new();
        let offset = out.len() as u32;
        u32le(&mut out, 0x0403_4B50);
        u16le(&mut out, 20);
        u16le(&mut out, it.flags);
        u16le(&mut out, it.method);
        u16le(&mut out, 0);
        u16le(&mut out, 0x21);
        u32le(&mut out, it.crc);
        u32le(&mut out, it.data.len() as u32);
        u32le(&mut out, it.size as u32);
        let local_name = it.local_name.as_ref().unwrap_or(&it.name);
        u16le(&mut out, local_name.len() as u16);
        u16le(&mut out, it.local_extra.len() as u16);
        out.extend_from_slice(local_name);
        out.extend_from_slice(&it.local_extra);
        out.extend_from_slice(&it.data);
        out.extend_from_slice(&it.after_data);
        u32le(&mut central, 0x0201_4B50);
        u16le(&mut central, 20);
        u16le(&mut central, 20);
        u16le(&mut central, it.flags);
        u16le(&mut central, it.method);
        u16le(&mut central, 0);
        u16le(&mut central, 0x21);
        u32le(&mut central, it.crc);
        u32le(&mut central, it.data.len() as u32);
        u32le(&mut central, it.size as u32);
        u16le(&mut central, it.name.len() as u16);
        u16le(&mut central, it.extra.len() as u16);
        u16le(&mut central, it.comment.len() as u16);
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u32le(&mut central, 0);
        u32le(&mut central, offset);
        central.extend_from_slice(&it.name);
        central.extend_from_slice(&it.extra);
        central.extend_from_slice(&it.comment);
        rows.push(central);
    }
    if reversed {
        rows.reverse();
    }
    let central = rows.concat();
    let (start, len) = (out.len() as u32, central.len() as u32);
    out.extend_from_slice(&central);
    out.extend_from_slice(&opts.before_end);
    u32le(&mut out, 0x0605_4B50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, items.len() as u16);
    u16le(&mut out, items.len() as u16);
    u32le(&mut out, len);
    u32le(&mut out, start);
    u16le(&mut out, opts.comment.len() as u16);
    out.extend_from_slice(&opts.comment);
    out.extend_from_slice(&opts.after_end);
    out
}
