//! Fuzzing (AGENTS.md: importers and parsers are fuzzed): bytes of any kind, archives this crate wrote and then damaged, archives written
//! from any names and bytes, and XML of any shape give an archive, a tree or an error, never a panic; and what they give holds the rules.
//!
//! These run under `proptest` with a fixed budget of cases; a failure prints the input that made it. Random bytes find crashes and
//! not hangs: the shapes that cost the most are built by hand in `cost.rs` and counted.

use diacraft_container::xml::{Node, Tree, parse, parse_with, to_bytes};
use diacraft_container::zip::{Archive, ArchiveWriter, Copied, Method, copy_account};
use diacraft_container::{Limits, Work, XmlLimits};
use proptest::prelude::*;

use crate::stream;
use crate::zipbuild::{Item, Opts, build_with, extra_record};

const CLASSES: &[&str] = &[
    "not_zip",
    "zip_corrupt",
    "path_traversal",
    "invalid_part_name",
    "zip_ratio_exceeded",
    "part_too_large",
    "package_too_large",
    "too_many_parts",
    "unsupported_compression",
    "encrypted_package",
    "duplicate_part",
    "out_of_memory",
];

/// Small limits, so that every limit is within reach of a small input.
fn small_limits() -> Limits {
    Limits {
        max_parts: 8,
        max_package_bytes: 4000,
        max_part_bytes: 1500,
        max_ratio: 50,
        max_part_name_bytes: 24,
        xml: XmlLimits { max_depth: 12, max_nodes: 200, max_attr_bytes: 100 },
    }
}

/// What every archive that reads must hold, and every entry that inflates: plain names, none twice, folders with no data, every limit.
fn check_archive(bytes: &[u8], limits: &Limits) {
    let mut archive = match Archive::read(bytes, limits) {
        Ok(archive) => archive,
        Err(e) => {
            assert!(CLASSES.contains(&e.class()), "an unknown class: {e:?}");
            let _shown = e.to_string(); // Display holds for any error.
            return;
        }
    };
    check_copy(bytes, &archive, limits);
    let entries = archive.entries().to_vec();
    assert!(entries.iter().filter(|e| !e.folder).count() <= limits.max_parts);
    assert!(archive.rows() >= entries.len());
    let mut names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(before, names.len(), "two entries of one name");
    let mut total = 0u64;
    let mut work = Work::default();
    for (index, entry) in entries.iter().enumerate() {
        let bare = entry.name.strip_suffix('/').unwrap_or(&entry.name);
        assert!(!bare.is_empty() && bare.len() <= limits.max_part_name_bytes, "{entry:?}");
        assert!(!entry.name.starts_with('/') && !entry.name.contains('\\') && !bare.split('/').any(|s| s == ".."), "{entry:?}");
        assert!(entry.written.is_none() && entry.data_offset >= entry.local_offset + 30, "{entry:?}");
        assert_eq!(entry.folder, entry.name.ends_with('/'));
        if entry.folder {
            assert_eq!(entry.size, 0);
        } else {
            assert!(entry.compression().is_some() && entry.flags & 0x2041 == 0 && entry.size <= limits.max_part_bytes, "{entry:?}");
        }
        match archive.inflate(index, &mut work) {
            Ok(data) => {
                assert_eq!(data.len() as u64, entry.size);
                assert!(!entry.folder || data.is_empty());
                total += data.len() as u64;
            }
            Err(e) => assert!(CLASSES.contains(&e.class()), "an unknown class: {e:?}"),
        }
    }
    assert!(total <= limits.max_package_bytes);
    // What came out is at most what the inflater gave, and no more than the limits allow it to give (one byte past a limit at most, each).
    assert!(work.inflated >= total);
    assert!(work.inflated <= limits.max_package_bytes + entries.len() as u64 * (limits.max_part_bytes + 1));
}

/// What a copy of every entry of an archive that reads must hold: the account of it is had without error and counts every
/// entry; when the copy can be written (the writer refuses some names a judge would keep) it is the file exactly when the account says so,
/// as much shorter as the account says when no header was written anew, it reads back with as many entries and its own account is exact;
/// and a streaming reader that reads the file whole reads the copy whole.
fn check_copy(bytes: &[u8], archive: &Archive<'_>, limits: &Limits) {
    let account = copy_account(archive).unwrap();
    let entries = archive.entries();
    assert_eq!(account.exact + account.rewritten, entries.len(), "{account:?}");
    assert_eq!(account.rows_not_kept, 0, "the plain judge keeps every row");
    let mut order: Vec<(usize, usize)> = entries.iter().enumerate().map(|(i, e)| (e.local_offset, i)).collect();
    order.sort_unstable();
    let mut writer = ArchiveWriter::new();
    let mut how = Vec::new();
    for (_, i) in order {
        match writer.copy(archive, i) {
            Ok(copied) => how.push(copied),
            Err(e) => return assert!(CLASSES.contains(&e.class()), "an unknown class: {e:?}"),
        }
    }
    assert_eq!(how.iter().filter(|&&c| c == Copied::Exact).count(), account.exact, "the copy and the account plan alike");
    let copy = match writer.finish() {
        Ok(copy) => copy,
        Err(e) => return assert_eq!(e.class(), "duplicate_part"),
    };
    assert_eq!(account.is_exact(), copy == bytes, "{account:?}");
    if account.rewritten == 0 {
        assert_eq!(copy.len() as u64 + account.dropped.total(), bytes.len() as u64, "{account:?}");
    }
    let again = Archive::read(&copy, limits).unwrap();
    assert_eq!(again.entries().len(), entries.len());
    assert!(copy_account(&again).unwrap().is_exact(), "a copy of the copy is the copy");
    if stream::walk(bytes).is_ok_and(|names| names.len() == entries.len()) {
        assert_eq!(stream::walk(&copy).map(|names| names.len()), Ok(entries.len()), "the copy streams as the file did");
    }
}

/// A valid archive this crate's writer made, of a few entries of each kind.
fn sample() -> Vec<u8> {
    let mut w = ArchiveWriter::new();
    w.add("mimetype", b"application/vnd.oasis.opendocument.graphics", Method::Stored).unwrap();
    w.add("content.xml", &b"<office:document-content><draw:page/></office:document-content>".repeat(3), Method::Deflated).unwrap();
    w.add_directory("Configurations2/").unwrap();
    w.add("Object 1/content.xml", b"<a/>", Method::Deflated).unwrap();
    w.add_smallest("Pictures/caf\u{e9}.png", &[0x89, b'P', b'N', b'G', 0, 1, 2, 3]).unwrap();
    w.finish().unwrap()
}

/// An archive with what a copy must plan around, built by hand: a zero-length folder with flag bit 3, a ZIP64 record
/// (0, 0) in its local header and a 24-byte descriptor; a deflated entry with a short descriptor; a 16-byte descriptor after an entry whose
/// flags announce none, bytes before the first entry, and a comment, unless `clean` (then a streaming reader reads it whole).
fn described_sample(clean: bool) -> Vec<u8> {
    let zeros = extra_record(1, &[0u8; 16]);
    let wide = [&0x0807_4B50u32.to_le_bytes()[..], &[0u8; 20]].concat();
    let folder = Item {
        local_sizes: Some((0, 0xFFFF_FFFF, 0xFFFF_FFFF)),
        local_extra: zeros,
        after_data: wide,
        ..Item::stored("Configurations2/", b"").with_flags(0x0008)
    };
    let content = Item::deflated("content.xml", &b"<office:document-content/>".repeat(4)).with_flags(0x0008);
    let short = [
        &0x0807_4B50u32.to_le_bytes()[..],
        &content.crc.to_le_bytes(),
        &(content.data.len() as u32).to_le_bytes(),
        &(content.size as u32).to_le_bytes(),
    ]
    .concat();
    let content = Item { local_sizes: Some((0, 0, 0)), after_data: short, ..content };
    let styles = Item::deflated("styles.xml", b"<office:document-styles/>");
    let unannounced = [
        &0x0807_4B50u32.to_le_bytes()[..],
        &styles.crc.to_le_bytes(),
        &(styles.data.len() as u32).to_le_bytes(),
        &(styles.size as u32).to_le_bytes(),
    ]
    .concat();
    let styles = if clean { styles } else { Item { after_data: unannounced, ..styles } };
    let items = [Item::stored("mimetype", b"application/vnd.oasis.opendocument.graphics"), folder, content, styles];
    if clean {
        return build_with(&items, &Opts::default());
    }
    build_with(&items, &Opts { prefix: b"lead".to_vec(), comment: b"a comment".to_vec(), ..Opts::default() })
}

#[test]
fn the_sample_with_descriptors_holds_what_it_says() {
    // The seed of the fuzz below, undamaged: every entry kept byte for byte, and the 4 bytes before the first entry, the 16 that no flag
    // announces and the 9 of the comment left out.
    let bytes = described_sample(false);
    let account = copy_account(&Archive::read(&bytes, &Limits::default()).unwrap()).unwrap();
    assert_eq!((account.exact, account.rewritten, account.dropped.unannounced, account.dropped.archive_comment), (4, 0, 20, 9), "{account:?}");
    assert_eq!(stream::walk(&bytes).map_err(|broke| broke.entry), Err(1), "the bytes before the first entry");
    check_archive(&bytes, &Limits::default());
    let clean = described_sample(true);
    assert!(copy_account(&Archive::read(&clean, &Limits::default()).unwrap()).unwrap().is_exact());
    assert_eq!(stream::walk(&clean).map(|names| names.len()), Ok(4));
}

/// A name of a few segments, with what names hold: spaces, dots, non-ASCII letters, and now and then a separator or a dot pair.
fn name() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![Just("a"), Just("Object 1"), Just("."), Just(".."), Just(""), Just("caf\u{e9}"), Just("b.xml"), Just("\\")],
        1..4,
    )
    .prop_map(|segments| segments.join("/"))
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn any_bytes_give_an_archive_or_an_error(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
        check_archive(&bytes, &small_limits());
        check_archive(&bytes, &Limits::default());
    }

    #[test]
    fn bytes_that_end_like_an_archive_give_an_archive_or_an_error(head in proptest::collection::vec(any::<u8>(), 0..300), count in 0u16..4, size in 0u32..400, start in 0u32..400) {
        // An end record of any numbers after any bytes: the shapes random bytes almost never reach.
        let mut bytes = head;
        bytes.extend_from_slice(b"PK\x05\x06\0\0\0\0");
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&size.to_le_bytes());
        bytes.extend_from_slice(&start.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        check_archive(&bytes, &small_limits());
    }

    #[test]
    fn a_damaged_archive_gives_an_archive_or_an_error(changes in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8), cut in any::<prop::sample::Index>(), truncate in any::<bool>()) {
        let mut bytes = sample();
        for (at, value) in changes {
            let at = at.index(bytes.len());
            bytes[at] = value;
        }
        if truncate {
            let keep = cut.index(bytes.len());
            bytes.truncate(keep);
        }
        check_archive(&bytes, &small_limits());
        check_archive(&bytes, &Limits::default());
    }

    #[test]
    fn a_damaged_archive_with_descriptors_gives_an_archive_or_an_error(changes in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 0..8), cut in any::<prop::sample::Index>(), truncate in any::<bool>(), clean in any::<bool>()) {
        // The copy's plans meet each field of a descriptor, a ZIP64 record and the bytes between entries changed.
        let mut bytes = described_sample(clean);
        for (at, value) in changes {
            let at = at.index(bytes.len());
            bytes[at] = value;
        }
        if truncate {
            let keep = cut.index(bytes.len());
            bytes.truncate(keep);
        }
        check_archive(&bytes, &Limits::default());
    }

    #[test]
    fn what_the_writer_takes_reads_back_as_it_went(entries in proptest::collection::vec((name(), proptest::collection::vec(any::<u8>(), 0..200), 0u8..3), 0..8)) {
        let mut w = ArchiveWriter::new();
        let mut written: Vec<(String, Vec<u8>)> = Vec::new();
        for (name, data, how) in entries {
            let result = match how {
                0 => w.add(&name, &data, Method::Stored),
                1 => w.add(&name, &data, Method::Deflated),
                _ => w.add_smallest(&name, &data).map(|_| ()),
            };
            match result {
                Ok(()) => written.push((name, data)),
                Err(e) => prop_assert!(matches!(e.class(), "path_traversal" | "invalid_part_name"), "{e:?}"),
            }
        }
        match w.finish() {
            Ok(zip) => {
                let mut archive = Archive::read(&zip, &Limits::default()).unwrap();
                prop_assert_eq!(archive.entries().len(), written.len());
                for (index, (name, data)) in written.iter().enumerate() {
                    prop_assert_eq!(&archive.entries()[index].name, name);
                    prop_assert_eq!(&archive.inflate(index, &mut Work::default()).unwrap(), data);
                }
            }
            Err(e) => {
                prop_assert_eq!(e.class(), "duplicate_part");
                let mut names: Vec<&String> = written.iter().map(|(n, _)| n).collect();
                names.sort_unstable();
                let before = names.len();
                names.dedup();
                prop_assert!(names.len() < before, "refused as a duplicate with none");
            }
        }
    }

    #[test]
    fn any_text_gives_a_tree_or_an_error(text in "[<>/a-c&;#x0-9=\"' \n!\\-\\[\\]?:]{0,80}") {
        match parse(text.as_bytes()) {
            Ok(tree) => {
                // What parses writes back and parses to the same tree.
                let bytes = to_bytes(&tree, b"", b"", false).unwrap();
                prop_assert_eq!(parse(&bytes).unwrap(), tree);
            }
            Err(e) => prop_assert!(matches!(e.class(), "xml_malformed" | "xml_entity_refused" | "xml_too_complex")),
        }
    }

    #[test]
    fn deep_and_wide_trees_hold_their_limits(depth in 0usize..40, width in 0usize..40) {
        let text = format!("{}{}{}", "<a>".repeat(depth + 1), "<b x=\"1\"/>".repeat(width), "</a>".repeat(depth + 1));
        let limits = XmlLimits { max_depth: 20, max_nodes: 50, max_attr_bytes: 10 };
        match parse_with(text.as_bytes(), &limits) {
            Ok(tree) => {
                prop_assert!(depth < 20 && depth + 1 + 2 * width <= 50);
                let mut el: &Tree = &tree;
                for _ in 0..depth {
                    el = el.child("a").unwrap();
                }
                prop_assert_eq!(el.elements().count(), width);
                prop_assert!(el.children.iter().all(|n| matches!(n, Node::El(_))));
            }
            Err(e) => prop_assert_eq!(e.class(), "xml_too_complex"),
        }
    }
}
