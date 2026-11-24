//! Fuzzing (AGENTS.md: importers are fuzzed): bytes of any kind, packages this crate's tests built and then damaged, manifests of any
//! shape, and pairs of drawings of any small shape give a package, a comparison or an error, never a panic; and what they give holds the
//! rules. These run under `proptest` with a fixed budget of cases. Random bytes find crashes and not hangs: the shapes that cost the most
//! are built by hand in `cost.rs` and `hostile.rs` and counted.

use diacraft_odg::package::{MANIFEST_PATH, Package};
use diacraft_odg::{Limits, Manifest, copy_account, copy_unchanged, diff_packages, sniff};
use proptest::prelude::*;

use crate::build::{DRAWING, content, drawing, files, page, shape, with_manifest, write};
use crate::zipbuild::{Item, Opts, build_with};

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
    "xml_malformed",
    "xml_entity_refused",
    "xml_too_complex",
    "missing_manifest",
    "manifest_malformed",
    "not_a_drawing",
    "too_many_manifest_entries",
    "too_much_xml",
];

const WARNINGS: &[&str] = &[
    "bytes_after_end_record",
    "mimetype_irregular",
    "manifest_incomplete",
    "manifest_lists_missing_file",
    "manifest_duplicate_entry",
    "manifest_entry_incomplete",
    "manifest_no_root_entry",
    "media_type_unknown",
    "part_not_scanned",
];

/// What every read must hold: a known class, or a package whose entries have one name each and whose warnings are known; and then a copy
/// of it that reads back with the same files, and that is the file exactly when the account of it says nothing was rewritten, reordered or
/// left out.
fn check(bytes: &[u8]) {
    let limits = Limits::default();
    let _variant = sniff(bytes); // Any answer, and no panic.
    if let Err(e) = Manifest::read(bytes, &limits) {
        assert!(CLASSES.contains(&e.class()), "{e:?}");
    }
    let package = match Package::read(bytes, &limits) {
        Ok(package) => package,
        Err(e) => {
            assert!(CLASSES.contains(&e.class()), "an unknown class: {e:?}");
            let _shown = e.to_string();
            return;
        }
    };
    for warning in package.warnings() {
        assert!(WARNINGS.contains(&warning.class()), "{warning:?}");
        let _shown = warning.to_string();
    }
    let mut names: Vec<&str> = package.entries().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(before, names.len());
    assert!(package.part(MANIFEST_PATH).is_some());
    let copy = copy_unchanged(bytes, &limits).unwrap();
    let account = copy_account(bytes, &limits).unwrap();
    assert_eq!(account.is_exact(), copy == bytes, "{account:?}");
    if account.rewritten == 0 {
        assert_eq!(copy.len() as u64 + account.dropped.total(), bytes.len() as u64, "{account:?}");
    }
    assert!(copy_account(&copy, &limits).unwrap().is_exact(), "a copy of the copy is the copy");
    let again = Package::read(&copy, &limits).unwrap();
    let sorted = |p: &Package| {
        let mut names: Vec<String> = p.entries().map(|e| e.name.clone()).collect();
        names.sort_unstable();
        names
    };
    assert_eq!(sorted(&package), sorted(&again), "the copy holds the same entries");
    for name in package.parts() {
        assert_eq!(package.part(name), again.part(name), "{name}");
    }
    let diff = diff_packages(bytes, &copy, &limits).unwrap();
    assert!(diff.parts.is_empty() && diff.bytes_only.is_empty() && diff.added.is_empty() && diff.removed.is_empty(), "{diff:?}");
}

/// A package with every kind of bytes a copy leaves out (the seeds held none, so the add-up check of [`check`], that the
/// copy and what it dropped are the file, could not fail): 4 bytes before the first entry (so `mimetype` is not first), 16 after the data of
/// `styles.xml` that no flag announces, 6 between the directory and the end record, an archive comment of 9 and 5 bytes after the end record.
fn dropping_sample() -> Vec<u8> {
    let entries = with_manifest(DRAWING, files(&page("p1", &(shape("a", "1cm") + &shape("b", "2cm")))));
    let mut items = vec![Item::stored("mimetype", DRAWING.as_bytes())];
    for (name, data) in &entries {
        let item = Item::deflated(name, data);
        items.push(if name == "styles.xml" { Item { after_data: vec![0xAB; 16], ..item } } else { item });
    }
    let opts = Opts { prefix: b"lead".to_vec(), before_end: b"before".to_vec(), comment: b"a comment".to_vec(), after_end: b"after".to_vec() };
    build_with(&items, false, &opts)
}

#[test]
fn the_sample_a_copy_leaves_bytes_out_of_holds_what_it_says() {
    let bytes = dropping_sample();
    let account = copy_account(&bytes, &Limits::default()).unwrap();
    assert_eq!(account.rewritten, 0, "{account:?}");
    assert_eq!(
        (account.dropped.unannounced, account.dropped.archive_comment, account.dropped.after_end_record, account.dropped.total()),
        (4 + 16 + 6, 9, 5, 4 + 16 + 6 + 9 + 5),
        "{account:?}"
    );
    // The add-up of `check` has something to add up here: the copy is the file less what it left out, and the package reads.
    let copy = copy_unchanged(&bytes, &Limits::default()).unwrap();
    assert_eq!(copy.len() as u64 + account.dropped.total(), bytes.len() as u64);
    check(&bytes);
    assert!(Package::read(&bytes, &Limits::default()).unwrap().warnings().iter().any(|w| w.class() == "mimetype_irregular"));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn a_damaged_package_with_bytes_a_copy_drops_gives_a_package_or_an_error(
        flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..6),
    ) {
        let mut bytes = dropping_sample();
        for (at, value) in flips {
            let i = at.index(bytes.len());
            bytes[i] = value;
        }
        check(&bytes);
    }

    #[test]
    fn any_bytes_give_a_package_or_an_error(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        check(&bytes);
    }

    #[test]
    fn a_damaged_drawing_gives_a_package_or_an_error(flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8)) {
        let mut bytes = drawing(&page("p1", &(shape("a", "1cm") + &shape("b", "2cm"))));
        for (at, value) in flips {
            let i = at.index(bytes.len());
            bytes[i] = value;
        }
        check(&bytes);
    }

    #[test]
    fn a_manifest_of_any_shape_gives_a_package_or_an_error(
        rows in proptest::collection::vec(("[a-zA-Z0-9 ./%-]{0,12}", "[a-z/.-]{0,10}", any::<bool>()), 0..12),
        root in prop_oneof![Just(DRAWING.to_owned()), Just(String::new()), "[a-z./-]{0,20}"],
        prefix in prop_oneof![Just("manifest"), Just("m"), Just("")],
    ) {
        let decl = if prefix.is_empty() { "xmlns".to_owned() } else { format!("xmlns:{prefix}") };
        let p = if prefix.is_empty() { String::new() } else { format!("{prefix}:") };
        let mut text = format!("<{p}manifest {decl}=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\">");
        text.push_str(&format!("<{p}file-entry manifest:full-path=\"/\" manifest:media-type=\"{root}\"/>"));
        for (path, media, encrypted) in &rows {
            let inner = if *encrypted { format!("<{p}encryption-data/>") } else { String::new() };
            text.push_str(&format!("<{p}file-entry manifest:full-path=\"{path}\" manifest:media-type=\"{media}\">{inner}</{p}file-entry>"));
        }
        text.push_str(&format!("</{p}manifest>"));
        let mut entries = files("");
        entries.push((MANIFEST_PATH.to_owned(), text.into_bytes()));
        check(&write(Some(DRAWING), &entries));
    }

    #[test]
    fn two_drawings_of_any_small_shape_compare_without_a_panic(
        a in proptest::collection::vec((0u8..4, 0u8..3), 0..10),
        b in proptest::collection::vec((0u8..4, 0u8..3), 0..10),
    ) {
        // Shapes of a few names and attribute values, some nested in groups, some with text: added, removed, moved and changed all reach.
        let render = |spec: &[(u8, u8)]| -> String {
            spec.iter().map(|&(kind, value)| match kind {
                0 => shape("s", &format!("{value}cm")),
                1 => format!("<draw:rect svg:x=\"{value}cm\"/>"),
                2 => format!("<draw:g>{}</draw:g>", shape("in", &format!("{value}cm"))),
                _ => format!("<draw:frame>{}</draw:frame>", " ".repeat(usize::from(value))),
            }).collect()
        };
        let (x, y) = (drawing(&page("p1", &render(&a))), drawing(&page("p1", &render(&b))));
        let limits = Limits::default();
        let d = diff_packages(&x, &y, &limits).unwrap();
        let same = diff_packages(&x, &x, &limits).unwrap();
        prop_assert!(same.is_empty());
        if a == b {
            prop_assert!(d.is_empty());
        } else if content(&page("p1", &render(&a))) != content(&page("p1", &render(&b))) {
            // The parts differ, so something is listed of content.xml.
            prop_assert!(!d.parts.is_empty() || !d.bytes_only.is_empty(), "{d:?}");
        }
        for part in &d.parts {
            let c = part.counts;
            prop_assert!(c.added + c.removed + c.changed + c.moved + c.whitespace > 0);
            prop_assert_eq!(part.added.len(), c.added);
            prop_assert_eq!(part.removed.len(), c.removed);
        }
    }
}
