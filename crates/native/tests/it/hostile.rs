//! Hostile inputs: the caps, refused in words and never as an abort or a panic (the public spec's 6.2, the plan's `hostile.rs`). Each crafted input is built by this code.
use diacraft_native::{Limits, NativeError, WriteOptions, read, sniff, write};

use crate::util::*;
use crate::zipbuild::Item;

/// Vector 1's entries with `extra` blob entries added, each listed in the manifest (and named by no record): `total` entries in all.
fn with_entries(total: usize) -> Vec<Item> {
    let mut items = vector_1();
    let extra = total - items.len();
    let mut rows =
        vec![("document.xml".to_owned(), "document".to_owned(), String::new()), ("pages/page1.xml".to_owned(), "page".to_owned(), String::new())];
    rows.extend((0..extra).map(|i| (format!("b/{i}"), "blob".to_owned(), String::new())));
    rows.push(("state.xml".to_owned(), "state".to_owned(), String::new()));
    let borrowed: Vec<(&str, &str, &str)> = rows.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str())).collect();
    replace(&mut items, "manifest.xml", manifest("1.0", "drawing", &borrowed));
    items.extend((0..extra).map(|i| Item::stored(&format!("b/{i}"), b"x")));
    items
}

#[test]
fn fifty_thousand_entries_are_counted_and_one_more_is_refused_before_inflating() {
    // 50,000 entries open (the extra blobs, which no record names, are left out and counted).
    let items = with_entries(50_000);
    assert_eq!(items.len(), 50_000);
    let (doc, opened) = open_ok(&package(&items));
    assert_eq!(doc.pages.len(), 1);
    assert_eq!(opened.notes, [diacraft_native::ReadNote::UnusedBlob { count: 49_995 }]);
    assert_eq!(opened.work.rows, 50_000);
    // One more is refused by the count, from the directory, before an entry is inflated: the checksum of the first entry is wrong, and it is not what is reported.
    let mut items = with_entries(50_001);
    items[0].crc ^= 1;
    let error = refused(&package(&items));
    assert_eq!(error.class(), "too_many_parts");
    assert!(error.to_string().contains("50000"), "{error}");
    // The same count is the limit, and the limit is the caller's: ten entries.
    let mut small = limits();
    small.container.max_parts = 10;
    assert!(read(&package(&with_entries(10)), &small).is_ok());
    assert_eq!(read(&package(&with_entries(11)), &small).unwrap_err().class(), "too_many_parts");
}

#[test]
fn a_manifest_that_lists_more_rows_than_a_package_may_hold_is_refused_before_they_are_stored() {
    // 50,001 rows for entries that are not there (blob rows are the only rows that may name nothing): refused at the 50,001st, in words.
    let rows: Vec<(String, String, String)> = (0..50_001).map(|i| (format!("b/{i}"), "blob".to_owned(), String::new())).collect();
    let borrowed: Vec<(&str, &str, &str)> = rows.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str())).collect();
    let mut items = vector_1();
    replace(&mut items, "manifest.xml", manifest("1.0", "drawing", &borrowed));
    let error = refused(&package(&items));
    assert_eq!(error.class(), "too_many_parts");
}

#[test]
fn a_manifest_that_lies_about_sizes_is_damage() {
    // The directory states a size that is not what the entry inflates to: damaged, the entry's own claim checked against what came out.
    for (name, claimed) in [("document.xml", 10u64), ("pages/page1.xml", 1_000_000), ("manifest.xml", 1), ("state.xml", 7)] {
        let mut items = vector_1();
        let at = items.iter().position(|i| i.name == name.as_bytes()).unwrap();
        items[at] = items[at].clone().claiming_size(claimed);
        let error = refused(&package(&items));
        assert!(["zip_corrupt", "zip_ratio_exceeded", "part_too_large"].contains(&error.class()), "{name} claiming {claimed}: {error}");
    }
    // A size past every cap, claimed by the directory: refused from the headers, with no byte inflated.
    let mut items = vector_1();
    items[2] = items[2].clone().claiming_size(4_000_000_000);
    assert!(["zip_ratio_exceeded", "part_too_large", "package_too_large"].contains(&refused(&package(&items)).class()));
    // The document part's blob records state a size that is not the entry's: the record is the lie. Smaller than the entry: a lost blob. Larger than any pool: refused in
    // words before the blob is fetched.
    let record = |size: &str| {
        let mut items = vector_5();
        let document = String::from_utf8(entry(&package(&items), "document.xml")).unwrap().replace("size=\"12\"", &format!("size=\"{size}\""));
        replace(&mut items, "document.xml", document.into_bytes());
        package(&items)
    };
    let (_, opened) = open_ok(&record("5"));
    assert_eq!(opened.notes, [diacraft_native::ReadNote::PictureLost { count: 1 }]);
    let (_, opened) = open_ok(&record("13"));
    assert_eq!(opened.notes, [diacraft_native::ReadNote::PictureLost { count: 1 }]);
    let error = refused(&record("1099511627776"));
    assert_eq!(error.class(), "model_too_complex");
    assert!(error.to_string().contains("pictures"), "words that say what is large: {error}");
    // Many blobs whose records add up past the pool, each small as an entry: refused by the sum, never fetched.
    let sum = {
        let mut items = vector_5();
        let blob = "<diacraft:blob path=\"blobs/0.png\" media-type=\"image/png\" size=\"12\" stamp=\"71df7ba063b3b4da\"/>";
        let many: String = (0..5)
            .map(|i| {
                format!("<diacraft:blob id=\"{i}\" path=\"blobs/0.png\" media-type=\"image/png\" size=\"100000000\" stamp=\"71df7ba063b3b4da\"/>")
            })
            .collect();
        let document = String::from_utf8(entry(&package(&items), "document.xml")).unwrap().replace(blob, &many);
        replace(&mut items, "document.xml", document.into_bytes());
        package(&items)
    };
    assert_eq!(refused(&sum).class(), "model_too_complex");
}

/// A page of `shapes` empty shapes: the items of its tree are the root's four (the element, the namespace declaration, `id` and `name`) and one for each shape.
fn page_of_empty_shapes(shapes: usize) -> Vec<u8> {
    part(&format!("<diacraft:page xmlns:diacraft=\"{NS}\" id=\"1\" name=\"Page-1\">{}</diacraft:page>", "<diacraft:shape/>".repeat(shapes)))
}

/// A page of one shape that holds `elements` elements of its own: a child of the page's root of `elements` and two items.
fn page_of_one_shape_with(elements: usize) -> Vec<u8> {
    part(&format!(
        "<diacraft:page xmlns:diacraft=\"{NS}\" id=\"1\" name=\"Page-1\"><diacraft:shape id=\"1\">{}</diacraft:shape></diacraft:page>",
        "<diacraft:x/>".repeat(elements)
    ))
}

#[test]
fn a_model_part_at_the_node_cap() {
    let mut small = limits();
    small.container.xml.max_nodes = 1_000;
    let read_page = |bytes: Vec<u8>| {
        let mut items = vector_1();
        replace(&mut items, "pages/page1.xml", bytes);
        read(&package(&items), &small)
    };
    // A page is read as a stream of the children of its root: the cap of items is a CHILD's, and a page of any number of shapes is one the reader takes.
    let (doc, _) = read_page(page_of_empty_shapes(5_000)).unwrap();
    assert_eq!(doc.pages.get(0).unwrap().shapes.len(), 5_000);
    // A child of 1,000 items (the shape, its attribute and 998 elements): the cap, taken. One more is refused, in words that say what is large.
    let (doc, _) = read_page(page_of_one_shape_with(998)).unwrap();
    assert_eq!(doc.pages.get(0).unwrap().shapes.len(), 1);
    let error = read_page(page_of_one_shape_with(999)).unwrap_err();
    assert_eq!(error.class(), "model_too_complex");
    assert!(error.to_string().contains("1000") && error.to_string().contains("XML items in one element of a part"), "{error}");
    // The same cap holds for the document part and for the manifest and the state.
    let mut items = vector_1();
    replace(&mut items, "state.xml", part(&format!("<diacraft:state xmlns:diacraft=\"{NS}\">{}</diacraft:state>", "<diacraft:x/>".repeat(1_000))));
    assert_eq!(read(&package(&items), &small).unwrap_err().class(), "xml_too_complex");
    // The sum of the items of every tree is held too: pages that each fit, and together do not, are refused at the item that passes the sum of an open.
    let mut tight = small.clone();
    tight.container.max_package_bytes = 800;
    let mut items = vector_1();
    replace(&mut items, "pages/page1.xml", page_of_empty_shapes(10));
    let error = read(&package(&items), &tight).unwrap_err();
    assert!(["model_too_complex", "package_too_large", "part_too_large", "zip_ratio_exceeded"].contains(&error.class()), "{error}");
}

#[test]
fn a_part_over_the_cap_of_an_entry_is_refused_and_one_at_the_cap_is_read() {
    // The cap of an XML part, here 3,000 bytes: a page of exactly that is read, a page of one byte more is refused, in words.
    let mut small = limits();
    small.container.max_part_bytes = 3_000;
    let exact = |len: usize| {
        let pad =
            len - page_with_start(&format!("<diacraft:page xmlns:diacraft=\"{NS}\" id=\"1\" name=\"Page-1\" name-u=\"Page-1\" x=\"\">"), "").len();
        page_with_start(&format!("<diacraft:page xmlns:diacraft=\"{NS}\" id=\"1\" name=\"Page-1\" name-u=\"Page-1\" x=\"{}\">", "n".repeat(pad)), "")
    };
    for (len, ok) in [(2_999, true), (3_000, true), (3_001, false)] {
        let page = exact(len);
        assert_eq!(page.len(), len);
        let mut items = vector_1();
        replace(&mut items, "pages/page1.xml", page);
        let result = read(&package(&items), &small);
        if ok {
            assert!(result.is_ok(), "{len}: {:?}", result.err());
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.class(), "part_too_large", "{len}");
            assert!(error.to_string().contains("3000"), "{error}");
        }
    }
    // A page that the document lists under another name is held to the cap of the memory the open leaves, from the size its entry states, before it is inflated.
    let page = exact(3_000);
    let mut items = vector_1();
    replace(&mut items, "pages/page1.xml", page_1(""));
    let document = String::from_utf8(entry(&package(&items), "document.xml")).unwrap().replace("pages/page1.xml", "weird/p.xml");
    replace(&mut items, "document.xml", document.into_bytes());
    replace(
        &mut items,
        "manifest.xml",
        manifest("1.0", "drawing", &[("document.xml", "document", ""), ("weird/p.xml", "page", ""), ("state.xml", "state", "")]),
    );
    items[3] = Item::deflated("weird/p.xml", &page);
    let mut tiny = limits();
    tiny.max_memory = 3_000_000;
    let result = read(&package(&items), &tiny);
    assert!(result.is_ok() || result.as_ref().err().is_some_and(|e| e.class() == "too_large"), "{:?}", result.err());
    assert!(read(&package(&items), &limits()).is_ok(), "the page under its own name reads when the memory is there");
}

#[test]
fn a_file_over_the_cap_is_refused_before_reading() {
    let bytes = package(&vector_1());
    let mut cap = limits();
    cap.max_file_bytes = bytes.len() as u64;
    assert!(read(&bytes, &cap).is_ok(), "a file as long as the cap is read");
    cap.max_file_bytes = bytes.len() as u64 - 1;
    let error = read(&bytes, &cap).unwrap_err();
    assert_eq!(error.class(), "too_large");
    assert_eq!(error, NativeError::TooLarge("the file is larger than a DiaCraft file may be"));
    // Before anything is read: bytes that are no package at all, longer than the cap, are `too_large` and not `not_native`.
    cap.max_file_bytes = 100;
    assert_eq!(read(&[0u8; 101], &cap).unwrap_err().class(), "too_large");
    assert_eq!(read(&[0u8; 100], &cap).unwrap_err().class(), "not_native");
    // The default cap is the package cap and 64 MiB of the container's own overhead (the spec's 6.4).
    assert_eq!(Limits::default().max_file_bytes, (512 + 64) << 20);
}

#[test]
fn a_read_that_the_memory_cannot_hold_is_refused_in_words_and_a_smaller_memory_never_opens_more() {
    // A drawing with a picture of a megabyte, read at memory from 0 to 4 GB: once it opens, every larger memory opens it too, and each refusal says what is wanted.
    let doc = drawing_of(vec![page_showing(1, &[&patterned(1, 1 << 20)])]);
    let bytes = write_doc(&doc);
    let mut opened = false;
    let mut memory = 0u64;
    let mut refusals = 0;
    while memory < 4_000_000_000 {
        let result = read(&bytes, &Limits { max_memory: memory, ..Limits::default() });
        match result {
            Ok(_) => opened = true,
            Err(e) => {
                assert!(!opened, "{memory}: refused after a smaller memory opened it: {e}");
                assert!(
                    ["too_large", "model_too_complex", "part_too_large", "package_too_large", "xml_too_complex", "zip_ratio_exceeded"]
                        .contains(&e.class()),
                    "{memory}: {e}"
                );
                assert!(!e.to_string().is_empty());
                refusals += 1;
            }
        }
        memory = if memory == 0 { 1 << 10 } else { memory + memory / 4 + 1 };
    }
    assert!(opened && refusals > 5, "a read opens when the memory is there and is refused below it");
    // `Limits::beside` gives what is left of the budget after what the process holds, and never less than nothing.
    assert_eq!(Limits::beside(0).max_memory, diacraft_native::MAX_MEMORY);
    assert_eq!(Limits::beside(1 << 30).max_memory, diacraft_native::MAX_MEMORY - (1 << 30));
    assert_eq!(Limits::beside(u64::MAX).max_memory, 0);
    assert_eq!(Limits::beside(u64::MAX).container, Limits::default().container);
}

#[test]
fn a_package_of_a_hostile_directory_is_refused_by_the_container_in_words() {
    let sound = package(&vector_1());
    // Entries that overlap, a directory that counts more than it holds, names that leave the package, names that are not names.
    let mut items = vector_1();
    items.push(Item::stored("../evil", b"x"));
    assert_eq!(refused(&package(&items)).class(), "path_traversal");
    for name in ["/abs", "a\\b", "C:x", "a//b", "a/./b", "a/../b"] {
        let mut items = vector_1();
        items.push(Item::stored(name, b"x"));
        let class = refused(&package(&items)).class();
        assert!(["path_traversal", "invalid_part_name"].contains(&class), "{name}: {class}");
    }
    let mut items = vector_1();
    items.push(Item::stored("é.txt", b"x"));
    assert_eq!(refused(&package(&items)).class(), "invalid_part_name", "a name that is not ASCII, with no UTF-8 flag");
    let mut items = vector_1();
    items.push(Item::stored("é.txt", b"x").with_flags(0x0800));
    assert_eq!(refused(&package(&items)).class(), "entry_unlisted", "with the flag it is a name, and the manifest does not list it");
    // Two entries of one name.
    let mut items = vector_1();
    items.push(Item::stored("state.xml", b"x"));
    assert_eq!(refused(&package(&items)).class(), "duplicate_part");
    // Encrypted, and a method that is not stored or deflate.
    let mut items = vector_1();
    items[4] = items[4].clone().with_flags(1);
    assert_eq!(refused(&package(&items)).class(), "encrypted_package");
    let mut items = vector_1();
    items[4] = items[4].clone().with_method(12);
    assert_eq!(refused(&package(&items)).class(), "unsupported_compression");
    // Truncations and a flipped byte never panic.
    for cut in (0..sound.len()).step_by(7) {
        let _ = read(&sound[..cut], &limits());
    }
    let _ = sniff(&sound);
}

#[test]
fn mutated_packages_are_refused_in_words_or_read_and_never_panic() {
    // A seeded fuzz in the default run (the libFuzzer targets in `fuzz/` are the long one): every byte of valid packages is flipped, truncated or repeated, and
    // each result is a refusal with words or a model that writes and reads back.
    let (p, q) = (png(1, 40), png(2, 40));
    let doc = drawing_of(vec![page_showing(1, &[&p, &q]), page_showing(2, &[&p])]);
    let seeds = [package(&vector_1()), package(&vector_2()), package(&vector_5()), write_doc(&doc)];
    let mut rng = Rng(0xD1AC_4AF7);
    let mut read_ok = 0;
    for round in 0..1_500 {
        let mut bytes = seeds[round % seeds.len()].clone();
        for _ in 0..1 + rng.below(3) {
            let at = rng.below(bytes.len());
            match rng.below(5) {
                0 => bytes[at] ^= 1 << rng.below(8),
                1 => bytes[at] = rng.next() as u8,
                2 => bytes.truncate(at.max(1)),
                3 => {
                    let end = (at + rng.below(64)).min(bytes.len());
                    let piece = bytes[at..end].to_vec();
                    bytes.splice(at..at, piece);
                }
                _ => {
                    bytes.remove(at);
                }
            }
        }
        match read(&bytes, &limits()) {
            Ok((model, opened)) => {
                read_ok += 1;
                // What read writes, and reads back as it went.
                let written = write(&model, &opened.state, None, &WriteOptions::default());
                if let Ok(written) = written {
                    let (back, _) = open_ok(&written.bytes);
                    assert!(back == model, "round {round}: a model that was read comes back from its package: {:?}", first_difference(&back, &model));
                }
            }
            Err(e) => assert!(!e.to_string().is_empty() && !e.class().is_empty(), "round {round}"),
        }
    }
    assert!(read_ok > 0, "some mutations leave a package that reads (a flipped byte in a name of a blob or in the bytes of a picture)");
}

#[test]
fn every_error_has_words_and_a_class() {
    let errors = [
        NativeError::NotNative,
        NativeError::Damaged("kind_mismatch"),
        NativeError::NewerMajor { major: 2, minor: 0 },
        NativeError::RequiredRole("x".to_owned()),
        NativeError::TooLarge("the file is larger than a DiaCraft file may be"),
        NativeError::Unwritable("why"),
    ];
    for e in errors {
        assert!(!e.to_string().is_empty() && !e.class().is_empty());
    }
    // Every class of damage has its sentence.
    for class in [
        "mimetype_bad",
        "kind_unknown",
        "kind_mismatch",
        "manifest_missing",
        "manifest_malformed",
        "manifest_version",
        "manifest_kind",
        "manifest_row",
        "entry_unlisted",
        "entry_missing",
        "entry_listed_twice",
        "document_missing",
        "part_missing",
        "part_unlisted",
        "state_malformed",
    ] {
        let words = NativeError::Damaged(class).to_string();
        assert!(words.starts_with("the file is damaged: ") && !words.contains("a part of it is not what the format says"), "{class}: {words}");
    }
}

#[test]
fn the_names_of_a_file_never_reach_a_message() {
    // A hostile name in a package is escaped, and a role word is cut: no control character or terminal escape reaches the words a person is shown.
    let mut items = vector_1();
    items.push(Item::stored("a\u{1b}[2J\n.txt", b"x").with_flags(0x0800));
    let error = refused(&package(&items));
    assert!(!error.to_string().contains('\u{1b}') && !error.to_string().contains('\n'), "{error:?}");
}

#[test]
#[ignore = "writes the seed corpus of the fuzz targets into the folder named by NATIVE_SEED_DIR"]
fn write_fuzz_seeds() {
    let dir = std::env::var("NATIVE_SEED_DIR").expect("NATIVE_SEED_DIR names a folder");
    let (p, q) = (png(1, 40), png(2, 40));
    let doc = drawing_of(vec![page_showing(1, &[&p, &q]), page_showing(2, &[&p])]);
    let seeds = [
        ("vector1", package(&vector_1())),
        ("vector2", package(&vector_2())),
        ("vector5", package(&vector_5())),
        ("drawing", write_doc(&doc)),
        ("blank", write_doc(&diacraft_doc::Document::new_blank())),
    ];
    for (name, bytes) in seeds {
        std::fs::write(format!("{dir}/{name}.diacraft"), bytes).expect("a seed is written");
    }
}

#[test]
fn the_items_of_every_part_are_summed_and_held_to_what_an_open_may_read() {
    // Pages that each fit the tree of a part, and together pass the items an open may read, are refused where the sum passes: the open's items are an eighth of the
    // package's bytes, here 5,000 of 40,000. Each page has 1,300 elements of one name that the format does not know, in the format's namespace.
    let mut small = limits();
    small.container.max_package_bytes = 40_000;
    let page = |i: usize| part(&format!("<page xmlns=\"{NS}\" id=\"{i}\">{}</page>", "<a/>".repeat(1_300)));
    let read_pages = |n: usize| {
        let pages: Vec<(String, Vec<u8>)> = (1..=n).map(|i| (format!("pages/page{i}.xml"), page(i))).collect();
        let page_refs: String = (1..=n).map(|i| format!("<diacraft:page-ref id=\"{i}\" part=\"pages/page{i}.xml\"/>")).collect();
        let mut rows = vec![("document.xml".to_owned(), "document"), ("state.xml".to_owned(), "state")];
        rows.extend(pages.iter().map(|(n, _)| (n.clone(), "page")));
        let borrowed: Vec<(&str, &str, &str)> = rows.iter().map(|(a, b)| (a.as_str(), *b, "")).collect();
        let mut items = vec![
            Item::stored("mimetype", MEDIA_DRAWING.as_bytes()),
            Item::deflated("manifest.xml", &manifest("1.0", "drawing", &borrowed)),
            Item::deflated("document.xml", &part(&format!("<diacraft:document xmlns:diacraft=\"{NS}\">{page_refs}</diacraft:document>"))),
            Item::deflated("state.xml", &state_empty()),
        ];
        items.extend(pages.iter().map(|(n, t)| Item::deflated(n, t)));
        read(&package(&items), &small)
    };
    assert!(read_pages(1).is_ok());
    let three = read_pages(3);
    assert!(three.is_ok(), "3,900 items and the few beside them: under the 5,000: {:?}", three.err().map(|e| e.to_string()));
    let error = read_pages(4).unwrap_err();
    assert_eq!(error.class(), "model_too_complex", "{error}");
    assert!(error.to_string().contains("items"), "{error}");
}
