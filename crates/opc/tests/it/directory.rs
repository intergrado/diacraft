//! The central directory, read by this crate: where the end record is found, what the ZIP64 records and the rows may say,
//! where the entries are, and what the local header is and is not believed for (the central row wins).
//!
//! The hostile shapes here are *built*, one test for each, and timed: random mutation of a valid archive (the fuzz tests) never makes an
//! archive of fifteen hundred nested end records or a million rows, and that is what went unseen in the first version of this reader.
//! Each bound is a generous ten seconds, so that a loaded machine does not fail it. What the first version, with a third-party ZIP
//! reader, did on each: `fifteen_hundred_nested_end_records_with_locators...` took 26.5 s there, and `a_64_mb_file_of_extra_fields...`
//! and `rows_behind_a_stub...` (in `memory.rs`) aborted for memory under 300 MB and 400 MB. The other shapes below it ran in
//! milliseconds in that reader too (the shape that took it 0.88 s a megabyte could not be rebuilt from the review's description); they
//! stay as the classification of what such files are, and as bounds.

use std::time::{Duration, Instant};

use diacraft_opc::{Limits, Package, Warning};

use crate::zipbuild::{
    Dir, Ends, Item, Local, Opts, Zip64, build, build_with, close, extra_record, many_entries, rows_only, without_end, write_ends,
};
use crate::{base_items, class_of, content_types_xml, package_with};

/// A hang guard and no measure of cost, for every shape that is timed: the shape's honest cost is milliseconds, and each test asserts what the
/// read gave. Thirty seconds is generous for a machine at load 50, and a read that scanned the directory for each row would not end.
const BOUND: Duration = Duration::from_secs(30);

fn class(bytes: &[u8]) -> &'static str {
    class_of(bytes, &Limits::default())
}

fn timed_class(bytes: &[u8]) -> &'static str {
    let start = Instant::now();
    let class = class(bytes);
    assert!(start.elapsed() < BOUND, "took {:?}", start.elapsed());
    class
}

fn with_one_part() -> Vec<Item> {
    let mut items = base_items();
    items.push(Item::deflated("visio/document.xml", b"<d/>"));
    items
}

/// A valid end record of an archive of `rows` empty entries, as bytes of a comment: the claim of `count`, a directory of `size` at
/// `start`, and a comment of `comment_len` that the bytes do not have.
fn fake_end_record(count: u16, size: u32, start: u32, comment_len: u16) -> Vec<u8> {
    let mut fake = Vec::new();
    fake.extend_from_slice(b"PK\x05\x06");
    fake.extend_from_slice(&[0, 0, 0, 0]);
    fake.extend_from_slice(&count.to_le_bytes());
    fake.extend_from_slice(&count.to_le_bytes());
    fake.extend_from_slice(&size.to_le_bytes());
    fake.extend_from_slice(&start.to_le_bytes());
    fake.extend_from_slice(&comment_len.to_le_bytes());
    fake
}

// ---- the end record

#[test]
fn a_fake_end_record_inside_a_comment_cannot_refuse_a_valid_file() {
    // The comment holds bytes that look like an end record: it claims 60,000 entries and a directory of 1,000 bytes at the start, and its
    // own comment length (7) does not reach the end of the file, so it is not the end record. The real one is. (Before: the `zip` crate
    // chose among such records, and one could refuse a good file.)
    let mut comment = b"a comment, then ".to_vec();
    comment.extend(fake_end_record(60_000, 1000, 0, 7));
    comment.extend_from_slice(b" and some more comment");
    let bytes = build_with(&with_one_part(), &Opts { comment, ..Opts::default() });
    let p = Package::read(&bytes, &Limits::default()).expect("the real end record is the one that ends the file");
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/visio/document.xml"]);
}

#[test]
fn an_end_record_that_ends_the_file_decides_even_when_it_is_inside_a_comment() {
    // The rule is "the last signature whose comment length ends exactly at the end of the file", so a record put at the very end of a
    // comment with a comment length of 0 *is* that record, and its numbers decide: here they reach nothing, so the file is damaged. A
    // file built to have two readings gets one of them, and this is which. (Documented in the crate docs.)
    let comment = fake_end_record(1, 46, 0, 0);
    let bytes = build_with(&with_one_part(), &Opts { comment, ..Opts::default() });
    assert_eq!(class(&bytes), "zip_corrupt");
}

#[test]
fn the_longest_comment_an_end_record_can_have_is_found_and_the_window_for_bytes_after_it_ends_where_the_scan_does() {
    let items = with_one_part();
    let longest = build_with(&items, &Opts { comment: vec![b'c'; 65_535], ..Opts::default() });
    let p = Package::read(&longest, &Limits::default()).unwrap();
    assert!(p.warnings().is_empty(), "{:?}", p.warnings());
    // Bytes after the record's comment are ignored, with a warning, while the record is in the final 65,557 bytes. With a
    // comment of 65,534 bytes and one byte after it the record starts exactly 65,557 bytes from the end: found.
    let mut at_the_edge = build_with(&items, &Opts { comment: vec![b'c'; 65_534], ..Opts::default() });
    at_the_edge.push(b'!');
    let p = Package::read(&at_the_edge, &Limits::default()).unwrap();
    assert_eq!(p.warnings(), [Warning::BytesAfterEndRecord { count: 1 }]);
    // The longest comment and one byte after it: the record is 65,558 bytes from the end, a byte outside the scan, and no record fits.
    let mut more = longest.clone();
    more.push(b'!');
    assert_eq!(class(&more), "zip_corrupt");
}

#[test]
fn a_file_with_no_end_record_is_not_zip_or_corrupt_by_how_it_starts() {
    assert_eq!(class(b"just text, with PK\x05\x06 inside it, and no end record that ends it"), "not_zip");
    assert_eq!(class(b"PK\x03\x04 but then nothing that is a zip"), "zip_corrupt");
    // No record fits where the one in the file says it has more comment than the file has left, and that is told by how the file
    // starts. (A valid archive with bytes after its end record is another matter, and reads: `bytes_after_the_end_record_...`.)
    let mut cut = build(&with_one_part());
    let at = cut.len() - 22;
    cut[at + 20] = 5; // a comment of five bytes
    cut.extend_from_slice(b"abc"); // and three of them
    assert_eq!(class(&cut), "zip_corrupt");
    // The same, but not starting with a local header: nothing says it is a zip.
    let mut plain = vec![b'x'; 40];
    plain.extend_from_slice(&fake_end_record(0, 0, 0, 9));
    assert_eq!(class(&plain), "not_zip");
    let mut late = vec![b'x'; 40];
    late.extend_from_slice(&fake_end_record(0, 0, 0, 9));
    late.extend_from_slice(b"abc");
    assert_eq!(class(&late), "not_zip");
}

fn warnings_of(bytes: &[u8]) -> Vec<Warning> {
    Package::read(bytes, &Limits::default()).unwrap().warnings().to_vec()
}

#[test]
fn bytes_after_the_end_record_read_with_a_warning_that_names_them() {
    // Three of 1,906 oracle files have 26 bytes of text after their end record. They read, and say so.
    let items = with_one_part();
    let text = b"a line of text, 26 bytes.\n";
    assert_eq!(text.len(), 26);
    let mut bytes = build(&items);
    bytes.extend_from_slice(text);
    let p = Package::read(&bytes, &Limits::default()).unwrap();
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/visio/document.xml"]);
    assert_eq!(p.warnings(), [Warning::BytesAfterEndRecord { count: 26 }]);
    // The count is what follows the end record and its comment, and with ZIP64 records before it, which are still found.
    let mut commented = build_with(&items, &Opts { comment: b"a comment".to_vec(), zip64: true, ..Opts::default() });
    commented.extend_from_slice(text);
    assert_eq!(warnings_of(&commented), [Warning::BytesAfterEndRecord { count: 26 }]);
    // A comment that holds the extra byte, with its length saying so, is no extra byte: no warning.
    let comment = build_with(&items, &Opts { comment: b"!".to_vec(), ..Opts::default() });
    assert!(warnings_of(&comment).is_empty());
    // Bytes that are not text, and one of them. The warning is the count and nothing else.
    let mut one = build(&items);
    one.push(0);
    assert_eq!(warnings_of(&one), [Warning::BytesAfterEndRecord { count: 1 }]);
    let mut zeros = build(&items);
    zeros.extend_from_slice(&[0u8; 5000]);
    assert_eq!(warnings_of(&zeros), [Warning::BytesAfterEndRecord { count: 5000 }]);
    // The warning is the first of the package's, and does not displace the others.
    let mut orphan = base_items();
    orphan.push(Item::stored("o/_rels/x.xml.rels", b"<Relationships/>"));
    let mut bytes = build(&orphan);
    bytes.extend_from_slice(b"!!");
    assert_eq!(
        warnings_of(&bytes),
        [Warning::BytesAfterEndRecord { count: 2 }, Warning::OrphanRelationships { part: "/o/_rels/x.xml.rels".to_owned() }]
    );
    // And bytes in front of the archive are still refused: the offsets reach nothing.
    let mut stub = b"MZ stub".to_vec();
    stub.extend_from_slice(&build(&items));
    assert_eq!(class(&stub), "zip_corrupt");
}

/// An archive `a`, then a second end record `fake` that names the first row of its directory only, with `comment` as its comment and
/// `after` bytes after that comment: so that which record is read shows in the parts.
fn archive_with_a_later_record(comment: &[u8], after: &[u8]) -> Vec<u8> {
    let mut items = base_items();
    items.extend([Item::stored("a.xml", b"<a/>"), Item::stored("b.xml", b"<b/>")]);
    let (mut bytes, dir) = without_end(&items);
    let first_row = 46 + "[Content_Types].xml".len() as u64;
    write_ends(&mut bytes, &Ends::plain(dir));
    // The later record: one row of the same directory.
    let mut later = Ends::plain(Dir { count: 1, len: first_row, start: dir.start });
    later.comment = comment.to_vec();
    write_ends(&mut bytes, &later);
    bytes.extend_from_slice(after);
    bytes
}

#[test]
fn the_end_record_is_the_last_that_ends_the_file_and_failing_that_the_last_that_fits() {
    // Which record wins, which is the question the shapes of the first reader could not answer one way only. The archive has two end
    // records: its own, which says the three rows, and a later one that says one.
    //
    // 1. The later record ends the file (comment of two bytes, and two bytes): it is the one that decides, and the package has no parts.
    let p = Package::read(&archive_with_a_later_record(b"ZZ", b""), &Limits::default()).unwrap();
    assert_eq!(p.parts().count(), 0);
    assert!(p.warnings().is_empty());
    // 2. Neither ends the file (the later one has a comment of two bytes and two more follow it): no record ends the file, so the last
    //    that fits decides, which is the later one. The bytes after its comment are counted: two. The first record also fits, and is not
    //    the one.
    let inside = archive_with_a_later_record(b"ZZ", b"!!");
    let p = Package::read(&inside, &Limits::default()).unwrap();
    assert_eq!(p.parts().count(), 0, "the last record that fits");
    assert_eq!(p.warnings(), [Warning::BytesAfterEndRecord { count: 2 }]);
    // 3. A record that ends the file wins over one that is later and only fits: the first record is put at the end of a comment the later
    //    record's length reaches, so that the later record ends inside the file and the earlier one ends it.
    let mut items = base_items();
    items.extend([Item::stored("a.xml", b"<a/>"), Item::stored("b.xml", b"<b/>")]);
    let (mut bytes, dir) = without_end(&items);
    let first_row = 46 + "[Content_Types].xml".len() as u64;
    let mut real = Ends::plain(dir);
    let mut fake = Vec::new();
    write_ends(&mut fake, &Ends::plain(Dir { count: 1, len: first_row, start: dir.start }));
    fake.truncate(20);
    fake.extend_from_slice(&[3, 0]); // a comment of three bytes
    fake.extend_from_slice(b"abc");
    fake.extend_from_slice(b"!!"); // and two more after it: it fits, and does not end the file
    real.comment = fake;
    write_ends(&mut bytes, &real);
    let p = Package::read(&bytes, &Limits::default()).unwrap();
    assert_eq!(p.parts().count(), 2, "a record that ends the file wins over a later one that only fits");
    assert!(p.warnings().is_empty());
    // 4. The later record's comment runs past the end of the file: it does not fit, and the first record is the last that does (it fits,
    //    and the bytes after it, the later record and its two, are the count).
    let mut cut = archive_with_a_later_record(b"ZZ", b"");
    let comment_length_at = cut.len() - 4;
    cut[comment_length_at] = 9; // nine bytes of comment, and there are two
    let p = Package::read(&cut, &Limits::default()).unwrap();
    assert_eq!(p.parts().count(), 2, "a record whose comment runs past the end of the file does not fit");
    assert_eq!(p.warnings(), [Warning::BytesAfterEndRecord { count: 24 }]);
}

#[test]
fn an_appended_directory_is_read_by_its_end_record_and_the_rest_is_unread_bytes() {
    // Finding 12: a second directory and end record appended to a good archive. The end record at the end decides: here it names only the
    // content types stream, so the package reads with no parts and `first.xml` (still in the file, still in the first directory) is
    // bytes nobody reads: not a part, not an error. This is the one reading, documented in the crate docs.
    let mut items = base_items();
    items.push(Item::stored("first.xml", b"<first/>"));
    let mut bytes = build(&items);
    assert!(Package::read(&bytes, &Limits::default()).unwrap().part("first.xml").is_some());
    // The first end record stays where it is (it is now inside the file); a directory of one row follows, for the stream at offset 0.
    let stream = &items[0];
    let mut row = Vec::new();
    row.extend_from_slice(b"PK\x01\x02");
    row.extend_from_slice(&[45, 0, 20, 0, 0, 0]);
    row.extend_from_slice(&8u16.to_le_bytes()); // deflate
    row.extend_from_slice(&[0, 0, 0x21, 0]);
    row.extend_from_slice(&stream.crc.to_le_bytes());
    row.extend_from_slice(&(stream.csize as u32).to_le_bytes());
    row.extend_from_slice(&(stream.size as u32).to_le_bytes());
    row.extend_from_slice(&(stream.name.len() as u16).to_le_bytes());
    row.extend_from_slice(&[0; 12]); // extra, comment, disk, attributes
    row.extend_from_slice(&0u32.to_le_bytes()); // offset of the stream's local header
    row.extend_from_slice(&stream.name);
    let second_directory = bytes.len();
    bytes.extend_from_slice(&row);
    close(&mut bytes, &Opts::default(), 1, row.len() as u64, second_directory as u64);
    let p = Package::read(&bytes, &Limits::default()).unwrap();
    assert_eq!(p.parts().count(), 0, "the last end record decides");
    assert_eq!(p.part("first.xml"), None);
    assert!(p.warnings().is_empty(), "nothing is reported for the bytes nobody reads: {:?}", p.warnings());
}

#[test]
fn an_archive_that_says_it_spans_disks_is_refused() {
    let mut bytes = build(&with_one_part());
    let at = bytes.len() - 22;
    bytes[at + 4] = 1; // this disk is disk 1
    assert_eq!(class(&bytes), "zip_corrupt");
    let mut bytes = build(&with_one_part());
    let at = bytes.len() - 22;
    bytes[at + 8] = bytes[at + 8].wrapping_add(1); // entries on this disk differ from the total
    assert_eq!(class(&bytes), "zip_corrupt");
}

// ---- ZIP64

#[test]
fn zip64_end_records_with_honest_values_read() {
    let bytes = build_with(&with_one_part(), &Opts { zip64: true, ..Opts::default() });
    assert_eq!(Package::read(&bytes, &Limits::default()).unwrap().parts().collect::<Vec<_>>(), ["/visio/document.xml"]);
}

#[test]
fn a_zip64_locator_that_points_nowhere_is_corrupt() {
    let good = build_with(&with_one_part(), &Opts { zip64: true, ..Opts::default() });
    let locator = good.windows(4).rposition(|w| w == b"PK\x06\x07").unwrap();
    // Offset of the ZIP64 record: past the end, and into the middle of the directory.
    for offset in [u64::MAX, good.len() as u64, 7] {
        let mut bad = good.clone();
        bad[locator + 8..locator + 16].copy_from_slice(&offset.to_le_bytes());
        assert_eq!(class(&bad), "zip_corrupt", "offset {offset}");
    }
    // The ZIP64 record's signature destroyed.
    let mut bad = good.clone();
    let record = good.windows(4).rposition(|w| w == b"PK\x06\x06").unwrap();
    bad[record] = b'X';
    assert_eq!(class(&bad), "zip_corrupt");
    // And a record that says it is shorter than the fields it must have.
    let mut short = good.clone();
    short[record + 4..record + 12].copy_from_slice(&43u64.to_le_bytes());
    assert_eq!(class(&short), "zip_corrupt");
}

#[test]
fn a_zip64_marker_with_no_locator_is_only_a_number() {
    // 0xFFFF in the plain count with no ZIP64 locator is a count of 65,535 (a legal one): it cannot fit in a directory of a few bytes, so
    // it is damage, and not an attempt to find a record that is not there.
    let mut bytes = build(&with_one_part());
    let at = bytes.len() - 22;
    bytes[at + 8..at + 12].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
    assert_eq!(class(&bytes), "zip_corrupt");
}

#[test]
fn sizes_given_as_zip64_sentinels_with_an_honest_field_read() {
    // Review finding 14: no test had this shape. The sizes are small; the row and the local header write 0xFFFFFFFF and carry the true
    // values in the ZIP64 field, as writers do for any entry once they have gone ZIP64.
    let mut items = base_items();
    items.push(Item::deflated("visio/document.xml", &b"<d/>".repeat(50)).with_zip64_field());
    items.push(Item::stored("visio/small.bin", b"1234").with_zip64_field());
    let bytes = build(&items);
    let p = Package::read(&bytes, &Limits::default()).unwrap();
    assert_eq!(p.part("/visio/document.xml"), Some(&b"<d/>".repeat(50)[..]));
    assert_eq!(p.part("/visio/small.bin"), Some(&b"1234"[..]));
}

#[test]
fn a_row_that_leaves_a_value_to_its_zip64_field_and_has_none_is_corrupt() {
    // The row's two sizes are the sentinel and its extra field has no ZIP64 field, one that is too short for the values, or one for
    // another id.
    for extra in [Vec::new(), extra_record(1, &[1, 2, 3]), extra_record(0xBEEF, &[0; 16])] {
        let mut items = base_items();
        items.push(Item::stored("a.bin", b"data").with_extra(&extra));
        let mut bytes = build(&items);
        let row = bytes.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
        bytes[row + 20..row + 28].copy_from_slice(&[0xFF; 8]); // the compressed and the uncompressed size
        assert_eq!(class(&bytes), "zip_corrupt");
    }
}

#[test]
fn a_locator_is_ignored_when_no_field_of_the_end_record_asks_for_it() {
    // Twenty bytes that look like a ZIP64 locator, pointing nowhere, in front of a plain end record whose fields are all its own: they
    // are only bytes between the directory and the end record, and the file reads.
    let bytes = build(&with_one_part());
    let at = bytes.len() - 22;
    let mut locator = b"PK\x06\x07\0\0\0\0".to_vec();
    locator.extend_from_slice(&999_999u64.to_le_bytes());
    locator.extend_from_slice(&1u32.to_le_bytes());
    let mut with = bytes[..at].to_vec();
    with.extend_from_slice(&locator);
    with.extend_from_slice(&bytes[at..]);
    assert_eq!(Package::read(&with, &Limits::default()).unwrap().parts().count(), 1);
}

#[test]
fn a_directory_that_runs_over_its_end_record_is_corrupt() {
    // The size says the directory is 22 bytes longer than the space before the end record: its last rows would be the end record.
    let mut bytes = build(&with_one_part());
    let at = bytes.len() - 22;
    let size = u32::from_le_bytes(bytes[at + 12..at + 16].try_into().unwrap());
    bytes[at + 12..at + 16].copy_from_slice(&(size + 22).to_le_bytes());
    assert_eq!(class(&bytes), "zip_corrupt");
    // And the same by an offset that starts the directory 22 bytes late: the rows end where the end record is, but run past it.
    let mut late = build(&with_one_part());
    let start = u32::from_le_bytes(late[at + 16..at + 20].try_into().unwrap());
    late[at + 16..at + 20].copy_from_slice(&(start + 22).to_le_bytes());
    assert_eq!(class(&late), "zip_corrupt");
}

#[test]
fn a_row_that_says_its_entry_is_on_another_disk_is_corrupt() {
    let mut bytes = package_with(vec![Item::stored("a.xml", b"<a/>")]);
    let row = bytes.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
    bytes[row + 34] = 1; // the disk number where the entry starts
    assert_eq!(class(&bytes), "zip_corrupt");
    // A disk number of 0xFFFF is "see the ZIP64 field", and with none it is damage. (With one that says 0 it reads, and with one that
    // says another disk it is refused: `a_disk_number_left_to_the_zip64_field_is_read_after_the_values_that_come_before_it`.)
    let mut sentinel = package_with(vec![Item::stored("a.xml", b"<a/>")]);
    let row = sentinel.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
    sentinel[row + 34..row + 36].copy_from_slice(&[0xFF, 0xFF]);
    assert_eq!(class(&sentinel), "zip_corrupt");
}

// ---- rows

#[test]
fn a_row_longer_than_65535_bytes_is_corrupt_and_one_of_exactly_that_is_not() {
    // A central directory file header must not exceed 65,535 bytes. The row is 46 bytes, the name (5), the extra field and the comment.
    let mut at = Item::stored("a.xml", b"<a/>");
    at.row_comment = vec![b'c'; 65_535 - 46 - 5];
    let mut over = at.clone();
    over.row_comment.push(b'c');
    let mut items = base_items();
    items.push(at);
    assert!(Package::read(&build(&items), &Limits::default()).is_ok());
    let mut items = base_items();
    items.push(over);
    assert_eq!(class(&build(&items)), "zip_corrupt");
    // The same through the extra field, which the reader walks and never keeps.
    let mut extra = Item::stored("a.xml", b"<a/>");
    extra.extra = vec![0; 65_535 - 46 - 5 + 1];
    let mut items = base_items();
    items.push(extra);
    assert_eq!(class(&build(&items)), "zip_corrupt");
}

#[test]
fn a_name_far_past_the_limit_is_refused_without_being_copied_into_the_error() {
    // 3,073 bytes is the first that is more than three times the limit of 1,024, 60,000 is far past it; both are refused from the row,
    // and the error holds a few bytes of the name and not the name.
    for length in [3_073usize, 5_000, 60_000] {
        let long = "n".repeat(length);
        let bytes = package_with(vec![Item::stored(&long, b"x")]);
        let err = Package::read(&bytes, &Limits::default()).unwrap_err();
        assert_eq!(err.class(), "invalid_part_name");
        assert!(err.to_string().len() < 500, "a name of {length} bytes: the error holds {} bytes", err.to_string().len());
    }
    // A name of 1,025 to 3,072 bytes is refused too, but after it is decoded, and the error does hold it.
    let middling = package_with(vec![Item::stored(&"n".repeat(2_000), b"x")]);
    assert_eq!(class(&middling), "invalid_part_name");
}

#[test]
fn extra_fields_that_are_not_zip64_or_a_path_are_ignored_even_when_broken() {
    // Unknown records, a truncated record at the end, a lone byte: none is kept and none refuses the row.
    let mut extra = extra_record(0xCAFE, b"unknown record");
    extra.extend(extra_record(0x5455, &[1, 2, 3, 4, 5]));
    extra.extend_from_slice(&[0x34, 0x12, 0xFF, 0x00, 1, 2, 3]); // a record that says 255 bytes and has 3
    let mut items = base_items();
    items.push(Item::stored("a.xml", b"<a/>").with_extra(&extra));
    items.push(Item::stored("b.xml", b"<b/>").with_extra(&[7]));
    let p = Package::read(&build(&items), &Limits::default()).unwrap();
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/a.xml", "/b.xml"]);
}

#[test]
fn the_unicode_path_field_gives_the_name_when_it_is_for_this_name() {
    // Info-ZIP's `up` field: version 1, the CRC-32 of the name as written, then the UTF-8 name.
    let mut items = base_items();
    items.push(Item::stored("cafe.xml", b"1").with_unicode_path("caf\u{e9}.xml"));
    // The checksum is of another name: the field is not for this row and is ignored.
    let stale = Item::stored("old.xml", b"2").with_unicode_path("never.xml").named_bytes(b"new.xml");
    items.push(stale);
    // A version other than 1 is ignored.
    let mut body = vec![2u8];
    body.extend_from_slice(&crc32fast::hash(b"v2.xml").to_le_bytes());
    body.extend_from_slice(b"ignored.xml");
    items.push(Item::stored("v2.xml", b"3").with_extra(&extra_record(0x7075, &body)));
    let p = Package::read(&build(&items), &Limits::default()).unwrap();
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/café.xml", "/new.xml", "/v2.xml"]);
}

#[test]
fn a_name_flagged_as_utf_8_that_is_not_is_corrupt_and_valid_utf_8_is_read_as_such() {
    let flagged = |name: &[u8]| {
        let mut items = base_items();
        items.push(Item::stored("x", b"1").named_bytes(name).with_flags(0x0800));
        build(&items)
    };
    assert_eq!(class(&flagged(b"caf\x82.xml")), "zip_corrupt");
    let p = Package::read(&flagged("caf\u{e9}.xml".as_bytes()), &Limits::default()).unwrap();
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/café.xml"]);
}

#[test]
fn every_flag_that_means_encryption_is_an_encrypted_package() {
    // Review finding 7: bit 0 (encrypted), bit 6 (strong encryption) and bit 13 (masked local header values).
    for bit in [0x0001u16, 0x0040, 0x2000] {
        assert_eq!(crate::class_of_more(vec![Item::stored("a.xml", b"<a/>").with_flags(bit)]), "encrypted_package", "flag {bit:#06x}");
    }
    // Bits that are not: the data descriptor (3), UTF-8 names (11), enhanced deflating (4) and the compression option bits (1, 2).
    for bit in [0x0002u16, 0x0004, 0x0008, 0x0010, 0x0800] {
        assert!(Package::read(&package_with(vec![Item::stored("a.xml", b"<a/>").with_flags(bit)]), &Limits::default()).is_ok(), "flag {bit:#06x}");
    }
}

// ---- where the entries are

#[test]
fn a_row_whose_local_header_is_not_where_it_says_is_corrupt() {
    let mut bytes = package_with(vec![Item::stored("a.xml", b"<a/>")]);
    let row = bytes.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
    for offset in [3u32, 100_000, u32::MAX - 1] {
        let mut bad = bytes.clone();
        bad[row + 42..row + 46].copy_from_slice(&offset.to_le_bytes());
        assert_eq!(class(&bad), "zip_corrupt", "offset {offset}");
    }
    // An entry whose data runs into the directory, by its own compressed size (and its header says the same, so the ratio is fine).
    let last = bytes.windows(4).rposition(|w| w == b"PK\x03\x04").unwrap();
    bytes[last + 18..last + 22].copy_from_slice(&1000u32.to_le_bytes());
    bytes[row + 20..row + 24].copy_from_slice(&1000u32.to_le_bytes());
    bytes[row + 24..row + 28].copy_from_slice(&1000u32.to_le_bytes());
    assert_eq!(class(&bytes), "zip_corrupt");
}

#[test]
fn directory_items_count_toward_the_row_limit_and_are_not_all_kept() {
    // Review finding 3: a million folders read as zero parts with a million warnings. The rows are bounded by the count (twice the part
    // limit and sixteen: 100,016 by default), and the folders named in warnings are bounded too.
    let names = |n: usize| (0..n).map(|i| format!("d{i}/").into_bytes());
    let ct = content_types_xml(&[("xml", "application/xml")], &[]);
    let limit = 2 * Limits::default().max_parts + 16;
    // At the limit (the content types stream and the rest folders): reads, names a hundred of them and counts the rest in one warning.
    let start = Instant::now();
    let at = many_entries(names(limit - 1), 0, ct.as_bytes());
    let p = Package::read(&at, &Limits::default()).unwrap();
    assert_eq!(p.parts().count(), 0);
    let named = p.warnings().iter().filter(|w| matches!(w, Warning::DirectoryItem { .. })).count();
    assert_eq!(named, 100);
    assert!(p.warnings().contains(&Warning::MoreDirectoryItems { count: limit - 1 - 100 }), "{:?}", p.warnings().last());
    assert_eq!(p.warnings().len(), 101);
    // One more row: too many.
    let over = many_entries(names(limit), 0, ct.as_bytes());
    assert_eq!(class(&over), "too_many_parts");
    assert!(start.elapsed() < BOUND, "took {:?}", start.elapsed());
}

#[test]
fn a_count_is_checked_against_46_bytes_a_row_and_not_a_smaller_number() {
    // A directory of four rows with long comments is about 700 bytes. A part limit of 1 lets 18 rows (twice it and sixteen). A claim of 25
    // rows would take at least 25 x 46 = 1,150 bytes, so it is damage; a check at 20 bytes a row (500) would let it by to be refused for
    // its count, as `too_many_parts`, which says "a bigger limit would open it" of a file that is only wrong.
    let limits = Limits { max_parts: 1, ..Limits::default() };
    let mut items = base_items();
    for i in 0..3 {
        let mut item = Item::stored(&format!("p{i}.bin"), b"x");
        item.row_comment = vec![b'c'; 120];
        items.push(item);
    }
    let bytes = build_with(&items, &Opts { claimed_entries: Some(25), ..Opts::default() });
    assert_eq!(class_of(&bytes, &limits), "zip_corrupt");
    // The same directory with the honest count is past the limit of parts (three, with a limit of one) and not the rows'.
    assert_eq!(class_of(&build(&items), &limits), "too_many_parts");
}

#[test]
fn an_entry_whose_size_reaches_into_the_directory_is_refused_though_its_stream_ends_early() {
    // A deflate stream that ends where it should, and a row that says the entry is 50 bytes longer than that (less than the directory and
    // the end record together, so that the claim stays inside the file): the extra bytes are the directory. A reader that inflates it reads the right bytes (the stream stops by itself) and a checksum that is right, so only
    // the layout says it is wrong.
    let content = b"<a>an entry</a>".repeat(10);
    let honest = Item::deflated("a.xml", &content);
    let longer = honest.csize + 50;
    let mut items = base_items();
    items.push(honest.clone().claiming_csize(longer));
    assert_eq!(class(&build(&items)), "zip_corrupt");
    let mut items = base_items();
    items.push(honest);
    assert!(Package::read(&build(&items), &Limits::default()).is_ok());
}

#[test]
fn a_local_header_with_a_damaged_signature_is_corrupt_though_its_lengths_would_do() {
    // The row points at a local header whose signature is one byte wrong and whose lengths are right: only the signature says it is not one.
    let mut bytes = package_with(vec![Item::stored("a.xml", b"<a/>")]);
    let local = bytes.windows(4).rposition(|w| w == b"PK\x03\x04").unwrap();
    bytes[local + 3] = 5;
    assert_eq!(class(&bytes), "zip_corrupt");
}

// ---- the central row wins

fn central_wins(item: Item) -> Package {
    let mut items = base_items();
    items.push(item);
    Package::read(&build(&items), &Limits::default()).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn central_wins_on_the_name_and_a_local_name_of_dotdot_is_never_used() {
    let local = Local { name: Some(b"../evil.xml".to_vec()), ..Local::default() };
    let p = central_wins(Item::deflated("visio/a.xml", b"<a/>").with_local(local));
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/visio/a.xml"]);
    assert_eq!(p.part("/visio/a.xml"), Some(&b"<a/>"[..]));
    assert_eq!(p.part("../evil.xml"), None);
}

#[test]
fn central_wins_on_method_size_checksum_and_flags() {
    // The local header says stored, 999 bytes, a checksum of 0, and that the entry is encrypted: none of it is read.
    let local = Local { method: Some(0), sizes: Some((999, 999)), crc: Some(0), flags: Some(0x0001 | 0x0800), ..Local::default() };
    let content = b"<a>the central row says deflate, and what the bytes really are</a>".repeat(5);
    let p = central_wins(Item::deflated("a.xml", &content).with_local(local));
    assert_eq!(p.part("a.xml"), Some(&content[..]));
    // And the central row's own lie is caught by the central row's own checksum, whatever the local header says: the local header
    // here is honest and the row is not.
    let mut items = base_items();
    items.push(Item::deflated("b.xml", &content).with_bad_crc().with_local(Local { crc: Some(crc32fast::hash(&content)), ..Local::default() }));
    assert_eq!(class(&build(&items)), "zip_corrupt");
}

#[test]
fn central_wins_a_data_descriptor_is_never_read() {
    // Flag bit 3: the local header has no sizes, and a data descriptor follows the data. This one lies (huge sizes, wrong checksum, a
    // second signature after it): the row's sizes decide, and the bytes after the data are not looked at.
    let mut descriptor = b"PK\x07\x08".to_vec();
    descriptor.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    descriptor.extend_from_slice(&u32::MAX.to_le_bytes());
    descriptor.extend_from_slice(&u32::MAX.to_le_bytes());
    descriptor.extend_from_slice(b"PK\x03\x04PK\x01\x02PK\x05\x06");
    let local = Local { flags: Some(0x0008), sizes: Some((0, 0)), crc: Some(0), ..Local::default() };
    let content = b"<a/>".repeat(100);
    let item = Item::deflated("a.xml", &content).with_flags(0x0008).with_local(local).with_descriptor(&descriptor);
    let p = central_wins(item);
    assert_eq!(p.part("a.xml"), Some(&content[..]));
}

#[test]
fn central_wins_the_local_extra_field_only_says_where_the_data_starts() {
    // The local header's extra field has another length than the row's, as writers make them (a timestamp in one, ZIP64 in the other):
    // the data is found by the local header's own lengths.
    let local = Local { extra: vec![0xAA; 37], ..Local::default() };
    let p = central_wins(Item::stored("a.xml", b"<a/>").with_local(local).with_extra(&extra_record(0xCAFE, &[0; 3])));
    assert_eq!(p.part("a.xml"), Some(&b"<a/>"[..]));
}

// ---- hostile shapes, built and timed (review findings 1 and 3)

#[test]
fn ten_thousand_end_records_and_a_megabyte_of_zip64_signatures_cost_one_scan() {
    // Finding 1, the shape as described: 1 MB of ZIP64 end record signatures, a tail of 10,000 end record signatures, none of which ends
    // the file; and the same with a last record that does. (The first reader answered this one in microseconds too: the shape that cost
    // it minutes is the next test's.)
    let mut bytes = Vec::new();
    for _ in 0..(1 << 20) / 8 {
        bytes.extend_from_slice(b"PK\x06\x06\x2c\0\0\0");
    }
    let body = bytes.len();
    for _ in 0..10_000 {
        bytes.extend_from_slice(b"PK\x05\x06\0\0\0\0\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0\x01\x00");
    }
    assert!(bytes.len() > body + 200_000);
    // The last record but one has a comment of one byte, which is inside the file: it is the end record that fits, with the bytes after it
    // ignored, and its numbers reach nothing. A record that fits is a record, so this is `zip_corrupt` and no longer
    // `not_zip`; the cost is the one scan.
    assert_eq!(timed_class(&bytes), "zip_corrupt");
    let mut starts_like_a_zip = b"PK\x03\x04".to_vec();
    starts_like_a_zip.extend(&bytes);
    assert_eq!(timed_class(&starts_like_a_zip), "zip_corrupt");
    // With the last record ending the file and a locator before it, so that the ZIP64 record is looked for: it is not where the locator says.
    let mut ends = Vec::new();
    ends.extend_from_slice(&bytes[..1 << 20]);
    ends.extend_from_slice(b"PK\x06\x07\0\0\0\0");
    ends.extend_from_slice(&(7u64).to_le_bytes());
    ends.extend_from_slice(&1u32.to_le_bytes());
    ends.extend_from_slice(b"PK\x05\x06\0\0\0\0\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0");
    assert_eq!(timed_class(&ends), "zip_corrupt");
    let mut zipped = b"PK\x03\x04".to_vec();
    zipped.extend(&ends);
    assert_eq!(timed_class(&zipped), "zip_corrupt");
}

/// End records nested so that every one has a comment that reaches the end of the file, each behind a ZIP64 locator, after `megabytes`
/// of ZIP64 record signatures: every one is "the end record" by the comment rule, and every one points at a ZIP64 record that is one
/// of the signatures. The first reader retried every signature for every record.
fn nested_end_records_with_locators(records: usize, megabytes: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for _ in 0..(megabytes << 20) / 8 {
        bytes.extend_from_slice(b"PK\x06\x06\x2c\0\0\0");
    }
    let mut tail: Vec<u8> = Vec::new();
    for _ in 0..records {
        let mut record = b"PK\x06\x07\0\0\0\0".to_vec();
        record.extend_from_slice(&5u64.to_le_bytes());
        record.extend_from_slice(&1u32.to_le_bytes());
        record.extend_from_slice(b"PK\x05\x06\0\0\0\0\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff");
        record.extend_from_slice(&(tail.len() as u16).to_le_bytes());
        record.extend_from_slice(&tail);
        tail = record;
    }
    bytes.extend_from_slice(&tail);
    bytes
}

#[test]
fn fifteen_hundred_nested_end_records_with_locators_behind_nine_megabytes_of_zip64_signatures_cost_one_scan() {
    // Finding 1, built: with the first reader this file (9.5 MB) took 26.5 seconds (100 records and 1 MB took 0.19 s and 300 took 0.55 s:
    // the cost is the records times the signatures, and 1,500 records is the most the last 65,557 bytes hold). Here the last record that
    // ends the file is the end record, its ZIP64 locator points at nothing, and that is `zip_corrupt` after one backward scan.
    let bytes = nested_end_records_with_locators(1500, 9);
    assert!(bytes.len() > 9_000_000);
    assert_eq!(timed_class(&bytes), "zip_corrupt");
    let mut starts_like_a_zip = b"PK\x03\x04".to_vec();
    starts_like_a_zip.extend(&bytes);
    assert_eq!(timed_class(&starts_like_a_zip), "zip_corrupt");
}

#[test]
fn four_megabytes_of_directory_row_signatures_cost_nothing_that_grows_with_them() {
    // Finding 1, the signature cache (the review measured 0.88 s for 1 MB and four times that for each doubling; that shape could not be
    // rebuilt, and this one cost the first reader 36 ms): four megabytes of row signatures, an end record that ends the file and says
    // there is a directory in there.
    let mut bytes = Vec::new();
    for _ in 0..(4 << 20) / 4 {
        bytes.extend_from_slice(b"PK\x01\x02");
    }
    let (start, len) = (0u64, bytes.len() as u64);
    close(&mut bytes, &Opts::default(), 1000, len, start);
    // The first row's fields are the signature over and over: a name length of 19,280, far past the limit, which is refused before a
    // byte of it is copied. (A row of this shape cannot be anything else, and what is measured is the time.)
    assert_eq!(timed_class(&bytes), "invalid_part_name");
    // And as ZIP64 records.
    let mut wide = Vec::new();
    for _ in 0..(4 << 20) / 4 {
        wide.extend_from_slice(b"PK\x06\x06");
    }
    let len = wide.len() as u64;
    close(&mut wide, &Opts { zip64: true, ..Opts::default() }, 1 << 40, len, 0);
    assert_eq!(timed_class(&wide), "zip_corrupt");
}

#[test]
fn rows_behind_a_stub_and_rows_past_the_limit_are_refused_at_once() {
    // Finding 3: 4 million rows behind a stub (a 202 MB file) took 942 MB, and a one-byte stub defeated the check of the claim. Here the
    // count is checked against the directory's bytes and then the limit before a row is read: a stub is damage (the offsets reach
    // nothing), no stub is `too_many_parts`, and either is instant. 1.5 million rows, 75 MB.
    let start = Instant::now();
    assert_eq!(class(&rows_only(1_500_000, 1)), "zip_corrupt");
    assert_eq!(class(&rows_only(1_500_000, 0)), "too_many_parts");
    assert!(start.elapsed() < BOUND, "took {:?}", start.elapsed());
}

// ----: the walk ends at the count, and the directory has to end there too

/// `rows` rows: the content types stream and `rows - 1` stored parts.
fn rows_of(rows: usize) -> Vec<Item> {
    let mut items = base_items();
    items.extend((1..rows).map(|i| Item::stored(&format!("p{i}.xml"), b"")));
    items
}

/// The position of the last row of the directory in `bytes`.
fn last_row(bytes: &[u8]) -> usize {
    bytes.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap()
}

#[test]
fn a_count_below_the_rows_of_the_directory_is_damage_and_never_a_package_with_parts_gone() {
    // Three rows and a count of two read as a package with `p2.xml` gone, and a later save would have lost it. Rows that the count leaves
    // behind are a row signature where the directory should be over.
    let items = rows_of(3);
    for zip64 in [false, true] {
        let honest = build_with(&items, &Opts { zip64, ..Opts::default() });
        assert_eq!(Package::read(&honest, &Limits::default()).unwrap().parts().count(), 2, "zip64 {zip64}");
        for claimed in [2u64, 1, 0] {
            let short = build_with(&items, &Opts { zip64, claimed_entries: Some(claimed), ..Opts::default() });
            assert_eq!(class(&short), "zip_corrupt", "zip64 {zip64}, count {claimed}");
        }
    }
}

#[test]
fn every_single_bit_of_a_count_flipped_is_damage_in_the_plain_record_and_in_the_zip64_one() {
    // 100 rows (bits 2, 5 and 6 are set): a bit cleared makes a count below the rows (36 with bit 6: 35 parts and no warning, before), a
    // bit set makes one above them. The plain record has 16 bits and the ZIP64 count 64: here 24 of them, past the 16.
    let items = rows_of(100);
    for bit in 0..16 {
        let claimed = 100u64 ^ (1 << bit);
        let bytes = build_with(&items, &Opts { claimed_entries: Some(claimed), ..Opts::default() });
        assert_eq!(class(&bytes), "zip_corrupt", "plain count {claimed}: bit {bit} flipped");
    }
    for bit in 0..24 {
        let claimed = 100u64 ^ (1 << bit);
        let bytes = build_with(&items, &Opts { zip64: true, claimed_entries: Some(claimed), ..Opts::default() });
        assert_eq!(class(&bytes), "zip_corrupt", "ZIP64 count {claimed}: bit {bit} flipped");
    }
}

#[test]
fn a_count_that_wrapped_past_65535_is_damage_and_not_a_package_of_nothing() {
    // 65,537 rows (the stream and 65,536 folders) whose count a 16-bit writer left at 65,537 - 65,536 = 1: before, that read as no parts
    // and no warning, in the plain record and in the ZIP64 one. The honest ZIP64 count reads.
    let mut items = base_items();
    items.extend((0..65_536).map(|i| Item::stored(&format!("d{i}/"), b"")));
    let honest = build_with(&items, &Opts { zip64: true, ..Opts::default() });
    assert_eq!(Package::read(&honest, &Limits::default()).unwrap().parts().count(), 0);
    for zip64 in [false, true] {
        let wrapped = build_with(&items, &Opts { zip64, claimed_entries: Some(1), ..Opts::default() });
        assert_eq!(class(&wrapped), "zip_corrupt", "zip64 {zip64}");
    }
}

/// `bytes` (an archive with a plain end record and no comment), with `extra` after its last row and covered by the size the end record
/// gives the directory.
fn with_bytes_after_the_rows(bytes: &[u8], extra: &[u8]) -> Vec<u8> {
    let at = bytes.len() - 22;
    let mut out = bytes[..at].to_vec();
    out.extend_from_slice(extra);
    out.extend_from_slice(&bytes[at..]);
    let size_at = at + extra.len() + 12;
    let size = u32::from_le_bytes(out[size_at..size_at + 4].try_into().unwrap());
    out[size_at..size_at + 4].copy_from_slice(&(size + extra.len() as u32).to_le_bytes());
    out
}

#[test]
fn a_digital_signature_record_after_the_rows_is_ignored_and_a_row_after_them_is_not() {
    // Digital signature records in the ZIP are ignored on read. The record is `PK\x05\x05`, a 16-bit size and that many bytes, and
    // it ends the directory. Bytes that are not a row are ignored too; bytes that start a row are the rows the count left behind.
    let bytes = build(&with_one_part());
    let mut signature = b"PK\x05\x05".to_vec();
    signature.extend_from_slice(&30u16.to_le_bytes());
    signature.extend_from_slice(&[0xAB; 30]);
    let signed = with_bytes_after_the_rows(&bytes, &signature);
    assert_eq!(Package::read(&signed, &Limits::default()).unwrap().parts().collect::<Vec<_>>(), ["/visio/document.xml"]);
    let junk = with_bytes_after_the_rows(&bytes, b"not a row at all");
    assert_eq!(Package::read(&junk, &Limits::default()).unwrap().parts().count(), 1);
    // Only the signature of a row is looked for: four bytes of it are enough, and three are not.
    let four = with_bytes_after_the_rows(&bytes, b"PK\x01\x02");
    assert_eq!(class(&four), "zip_corrupt");
    let three = with_bytes_after_the_rows(&bytes, b"PK\x01");
    assert_eq!(Package::read(&three, &Limits::default()).unwrap().parts().count(), 1);
    // A whole row after the last the count allows.
    let row = &bytes[last_row(&bytes)..bytes.len() - 22];
    let one_too_many = with_bytes_after_the_rows(&bytes, row);
    assert_eq!(class(&one_too_many), "zip_corrupt");
    // Behind a signature record, a row is no longer the next thing in the directory: the signature ended it, and it is not read as one.
    let hidden = with_bytes_after_the_rows(&bytes, &[&signature[..], row].concat());
    assert_eq!(Package::read(&hidden, &Limits::default()).unwrap().parts().count(), 1);
}

// ----: a name is judged as written before the Unicode path field can stand in for it

/// A package of the content types stream and one entry whose name is `raw` as written, with a Unicode path field that says `path`.
fn named_with_unicode_path(raw: &[u8], path: &str) -> Vec<u8> {
    let mut items = base_items();
    items.push(Item::stored("x", b"<x/>").named_bytes(raw).with_unicode_path(path));
    build(&items)
}

#[test]
fn a_unicode_path_cannot_stand_in_for_a_name_that_is_a_traversal_as_written() {
    // Any reader that does not read the field sees these names, and so does this one before the field replaces them. Each was read
    // as `/ok.xml` before.
    let control = Package::read(&named_with_unicode_path(b"file.xml", "ok.xml"), &Limits::default()).unwrap();
    assert_eq!(control.parts().collect::<Vec<_>>(), ["/ok.xml"]);
    let hostile: [&[u8]; 9] = [
        b"../evil.xml",
        b"/abs.xml",
        b"a\\b.xml",
        b"%2e%2e/x.xml",
        b"c:/x.xml",
        b"a%2Fb.xml",
        b"a/../b.xml",
        b"\xff/../x.xml", // not UTF-8: read by its bytes
        b"%FF/../x.xml",  // percent-escapes that are not UTF-8
    ];
    for raw in hostile {
        let bytes = named_with_unicode_path(raw, "ok.xml");
        assert_eq!(class(&bytes), "path_traversal", "{:?}", String::from_utf8_lossy(raw));
    }
}

#[test]
fn a_unicode_path_cannot_make_a_file_a_folder_and_so_vanish_or_a_folder_a_file() {
    // The raw name `file.xml` is a file whatever the field says: a field that makes it `d/` disagrees about being a folder, and the row is
    // refused (in round 2 it was refused for the empty segment of `d/`, before that it was a folder, dropped, and the package
    // read with one part gone and a warning). The raw name `d/` is a folder whatever the field says, and a field that makes it
    // `file.xml` disagrees too.
    assert_eq!(class(&named_with_unicode_path(b"file.xml", "d/")), "invalid_part_name");
    assert_eq!(class(&named_with_unicode_path(b"d/", "file.xml")), "invalid_part_name");
    // Where the field agrees that it is a folder, it gives the folder's name.
    let mut items = base_items();
    items.push(Item::stored("x", b"").named_bytes(b"d/").with_unicode_path("e/"));
    let p = Package::read(&build(&items), &Limits::default()).unwrap();
    assert_eq!(p.parts().count(), 0);
    assert_eq!(p.warnings(), [Warning::DirectoryItem { name: "e/".to_owned() }]);
}

/// A package whose only entry is the content types stream, with the name `raw` as written and the Unicode path field `path` (none when it
/// is `None`), and a part `a.xml`.
fn stream_named(raw: &[u8], path: Option<&str>) -> Vec<u8> {
    let ct = content_types_xml(&[("xml", "application/xml")], &[]);
    let mut stream = Item::deflated("x", ct.as_bytes()).named_bytes(raw);
    if let Some(path) = path {
        stream = stream.with_unicode_path(path);
    }
    build(&[stream, Item::stored("a.xml", b"<a/>")])
}

#[test]
fn a_unicode_path_cannot_make_a_part_the_content_types_stream_or_the_stream_a_part() {
    // The stream is told by its name as written, too: a field that spells `[Content_Types].xml` over another name is refused, as is any
    // field over the stream's own name that is not the stream's name (round 3: before, the stream stayed the stream and no error said the
    // two names disagreed; a reader that honours the field would see a part, or a traversal, or a folder).
    assert_eq!(class(&named_with_unicode_path(b"file.xml", "[Content_Types].xml")), "invalid_part_name");
    for field in ["elsewhere.xml", "../evil.xml", "d/", "[Content_Types].xml/", "x/[Content_Types].xml"] {
        assert_eq!(class(&stream_named(b"[Content_Types].xml", Some(field))), "invalid_part_name", "field {field:?}");
    }
    // Where they agree, it is the stream (in any case) and `a.xml` is the one part.
    for (raw, field) in [
        (&b"[Content_Types].xml"[..], Some("[Content_Types].xml")),
        (b"[content_types].xml", Some("[CONTENT_TYPES].XML")),
        (b"[Content_Types].xml", None),
    ] {
        let p = Package::read(&stream_named(raw, field), &Limits::default()).unwrap();
        assert_eq!(p.parts().collect::<Vec<_>>(), ["/a.xml"], "{field:?}");
    }
}

#[test]
fn a_field_that_names_another_valid_part_on_a_part_row_is_still_the_name() {
    // The one thing a field is for: the same part under its UTF-8 name, or another valid part. (A row that is neither a folder nor the
    // stream, by either name.)
    let p = Package::read(&named_with_unicode_path(b"cafe.xml", "caf\u{e9}.xml"), &Limits::default()).unwrap();
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/café.xml"]);
    let p = Package::read(&named_with_unicode_path(b"a.xml", "b/c.xml"), &Limits::default()).unwrap();
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/b/c.xml"]);
}

#[test]
fn a_name_refused_without_a_unicode_path_is_refused_with_one_whatever_the_field_says() {
    // Item 1: the field gave `/ok.xml` to each of these, which are refused as they stand. The name as written is judged as it
    // would be without the field (by the same rules, and so with the same class), whether it is a part or a folder.
    let items = |raw: &[u8], field: Option<&str>| {
        let mut item = Item::stored("x", b"<x/>").named_bytes(raw);
        if let Some(field) = field {
            item = item.with_unicode_path(field);
        }
        class_of(&build(&[base_items().remove(0), item]), &Limits::default())
    };
    let refused: [(&[u8], &str); 14] = [
        (b"%5BContent_Types%5D.xml", "invalid_part_name"), // decodes to the stream's name
        (b"%41.xml", "invalid_part_name"),                 // an escape of a letter
        (b"a//b.xml", "invalid_part_name"),                // an empty segment
        (b"a./b.xml", "invalid_part_name"),                // a segment that ends in a dot
        (b"", "invalid_part_name"),                        // nothing at all
        (b"a\x01b.xml", "invalid_part_name"),              // a control character
        (b"a%00b.xml", "invalid_part_name"),
        (b"a%FFb.xml", "invalid_part_name"), // escapes that are not UTF-8
        (b"../evil.xml", "path_traversal"),
        (b"/abs.xml", "path_traversal"),
        (b"a\\b.xml", "path_traversal"),
        (b"%2e%2e/x.xml", "path_traversal"),
        (b"%FF/../x.xml", "path_traversal"),
        (b"a%2Fb.xml", "path_traversal"),
    ];
    for (raw, class) in refused {
        let label = String::from_utf8_lossy(raw).into_owned();
        assert_eq!(items(raw, None), class, "{label:?} with no field");
        assert_eq!(items(raw, Some("ok.xml")), class, "{label:?} with a field that says ok.xml");
    }
    // The limit on a name's length, too: 1,025 bytes as written is refused whatever the field says.
    let long = vec![b'n'; 1025];
    assert_eq!(items(&long, None), "invalid_part_name");
    assert_eq!(items(&long, Some("ok.xml")), "invalid_part_name");
    // A folder is judged without its slash, as it is without the field.
    let folders: [(&[u8], &str); 5] = [
        (b"a//b/", "invalid_part_name"),
        (b"%41/", "invalid_part_name"),
        (b"a./", "invalid_part_name"),
        (b"../x/", "path_traversal"),
        (b"%5BContent_Types%5D.xml/", "invalid_part_name"),
    ];
    for (raw, class) in folders {
        let label = String::from_utf8_lossy(raw).into_owned();
        assert_eq!(items(raw, None), class, "{label:?} with no field");
        assert_eq!(items(raw, Some("ok/")), class, "{label:?} with a field that says ok/");
    }
    // And what is fine without a field is fine with one: a name as written in code page 437, a folder, the stream.
    let fine = Package::read(&named_with_unicode_path(b"caf\x82.xml", "caf\u{e9}.xml"), &Limits::default()).unwrap();
    assert_eq!(fine.parts().collect::<Vec<_>>(), ["/café.xml"]);
    // A name flagged as UTF-8 that is not is damage, field or no field.
    let mut items = base_items();
    items.push(Item::stored("x", b"1").named_bytes(b"caf\x82.xml").with_flags(0x0800).with_unicode_path("ok.xml"));
    assert_eq!(class(&build(&items)), "zip_corrupt");
}

#[test]
fn a_folder_row_that_holds_data_is_damage_and_one_that_holds_none_is_a_folder() {
    // Item 3: a flipped bit turns `logo` into `log/`, a folder, and the data of the part was dropped with a warning. A
    // folder has no data. (Whether a real writer stores a folder with bytes is for the sample-file run of T5 to say.)
    let mut items = base_items();
    items.push(Item::stored("logo", b"the bytes of a picture"));
    let mut bytes = build(&items);
    assert_eq!(Package::read(&bytes, &Limits::default()).unwrap().parts().collect::<Vec<_>>(), ["/logo"]);
    let name = bytes.windows(4).rposition(|w| w == b"logo").unwrap(); // the row's: it comes after the entry's own header
    bytes[name + 3] ^= 0x40; // `o` is 0x6F and `/` is 0x2F
    assert_eq!(class(&bytes), "zip_corrupt");
    // Whichever method and however it is said: stored, deflated, a size from a ZIP64 field.
    for item in [
        Item::stored("log/", b"x"), // one byte is data
        Item::stored("log/", b"bytes"),
        Item::deflated("log/", &[b'x'; 500]),
        Item::stored("log/", b"bytes").with_zip64_field(),
    ] {
        let mut items = base_items();
        items.push(item);
        assert_eq!(class(&build(&items)), "zip_corrupt");
    }
    // The folders past the hundredth, which are counted and not named, are judged the same.
    let mut items = base_items();
    items.extend((0..150).map(|i| Item::stored(&format!("d{i}/"), b"")));
    items.push(Item::stored("last/", b"bytes"));
    assert_eq!(class(&build(&items)), "zip_corrupt");
    // A folder with nothing in it reads as a folder, as an empty stored entry and as an empty deflate stream (two bytes, which some
    // writers give an empty entry): no data is no uncompressed bytes.
    // Or as an empty stored block of deflate (five bytes): the folder's stream is never inflated, and what says it holds no data is its
    // uncompressed size of 0, whatever it is made of.
    let mut empty_block = Item::stored("log/", b"").with_method(8);
    (empty_block.data, empty_block.csize) = (vec![1, 0, 0, 0xFF, 0xFF], 5);
    for item in [Item::stored("log/", b""), Item::deflated("log/", b""), empty_block] {
        let mut items = base_items();
        items.push(item);
        let p = Package::read(&build(&items), &Limits::default()).unwrap();
        assert_eq!(p.parts().count(), 0);
        assert_eq!(p.warnings(), [Warning::DirectoryItem { name: "log/".to_owned() }]);
    }
}

#[test]
fn m2_ruling_22b_a_dot_folder_reads_with_a_warning_and_a_dot_dot_folder_stays_a_traversal() {
    // Two real files from one source hold a folder row called `./`. R3 binds part names, a folder is no part, and `.` is no traversal.
    for folder in ["./", "././", "a/./", "./a/", "a/./b/"] {
        let mut items = with_one_part();
        items.push(Item::stored(folder, b""));
        let p = Package::read(&build(&items), &Limits::default()).unwrap_or_else(|e| panic!("{folder}: {e}"));
        assert_eq!(p.parts().collect::<Vec<_>>(), ["/visio/document.xml"], "{folder} is no part");
        assert_eq!(p.warnings(), [Warning::DirectoryItem { name: folder.to_owned() }], "{folder} is warned of");
    }
    // `..` is the one that leaves the package, whatever stands beside it; and a folder is judged by every other rule once `.` is set aside.
    for (folder, expected) in [
        ("../", "path_traversal"),
        ("./../", "path_traversal"),
        ("a/../", "path_traversal"),
        (".././", "path_traversal"),
        ("a//", "invalid_part_name"),
        ("a./", "invalid_part_name"),
        ("./a./", "invalid_part_name"),
        ("%2E/", "invalid_part_name"),
    ] {
        let mut items = with_one_part();
        items.push(Item::stored(folder, b""));
        assert_eq!(class(&build(&items)), expected, "{folder}");
    }
    // A Unicode path field cannot make the judgement different: the name as written is a `./` folder and the field gives another folder.
    let mut items = with_one_part();
    items.push(Item::stored("./", b"").with_unicode_path("other/"));
    let p = Package::read(&build(&items), &Limits::default()).unwrap();
    assert_eq!(p.warnings(), [Warning::DirectoryItem { name: "other/".to_owned() }]);
    let mut items = with_one_part();
    items.push(Item::stored("../", b"").with_unicode_path("other/"));
    assert_eq!(class(&build(&items)), "path_traversal");
}

// ----: the end records, field by field

/// An archive of `items` with the end records `ends` makes of where its directory is.
fn archive_with_ends(items: &[Item], ends: impl FnOnce(Dir) -> Ends) -> Vec<u8> {
    let (mut bytes, dir) = without_end(items);
    write_ends(&mut bytes, &ends(dir));
    bytes
}

/// A named change to the numbers of an end record.
type Change<T> = (&'static str, fn(&mut T));

fn parts(bytes: &[u8]) -> Result<Vec<String>, String> {
    Package::read(bytes, &Limits::default()).map(|p| p.parts().map(str::to_owned).collect()).map_err(|e| e.class().to_owned())
}

#[test]
fn each_field_of_the_plain_end_record_alone_can_ask_for_the_zip64_record() {
    // Real writers ask with one field: the count of an archive of more than 65,535 entries, the start of a directory past 4 GiB. Each of
    // the six, alone, with the rest real: the ZIP64 record is read, and the archive reads. (Were the field ignored, its sentinel would
    // be the number: 65,535 entries, a disk 65,535, a directory of 4 GiB.)
    let items = with_one_part();
    let asks: [Change<Ends>; 6] = [
        ("disk", |e| e.disk = 0xFFFF),
        ("directory disk", |e| e.directory_disk = 0xFFFF),
        ("entries on this disk", |e| e.on_disk = 0xFFFF),
        ("total entries", |e| e.total = 0xFFFF),
        ("size", |e| e.size = 0xFFFF_FFFF),
        ("start", |e| e.start = 0xFFFF_FFFF),
    ];
    for (field, ask) in asks {
        let bytes = archive_with_ends(&items, |dir| {
            let mut ends = Ends::plain(dir);
            ends.zip64 = Ends::wide(dir).zip64;
            ask(&mut ends);
            ends
        });
        assert_eq!(parts(&bytes), Ok(vec!["/visio/document.xml".to_owned()]), "only the {field} asks");
    }
    // And all of them at once, which is what `Ends::wide` is.
    assert_eq!(parts(&archive_with_ends(&items, Ends::wide)), Ok(vec!["/visio/document.xml".to_owned()]));
}

#[test]
fn a_value_the_plain_record_gives_for_itself_stands_over_the_zip64_one() {
    // The ZIP64 record is read for the values the plain one leaves to it, and for no other: here it is asked for by one field and
    // disagrees about the others (a directory of 9 bytes at 9, and one row where there are two), and the plain record is right.
    let items = with_one_part();
    let count_asks = archive_with_ends(&items, |dir| {
        let mut ends = Ends::plain(dir);
        ends.total = 0xFFFF;
        let mut z = Ends::wide(dir).zip64.unwrap();
        (z.size, z.start) = (9, 9);
        ends.zip64 = Some(z);
        ends
    });
    assert_eq!(parts(&count_asks), Ok(vec!["/visio/document.xml".to_owned()]), "only the count asks");
    let size_asks = archive_with_ends(&items, |dir| {
        let mut ends = Ends::plain(dir);
        ends.size = 0xFFFF_FFFF;
        let mut z = Ends::wide(dir).zip64.unwrap();
        (z.total, z.on_disk, z.start) = (1, 1, 9);
        ends.zip64 = Some(z);
        ends
    });
    assert_eq!(parts(&size_asks), Ok(vec!["/visio/document.xml".to_owned()]), "only the size asks");
    let start_asks = archive_with_ends(&items, |dir| {
        let mut ends = Ends::plain(dir);
        ends.start = 0xFFFF_FFFF;
        let mut z = Ends::wide(dir).zip64.unwrap();
        (z.total, z.on_disk, z.size) = (1, 1, 9);
        ends.zip64 = Some(z);
        ends
    });
    assert_eq!(parts(&start_asks), Ok(vec!["/visio/document.xml".to_owned()]), "only the start asks");
}

#[test]
fn a_zip64_record_that_says_the_archive_spans_disks_is_refused_whichever_field_says_so() {
    let items = with_one_part();
    let spans: [Change<Zip64>; 4] = [
        ("this disk", |z| z.disk = 1),
        ("the directory's disk", |z| z.directory_disk = 1),
        ("entries on this disk", |z| z.on_disk = z.total - 1),
        ("entries on this disk, more", |z| z.on_disk = z.total + 1),
    ];
    for (field, span) in spans {
        let bytes = archive_with_ends(&items, |dir| {
            let mut ends = Ends::wide(dir);
            span(ends.zip64.as_mut().unwrap());
            ends
        });
        assert_eq!(parts(&bytes), Err("zip_corrupt".to_owned()), "{field}");
    }
}

#[test]
fn the_disk_numbers_of_the_plain_record_and_the_locator_are_not_read_when_the_zip64_record_is() {
    // Labelled (review M3): once the ZIP64 record is the one read, its own disk fields decide whether the archive is a single disk, and
    // the plain record's disk and directory disk, and the locator's disk of the ZIP64 record and number of disks, are not looked at.
    // That is harmless: every offset is from byte 0 and is verified against the signature it should reach, so a multi-disk archive
    // that says it is one finds no record where it says one is. This pins what the reader does, so that a change is a decision.
    let items = with_one_part();
    let bytes = archive_with_ends(&items, |dir| {
        let mut ends = Ends::wide(dir);
        (ends.disk, ends.directory_disk) = (5, 5);
        let z = ends.zip64.as_mut().unwrap();
        (z.locator_disk, z.locator_disks) = (7, 9);
        ends
    });
    assert_eq!(parts(&bytes), Ok(vec!["/visio/document.xml".to_owned()]));
    // And the plain record's own say, when it asks for nothing, is still a refusal.
    let bytes = archive_with_ends(&items, |dir| {
        let mut ends = Ends::plain(dir);
        ends.disk = 5;
        ends
    });
    assert_eq!(parts(&bytes), Err("zip_corrupt".to_owned()));
}

#[test]
fn a_zip64_record_has_to_come_before_its_locator_and_the_directory_has_to_end_before_the_record() {
    let items = with_one_part();
    // [entries][directory][locator][end record][ZIP64 record, as the end record's comment]: the locator points forward, past the end
    // record. The directory is made to end where that record begins, over the locator and the end record, so that nothing else is wrong.
    let (mut bytes, dir) = without_end(&items);
    let locator_at = bytes.len();
    let record_at = (locator_at + 20 + 22) as u64;
    bytes.extend_from_slice(b"PK\x06\x07");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&record_at.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    let plain = Ends {
        disk: 0xFFFF,
        directory_disk: 0xFFFF,
        on_disk: 0xFFFF,
        total: 0xFFFF,
        size: 0xFFFF_FFFF,
        start: 0xFFFF_FFFF,
        zip64: None,
        comment: Vec::new(),
    };
    let mut end = Vec::new();
    write_ends(&mut end, &Ends { comment: vec![0; 56], ..plain });
    bytes.extend_from_slice(&end);
    let comment_at = bytes.len() - 56;
    let mut record = Vec::new();
    record.extend_from_slice(b"PK\x06\x06");
    record.extend_from_slice(&44u64.to_le_bytes());
    record.extend_from_slice(&[45, 0, 45, 0]);
    record.extend_from_slice(&[0; 8]); // disks
    for number in [dir.count, dir.count, dir.len + 20 + 22, dir.start] {
        record.extend_from_slice(&number.to_le_bytes());
    }
    bytes[comment_at..].copy_from_slice(&record);
    assert_eq!(parts(&bytes), Err("zip_corrupt".to_owned()), "a record after its locator");
    // The same file with the record where it belongs reads, and so with a directory that ends where the record begins.
    assert!(parts(&archive_with_ends(&items, Ends::wide)).is_ok());
    // A directory that goes one byte into the ZIP64 record: its size is 1 more than the space before the record.
    let one_over = archive_with_ends(&items, |dir| {
        let mut ends = Ends::wide(dir);
        ends.zip64.as_mut().unwrap().size = dir.len + 1;
        ends
    });
    assert_eq!(parts(&one_over), Err("zip_corrupt".to_owned()));
    // And one that takes in the whole record and its locator, up to the plain end record: bytes that are not a row, so that only the
    // directory's end is wrong.
    let over_all = archive_with_ends(&items, |dir| {
        let mut ends = Ends::wide(dir);
        ends.zip64.as_mut().unwrap().size = dir.len + 56 + 20;
        ends
    });
    assert_eq!(parts(&over_all), Err("zip_corrupt".to_owned()));
}

#[test]
fn a_zip64_record_that_overlaps_its_locator_is_refused_though_it_ends_before_the_end_record() {
    // [the first 36 bytes of the record][the locator, 20 bytes, which are also the record's last 20][the end record]: the record ends where
    // the end record begins, so only its overlap with the locator is wrong. (The record's total has the locator's signature for its high
    // half, so the entries on this disk are made the same; the plain record gives every number but one itself and only asks for the
    // ZIP64 record with that one.) A reader that only keeps the record before the end record reads this.
    let items = with_one_part();
    let (mut bytes, dir) = without_end(&items);
    let record_at = bytes.len() as u64;
    let total = (u64::from(u32::from_le_bytes(*b"PK\x06\x07")) << 32) | 2;
    bytes.extend_from_slice(b"PK\x06\x06");
    bytes.extend_from_slice(&44u64.to_le_bytes());
    bytes.extend_from_slice(&[45, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // the versions and the disks
    bytes.extend_from_slice(&total.to_le_bytes()); // entries on this disk
    bytes.extend_from_slice(&(total as u32).to_le_bytes()); // the low half of the total
    bytes.extend_from_slice(b"PK\x06\x07"); // from here the record's bytes are the locator's
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&record_at.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    let mut ends = Ends::plain(dir);
    ends.disk = 0xFFFF;
    write_ends(&mut bytes, &ends);
    assert_eq!(parts(&bytes), Err("zip_corrupt".to_owned()));
}

// ----: what a row's extra field may say

/// The package of the content types stream and `a.xml` (`<a/>`, stored), whose row has `extra` for its extra field, and then `patch`
/// applied to the row's bytes.
fn row_patched(extra: &[u8], patch: impl FnOnce(&mut [u8])) -> Vec<u8> {
    let mut items = base_items();
    items.push(Item::stored("a.xml", b"<a/>").with_extra(extra));
    let mut bytes = build(&items);
    let row = last_row(&bytes);
    patch(&mut bytes[row..]);
    bytes
}

fn a_xml(bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
    Package::read(bytes, &Limits::default()).map(|p| p.part("a.xml").map(<[u8]>::to_vec)).map_err(|e| e.class().to_owned())
}

#[test]
fn a_disk_number_left_to_the_zip64_field_is_read_after_the_values_that_come_before_it() {
    let disk = |n: u32| extra_record(1, &n.to_le_bytes());
    let sentinel_disk = |row: &mut [u8]| row[34..36].copy_from_slice(&[0xFF, 0xFF]);
    // The disk alone: 0 reads, another disk is refused.
    assert_eq!(a_xml(&row_patched(&disk(0), sentinel_disk)), Ok(Some(b"<a/>".to_vec())));
    assert_eq!(a_xml(&row_patched(&disk(3), sentinel_disk)), Err("zip_corrupt".to_owned()));
    // After the two sizes: the field is the uncompressed size, the compressed size and the disk, in that order.
    let sizes_then_disk =
        |n: u32| extra_record(1, &[4u64.to_le_bytes().as_slice(), 4u64.to_le_bytes().as_slice(), n.to_le_bytes().as_slice()].concat());
    let sentinels = |row: &mut [u8]| {
        row[20..28].copy_from_slice(&[0xFF; 8]);
        row[34..36].copy_from_slice(&[0xFF, 0xFF]);
    };
    assert_eq!(a_xml(&row_patched(&sizes_then_disk(0), sentinels)), Ok(Some(b"<a/>".to_vec())));
    assert_eq!(a_xml(&row_patched(&sizes_then_disk(3), sentinels)), Err("zip_corrupt".to_owned()));
    // After the sizes and the offset of the local header: the disk is the fourth.
    let mut items = base_items();
    items.push(Item::stored("a.xml", b"<a/>"));
    let plain = build(&items);
    let local = u32::from_le_bytes(plain[last_row(&plain) + 42..last_row(&plain) + 46].try_into().unwrap());
    let all = |n: u32| {
        let mut body = Vec::new();
        for value in [4u64, 4, u64::from(local)] {
            body.extend_from_slice(&value.to_le_bytes());
        }
        body.extend_from_slice(&n.to_le_bytes());
        extra_record(1, &body)
    };
    let everything = |row: &mut [u8]| {
        row[20..28].copy_from_slice(&[0xFF; 8]);
        row[34..36].copy_from_slice(&[0xFF, 0xFF]);
        row[42..46].copy_from_slice(&[0xFF; 4]);
    };
    assert_eq!(a_xml(&row_patched(&all(0), everything)), Ok(Some(b"<a/>".to_vec())));
    assert_eq!(a_xml(&row_patched(&all(3), everything)), Err("zip_corrupt".to_owned()));
    // A disk number that is the sentinel and no field to read it from is damage (and was before).
    assert_eq!(a_xml(&row_patched(&[], sentinel_disk)), Err("zip_corrupt".to_owned()));
}

#[test]
fn the_first_zip64_field_and_the_first_unicode_path_field_of_a_row_win() {
    // Two ZIP64 fields: the first is honest, the second says 99 bytes. Reading the last would make a row of 99 bytes from 4.
    let honest = extra_record(1, &[4u64.to_le_bytes(), 4u64.to_le_bytes()].concat());
    let lying = extra_record(1, &[99u64.to_le_bytes(), 99u64.to_le_bytes()].concat());
    let sizes = |row: &mut [u8]| row[20..28].copy_from_slice(&[0xFF; 8]);
    let both = [honest.clone(), lying.clone()].concat();
    assert_eq!(a_xml(&row_patched(&both, sizes)), Ok(Some(b"<a/>".to_vec())));
    // And the order is the whole difference: the lying one first is refused.
    let reversed = [lying, honest].concat();
    assert!(a_xml(&row_patched(&reversed, sizes)).is_err());
    // Two Unicode path fields, both for this name: the first gives the name.
    let mut items = base_items();
    items.push(Item::stored("raw.xml", b"1").with_unicode_path("one.xml").with_unicode_path("two.xml"));
    assert_eq!(Package::read(&build(&items), &Limits::default()).unwrap().parts().collect::<Vec<_>>(), ["/one.xml"]);
}

// ----: where the entries are

#[test]
fn rows_in_the_opposite_order_to_their_entries_read() {
    // Nothing in the format puts the rows in the order of the entries: a writer may list them any way, and the layout is found
    // by sorting the places the rows point at, so a row that points back at an earlier entry is no overlap.
    let mut items = base_items();
    items.push(Item::deflated("a.xml", b"<a>first</a>"));
    items.push(Item::stored("b.xml", b"<b>second</b>"));
    items.push(Item::deflated("c.xml", b"<c>third</c>"));
    let bytes = build_with(&items, &Opts { reverse_rows: true, ..Opts::default() });
    let p = Package::read(&bytes, &Limits::default()).unwrap();
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/c.xml", "/b.xml", "/a.xml"]);
    assert_eq!(p.part("a.xml"), Some(&b"<a>first</a>"[..]));
    assert_eq!(p.part("c.xml"), Some(&b"<c>third</c>"[..]));
}

#[test]
fn an_overlap_is_found_whatever_the_order_of_the_rows() {
    // Entry `b.xml` is an alias of `a.xml` (its row points at the same local header), and the rows are in the opposite order too.
    let mut items = base_items();
    items.push(Item::stored("a.xml", b"<a>some data</a>"));
    items.push(Item::stored("b.xml", b"<a>some data</a>").aliasing(1));
    items.push(Item::stored("c.xml", b"<c/>"));
    assert_eq!(class(&build(&items)), "zip_corrupt");
    assert_eq!(class(&build_with(&items, &Opts { reverse_rows: true, ..Opts::default() })), "zip_corrupt");
}

#[test]
fn a_folder_has_a_place_in_the_archive_like_any_other_row() {
    // A folder row is not a part, but its row points at a local header all the same, and that is checked: one that points at nothing is
    // damage, and one that points into another entry is an overlap.
    let mut items = base_items();
    items.push(Item::stored("a.xml", b"<a/>"));
    items.push(Item::stored("d/", b""));
    let mut bytes = build(&items);
    assert!(Package::read(&bytes, &Limits::default()).is_ok());
    let row = last_row(&bytes);
    bytes[row + 42..row + 46].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(class(&bytes), "zip_corrupt", "a folder whose local header is not where its row says");
    let mut items = base_items();
    items.push(Item::stored("a.xml", b"<a>data</a>"));
    items.push(Item::stored("d/", b"").aliasing(1));
    assert_eq!(class(&build(&items)), "zip_corrupt", "a folder that points into an entry");
}

#[test]
fn an_entry_that_reaches_into_the_header_of_the_next_is_an_overlap_and_that_is_judged_from_the_header() {
    // `a.xml` is deflated, its row says it is 5 bytes longer than its stream, and `b.xml` follows at once. The extra 5 bytes are the first
    // of `b.xml`'s local header (30 bytes and its name): the stream ends where it should, the size and the checksum are right, and only
    // the layout says it is wrong. It is judged from the start of each local header, and by the lengths of the entry's own header.
    let content = b"<a>an entry</a>".repeat(10);
    let honest = Item::deflated("a.xml", &content);
    let longer = honest.csize + 5;
    let next = Item::stored("b.xml", b"<b/>");
    let read = |a: Item| {
        let mut items = base_items();
        items.push(a);
        items.push(next.clone());
        Package::read(&build(&items), &Limits::default()).map(|p| p.parts().count()).map_err(|e| e.class().to_owned())
    };
    assert_eq!(read(honest.clone()), Ok(2));
    assert_eq!(read(honest.clone().claiming_csize(longer)), Err("zip_corrupt".to_owned()));
    // The same behind a local header with a long name and a long extra field of its own: the data starts after them.
    let long_local = Local { name: Some(vec![b'n'; 100]), extra: vec![0; 40], ..Local::default() };
    assert_eq!(read(honest.clone().with_local(long_local.clone())), Ok(2));
    assert_eq!(read(honest.claiming_csize(longer).with_local(long_local)), Err("zip_corrupt".to_owned()));
}

// ---- and M2: orphans

#[test]
fn a_relationships_part_finds_its_source_whatever_the_case() {
    // The index is by folded name: a source with capital letters is found by its folded name. (Looked up as written, it was not, and the
    // package said a part with a capital was missing when it was there.)
    let mut items = base_items();
    items.push(Item::stored("Visio/Document.xml", b"<d/>"));
    items.push(Item::stored("Visio/_rels/Document.xml.rels", b"<Relationships/>"));
    let p = Package::read(&build(&items), &Limits::default()).unwrap();
    assert!(p.warnings().is_empty(), "{:?}", p.warnings());
    // A part that is missing is still said to be.
    let mut items = base_items();
    items.push(Item::stored("Visio/_rels/Other.xml.rels", b"<Relationships/>"));
    let p = Package::read(&build(&items), &Limits::default()).unwrap();
    assert_eq!(p.warnings(), [Warning::OrphanRelationships { part: "/Visio/_rels/Other.xml.rels".to_owned() }]);
}

#[test]
fn orphan_relationships_are_named_for_the_first_hundred_and_counted_after_that() {
    // Review M2: fifty thousand relationships parts with no source made fifty thousand warnings and 51 MB of names, outside the
    // documented bound. As with folders, the first hundred are named and the rest counted in one.
    let mut items = base_items();
    items.extend((0..250).map(|i| Item::stored(&format!("o{i}/_rels/x.xml.rels"), b"<Relationships/>")));
    let p = Package::read(&build(&items), &Limits::default()).unwrap();
    let named = p.warnings().iter().filter(|w| matches!(w, Warning::OrphanRelationships { .. })).count();
    assert_eq!(named, 100);
    assert!(p.warnings().contains(&Warning::MoreOrphanRelationships { count: 150 }), "{:?}", p.warnings().last());
    assert_eq!(p.warnings().len(), 101);
    // At the hundred exactly there is nothing to count.
    let mut items = base_items();
    items.extend((0..100).map(|i| Item::stored(&format!("o{i}/_rels/x.xml.rels"), b"<Relationships/>")));
    let p = Package::read(&build(&items), &Limits::default()).unwrap();
    assert_eq!(p.warnings().len(), 100);
}

// ----: the duplicate and prefix rules hold over the names as written as well as the names the fields give

/// A package of the content types stream and one row for each of `rows`: the name as written and, if there is one, the Unicode path field.
fn rows_named(rows: &[(&str, Option<&str>)]) -> Vec<u8> {
    let mut items = base_items();
    for (i, (written, field)) in rows.iter().enumerate() {
        let mut item = Item::stored("x", format!("<row{i}/>").as_bytes()).named_bytes(written.as_bytes());
        if let Some(field) = field {
            item = item.with_unicode_path(field);
        }
        items.push(item);
    }
    build(&items)
}

fn class_of_rows(rows: &[(&str, Option<&str>)]) -> Result<Vec<String>, &'static str> {
    parts(&rows_named(rows)).map_err(|e| Box::leak(e.into_boxed_str()) as &'static str)
}

#[test]
fn two_rows_with_one_name_as_written_are_duplicates_whatever_their_fields_say() {
    // Both are `a.xml` to a reader that ignores the fields, and this reader read `/b.xml` and `/c.xml`.
    assert_eq!(class_of_rows(&[("a.xml", Some("b.xml")), ("a.xml", Some("c.xml"))]), Err("duplicate_part"));
    // Whichever way round, in any case, and with the name as written spelled in escapes (a reader that ignores the field decodes them).
    assert_eq!(class_of_rows(&[("A.xml", Some("b.xml")), ("a.XML", Some("c.xml"))]), Err("duplicate_part"));
    assert_eq!(class_of_rows(&[("caf%C3%A9.xml", Some("x.xml")), ("caf\u{e9}.xml", Some("y.xml"))]), Err("duplicate_part"));
    // One of them with no field is the same: its name is its name as written.
    assert_eq!(class_of_rows(&[("a.xml", Some("b.xml")), ("a.xml", None)]), Err("duplicate_part"));
    assert_eq!(class_of_rows(&[("a.xml", None), ("a.xml", Some("b.xml"))]), Err("duplicate_part"));
    // Distinct names as written read, and the fields name them.
    assert_eq!(class_of_rows(&[("a.xml", Some("b.xml")), ("c.xml", Some("d.xml"))]), Ok(vec!["/b.xml".to_owned(), "/d.xml".to_owned()]));
}

#[test]
fn a_name_as_written_is_checked_against_the_field_names_of_the_other_rows_too() {
    // `/b.xml` is row one's to a reader that reads the fields and row two's to one that does not: refused in either order, and with the
    // second row's own field naming something else (so that neither reading, taken alone, has two of one name).
    assert_eq!(class_of_rows(&[("b.xml", None), ("a.xml", Some("b.xml"))]), Err("duplicate_part"));
    assert_eq!(class_of_rows(&[("a.xml", Some("b.xml")), ("b.xml", None)]), Err("duplicate_part"));
    assert_eq!(class_of_rows(&[("a.xml", Some("b.xml")), ("b.xml", Some("c.xml"))]), Err("duplicate_part"));
    assert_eq!(class_of_rows(&[("b.xml", Some("c.xml")), ("a.xml", Some("b.xml"))]), Err("duplicate_part"));
    // A chain of three in which no two rows share a name as written or a name from a field, and no name stands for two rows.
    assert_eq!(
        class_of_rows(&[("a.xml", Some("b.xml")), ("c.xml", Some("d.xml")), ("e.xml", None)]),
        Ok(vec!["/b.xml".to_owned(), "/d.xml".to_owned(), "/e.xml".to_owned()])
    );
}

#[test]
fn a_name_that_is_a_path_prefix_of_another_is_refused_over_the_names_as_written_and_across_them() {
    // Over the names as written: `/a` and `/a/b` to a reader that ignores the fields.
    assert_eq!(class_of_rows(&[("a", Some("x.xml")), ("a/b", Some("y.xml"))]), Err("part_name_prefix"));
    assert_eq!(class_of_rows(&[("a/b", Some("y.xml")), ("a", Some("x.xml"))]), Err("part_name_prefix"));
    // Across: row one's name as written is a prefix of row two's field name.
    assert_eq!(class_of_rows(&[("a.xml", Some("p.xml")), ("q.xml", Some("a.xml/z.xml"))]), Err("part_name_prefix"));
    assert_eq!(class_of_rows(&[("q.xml", Some("a.xml/z.xml")), ("a.xml", Some("p.xml"))]), Err("part_name_prefix"));
    // The names a row has are not in each other's way: it is one entry, and each reading has one name for it.
    assert_eq!(class_of_rows(&[("a.xml", Some("a.xml/b.xml"))]), Ok(vec!["/a.xml/b.xml".to_owned()]));
    assert_eq!(class_of_rows(&[("a.xml", Some("A.xml"))]), Ok(vec!["/A.xml".to_owned()]));
    // A name that has a row's name under it, past the row's own other name under it: `a`, `a/x` are one row's and `a/y` another's.
    assert_eq!(class_of_rows(&[("a", Some("a/x")), ("a/y", None)]), Err("part_name_prefix"));
    assert_eq!(class_of_rows(&[("a/y", None), ("a", Some("a/x"))]), Err("part_name_prefix"));
    // Names that merely share a start are not prefixes.
    assert_eq!(class_of_rows(&[("a.xml", Some("p.xml")), ("a.xml2", Some("q.xml"))]), Ok(vec!["/p.xml".to_owned(), "/q.xml".to_owned()]));
}
