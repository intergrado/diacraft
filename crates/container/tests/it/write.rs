//! The writer: what an ODF package needs of the ZIP layer (the design's "The zip layer"), and
//! what any archive it writes must be (local and central headers that agree, the method asked for, the same bytes for the same entries),
//! each read back with this crate's reader and checked byte by byte against the APPNOTE's layout of the headers (section 4.3.7 for the
//! local header, 4.3.12 for a central row, 4.3.16 for the end record).

use diacraft_container::zip::{Archive, ArchiveWriter, Method};
use diacraft_container::{Limits, Work};

use crate::read_all;
use crate::zipbuild::{Item, build};

const MEDIA_TYPE: &[u8] = b"application/vnd.oasis.opendocument.graphics";

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// The fields a local header and a central row both state.
#[derive(Debug, PartialEq, Eq)]
struct Header {
    version_needed: u16,
    flags: u16,
    method: u16,
    time: u16,
    date: u16,
    crc: u32,
    compressed: u32,
    size: u32,
    name: Vec<u8>,
    extra_len: u16,
}

/// Every row of an archive this writer made (no comment, no ZIP64): its central row's fields, its local header's, where its data starts,
/// its version made by and its external attributes.
struct Written {
    central: Header,
    local: Header,
    data_at: usize,
    made_by: u16,
    external: u32,
}

fn rows(zip: &[u8]) -> Vec<Written> {
    let end = zip.len() - 22;
    assert_eq!(u32_at(zip, end), 0x0605_4B50, "an end record with no comment is the last 22 bytes");
    let count = u16_at(zip, end + 10) as usize;
    assert_eq!(count, u16_at(zip, end + 8) as usize, "the entries on this disk are all the entries");
    let mut at = u32_at(zip, end + 16) as usize;
    assert_eq!(at + u32_at(zip, end + 12) as usize, end, "the directory ends where the end record starts");
    let mut out = Vec::new();
    for _ in 0..count {
        assert_eq!(u32_at(zip, at), 0x0201_4B50);
        let (name_len, extra_len, comment_len) = (u16_at(zip, at + 28) as usize, u16_at(zip, at + 30), u16_at(zip, at + 32));
        assert_eq!(comment_len, 0, "no comment");
        assert_eq!(u16_at(zip, at + 34), 0, "disk");
        let central = Header {
            version_needed: u16_at(zip, at + 6),
            flags: u16_at(zip, at + 8),
            method: u16_at(zip, at + 10),
            time: u16_at(zip, at + 12),
            date: u16_at(zip, at + 14),
            crc: u32_at(zip, at + 16),
            compressed: u32_at(zip, at + 20),
            size: u32_at(zip, at + 24),
            name: zip[at + 46..at + 46 + name_len].to_vec(),
            extra_len,
        };
        let (made_by, external) = (u16_at(zip, at + 4), u32_at(zip, at + 38));
        let local_at = u32_at(zip, at + 42) as usize;
        assert_eq!(u32_at(zip, local_at), 0x0403_4B50);
        let (l_name, l_extra) = (u16_at(zip, local_at + 26) as usize, u16_at(zip, local_at + 28));
        let local = Header {
            version_needed: u16_at(zip, local_at + 4),
            flags: u16_at(zip, local_at + 6),
            method: u16_at(zip, local_at + 8),
            time: u16_at(zip, local_at + 10),
            date: u16_at(zip, local_at + 12),
            crc: u32_at(zip, local_at + 14),
            compressed: u32_at(zip, local_at + 18),
            size: u32_at(zip, local_at + 22),
            name: zip[local_at + 30..local_at + 30 + l_name].to_vec(),
            extra_len: l_extra,
        };
        out.push(Written { central, local, data_at: local_at + 30 + l_name + l_extra as usize, made_by, external });
        at += 46 + name_len + extra_len as usize;
    }
    out
}

/// A small drawing package: `mimetype` stored first, then XML deflated, the folder LibreOffice writes, and a sub-document folder with a
/// space in its name.
fn drawing() -> Vec<u8> {
    let mut w = ArchiveWriter::new();
    w.add("mimetype", MEDIA_TYPE, Method::Stored).unwrap();
    w.add("content.xml", b"<office:document-content/>", Method::Deflated).unwrap();
    w.add_directory("Configurations2/").unwrap();
    w.add("Object 1/content.xml", b"<office:document-content>an object</office:document-content>", Method::Deflated).unwrap();
    w.add_smallest("META-INF/manifest.xml", b"<manifest:manifest/>").unwrap();
    w.finish().unwrap()
}

#[test]
fn first_entry_stored_without_extra_field() {
    // `PK` at bytes 0 and 1, `mimetype` at bytes 30 to 37, and the media type from byte 38, which is what a reader that detects
    // the type by fixed offsets reads. The rest of the local header is the APPNOTE's 4.3.7, field by field.
    let zip = drawing();
    assert_eq!(&zip[0..4], b"PK\x03\x04", "the local header's signature");
    assert_eq!(u16_at(&zip, 4), 10, "version needed: 1.0, a stored file");
    assert_eq!(u16_at(&zip, 6), 0, "no flag: an ASCII name, no data descriptor, no encryption");
    assert_eq!(u16_at(&zip, 8), 0, "method 0, stored");
    assert_eq!((u16_at(&zip, 10), u16_at(&zip, 12)), (0, 0x21), "midnight, 1980-01-01");
    assert_eq!(u32_at(&zip, 14), crc32fast::hash(MEDIA_TYPE), "the checksum is in the local header (no data descriptor)");
    assert_eq!(u32_at(&zip, 18) as usize, MEDIA_TYPE.len(), "compressed size: the bytes as they are");
    assert_eq!(u32_at(&zip, 22) as usize, MEDIA_TYPE.len(), "uncompressed size");
    assert_eq!(u16_at(&zip, 26), 8, "the name's length");
    assert_eq!(u16_at(&zip, 28), 0, "no extra field: nothing between the name and the bytes");
    assert_eq!(&zip[30..38], b"mimetype");
    assert_eq!(&zip[38..38 + MEDIA_TYPE.len()], MEDIA_TYPE);
    // The reader says the same: the first entry, at byte 0, stored, its data at byte 38.
    let archive = Archive::read(&zip, &Limits::default()).unwrap();
    let first = &archive.entries()[0];
    assert_eq!((first.name.as_str(), first.local_offset, first.data_offset, first.local_extra_len), ("mimetype", 0, 38, 0));
    assert!(first.is_plain_stored());
    assert_eq!(read_all(&zip).unwrap()[0].1, MEDIA_TYPE);
}

#[test]
fn directory_entry_round_trips() {
    // A folder `Configurations2/` of size 0 is an entry of its own, read as a folder and kept in its place, not dropped.
    let zip = drawing();
    let archive = Archive::read(&zip, &Limits::default()).unwrap();
    let names: Vec<&str> = archive.entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["mimetype", "content.xml", "Configurations2/", "Object 1/content.xml", "META-INF/manifest.xml"]);
    assert_eq!(archive.rows(), 5);
    let folder = &archive.entries()[2];
    assert!(folder.folder && folder.size == 0 && folder.compressed == 0 && folder.compression() == Some(Method::Stored), "{folder:?}");
    assert!(!folder.counted, "a folder is no part");
    // APPNOTE 4.4.3.2: version 2.0 for a folder; the MS-DOS directory attribute in its row's external attributes.
    let row = &rows(&zip)[2];
    assert_eq!((row.central.version_needed, row.local.version_needed, row.made_by), (20, 20, 20));
    assert_eq!(row.external, 0x10);
    assert_eq!(row.central.name, b"Configurations2/");
    // Its bytes are none, and reading it costs nothing.
    let all = read_all(&zip).unwrap();
    assert_eq!(all[2], ("Configurations2/".to_owned(), Vec::new()));
    // A folder written and read again, and written again from what was read, is the same archive.
    let mut again = ArchiveWriter::new();
    for (entry, (name, data)) in archive.entries().iter().zip(&all) {
        if entry.folder {
            again.add_directory(name).unwrap();
        } else {
            again.add(name, data, entry.compression().unwrap()).unwrap();
        }
    }
    assert_eq!(again.finish().unwrap(), zip);
}

#[test]
fn name_with_space_round_trips() {
    // Names with spaces are legal and common in sub-document folders, written as they are (no `%20`) and compared exactly.
    // `Object 1/content.xml` and `object 1/content.xml` are two entries.
    let mut w = ArchiveWriter::new();
    w.add("Object 1/content.xml", b"one", Method::Stored).unwrap();
    w.add("object 1/content.xml", b"two", Method::Stored).unwrap();
    w.add("ObjectReplacements/Object 1", b"three", Method::Deflated).unwrap();
    let zip = w.finish().unwrap();
    assert!(zip.windows(20).any(|w| w == b"Object 1/content.xml"), "the space is written as a space");
    assert!(!zip.windows(3).any(|w| w == b"%20"));
    let all = read_all(&zip).unwrap();
    let want = [("Object 1/content.xml", &b"one"[..]), ("object 1/content.xml", b"two"), ("ObjectReplacements/Object 1", b"three")];
    assert_eq!(all.len(), 3);
    for ((name, data), (want_name, want_data)) in all.iter().zip(want) {
        assert_eq!((name.as_str(), data.as_slice()), (want_name, want_data));
    }
    // An ASCII name sets no flag.
    assert!(rows(&zip).iter().all(|r| r.central.flags == 0 && r.local.flags == 0));
}

#[test]
fn mimetype_compressed_is_reported() {
    // R2 asks for `mimetype` first, stored, with no extra field; R4 has the ODF reader give a warning, or refuse, when it is not. The
    // container reads the archive either way and says what the first entry is, so that the ODF layer can.
    let mut w = ArchiveWriter::new();
    w.add("mimetype", MEDIA_TYPE, Method::Deflated).unwrap();
    w.add("content.xml", b"<a/>", Method::Deflated).unwrap();
    let zip = w.finish().unwrap();
    let archive = Archive::read(&zip, &Limits::default()).unwrap();
    let first = &archive.entries()[0];
    assert_eq!((first.name.as_str(), first.local_offset), ("mimetype", 0));
    assert_eq!(first.compression(), Some(Method::Deflated));
    assert!(!first.is_plain_stored(), "a deflated mimetype is reported");
    assert_ne!(&zip[38..38 + MEDIA_TYPE.len()], MEDIA_TYPE, "and the media type is not at byte 38");
    assert_eq!(read_all(&zip).unwrap()[0].1, MEDIA_TYPE, "though it reads");
    // Stored with an extra field in its local header (an extended timestamp, as some writers add): reported too, and the name is no longer
    // followed by the media type.
    let stamped = build(&[Item::stored("mimetype", MEDIA_TYPE).with_local_extra(&[0x55, 0x54, 5, 0, 1, 0, 0, 0, 0]), Item::stored("a.xml", b"<a/>")]);
    let archive = Archive::read(&stamped, &Limits::default()).unwrap();
    let first = &archive.entries()[0];
    assert_eq!((first.compression(), first.local_extra_len, first.data_offset), (Some(Method::Stored), 9, 47));
    assert!(!first.is_plain_stored());
    // Stored and plain but not first: its local header is not at byte 0.
    let second = build(&[Item::stored("a.xml", b"<a/>"), Item::stored("mimetype", MEDIA_TYPE)]);
    let archive = Archive::read(&second, &Limits::default()).unwrap();
    let mimetype = &archive.entries()[archive.position("mimetype").unwrap()];
    assert!(mimetype.is_plain_stored() && mimetype.local_offset > 0);
}

#[test]
fn local_and_central_headers_agree_and_the_method_is_the_one_asked_for() {
    // Bytes deflate cannot shrink: xorshift64.
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let noise: Vec<u8> = (0..4000)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect();
    let mut w = ArchiveWriter::new();
    w.add("stored.bin", &noise, Method::Stored).unwrap();
    // Deflated as asked, though deflate makes noise larger: a save that keeps an entry's method wants exactly that.
    w.add("deflated.bin", &noise, Method::Deflated).unwrap();
    assert_eq!(w.add_smallest("smallest.bin", &noise).unwrap(), Method::Stored, "deflate does not make noise smaller");
    assert_eq!(w.add_smallest("text.xml", &b"<a>text</a>".repeat(100)).unwrap(), Method::Deflated);
    assert_eq!(w.add_smallest("empty.xml", b"").unwrap(), Method::Stored, "an empty entry is stored");
    w.add("empty-deflated.xml", b"", Method::Deflated).unwrap();
    assert_eq!(w.len(), 6);
    let zip = w.finish().unwrap();
    let written = rows(&zip);
    assert_eq!(written.len(), 6);
    for row in &written {
        assert_eq!(row.local, row.central, "Local and central headers agree on {:?}", String::from_utf8_lossy(&row.central.name));
        assert_eq!(row.central.extra_len, 0, "no extra field in either");
        assert_eq!(row.central.version_needed, if row.central.method == 8 { 20 } else { 10 });
        assert_eq!(row.made_by, row.central.version_needed, "made by MS-DOS, at the version of the features used");
        assert_eq!(row.external, 0);
        assert!(row.data_at + row.central.compressed as usize <= zip.len());
    }
    let methods: Vec<u16> = written.iter().map(|r| r.central.method).collect();
    assert_eq!(methods, [0, 8, 0, 8, 0, 8]);
    assert!(written[1].central.compressed > written[1].central.size, "deflated, and larger");
    let all = read_all(&zip).unwrap();
    assert_eq!(all[0].1, noise);
    assert_eq!(all[1].1, noise);
    assert_eq!(all[3].1, b"<a>text</a>".repeat(100));
    assert!(all[4].1.is_empty() && all[5].1.is_empty());
}

#[test]
fn the_same_entries_give_the_same_bytes() {
    // No clock and no randomness: a save of the same drawing is the same file.
    assert_eq!(drawing(), drawing());
}

/// `data` deflated by `flate2` at `level`, as the writer is meant to.
fn deflated(data: &[u8], level: flate2::Compression) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), level);
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

#[test]
fn an_entry_up_to_4_mib_is_deflated_at_the_default_level_and_a_larger_one_at_the_fast_level() {
    // A person waits for a save, and the size of the file is not content. Text that the two levels compress to different bytes.
    let text: Vec<u8> = (0..(4 << 20) + 1).map(|i: usize| b"<cell N=\"PinX\" V=\"1.25\"/>\n"[i % 26] ^ ((i / 4099) as u8 & 3)).collect();
    let (at, over) = (&text[..4 << 20], &text[..]);
    assert_ne!(deflated(at, flate2::Compression::default()), deflated(at, flate2::Compression::fast()), "the levels differ on this text");
    assert_ne!(deflated(over, flate2::Compression::default()), deflated(over, flate2::Compression::fast()));
    let mut w = ArchiveWriter::new();
    w.add("at.xml", at, Method::Deflated).unwrap();
    w.add("over.xml", over, Method::Deflated).unwrap();
    let zip = w.finish().unwrap();
    let written = rows(&zip);
    let data_of = |row: &Written| zip[row.data_at..row.data_at + row.central.compressed as usize].to_vec();
    assert_eq!(data_of(&written[0]), deflated(at, flate2::Compression::default()));
    assert_eq!(data_of(&written[1]), deflated(over, flate2::Compression::fast()));
}

/// An entry that deflate makes smaller than a thousandth of itself is one this crate's own reader refuses (`zip_ratio_exceeded`: it inflates more than
/// 1,000 times its size), so a package of it that the writer made was one the writer's caller could not read back: a part of one repeated pattern that a save only copied (a properties part
/// the reader keeps as the bytes it is) made the save refused. `add_smallest` stores such an entry, and so does `pack`; the entry is read back, whole.
#[test]
fn an_entry_that_deflate_would_make_past_the_ratio_the_reader_holds_is_stored_by_add_smallest_and_packs_the_same() {
    use diacraft_container::zip::{Pack, pack};
    let past = b"<a/>".repeat(262_144);
    let packed = deflated(&past, flate2::Compression::default()).len() as u64;
    assert!(past.len() as u64 > 1_000 * packed, "the vector is past the ratio ({} bytes from {packed})", past.len());
    let mut w = ArchiveWriter::new();
    assert_eq!(w.add_smallest("docProps/app.xml", &past).unwrap(), Method::Stored, "stored, as deflate would pass the ratio");
    // The same just under the ratio is deflated: 64 KiB of the pattern (the sweep saved it) and a run of every size that deflate makes at most a thousand times smaller.
    let under = b"<a/>".repeat(16_384);
    let packed = deflated(&under, flate2::Compression::default()).len() as u64;
    assert!(under.len() as u64 <= 1_000 * packed, "the control is within the ratio ({} bytes from {packed})", under.len());
    assert_eq!(w.add_smallest("under.xml", &under).unwrap(), Method::Deflated);
    let files = read_all(&w.finish().unwrap()).expect("a package the writer made reads under the default limits");
    assert_eq!(files[0], ("docProps/app.xml".to_owned(), past.clone()));
    assert_eq!(files[1].1, under);
    // Compressed apart, an entry is written as it is where it is added.
    let apart = pack("docProps/app.xml", &past, Pack::Smallest).unwrap();
    assert_eq!(apart.method(), Method::Stored);
    let mut by_parts = ArchiveWriter::new();
    by_parts.add_packed("docProps/app.xml", &past, Pack::Smallest, Ok(apart)).unwrap();
    let mut at_once = ArchiveWriter::new();
    at_once.add_smallest("docProps/app.xml", &past).unwrap();
    assert!(by_parts.finish().unwrap() == at_once.finish().unwrap());
    // A method asked for is kept: `add` deflates what it is told to, whatever the ratio (a save that keeps an entry's method wants it).
    let mut kept = ArchiveWriter::new();
    kept.add("kept.xml", &past, Method::Deflated).unwrap();
    assert_eq!(rows(&kept.finish().unwrap())[0].central.method, 8);
}

#[test]
fn deflate_that_saves_nothing_is_stored_by_add_smallest() {
    // Five equal bytes deflate to five bytes: no smaller, so stored.
    let five = b"aaaaa";
    assert_eq!(deflated(five, flate2::Compression::default()).len(), five.len(), "a tie of deflate and the bytes");
    let mut w = ArchiveWriter::new();
    assert_eq!(w.add_smallest("five.txt", five).unwrap(), Method::Stored);
    // Shorter and longer runs: deflated exactly when deflate is shorter.
    for n in 1..=12 {
        let run = vec![b'a'; n];
        let want = if deflated(&run, flate2::Compression::default()).len() < n { Method::Deflated } else { Method::Stored };
        assert_eq!(w.add_smallest(&format!("run{n}.txt"), &run).unwrap(), want, "{n} bytes");
    }
    assert_eq!(w.add_smallest("many.txt", &[b'a'; 40]).unwrap(), Method::Deflated);
    let methods: Vec<u16> = rows(&w.finish().unwrap()).iter().map(|r| r.central.method).collect();
    assert_eq!((methods[0], methods[13]), (0, 8));
}

#[test]
fn long_names_are_written_whole_in_both_headers() {
    let long = format!("Object 1/{}/content.xml", "deep folder ".repeat(40));
    let mut w = ArchiveWriter::new();
    w.add(&long, b"<a/>", Method::Deflated).unwrap();
    w.add_directory(&format!("{}/", "d".repeat(300))).unwrap();
    let zip = w.finish().unwrap();
    let written = rows(&zip);
    assert_eq!(written[0].central.name, long.as_bytes());
    assert_eq!(written[0].local.name, long.as_bytes());
    let all = read_all(&zip).unwrap();
    assert_eq!((all[0].0.as_str(), all[0].1.as_slice()), (long.as_str(), &b"<a/>"[..]));
    assert_eq!(all[1].0.len(), 301);
}

#[test]
fn a_name_that_is_not_ascii_is_flagged_utf8_and_reads_back() {
    // APPNOTE 4.4.4, bit 11: the name is UTF-8. Without it a reader takes the bytes for code page 437.
    let mut w = ArchiveWriter::new();
    w.add("Pictures/café 1.png", b"\x89PNG", Method::Stored).unwrap();
    let zip = w.finish().unwrap();
    let row = &rows(&zip)[0];
    assert_eq!((row.central.flags, row.local.flags), (0x0800, 0x0800));
    assert_eq!(read_all(&zip).unwrap()[0].0, "Pictures/café 1.png");
}

#[test]
fn names_the_reader_would_refuse_are_refused() {
    let class_of = |name: &str| {
        let mut w = ArchiveWriter::new();
        w.add(name, b"x", Method::Stored).unwrap_err().class()
    };
    for traversal in ["../x.xml", "a/../../x.xml", "/x.xml", "a\\b.xml", "C:x.xml", ".."] {
        assert_eq!(class_of(traversal), "path_traversal", "{traversal:?}");
    }
    assert_eq!(class_of(""), "invalid_part_name");
    assert_eq!(class_of("a/"), "invalid_part_name", "a file's name that ends in a slash would read back as a folder");
    let mut w = ArchiveWriter::new();
    assert_eq!(w.add_directory("Configurations2").unwrap_err().class(), "invalid_part_name", "a folder's name ends in a slash");
    assert_eq!(w.add_directory("../up/").unwrap_err().class(), "path_traversal");
    assert_eq!(w.add_directory("/").unwrap_err().class(), "path_traversal");
    assert_eq!(w.add(&"n".repeat(65_536), b"", Method::Stored).unwrap_err().class(), "invalid_part_name", "past what a ZIP name can be");
    // A central row, its 46 bytes, name, extra field and comment, may take 65,535 bytes, so a name of 65,490 bytes cannot be written
    // where a reader takes it back; one of 65,489 can, and is read back with limits that allow its length.
    assert_eq!(w.add(&"n".repeat(65_490), b"", Method::Stored).unwrap_err().class(), "invalid_part_name", "a row past 65,535 bytes");
    assert!(w.is_empty(), "nothing refused was written");
    let mut longest = ArchiveWriter::new();
    longest.add(&"n".repeat(65_489), b"x", Method::Stored).unwrap();
    let zip = longest.finish().unwrap();
    let wide = Limits { max_part_name_bytes: 65_535, ..Limits::default() };
    assert_eq!(Archive::read(&zip, &wide).unwrap().entries()[0].name.len(), 65_489);
    w.add_directory("d/").unwrap();
    assert!(!w.is_empty());
    // Two entries of one name, exactly: refused when the archive is finished, since the reader would refuse it.
    let mut w = ArchiveWriter::new();
    w.add("a.xml", b"1", Method::Stored).unwrap();
    w.add("A.xml", b"2", Method::Stored).unwrap();
    w.add("a.xml", b"3", Method::Stored).unwrap();
    assert_eq!(w.finish().unwrap_err().class(), "duplicate_part");
    let mut w = ArchiveWriter::new();
    w.add("a.xml", b"1", Method::Stored).unwrap();
    w.add("A.xml", b"2", Method::Stored).unwrap();
    assert_eq!(read_all(&w.finish().unwrap()).unwrap().len(), 2, "names that differ in case are two entries");
}

#[test]
fn an_archive_holds_at_most_what_a_plain_end_record_counts() {
    // No ZIP64: 65,535 entries, and the next one is refused before anything of it is written.
    let mut w = ArchiveWriter::new();
    for i in 0..65_535u32 {
        w.add(&format!("e{i}"), b"", Method::Stored).unwrap();
    }
    let error = w.add("one-more", b"", Method::Stored).unwrap_err();
    assert_eq!(error.class(), "too_many_parts", "{error}");
    let zip = w.finish().unwrap();
    assert_eq!(u16_at(&zip, zip.len() - 22 + 10), 65_535);
    let limits = Limits { max_parts: 65_535, ..Limits::default() };
    let archive = Archive::read(&zip, &limits).unwrap();
    assert_eq!(archive.entries().len(), 65_535);
}

#[test]
fn what_is_written_is_what_the_reader_counts() {
    let zip = drawing();
    let mut work = Work::default();
    let archive = Archive::read_with(&zip, &Limits::default(), &mut diacraft_container::zip::Plain::new(&Limits::default()), &mut work).unwrap();
    assert_eq!(work.rows, 5);
    assert_eq!(work.extra_records, 0, "no extra field anywhere");
    // One local header for each row to lay them out, and one more for each entry kept.
    assert_eq!(work.local_headers, 10);
    assert_eq!(archive.entries().len(), 5);
}

// ---------------------------------------------------------------------------------------------------------------------------- `add_at_level`

/// What `add_at_level` writes, for a level: one entry of text that the levels compress to bytes of their own.
fn at_level(data: &[u8], level: u32) -> Vec<u8> {
    let mut w = ArchiveWriter::new();
    w.add_at_level("part.xml", data, level).unwrap();
    w.finish().unwrap()
}

/// `add_at_level` and `pack` of a level deflate whatever the size and whatever the gain (the same model gives the same bytes), but **an entry that the level would
/// make a thousand times smaller and more is stored**: this crate's reader refuses such an entry, and a `.diacraft` file with a part of one repeated pattern (a properties part a VSDX
/// file held as bytes) was refused by its own writer's check. The same data gives the same bytes each time, and the entry reads back.
#[test]
fn add_at_level_stores_an_entry_that_the_level_would_make_past_the_ratio_the_reader_holds() {
    use diacraft_container::zip::{Pack, pack};
    let past = b"<a/>".repeat(262_144);
    for level in [1u32, 6, 9] {
        let packed = deflated(&past, flate2::Compression::new(level)).len() as u64;
        if past.len() as u64 <= 1_000 * packed {
            continue;
        }
        let first = at_level(&past, level);
        assert_eq!(first, at_level(&past, level), "level {level}: the same entry gives the same bytes");
        assert_eq!(rows(&first)[0].central.method, 0, "level {level}: stored, as the level would pass the ratio");
        assert_eq!(read_all(&first).expect("reads back"), [("part.xml".to_owned(), past.clone())], "level {level}");
        let apart = pack("part.xml", &past, Pack::Level(level)).unwrap();
        assert_eq!(apart.method(), Method::Stored, "level {level}: packed apart the same");
    }
    // At level 6, the one the files of DiaCraft are written at, the vector is past the ratio (or this test looked at nothing).
    assert!(past.len() as u64 > 1_000 * deflated(&past, flate2::Compression::new(6)).len() as u64);
    // What is within the ratio stays deflated at the level, even where it gains little: 64 KiB of the same pattern.
    let under = b"<a/>".repeat(16_384);
    assert_eq!(rows(&at_level(&under, 6))[0].central.method, 8);
    assert_eq!(pack("part.xml", &under, Pack::Level(6)).unwrap().method(), Method::Deflated);
}

#[test]
fn add_at_level_is_deterministic_and_reads_back() {
    // A package is written at one fixed level, whatever the size of an entry, so the same model gives the same bytes. An entry over
    // 4 MiB is **not** deflated at the fast level here, as `add` deflates it.
    let text: Vec<u8> = (0..(4 << 20) + 1).map(|i: usize| b"<cell N=\"PinX\" V=\"1.25\"/>\n"[i % 26] ^ ((i / 4099) as u8 & 3)).collect();
    for data in [&text[..0], &text[..1], &text[..100_000], &text[..]] {
        let first = at_level(data, 6);
        assert_eq!(first, at_level(data, 6), "the same entry gives the same bytes ({} bytes)", data.len());
        let back = read_all(&first).unwrap();
        assert_eq!(back, [("part.xml".to_owned(), data.to_vec())], "and it reads back ({} bytes)", data.len());
        let row = &rows(&first)[0];
        assert_eq!(row.central.method, 8, "deflated, even where that is no smaller");
        assert_eq!(row.central, row.local, "the two headers agree");
        assert_eq!(row.central.size as usize, data.len());
        assert_eq!(row.central.flags & 0x0008, 0, "no data descriptor");
        assert_eq!(row.central.extra_len, 0, "no extra field");
    }
    // The bytes are those of `flate2` at the level asked for, at any size.
    let zip = at_level(&text, 6);
    let row = &rows(&zip)[0];
    assert_eq!(zip[row.data_at..row.data_at + row.central.compressed as usize], deflated(&text, flate2::Compression::new(6))[..]);
    assert_ne!(deflated(&text, flate2::Compression::new(6)), deflated(&text, flate2::Compression::fast()), "the levels differ on this text");
    // A level of its own changes the bytes, and still reads back.
    for level in [0u32, 1, 9] {
        let zip = at_level(&text[..100_000], level);
        let row = &rows(&zip)[0];
        assert_eq!(zip[row.data_at..row.data_at + row.central.compressed as usize], deflated(&text[..100_000], flate2::Compression::new(level))[..]);
        assert_eq!(read_all(&zip).unwrap(), [("part.xml".to_owned(), text[..100_000].to_vec())], "level {level}");
    }
    assert_ne!(at_level(&text[..100_000], 1), at_level(&text[..100_000], 9));
}

#[test]
fn levels_0_to_9_are_accepted_and_10_is_refused() {
    for level in 0..=9u32 {
        let mut w = ArchiveWriter::new();
        assert!(w.add_at_level("a.xml", b"<a/>", level).is_ok(), "level {level}");
        assert_eq!(w.len(), 1);
    }
    for level in [10u32, 11, 255, u32::MAX] {
        let mut w = ArchiveWriter::new();
        let refused = w.add_at_level("a.xml", b"<a/>", level).unwrap_err();
        assert_eq!(refused.class(), "unsupported_compression", "level {level}");
        assert!(refused.to_string().contains("deflate level"), "in words: {refused}");
        assert_eq!(w.len(), 0, "nothing is written by a refusal");
    }
    // The name rules of every entry hold for this one too.
    let mut w = ArchiveWriter::new();
    assert_eq!(w.add_at_level("../a.xml", b"<a/>", 6).unwrap_err().class(), "path_traversal");
    assert_eq!(w.add_at_level("", b"<a/>", 6).unwrap_err().class(), "invalid_part_name");
    assert_eq!(w.add_at_level("dir/", b"", 6).unwrap_err().class(), "invalid_part_name");
}

#[test]
fn written_bytes_counts_the_local_headers_and_the_data_and_not_the_directory() {
    let mut w = ArchiveWriter::new();
    assert_eq!(w.written_bytes(), 0);
    w.add("a.txt", b"hello", Method::Stored).unwrap();
    // A local header is 30 bytes and the name, then the data.
    assert_eq!(w.written_bytes(), 30 + 5 + 5);
    let before = w.written_bytes();
    w.add_at_level("b.xml", &[b'x'; 1000], 6).unwrap();
    let deflated = w.written_bytes() - before - 30 - 5;
    assert!(deflated > 0 && deflated < 100, "a thousand equal bytes deflate to a few: {deflated}");
    let at = w.written_bytes();
    let zip = w.finish().unwrap();
    assert!(zip.len() > at, "the directory and the end record come after: {} of {}", at, zip.len());
    assert_eq!(&zip[..at].len(), &at);
}

#[test]
fn an_archive_of_many_entries_reads_back_whatever_it_grew_by() {
    // The output of a `tight` writer grows by an eighth and not by what a `Vec` does by itself: a few hundred entries of different sizes read back the same, and are the
    // same bytes as those of a writer that grows as it always did.
    let mut loose = ArchiveWriter::new();
    let mut tight = ArchiveWriter::new().tight();
    let mut want = Vec::new();
    for i in 0..300usize {
        let data: Vec<u8> = (0..(i * 37) % 500).map(|j| (i + j) as u8).collect();
        loose.add(&format!("e/{i}.bin"), &data, Method::Stored).unwrap();
        tight.add(&format!("e/{i}.bin"), &data, Method::Stored).unwrap();
        want.push((format!("e/{i}.bin"), data));
    }
    let (loose, tight) = (loose.finish().unwrap(), tight.finish().unwrap());
    assert_eq!(read_all(&tight).unwrap(), want);
    assert_eq!(loose, tight, "the growth is not in the bytes");
}
