//! Reading a package: parts, names, case, bytes, and the first vectors.

use diacraft_opc::{Limits, OpcError, Package, Warning};

use crate::zipbuild::{Item, Opts, build, build_with, deflate_zeros, inflate_all};
use crate::{base_items, class_of, class_of_more, content_types_xml, package_with, read_items};

#[test]
fn the_test_stream_of_zeros_is_a_valid_deflate_stream_at_the_formats_own_ratio() {
    // The generator of the bombs below is checked against the inflater the crate uses, so that no test of a bomb rests on a
    // broken stream.
    for n in [1u64, 2, 100, 257, 258, 259, 1000, 258 * 1000 + 7] {
        let stream = deflate_zeros(n);
        let out = inflate_all(&stream);
        assert_eq!(out.len() as u64, n, "length for n = {n}");
        assert!(out.iter().all(|b| *b == 0), "all zeros for n = {n}");
    }
    let big = deflate_zeros(258 * 100_000 + 1);
    let ratio = (258 * 100_000 + 1) / big.len() as u64;
    assert!(ratio > 1000, "the generator must beat the cap of 1,000 to test it: it made {ratio}");
}

#[test]
fn parts_come_back_in_archive_order_with_the_case_they_arrived_with() {
    // Names compare without regard to ASCII case, and output keeps the case the part arrived with.
    let bytes = package_with(vec![
        Item::deflated("visio/Document.XML", b"<d/>"),
        Item::stored("docProps/App.xml", b"<a/>"),
        Item::stored("visio/media/IMAGE1.PNG", &[1, 2, 3]),
    ]);
    let p = Package::read(&bytes, &Limits::default()).unwrap();
    let names: Vec<&str> = p.parts().collect();
    assert_eq!(names, ["/visio/Document.XML", "/docProps/App.xml", "/visio/media/IMAGE1.PNG"]);
}

#[test]
fn a_part_is_found_with_or_without_its_slash_and_in_any_ascii_case() {
    let p = Package::read(&package_with(vec![Item::stored("visio/Document.XML", b"<d/>")]), &Limits::default()).unwrap();
    for name in ["visio/Document.XML", "/visio/Document.XML", "/visio/document.xml", "VISIO/DOCUMENT.XML", "/Visio/Document.Xml"] {
        assert_eq!(p.part(name), Some(&b"<d/>"[..]), "lookup of {name}");
    }
    for name in ["", "/", "visio", "/visio/document.xml/", "visio/document.xml.bak", "\\visio\\document.xml", "/visio/../visio/document.xml"] {
        assert_eq!(p.part(name), None, "lookup of {name:?} must not find the part: names are looked up as they are, with no path tricks");
    }
}

#[test]
fn part_bytes_come_back_exactly_as_stored() {
    // The reader hands back raw part bytes unchanged (vsdx-preservation R4 "Bytes" level): CR LF line ends, a byte order mark,
    // single-quoted attributes, trailing blanks, an XML part that is not well-formed, and bytes that are not text at all.
    let crlf = b"\xEF\xBB\xBF<?xml version='1.0' encoding='utf-8'?>\r\n<VisioDocument xmlns='x' xml:space='preserve'>\r\n  <a b='1'  c = \"2\" />\r\n</VisioDocument>\r\n  \r\n".to_vec();
    let broken = b"<a><b></a>".to_vec();
    let binary: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
    let p = Package::read(
        &package_with(vec![
            Item::deflated("visio/document.xml", &crlf),
            Item::stored("visio/broken.xml", &broken),
            Item::deflated("visio/vbaProject.bin", &binary),
            Item::stored("visio/empty.xml", b""),
        ]),
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(p.part("/visio/document.xml"), Some(&crlf[..]));
    assert_eq!(p.part("/visio/broken.xml"), Some(&broken[..]));
    assert_eq!(p.part("/visio/vbaProject.bin"), Some(&binary[..]));
    assert_eq!(p.part("/visio/empty.xml"), Some(&b""[..]), "an empty part is a part");
}

#[test]
fn stored_and_deflated_parts_read_the_same_way() {
    let content = b"the same bytes, the same bytes, the same bytes".repeat(20);
    let p = read_items(&[base_items().remove(0), Item::stored("a.bin", &content), Item::deflated("b.bin", &content)]).unwrap();
    assert_eq!(p.part("a.bin"), p.part("b.bin"));
    assert_eq!(p.part("a.bin"), Some(&content[..]));
}

#[test]
fn the_content_types_stream_is_not_a_part() {
    // It is a ZIP item and not a part.
    let p = Package::read(&package_with(vec![Item::stored("visio/document.xml", b"<d/>")]), &Limits::default()).unwrap();
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/visio/document.xml"]);
    assert_eq!(p.part("[Content_Types].xml"), None);
    assert_eq!(p.part("/[Content_Types].xml"), None);
}

#[test]
fn the_content_types_stream_may_be_in_any_ascii_case_and_in_any_place() {
    let ct = content_types_xml(&[("xml", "application/xml")], &[]);
    let items = vec![Item::stored("visio/document.xml", b"<d/>"), Item::stored("[CONTENT_types].XML", ct.as_bytes())];
    let p = read_items(&items).unwrap();
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/visio/document.xml"]);
    assert_eq!(p.content_type("/visio/document.xml"), Some("application/xml"));
}

#[test]
fn a_zip_with_no_content_types_stream_is_not_a_package() {
    let err = read_items(&[Item::stored("visio/document.xml", b"<d/>")]).unwrap_err();
    assert_eq!(err.class(), "missing_content_types");
}

#[test]
fn a_package_of_only_its_content_types_has_no_parts() {
    let p = read_items(&base_items()).unwrap();
    assert_eq!(p.parts().count(), 0);
    assert!(p.warnings().is_empty());
}

#[test]
fn two_items_that_differ_only_in_case_are_a_duplicate_part() {
    // `visio/document.xml` and `VISIO/DOCUMENT.XML`.
    assert_eq!(class_of_more(vec![Item::stored("visio/document.xml", b"1"), Item::stored("VISIO/DOCUMENT.XML", b"2")]), "duplicate_part");
    // The other order, and one letter apart.
    assert_eq!(class_of_more(vec![Item::stored("VISIO/DOCUMENT.XML", b"2"), Item::stored("visio/document.xml", b"1")]), "duplicate_part");
    assert_eq!(class_of_more(vec![Item::stored("a/b.xml", b"1"), Item::stored("a/B.xml", b"2")]), "duplicate_part");
}

#[test]
fn the_error_for_a_duplicate_names_both_items() {
    let err = read_items(&[
        base_items().remove(0),
        Item::stored("visio/document.xml", b"1"),
        Item::stored("other.xml", b"3"),
        Item::stored("Visio/Document.xml", b"2"),
    ])
    .unwrap_err();
    let text = err.to_string();
    assert!(text.contains("visio/document.xml") && text.contains("Visio/Document.xml"), "{text}");
    // The first is shown in the case it arrived with, and not as the folded key it is compared by.
    let err = read_items(&[base_items().remove(0), Item::stored("Visio/Document.xml", b"1"), Item::stored("visio/document.xml", b"2")]).unwrap_err();
    assert_eq!(err, OpcError::DuplicatePart { first: "/Visio/Document.xml".into(), second: "/visio/document.xml".into() });
    // A first that is a name as written behind a Unicode path field is not kept, so its folded key is what is shown, and not the part the
    // field gave the row.
    let err = read_items(&[base_items().remove(0), Item::stored("a.xml", b"1").with_unicode_path("b.xml"), Item::stored("A.xml", b"2")]).unwrap_err();
    assert_eq!(err, OpcError::DuplicatePart { first: "/a.xml".into(), second: "/A.xml".into() });
}

#[test]
fn a_unicode_path_past_three_times_the_name_limit_is_refused_by_its_first_64_characters() {
    // One byte past three times the limit: refused as the field's name, cut to 64 characters. At three times the limit the field passes
    // that check and the name is refused as a part name, whole.
    let past = "f".repeat(3 * 1024 + 1);
    let err = read_items(&[base_items().remove(0), Item::stored("a.xml", b"1").with_unicode_path(&past)]).unwrap_err();
    assert_eq!(err, OpcError::InvalidPartName { name: "f".repeat(64), reason: "longer than the limit" });
    let at = "f".repeat(3 * 1024);
    let err = read_items(&[base_items().remove(0), Item::stored("a.xml", b"1").with_unicode_path(&at)]).unwrap_err();
    assert_eq!(err, OpcError::InvalidPartName { name: at.clone(), reason: "longer than the limit" });
}

#[test]
fn the_same_item_name_twice_is_a_duplicate_part() {
    assert_eq!(class_of_more(vec![Item::stored("a.xml", b"1"), Item::stored("a.xml", b"2")]), "duplicate_part");
    let err = read_items(&[base_items().remove(0), Item::stored("a.xml", b"1"), Item::stored("a.xml", b"2")]).unwrap_err();
    assert!(err.to_string().matches("/a.xml").count() == 2, "both items are named: {err}");
}

#[test]
fn a_second_content_types_stream_is_a_duplicate_part_whatever_its_case_and_its_place() {
    // One stream, told by its name as written in any ASCII case. A second, in either spelling and before or after the first, is
    // refused with both names as written, and the package is never read with one of the two streams in place of the other (here the
    // second gives `a.xml` a type the first does not).
    let first_types = content_types_xml(&[("xml", "application/xml")], &[]);
    let second_types = content_types_xml(&[("xml", "text/plain")], &[]);
    let spellings = ["[Content_Types].xml", "[content_types].xml"];
    for (first, second) in [(spellings[0], spellings[1]), (spellings[1], spellings[0]), (spellings[0], spellings[0]), (spellings[1], spellings[1])] {
        let bytes =
            build(&[Item::deflated(first, first_types.as_bytes()), Item::stored("a.xml", b"<a/>"), Item::deflated(second, second_types.as_bytes())]);
        let error = Package::read(&bytes, &Limits::default()).unwrap_err();
        assert_eq!(error, OpcError::DuplicatePart { first: first.to_owned(), second: second.to_owned() }, "{first} then {second}");
    }
}

#[test]
fn a_document_relationship_to_any_name_resolves_and_finds_the_part() {
    // The half that is this crate's: a package relationship to `custom/main.xml`, and no part at
    // `visio/document.xml`. The relationship resolves to a part that is there. (Which part is *the document* is the `vsdx` crate's.)
    let rels = crate::rels_xml(&[("rId1", "http://schemas.microsoft.com/visio/2010/relationships/document", "custom/main.xml", None)]);
    let p = package_with(vec![Item::stored("_rels/.rels", rels.as_bytes()), Item::stored("custom/main.xml", b"<VisioDocument/>")]);
    let p = Package::read(&p, &Limits::default()).unwrap();
    let target = p.rels("/").unwrap().items[0].target.clone().unwrap();
    assert_eq!(target, "/custom/main.xml");
    assert_eq!(p.part(&target), Some(&b"<VisioDocument/>"[..]));
    assert_eq!(p.part("/visio/document.xml"), None);
}

#[test]
fn a_part_name_that_is_a_path_prefix_of_another_is_refused() {
    // `a` and `a/b`.
    assert_eq!(class_of_more(vec![Item::stored("a", b"1"), Item::stored("a/b", b"2")]), "part_name_prefix");
    assert_eq!(class_of_more(vec![Item::stored("a/b", b"2"), Item::stored("a", b"1")]), "part_name_prefix");
    // Any depth, and not only the nearest ancestor, and folded for case.
    assert_eq!(
        class_of_more(vec![Item::stored("x/y", b"1"), Item::stored("p/q/r/s.xml", b"2"), Item::stored("X/Y/z/w.xml", b"3")]),
        "part_name_prefix"
    );
    // The neighbour in a plain sort is not the one under it: `a-b` and `a.b` sort between `a` and `a/c`.
    assert_eq!(
        class_of_more(vec![Item::stored("a", b"1"), Item::stored("a-b", b"2"), Item::stored("a.b", b"3"), Item::stored("a/c", b"4")]),
        "part_name_prefix"
    );
    assert_eq!(
        class_of_more(vec![Item::stored("a/c", b"4"), Item::stored("a.b", b"3"), Item::stored("a-b", b"2"), Item::stored("a", b"1")]),
        "part_name_prefix"
    );
    // Names longer than the 64-byte blocks the comparison skips when two names share them (the blocks of the folded name, which starts with
    // a slash): `b…baaaaaa` and the part under it, and a name whose first block differs and whose tail would sort it between the two if
    // differing blocks were skipped too.
    let parent = format!("{}aaaaaa", "b".repeat(63));
    let (child, between) = (format!("{parent}/c.xml"), format!("{}aaaaaa/b", "a".repeat(63)));
    for order in [[&parent, &child, &between], [&between, &child, &parent], [&child, &between, &parent]] {
        let items = order.iter().map(|name| Item::stored(name, b"x")).collect();
        assert_eq!(class_of_more(items), "part_name_prefix", "{order:?}");
    }
    // A name that merely starts with another's letters is not under it.
    let p = read_items(&[base_items().remove(0), Item::stored("a", b"1"), Item::stored("ab/c", b"2"), Item::stored("a.b/c", b"3")]).unwrap();
    assert_eq!(p.parts().count(), 3);
}

#[test]
fn a_directory_item_is_not_a_part_and_is_reported() {
    // `visio/pages/` gives no part.
    let p = read_items(&[
        base_items().remove(0),
        Item::stored("visio/", b""),
        Item::stored("visio/pages/", b""),
        Item::stored("visio/pages/page1.xml", b"<p/>"),
    ])
    .unwrap();
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/visio/pages/page1.xml"]);
    assert_eq!(p.part("/visio/pages"), None);
    assert_eq!(p.part("/visio/pages/"), None);
    assert!(p.warnings().contains(&Warning::DirectoryItem { name: "visio/pages/".to_owned() }), "{:?}", p.warnings());
    // A directory item does not make its folder a part, so it cannot make a prefix of a part or collide with one.
    let p = read_items(&[base_items().remove(0), Item::stored("a/", b""), Item::stored("a/b", b"1")]).unwrap();
    assert_eq!(p.parts().count(), 1);
}

#[test]
fn a_directory_item_with_a_hostile_name_still_refuses_the_package() {
    assert_eq!(class_of_more(vec![Item::stored("../x/", b"")]), "path_traversal");
}

#[test]
fn a_percent_encoded_item_name_is_decoded_and_the_slash_added_back() {
    // Non-ASCII characters are percent-encoded in the ZIP item name; on read they are decoded.
    let p = read_items(&[base_items().remove(0), Item::stored("visio/media/image%201.png", b"1"), Item::stored("visio/media/caf%C3%A9.png", b"2")])
        .unwrap();
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/visio/media/image 1.png", "/visio/media/café.png"]);
    assert_eq!(p.part("/visio/media/image 1.png"), Some(&b"1"[..]));
    assert_eq!(p.part("/visio/media/café.png"), Some(&b"2"[..]));
    assert_eq!(p.part("/visio/media/CAFÉ.png"), None, "case folds ASCII only");
}

#[test]
fn a_name_given_literally_and_the_same_name_percent_encoded_are_one_part() {
    // Many writers put the UTF-8 name in the item as it is. Both spellings read; both together are a duplicate.
    let p = read_items(&[base_items().remove(0), Item::stored("caf\u{e9}.xml", b"1")]).unwrap();
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/café.xml"]);
    assert_eq!(class_of_more(vec![Item::stored("caf\u{e9}.xml", b"1"), Item::stored("caf%C3%A9.xml", b"2")]), "duplicate_part");
}

#[test]
fn a_name_that_is_not_utf_8_reads_as_code_page_437_as_zip_says() {
    // Without the UTF-8 flag the ZIP format says CP437: byte 0x82 is `é`. (`zipdir.rs` has the table; this pins it.)
    let p = read_items(&[base_items().remove(0), Item::stored("x", b"1").named_bytes(b"caf\x82.xml")]).unwrap();
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/café.xml"]);
}

#[test]
fn a_package_with_an_archive_comment_still_reads() {
    let mut items = base_items();
    items.push(Item::deflated("visio/document.xml", b"<d/>"));
    let p = Package::read(&build_with(&items, &Opts { comment: b"made by hand".to_vec(), ..Opts::default() }), &Limits::default()).unwrap();
    assert_eq!(p.part("/visio/document.xml"), Some(&b"<d/>"[..]));
}

#[test]
fn bytes_in_front_of_the_archive_are_refused_because_offsets_are_from_byte_zero() {
    // No shift is guessed. A self-extracting stub, a prefix, a single byte: the end record is found (its comment ends the
    // file) and its offsets reach nothing, so it is `zip_corrupt`; and with ZIP64 records the locator's offset is off too.
    let mut items = base_items();
    items.push(Item::deflated("visio/document.xml", b"<d/>"));
    for stub in [&b"M"[..], b"MZ stub bytes that are not a zip", &[0u8; 4096]] {
        for zip64 in [false, true] {
            let bytes = build_with(&items, &Opts { prefix: stub.to_vec(), zip64, ..Opts::default() });
            assert_eq!(class_of(&bytes, &Limits::default()), "zip_corrupt", "stub of {} bytes, zip64 {zip64}", stub.len());
        }
    }
    // And the same archive with no stub reads.
    assert!(Package::read(&build_with(&items, &Opts::default()), &Limits::default()).is_ok());
}

#[test]
fn many_parts_read_and_each_is_found_by_name() {
    let mut items = base_items();
    for i in 0..2000 {
        items.push(Item::stored(&format!("visio/pages/page{i}.xml"), format!("<page n='{i}'/>").as_bytes()));
    }
    let p = Package::read(&build(&items), &Limits::default()).unwrap();
    assert_eq!(p.parts().count(), 2000);
    for i in [0, 1, 999, 1999] {
        assert_eq!(p.part(&format!("/visio/pages/PAGE{i}.xml")), Some(format!("<page n='{i}'/>").as_bytes()));
    }
}

#[test]
fn a_part_taken_out_of_the_package_is_the_callers_and_the_package_has_none_of_it_any_more() {
    // The bytes move, they are not copied.
    let mut p = Package::read(
        &package_with(vec![
            Item::deflated("visio/Document.XML", b"<d/>"),
            Item::stored("visio/media/i.png", &[1, 2, 3]),
            Item::stored("x/y.bin", &[]),
        ]),
        &Limits::default(),
    )
    .unwrap();
    // Names are looked up as `part` looks them up: any ASCII case, with or without the slash.
    assert_eq!(p.take_part("/VISIO/MEDIA/I.PNG"), Some(vec![1, 2, 3]));
    assert_eq!(p.part("visio/media/i.png"), Some(&b""[..]), "the part stays, named and empty");
    assert_eq!(p.take_part("visio/media/i.png"), Some(Vec::new()), "a second take gets nothing");
    assert_eq!(p.take_part("visio/missing.bin"), None);
    assert_eq!(p.take_part(""), None);
    assert_eq!(p.take_part("x/y.bin"), Some(Vec::new()));
    // The others are untouched, and the names are all still there.
    assert_eq!(p.part("visio/document.xml"), Some(&b"<d/>"[..]));
    assert_eq!(p.parts().count(), 3);
}

#[test]
fn parts_the_caller_does_not_keep_are_let_go_and_the_ones_it_keeps_are_not() {
    let mut p = Package::read(
        &package_with(vec![Item::deflated("a.xml", b"<a/>"), Item::stored("b.bin", &[7u8; 100]), Item::stored("c.bin", &[8u8; 50])]),
        &Limits::default(),
    )
    .unwrap();
    let mut asked = Vec::new();
    p.release_parts(|name| {
        asked.push(name.to_owned());
        name == "/b.bin"
    });
    assert_eq!(asked, ["/a.xml", "/b.bin", "/c.bin"], "each part is asked once, by the name `parts` gives");
    assert_eq!((p.part("a.xml"), p.part("c.bin")), (Some(&b""[..]), Some(&b""[..])));
    assert_eq!(p.take_part("b.bin"), Some(vec![7u8; 100]));
}

#[test]
fn a_package_read_knowing_some_parts_shares_the_bytes_their_entries_hold_and_is_the_package_read_gives() {
    // A writer that reads back the file it made has the bytes of its big parts already; the read shares them where an entry holds them (their size and checksum)
    // and inflates every other part, so the package is the one `read` gives, byte for byte, with the big parts held once.
    let big: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
    let other: Vec<u8> = big.iter().map(|b| b ^ 1).collect();
    let bytes = package_with(vec![
        Item::deflated("visio/document.xml", b"<d/>"),
        Item::stored("visio/embeddings/A.bin", &big),
        Item::deflated("visio/embeddings/b.bin", &big),
    ]);
    let shared: std::sync::Arc<[u8]> = std::sync::Arc::from(big.clone());
    let wrong: std::sync::Arc<[u8]> = std::sync::Arc::from(other);
    let known = |name: &str| match name {
        "/visio/embeddings/A.bin" => Some(std::sync::Arc::clone(&shared)),
        "/visio/embeddings/b.bin" => Some(std::sync::Arc::clone(&wrong)),
        _ => None,
    };
    let plain = Package::read(&bytes, &Limits::default()).unwrap();
    let mut knowing = Package::read_knowing(&bytes, &Limits::default(), &known).unwrap();
    let names: Vec<&str> = plain.parts().collect();
    assert_eq!(knowing.parts().collect::<Vec<_>>(), names);
    for name in &names {
        assert_eq!(knowing.part(name), plain.part(name), "{name}");
    }
    assert_eq!(knowing.content_types_bytes(), plain.content_types_bytes());
    // The part whose entry holds the caller's bytes is those bytes; the one whose entry holds others is the file's own, inflated.
    assert_eq!(knowing.part("visio/embeddings/a.bin").map(<[u8]>::as_ptr), Some(shared.as_ptr()), "shared, not a copy");
    assert_ne!(knowing.part("visio/embeddings/b.bin").map(<[u8]>::as_ptr), Some(wrong.as_ptr()));
    assert_eq!(knowing.part("visio/embeddings/b.bin"), Some(&big[..]));
    // A shared part given up is a copy of the bytes, and the caller's are untouched; a part let go of drops its share.
    assert_eq!(knowing.take_part("visio/embeddings/a.bin"), Some(big.clone()));
    assert_eq!(knowing.part("visio/embeddings/a.bin"), Some(&b""[..]));
    let mut again = Package::read_knowing(&bytes, &Limits::default(), &known).unwrap();
    assert_eq!(std::sync::Arc::strong_count(&shared), 2, "the package holds one share");
    again.release_parts(|_| false);
    assert_eq!(std::sync::Arc::strong_count(&shared), 1, "and lets go of it");
    // What `read` refuses, a read that knows the parts refuses with the same error: a damaged entry of a known part is damage.
    let mut items = crate::base_items();
    let mut bad = Item::stored("visio/embeddings/A.bin", &big);
    bad.crc ^= 1;
    items.push(bad);
    let damaged = build(&items);
    assert_eq!(Package::read_knowing(&damaged, &Limits::default(), &known).unwrap_err(), Package::read(&damaged, &Limits::default()).unwrap_err());
}
