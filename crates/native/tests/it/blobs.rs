//! Blobs: each distinct byte string once, numbered by first use, stored or deflated by what it starts with, and lost without refusing the file (the public spec's 2.6,
//! 2.10 and 3.16).
use std::sync::Arc;

use diacraft_container::zip::Method;
use diacraft_doc::{Document, Format, Part, PartRole, RawAttrs, SharedVec, Theme};
use diacraft_native::{BlobTable, ReadNote, WriteOptions, stored, write};
use diacraft_sheetxml::model::{BlobId, BlobSink, Stamp};

use crate::util::*;
use crate::zipbuild::Item;

fn blob_entries(bytes: &[u8]) -> Vec<String> {
    names(bytes).into_iter().filter(|n| n.starts_with("blobs/")).collect()
}

#[test]
fn equal_bytes_are_stored_once() {
    // Two pages show equal bytes in distinct allocations: one `blobs/` entry, and every shape that names it is given one copy of its bytes.
    let (a, b) = (png(1, 64), png(1, 64));
    assert!(!Arc::ptr_eq(&a, &b) && a == b);
    let doc = drawing_of(vec![page_showing(1, &[&a]), page_showing(2, &[&b, &a])]);
    let bytes = write_doc(&doc);
    assert_eq!(blob_entries(&bytes), ["blobs/0.png"]);
    let (back, opened) = open_ok(&bytes);
    let shown: Vec<Arc<[u8]>> = [(0, 0), (1, 0), (1, 1)].iter().map(|(p, s)| picture_of(&back, *p, *s).unwrap()).collect();
    assert!(shown.iter().all(|s| **s == *a));
    assert!(Arc::ptr_eq(&shown[0], &shown[1]) && Arc::ptr_eq(&shown[1], &shown[2]), "one copy of the bytes for every shape that names the blob");
    assert_eq!(opened.decoded.blobs(), 1);
    // The record says what the entry holds (3.16): the media type of the first use, the size, and the stamp.
    let document = String::from_utf8(entry(&bytes, "document.xml")).unwrap();
    assert!(document.contains("<diacraft:blob path=\"blobs/0.png\" media-type=\"image/png\" size=\"72\" stamp=\""), "{document}");
    // The media type takes no part in finding equal bytes: the same bytes under two types are one blob, and the record carries the first use's type.
    let doc = drawing_of(vec![page_of_typed(1, &[("image/png", a.clone()), ("image/x-other", b.clone())])]);
    let bytes = write_doc(&doc);
    assert_eq!(blob_entries(&bytes), ["blobs/0.png"]);
    assert!(String::from_utf8(entry(&bytes, "document.xml")).unwrap().contains("media-type=\"image/png\""));
}

#[test]
fn blobs_are_numbered_by_first_use_in_the_order_the_parts_are_written() {
    let (a, b, c) = (png(1, 10), png(2, 10), png(3, 10));
    let doc = drawing_of(vec![page_showing(1, &[&b]), page_showing(2, &[&a, &c, &b])]);
    let bytes = write_doc(&doc);
    assert_eq!(blob_entries(&bytes), ["blobs/0.png", "blobs/1.png", "blobs/2.png"]);
    assert_eq!(&entry(&bytes, "blobs/0.png")[8..], &b[8..], "the first picture used is blob 0");
    assert_eq!(&entry(&bytes, "blobs/1.png")[8..], &a[8..]);
    assert_eq!(&entry(&bytes, "blobs/2.png")[8..], &c[8..]);
    // The same model gives the same numbers, whatever the allocations.
    let again = drawing_of(vec![page_showing(1, &[&png(2, 10)]), page_showing(2, &[&png(1, 10), &png(3, 10), &png(2, 10)])]);
    assert_eq!(write_doc(&again), bytes);
}

#[test]
fn equal_size_different_bytes_are_two_blobs() {
    let (a, b) = (png(1, 64), png(2, 64));
    assert_eq!(a.len(), b.len());
    let doc = drawing_of(vec![page_showing(1, &[&a, &b])]);
    let bytes = write_doc(&doc);
    assert_eq!(blob_entries(&bytes), ["blobs/0.png", "blobs/1.png"]);
    let back = open_ok(&bytes).0;
    assert_eq!(picture_of(&back, 0, 0).as_deref(), Some(&a[..]));
    assert_eq!(picture_of(&back, 0, 1).as_deref(), Some(&b[..]));
}

#[test]
fn a_stamp_collision_still_compares_bytes() {
    // The seam: every stamp is the same, as a crafted pair of byte strings would make it. Equal bytes still join, and different bytes of one size never do.
    let mut table = BlobTable::with_hash(|_| Stamp(0));
    let (a, b, c) = (png(1, 16), png(2, 16), png(3, 16));
    let ids: Vec<u32> = [&a, &b, &c, &png(1, 16), &png(2, 16), &a].iter().map(|x| table.put(x, "image/png").unwrap().0).collect();
    assert_eq!(ids, [0, 1, 2, 0, 1, 0]);
    assert_eq!(table.len(), 3);
    assert!(Arc::ptr_eq(table.arc(0).unwrap(), &a), "the table holds the allocation of the first use");
    assert_eq!(table.entry(BlobId(1)).map(|(bytes, media)| (bytes.len(), media)), Some((24, "image/png")));
    assert!(table.arc(3).is_none());
    // And the same for a model written through the real table: the pair of one size is two entries.
    let mut real = BlobTable::new();
    assert_eq!(real.put(&a, "image/png").unwrap().0, 0);
    assert_eq!(real.put(&b, "image/png").unwrap().0, 1);
    assert!(!real.is_empty());
}

#[test]
fn a_blob_is_found_by_its_allocation_without_a_hash_of_its_bytes_being_trusted() {
    // A table that is given the same allocation twice answers from the address; a different allocation of the same bytes is found by size, stamp and bytes.
    let mut table = BlobTable::new();
    let a = png(1, 1000);
    assert_eq!(table.put(&a, "image/png").unwrap(), BlobId(0));
    assert_eq!(table.put(&a, "image/jpeg").unwrap(), BlobId(0));
    let copy: Arc<[u8]> = Arc::from(a.to_vec());
    assert_eq!(table.put(&copy, "image/png").unwrap(), BlobId(0));
    assert_eq!(table.len(), 1);
    // A table holds at most the entries a package may.
    let mut full = BlobTable::new();
    for i in 0..50_000u32 {
        let bytes: Arc<[u8]> = Arc::from(i.to_le_bytes().to_vec());
        full.put(&bytes, "application/octet-stream").unwrap();
    }
    let one_more: Arc<[u8]> = Arc::from(vec![9u8; 9]);
    assert!(full.put(&one_more, "application/octet-stream").is_err());
    // ... but bytes it already holds are still found.
    let held: Arc<[u8]> = Arc::from(7u32.to_le_bytes().to_vec());
    assert_eq!(full.put(&held, "application/octet-stream").unwrap(), BlobId(7));
}

#[test]
fn png_jpeg_gif_and_zip_are_stored_others_deflated() {
    let pad = |head: &[u8]| -> Arc<[u8]> { Arc::from([head, &[0x55u8; 200][..]].concat()) };
    let blobs: Vec<(&str, Arc<[u8]>, bool)> = vec![
        ("image/png", pad(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]), true),
        ("image/jpeg", pad(&[0xFF, 0xD8, 0xFF, 0xE0]), true),
        ("image/gif", pad(b"GIF89a"), true),
        ("image/gif", pad(b"GIF87a"), true),
        ("application/zip", pad(b"PK\x03\x04"), true),
        ("image/bmp", pad(b"BM"), false),
        ("image/gif", pad(b"GIF8"), false),
        ("application/zip", pad(b"PK\x05\x06"), false),
        ("image/png", pad(&[0x89, b'P', b'N', b'G']), false),
        ("application/octet-stream", pad(b""), false),
        ("image/jpeg", pad(&[0xFF, 0xD8]), false),
    ];
    for (_, bytes, want) in &blobs {
        assert_eq!(stored(bytes), *want);
    }
    let typed: Vec<(&str, Arc<[u8]>)> = blobs.iter().map(|(t, b, _)| (*t, Arc::clone(b))).collect();
    let bytes = write_doc(&drawing_of(vec![page_of_typed(1, &typed)]));
    let methods = methods(&bytes);
    for (i, (_, _, want)) in blobs.iter().enumerate() {
        let (name, method) = methods.iter().find(|(n, _)| n.starts_with(&format!("blobs/{i}."))).unwrap();
        assert_eq!(*method, Some(if *want { Method::Stored } else { Method::Deflated }), "{name}");
    }
    // The extension follows the media type of the first use, and `bin` is every other type.
    let names = blob_entries(&bytes);
    assert_eq!(
        names,
        [
            "blobs/0.png",
            "blobs/1.jpg",
            "blobs/2.gif",
            "blobs/3.gif",
            "blobs/4.zip",
            "blobs/5.bmp",
            "blobs/6.gif",
            "blobs/7.zip",
            "blobs/8.png",
            "blobs/9.bin",
            "blobs/10.jpg"
        ]
    );
    // Every blob reads back whichever way it was stored.
    let back = open_ok(&bytes).0;
    for (i, (_, b, _)) in blobs.iter().enumerate() {
        assert_eq!(picture_of(&back, 0, i).as_deref(), Some(&b[..]));
    }
}

/// Vector 5 with its blob replaced by `blob`, or left out.
fn vector_5_with(blob: Option<Item>) -> Vec<u8> {
    let mut items = vector_5();
    remove(&mut items, "blobs/0.png");
    if let Some(item) = blob {
        let at = items.iter().position(|i| i.name == b"state.xml").unwrap();
        items.insert(at, item);
    }
    package(&items)
}

#[test]
fn a_damaged_blob_opens_with_a_note() {
    // The spec's 8.3: the blob missing, its last byte changed (the stamp does not match), 13 bytes (the size does not), and a checksum that fails: each opens, both pictures
    // are lost, and one note says one picture was lost.
    let mut changed = PICTURE.to_vec();
    *changed.last_mut().unwrap() ^= 0xFF;
    let mut bad_crc = Item::stored("blobs/0.png", &PICTURE);
    bad_crc.crc ^= 1;
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("missing", vector_5_with(None)),
        ("changed", vector_5_with(Some(Item::stored("blobs/0.png", &changed)))),
        ("thirteen bytes", vector_5_with(Some(Item::stored("blobs/0.png", &[PICTURE.as_slice(), &[0]].concat())))),
        ("checksum", vector_5_with(Some(bad_crc))),
    ];
    for (what, bytes) in cases {
        let (doc, opened) = open_ok(&bytes);
        assert_eq!(opened.notes, [ReadNote::PictureLost { count: 1 }], "{what}");
        assert_eq!(opened.notes[0].to_string(), "1 picture could not be loaded");
        assert_eq!(doc.pages.get(0).unwrap().shapes.len(), 2, "{what}: both shapes are still there");
        for shape in 0..2 {
            let foreign = doc.pages.get(0).unwrap().shapes.get(shape).unwrap().foreign.clone().unwrap();
            assert!(foreign.picture.is_none(), "{what}: a picture that could not be loaded shows nothing");
        }
        // A model that lost a picture is written again without one, and reads back as it is.
        let again = write_doc(&doc);
        assert_eq!(open_ok(&again).1.notes, []);
    }
    // The record's stamp in capitals is damage (a stamp is lowercase), and so is a record without a stamp.
    let mut items = vector_5();
    let document = String::from_utf8(entry(&package(&items), "document.xml")).unwrap().replace("71df7ba063b3b4da", "71DF7BA063B3B4DA");
    replace(&mut items, "document.xml", document.into_bytes());
    assert_eq!(refused(&package(&items)).class(), "model_malformed");
    let mut items = vector_5();
    let document = String::from_utf8(entry(&package(&items), "document.xml")).unwrap().replace(" stamp=\"71df7ba063b3b4da\"", "");
    replace(&mut items, "document.xml", document.into_bytes());
    assert_eq!(refused(&package(&items)).class(), "model_malformed");
}

fn held_part(name: &str, bytes: &[u8]) -> Arc<Part> {
    Arc::new(Part {
        format: Format::Vsdx,
        name: Arc::from(name),
        media_type: Arc::from("application/octet-stream"),
        role: PartRole::Embedding,
        manifest: RawAttrs::new(),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(bytes.to_vec()),
    })
}

#[test]
fn a_lost_theme_is_left_out_and_a_lost_part_too() {
    let mut doc = Document::new_blank();
    doc.themes =
        SharedVec::from_vec(vec![Arc::new(Theme { part: Arc::from("/t/theme1.xml"), bytes: Arc::from(b"<theme/>".to_vec()), ..Theme::default() })]);
    doc.parts = SharedVec::from_vec(vec![held_part("/e/one.bin", b"one one"), held_part("/e/two.bin", b"two two two")]);
    let bytes = write_doc(&doc);
    let (_, intact) = open_ok(&bytes);
    assert!(intact.notes.is_empty());
    assert_eq!(blob_entries(&bytes).len(), 3, "the theme and the two parts are blobs");
    // Every blob entry gone from the package: the theme is left out (ThemeLost), each part is left out (PartLost), and the file opens.
    let items = rebuilt(&bytes, |name, content| (!name.starts_with("blobs/")).then_some(content));
    let (doc, opened) = open_ok(&package(&items));
    assert_eq!(opened.notes, [ReadNote::ThemeLost { count: 1 }, ReadNote::PartLost { count: 2 }]);
    assert_eq!(opened.notes[0].to_string(), "1 theme was left out because its bytes are lost");
    assert_eq!(opened.notes[1].to_string(), "2 parts of the file were left out because their bytes are lost");
    assert!(doc.themes.is_empty() && doc.parts.is_empty());
    // One of the two parts lost: the other is kept.
    let items = rebuilt(&bytes, |name, content| (name != "blobs/1.bin").then_some(content));
    let (doc, opened) = open_ok(&package(&items));
    assert_eq!(opened.notes, [ReadNote::PartLost { count: 1 }]);
    assert_eq!(doc.parts.len(), 1);
}

#[test]
fn a_theme_or_a_part_that_the_stamp_loses_is_not_remembered() {
    // A blob whose size is its record's and whose stamp is not is lost, whatever it was for. The note of a lost picture names its number, and the notes of a lost
    // theme and a lost part name them by name, so the read let go of the picture's bytes and held on to the others in `Decoded`, which a later save could never copy.
    let mut doc = Document::new_blank();
    doc.themes =
        SharedVec::from_vec(vec![Arc::new(Theme { part: Arc::from("/t/theme1.xml"), bytes: Arc::from(b"<theme/>".to_vec()), ..Theme::default() })]);
    doc.parts = SharedVec::from_vec(vec![held_part("/e/one.bin", b"one one"), held_part("/e/two.bin", b"two two two")]);
    let bytes = write_doc(&doc);
    let (_, intact) = open_ok(&bytes);
    assert_eq!(intact.decoded.blobs(), 3, "the theme and the two parts are blobs of the package");
    let names = blob_entries(&bytes);
    // Every blob's bytes replaced by others of the same length: the size is the record's and the stamp is not.
    let items = rebuilt(&bytes, |name, mut content| {
        if name.starts_with("blobs/") {
            content[0] ^= 0xFF;
        }
        Some(content)
    });
    let (back, opened) = open_ok(&package(&items));
    assert_eq!(opened.notes, [ReadNote::ThemeLost { count: 1 }, ReadNote::PartLost { count: 2 }]);
    assert!(back.themes.is_empty() && back.parts.is_empty());
    assert_eq!(opened.decoded.blobs(), 0, "lost blobs are no blobs of the package: {names:?}");
    // One of the three lost and the other two intact: only the two the model holds are remembered.
    let victim = names.iter().find(|n| n.ends_with("1.bin")).cloned().unwrap_or_default();
    let items = rebuilt(&bytes, |name, mut content| {
        if name == victim {
            content[0] ^= 0xFF;
        }
        Some(content)
    });
    let (back, opened) = open_ok(&package(&items));
    assert_eq!(opened.notes, [ReadNote::PartLost { count: 1 }]);
    assert_eq!(back.parts.len(), 1);
    assert_eq!(opened.decoded.blobs(), 2);
}

#[test]
fn a_blob_holding_a_zip_or_a_zip_bomb_is_never_opened() {
    // The bytes of an embedded package are copied as bytes: `work.inflated` is the entries' own sizes whatever the blob holds, and no XML event is made of it.
    let nested = |inner: &[u8]| -> Vec<u8> { package(&[Item::stored("mimetype", b"application/x"), Item::deflated("big.xml", inner)]) };
    let bomb = nested(&vec![b'<'; 1 << 20]);
    let honest = nested(b"<a/>");
    let length = bomb.len().max(honest.len()) + 10;
    let mut measured = Vec::new();
    for inside in [bomb.clone(), honest.clone(), vec![0u8; length]] {
        let mut blob = inside;
        blob.resize(length, 0);
        let blob: Arc<[u8]> = Arc::from(blob);
        let mut doc = Document::new_blank();
        doc.parts = SharedVec::from_vec(vec![held_part("/e/object.bin", &blob)]);
        let bytes = write_doc(&doc);
        let (back, opened) = open_ok(&bytes);
        assert_eq!(&*back.parts.iter().next().unwrap().bytes, &*blob, "the bytes come back as they went");
        measured.push((opened.work.inflated, opened.work.xml_events));
    }
    // The same document with a blob of the same size gives the same work, whatever the blob holds: nothing nested is read.
    assert_eq!(measured[0].1, measured[1].1, "a bomb and an honest archive of one size make the same XML events");
    assert_eq!(measured[0].1, measured[2].1);
    assert_eq!(measured[0].0, measured[1].0, "and inflate the same bytes: the blob's own size");
    assert_eq!(measured[0].0, measured[2].0);
}

#[test]
fn a_macro_project_blob_is_only_copied() {
    // A VBA project and an ODF `Basic/` part held as blobs. Opening and saving makes zero XML events over their bytes, and they come back byte for byte, whether they
    // would parse as XML or not.
    let xml_like: Vec<u8> = b"<script:module xmlns:script=\"urn:x\"><a b=\"c\"/></script:module>".repeat(500);
    let ole: Vec<u8> = [&[0xD0u8, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1][..], &[0u8; 4000][..]].concat();
    let with = |a: &[u8], b: &[u8]| {
        let mut doc = Document::new_blank();
        let macro_project = Part { role: PartRole::MacroProject, ..(*held_part("/m/vbaProject.bin", a)).clone() };
        let basic = Part { format: Format::Odf, ..(*held_part("Basic/Standard/Module1.xml", b)).clone() };
        doc.parts = SharedVec::from_vec(vec![Arc::new(macro_project), Arc::new(basic)]);
        doc
    };
    let doc = with(&ole, &xml_like);
    let bytes = write_doc(&doc);
    let (back, opened) = open_ok(&bytes);
    assert!(back == doc, "byte for byte");
    // A control of the same sizes that is not XML: the same number of XML events, so none were made over the bytes of the parts.
    let control = with(&vec![0u8; ole.len()], &vec![b'x'; xml_like.len()]);
    let (_, control_opened) = open_ok(&write_doc(&control));
    assert_eq!(opened.work.xml_events, control_opened.work.xml_events, "no event over a part's bytes");
    // And saved again, they are the same bytes.
    let again = write(&back, &opened.state, None, &WriteOptions::default()).unwrap();
    let (twice, _) = open_ok(&again.bytes);
    assert!(twice == doc);
}
