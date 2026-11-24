//! The reader: the plain judge's rules for names, the archive's own rules whatever a judge says,
//! and the contract between the two that the `opc` crate's reading rests on (which name an error shows,
//! what a dropped row still counts for, when the whole list is judged).

use diacraft_container::zip::{Archive, Entry, Judge, Method, Verdict};
use diacraft_container::{ContainerError, Limits, Work};

use crate::zipbuild::{Item, build, extra_record};
use crate::{class, class_with, read_all};

#[test]
fn plain_names_are_compared_exactly() {
    // Two entries that differ in case are two entries; two of one name are `duplicate_part`; `../x.xml` is `path_traversal`.
    let both = build(&[Item::stored("Object 1/content.xml", b"a"), Item::stored("object 1/content.xml", b"b")]);
    assert_eq!(read_all(&both).unwrap().len(), 2);
    let twice = build(&[Item::stored("Pictures/a.png", b"a"), Item::stored("Pictures/a.png", b"b")]);
    assert_eq!(class(&twice), "duplicate_part");
    match Archive::read(&twice, &Limits::default()) {
        Err(ContainerError::DuplicatePart { first, second }) => assert_eq!((first.as_str(), second.as_str()), ("Pictures/a.png", "Pictures/a.png")),
        other => panic!("{other:?}"),
    }
    assert_eq!(class(&build(&[Item::stored("../x.xml", b"a")])), "path_traversal");
}

#[test]
fn names_that_would_leave_the_package_and_empty_names_are_refused() {
    for name in ["/x.xml", "a\\b.xml", "C:x.xml", "a/../b.xml", "../", "a/../", "..", "/"] {
        assert_eq!(class(&build(&[Item::stored(name, b"")])), "path_traversal", "{name:?}");
    }
    assert_eq!(class(&build(&[Item::stored("", b"")])), "invalid_part_name");
    // A dot segment and a name that only looks like a traversal are names like any other: ODF names are not percent-decoded.
    assert_eq!(read_all(&build(&[Item::stored("./a.xml", b""), Item::stored("%2e%2e/b.xml", b""), Item::stored("a..b", b"")])).unwrap().len(), 3);
    // The name limit, in bytes: at it reads, past it is refused, and three times past it is refused before it is copied (`cost.rs`).
    let limits = Limits { max_part_name_bytes: 10, ..Limits::default() };
    assert_eq!(class_with(&build(&[Item::stored("0123456789", b"")]), &limits), "read");
    assert_eq!(class_with(&build(&[Item::stored("0123456789/", b"")]), &limits), "read", "a folder's slash is not counted");
    assert_eq!(class_with(&build(&[Item::stored("0123456789a", b"")]), &limits), "invalid_part_name");
}

#[test]
fn a_unicode_path_field_gives_the_name_and_cannot_launder_one() {
    // The field (Info-ZIP's `up`, with the checksum of the name as written) gives the name: here a code page 437 name as written.
    let renamed = build(&[Item::stored("", b"x").named_bytes(b"caf\x82.xml").with_unicode_path("café.xml")]);
    assert_eq!(read_all(&renamed).unwrap()[0].0, "café.xml");
    // The name as written is the judge's to see, and is not kept with the entry.
    let other = build(&[Item::stored("a.xml", b"x").with_unicode_path("b.xml")]);
    let archive = Archive::read(&other, &Limits::default()).unwrap();
    assert_eq!((archive.entries()[0].name.as_str(), archive.entries()[0].written.as_deref()), ("b.xml", None));
    // Without the field the same bytes are code page 437, where 0x82 is `é` too.
    assert_eq!(read_all(&build(&[Item::stored("", b"x").named_bytes(b"caf\x82.xml")])).unwrap()[0].0, "café.xml");
    // The name as written is held to the same rules: a field cannot make `../evil.xml` into `ok.xml`.
    assert_eq!(class(&build(&[Item::stored("../evil.xml", b"x").with_unicode_path("ok.xml")])), "path_traversal");
    // Nor a file into a folder, which a reader would drop, nor a folder into a file.
    assert_eq!(class(&build(&[Item::stored("a.xml", b"").with_unicode_path("a/")])), "invalid_part_name");
    assert_eq!(class(&build(&[Item::stored("a/", b"").with_unicode_path("a.xml")])), "invalid_part_name");
    // Two rows that are one name as written are two of one name to a reader that ignores the fields.
    let twice = build(&[Item::stored("a.xml", b"1").with_unicode_path("b.xml"), Item::stored("a.xml", b"2").with_unicode_path("c.xml")]);
    assert_eq!(class(&twice), "duplicate_part");
    // And a name as written that is another row's name: which row `b.xml` is would depend on whether the field is read.
    let crossed = build(&[Item::stored("b.xml", b"1"), Item::stored("b.xml", b"2").with_unicode_path("c.xml")]);
    assert_eq!(class(&crossed), "duplicate_part");
    // A row's own two names are not in each other's way.
    assert_eq!(read_all(&build(&[Item::stored("a.xml", b"1").with_unicode_path("a.xml")])).unwrap().len(), 1);
    // A field with another checksum is for another name and does not count.
    let mut stale = Item::stored("a.xml", b"1");
    let mut body = vec![1u8];
    body.extend_from_slice(&crc32fast::hash(b"not a.xml").to_le_bytes());
    body.extend_from_slice(b"b.xml");
    stale = stale.with_extra(&extra_record(0x7075, &body));
    assert_eq!(read_all(&build(&[stale])).unwrap()[0].0, "a.xml");
}

#[test]
fn a_row_of_65535_bytes_reads_and_one_byte_more_is_damage() {
    // A central directory row, name, extra field and comment included, is at most 65,535 bytes. 46 fixed bytes, a name of one byte, and
    // 65,488 bytes of empty extra records (16,372 of them) are exactly that.
    let field: Vec<u8> = (0..16_372).flat_map(|_| extra_record(0xCAFE, &[])).collect();
    assert_eq!(46 + 1 + field.len(), 65_535);
    let at_the_limit = build(&[Item::stored("a", b"x").with_extra(&field)]);
    assert_eq!(read_all(&at_the_limit).unwrap(), [("a".to_owned(), b"x".to_vec())]);
    let mut longer = field.clone();
    longer.push(0);
    assert_eq!(class(&build(&[Item::stored("a", b"x").with_extra(&longer)])), "zip_corrupt");
}

#[test]
fn a_name_flagged_utf8_must_be_utf8() {
    let bad = build(&[Item::stored("", b"").named_bytes(b"caf\xe9.xml").with_flags(0x0800)]);
    assert_eq!(class(&bad), "zip_corrupt");
}

#[test]
fn a_folder_has_no_data() {
    // A flipped bit turns `logo` into `log/`: its data would be dropped with the folder.
    assert_eq!(class(&build(&[Item::stored("log/", b"data")])), "zip_corrupt");
    // An empty deflate stream is no data.
    let folder = build(&[Item::deflated("Configurations2/", b"")]);
    let archive = Archive::read(&folder, &Limits::default()).unwrap();
    assert!(archive.entries()[0].folder);
    // A folder's method is not judged: it has no data to inflate.
    assert_eq!(class(&build(&[Item::stored("d/", b"").with_method(99)])), "read");
}

#[test]
fn a_folder_inflates_to_nothing_whatever_its_method_and_its_bytes() {
    // A folder of size 0 under a method nobody supports, with three bytes behind its local header: kept by the plain judge, and its bytes
    // are none. Nothing of it is inflated, and its method is never asked about.
    let folder = build(&[Item::stored("d/", b"abc").claiming_size(0).with_method(99), Item::stored("a.xml", b"<a/>")]);
    let mut archive = Archive::read(&folder, &Limits::default()).unwrap();
    let entry = &archive.entries()[0];
    assert_eq!((entry.name.as_str(), entry.folder, entry.method, entry.compressed, entry.size), ("d/", true, 99, 3, 0));
    let mut work = Work::default();
    assert_eq!(archive.inflate(0, &mut work).unwrap(), Vec::<u8>::new());
    assert_eq!(work.inflated, 0);
    assert_eq!(archive.inflate(1, &mut work).unwrap(), b"<a/>");
}

#[test]
fn encryption_and_methods_but_stored_and_deflate_are_refused() {
    for bit in [0x0001, 0x0040, 0x2000] {
        assert_eq!(class(&build(&[Item::stored("a.xml", b"x").with_flags(bit)])), "encrypted_package", "flag {bit:#06x}");
    }
    for method in [1u16, 9, 12, 14, 93, 99] {
        let error = Archive::read(&build(&[Item::stored("a.xml", b"x").with_method(method)]), &Limits::default()).unwrap_err();
        assert_eq!(error, ContainerError::UnsupportedCompression { name: "a.xml".into(), method: method.to_string() });
    }
    assert_eq!(Method::from_number(0), Some(Method::Stored));
    assert_eq!(Method::from_number(8), Some(Method::Deflated));
    assert_eq!(Method::from_number(9), None);
    assert_eq!((Method::Stored.number(), Method::Deflated.number()), (0, 8));
}

#[test]
fn the_limits_hold_by_the_headers_and_by_the_bytes() {
    let limits = Limits { max_parts: 3, max_package_bytes: 100, max_part_bytes: 60, max_ratio: 10, ..Limits::default() };
    // The ratio by the header: 11 bytes claimed for each byte stored.
    assert_eq!(class_with(&build(&[Item::stored("a", b"x").claiming_size(11)]), &limits), "zip_ratio_exceeded");
    // The part cap by the header, and at it.
    assert_eq!(class_with(&build(&[Item::stored("a", &[b'a'; 61])]), &limits), "part_too_large");
    assert_eq!(class_with(&build(&[Item::stored("a", &[b'a'; 60])]), &limits), "read");
    // The package total by the headers: 60 and 41.
    assert_eq!(class_with(&build(&[Item::stored("a", &[b'a'; 60]), Item::stored("b", &[b'b'; 41])]), &limits), "package_too_large");
    // The part count: folders are not parts.
    let four = build(&[Item::stored("a", b""), Item::stored("b", b""), Item::stored("c", b""), Item::stored("d", b"")]);
    assert_eq!(class_with(&four, &limits), "too_many_parts");
    let folders = build(&[Item::stored("a", b""), Item::stored("b", b""), Item::stored("c", b""), Item::stored("d/", b""), Item::stored("e/", b"")]);
    assert_eq!(class_with(&folders, &limits), "read");
    // By the bytes, when the header lies low: an entry that says 10 bytes and holds 61 is stopped at the cap, from what came out (the
    // ratio of the default limits is not the nearer limit here).
    let roomy = Limits { max_part_bytes: 60, ..Limits::default() };
    let lying = build(&[Item::deflated("a", &[b'a'; 61]).claiming_size(10)]);
    let mut archive = Archive::read(&lying, &roomy).unwrap();
    assert_eq!(archive.inflate(0, &mut Work::default()).unwrap_err().class(), "part_too_large");
    // With the ratio the nearer limit, the same bytes are refused for the ratio.
    let mut archive = Archive::read(&lying, &Limits { max_ratio: 3, ..roomy.clone() }).unwrap();
    assert_eq!(archive.inflate(0, &mut Work::default()).unwrap_err().class(), "zip_ratio_exceeded");
    // And one that holds another size than it says, within the limits, is damage.
    let short = build(&[Item::deflated("a", &[b'a'; 20]).claiming_size(30)]);
    let mut archive = Archive::read(&short, &roomy).unwrap();
    assert_eq!(archive.inflate(0, &mut Work::default()).unwrap_err().class(), "zip_corrupt");
    // A wrong checksum is damage.
    let mut bad = Item::stored("a", b"abc");
    bad.crc ^= 1;
    let bad = build(&[bad]);
    let mut archive = Archive::read(&bad, &roomy).unwrap();
    assert_eq!(archive.inflate(0, &mut Work::default()).unwrap_err().class(), "zip_corrupt");
}

#[test]
fn an_entry_inflated_twice_is_counted_once_and_an_index_past_the_entries_is_damage() {
    let limits = Limits { max_package_bytes: 100, ..Limits::default() };
    let bytes = build(&[Item::stored("a", &[b'a'; 60]), Item::stored("b", &[b'b'; 40])]);
    let mut archive = Archive::read(&bytes, &limits).unwrap();
    let mut work = Work::default();
    for _ in 0..3 {
        assert_eq!(archive.inflate(0, &mut work).unwrap().len(), 60);
    }
    assert_eq!(archive.inflate(1, &mut work).unwrap().len(), 40, "the first entry's three reads are 60 bytes of the total, not 180");
    assert_eq!(work.inflated, 220, "though each read is work");
    assert_eq!(archive.inflate(2, &mut work).unwrap_err().class(), "zip_corrupt");
}

/// A judge with rules of its own, as the `opc` crate's is: folders dropped, names given in another form, a smaller cap for one entry, one
/// entry that is not counted, and a rule over the whole list.
#[derive(Default)]
struct Renaming {
    seen: Vec<String>,
    listed: usize,
}

impl Judge for Renaming {
    type Error = ContainerError;

    fn entry(&mut self, entry: &Entry, kept: &[Entry]) -> Result<Verdict, ContainerError> {
        // What the judge is shown: the name as the row gives it, nothing of the layout yet, and the entries kept so far.
        assert_eq!((entry.data_offset, entry.local_extra_len, entry.cap, entry.counted), (0, 0, 0, false));
        assert_eq!(kept.len(), self.seen.iter().filter(|n| !n.ends_with('/')).count());
        self.seen.push(entry.name.clone());
        if entry.folder {
            return Ok(Verdict::Drop);
        }
        let cap = if entry.name == "small.bin" { 4 } else { 1000 };
        Ok(Verdict::Keep { rename: Some(format!("/{}", entry.name)), cap, counted: entry.name != "stream.xml" })
    }

    fn listed(&mut self, kept: &[Entry], _work: &mut Work) -> Result<(), ContainerError> {
        self.listed = kept.len();
        if kept.iter().any(|e| e.name == "/refuse.me") {
            return Err(ContainerError::InvalidPartName { name: "/refuse.me".into(), reason: "the list said no" });
        }
        Ok(())
    }
}

fn with_judge(bytes: &[u8], limits: &Limits) -> (Result<Vec<Entry>, ContainerError>, Renaming) {
    let mut judge = Renaming::default();
    let result = Archive::read_with(bytes, limits, &mut judge, &mut Work::default()).map(Archive::into_entries);
    (result, judge)
}

#[test]
fn a_judge_keeps_renames_caps_and_drops_and_the_archive_still_holds_its_own_rules() {
    let bytes =
        build(&[Item::stored("stream.xml", b"<s/>"), Item::stored("dir/", b""), Item::stored("small.bin", b"1234"), Item::stored("p.xml", b"<p/>")]);
    let (entries, judge) = with_judge(&bytes, &Limits { max_parts: 2, ..Limits::default() });
    let entries = entries.unwrap();
    assert_eq!(judge.seen, ["stream.xml", "dir/", "small.bin", "p.xml"]);
    assert_eq!(judge.listed, 3, "the whole list, after the last row");
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["/stream.xml", "/small.bin", "/p.xml"], "kept under the names the judge gave, folders dropped");
    assert_eq!(entries.iter().map(|e| (e.cap, e.counted)).collect::<Vec<_>>(), [(1000, false), (4, true), (1000, true)]);
    assert!(entries.iter().all(|e| e.written.is_none() && e.data_offset > 0));
    // Two parts with a limit of two: the entry that is not counted is not a part.
    let (result, _) = with_judge(&bytes, &Limits { max_parts: 1, ..Limits::default() });
    assert_eq!(result.unwrap_err().class(), "too_many_parts");
    // The archive's own refusals of a row name it as the row does, not as the judge renamed it.
    let encrypted = build(&[Item::stored("p.xml", b"x").with_flags(1)]);
    assert_eq!(with_judge(&encrypted, &Limits::default()).0.unwrap_err(), ContainerError::EncryptedPackage { name: "p.xml".into() });
    let capped = build(&[Item::stored("small.bin", b"12345")]);
    assert_eq!(with_judge(&capped, &Limits::default()).0.unwrap_err(), ContainerError::PartTooLarge { name: "small.bin".into(), limit: 4 });
    // What is refused from the bytes names the entry as it was kept.
    let lying = build(&[Item::stored("small.bin", b"12345").claiming_size(4)]);
    let mut judge = Renaming::default();
    let mut archive = Archive::read_with(&lying, &Limits::default(), &mut judge, &mut Work::default()).unwrap();
    assert_eq!(archive.inflate(0, &mut Work::default()).unwrap_err(), ContainerError::PartTooLarge { name: "/small.bin".into(), limit: 4 });
    // A rule over the whole list refuses before the layout is read: the overlap of the folder's row with the file's is not reached.
    let refused = overlapping("refuse.me");
    assert_eq!(with_judge(&refused, &Limits::default()).0.unwrap_err().class(), "invalid_part_name");
    assert_eq!(with_judge(&overlapping("a.xml"), &Limits::default()).0.unwrap_err().class(), "zip_corrupt");
}

#[test]
fn an_entry_the_judge_does_not_count_is_held_to_its_cap_and_not_to_the_package_total() {
    // Counted parts of 60 and 40 bytes fill a package total of 100, by their headers and by their bytes; the stream after them, not
    // counted, still reads, within its own cap of 1,000.
    let limits = Limits { max_package_bytes: 100, ..Limits::default() };
    let bytes = build(&[Item::stored("a.bin", &[b'a'; 60]), Item::stored("b.bin", &[b'b'; 40]), Item::stored("stream.xml", &[b's'; 50])]);
    let mut judge = Renaming::default();
    let mut archive = Archive::read_with(&bytes, &limits, &mut judge, &mut Work::default()).unwrap();
    assert_eq!(archive.entries().iter().map(|e| e.counted).collect::<Vec<_>>(), [true, true, false]);
    let mut work = Work::default();
    let sizes: Vec<usize> = (0..3).map(|i| archive.inflate(i, &mut work).unwrap().len()).collect();
    assert_eq!(sizes, [60, 40, 50]);
    // One more counted byte is past the total, by the header.
    let over = build(&[Item::stored("a.bin", &[b'a'; 60]), Item::stored("b.bin", &[b'b'; 41]), Item::stored("stream.xml", &[b's'; 50])]);
    assert_eq!(with_judge(&over, &limits).0.unwrap_err().class(), "package_too_large");
    // And the uncounted entry's own cap holds: past it, by the bytes, it is `part_too_large` and not the total's refusal.
    let big = build(&[Item::stored("a.bin", &[b'a'; 60]), Item::stored("stream.xml", &[b's'; 1001]).claiming_size(1000)]);
    let mut archive = Archive::read_with(&big, &limits, &mut Renaming::default(), &mut Work::default()).unwrap();
    assert_eq!(archive.inflate(0, &mut Work::default()).unwrap().len(), 60);
    assert_eq!(archive.inflate(1, &mut Work::default()).unwrap_err(), ContainerError::PartTooLarge { name: "/stream.xml".into(), limit: 1000 });
}

/// A file `name` and a folder `d/` whose row points at the file's local header: two rows that share bytes.
fn overlapping(name: &str) -> Vec<u8> {
    let mut bytes = build(&[Item::stored(name, b"<a/>"), Item::stored("d/", b"")]);
    let end = bytes.len() - 22;
    let start = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
    let second_row = start + 46 + name.len();
    bytes[second_row + 42..second_row + 46].copy_from_slice(&0u32.to_le_bytes());
    bytes
}

#[test]
fn a_dropped_row_is_still_part_of_the_layout() {
    // The folder's row points at the local header of the file: two rows share bytes, whatever the judge made of either (this judge drops
    // folders, and the plain one keeps them).
    let bytes = overlapping("a.xml");
    assert_eq!(with_judge(&bytes, &Limits::default()).0.unwrap_err().class(), "zip_corrupt");
    assert_eq!(class(&bytes), "zip_corrupt");
}

#[test]
fn an_entry_whose_row_says_it_is_empty_and_has_bytes_is_damage() {
    // Size 0 and checksum 0, which an empty entry has, and four bytes of data: the bytes are there, so they are read, and they are not
    // what the row says. Only an entry with no bytes at all is empty without being read.
    let mut lying = Item::stored("a.xml", b"data").claiming_size(0);
    lying.crc = 0;
    let bytes = build(&[lying]);
    let mut archive = Archive::read(&bytes, &Limits::default()).unwrap();
    assert_eq!(archive.inflate(0, &mut Work::default()).unwrap_err().class(), "zip_corrupt");
    // Its honest neighbours: no bytes, an empty deflate stream, and a row that names deflate and has no stream at all (some writers give an
    // empty entry none).
    let bytes =
        build(&[Item::stored("empty.xml", b""), Item::deflated("empty-deflated.xml", b""), Item::stored("no-stream.xml", b"").with_method(8)]);
    let all = read_all(&bytes).unwrap();
    assert_eq!(
        all.iter().map(|(n, d)| (n.as_str(), d.len())).collect::<Vec<_>>(),
        [("empty.xml", 0), ("empty-deflated.xml", 0), ("no-stream.xml", 0)]
    );
}

#[test]
fn an_archive_shows_its_entries_and_not_the_bytes_of_the_file() {
    let bytes = build(&[Item::stored("secret.bin", b"A-SECRET-THE-LOG-MUST-NOT-HOLD")]);
    let archive = Archive::read(&bytes, &Limits::default()).unwrap();
    let shown = format!("{archive:?}");
    assert!(shown.starts_with("Archive {") && shown.contains("secret.bin") && shown.contains("rows: 1"), "{shown}");
    assert!(!shown.contains("A-SECRET") && !shown.contains("65, 45, 83"), "{shown}");
}

/// A judge that keeps folders under another name.
struct FolderKeeper;

impl Judge for FolderKeeper {
    type Error = ContainerError;

    fn entry(&mut self, entry: &Entry, _kept: &[Entry]) -> Result<Verdict, ContainerError> {
        Ok(Verdict::Keep { rename: Some(format!("/{}", entry.name)), cap: 100, counted: !entry.folder })
    }
}

#[test]
fn a_folder_kept_is_kept_under_its_new_name_and_without_its_name_as_written() {
    let bytes = build(&[Item::stored("d/", b""), Item::stored("e.xml", b"<e/>")]);
    let entries = Archive::read_with(&bytes, &Limits::default(), &mut FolderKeeper, &mut Work::default()).unwrap().into_entries();
    assert_eq!(entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["/d/", "/e.xml"]);
    assert!(entries[0].folder && entries[0].cap == 0 && !entries[0].counted, "a folder takes no cap and is not counted: {:?}", entries[0]);
    // A folder whose Unicode path field gives it another name: kept under that name, and the name as written is not kept.
    let bytes = build(&[Item::stored("d/", b"").with_unicode_path("dossier/")]);
    let archive = Archive::read(&bytes, &Limits::default()).unwrap();
    let folder = &archive.entries()[0];
    assert_eq!((folder.name.as_str(), folder.written.as_deref(), folder.folder), ("dossier/", None, true));
}

#[test]
fn an_entry_is_found_by_its_exact_name() {
    let bytes = build(&[Item::stored("mimetype", b"m"), Item::stored("Content.xml", b"c"), Item::stored("content.xml", b"d")]);
    let archive = Archive::read(&bytes, &Limits::default()).unwrap();
    assert_eq!(archive.position("mimetype"), Some(0));
    assert_eq!(archive.position("content.xml"), Some(2));
    assert_eq!(archive.position("Content.xml"), Some(1));
    assert_eq!(archive.position("CONTENT.XML"), None);
    assert_eq!(archive.position("styles.xml"), None);
}

#[test]
fn bytes_after_the_end_record_are_counted_and_not_read() {
    let mut bytes = build(&[Item::stored("a.xml", b"<a/>")]);
    bytes.extend_from_slice(b"26 bytes of trailing text!");
    let archive = Archive::read(&bytes, &Limits::default()).unwrap();
    assert_eq!(archive.bytes_after_end_record(), 26);
    assert_eq!(archive.rows(), 1);
    // A file that is not a zip, and one that starts like one and has no end record.
    assert_eq!(class(b"plain text, and no end record"), "not_zip");
    assert_eq!(class(b"PK\x03\x04 and then nothing"), "zip_corrupt");
}

// ------------------------------------------------------------------------------------------------------------------------ `inflate_into`

/// A kilobyte of text that deflates.
fn text(len: usize) -> Vec<u8> {
    (0..len).map(|i| b"<cell N=\"PinX\" V=\"1.25\"/>\n"[i % 26]).collect()
}

#[test]
fn inflate_into_gives_the_bytes_inflate_gives_into_a_buffer_of_the_entrys_size() {
    // The bytes of an entry are made once, in the buffer the caller sized from the size the archive states: an entry stored or deflated, empty or not, comes out as it does by
    // `inflate`, and the cost and the total are counted alike.
    let data = text(5_000);
    let zip = build(&[Item::stored("a.bin", &data), Item::deflated("b.xml", &data), Item::stored("empty", b""), Item::deflated("empty2", b"")]);
    let mut by_vec = Archive::read(&zip, &Limits::default()).unwrap();
    let mut by_buffer = Archive::read(&zip, &Limits::default()).unwrap();
    let (mut a, mut b) = (Work::default(), Work::default());
    for index in 0..4 {
        let want = by_vec.inflate(index, &mut a).unwrap();
        let size = by_buffer.entries()[index].size as usize;
        let mut buffer = vec![0xAAu8; size];
        by_buffer.inflate_into(index, &mut buffer, &mut b).unwrap();
        assert_eq!(buffer, want, "entry {index}");
    }
    assert_eq!(a.inflated, b.inflated);
}

#[test]
fn inflate_into_refuses_a_buffer_that_is_not_the_size_the_entry_states_and_what_inflate_refuses() {
    let data = text(5_000);
    let zip = build(&[Item::deflated("a.xml", &data), Item::deflated("lie.xml", &data).claiming_size(4_000), Item::stored("crc.bin", &data)]);
    let mut archive = Archive::read(&zip, &Limits::default()).unwrap();
    let mut work = Work::default();
    // A buffer of another size: damage, and nothing of it is trusted.
    for len in [0usize, 4_999, 5_001, 10_000] {
        let mut buffer = vec![0u8; len];
        assert_eq!(archive.inflate_into(0, &mut buffer, &mut work).unwrap_err().class(), "zip_corrupt", "{len}");
    }
    // An entry that inflates to more than it states, or to less: damage, as by `inflate`.
    let mut buffer = vec![0u8; 4_000];
    assert_eq!(archive.inflate_into(1, &mut buffer, &mut work).unwrap_err().class(), "zip_corrupt");
    // An index past the entries.
    assert_eq!(archive.inflate_into(9, &mut [], &mut work).unwrap_err().class(), "zip_corrupt");
    // A checksum that is not the entry's own.
    let mut bad = Item::stored("crc.bin", &data);
    bad.crc ^= 1;
    let zip = build(&[bad]);
    let mut archive = Archive::read(&zip, &Limits::default()).unwrap();
    let mut buffer = vec![0u8; 5_000];
    let error = archive.inflate_into(0, &mut buffer, &mut work).unwrap_err();
    assert_eq!(error.class(), "zip_corrupt");
    assert!(error.to_string().contains("checksum"), "{error}");
    // Counted once however often an entry is read, as `inflate` counts it: two entries of 60,000 bytes in a package of 130,000, each read twice.
    let zip = build(&[Item::deflated("a.xml", &text(60_000)), Item::deflated("b.xml", &text(60_000))]);
    let mut archive = Archive::read(&zip, &Limits { max_package_bytes: 130_000, ..Limits::default() }).unwrap();
    let mut buffer = vec![0u8; 60_000];
    for index in [0, 1, 0, 1] {
        archive.inflate_into(index, &mut buffer, &mut work).unwrap();
    }
}

// ---------------------------------------------------------------------------------------------------------------------------- `holds`

#[test]
fn holds_says_whether_an_entry_holds_bytes_the_caller_has_and_reads_it_as_inflate_does() {
    // An entry stored or deflated, and an empty one, hold what they were made of: their data is read and kept nowhere, and the cost counted is what an inflate counts.
    let data = text(5_000);
    let zip = build(&[Item::stored("a.bin", &data), Item::deflated("b.xml", &data), Item::stored("empty", b""), Item::stored("dir/", b"")]);
    let mut by_inflate = Archive::read(&zip, &Limits::default()).unwrap();
    let mut by_holds = Archive::read(&zip, &Limits::default()).unwrap();
    let (mut a, mut b) = (Work::default(), Work::default());
    for (index, known) in [(0usize, &data[..]), (1, &data[..]), (2, &b""[..])] {
        assert_eq!(by_inflate.inflate(index, &mut a).unwrap(), known);
        assert!(by_holds.holds(index, known, &mut b).unwrap(), "entry {index}");
    }
    assert_eq!(a.inflated, b.inflated, "the data is read as an inflate reads it");
    // Other bytes of the same length, or bytes of another length: not held, and nothing is read.
    let mut other = data.clone();
    other[4_999] ^= 1;
    let before = b.inflated;
    assert!(!by_holds.holds(0, &other, &mut b).unwrap());
    assert!(!by_holds.holds(1, &data[..4_999], &mut b).unwrap());
    assert!(!by_holds.holds(2, b"x", &mut b).unwrap());
    assert_eq!(b.inflated, before, "a row that states other bytes is not read");
    // A folder holds nothing; an index past the entries is damage.
    assert!(!by_holds.holds(3, b"", &mut b).unwrap());
    assert_eq!(by_holds.holds(9, b"", &mut b).unwrap_err().class(), "zip_corrupt");
}

#[test]
fn holds_refuses_an_entry_whose_data_is_not_what_its_row_states_and_counts_it_once_toward_the_total() {
    let data = text(5_000);
    let state_of = |bytes: &[u8]| Archive::read(&build(&[Item::stored("x", bytes)]), &Limits::default()).unwrap().entries()[0].crc;
    let mut work = Work::default();
    // A row that states the size and the checksum of the caller's bytes over data that is not them: damage, as an inflate finds it, and not a part that holds them.
    let mut known = data.clone();
    known[0] ^= 1;
    let mut lying = Item::stored("crc.bin", &data);
    lying.crc = state_of(&known);
    let zip = build(&[lying]);
    let mut archive = Archive::read(&zip, &Limits::default()).unwrap();
    let error = archive.holds(0, &known, &mut work).unwrap_err();
    assert_eq!(error.class(), "zip_corrupt");
    assert!(error.to_string().contains("checksum"), "{error}");
    // A row that states a size its data does not have.
    let short = &data[..4_000];
    let mut lie = Item::deflated("lie.xml", &data).claiming_size(4_000);
    lie.crc = state_of(short);
    let zip = build(&[lie]);
    let mut archive = Archive::read(&zip, &Limits::default()).unwrap();
    let error = archive.holds(0, short, &mut work).unwrap_err();
    assert_eq!(error.class(), "zip_corrupt");
    assert!(error.to_string().contains("says it is 4000 bytes"), "{error}");
    // Counted toward the package total once however often an entry is read, as `inflate` counts it: two entries of 60,000 bytes in a package of 130,000, each read twice.
    let big = text(60_000);
    let zip = build(&[Item::deflated("a.xml", &big), Item::deflated("b.xml", &big)]);
    let mut archive = Archive::read(&zip, &Limits { max_package_bytes: 130_000, ..Limits::default() }).unwrap();
    for index in [0, 1, 0, 1] {
        assert!(archive.holds(index, &big, &mut work).unwrap());
    }
    // And held to it: an entry that states 30,000 bytes and holds 60,000, after one of 60,000, is past a total of 100,000 when its data is read, as by `inflate`.
    let (half, mut over) = (&big[..30_000], Item::stored("b.bin", &big).claiming_size(30_000));
    over.crc = state_of(half);
    let zip = build(&[Item::stored("a.bin", &big), over]);
    let limits = Limits { max_package_bytes: 100_000, ..Limits::default() };
    let mut archive = Archive::read(&zip, &limits).unwrap();
    assert!(archive.holds(0, &big, &mut work).unwrap());
    assert_eq!(archive.holds(1, half, &mut work).unwrap_err().class(), "package_too_large");
    let mut archive = Archive::read(&zip, &limits).unwrap();
    archive.inflate(0, &mut work).unwrap();
    assert_eq!(archive.inflate(1, &mut work).unwrap_err().class(), "package_too_large", "the class an inflate gives");
}

#[test]
fn directory_rows_is_the_count_the_read_makes_and_a_second_end_record_in_the_comment_does_not_change_it() {
    // A count that a budget of memory is sized by is the count the archive is read by. A file of 3 rows and a comment that holds a signature of an end record
    // (which does not end the file, so the read does not take it) has 3 rows; a comment that holds one that does end the file is the record that is read, as the read reads it.
    use diacraft_container::zip::{ArchiveWriter, directory_rows};
    let mut w = ArchiveWriter::new();
    for i in 0..3 {
        w.add(&format!("e{i}"), b"x", Method::Stored).unwrap();
    }
    let clean = w.finish().unwrap();
    assert_eq!(directory_rows(&clean).unwrap(), 3);
    assert_eq!(Archive::read(&clean, &Limits::default()).unwrap().entries().len(), 3);
    // A comment that holds the signature of an end record counting 1,  whose own comment length (65,535) does not end where the file does.
    let mut fake = vec![0x50, 0x4B, 0x05, 0x06, 0, 0, 0, 0, 1, 0, 1, 0, 46, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF];
    fake.extend_from_slice(b" and the rest of the comment");
    let mut with_comment = clean.clone();
    let at = with_comment.len() - 2;
    with_comment[at..].copy_from_slice(&(fake.len() as u16).to_le_bytes());
    with_comment.extend_from_slice(&fake);
    assert_eq!(directory_rows(&with_comment).unwrap(), 3, "the record that fits the file is the real one");
    assert_eq!(Archive::read(&with_comment, &Limits::default()).unwrap().entries().len(), 3);
    // What is no archive has no count, in the words the read gives.
    assert_eq!(directory_rows(b"not a zip").unwrap_err().class(), "not_zip");
    assert_eq!(directory_rows(b"").unwrap_err().class(), "not_zip");
    // The count is checked against the directory it gives: a record that counts more rows than 46 bytes each fit in the directory is refused as the read refuses it.
    let mut lying = clean.clone();
    let at = lying.len() - 22 + 10;
    lying[at..at + 2].copy_from_slice(&50_000u16.to_le_bytes());
    assert_eq!(directory_rows(&lying).unwrap_err().class(), "zip_corrupt");
}
