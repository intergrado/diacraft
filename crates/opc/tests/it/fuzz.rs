//! Fuzzing (AGENTS.md: importers and parsers are fuzzed): bytes of any kind, archives mutated from a valid one, archives built with
//! lying headers, hostile names and XML of any shape give a package or an error, never a panic, and what they give holds the rules.
//!
//! These run under `proptest` with a fixed budget of cases; a failure prints the input that made it.

use diacraft_opc::name::{fold, rels_part_name, resolve};
use diacraft_opc::xml::{El, Node, parse, parse_with};
use diacraft_opc::{Limits, Package, Warning, XmlLimits};
use proptest::prelude::*;

use crate::zipbuild::{Item, Local, Opts, build, build_with, extra_record};
use crate::{base_items, class_of, content_types_xml, rels_xml};

const CLASSES: &[&str] = &[
    "not_zip",
    "ole_with_package_extension",
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
    "part_name_prefix",
    "missing_content_types",
    "too_many_relationships",
    "too_many_content_type_entries",
    "xml_malformed",
    "xml_entity_refused",
    "xml_too_complex",
    "out_of_memory",
];

/// Small limits, so that every limit is within reach of a small input.
fn small_limits() -> Limits {
    Limits {
        max_parts: 20,
        max_package_bytes: 100_000,
        max_part_bytes: 40_000,
        max_ratio: 50,
        max_part_name_bytes: 40,
        max_package_xml_bytes: 20_000,
        max_package_xml_nodes: 400,
        max_relationships: 20,
        max_content_type_entries: 20,
        xml: XmlLimits { max_depth: 12, max_nodes: 500, max_attr_bytes: 200 },
    }
}

/// What every package that reads must hold: names are part names, no two are equal folded, none is a prefix of another, each is
/// found by its own name, and the limits held.
fn check_package(p: &Package, limits: &Limits) {
    let names: Vec<&str> = p.parts().collect();
    assert!(names.len() <= limits.max_parts);
    let mut folded: Vec<String> = names.iter().map(|n| fold(n)).collect();
    let mut total = 0u64;
    for n in &names {
        assert!(n.starts_with('/') && n.len() > 1 && n.len() <= limits.max_part_name_bytes + 1, "{n:?}");
        assert!(!n.split('/').skip(1).any(|s| s.is_empty() || s.ends_with('.')), "{n:?}");
        assert!(!n.contains('\\') && !n.chars().any(char::is_control), "{n:?}");
        let data = p.part(n).unwrap_or_else(|| panic!("part {n:?} is not found by its own name"));
        assert!(data.len() as u64 <= limits.max_part_bytes);
        total += data.len() as u64;
    }
    assert!(total <= limits.max_package_bytes);
    folded.sort();
    let before = folded.len();
    folded.dedup();
    assert_eq!(before, folded.len(), "two names are equal folded");
    for a in &folded {
        for b in &folded {
            assert!(!b.starts_with(&format!("{a}/")), "{a} is a prefix of {b}");
        }
    }
    // The content type lookup and the relationships of every part answer, without a panic.
    for n in &names {
        let _ = p.content_type(n);
        let _ = p.rels(n);
    }
    let _ = p.rels("/");
}

/// A valid package of a few parts, to mutate.
fn seed_package() -> Vec<u8> {
    let rels = rels_xml(&[("rId1", "urn:t", "visio/document.xml", None), ("rId2", "urn:t", "http://e/x", Some("External"))]);
    let ct = content_types_xml(&[("rels", "r"), ("xml", "application/xml"), ("png", "image/png")], &[("/visio/document.xml", "d")]);
    build(&[
        Item::deflated("[Content_Types].xml", ct.as_bytes()),
        Item::deflated("_rels/.rels", rels.as_bytes()),
        Item::deflated("visio/document.xml", b"<VisioDocument xmlns='x'><A b='1'/>text</VisioDocument>"),
        Item::stored("visio/media/image1.png", &[0x89, b'P', b'N', b'G', 1, 2, 3]),
        Item::deflated("visio/pages/page1.xml", &b"<PageContents><Shapes/></PageContents>".repeat(30)),
    ])
}

/// Archives with the shapes real writers make and that random mutation of a plain archive does not build (review finding 14): a data
/// descriptor, ZIP64 sentinel sizes with an honest ZIP64 field, a Unicode path field, a comment on a row, a local extra field of another
/// length than the row's, a folder item; with plain end records, with ZIP64 ones, with an archive comment, with bytes after the end record
/// (it reads, with a warning), and (the last, which does not read) behind a stub with ZIP64 records, which is a ZIP64 claim
/// whose offsets are shifted.
fn seed_shapes() -> Vec<Vec<u8>> {
    let mut descriptor = b"PK\x07\x08".to_vec();
    descriptor.extend_from_slice(&[9; 12]);
    let local = Local { flags: Some(8), sizes: Some((0, 0)), crc: Some(0), ..Local::default() };
    let mut with_comment = Item::stored("c.xml", b"<c/>").with_unicode_path("d.xml");
    with_comment.row_comment = b"a comment on a row".to_vec();
    let mut items = base_items();
    items.extend([
        Item::deflated("a.xml", &b"<a/>".repeat(20)).with_flags(8).with_local(local).with_descriptor(&descriptor),
        Item::deflated("b.xml", &b"<b/>".repeat(20)).with_zip64_field(),
        with_comment,
        Item::stored("e.xml", b"1").with_extra(&extra_record(0xCAFE, &[0; 3])),
        Item::stored("f/", b""),
        Item::stored("g.bin", b"1234").with_local(Local { extra: vec![0; 9], ..Local::default() }),
    ]);
    let mut after = build_with(&items, &Opts { comment: b"an archive comment".to_vec(), ..Opts::default() });
    after.extend_from_slice(b"a line of text after the end record\n");
    vec![
        seed_package(),
        build(&items),
        build_with(&items, &Opts { zip64: true, ..Opts::default() }),
        build_with(&items, &Opts { comment: b"an archive comment".to_vec(), ..Opts::default() }),
        after,
        build_with(&items, &Opts { zip64: true, prefix: b"STUB".to_vec(), ..Opts::default() }),
    ]
}

#[test]
fn the_seeds_of_the_mutation_fuzz_read_so_that_the_mutations_start_from_something() {
    // A fuzz test that only ever sees errors passes whatever the reader does: each seed must read, with the small limits, and the last,
    // behind its stub, must not.
    let limits = small_limits();
    let seeds = seed_shapes();
    let p = Package::read(&seeds[0], &limits).unwrap();
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/_rels/.rels", "/visio/document.xml", "/visio/media/image1.png", "/visio/pages/page1.xml"]);
    assert_eq!(p.rels("/").unwrap().len(), 2);
    check_package(&p, &limits);
    for (i, seed) in seeds.iter().enumerate().take(5).skip(1) {
        let p = Package::read(seed, &limits).unwrap_or_else(|e| panic!("seed {i}: {e}"));
        assert_eq!(p.parts().collect::<Vec<_>>(), ["/a.xml", "/b.xml", "/d.xml", "/e.xml", "/g.bin"], "seed {i}");
        assert_eq!(p.part("a.xml"), Some(&b"<a/>".repeat(20)[..]));
        check_package(&p, &limits);
    }
    assert_eq!(Package::read(&seeds[4], &limits).unwrap().warnings().first(), Some(&Warning::BytesAfterEndRecord { count: 36 }));
    assert_eq!(class_of(&seeds[5], &limits), "zip_corrupt");
}

fn config() -> ProptestConfig {
    ProptestConfig { cases: 300, failure_persistence: None, ..ProptestConfig::default() }
}

/// A number from the places that break the arithmetic of sizes, counts and offsets.
fn interesting_number() -> impl Strategy<Value = u64> {
    prop_oneof![
        3 => 0u64..200,
        1 => Just(0xFFFFu64),
        1 => Just(0xFFFF_FFFFu64),
        1 => Just(0x1_0000u64),
        1 => Just(1u64 << 32),
        1 => Just(1u64 << 40),
        1 => Just(1u64 << 63),
        1 => Just(u64::MAX),
        1 => Just(u64::MAX - 21),
        2 => 200u64..2000,
    ]
}

/// A name from the places that break path handling.
fn hostile_name() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        3 => "[a-zA-Z0-9]{1,5}",
        1 => Just("..".to_owned()),
        1 => Just(".".to_owned()),
        1 => Just("".to_owned()),
        1 => Just("\\".to_owned()),
        1 => Just("%2e".to_owned()),
        1 => Just("%2F".to_owned()),
        1 => Just("%".to_owned()),
        1 => Just("%FF".to_owned()),
        1 => Just("C:".to_owned()),
        1 => Just("\u{0}".to_owned()),
        1 => Just("\u{e9}".to_owned()),
        1 => Just("\u{202e}".to_owned()),
        1 => Just("a.".to_owned()),
        1 => Just("#frag".to_owned()),
        1 => Just("?q".to_owned()),
    ];
    let sep = prop_oneof![4 => Just("/".to_owned()), 1 => Just("".to_owned()), 1 => Just("//".to_owned())];
    prop::collection::vec((piece, sep), 0..8).prop_map(|v| v.into_iter().map(|(p, s)| format!("{p}{s}")).collect::<String>())
}

/// An item with every number a header holds chosen to be wrong in some way.
fn lying_item() -> impl Strategy<Value = Item> {
    let size = prop_oneof![Just(0u64), Just(1), 0..100_000u64, Just(1 << 31), Just((1 << 32) - 1), Just(1 << 32), Just(1 << 40), Just(u64::MAX)];
    (
        hostile_name(),
        prop_oneof![Just(0u16), Just(8), Just(8), Just(1), Just(12), Just(99), any::<u16>()],
        prop::collection::vec(any::<u8>(), 0..400),
        any::<u32>(),
        size.clone(),
        size,
        prop_oneof![5 => Just(0u16), 1 => Just(1u16), 1 => any::<u16>()],
        prop::bool::weighted(0.5),
        1..2000u64,
    )
        .prop_map(|(name, method, data, crc, size, csize, flags, honest_zeros, zeros)| {
            if honest_zeros {
                // A real stream of zeros, with a header that may or may not say so.
                let mut it = Item::zeros(&name, zeros);
                if crc % 3 == 0 {
                    it = it.claiming_size(size.min(1 << 20));
                }
                it
            } else {
                Item { name: name.into_bytes(), method, data, crc, size, csize, flags, ..Item::stored("", b"") }
            }
        })
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn any_bytes_give_a_package_or_an_error(bytes in prop::collection::vec(any::<u8>(), 0..3000)) {
        let limits = small_limits();
        match Package::read(&bytes, &limits) {
            Ok(p) => check_package(&p, &limits),
            Err(e) => prop_assert!(CLASSES.contains(&e.class()), "{}", e.class()),
        }
    }

    #[test]
    fn bytes_that_start_like_a_zip_give_a_package_or_an_error(tail in prop::collection::vec(any::<u8>(), 0..3000)) {
        let limits = small_limits();
        let mut bytes = vec![0x50, 0x4B, 0x03, 0x04];
        bytes.extend(tail);
        match Package::read(&bytes, &limits) {
            Ok(p) => check_package(&p, &limits),
            Err(e) => prop_assert!(CLASSES.contains(&e.class()), "{}", e.class()),
        }
    }

    #[test]
    fn a_valid_package_with_bytes_changed_gives_a_package_or_an_error(
        seed in any::<prop::sample::Index>(),
        flips in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..12),
        cut in prop::option::of(any::<prop::sample::Index>()),
        insert in prop::option::of((any::<prop::sample::Index>(), prop::collection::vec(any::<u8>(), 1..40))),
    ) {
        let limits = small_limits();
        let seeds = seed_shapes();
        let mut bytes = seeds[seed.index(seeds.len())].clone();
        for (at, v) in &flips {
            let i = at.index(bytes.len());
            bytes[i] = *v;
        }
        if let Some((at, junk)) = &insert {
            let i = at.index(bytes.len());
            bytes.splice(i..i, junk.iter().copied());
        }
        if let Some(at) = cut {
            bytes.truncate(at.index(bytes.len() + 1));
        }
        match Package::read(&bytes, &limits) {
            Ok(p) => check_package(&p, &limits),
            Err(e) => prop_assert!(CLASSES.contains(&e.class()), "{}", e.class()),
        }
    }

    #[test]
    fn end_records_with_fields_of_any_value_give_a_package_or_an_error(
        seed in 0usize..4,
        patches in prop::collection::vec((0usize..16, interesting_number()), 1..6),
    ) {
        // The fields of the end record, the ZIP64 locator and the ZIP64 record, each set to a number from the places that break
        // arithmetic: this builds the lying counts, sizes and offsets that flipping bytes almost never does.
        let limits = small_limits();
        let mut bytes = seed_shapes()[seed].clone();
        // (signature to find the record by, offset of the field in it, width in bytes)
        const FIELDS: [(&[u8], usize, usize); 16] = [
            (b"PK\x05\x06", 4, 2), (b"PK\x05\x06", 6, 2), (b"PK\x05\x06", 8, 2), (b"PK\x05\x06", 10, 2),
            (b"PK\x05\x06", 12, 4), (b"PK\x05\x06", 16, 4), (b"PK\x06\x07", 4, 4), (b"PK\x06\x07", 8, 8),
            (b"PK\x06\x07", 16, 4), (b"PK\x06\x06", 4, 8), (b"PK\x06\x06", 16, 4), (b"PK\x06\x06", 20, 4),
            (b"PK\x06\x06", 24, 8), (b"PK\x06\x06", 32, 8), (b"PK\x06\x06", 40, 8), (b"PK\x06\x06", 48, 8),
        ];
        for (which, value) in &patches {
            let (signature, offset, width) = FIELDS[*which];
            if let Some(record) = bytes.windows(4).rposition(|w| w == signature) {
                let at = record + offset;
                if let Some(field) = bytes.get_mut(at..at + width) {
                    field.copy_from_slice(&value.to_le_bytes()[..width]);
                }
            }
        }
        match Package::read(&bytes, &limits) {
            Ok(p) => check_package(&p, &limits),
            Err(e) => prop_assert!(CLASSES.contains(&e.class()), "{}", e.class()),
        }
    }

    #[test]
    fn archives_with_headers_that_lie_give_a_package_or_an_error(
        items in prop::collection::vec(lying_item(), 0..6),
        with_types in any::<bool>(),
    ) {
        let limits = small_limits();
        let mut all = if with_types { base_items() } else { vec![] };
        all.extend(items);
        let bytes = build(&all);
        match Package::read(&bytes, &limits) {
            Ok(p) => check_package(&p, &limits),
            Err(e) => prop_assert!(CLASSES.contains(&e.class()), "{}", e.class()),
        }
        // And with the limits the spec gives, which a small input does not reach, the same.
        match Package::read(&bytes, &Limits::default()) {
            Ok(p) => check_package(&p, &Limits::default()),
            Err(e) => prop_assert!(CLASSES.contains(&e.class()), "{}", e.class()),
        }
    }

    #[test]
    fn hostile_names_in_a_valid_archive_give_a_package_or_an_error(names in prop::collection::vec(hostile_name(), 1..6)) {
        let limits = small_limits();
        let mut items = base_items();
        for n in &names {
            items.push(Item::stored(n, b"x"));
        }
        match Package::read(&build(&items), &limits) {
            Ok(p) => check_package(&p, &limits),
            Err(e) => prop_assert!(CLASSES.contains(&e.class()), "{}", e.class()),
        }
    }

    #[test]
    fn a_target_resolves_to_a_part_name_or_to_nothing(source in hostile_name(), target in hostile_name()) {
        if let Some(name) = resolve(&source, &target) {
            prop_assert!(name.starts_with('/') && name.len() > 1);
            prop_assert!(!name.split('/').skip(1).any(|s| s.is_empty() || s == ".." || s == "." || s.ends_with('.')), "{:?}", name);
            prop_assert!(!name.contains('\\') && !name.chars().any(char::is_control), "{:?}", name);
            prop_assert!(name.len() <= diacraft_opc::MAX_PART_NAME_BYTES);
        }
        let rels = rels_part_name(&source);
        prop_assert!(rels.starts_with('/') && rels.ends_with(".rels"));
        let key = fold(&source);
        prop_assert!(key.starts_with('/') && !key.chars().any(|c| c.is_ascii_uppercase()));
        prop_assert_eq!(fold(&key), key.clone(), "folding twice is folding once");
    }

    #[test]
    fn a_relationships_part_of_any_bytes_gives_rows_or_an_error(content in prop::collection::vec(any::<u8>(), 0..2000)) {
        let mut items = base_items();
        items.push(Item::stored("a.xml", b"<a/>"));
        items.push(Item::stored("_rels/a.xml.rels", &content));
        if let Ok(p) = Package::read(&build(&items), &Limits::default()) {
            match p.rels("/a.xml") {
                Ok(r) => prop_assert!(r.items.len() <= diacraft_opc::MAX_RELATIONSHIPS),
                Err(e) => prop_assert!(CLASSES.contains(&e.class())),
            }
        }
    }

    #[test]
    fn any_bytes_give_a_tree_or_an_error(bytes in prop::collection::vec(any::<u8>(), 0..2000)) {
        let limits = XmlLimits { max_depth: 12, max_nodes: 300, max_attr_bytes: 100 };
        match parse_with(&bytes, &limits) {
            Ok(el) => check_tree(&el, &limits),
            Err(e) => prop_assert!(CLASSES.contains(&e.class())),
        }
    }

    #[test]
    fn xml_made_of_markup_pieces_gives_a_tree_or_an_error(pieces in prop::collection::vec(xml_piece(), 0..60)) {
        let text: String = pieces.concat();
        let limits = XmlLimits { max_depth: 12, max_nodes: 300, max_attr_bytes: 100 };
        match parse_with(text.as_bytes(), &limits) {
            Ok(el) => check_tree(&el, &limits),
            Err(e) => prop_assert!(CLASSES.contains(&e.class())),
        }
        // Whatever the limits, the same input never panics with the defaults.
        let _ = parse(text.as_bytes());
    }

    #[test]
    fn well_nested_xml_always_reads_and_keeps_its_attribute_order(
        names in prop::collection::vec("[a-z]{1,4}", 1..6),
        values in prop::collection::vec("[a-zA-Z0-9 .#!]{0,8}", 0..6),
    ) {
        let mut seen = std::collections::HashSet::new();
        let attrs: Vec<(String, &String)> = names.iter().zip(&values).filter(|(n, _)| seen.insert((*n).clone())).map(|(n, v)| (format!("{n}x"), v)).collect();
        let attr_text: String = attrs.iter().map(|(n, v)| format!(" {n}=\"{v}\"")).collect();
        let el = parse(format!("<r{attr_text}><c/></r>").as_bytes()).unwrap();
        let got: Vec<(&str, &str)> = el.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let want: Vec<(&str, &str)> = attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        prop_assert_eq!(got, want);
    }
}

/// A piece of XML markup, some well-formed and some not.
fn xml_piece() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => "<[a-z]{1,3}>".prop_map(|s| s),
        4 => "</[a-z]{1,3}>".prop_map(|s| s),
        3 => "<[a-z]{1,3}/>".prop_map(|s| s),
        3 => "<[a-z]{1,3} [a-z]{1,3}=\"[a-z0-9&;#]{0,6}\">".prop_map(|s| s),
        2 => "[a-z ]{0,6}".prop_map(|s| s),
        1 => Just("&amp;".to_owned()),
        1 => Just("&#65;".to_owned()),
        1 => Just("&bogus;".to_owned()),
        1 => Just("&".to_owned()),
        1 => Just("<!-- c -->".to_owned()),
        1 => Just("<![CDATA[x]]>".to_owned()),
        1 => Just("<?pi d?>".to_owned()),
        1 => Just("<?xml version=\"1.0\"?>".to_owned()),
        1 => Just("<!DOCTYPE a [<!ENTITY e \"v\">]>".to_owned()),
        1 => Just("\r\n".to_owned()),
        1 => Just("<".to_owned()),
        1 => Just(">".to_owned()),
    ]
}

/// What every tree that parses must hold.
fn check_tree(root: &El, limits: &XmlLimits) {
    fn walk(el: &El, depth: usize, limits: &XmlLimits, count: &mut usize) {
        assert!(depth <= limits.max_depth, "depth {depth}");
        *count += 1 + el.attrs.len();
        let mut seen = std::collections::HashSet::new();
        for (k, v) in &el.attrs {
            assert!(k.len() <= limits.max_attr_bytes && v.len() <= limits.max_attr_bytes);
            assert!(seen.insert(k.clone()), "duplicate attribute {k}");
        }
        assert!(!el.name.is_empty() && !el.name.contains(['<', '>', '&', ' ']), "{:?}", el.name);
        let mut last_text = false;
        for c in &el.children {
            *count += 1;
            match c {
                Node::El(e) => {
                    *count -= 1;
                    walk(e, depth + 1, limits, count);
                    last_text = false;
                }
                Node::Text(t) => {
                    assert!(!last_text, "two text nodes in a row");
                    assert!(!t.is_empty());
                    last_text = true;
                }
                _ => last_text = false,
            }
        }
    }
    let mut count = 0;
    walk(root, 1, limits, &mut count);
    assert!(count <= limits.max_nodes, "{count} items");
}
