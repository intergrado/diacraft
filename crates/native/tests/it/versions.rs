//! Versions (the public spec's section 5): a newer major is refused in words, a higher minor opens, and a part of a role this version does not know is kept.
use std::sync::Arc;

use diacraft_doc::Format;
use diacraft_native::{FORMAT, NativeError, Previous, ReadNote, WriteOptions, write};

use crate::util::*;
use crate::zipbuild::Item;

fn with_format(format: &str) -> Vec<u8> {
    let mut items = vector_1();
    replace(
        &mut items,
        "manifest.xml",
        manifest(format, "drawing", &[("document.xml", "document", ""), ("pages/page1.xml", "page", ""), ("state.xml", "state", "")]),
    );
    package(&items)
}

#[test]
fn a_newer_major_is_refused_in_words() {
    let error = refused(&with_format("2.0"));
    assert_eq!(error, NativeError::NewerMajor { major: 2, minor: 0 });
    // The words are exactly these (the spec's 5.1): they name the version and nothing from the file.
    assert_eq!(error.to_string(), "made by a newer DiaCraft (format 2.0)");
    assert_eq!(error.class(), "newer_major");
    assert_eq!(refused(&with_format("3.14")).to_string(), "made by a newer DiaCraft (format 3.14)");
    // A major this build does not know is any other, 0 included; and a number too large for a `u16` is still a version and is named.
    assert_eq!(refused(&with_format("0.5")).to_string(), "made by a newer DiaCraft (format 0.5)");
    assert_eq!(refused(&with_format("70000.1")).to_string(), "made by a newer DiaCraft (format 70000.1)");
}

#[test]
fn a_version_that_is_not_two_numbers_is_damage() {
    for bad in
        ["1", "1.", ".1", "1.0.0", "a.b", "1.-1", "+1.0", " 1.0", "1.0 ", "", "1,0", "1.0e1", "99999999999999999999.0", "1.99999999999999999999"]
    {
        let error = refused(&with_format(bad));
        assert_eq!(error.class(), "manifest_version", "{bad:?}: {error}");
    }
}

#[test]
fn a_higher_minor_opens() {
    // Vector 1 with the manifest `format="1.7"` opens, as Vector 1 (the spec's 8.3).
    let (doc, opened) = open_ok(&with_format("1.7"));
    assert_eq!(opened.format, (1, 7));
    assert_eq!(opened.kind, diacraft_native::Kind::Drawing);
    assert_eq!(doc.pages.len(), 1);
    // And a file of the version this build writes, and of 1.0, which every newer reader reads.
    assert_eq!(open_ok(&with_format("1.0")).1.format, (1, 0));
    assert_eq!(open_ok(&with_format(&format!("{}.{}", FORMAT.0, FORMAT.1))).1.format, FORMAT);
    assert_eq!(open_ok(&with_format("1.999999999")).1.format, (1, 999_999_999));
}

/// Vector 6: Vector 1 and a part of the role `annotation`, from a newer minor version.
fn vector_6(required: bool, role: &str) -> Vec<u8> {
    let mut items = vector_1();
    let req = if required { " required=\"1\"" } else { "" };
    replace(
        &mut items,
        "manifest.xml",
        manifest(
            "1.2",
            "drawing",
            &[
                ("document.xml", "document", ""),
                ("pages/page1.xml", "page", ""),
                ("state.xml", "state", ""),
                ("notes/review.txt", role, &format!(" media-type=\"text/plain\"{req}")),
            ],
        ),
    );
    items.push(Item::stored("notes/review.txt", b"check the margins"));
    package(&items)
}

#[test]
fn a_part_of_an_unknown_role_is_kept_by_raw_copy_at_every_save() {
    let bytes = vector_6(false, "annotation");
    let (doc, opened) = open_ok(&bytes);
    assert_eq!(opened.kept_parts, 1);
    let held: Vec<_> = doc.parts.iter().collect();
    assert_eq!(held.len(), 1);
    let part = held[0];
    assert_eq!(part.format, Format::Native);
    assert_eq!(&*part.name, "notes/review.txt");
    assert_eq!(&*part.media_type, "text/plain");
    assert_eq!(part.manifest.get("role"), Some("annotation"), "the role word stays with the part");
    assert_eq!(&*part.bytes, b"check the margins");
    // Written again, as a writer of this version does (the spec's vector 6): the part is an entry of its own at the same path, after `state.xml`, and nothing is written
    // for it in the document part.
    let written = write(&doc, &opened.state, Some(&Previous { bytes: &bytes, decoded: &opened.decoded }), &WriteOptions::default()).unwrap();
    assert_eq!(names(&written.bytes), ["mimetype", "manifest.xml", "document.xml", "pages/page1.xml", "state.xml", "notes/review.txt"]);
    assert_eq!(entry(&written.bytes, "notes/review.txt"), b"check the margins");
    assert!(!String::from_utf8(entry(&written.bytes, "document.xml")).unwrap().contains("notes/review"), "no part element names it");
    let manifest = String::from_utf8(entry(&written.bytes, "manifest.xml")).unwrap();
    assert!(manifest.contains(&format!("format=\"{}.{}\"", FORMAT.0, FORMAT.1)), "the writer's own version: {manifest}");
    assert!(manifest.contains("<diacraft:part path=\"notes/review.txt\" role=\"annotation\" media-type=\"text/plain\"/>"), "{manifest}");
    // The file was zipped by another tool (the headers of the ZIP builder are not this writer's), so the entry is written afresh, and the save after it copies it as it was.
    assert_eq!(written.copied, 0);
    let copied = write(&doc, &opened.state, Some(&Previous { bytes: &written.bytes, decoded: &written.decoded }), &WriteOptions::default()).unwrap();
    assert!(copied.copied >= 1, "the part is copied once the file is this writer's own");
    assert_eq!(copied.bytes, written.bytes);
    // Every later save keeps it, with and without the previous package.
    let again = write(&doc, &opened.state, None, &WriteOptions::default()).unwrap();
    assert_eq!(entry(&again.bytes, "notes/review.txt"), b"check the margins");
    let (back, reopened) = open_ok(&again.bytes);
    assert_eq!(reopened.kept_parts, 1);
    assert_eq!(back.parts.iter().next().unwrap().manifest.get("role"), Some("annotation"));
    // The part is held in the model, tagged `Native`, whatever the target of a later save is.
    assert!(doc.parts.iter().all(|p| p.format == Format::Native));
}

#[test]
fn the_other_attributes_of_an_extra_parts_row_come_back() {
    let mut items = vector_1();
    replace(
        &mut items,
        "manifest.xml",
        manifest(
            "1.2",
            "drawing",
            &[
                ("document.xml", "document", ""),
                ("pages/page1.xml", "page", ""),
                ("state.xml", "state", ""),
                ("x/y.bin", "layers", " depth=\"3\" label=\"a&amp;b\""),
            ],
        ),
    );
    items.push(Item::deflated("x/y.bin", &[7u8; 40]));
    let (doc, opened) = open_ok(&package(&items));
    let part = doc.parts.iter().next().unwrap();
    let pairs: Vec<_> = part.manifest.iter().collect();
    assert_eq!(pairs, [("role", "layers"), ("depth", "3"), ("label", "a&b")]);
    let written = write(&doc, &opened.state, None, &WriteOptions::default()).unwrap();
    let manifest = String::from_utf8(entry(&written.bytes, "manifest.xml")).unwrap();
    assert!(manifest.contains("role=\"layers\" depth=\"3\" label=\"a&amp;b\"/>"), "{manifest}");
    assert_eq!(entry(&written.bytes, "x/y.bin"), [7u8; 40]);
}

#[test]
fn a_required_part_of_an_unknown_role_is_refused_naming_the_role() {
    // Vector 6 with `required="1"` on the row of `notes/review.txt`, and the role `layers`.
    let error = refused(&vector_6(true, "layers"));
    assert_eq!(error, NativeError::RequiredRole("layers".to_owned()));
    assert!(error.to_string().contains("layers"), "{error}");
    assert_eq!(error.class(), "required_role");
    // A long role word is cut, and one with a line break or a tab is escaped.
    let error = refused(&vector_6(true, &"r".repeat(500)));
    assert!(error.to_string().matches('r').count() < 200, "cut: {error}");
    let error = refused(&vector_6(true, "a&#10;b&#9;c"));
    assert!(!error.to_string().contains('\n') && !error.to_string().contains('\t'), "escaped: {error:?}");
}

#[test]
fn an_extra_part_that_is_listed_and_missing_is_damage() {
    let mut items = vector_1();
    replace(
        &mut items,
        "manifest.xml",
        manifest(
            "1.2",
            "drawing",
            &[("document.xml", "document", ""), ("pages/page1.xml", "page", ""), ("state.xml", "state", ""), ("notes/review.txt", "annotation", "")],
        ),
    );
    assert_eq!(refused(&package(&items)).class(), "entry_missing");
}

#[test]
fn a_part_of_a_known_role_written_by_a_hand_built_model_is_not_written_as_an_extra() {
    // A model whose native part names a role this version reads as its own is refused in words, and so is one whose path is the path of an entry the package makes.
    let (mut doc, opened) = open_ok(&vector_6(false, "annotation"));
    let part = doc.parts.iter().next().unwrap().clone();
    let cases = [
        ("notes/review.txt", "page"),
        ("notes/review.txt", "document"),
        ("notes/review.txt", "state"),
        ("document.xml", "annotation"),
        ("pages/page1.xml", "annotation"),
        ("state.xml", "x"),
        ("preview.png", "x"),
        ("../x", "x"),
        ("/x", "x"),
        ("a//b", "x"),
        ("", "x"),
    ];
    for (path, role) in cases {
        let changed = diacraft_doc::Part { name: Arc::from(path), manifest: diacraft_doc::RawAttrs::from_pairs([("role", role)]), ..(*part).clone() };
        doc.parts = diacraft_doc::SharedVec::from_vec(vec![Arc::new(changed)]);
        let error = write(&doc, &opened.state, None, &WriteOptions::default()).unwrap_err();
        assert_eq!(error.class(), "unwritable", "{path} {role}: {error}");
    }
    // Two parts of one name are refused too.
    let one = (*part).clone();
    doc.parts = diacraft_doc::SharedVec::from_vec(vec![Arc::new(one.clone()), Arc::new(one)]);
    assert_eq!(write(&doc, &opened.state, None, &WriteOptions::default()).unwrap_err().class(), "unwritable");
}

#[test]
fn a_blob_entry_that_no_record_names_is_dropped_and_said_and_kept_in_a_newer_minor() {
    // A blob entry that no record names is ignored for the model. In a file of this version's minor it is dropped and the person is told; in a newer minor it is held as an
    // extra part and written again at its own path (2.6).
    let this = format!("{}.{}", FORMAT.0, FORMAT.1);
    for (format, kept) in [("1.0", false), (this.as_str(), false), ("1.9", true)] {
        let mut items = vector_1();
        replace(
            &mut items,
            "manifest.xml",
            manifest(
                format,
                "drawing",
                &[
                    ("document.xml", "document", ""),
                    ("pages/page1.xml", "page", ""),
                    ("blobs/9.bin", "blob", " media-type=\"application/octet-stream\""),
                    ("state.xml", "state", ""),
                ],
            ),
        );
        items.push(Item::deflated("blobs/9.bin", b"nobody names me"));
        let (doc, opened) = open_ok(&package(&items));
        assert_eq!(opened.kept_parts, usize::from(kept), "{format}");
        if kept {
            assert!(opened.notes.is_empty(), "{:?}", opened.notes);
            let written = write(&doc, &opened.state, None, &WriteOptions::default()).unwrap();
            assert_eq!(entry(&written.bytes, "blobs/9.bin"), b"nobody names me");
        } else {
            assert_eq!(opened.notes, [ReadNote::UnusedBlob { count: 1 }], "{format}");
            assert_eq!(opened.notes[0].to_string(), "1 entry that nothing in the file uses was left out");
            assert!(doc.parts.is_empty());
        }
    }
}

#[test]
fn a_manifest_that_lists_the_mimetype_is_damage() {
    let mut items = vector_1();
    replace(
        &mut items,
        "manifest.xml",
        manifest(
            "1.0",
            "drawing",
            &[("document.xml", "document", ""), ("pages/page1.xml", "page", ""), ("state.xml", "state", ""), ("mimetype", "other", "")],
        ),
    );
    assert_eq!(refused(&package(&items)).class(), "manifest_row");
}

#[test]
fn the_words_of_a_part_of_an_unknown_role_are_held_to_what_a_manifest_row_holds() {
    let (mut doc, opened) = open_ok(&vector_6(false, "annotation"));
    let part = doc.parts.iter().next().unwrap().clone();
    let words = [
        (diacraft_doc::RawAttrs::from_pairs([("role", "r".repeat(2_000).as_str())]), "text/plain".to_owned()),
        (diacraft_doc::RawAttrs::from_pairs([("role", "annotation")]), "t".repeat(2_000)),
        (diacraft_doc::RawAttrs::from_pairs([("role", "a\u{1}b")]), "text/plain".to_owned()),
        (diacraft_doc::RawAttrs::from_pairs([("role", "annotation")]), "text/\u{fffe}".to_owned()),
    ];
    for (manifest, media_type) in words {
        let changed = diacraft_doc::Part { manifest, media_type: Arc::from(media_type.as_str()), ..(*part).clone() };
        doc.parts = diacraft_doc::SharedVec::from_vec(vec![Arc::new(changed)]);
        assert_eq!(write(&doc, &opened.state, None, &WriteOptions::default()).unwrap_err().class(), "unwritable");
    }
}
