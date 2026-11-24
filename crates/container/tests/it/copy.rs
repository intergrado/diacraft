//! Copying an entry as it is: an entry a save did not change keeps its compressed
//! bytes, flags, extra fields, dates and attributes, so that a package copied entry by entry is the same file, central directory and
//! `mimetype` included; and an entry whose headers cannot be kept as they are is still copied without being inflated, under headers that
//! agree. Every archive is built here, by this crate's writer or by hand (`zipbuild.rs`).

use diacraft_container::zip::{Archive, ArchiveWriter, Copied, CopyAccount, Dropped, Entry, Judge, Method, Verdict, copy_account};
use diacraft_container::{ContainerError, Limits, Work};

use crate::read_all;
use crate::stream;
use crate::zipbuild::{Item, Opts, build, build_with, extra_record};

const MEDIA_TYPE: &[u8] = b"application/vnd.oasis.opendocument.graphics";

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Every entry of `bytes` copied in order into a new archive, and how each was copied.
fn copy_all(bytes: &[u8]) -> (Vec<u8>, Vec<Copied>) {
    let archive = Archive::read(bytes, &Limits::default()).unwrap();
    let mut writer = ArchiveWriter::new();
    let how: Vec<Copied> = (0..archive.entries().len()).map(|i| writer.copy(&archive, i).unwrap()).collect();
    (writer.finish().unwrap(), how)
}

/// A drawing package as this crate's writer makes one: `mimetype` first and stored, XML deflated, a folder, a name with a space and a
/// name that is not ASCII.
fn written_package() -> Vec<u8> {
    let mut w = ArchiveWriter::new();
    w.add("mimetype", MEDIA_TYPE, Method::Stored).unwrap();
    w.add("content.xml", &b"<office:document-content>a page</office:document-content>".repeat(20), Method::Deflated).unwrap();
    w.add_directory("Configurations2/").unwrap();
    w.add("Object 1/content.xml", b"<office:document-content/>", Method::Deflated).unwrap();
    w.add("Pictures/caf\u{e9}.png", b"\x89PNG not really", Method::Stored).unwrap();
    w.add_smallest("META-INF/manifest.xml", &b"<manifest:manifest/>".repeat(4)).unwrap();
    w.finish().unwrap()
}

#[test]
fn a_package_copied_entry_by_entry_comes_back_byte_identical() {
    let original = written_package();
    let (copy, how) = copy_all(&original);
    assert_eq!(how, vec![Copied::Exact; 6]);
    assert_eq!(copy, original, "every byte: the entries, the central directory and the end record");
    // V1 still holds: `mimetype` first, stored, its name at byte 30 and the media type at byte 38.
    assert_eq!(&copy[30..38], b"mimetype");
    assert_eq!(&copy[38..38 + MEDIA_TYPE.len()], MEDIA_TYPE);
    // A copy of a copy is the same again.
    assert_eq!(copy_all(&copy).0, original);
}

#[test]
fn a_copy_keeps_extra_fields_dates_attributes_and_row_comments() {
    // What another writer puts in its rows: a "made by" of Unix, a time and a date, file attributes, an extended timestamp in the local
    // header and another in the row, a row comment, a stored `mimetype` first with none of these.
    let stamp = |n: u8| extra_record(0x5455, &[1, n, 0, 0, 0]);
    let items = vec![
        Item::stored("mimetype", MEDIA_TYPE),
        Item {
            made_by: 0x031E,
            time: 0x6A21,
            date: 0x5B67,
            internal: 1,
            external: 0o100644 << 16,
            comment: b"a row's own comment".to_vec(),
            ..Item::deflated("content.xml", &b"<a>text</a>".repeat(50)).with_local_extra(&stamp(1)).with_extra(&stamp(2))
        },
        Item { made_by: 0x0314, time: 0x0001, date: 0x0042, external: 0x10, ..Item::stored("Configurations2/", b"") },
        Item::stored("Basic/x.xml", b"<b/>").with_flags(0x0800),
    ];
    let original = build(&items);
    let archive = Archive::read(&original, &Limits::default()).unwrap();
    // The reader says what the rows say.
    let content = &archive.entries()[1];
    assert_eq!((content.version_made_by, content.version_needed, content.time, content.date), (0x031E, 20, 0x6A21, 0x5B67));
    assert_eq!((content.internal_attributes, content.external_attributes), (1, 0o100644 << 16));
    let row = &original[content.row_offset..content.row_offset + content.row_len];
    assert_eq!(&row[..4], b"PK\x01\x02");
    assert!(row.ends_with(b"a row's own comment"), "the row's length covers its extra field and its comment");
    let (copy, how) = copy_all(&original);
    assert_eq!(how, vec![Copied::Exact; 4]);
    assert_eq!(copy, original);
}

/// A data descriptor stating `crc`, `compressed` and `size` (APPNOTE 4.3.9): with its signature or without, with sizes of 8 bytes
/// (`wide`) or of 4.
fn descriptor(signed: bool, wide: bool, crc: u32, compressed: u64, size: u64) -> Vec<u8> {
    let mut out = Vec::new();
    if signed {
        out.extend_from_slice(&0x0807_4B50u32.to_le_bytes());
    }
    out.extend_from_slice(&crc.to_le_bytes());
    if wide {
        out.extend_from_slice(&compressed.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
    } else {
        out.extend_from_slice(&(compressed as u32).to_le_bytes());
        out.extend_from_slice(&(size as u32).to_le_bytes());
    }
    out
}

/// A local header's ZIP64 record for an entry with flag bit 3: both sizes zero, the descriptor holding them (APPNOTE 4.5.3).
fn zip64_zeros() -> Vec<u8> {
    extra_record(1, &[0u8; 16])
}

/// What a copy of every entry of `bytes`, in the order of the file, keeps and leaves out.
fn account(bytes: &[u8]) -> CopyAccount {
    copy_account(&Archive::read(bytes, &Limits::default()).unwrap()).unwrap()
}

/// Every entry of `bytes` copied in the order the entries lie in the file, as an unedited save copies them.
fn copy_in_file_order(bytes: &[u8]) -> Vec<u8> {
    let archive = Archive::read(bytes, &Limits::default()).unwrap();
    let mut order: Vec<(usize, usize)> = archive.entries().iter().enumerate().map(|(i, e)| (e.local_offset, i)).collect();
    order.sort_unstable();
    let mut writer = ArchiveWriter::new();
    for (_, i) in order {
        writer.copy(&archive, i).unwrap();
    }
    writer.finish().unwrap()
}

#[test]
fn a_data_descriptor_is_kept_when_it_states_what_the_row_states_in_the_form_its_local_header_announces() {
    // APPNOTE 4.3.9: with flag bit 3 the local header's checksum and sizes are zero and a descriptor follows the data, with or without
    // its signature; its sizes are 8 bytes each when the local header holds a ZIP64 record (which leaves the sizes to it), else 4 (4.3.9.2).
    let content = b"<office:document-content>described</office:document-content>".repeat(8);
    let base = Item::deflated("content.xml", &content).with_flags(0x0008);
    let (crc, compressed, size) = (base.crc, base.data.len() as u64, base.size);
    let short = Item { local_sizes: Some((0, 0, 0)), ..base.clone() };
    let wide = Item { local_sizes: Some((0, 0xFFFF_FFFF, 0xFFFF_FFFF)), local_extra: zip64_zeros(), ..base.clone() };
    for (header, form) in [(&short, false), (&wide, true)] {
        for signed in [true, false] {
            let described = descriptor(signed, form, crc, compressed, size);
            let item = Item { after_data: described.clone(), ..header.clone() };
            let original = build(&[Item::stored("mimetype", MEDIA_TYPE), item, Item::stored("styles.xml", b"<s/>")]);
            let (copy, how) = copy_all(&original);
            assert_eq!(how, [Copied::Exact; 3], "{} bytes of descriptor", described.len());
            assert_eq!(copy, original, "the descriptor is copied with its entry");
            assert_eq!(stream::walk(&copy).map(|names| names.len()), Ok(3), "{} bytes of descriptor", described.len());
            assert!(account(&original).is_exact());
        }
    }
    // A descriptor of the other form is not the one its local header announces: a streaming reader of the file reads it in the form
    // announced and breaks. The entry is copied under headers written from the row, with no descriptor, the descriptor's bytes are left out
    // and counted as bytes no header announces, and a streaming reader reads the copy whole.
    for (header, form) in [(&short, true), (&wide, false)] {
        for signed in [true, false] {
            let described = descriptor(signed, form, crc, compressed, size);
            let item = Item { after_data: described.clone(), ..header.clone() };
            let original = build(&[Item::stored("mimetype", MEDIA_TYPE), item, Item::stored("styles.xml", b"<s/>")]);
            let what =
                format!("{} bytes of descriptor after a local header {} a ZIP64 record", described.len(), if form { "without" } else { "with" });
            assert_eq!(stream::walk(&original).map_err(|broke| broke.entry), Err(2), "{what}");
            let (copy, how) = copy_all(&original);
            assert_eq!(how, [Copied::Exact, Copied::Rewritten, Copied::Exact], "{what}");
            let account = account(&original);
            // A descriptor of another form than its local header announces has a reason of its own.
            assert_eq!(
                (account.rewritten, account.dropped.descriptor_form, account.dropped.unannounced, account.dropped.total()),
                (1, described.len() as u64, 0, described.len() as u64)
            );
            assert_eq!(account.dropped.reasons()[1], ("descriptor_form", described.len() as u64));
            assert_eq!(stream::walk(&copy).map(|names| names.len()), Ok(3), "{what}");
            assert_eq!(read_all(&copy).unwrap(), read_all(&original).unwrap(), "{what}");
        }
    }
    // A descriptor that states another checksum is not what the row says: the entry is copied under headers written from the row, with no
    // descriptor and the checksum and sizes in the local header, and it reads back with the same bytes.
    let (crc, compressed, size) = (crc, compressed as u32, size as u32);
    let mut wrong = descriptor(false, false, crc, u64::from(compressed), u64::from(size));
    wrong[0] ^= 1;
    let item = Item { local_sizes: Some((0, 0, 0)), after_data: wrong, ..base.clone() };
    let original = build(&[Item::stored("mimetype", MEDIA_TYPE), item]);
    let (copy, how) = copy_all(&original);
    assert_eq!(how, [Copied::Exact, Copied::Rewritten]);
    assert_eq!(u32_at(&copy, 0), 0x0403_4B50);
    let second = 30 + 8 + MEDIA_TYPE.len();
    assert_eq!(u16_at(&copy, second + 6) & 0x0008, 0, "no descriptor flag");
    assert_eq!((u32_at(&copy, second + 14), u32_at(&copy, second + 18), u32_at(&copy, second + 22)), (crc, compressed, size));
    assert_eq!(read_all(&copy).unwrap()[1].1, content);
}

#[test]
fn headers_that_disagree_or_need_zip64_are_written_anew_from_the_row() {
    // The central header wins: a local name, method or set of flags that is not the row's, a local checksum that is
    // not the row's, and a row that leaves a size to a ZIP64 field, are copied without being inflated under headers written from the row.
    let content = b"<office:document-content>kept</office:document-content>".repeat(8);
    let plain = Item::deflated("content.xml", &content);
    let cases: Vec<(&str, Item)> = vec![
        ("a local name that leaves the package", Item { local_name: Some(b"../evil.xml".to_vec()), ..plain.clone() }),
        ("a local method", Item { local_method: Some(0), ..plain.clone() }),
        ("local flags", Item { local_flags: Some(0x0002), ..plain.clone() }),
        ("a local checksum", Item { local_sizes: Some((plain.crc ^ 1, plain.data.len() as u32, plain.size as u32)), ..plain.clone() }),
        ("a descriptor flag with no descriptor", Item { local_sizes: Some((0, 0, 0)), ..plain.clone().with_flags(0x0008) }),
        (
            "a size left to a ZIP64 field",
            Item {
                size: 0xFFFF_FFFF,
                extra: extra_record(1, &plain.size.to_le_bytes()),
                local_sizes: Some((plain.crc, plain.data.len() as u32, plain.size as u32)),
                ..plain.clone()
            },
        ),
    ];
    for (what, item) in cases {
        let original = build(&[Item::stored("mimetype", MEDIA_TYPE), item, Item::stored("styles.xml", b"<s/>")]);
        let archive = Archive::read(&original, &Limits::default()).unwrap_or_else(|e| panic!("{what}: {e}"));
        let mut writer = ArchiveWriter::new();
        let how: Vec<Copied> = (0..3).map(|i| writer.copy(&archive, i).unwrap()).collect();
        assert_eq!(how, [Copied::Exact, Copied::Rewritten, Copied::Exact], "{what}");
        let copy = writer.finish().unwrap();
        // Local and central headers now agree, the data is the same compressed bytes, and nothing was inflated to copy it.
        let second = 30 + 8 + MEDIA_TYPE.len();
        assert_eq!(&copy[second + 30..second + 30 + 11], b"content.xml", "{what}: the row's name");
        assert_eq!(u16_at(&copy, second + 28), 0, "{what}: no extra field");
        assert_eq!(u16_at(&copy, second + 8), 8, "{what}: the row's method");
        let data_at = second + 41;
        assert_eq!(&copy[data_at..data_at + plain.data.len()], &plain.data[..], "{what}: the compressed bytes as they were");
        let all = read_all(&copy).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert_eq!(all[1], ("content.xml".to_owned(), content.clone()), "{what}");
        assert_eq!(copy_all(&copy).1, [Copied::Exact; 3], "{what}: and the copy copies exactly");
    }
}

#[test]
fn a_row_that_leaves_both_sizes_to_zip64_is_rewritten_with_plain_sizes() {
    let content = b"<a>zip64</a>".repeat(30);
    let plain = Item::deflated("content.xml", &content);
    let (size, compressed) = (plain.size, plain.data.len() as u64);
    let item = Item {
        extra: extra_record(1, &[&size.to_le_bytes()[..], &compressed.to_le_bytes()].concat()),
        local_sizes: Some((plain.crc, compressed as u32, size as u32)),
        ..plain.clone()
    };
    let mut original = build(&[item]);
    // Patch the row: both 32-bit sizes say "look elsewhere".
    let row = original.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    original[row + 20..row + 28].copy_from_slice(&[0xFF; 8]);
    let archive = Archive::read(&original, &Limits::default()).unwrap();
    assert_eq!((archive.entries()[0].size, archive.entries()[0].compressed), (size, compressed));
    let mut writer = ArchiveWriter::new();
    assert_eq!(writer.copy(&archive, 0).unwrap(), Copied::Rewritten);
    let copy = writer.finish().unwrap();
    let row = copy.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    assert_eq!((u32_at(&copy, row + 20) as u64, u32_at(&copy, row + 24) as u64), (compressed, size), "plain sizes in the row");
    assert_eq!(u16_at(&copy, row + 30), 0, "no ZIP64 field");
    assert_eq!(read_all(&copy).unwrap()[0].1, content);
}

#[test]
fn sizes_left_to_zip64_fields_alike_in_both_headers_are_kept_and_an_offset_left_to_one_is_not() {
    // Both headers leave the two sizes to a ZIP64 field, as a writer that writes every entry so does: nothing disagrees, and the offset is
    // in the row's own field, so the entry is copied byte for byte, its fields with it.
    let content = b"<a>zip64</a>".repeat(30);
    let plain = Item::deflated("content.xml", &content);
    let (size, compressed) = (plain.size, plain.data.len() as u64);
    let sizes = extra_record(1, &[&size.to_le_bytes()[..], &compressed.to_le_bytes()].concat());
    let item = Item { extra: sizes.clone(), local_extra: sizes, local_sizes: Some((plain.crc, 0xFFFF_FFFF, 0xFFFF_FFFF)), ..plain.clone() };
    let mut original = build(&[Item::stored("mimetype", MEDIA_TYPE), item]);
    let row = original.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
    original[row + 20..row + 28].copy_from_slice(&[0xFF; 8]);
    let (copy, how) = copy_all(&original);
    assert_eq!(how, [Copied::Exact, Copied::Exact]);
    assert_eq!(copy, original);
    assert_eq!(read_all(&copy).unwrap()[1].1, content);
    // A row whose local header's offset is left to a ZIP64 field: that is the field a copy must write anew, so the headers are written
    // from the row, with the offset in the plain field.
    let item = Item { extra: extra_record(1, &0u64.to_le_bytes()), ..plain.clone() };
    let mut original = build(&[item]);
    let row = original.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    original[row + 42..row + 46].copy_from_slice(&[0xFF; 4]);
    let archive = Archive::read(&original, &Limits::default()).unwrap();
    assert_eq!(archive.entries()[0].local_offset, 0);
    let mut writer = ArchiveWriter::new();
    assert_eq!(writer.copy(&archive, 0).unwrap(), Copied::Rewritten);
    let copy = writer.finish().unwrap();
    let row = copy.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    assert_eq!((u32_at(&copy, row + 42), u16_at(&copy, row + 30)), (0, 0), "the offset in the row's field, and no ZIP64 field");
    assert_eq!(read_all(&copy).unwrap()[0].1, content);
}

#[test]
fn a_rewritten_copy_keeps_the_extra_fields_and_the_comment_but_their_zip64_records() {
    // A local header that disagrees with its row (another name), so the headers are written anew: the row's extended timestamp and
    // comment are kept, its ZIP64 record (an offset of 0) is not, and the local header keeps its own extra field, its ZIP64 record left out.
    let content = b"<a>kept</a>".repeat(20);
    let plain = Item::deflated("content.xml", &content);
    let (stamp, local_stamp) = (extra_record(0x5455, &[1, 9, 9, 9, 9]), extra_record(0x5455, &[3, 1, 1, 1, 1, 2, 2, 2, 2]));
    let zip64 = extra_record(1, &0u64.to_le_bytes());
    let item = Item {
        extra: [&stamp[..], &zip64[..]].concat(),
        local_extra: [&zip64[..], &local_stamp[..]].concat(),
        comment: b"the row's comment".to_vec(),
        local_name: Some(b"Content.xml".to_vec()),
        ..plain.clone()
    };
    let mut original = build(&[item]);
    let row = original.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    original[row + 42..row + 46].copy_from_slice(&[0xFF; 4]); // the offset left to the ZIP64 record
    let archive = Archive::read(&original, &Limits::default()).unwrap();
    let mut writer = ArchiveWriter::new();
    assert_eq!(writer.copy(&archive, 0).unwrap(), Copied::Rewritten);
    let copy = writer.finish().unwrap();
    // The local header: the row's name, and its own extra field without the ZIP64 record.
    let local_extra_len = u16_at(&copy, 28) as usize;
    assert_eq!(&copy[30..41], b"content.xml");
    assert_eq!(&copy[41..41 + local_extra_len], &local_stamp[..]);
    // The row: its extra field without the ZIP64 record, and its comment.
    let row = copy.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    let (name_len, extra_len, comment_len) = (u16_at(&copy, row + 28) as usize, u16_at(&copy, row + 30) as usize, u16_at(&copy, row + 32) as usize);
    let extra_at = row + 46 + name_len;
    assert_eq!(&copy[extra_at..extra_at + extra_len], &stamp[..]);
    assert_eq!(&copy[extra_at + extra_len..extra_at + extra_len + comment_len], b"the row's comment");
    assert_eq!(read_all(&copy).unwrap()[0].1, content);
    // A copy of the copy is exact: nothing in it disagrees now.
    assert_eq!(copy_all(&copy), (copy.clone(), vec![Copied::Exact]));
    // The entry gives its own extra fields and comment back, from the bytes of its file.
    let again = Archive::read(&copy, &Limits::default()).unwrap();
    let entry = &again.entries()[0];
    assert_eq!(entry.row_extra_and_comment(&copy), Some((&stamp[..], &b"the row's comment"[..])));
    assert_eq!(entry.local_extra(&copy), Some(&local_stamp[..]));
    assert_eq!(entry.row_extra_and_comment(b"not the file"), None);
}

#[test]
fn a_local_unicode_path_that_could_name_a_path_after_a_rewrite_is_dropped() {
    // The review's input: the local header names `other.xml`, the row `content.xml`, and the local extra field holds a Unicode path record
    // whose checksum is of `content.xml` and which names `../../evil.xml`. Under the local name it applied to nothing; a copy that wrote the
    // row's name into the local header and kept the record would make it apply. The rewritten local header keeps its other records and
    // drops that one; the row's own extra field, with the record the reader checked, is kept.
    let content = b"<a>kept</a>".repeat(10);
    let local_path = extra_record(0x7075, &[&[1u8][..], &crc32fast::hash(b"content.xml").to_le_bytes(), b"../../evil.xml"].concat());
    let stamp = extra_record(0x5455, &[1, 9, 9, 9, 9]);
    let row_path = extra_record(0x7075, &[&[1u8][..], &crc32fast::hash(b"content.xml").to_le_bytes(), b"content.xml"].concat());
    let item = Item {
        local_name: Some(b"other.xml".to_vec()),
        local_extra: [&local_path[..], &stamp[..]].concat(),
        extra: row_path.clone(),
        ..Item::deflated("content.xml", &content)
    };
    let original = build(&[item]);
    let archive = Archive::read(&original, &Limits::default()).unwrap();
    let mut writer = ArchiveWriter::new();
    assert_eq!(writer.copy(&archive, 0).unwrap(), Copied::Rewritten);
    let copy = writer.finish().unwrap();
    let again = Archive::read(&copy, &Limits::default()).unwrap();
    let entry = &again.entries()[0];
    assert_eq!(entry.name, "content.xml");
    assert_eq!(entry.local_extra(&copy), Some(&stamp[..]), "the local Unicode path is gone, the timestamp kept");
    assert_eq!(entry.row_extra_and_comment(&copy).map(|(extra, _)| extra), Some(&row_path[..]), "the row's checked field is kept");
    assert!(!copy.windows(8).any(|w| w == b"evil.xml"), "the path is nowhere in the copy");
    assert_eq!(read_all(&copy).unwrap()[0].1, content);
}

#[test]
fn a_copied_name_from_a_unicode_path_field_is_held_to_the_duplicate_rule_by_both_its_names() {
    // The name as written is `a.xml` and the field gives `b.xml`: a reader that reads the field sees `b.xml`, one that does not sees `a.xml`,
    // so an entry added under either name is the same name twice.
    let original = build(&[Item::stored("a.xml", b"<a/>").with_unicode_path("b.xml")]);
    let archive = Archive::read(&original, &Limits::default()).unwrap();
    assert_eq!(archive.entries()[0].name, "b.xml");
    for other in ["a.xml", "b.xml"] {
        let mut writer = ArchiveWriter::new();
        assert_eq!(writer.copy(&archive, 0).unwrap(), Copied::Exact);
        writer.add(other, b"<x/>", Method::Stored).unwrap();
        assert_eq!(writer.finish().unwrap_err().class(), "duplicate_part", "{other}");
    }
    // Alone, it is copied whole, field and all.
    let (copy, how) = copy_all(&original);
    assert_eq!((how, copy), (vec![Copied::Exact], original));
}

#[test]
fn a_copied_name_is_held_to_the_duplicate_rule_and_nothing_is_inflated() {
    let original = written_package();
    let archive = Archive::read(&original, &Limits::default()).unwrap();
    // The same entry twice, or a copy and an entry added under its name: the reader would refuse either, so the writer does.
    let mut twice = ArchiveWriter::new();
    twice.copy(&archive, 1).unwrap();
    twice.copy(&archive, 1).unwrap();
    assert_eq!(twice.finish().unwrap_err().class(), "duplicate_part");
    let mut added = ArchiveWriter::new();
    added.copy(&archive, 4).unwrap();
    added.add("Pictures/caf\u{e9}.png", b"another", Method::Stored).unwrap();
    assert_eq!(added.finish().unwrap_err().class(), "duplicate_part", "a copied name that is not ASCII is known by its decoded name");
    // An index past the entries is refused, and nothing is written.
    let mut past = ArchiveWriter::new();
    assert_eq!(past.copy(&archive, 6).unwrap_err().class(), "zip_corrupt");
    assert!(past.is_empty());
    // A copy never inflates: an entry whose bytes are not deflate at all (the reader would refuse it when inflated) is copied as it is.
    let garbage = Item { method: 8, data: b"these bytes are not a deflate stream".to_vec(), crc: 7, size: 100, ..Item::stored("x.bin", b"") };
    let original = build(&[Item::stored("mimetype", MEDIA_TYPE), garbage]);
    let mut work = Work::default();
    let mut archive =
        Archive::read_with(&original, &Limits::default(), &mut diacraft_container::zip::Plain::new(&Limits::default()), &mut work).unwrap();
    assert_eq!(work.inflated, 0, "reading the directory inflates nothing");
    let mut writer = ArchiveWriter::new();
    assert_eq!((writer.copy(&archive, 0).unwrap(), writer.copy(&archive, 1).unwrap()), (Copied::Exact, Copied::Exact));
    assert_eq!(writer.finish().unwrap(), original);
    assert_eq!(archive.inflate(1, &mut work).unwrap_err().class(), "zip_corrupt", "the bytes really are not deflate");
}

/// The harness's vector: a folder entry with flag bit 3, local sizes 0xFFFFFFFF, a ZIP64 record (0, 0) in its local
/// header, and `after` as its descriptor.
fn zip64_described_folder(after: Vec<u8>) -> Item {
    Item {
        local_sizes: Some((0, 0xFFFF_FFFF, 0xFFFF_FFFF)),
        local_extra: zip64_zeros(),
        after_data: after,
        ..Item::stored("Configurations2/", b"").with_flags(0x0008)
    }
}

#[test]
fn a_zero_length_entry_keeps_the_zip64_descriptor_its_local_header_announces() {
    // The first 16 bytes of a zero-length entry's 24-byte descriptor (signature, checksum 0, sizes 0 and 0 of 8 bytes)
    // also read as a short descriptor that states zeros; the local header's ZIP64 record says which it is.
    let content = b"<office:document-content>after the folder</office:document-content>".repeat(4);
    let file = |name: &str| Item { name: name.as_bytes().to_vec(), ..zip64_described_folder(descriptor(true, true, 0, 0, 0)) };
    let cases = [
        ("the harness's vector: a folder and a 24-byte descriptor", zip64_described_folder(descriptor(true, true, 0, 0, 0))),
        ("a folder and the 20 bytes of a descriptor without its signature", zip64_described_folder(descriptor(false, true, 0, 0, 0))),
        ("a zero-length file", file("empty.txt")),
    ];
    for (what, entry) in cases {
        let original =
            build(&[Item::stored("mimetype", MEDIA_TYPE), entry, Item::deflated("content.xml", &content), Item::stored("styles.xml", b"<s/>")]);
        let names = stream::walk(&original).unwrap_or_else(|broke| panic!("{what}: the file streams: {broke:?}"));
        assert_eq!(names.len(), 4, "{what}");
        let (copy, how) = copy_all(&original);
        assert_eq!(how, [Copied::Exact; 4], "{what}");
        assert_eq!(copy, original, "{what}: byte for byte, the whole descriptor kept");
        assert_eq!(stream::walk(&copy), Ok(names), "{what}: a streaming reader reads the copy whole");
        assert!(account(&original).is_exact(), "{what}");
        assert_eq!(read_all(&copy).unwrap()[2].1, content, "{what}");
    }
    // What the copy wrote before: the first 16 bytes of the descriptor, as a short one, under the local header's ZIP64 record. A streaming
    // reader reads 24 bytes of descriptor and breaks at the next entry, though a reader of the directory reads every part.
    let short = build(&[
        Item::stored("mimetype", MEDIA_TYPE),
        zip64_described_folder(descriptor(true, false, 0, 0, 0)),
        Item::deflated("content.xml", &content),
    ]);
    assert_eq!(stream::walk(&short).map_err(|broke| broke.entry), Err(2));
    assert_eq!(read_all(&short).unwrap().len(), 3);
    // A copy of that file keeps no descriptor its local header does not announce: the folder's headers are written anew from its row, with
    // no descriptor, the 16 bytes are left out and counted, and a streaming reader reads the copy whole.
    let (copy, how) = copy_all(&short);
    assert_eq!(how, [Copied::Exact, Copied::Rewritten, Copied::Exact]);
    let told = account(&short);
    assert_eq!((told.rewritten, told.dropped.descriptor_form, told.dropped.unannounced, told.dropped.total()), (1, 16, 0, 16), "{told:?}");
    assert_eq!(stream::walk(&copy).map(|names| names.len()), Ok(3));
    assert_eq!(read_all(&copy).unwrap(), read_all(&short).unwrap());
}

#[test]
fn a_short_descriptor_after_a_local_header_with_no_zip64_record_is_kept() {
    // Flag bit 3 and no ZIP64 record: the descriptor's sizes are 4 bytes each, with its signature or without; a zero-length folder and a
    // file of some bytes alike.
    let content = b"<office:document-content>short</office:document-content>".repeat(6);
    for signed in [true, false] {
        let folder = Item {
            local_sizes: Some((0, 0, 0)),
            after_data: descriptor(signed, false, 0, 0, 0),
            ..Item::stored("Configurations2/", b"").with_flags(0x0008)
        };
        let base = Item::deflated("content.xml", &content).with_flags(0x0008);
        let described = descriptor(signed, false, base.crc, base.data.len() as u64, base.size);
        let file = Item { local_sizes: Some((0, 0, 0)), after_data: described, ..base };
        let original = build(&[Item::stored("mimetype", MEDIA_TYPE), folder, file, Item::stored("styles.xml", b"<s/>")]);
        let names = stream::walk(&original).unwrap();
        let (copy, how) = copy_all(&original);
        assert_eq!((how, &copy), (vec![Copied::Exact; 4], &original), "signed: {signed}");
        assert_eq!(stream::walk(&copy), Ok(names));
        assert!(account(&original).is_exact());
    }
}

#[test]
fn bytes_no_header_announces_are_left_out_and_counted() {
    // The harness's other finding: a writer that puts a 16-byte data descriptor after the data of entries whose flags
    // announce none (bit 3 clear, the checksum and sizes in the local header). No header announces those bytes, and a streaming reader meets
    // a descriptor's signature where it looks for the next local header. A copy writes a well-formed archive without them, says how many
    // it left out and why, and every part reads back the same.
    let xml = |root: &str| format!("<office:{root}>a part of some length</office:{root}>").repeat(5).into_bytes();
    let parts = [
        Item::stored("mimetype", MEDIA_TYPE),
        Item::deflated("content.xml", &xml("document-content")),
        Item::deflated("styles.xml", &xml("document-styles")),
        Item::deflated("meta.xml", &xml("document-meta")),
        Item::deflated("META-INF/manifest.xml", &xml("manifest")),
    ];
    let items: Vec<Item> = parts
        .iter()
        .map(|it| match it.name.as_slice() {
            b"mimetype" => it.clone(),
            _ => Item { after_data: descriptor(true, false, it.crc, it.data.len() as u64, it.size), ..it.clone() },
        })
        .collect();
    let original = build(&items);
    assert_eq!(stream::walk(&original).map_err(|broke| broke.entry), Err(3), "the descriptor after the second entry");
    let told = account(&original);
    assert_eq!(told, CopyAccount { exact: 5, dropped: Dropped { unannounced: 64, ..Dropped::default() }, ..CopyAccount::default() });
    assert_eq!(told.dropped.reasons()[0], ("unannounced", 64));
    let (copy, how) = copy_all(&original);
    assert_eq!(how, [Copied::Exact; 5]);
    assert_eq!(copy.len() + 64, original.len());
    assert_eq!(copy, build(&parts), "the archive with nothing but its entries");
    assert_eq!(read_all(&copy).unwrap(), read_all(&original).unwrap());
    assert_eq!(stream::walk(&copy).map(|names| names.len()), Ok(5));
    assert!(account(&copy).is_exact());
}

#[test]
fn everything_a_copy_leaves_out_or_writes_anew_is_counted_by_why() {
    // Each way an archive can differ from the copy of its entries, alone: the account names it, the copy is the file exactly when the
    // account says nothing, and with no header written anew the copy is as much shorter as the account says. Every copy is the same
    // archive of the same three entries.
    let items =
        vec![Item::stored("mimetype", MEDIA_TYPE), Item::deflated("content.xml", &b"<a>text</a>".repeat(30)), Item::stored("styles.xml", b"<s/>")];
    let plain = build(&items);
    assert!(account(&plain).is_exact());
    let laid_out = |opts: Opts| build_with(&items, &opts);
    let dropped = |dropped: Dropped| CopyAccount { exact: 3, dropped, ..CopyAccount::default() };
    let renamed = {
        let mut items = items.clone();
        items[1].local_name = Some(b"Content.xml".to_vec());
        build(&items)
    };
    let cases: Vec<(&str, Vec<u8>, CopyAccount)> = vec![
        (
            "bytes before the first entry",
            laid_out(Opts { prefix: b"0123456789".to_vec(), ..Opts::default() }),
            dropped(Dropped { unannounced: 10, ..Dropped::default() }),
        ),
        (
            "bytes between the directory and the end record",
            laid_out(Opts { before_end: b"1234567".to_vec(), ..Opts::default() }),
            dropped(Dropped { unannounced: 7, ..Dropped::default() }),
        ),
        (
            "a directory tail",
            laid_out(Opts { directory_tail: b"PK\x05\x05\0\0".to_vec(), ..Opts::default() }),
            dropped(Dropped { directory_tail: 6, ..Dropped::default() }),
        ),
        ("ZIP64 end records", laid_out(Opts { zip64: true, ..Opts::default() }), dropped(Dropped { zip64_end_records: 76, ..Dropped::default() })),
        (
            "bytes, then ZIP64 end records",
            laid_out(Opts { zip64: true, before_end: b"12345".to_vec(), ..Opts::default() }),
            dropped(Dropped { unannounced: 5, zip64_end_records: 76, ..Dropped::default() }),
        ),
        (
            "an archive comment",
            laid_out(Opts { comment: b"an archive comment".to_vec(), ..Opts::default() }),
            dropped(Dropped { archive_comment: 18, ..Dropped::default() }),
        ),
        (
            "bytes after the end record",
            laid_out(Opts { after_end: b"trailing".to_vec(), ..Opts::default() }),
            dropped(Dropped { after_end_record: 8, ..Dropped::default() }),
        ),
        (
            "rows listed the other way round",
            laid_out(Opts { reversed: true, ..Opts::default() }),
            CopyAccount { exact: 3, rows_reordered: true, ..CopyAccount::default() },
        ),
        ("a local name that is not the row's", renamed, CopyAccount { exact: 2, rewritten: 1, ..CopyAccount::default() }),
    ];
    for (what, bytes, want) in cases {
        let told = account(&bytes);
        assert_eq!(told, want, "{what}");
        assert!(!told.is_exact(), "{what}");
        let copy = copy_in_file_order(&bytes);
        assert_ne!(copy, bytes, "{what}");
        if told.rewritten == 0 {
            assert_eq!(copy.len() as u64 + told.dropped.total(), bytes.len() as u64, "{what}");
        }
        assert_eq!(copy, plain, "{what}: the same archive of the same entries");
    }
}

#[test]
fn a_descriptor_is_never_read_out_of_the_next_entry() {
    // A zero-length folder with flag bit 3 and no descriptor at all, whose row states the checksum 0x04034B50: the next local header's
    // first 12 bytes, read as a descriptor without its signature, state that checksum and two zero sizes when its version, flags, method
    // and time are 0. They are the next entry's bytes, not a descriptor: the folder's headers are written anew, and nothing of the next
    // entry is copied twice.
    let folder = Item { crc: 0x0403_4B50, local_sizes: Some((0, 0, 0)), ..Item::stored("x/", b"").with_flags(0x0008) };
    let mut original = build(&[folder, Item::stored("a.xml", b"<a/>")]);
    let next = 30 + 2;
    assert_eq!(&original[next..next + 4], b"PK\x03\x04");
    original[next + 4..next + 6].copy_from_slice(&0u16.to_le_bytes()); // the version needed to extract: 0
    let told = account(&original);
    assert_eq!((told.exact, told.rewritten, told.dropped.total()), (1, 1, 0), "{told:?}");
    let (copy, how) = copy_all(&original);
    assert_eq!(how, [Copied::Rewritten, Copied::Exact]);
    assert_eq!(copy.len(), original.len(), "no byte written twice");
    let again = Archive::read(&copy, &Limits::default()).unwrap();
    assert_eq!(again.entries().iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["x/", "a.xml"]);
}

#[test]
fn sizes_left_to_a_zip64_record_are_kept_only_when_the_local_record_states_the_rows() {
    // Both headers leave the sizes to ZIP64 records and the row's states them. A streaming reader takes them from the local record, both
    // sizes (APPNOTE 4.5.3), so a local record that states others, too few bytes of one, or none at all, is a local header that disagrees
    // with its row: written anew, with the sizes in its own fields.
    let content = b"<a>zip64</a>".repeat(30);
    let plain = Item::deflated("content.xml", &content);
    let (size, compressed) = (plain.size, plain.data.len() as u64);
    let sizes = |size: u64, compressed: u64| extra_record(1, &[&size.to_le_bytes()[..], &compressed.to_le_bytes()].concat());
    for (what, local_extra, how) in [
        ("the row's sizes", sizes(size, compressed), Copied::Exact),
        ("another size", sizes(size + 1, compressed), Copied::Rewritten),
        ("the sizes the other way round", sizes(compressed, size), Copied::Rewritten),
        ("one size only", extra_record(1, &size.to_le_bytes()), Copied::Rewritten),
        ("no record", Vec::new(), Copied::Rewritten),
    ] {
        let item = Item { extra: sizes(size, compressed), local_extra, local_sizes: Some((plain.crc, 0xFFFF_FFFF, 0xFFFF_FFFF)), ..plain.clone() };
        let mut original = build(&[Item::stored("mimetype", MEDIA_TYPE), item]);
        let row = original.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
        original[row + 20..row + 28].copy_from_slice(&[0xFF; 8]);
        let (copy, copied) = copy_all(&original);
        assert_eq!(copied, [Copied::Exact, how], "{what}");
        assert_eq!(stream::walk(&copy).map(|names| names.len()), Ok(2), "{what}");
        assert_eq!(read_all(&copy).unwrap()[1].1, content, "{what}");
    }
    // One size left to the record, the uncompressed one: the local record still holds both, and both are held to the row's.
    for (what, local_extra, how) in
        [("the row's sizes", sizes(size, compressed), Copied::Exact), ("another size", sizes(size + 1, compressed), Copied::Rewritten)]
    {
        let item = Item {
            extra: extra_record(1, &size.to_le_bytes()),
            local_extra,
            local_sizes: Some((plain.crc, compressed as u32, 0xFFFF_FFFF)),
            ..plain.clone()
        };
        let mut original = build(&[Item::stored("mimetype", MEDIA_TYPE), item]);
        let row = original.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
        original[row + 24..row + 28].copy_from_slice(&[0xFF; 4]);
        let (copy, copied) = copy_all(&original);
        assert_eq!(copied, [Copied::Exact, how], "one size: {what}");
        assert_eq!(read_all(&copy).unwrap()[1].1, content, "one size: {what}");
    }
}

/// A judge that keeps every row but the one named `skip.xml`.
struct Skipping;

impl Judge for Skipping {
    type Error = ContainerError;

    fn entry(&mut self, entry: &Entry, _kept: &[Entry]) -> Result<Verdict, ContainerError> {
        Ok(if entry.name == "skip.xml" { Verdict::Drop } else { Verdict::Keep { rename: None, cap: 1 << 20, counted: true } })
    }
}

#[test]
fn rows_a_judge_does_not_keep_are_counted_and_their_entries_are_among_the_bytes_left_out() {
    let original = build(&[Item::stored("mimetype", MEDIA_TYPE), Item::stored("skip.xml", b"<x/>"), Item::stored("styles.xml", b"<s/>")]);
    let archive = Archive::read_with(&original, &Limits::default(), &mut Skipping, &mut Work::default()).unwrap();
    let told = copy_account(&archive).unwrap();
    assert_eq!((told.exact, told.rewritten, told.rows_not_kept, told.rows_reordered), (2, 0, 1, false), "{told:?}");
    assert!(!told.is_exact());
    assert_eq!(told.dropped.unannounced, 30 + 8 + 4, "the dropped row's header, name and data");
}

#[test]
fn a_folder_whose_row_states_a_checksum_is_copied_under_the_rows_headers_and_still_streams() {
    // What the fuzz found first: one byte of a folder's row changed, so the row states a checksum its local header (0) does not. The
    // central header wins: the headers are written anew from the row, checksum and all, and a streaming reader still
    // reads every entry where it lies (it walks the layout; a folder has no bytes to check).
    let folder = Item { crc: 1, local_sizes: Some((0, 0, 0)), ..Item::stored("Configurations2/", b"") };
    let original = build(&[Item::stored("mimetype", MEDIA_TYPE), folder, Item::stored("styles.xml", b"<s/>")]);
    assert_eq!(stream::walk(&original).map(|names| names.len()), Ok(3));
    let (copy, how) = copy_all(&original);
    assert_eq!(how, [Copied::Exact, Copied::Rewritten, Copied::Exact]);
    assert_eq!(account(&original).rewritten, 1);
    assert_eq!(stream::walk(&copy).map(|names| names.len()), Ok(3));
    assert_eq!(u32_at(&copy, 30 + 8 + MEDIA_TYPE.len() + 14), 1, "the row's checksum in the local header");
}
