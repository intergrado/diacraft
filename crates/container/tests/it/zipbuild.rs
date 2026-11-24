//! A zip writer for tests, written by hand so that it can build what no honest writer would: a size the header lies about, a name that
//! is not valid UTF-8, an encrypted flag, a method nobody supports, a Unicode path field for any name, a directory of rows full of extra
//! records, end records that claim what is not there. It is the `opc` crate's test writer (`crates/opc/tests/it/zipbuild.rs`), cut to
//! what these tests use: this crate is what both read through.
//!
//! The format is PKWARE's APPNOTE, which is public.

use std::io::Write;

/// One archive entry as it sits in the file.
#[derive(Clone, Debug)]
pub struct Item {
    /// The name's bytes, as written.
    pub name: Vec<u8>,
    /// Compression method written in both headers: 0 stored, 8 deflate.
    pub method: u16,
    /// The bytes of the entry's data as they sit in the archive: compressed, when `method` is 8.
    pub data: Vec<u8>,
    /// The CRC-32 of the uncompressed bytes, as the headers state it.
    pub crc: u32,
    /// The uncompressed size, as the headers state it.
    pub size: u64,
    /// General purpose bit flags.
    pub flags: u16,
    /// Bytes of the central row's extra field.
    pub extra: Vec<u8>,
    /// Bytes of the local header's extra field.
    pub local_extra: Vec<u8>,
    /// The row's "version made by", time, date, internal and external attributes, and comment (the local header has the same time and
    /// date).
    pub made_by: u16,
    pub time: u16,
    pub date: u16,
    pub internal: u16,
    pub external: u32,
    pub comment: Vec<u8>,
    /// What the local header says where it disagrees with the row: its name, its method, its flags, and its checksum and two sizes.
    pub local_name: Option<Vec<u8>>,
    pub local_method: Option<u16>,
    pub local_flags: Option<u16>,
    pub local_sizes: Option<(u32, u32, u32)>,
    /// Bytes written right after the data: a data descriptor, or anything.
    pub after_data: Vec<u8>,
}

impl Item {
    fn raw(name: &str, method: u16, data: Vec<u8>, crc: u32, size: u64) -> Item {
        Item {
            name: name.as_bytes().to_vec(),
            method,
            data,
            crc,
            size,
            flags: 0,
            extra: Vec::new(),
            local_extra: Vec::new(),
            made_by: 45,
            time: 0,
            date: 0x21,
            internal: 0,
            external: 0,
            comment: Vec::new(),
            local_name: None,
            local_method: None,
            local_flags: None,
            local_sizes: None,
            after_data: Vec::new(),
        }
    }

    /// An honest stored entry.
    pub fn stored(name: &str, content: &[u8]) -> Item {
        Item::raw(name, 0, content.to_vec(), crc32fast::hash(content), content.len() as u64)
    }

    /// An honest deflated entry.
    pub fn deflated(name: &str, content: &[u8]) -> Item {
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(content).expect("deflate into memory");
        let data = enc.finish().expect("deflate into memory");
        Item::raw(name, 8, data, crc32fast::hash(content), content.len() as u64)
    }

    /// The same entry, with the headers claiming `size` uncompressed bytes.
    pub fn claiming_size(mut self, size: u64) -> Item {
        self.size = size;
        self
    }

    /// The same entry, with another method number in the headers (the data is not changed).
    pub fn with_method(mut self, method: u16) -> Item {
        self.method = method;
        self
    }

    /// The same entry, with these flag bits also set.
    pub fn with_flags(mut self, bits: u16) -> Item {
        self.flags |= bits;
        self
    }

    /// The same entry, with its name replaced by raw bytes.
    pub fn named_bytes(mut self, name: &[u8]) -> Item {
        self.name = name.to_vec();
        self
    }

    /// The same entry, with these bytes in its row's extra field.
    pub fn with_extra(mut self, extra: &[u8]) -> Item {
        self.extra.extend_from_slice(extra);
        self
    }

    /// The same entry, with these bytes in its local header's extra field.
    pub fn with_local_extra(mut self, extra: &[u8]) -> Item {
        self.local_extra.extend_from_slice(extra);
        self
    }

    /// The same entry, with a Unicode path field (Info-ZIP's `up`) for the name it has now, giving `path`.
    pub fn with_unicode_path(self, path: &str) -> Item {
        let mut body = vec![1u8];
        body.extend_from_slice(&crc32fast::hash(&self.name).to_le_bytes());
        body.extend_from_slice(path.as_bytes());
        let record = extra_record(0x7075, &body);
        self.with_extra(&record)
    }
}

/// One record of an extra field: `id`, the size of `body`, `body`.
pub fn extra_record(id: u16, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&(body.len() as u16).to_le_bytes());
    out.extend_from_slice(body);
    out
}

/// How the archive is laid out and closed.
#[derive(Clone, Default)]
pub struct Opts {
    /// Write the ZIP64 end records, and put 0xFFFF in the plain ones.
    pub zip64: bool,
    /// Bytes of archive comment.
    pub comment: Vec<u8>,
    /// Bytes before the first entry (the offsets count them: they are not a stub that shifts the archive).
    pub prefix: Vec<u8>,
    /// List the rows in the directory the other way round from the order the entries lie in the file.
    pub reversed: bool,
    /// Bytes of the directory after its last row, which its size counts.
    pub directory_tail: Vec<u8>,
    /// Bytes between the directory and the end records.
    pub before_end: Vec<u8>,
    /// Bytes after the end record and its comment.
    pub after_end: Vec<u8>,
}

fn u16le(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn u32le(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn u64le(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// An archive of `items` in order, closed by plain end records.
pub fn build(items: &[Item]) -> Vec<u8> {
    build_with(items, &Opts::default())
}

/// An archive of `items`, closed as `opts` says.
pub fn build_with(items: &[Item], opts: &Opts) -> Vec<u8> {
    let mut out = opts.prefix.clone();
    let mut rows = Vec::new();
    for it in items {
        let offset = out.len() as u32;
        local_header(&mut out, it.local_name.as_deref().unwrap_or(&it.name), it, &it.local_extra);
        out.extend_from_slice(&it.data);
        out.extend_from_slice(&it.after_data);
        let mut one = Vec::new();
        row(&mut one, &it.name, it, &it.extra, offset);
        rows.push(one);
    }
    if opts.reversed {
        rows.reverse();
    }
    let mut central = rows.concat();
    central.extend_from_slice(&opts.directory_tail);
    let (start, len) = (out.len() as u64, central.len() as u64);
    out.extend_from_slice(&central);
    out.extend_from_slice(&opts.before_end);
    close(&mut out, opts, items.len() as u64, len, start);
    out.extend_from_slice(&opts.after_end);
    out
}

fn local_header(out: &mut Vec<u8>, name: &[u8], it: &Item, extra: &[u8]) {
    let (crc, compressed, size) = it.local_sizes.unwrap_or((it.crc, it.data.len() as u32, it.size as u32));
    u32le(out, 0x0403_4B50);
    u16le(out, 20);
    u16le(out, it.local_flags.unwrap_or(it.flags));
    u16le(out, it.local_method.unwrap_or(it.method));
    u16le(out, it.time);
    u16le(out, it.date);
    u32le(out, crc);
    u32le(out, compressed);
    u32le(out, size);
    u16le(out, name.len() as u16);
    u16le(out, extra.len() as u16);
    out.extend_from_slice(name);
    out.extend_from_slice(extra);
}

fn row(central: &mut Vec<u8>, name: &[u8], it: &Item, extra: &[u8], offset: u32) {
    u32le(central, 0x0201_4B50);
    u16le(central, it.made_by);
    u16le(central, 20);
    u16le(central, it.flags);
    u16le(central, it.method);
    u16le(central, it.time);
    u16le(central, it.date);
    u32le(central, it.crc);
    u32le(central, it.data.len() as u32);
    u32le(central, it.size as u32);
    u16le(central, name.len() as u16);
    u16le(central, extra.len() as u16);
    u16le(central, it.comment.len() as u16);
    u16le(central, 0); // disk
    u16le(central, it.internal);
    u32le(central, it.external);
    u32le(central, offset);
    central.extend_from_slice(name);
    central.extend_from_slice(extra);
    central.extend_from_slice(&it.comment);
}

/// The end records, written after a directory of `cd_len` bytes at `cd_start` that is said to hold `count` rows.
pub fn close(out: &mut Vec<u8>, opts: &Opts, count: u64, cd_len: u64, cd_start: u64) {
    if opts.zip64 {
        let z64_at = out.len() as u64;
        u32le(out, 0x0606_4B50);
        u64le(out, 44);
        u16le(out, 45);
        u16le(out, 45);
        u32le(out, 0);
        u32le(out, 0);
        u64le(out, count);
        u64le(out, count);
        u64le(out, cd_len);
        u64le(out, cd_start);
        u32le(out, 0x0706_4B50);
        u32le(out, 0);
        u64le(out, z64_at);
        u32le(out, 1);
    }
    u32le(out, 0x0605_4B50);
    u16le(out, 0);
    u16le(out, 0);
    if opts.zip64 {
        u16le(out, 0xFFFF);
        u16le(out, 0xFFFF);
        u32le(out, 0xFFFF_FFFF);
        u32le(out, 0xFFFF_FFFF);
    } else {
        u16le(out, count.min(0xFFFF) as u16);
        u16le(out, count.min(0xFFFF) as u16);
        u32le(out, cd_len as u32);
        u32le(out, cd_start as u32);
    }
    u16le(out, opts.comment.len() as u16);
    out.extend_from_slice(&opts.comment);
}

/// An archive of `names`, each an empty stored entry with `extra` bytes of empty extra records in its central row (0 for none) and an
/// empty local name, written straight to bytes with ZIP64 end records. The names are made one at a time, so that an archive of many rows
/// with the longest names costs what it is and no more.
pub fn many_entries(names: impl Iterator<Item = Vec<u8>>, extra: usize) -> Vec<u8> {
    // An extra field of `extra` bytes: records with an unknown id and no body (4 bytes each), the rest as one record's worth of zeros.
    let extra_field = {
        let mut field = Vec::with_capacity(extra);
        while field.len() + 4 <= extra {
            field.extend_from_slice(&extra_record(0xCAFE, &[]));
        }
        field.resize(extra, 0);
        field
    };
    let mut out = Vec::new();
    let mut central = Vec::new();
    let mut count = 0u64;
    for name in names {
        let it = Item::stored("", b"");
        let offset = out.len() as u32;
        local_header(&mut out, &[], &it, &[]);
        row(&mut central, &name, &it, &extra_field, offset);
        count += 1;
    }
    let (start, len) = (out.len() as u64, central.len() as u64);
    out.extend_from_slice(&central);
    close(&mut out, &Opts { zip64: true, ..Opts::default() }, count, len, start);
    out
}
