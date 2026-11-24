//! A zip writer for tests, written by hand so that it can build what no honest writer would: a size the header lies about, a
//! checksum that is wrong, a name that is not valid UTF-8, an encrypted flag, a method nobody supports, a central directory that
//! claims a billion entries, entries that overlap. (The `zip` crate's writer refuses to make most of these, and a test that goes
//! through it tests only what it will write.)
//!
//! The format is PKWARE's APPNOTE, which is public; nothing here comes from a VSDX file.

use std::io::Write;

/// What the local header says where the test wants it to disagree with the central row (the central row is the item's own fields).
#[derive(Clone, Debug, Default)]
pub struct Local {
    pub name: Option<Vec<u8>>,
    pub method: Option<u16>,
    pub flags: Option<u16>,
    pub crc: Option<u32>,
    /// `(compressed, uncompressed)` as the local header states them.
    pub sizes: Option<(u64, u64)>,
    /// Bytes of the local header's extra field (the central row's is `Item::extra`).
    pub extra: Vec<u8>,
}

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
    /// The compressed size, as the headers state it.
    pub csize: u64,
    /// General purpose bit flags.
    pub flags: u16,
    /// When set, this entry has no local header and no data of its own: its central directory row points at the local header of
    /// the entry with this index, as the overlapping-entries bomb does.
    pub alias_of: Option<usize>,
    /// Bytes of the central row's extra field, after the ZIP64 field the writer adds when the sizes call for one.
    pub extra: Vec<u8>,
    /// Bytes of the central row's comment.
    pub row_comment: Vec<u8>,
    /// Write the sizes as ZIP64 sentinels with an honest ZIP64 field, though they are small.
    pub zip64: bool,
    /// Where the local header disagrees with the row.
    pub local: Local,
    /// Bytes written right after the data and covered by no size: a data descriptor, true or false.
    pub descriptor: Vec<u8>,
}

impl Item {
    fn raw(name: &str, method: u16, data: Vec<u8>, crc: u32, size: u64) -> Item {
        Item {
            name: name.as_bytes().to_vec(),
            method,
            csize: data.len() as u64,
            data,
            crc,
            size,
            flags: 0,
            alias_of: None,
            extra: Vec::new(),
            row_comment: Vec::new(),
            zip64: false,
            local: Local::default(),
            descriptor: Vec::new(),
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

    /// An honest deflated entry of `n` zero bytes (at least 1) that costs the writer almost nothing: the stream is written by
    /// hand, 2 bits for every 258 bytes, so the ratio is the format's own limit, about 1,032.
    pub fn zeros(name: &str, n: u64) -> Item {
        Item::raw(name, 8, deflate_zeros(n), crc32_of_zeros(n), n)
    }

    /// Like [`Item::zeros`], but a ratio of about 518 (under the cap of 1,000): after each run of 258 a literal is written, so that
    /// the part of `n` bytes reads under the default ratio limit and is as large as a part may be at almost no cost.
    pub fn zeros_under_the_ratio_cap(name: &str, n: u64) -> Item {
        Item::raw(name, 8, deflate_zeros_with_literals(n), crc32_of_zeros(n), n)
    }

    /// The same entry, with the headers claiming `size` uncompressed bytes.
    pub fn claiming_size(mut self, size: u64) -> Item {
        self.size = size;
        self
    }

    /// The same entry, with the headers claiming `csize` compressed bytes.
    pub fn claiming_csize(mut self, csize: u64) -> Item {
        self.csize = csize;
        self
    }

    /// The same entry, with another method number in the headers (the data is not changed).
    pub fn with_method(mut self, method: u16) -> Item {
        self.method = method;
        self
    }

    /// The same entry, with the encrypted flag set.
    pub fn encrypted(mut self) -> Item {
        self.flags |= 1;
        self
    }

    /// The same entry, with these flag bits also set.
    pub fn with_flags(mut self, bits: u16) -> Item {
        self.flags |= bits;
        self
    }

    /// The same entry, with a wrong checksum.
    pub fn with_bad_crc(mut self) -> Item {
        self.crc ^= 0xDEAD_BEEF;
        self
    }

    /// The same entry with no data of its own: its row in the central directory points at entry `index`'s local header.
    pub fn aliasing(mut self, index: usize) -> Item {
        self.alias_of = Some(index);
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

    /// The same entry, with a Unicode path field (Info-ZIP's `up`) for the name it has now, giving `path`.
    pub fn with_unicode_path(self, path: &str) -> Item {
        let mut body = vec![1u8];
        body.extend_from_slice(&crc32fast::hash(&self.name).to_le_bytes());
        body.extend_from_slice(path.as_bytes());
        let record = extra_record(0x7075, &body);
        self.with_extra(&record)
    }

    /// The same entry, sizes written as ZIP64 sentinels with an honest ZIP64 field.
    pub fn with_zip64_field(mut self) -> Item {
        self.zip64 = true;
        self
    }

    /// The same entry, with `bytes` after its data, covered by no size.
    pub fn with_descriptor(mut self, bytes: &[u8]) -> Item {
        self.descriptor = bytes.to_vec();
        self
    }

    /// The same entry, with its local header made to disagree as `local` says.
    pub fn with_local(mut self, local: Local) -> Item {
        self.local = local;
        self
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

/// How the archive is closed.
#[derive(Clone, Default)]
pub struct Opts {
    /// What the end record claims for the number of entries, in place of the real number.
    pub claimed_entries: Option<u64>,
    /// Write the ZIP64 end records, and put 0xFFFF in the plain ones.
    pub zip64: bool,
    /// Bytes of archive comment.
    pub comment: Vec<u8>,
    /// Bytes put in front of the first local header (a self-extracting stub, say). Offsets are not shifted, as with a real stub.
    pub prefix: Vec<u8>,
    /// Write the rows of the central directory in the opposite order to the entries: the last entry's row first.
    pub reverse_rows: bool,
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

/// The sizes of an item as its row writes them: ZIP64 sentinels and a field, or plain 32-bit numbers.
fn wide(it: &Item) -> bool {
    it.zip64 || it.size >= 0xFFFF_FFFF || it.csize >= 0xFFFF_FFFF
}

/// An archive of `items`, closed as `opts` says.
pub fn build_with(items: &[Item], opts: &Opts) -> Vec<u8> {
    let (mut out, dir) = entries_and_directory(items, opts);
    let count = opts.claimed_entries.unwrap_or(dir.count);
    close(&mut out, opts, count, dir.len, dir.start);
    out
}

/// Where the directory of an archive is, and how many rows it has.
#[derive(Clone, Copy, Debug)]
pub struct Dir {
    pub count: u64,
    pub len: u64,
    pub start: u64,
}

/// The entries and the central directory of `items`, and no end record: for a test that writes the end records by hand ([`Ends`]).
pub fn without_end(items: &[Item]) -> (Vec<u8>, Dir) {
    entries_and_directory(items, &Opts::default())
}

/// The numbers of the ZIP64 end record and its locator, each as given.
#[derive(Clone, Debug)]
pub struct Zip64 {
    pub disk: u32,
    pub directory_disk: u32,
    pub on_disk: u64,
    pub total: u64,
    pub size: u64,
    pub start: u64,
    /// The locator's disk of the ZIP64 record, and its number of disks.
    pub locator_disk: u32,
    pub locator_disks: u32,
}

/// Every number of the end records, set by hand, for the shapes that no writer makes and the ones that real writers do (one field
/// of the plain record given as "look elsewhere" and the others real).
#[derive(Clone, Debug)]
pub struct Ends {
    pub disk: u16,
    pub directory_disk: u16,
    pub on_disk: u16,
    pub total: u16,
    pub size: u32,
    pub start: u32,
    pub zip64: Option<Zip64>,
    pub comment: Vec<u8>,
}

impl Ends {
    /// The plain end record of an honest single-disk archive with a directory at `dir`, and no ZIP64 records.
    pub fn plain(dir: Dir) -> Ends {
        let count = dir.count as u16;
        Ends {
            disk: 0,
            directory_disk: 0,
            on_disk: count,
            total: count,
            size: dir.len as u32,
            start: dir.start as u32,
            zip64: None,
            comment: Vec::new(),
        }
    }

    /// Honest ZIP64 end records, and a plain record that gives every number as "look elsewhere" (as writers do for an archive that
    /// needs no more than one of them).
    pub fn wide(dir: Dir) -> Ends {
        Ends {
            disk: 0xFFFF,
            directory_disk: 0xFFFF,
            on_disk: 0xFFFF,
            total: 0xFFFF,
            size: 0xFFFF_FFFF,
            start: 0xFFFF_FFFF,
            zip64: Some(Zip64 {
                disk: 0,
                directory_disk: 0,
                on_disk: dir.count,
                total: dir.count,
                size: dir.len,
                start: dir.start,
                locator_disk: 0,
                locator_disks: 1,
            }),
            comment: Vec::new(),
        }
    }
}

/// Writes the end records `ends` says at the end of `out`: the ZIP64 record and its locator when `zip64` is set, then the plain record
/// and the comment.
pub fn write_ends(out: &mut Vec<u8>, ends: &Ends) {
    if let Some(z) = &ends.zip64 {
        let z64_at = out.len() as u64;
        u32le(out, 0x0606_4B50);
        u64le(out, 44);
        u16le(out, 45);
        u16le(out, 45);
        u32le(out, z.disk);
        u32le(out, z.directory_disk);
        u64le(out, z.on_disk);
        u64le(out, z.total);
        u64le(out, z.size);
        u64le(out, z.start);
        u32le(out, 0x0706_4B50);
        u32le(out, z.locator_disk);
        u64le(out, z64_at);
        u32le(out, z.locator_disks);
    }
    u32le(out, 0x0605_4B50);
    u16le(out, ends.disk);
    u16le(out, ends.directory_disk);
    u16le(out, ends.on_disk);
    u16le(out, ends.total);
    u32le(out, ends.size);
    u32le(out, ends.start);
    u16le(out, ends.comment.len() as u16);
    out.extend_from_slice(&ends.comment);
}

fn entries_and_directory(items: &[Item], opts: &Opts) -> (Vec<u8>, Dir) {
    let mut out = opts.prefix.clone();
    let mut offsets = Vec::with_capacity(items.len());
    for it in items {
        if let Some(target) = it.alias_of {
            offsets.push(offsets[target]);
            continue;
        }
        offsets.push((out.len() - opts.prefix.len()) as u64);
        let big = wide(it);
        let name = it.local.name.as_ref().unwrap_or(&it.name);
        // The local header states the sizes it is told to, or the item's own; ZIP64 sentinels only if it is not lying about them.
        let local_wide = big && it.local.sizes.is_none();
        let (local_csize, local_size) = it.local.sizes.unwrap_or((it.csize, it.size));
        let mut extra = if local_wide { extra_record(1, &[it.size.to_le_bytes(), it.csize.to_le_bytes()].concat()) } else { Vec::new() };
        extra.extend_from_slice(&it.local.extra);
        u32le(&mut out, 0x0403_4B50);
        u16le(&mut out, if big { 45 } else { 20 });
        u16le(&mut out, it.local.flags.unwrap_or(it.flags));
        u16le(&mut out, it.local.method.unwrap_or(it.method));
        u16le(&mut out, 0); // time
        u16le(&mut out, 0x21); // date: 1980-01-01
        u32le(&mut out, it.local.crc.unwrap_or(it.crc));
        if local_wide {
            u32le(&mut out, 0xFFFF_FFFF);
            u32le(&mut out, 0xFFFF_FFFF);
        } else {
            u32le(&mut out, local_csize as u32);
            u32le(&mut out, local_size as u32);
        }
        u16le(&mut out, name.len() as u16);
        u16le(&mut out, extra.len() as u16);
        out.extend_from_slice(name);
        out.extend_from_slice(&extra);
        out.extend_from_slice(&it.data);
        out.extend_from_slice(&it.descriptor);
    }
    let cd_start = (out.len() - opts.prefix.len()) as u64;
    let mut rows: Vec<(&Item, &u64)> = items.iter().zip(&offsets).collect();
    if opts.reverse_rows {
        rows.reverse();
    }
    for (it, off) in rows {
        let big = wide(it);
        let mut extra = if big { extra_record(1, &[it.size.to_le_bytes(), it.csize.to_le_bytes()].concat()) } else { Vec::new() };
        extra.extend_from_slice(&it.extra);
        u32le(&mut out, 0x0201_4B50);
        u16le(&mut out, 45); // made by
        u16le(&mut out, if big { 45 } else { 20 });
        u16le(&mut out, it.flags);
        u16le(&mut out, it.method);
        u16le(&mut out, 0);
        u16le(&mut out, 0x21);
        u32le(&mut out, it.crc);
        if big {
            u32le(&mut out, 0xFFFF_FFFF);
            u32le(&mut out, 0xFFFF_FFFF);
        } else {
            u32le(&mut out, it.csize as u32);
            u32le(&mut out, it.size as u32);
        }
        u16le(&mut out, it.name.len() as u16);
        u16le(&mut out, extra.len() as u16);
        u16le(&mut out, it.row_comment.len() as u16);
        u16le(&mut out, 0); // disk
        u16le(&mut out, 0); // internal attributes
        u32le(&mut out, 0); // external attributes
        u32le(&mut out, *off as u32);
        out.extend_from_slice(&it.name);
        out.extend_from_slice(&extra);
        out.extend_from_slice(&it.row_comment);
    }
    let cd_len = (out.len() - opts.prefix.len()) as u64 - cd_start;
    (out, Dir { count: items.len() as u64, len: cd_len, start: cd_start })
}

/// The end records, written after a directory of `cd_len` bytes at `cd_start` that is said to hold `count` rows.
pub fn close(out: &mut Vec<u8>, opts: &Opts, count: u64, cd_len: u64, cd_start: u64) {
    if opts.zip64 {
        let z64_at = (out.len() - opts.prefix.len()) as u64;
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

/// The CRC-32 of `n` zero bytes.
pub fn crc32_of_zeros(n: u64) -> u32 {
    let zeros = [0u8; 65536];
    let mut h = crc32fast::Hasher::new();
    let mut left = n;
    while left > 0 {
        let k = left.min(zeros.len() as u64) as usize;
        h.update(&zeros[..k]);
        left -= k as u64;
    }
    h.finalize()
}

/// Bits into bytes, least significant bit first, as deflate wants them.
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn put(&mut self, value: u32, nbits: u32) {
        self.acc |= u64::from(value) << self.n;
        self.n += nbits;
        while self.n >= 8 {
            self.out.push((self.acc & 0xFF) as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    /// A Huffman code: its bits go in most significant first.
    fn code(&mut self, code: u32, nbits: u32) {
        let mut rev = 0;
        for i in 0..nbits {
            rev |= ((code >> i) & 1) << (nbits - 1 - i);
        }
        self.put(rev, nbits);
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push((self.acc & 0xFF) as u8);
        }
        self.out
    }
}

/// A raw deflate stream that inflates to `n` zero bytes (at least 1) and is as small as the format allows: one final block with
/// its own Huffman codes, in which the length-258 symbol and the distance-1 symbol each take one bit, so every 258 bytes cost
/// 2 bits. About 1,032 bytes out for each byte in, which is deflate's own limit, and more than the ratio cap of 1,000.
///
/// The code lengths: literal 0 and end-of-block two bits, symbol 285 (length 258) one bit; one distance code of one bit. They are
/// sent through a code-length code of three symbols (1, 2 and 18, "repeat a zero length"), of lengths 2, 2 and 1.
pub fn deflate_zeros(n: u64) -> Vec<u8> {
    deflate_zeros_impl(n, false)
}

/// The stream of [`deflate_zeros`], with a literal zero after each run of 258: 259 bytes for 4 bits, a ratio of about 518.
pub fn deflate_zeros_with_literals(n: u64) -> Vec<u8> {
    deflate_zeros_impl(n, true)
}

fn deflate_zeros_impl(n: u64, with_literals: bool) -> Vec<u8> {
    assert!(n >= 1, "a stream of zero bytes needs at least one");
    let mut b = Bits { out: Vec::new(), acc: 0, n: 0 };
    b.put(1, 1); // final block
    b.put(2, 2); // dynamic Huffman codes
    b.put(29, 5); // 286 literal/length code lengths
    b.put(0, 5); // 1 distance code length
    b.put(14, 4); // 18 code-length code lengths, in the order 16 17 18 0 8 7 9 6 10 5 11 4 12 3 13 2 14 1
    for len in [0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 2] {
        b.put(len, 3);
    }
    // The 287 lengths: [2, 0 x 255, 2, 0 x 28, 1] for the literal/length alphabet and [1] for the distance one.
    b.code(0b11, 2); // length 2: literal 0
    b.code(0, 1); // 18: a run of zeros
    b.put(138 - 11, 7); // 138 of them
    b.code(0, 1);
    b.put(117 - 11, 7); // 117 more: symbols 1 to 255
    b.code(0b11, 2); // length 2: end of block
    b.code(0, 1);
    b.put(28 - 11, 7); // symbols 257 to 284
    b.code(0b10, 2); // length 1: symbol 285
    b.code(0b10, 2); // length 1: the distance code
    // The data. Literal/length codes: symbol 285 is `0`, literal 0 is `10`, end of block is `11`; the distance code is `0`.
    b.code(0b10, 2); // literal 0
    let mut left = n - 1;
    while left >= 258 {
        b.code(0, 1); // length 258
        b.code(0, 1); // distance 1
        left -= 258;
        if with_literals && left >= 1 {
            b.code(0b10, 2);
            left -= 1;
        }
    }
    for _ in 0..left {
        b.code(0b10, 2);
    }
    b.code(0b11, 2); // end of block
    b.finish()
}

/// An archive of `n` empty stored entries named `p/0`, `p/1`, ... , written straight to bytes (about 90 bytes each), after the
/// content types stream the reader needs. For the million-entry case, where an `Item` per entry would cost more than the archive.
pub fn many_empty_entries(n: u32, content_types: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(n as usize * 100 + 1024);
    let mut central = Vec::with_capacity(n as usize * 60 + 1024);
    let entry = |out: &mut Vec<u8>, central: &mut Vec<u8>, name: &[u8], data: &[u8]| {
        let offset = out.len() as u32;
        let crc = crc32fast::hash(data);
        u32le(out, 0x0403_4B50);
        u16le(out, 20);
        u16le(out, 0);
        u16le(out, 0);
        u16le(out, 0);
        u16le(out, 0x21);
        u32le(out, crc);
        u32le(out, data.len() as u32);
        u32le(out, data.len() as u32);
        u16le(out, name.len() as u16);
        u16le(out, 0);
        out.extend_from_slice(name);
        out.extend_from_slice(data);
        u32le(central, 0x0201_4B50);
        u16le(central, 45);
        u16le(central, 20);
        u16le(central, 0);
        u16le(central, 0);
        u16le(central, 0);
        u16le(central, 0x21);
        u32le(central, crc);
        u32le(central, data.len() as u32);
        u32le(central, data.len() as u32);
        u16le(central, name.len() as u16);
        u16le(central, 0);
        u16le(central, 0);
        u16le(central, 0);
        u16le(central, 0);
        u32le(central, 0);
        u32le(central, offset);
        central.extend_from_slice(name);
    };
    entry(&mut out, &mut central, b"[Content_Types].xml", content_types);
    for i in 0..n {
        entry(&mut out, &mut central, format!("p/{i}").as_bytes(), b"");
    }
    let count = u64::from(n) + 1;
    let cd_start = out.len() as u32;
    let cd_len = central.len() as u32;
    out.extend_from_slice(&central);
    // ZIP64 end records, as any writer makes them past 65,535 entries.
    let z64_at = out.len() as u64;
    u32le(&mut out, 0x0606_4B50);
    u64le(&mut out, 44);
    u16le(&mut out, 45);
    u16le(&mut out, 45);
    u32le(&mut out, 0);
    u32le(&mut out, 0);
    u64le(&mut out, count);
    u64le(&mut out, count);
    u64le(&mut out, u64::from(cd_len));
    u64le(&mut out, u64::from(cd_start));
    u32le(&mut out, 0x0706_4B50);
    u32le(&mut out, 0);
    u64le(&mut out, z64_at);
    u32le(&mut out, 1);
    u32le(&mut out, 0x0605_4B50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, 0xFFFF);
    u16le(&mut out, 0xFFFF);
    u32le(&mut out, 0xFFFF_FFFF);
    u32le(&mut out, 0xFFFF_FFFF);
    u16le(&mut out, 0);
    out
}

/// An archive of the content types stream and then `names`, each an empty stored entry with `extra` bytes of an empty-records extra
/// field in its central row (0 for none), written straight to bytes with ZIP64 end records. The names are made one at a time, so
/// that an archive of a hundred thousand rows with the longest names costs what it is and no more.
pub fn many_entries(names: impl Iterator<Item = Vec<u8>>, extra: usize, content_types: &[u8]) -> Vec<u8> {
    many_entries_with(names, extra, content_types, true)
}

/// Like [`many_entries`], with the local headers' names empty when `local_names` is false (the reader never uses them, and a file of
/// fifty thousand names of 1,024 bytes is then 55 MB and not 107).
pub fn many_entries_with(names: impl Iterator<Item = Vec<u8>>, extra: usize, content_types: &[u8], local_names: bool) -> Vec<u8> {
    // An extra field of `extra` bytes: records with an unknown id and no body (4 bytes each), the rest as one record.
    let extra_field = {
        let mut field = Vec::with_capacity(extra);
        while field.len() + 4 <= extra {
            field.extend_from_slice(&extra_record(0xCAFE, &[]));
        }
        field.resize(extra, 0);
        field
    };
    many_entries_rows(names.map(|name| (name, extra_field.clone())), content_types, local_names)
}

/// Like [`many_entries_with`], with a Unicode path field (Info-ZIP's `up`) in each row's extra field, giving the second of each pair as
/// the name for the first, as written.
#[cfg(target_os = "linux")]
pub fn many_entries_with_unicode_paths(rows: impl Iterator<Item = (Vec<u8>, String)>, content_types: &[u8], local_names: bool) -> Vec<u8> {
    many_entries_rows(
        rows.map(|(written, path)| {
            let mut body = vec![1u8];
            body.extend_from_slice(&crc32fast::hash(&written).to_le_bytes());
            body.extend_from_slice(path.as_bytes());
            (written, extra_record(0x7075, &body))
        }),
        content_types,
        local_names,
    )
}

/// Each empty stored entry of `rows` is a name as written and the bytes of its row's extra field.
fn many_entries_rows(rows: impl Iterator<Item = (Vec<u8>, Vec<u8>)>, content_types: &[u8], local_names: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    let mut count = 0u64;
    let mut entry = |out: &mut Vec<u8>, central: &mut Vec<u8>, name: &[u8], data: &[u8], extra: &[u8]| {
        let offset = out.len() as u32;
        let crc = crc32fast::hash(data);
        u32le(out, 0x0403_4B50);
        u16le(out, 20);
        u16le(out, 0);
        u16le(out, 0);
        u16le(out, 0);
        u16le(out, 0x21);
        u32le(out, crc);
        u32le(out, data.len() as u32);
        u32le(out, data.len() as u32);
        let local: &[u8] = if local_names { name } else { &[] };
        u16le(out, local.len() as u16);
        u16le(out, 0);
        out.extend_from_slice(local);
        out.extend_from_slice(data);
        u32le(central, 0x0201_4B50);
        u16le(central, 45);
        u16le(central, 20);
        u16le(central, 0);
        u16le(central, 0);
        u16le(central, 0);
        u16le(central, 0x21);
        u32le(central, crc);
        u32le(central, data.len() as u32);
        u32le(central, data.len() as u32);
        u16le(central, name.len() as u16);
        u16le(central, extra.len() as u16);
        u16le(central, 0);
        u16le(central, 0);
        u16le(central, 0);
        u32le(central, 0);
        u32le(central, offset);
        central.extend_from_slice(name);
        central.extend_from_slice(extra);
        count += 1;
    };
    entry(&mut out, &mut central, b"[Content_Types].xml", content_types, &[]);
    for (name, extra) in rows {
        entry(&mut out, &mut central, &name, b"", &extra);
    }
    let (cd_start, cd_len) = (out.len() as u64, central.len() as u64);
    out.extend_from_slice(&central);
    close(&mut out, &Opts { zip64: true, ..Opts::default() }, count, cd_len, cd_start);
    out
}

/// A file of `rows` central directory rows (each 50 bytes, with a four-byte name that differs from row to row) and ZIP64 end records, behind `stub` bytes that
/// shift every offset, and with no local header, no data and no content types stream behind any of them: the shape of a hostile
/// directory, which costs the writer nothing but the rows.
pub fn rows_only(rows: u32, stub: usize) -> Vec<u8> {
    let mut out = vec![b'S'; stub];
    let start = out.len() as u64;
    for row in 0..rows {
        u32le(&mut out, 0x0201_4B50);
        u16le(&mut out, 45);
        u16le(&mut out, 20);
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u16le(&mut out, 0x21);
        u32le(&mut out, 0);
        u32le(&mut out, 0);
        u32le(&mut out, 0);
        u16le(&mut out, 4);
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u32le(&mut out, 0);
        u32le(&mut out, 0);
        out.extend_from_slice(&row.to_be_bytes());
    }
    let len = out.len() as u64 - start;
    // The end records give offsets from byte 0 of the archive, which the stub has moved (as a real stub does).
    let stubbed = Opts { zip64: true, prefix: out.get(..stub).unwrap_or_default().to_vec(), ..Opts::default() };
    close(&mut out, &stubbed, u64::from(rows), len, start - stub as u64);
    out
}

/// `inflate(deflate_zeros(n))` is `n` zeros: the writer above is tested against the inflater the crate uses, so that a test built
/// on it tests the crate and not a broken stream.
pub fn inflate_all(deflated: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(deflated).read_to_end(&mut out).expect("a valid stream");
    out
}
