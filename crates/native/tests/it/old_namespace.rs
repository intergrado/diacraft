//! APPID: a `.diacraft` file that a build made before the id moved to `diacraft.app` wrote (its namespace is `urn:io.github.one_bit.diacraft:extension`) opens as it did, is written again
//! in the current namespace only, and a package that holds both reads as one drawing. The file is the one `main` at 3f696318 wrote (`diacraft_testkit::old_namespace`), not one
//! this code built by hand; the files of the other kind that the tests make are the same file's parts, put in the current namespace or half of them.

use diacraft_container::zip::{Archive, ArchiveWriter, Method};
use diacraft_container::{Limits as ContainerLimits, Work};
use diacraft_native::{Limits, Previous, WriteOptions, read, write};
use diacraft_testkit::old_namespace::{DRAWING_DIACRAFT, OLD_NAMESPACE, file, parts};

use crate::util::NS;

/// The archive of the old file's parts, each put through `change` (its name and text), written by the container's writer: its headers are the ones a save copies an entry from.
fn archive(change: impl Fn(&str, String) -> String) -> Vec<u8> {
    let mut out = ArchiveWriter::new();
    for (name, bytes) in parts(DRAWING_DIACRAFT) {
        match String::from_utf8(bytes) {
            Ok(text) if name == "mimetype" => out.add(&name, text.as_bytes(), Method::Stored).unwrap(),
            Ok(text) => out.add(&name, change(&name, text).as_bytes(), Method::Deflated).unwrap(),
            Err(bytes) => out.add(&name, bytes.as_bytes(), Method::Deflated).unwrap(),
        }
    }
    out.finish().unwrap()
}

/// The text of every part of a package, as the container reads it, with its name.
fn texts(bytes: &[u8]) -> Vec<(String, String)> {
    let mut archive = Archive::read(bytes, &ContainerLimits::default()).unwrap();
    let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
    names
        .into_iter()
        .enumerate()
        .map(|(at, name)| (name, String::from_utf8_lossy(&archive.inflate(at, &mut Work::default()).unwrap()).into_owned()))
        .collect()
}

#[test]
fn the_file_a_build_before_the_move_wrote_holds_the_old_namespace_in_every_part_of_the_model() {
    let held: Vec<String> =
        texts(&file(DRAWING_DIACRAFT)).into_iter().filter(|(_, text)| text.contains(OLD_NAMESPACE)).map(|(name, _)| name).collect();
    for expected in ["manifest.xml", "document.xml", "state.xml", "pages/page1.xml", "masters/master1.xml"] {
        assert!(held.iter().any(|n| n == expected), "{expected} is in the old namespace: {held:?}");
    }
}

#[test]
fn a_drawing_of_the_old_namespace_opens_as_the_drawing_of_the_current_one() {
    let (old, opened_old) = read(&file(DRAWING_DIACRAFT), &Limits::default()).unwrap();
    let current = archive(|_, text| text.replace(OLD_NAMESPACE, NS));
    let (new, opened_new) = read(&current, &Limits::default()).unwrap();
    assert!(old == new, "the same drawing, whichever namespace its parts are in");
    assert_eq!(old.pages.len(), 2);
    assert_eq!(opened_old.kind, opened_new.kind);
    assert_eq!(opened_old.format, opened_new.format);
    assert_eq!(opened_old.state, opened_new.state, "the ledger and the stamp are over the content, and not over how a part says its namespace");
    assert!(opened_old.notes.is_empty(), "{:?}", opened_old.notes);
}

#[test]
fn a_drawing_of_the_old_namespace_is_written_in_the_current_one_and_nowhere_in_the_old() {
    let (doc, opened) = read(&file(DRAWING_DIACRAFT), &Limits::default()).unwrap();
    let written = write(&doc, &opened.state, None, &WriteOptions::default()).unwrap().bytes;
    let parts = texts(&written);
    assert!(parts.iter().any(|(_, text)| text.contains(NS)));
    for (name, text) in &parts {
        assert!(!text.contains("one_bit"), "{name} still names the old namespace");
    }
    let (again, again_opened) = read(&written, &Limits::default()).unwrap();
    assert!(again == doc, "what was written is the drawing that was read");
    assert_eq!(again_opened.state, opened.state);
}

#[test]
fn a_save_that_changed_nothing_writes_the_file_it_read_and_a_package_that_holds_both_reads_as_one_drawing() {
    let bytes = archive(|_, text| text);
    let (doc, opened) = read(&bytes, &Limits::default()).unwrap();
    let previous = Previous { bytes: &bytes, decoded: &opened.decoded };
    let saved = write(&doc, &opened.state, Some(&previous), &WriteOptions::default()).unwrap();
    // The parts the save copied are the file's own, in the namespace they were written in; the manifest and the state are written again, in the current one.
    assert!(saved.copied >= 3, "the pages and the masters are copied: {}", saved.copied);
    let by_name: std::collections::HashMap<String, String> = texts(&saved.bytes).into_iter().collect();
    assert!(by_name["manifest.xml"].contains(NS) && !by_name["manifest.xml"].contains("one_bit"));
    assert!(by_name["pages/page1.xml"].contains(OLD_NAMESPACE), "a page that did not change is copied as it was");
    let (back, back_opened) = read(&saved.bytes, &Limits::default()).unwrap();
    assert!(back == doc, "a package of both is one drawing: each page and master once");
    assert_eq!(back.pages.len(), doc.pages.len());
    assert_eq!(back.masters.len(), doc.masters.len());
    assert_eq!(back_opened.state, opened.state);
    // A second save of it, and one that follows an edit, are the same drawing again.
    let again =
        write(&back, &back_opened.state, Some(&Previous { bytes: &saved.bytes, decoded: &back_opened.decoded }), &WriteOptions::default()).unwrap();
    assert!(read(&again.bytes, &Limits::default()).unwrap().0 == doc);
}

#[test]
fn a_package_whose_manifest_and_parts_are_in_different_namespaces_opens_whichever_is_which() {
    let (expected, _) = read(&file(DRAWING_DIACRAFT), &Limits::default()).unwrap();
    // The manifest and the state in the current namespace and the model in the old one, and the other way about.
    let manifest_new = archive(|name, text| if name == "manifest.xml" || name == "state.xml" { text.replace(OLD_NAMESPACE, NS) } else { text });
    let model_new = archive(|name, text| if name == "manifest.xml" || name == "state.xml" { text } else { text.replace(OLD_NAMESPACE, NS) });
    // One part of each namespace: the first page current, the second page and the rest old.
    let alternate =
        archive(|name, text| if name == "pages/page1.xml" || name == "masters/master2.xml" { text.replace(OLD_NAMESPACE, NS) } else { text });
    for (what, bytes) in [("manifest new", manifest_new), ("model new", model_new), ("alternate", alternate)] {
        let (doc, opened) = read(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("{what}: {e:?}"));
        assert!(doc == expected, "{what}");
        assert!(opened.notes.is_empty(), "{what}: {:?}", opened.notes);
    }
}

#[test]
fn a_root_in_a_namespace_that_is_neither_is_still_refused() {
    let other = archive(|name, text| if name == "manifest.xml" { text.replace(OLD_NAMESPACE, "urn:example:other") } else { text });
    assert!(read(&other, &Limits::default()).is_err(), "a manifest in another namespace is no manifest of DiaCraft's");
}

/// A picture of PNG bytes for a preview: the signature and a word.
fn preview_of(word: &[u8]) -> Vec<u8> {
    [&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], word].concat()
}

/// A package as a build before the move wrote it that holds a preview: the package the current writer makes of the old drawing with the picture, each XML part of it in the old namespace
/// and nothing else changed (the picture stays stored, the pages deflated: the headers of this writer, as `29a46bbf` made them).
fn old_package_with_a_preview(preview: &[u8]) -> Vec<u8> {
    let (doc, opened) = read(&file(DRAWING_DIACRAFT), &Limits::default()).unwrap();
    let written = write(
        &doc,
        &opened.state,
        None,
        &WriteOptions { preview: Some(preview.to_vec()), written_by: "DiaCraft test".to_owned(), ..WriteOptions::default() },
    )
    .unwrap();
    let items: Vec<crate::zipbuild::Item> = crate::util::entries(&written.bytes)
        .into_iter()
        .map(|(name, bytes)| match name.as_str() {
            "mimetype" | "preview.png" => crate::zipbuild::Item::stored(&name, &bytes),
            _ if name.ends_with(".xml") => {
                crate::zipbuild::Item::deflated(&name, String::from_utf8(bytes).unwrap().replace(NS, OLD_NAMESPACE).as_bytes())
            }
            _ => crate::zipbuild::Item::deflated(&name, &bytes),
        })
        .collect();
    crate::util::package(&items)
}

#[test]
fn a_save_that_changed_nothing_keeps_the_preview_of_a_file_in_the_old_namespace() {
    let preview = preview_of(b"first page");
    let bytes = old_package_with_a_preview(&preview);
    let held = crate::util::entries(&bytes);
    assert!(
        held.iter().any(|(name, text)| name == "document.xml" && String::from_utf8_lossy(text).contains(OLD_NAMESPACE)),
        "the document part is in the old namespace"
    );
    assert!(!held.iter().any(|(_, text)| String::from_utf8_lossy(text).contains(NS)), "no part of it is in the current one");
    let (doc, opened) = read(&bytes, &Limits::default()).unwrap();
    assert!(opened.decoded.has_preview(), "the package is read with its picture");
    let saved = write(&doc, &opened.state, Some(&Previous { bytes: &bytes, decoded: &opened.decoded }), &WriteOptions::default()).unwrap();
    assert!(saved.decoded.has_preview(), "the file as it is written has the picture");
    assert_eq!(crate::util::entry(&saved.bytes, "preview.png"), preview, "the picture it had");
    // The document part and the manifest are the current namespace's, and a second save of what was written copies everything it has, the picture too.
    assert!(!String::from_utf8_lossy(&crate::util::entry(&saved.bytes, "document.xml")).contains("one_bit"));
    let (back, back_opened) = read(&saved.bytes, &Limits::default()).unwrap();
    assert!(back == doc);
    let again =
        write(&back, &back_opened.state, Some(&Previous { bytes: &saved.bytes, decoded: &back_opened.decoded }), &WriteOptions::default()).unwrap();
    assert_eq!(crate::util::entry(&again.bytes, "preview.png"), preview);
}

#[test]
fn a_save_of_a_file_in_the_old_namespace_that_changed_the_document_part_gives_it_no_old_picture() {
    let preview = preview_of(b"first page");
    let bytes = old_package_with_a_preview(&preview);
    let (mut doc, opened) = read(&bytes, &Limits::default()).unwrap();
    doc.title = std::sync::Arc::from("another title");
    let saved = write(&doc, &opened.state, Some(&Previous { bytes: &bytes, decoded: &opened.decoded }), &WriteOptions::default()).unwrap();
    assert!(
        !crate::util::names(&saved.bytes).contains(&"preview.png".to_owned()),
        "a drawing that changed is not given the picture of the one before"
    );
}
