//! Hostile packages (the ODF plan's Review Focus 4): `mimetype` not first or compressed, a manifest listing absent or encrypted entries,
//! names with `..` or that differ only in case, zip bombs, and a `content.xml` of nested `draw:g` 100,000 deep. Each is refused or read
//! within the caps, never a panic or a hang, and what each cost is **counted** in the units the reader walks (bytes inflated, XML events,
//! name comparisons), never timed: a time moves with the load on the machine. One generous time bound over the whole test is a backstop
//! only.

use std::time::{Duration, Instant};

use diacraft_odg::package::{MANIFEST_PATH, Package};
use diacraft_odg::{DiffWork, Limits, Warning, Work, diff_packages_counted};

use crate::build::{DRAWING, content, files, manifest, package, page, shapes, with_manifest, write};
use crate::zipbuild::{Item, build};

/// What a read gave, by class, and what it cost.
fn counted(bytes: &[u8], limits: &Limits) -> (&'static str, Work) {
    let mut work = Work::default();
    let class = match Package::read_counted(bytes, limits, &mut work) {
        Ok(_) => "read",
        Err(e) => e.class(),
    };
    (class, work)
}

/// The parts of a small drawing and its manifest, as the tests' own items, after `first`.
fn items_after(first: Vec<Item>) -> Vec<Item> {
    let mut items = first;
    items.extend(with_manifest(DRAWING, files(&page("p1", &shapes(2)))).iter().map(|(n, d)| Item::deflated(n, d)));
    items
}

/// The uncompressed bytes of the parts of [`items_after`]: what a read of it may inflate at most, besides what a test adds.
fn honest_bytes() -> u64 {
    with_manifest(DRAWING, files(&page("p1", &shapes(2)))).iter().map(|(_, d)| d.len() as u64).sum()
}

#[test]
fn hostile_packages_are_refused_within_caps() {
    let started = Instant::now();
    let limits = Limits::default();
    let honest = honest_bytes();

    // `mimetype` not first, and compressed: read, with one warning, inflating the parts and nothing more.
    for first in
        [vec![Item::deflated("a.xml", b"<a/>"), Item::stored("mimetype", DRAWING.as_bytes())], vec![Item::deflated("mimetype", DRAWING.as_bytes())]]
    {
        let bytes = build(&items_after(first));
        let (class, work) = counted(&bytes, &limits);
        assert_eq!(class, "read");
        assert!(work.inflated <= honest + 4 + DRAWING.len() as u64, "{work:?}");
    }

    // A manifest that lists 10,000 files the package lacks, under folders of shared names, beside 1,000 it has: read, with 200
    // named and the rest counted, at the cost of a sort of the names and a binary search of them for each row that may name a folder.
    let folder = "Object 1/".repeat(10);
    let rows: Vec<String> = (0..10_000).map(|i| if i % 2 == 0 { format!("{folder}{i}/") } else { format!("{folder}{i}.xml") }).collect();
    let present: Vec<String> = (0..1_000).map(|i| format!("{folder}present{i}.xml")).collect();
    let mut entries = files("");
    entries.extend(present.iter().map(|name| (name.clone(), b"<x/>".to_vec())));
    let mut listed: Vec<(&str, &str)> = rows.iter().chain(&present).map(|r| (r.as_str(), "text/xml")).collect();
    listed.extend([("content.xml", "text/xml"), ("styles.xml", "text/xml"), ("meta.xml", "text/xml"), ("settings.xml", "text/xml")]);
    entries.push((MANIFEST_PATH.to_owned(), manifest(DRAWING, &listed).into_bytes()));
    let bytes = write(Some(DRAWING), &entries);
    let mut work = Work::default();
    let read = Package::read_counted(&bytes, &limits, &mut work).unwrap();
    let missing = read.warnings().iter().filter(|w| matches!(w, Warning::ManifestListsMissingFile { .. })).count();
    assert_eq!(missing, 200);
    assert!(read.warnings().contains(&Warning::MoreManifestListsMissingFile { count: 9_800 }));
    // 1,006 entries: two sorts of them (the directory's duplicate rule and this one), about 10,000 comparisons each, and a search of about
    // 11 for each of the 10,000 rows (a file's row that is not there is looked for as a folder too): 128,861 measured. A walk of the names
    // for each row would be 10 million.
    assert!(work.name_comparisons <= 140_000, "{}", work.name_comparisons);

    // A manifest that marks entries encrypted: refused before any entry but the manifest is inflated.
    let encrypted = manifest(DRAWING, &[("content.xml", "text/xml")]).replace(
        r#"manifest:full-path="content.xml" manifest:media-type="text/xml"/>"#,
        r#"manifest:full-path="content.xml" manifest:media-type="text/xml"><manifest:encryption-data/></manifest:file-entry>"#,
    );
    let entries = vec![("content.xml".to_owned(), vec![0x9C; 4096]), (MANIFEST_PATH.to_owned(), encrypted.clone().into_bytes())];
    let (class, work) = counted(&write(Some(DRAWING), &entries), &limits);
    assert_eq!(class, "encrypted_package");
    assert_eq!(work.inflated, encrypted.len() as u64);

    // Names with `..`, and names that differ only in case: refused from the directory with nothing inflated, and two entries.
    let (class, work) = counted(&build(&items_after(vec![Item::stored("Pictures/../../x.png", b"x")])), &limits);
    assert_eq!((class, work.inflated), ("path_traversal", 0));
    let (class, _) = counted(&build(&items_after(vec![Item::stored("Pictures/A.png", b"1"), Item::stored("Pictures/a.png", b"2")])), &limits);
    assert_eq!(class, "read");

    // Zip bombs. A header that claims 2 GiB from 4 KiB: refused from the header, nothing inflated.
    let claim = Item { size: 2 * 1024 * 1024 * 1024 - 1, ..Item::deflated("Pictures/bomb.png", &[0u8; 4096]) };
    let (class, work) = counted(&build(&items_after(vec![Item::stored("mimetype", DRAWING.as_bytes()), claim])), &limits);
    assert_eq!((class, work.inflated), ("zip_ratio_exceeded", 0));
    // An entry whose header says 1 KiB and which inflates to 8 MiB, under a part cap of 1 MiB: it costs the cap and a byte, and is refused.
    let small =
        Limits { container: diacraft_container::Limits { max_part_bytes: 1 << 20, ..diacraft_container::Limits::default() }, ..Limits::default() };
    let lie = Item { size: 1024, ..Item::deflated("Pictures/lie.png", &vec![7u8; 8 << 20]) };
    let (class, work) = counted(&build(&items_after(vec![Item::stored("mimetype", DRAWING.as_bytes()), lie])), &small);
    assert_eq!(class, "part_too_large");
    assert!(work.inflated <= honest + DRAWING.len() as u64 + (1 << 20) + 1, "{work:?}");
    // More entries than the part limit: refused from the count, nothing inflated.
    let few = Limits { container: diacraft_container::Limits { max_parts: 8, ..diacraft_container::Limits::default() }, ..Limits::default() };
    let many: Vec<Item> = (0..20).map(|i| Item::stored(&format!("Pictures/{i}.png"), b"x")).collect();
    let (class, work) = counted(&build(&items_after(many)), &few);
    assert_eq!((class, work.inflated), ("too_many_parts", 0));
    // A manifest of more rows than its cap.
    let rows: Vec<String> = (0..60).map(|i| format!("f{i}.xml")).collect();
    let rows: Vec<(&str, &str)> = rows.iter().map(|r| (r.as_str(), "text/xml")).collect();
    let entries = vec![("content.xml".to_owned(), content("").into_bytes()), (MANIFEST_PATH.to_owned(), manifest(DRAWING, &rows).into_bytes())];
    let capped = Limits { max_manifest_entries: 50, ..Limits::default() };
    assert_eq!(counted(&write(Some(DRAWING), &entries), &capped).0, "too_many_manifest_entries");
    // A manifest with a DTD and an entity: refused before anything is expanded.
    let bomb = "<!DOCTYPE m [<!ENTITY a \"aaaaaaaaaa\"><!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;\">]><m>&b;</m>";
    let entries = vec![("content.xml".to_owned(), content("").into_bytes()), (MANIFEST_PATH.to_owned(), bomb.as_bytes().to_vec())];
    let (class, work) = counted(&write(Some(DRAWING), &entries), &limits);
    assert_eq!(class, "xml_entity_refused");
    assert!(work.xml_events <= 4, "{work:?}");

    // A `content.xml` of nested `draw:g` 100,000 deep: the package is read and its bytes kept, and the parse that looks for macros stops at
    // the depth limit (200), costing an event for each level it read. The comparison says the part does not parse, at the same cost.
    let deep = format!("{}{}", "<draw:g>".repeat(100_000), "</draw:g>".repeat(100_000));
    let deep_content = content(&page("p1", &deep));
    let bytes = package(DRAWING, crate::build::replace(files(""), "content.xml", deep_content.clone().into_bytes()));
    let mut work = Work::default();
    let read = Package::read_counted(&bytes, &limits, &mut work).unwrap();
    assert_eq!(read.part("content.xml").unwrap(), deep_content.as_bytes());
    assert!(matches!(&read.warnings(), [Warning::PartNotScanned { part, class }] if part == "content.xml" && *class == "xml_too_complex"));
    assert!(work.xml_events < 1_000, "{work:?}");
    let shallow = package(DRAWING, files(""));
    let mut dwork = DiffWork::default();
    let d = diff_packages_counted(&shallow, &bytes, &limits, &mut dwork).unwrap();
    assert_eq!((d.unparsed.len(), d.unparsed[0].class), (1, "xml_too_complex"));
    assert!(dwork.xml.xml_events < 1_000, "{dwork:?}");

    assert!(started.elapsed() < Duration::from_secs(120), "a backstop only: the counts above are the test");
}
