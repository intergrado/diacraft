//! The package: the entries, their order and names, what is listed, and what a save copies as it is (the public spec's section 2, the plan's NP4 and Review Focus 1).
use std::sync::Arc;

use diacraft_container::xml::{Node, parse};
use diacraft_doc::{Document, Page, Shape, ShapeList};
use diacraft_native::{FORMAT, Kind, Previous, State, WriteOptions, read, write};
use diacraft_testkit::model::{ModelSize, any_document, fixture_documents, without_derived_theme_data};
use proptest::prelude::*;

use crate::util::*;
use crate::zipbuild::Item;

fn with_previous(doc: &Document, state: &State, bytes: &[u8], decoded: &diacraft_native::Decoded) -> diacraft_native::Written {
    write(doc, state, Some(&Previous { bytes, decoded }), &WriteOptions::default()).unwrap()
}

/// The rows of a manifest: `(path, role, required, media-type)`.
fn rows(bytes: &[u8]) -> Vec<(String, String, Option<String>, Option<String>)> {
    let tree = parse(&entry(bytes, "manifest.xml")).unwrap();
    tree.children
        .iter()
        .filter_map(|n| if let Node::El(e) = n { Some(e) } else { None })
        .map(|e| {
            (
                e.attr("path").unwrap().to_owned(),
                e.attr("role").unwrap().to_owned(),
                e.attr("required").map(str::to_owned),
                e.attr("media-type").map(str::to_owned),
            )
        })
        .collect()
}

#[test]
fn a_blank_drawing_round_trips() {
    let doc = Document::new_blank();
    let bytes = write_doc(&doc);
    let (back, opened) = open_ok(&bytes);
    assert!(back == doc, "the blank drawing comes back as it went");
    assert_eq!(opened.kind, Kind::Drawing);
    assert_eq!(opened.format, FORMAT);
    assert_eq!(opened.state, State::default());
    assert!(opened.notes.is_empty());
    assert_eq!(opened.kept_parts, 0);
}

#[test]
fn entries_are_in_the_fixed_order_and_listed_once() {
    // NP4: `mimetype`, `manifest.xml`, `document.xml`, the pages in page order, the masters in master order, the blobs by number, `state.xml`, `preview.png`.
    let (p, q) = (png(1, 40), png(2, 40));
    let mut doc = drawing_of(vec![page_showing(7, &[&p]), page_showing(3, &[&q, &p])]);
    doc.masters = diacraft_doc::SharedVec::from_vec(vec![Arc::new(master(5, "M")), Arc::new(master(2, "N"))]);
    let preview: Vec<u8> = [&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], b"preview"].concat();
    let options = WriteOptions { preview: Some(preview.clone()), written_by: "DiaCraft test".to_owned(), ..WriteOptions::default() };
    let written = write(&doc, &State::default(), None, &options).unwrap();
    let names = names(&written.bytes);
    assert_eq!(
        names,
        [
            "mimetype",
            "manifest.xml",
            "document.xml",
            "pages/page7.xml",
            "pages/page3.xml",
            "masters/master5.xml",
            "masters/master2.xml",
            "blobs/0.png",
            "blobs/1.png",
            "state.xml",
            "preview.png"
        ]
    );
    // The manifest lists every entry but the first two, once each, in the order of the entries, with the roles and flags of the spec's 2.4.
    let listed = rows(&written.bytes);
    let paths: Vec<&str> = listed.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(paths, &names[2..]);
    let roles: Vec<&str> = listed.iter().map(|r| r.1.as_str()).collect();
    assert_eq!(roles, ["document", "page", "page", "master", "master", "blob", "blob", "state", "preview"]);
    let required: Vec<bool> = listed.iter().map(|r| r.2.as_deref() == Some("1")).collect();
    assert_eq!(
        required,
        [true, true, true, true, true, false, false, false, false],
        "required on the entries of role document, page and master, and no other"
    );
    assert!(listed.iter().all(|r| r.2.is_none() || r.2.as_deref() == Some("1")));
    assert_eq!(listed[5].3.as_deref(), Some("image/png"), "a blob row carries its media type");
    assert_eq!(listed[0].3, None);
    let root = parse(&entry(&written.bytes, "manifest.xml")).unwrap();
    assert_eq!(root.attr("kind"), Some("drawing"));
    assert_eq!(root.attr("written-by"), Some("DiaCraft test"));
    assert_eq!(root.attr("format"), Some(format!("{}.{}", FORMAT.0, FORMAT.1).as_str()));
    // The pages come back in page order under the names the document part lists.
    let (back, opened) = open_ok(&written.bytes);
    assert_eq!(back.pages.iter().map(|p| p.id).collect::<Vec<_>>(), [7, 3]);
    assert_eq!(back.masters.iter().map(|m| m.id).collect::<Vec<_>>(), [5, 2]);
    assert_eq!(opened.decoded.pages(), 2);
    assert_eq!(opened.decoded.masters(), 2);
    assert_eq!(opened.decoded.blobs(), 2);
    assert_eq!(entry(&written.bytes, "preview.png"), preview);
    // Deleting a page renames no other page's part: parts are named by model ID.
    let mut fewer = doc.clone();
    fewer.pages = diacraft_doc::SharedVec::from_vec(fewer.pages.iter().skip(1).cloned().collect());
    assert!(self::names(&write_doc(&fewer)).contains(&"pages/page3.xml".to_owned()));
}

#[test]
fn the_mimetype_entry_is_first_stored_with_no_extra_field() {
    let bytes = write_doc(&Document::new_blank());
    let at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    assert_eq!(&bytes[..4], &[0x50, 0x4B, 0x03, 0x04], "a local header at 0");
    assert_eq!(at(8), 0, "stored");
    assert_eq!(at(26), 8, "a name of 8 bytes");
    assert_eq!(at(28), 0, "no extra field");
    assert_eq!(&bytes[30..38], b"mimetype");
    assert_eq!(&bytes[38..38 + 32], b"application/vnd.diacraft.drawing");
    assert_eq!(bytes[38 + 32], 0x50, "no line end after the media type: the next local header starts right after it");
    // Every other entry is deflated, but `preview.png` and the blobs that are compressed already; every entry is dated 1980-01-01 00:00:00; no data descriptor.
    for (name, method) in methods(&bytes) {
        assert_eq!(
            method,
            Some(if name == "mimetype" { diacraft_container::zip::Method::Stored } else { diacraft_container::zip::Method::Deflated }),
            "{name}"
        );
    }
    let archive = diacraft_container::zip::Archive::read(&bytes, &diacraft_container::Limits::default()).unwrap();
    for e in archive.entries() {
        assert_eq!((e.date, e.time), (0x0021, 0), "{}", e.name);
        assert_eq!(e.flags & 0x0008, 0, "no data descriptor: {}", e.name);
        assert_eq!(e.local_extra_len, 0, "no extra field: {}", e.name);
    }
    // The first entry's name and bytes also sit at the places a magic rule looks at, for the three kinds.
    for kind in Kind::ALL {
        let options = WriteOptions { kind, ..WriteOptions::default() };
        let bytes = write(&Document::new_blank(), &State::default(), None, &options).unwrap().bytes;
        assert_eq!(&bytes[38..38 + kind.media_type().len()], kind.media_type().as_bytes());
    }
}

#[test]
fn an_unlisted_entry_is_damage() {
    // Vector 1 with an entry `extra.txt` that the manifest does not list.
    let mut items = vector_1();
    items.push(Item::stored("extra.txt", b"x"));
    assert_eq!(refused(&package(&items)).class(), "entry_unlisted");
    // A folder entry is ignored (2.1), and is no entry to list.
    let mut items = vector_1();
    items.push(Item::stored("empty/", b""));
    assert!(read(&package(&items), &limits()).is_ok(), "a folder is ignored");
}

#[test]
fn a_listed_missing_entry_is_damage() {
    // Vector 1 with the manifest listing `pages/page2.xml`, which is not in the package.
    let mut items = vector_1();
    replace(
        &mut items,
        "manifest.xml",
        manifest(
            "1.0",
            "drawing",
            &[("document.xml", "document", ""), ("pages/page1.xml", "page", ""), ("pages/page2.xml", "page", ""), ("state.xml", "state", "")],
        ),
    );
    assert_eq!(refused(&package(&items)).class(), "entry_missing");
    // The document part missing, and a page part that its lists name missing.
    let mut items = vector_1();
    remove(&mut items, "document.xml");
    assert_eq!(refused(&package(&items)).class(), "entry_missing");
    let mut items = vector_1();
    replace(&mut items, "manifest.xml", manifest("1.0", "drawing", &[("pages/page1.xml", "page", ""), ("state.xml", "state", "")]));
    remove(&mut items, "document.xml");
    assert_eq!(refused(&package(&items)).class(), "document_missing");
    let mut items = vector_1();
    replace(&mut items, "manifest.xml", manifest("1.0", "drawing", &[("document.xml", "document", ""), ("state.xml", "state", "")]));
    remove(&mut items, "pages/page1.xml");
    assert_eq!(refused(&package(&items)).class(), "part_missing", "the document part lists a page the package lacks");
    // A page part that no list names.
    let mut items = vector_1();
    replace(
        &mut items,
        "manifest.xml",
        manifest(
            "1.0",
            "drawing",
            &[("document.xml", "document", ""), ("pages/page1.xml", "page", ""), ("pages/page9.xml", "page", ""), ("state.xml", "state", "")],
        ),
    );
    items.push(Item::deflated("pages/page9.xml", &page_1("")));
    assert_eq!(refused(&package(&items)).class(), "part_unlisted");
    // No manifest at all.
    let mut items = vector_1();
    remove(&mut items, "manifest.xml");
    assert_eq!(refused(&package(&items)).class(), "manifest_missing");
}

#[test]
fn an_entry_listed_twice_is_damage() {
    let mut items = vector_1();
    replace(
        &mut items,
        "manifest.xml",
        manifest(
            "1.0",
            "drawing",
            &[("document.xml", "document", ""), ("pages/page1.xml", "page", ""), ("state.xml", "state", ""), ("state.xml", "state", "")],
        ),
    );
    assert_eq!(refused(&package(&items)).class(), "entry_listed_twice");
}

#[test]
fn a_manifest_row_without_a_path_or_a_role_is_damage() {
    for bad in [
        format!("<diacraft:manifest xmlns:diacraft=\"{NS}\" format=\"1.0\" kind=\"drawing\"><diacraft:part role=\"document\"/></diacraft:manifest>"),
        format!(
            "<diacraft:manifest xmlns:diacraft=\"{NS}\" format=\"1.0\" kind=\"drawing\"><diacraft:part path=\"document.xml\"/></diacraft:manifest>"
        ),
        format!(
            "<diacraft:manifest xmlns:diacraft=\"{NS}\" format=\"1.0\" kind=\"drawing\"><diacraft:part path=\"document.xml\" role=\"document\" required=\"2\"/></diacraft:manifest>"
        ),
    ] {
        let mut items = vector_1();
        replace(&mut items, "manifest.xml", part(&bad));
        assert_eq!(refused(&package(&items)).class(), "manifest_row", "{bad}");
    }
    // A manifest that is not XML, and one whose root is not a manifest, or is in another namespace.
    let mut items = vector_1();
    replace(&mut items, "manifest.xml", b"<not xml".to_vec());
    assert_eq!(refused(&package(&items)).class(), "xml_malformed");
    for root in [format!("<diacraft:state xmlns:diacraft=\"{NS}\"/>"), "<manifest format=\"1.0\" kind=\"drawing\"/>".to_owned()] {
        let mut items = vector_1();
        replace(&mut items, "manifest.xml", part(&root));
        assert_eq!(refused(&package(&items)).class(), "manifest_malformed", "{root}");
    }
}

#[test]
fn a_reader_matches_the_namespace_and_never_the_prefix() {
    // The manifest, the document and the state with the default namespace and another prefix.
    let mut items = vector_1();
    replace(
        &mut items,
        "manifest.xml",
        part(&format!(
            "<m:manifest xmlns:m=\"{NS}\" format=\"1.0\" kind=\"drawing\"><m:part path=\"document.xml\" role=\"document\"/><m:part path=\"pages/page1.xml\" role=\"page\"/><m:part path=\"state.xml\" role=\"state\"/></m:manifest>"
        )),
    );
    replace(&mut items, "state.xml", part(&format!("<state xmlns=\"{NS}\"><ledger page=\"1\" mark=\"4\"/></state>")));
    let (_, opened) = open_ok(&package(&items));
    assert_eq!(opened.state.ledger.0.get(&1), Some(&4));
    // An element of another namespace with the right local name is not a row: the entry it names is unlisted.
    let mut items = vector_1();
    replace(
        &mut items,
        "manifest.xml",
        part(&format!(
            "<diacraft:manifest xmlns:diacraft=\"{NS}\" format=\"1.0\" kind=\"drawing\"><x:part xmlns:x=\"urn:other\" path=\"document.xml\" role=\"document\"/></diacraft:manifest>"
        )),
    );
    assert_eq!(refused(&package(&items)).class(), "entry_unlisted");
}

#[test]
fn an_unedited_save_raw_copies_every_part() {
    let (p, q) = (png(1, 40), png(2, 80));
    let mut doc = drawing_of(vec![page_showing(1, &[&p]), page_showing(2, &[&q]), page_showing(3, &[&p, &q])]);
    doc.masters = diacraft_doc::SharedVec::from_vec(vec![Arc::new(master(1, "M"))]);
    let first = write(&doc, &State::default(), None, &WriteOptions::default()).unwrap();
    assert_eq!(first.copied, 0, "nothing to copy from");
    let (back, opened) = open_ok(&first.bytes);
    // Written again from the model the file gave, with what the read learned: every page, master and blob is copied, and the file is the file that was read.
    let second = with_previous(&back, &opened.state, &first.bytes, &opened.decoded);
    assert_eq!(second.copied, 3 + 1 + 2, "pages + masters + blobs");
    assert_eq!(second.bytes, first.bytes);
    // And written again from what the write itself learned: the same, again.
    let third = with_previous(&doc, &State::default(), &first.bytes, &first.decoded);
    assert_eq!(third.copied, 3 + 1 + 2);
    assert_eq!(third.bytes, first.bytes);
    assert_eq!(second.decoded.pages(), 3);
    // With a preview: the first write gives one, and a write that changed nothing copies it, one more entry.
    let preview: Vec<u8> = [&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], b"first page"].concat();
    let with = write(&doc, &State::default(), None, &WriteOptions { preview: Some(preview.clone()), ..WriteOptions::default() }).unwrap();
    let (back, opened) = open_ok(&with.bytes);
    assert!(opened.decoded.has_preview() && with.decoded.has_preview(), "a package that holds a preview says so, as read and as written");
    assert!(!second.decoded.has_preview() && !first.decoded.has_preview(), "and one that holds none says so");
    let again = with_previous(&back, &opened.state, &with.bytes, &opened.decoded);
    assert_eq!(again.copied, 3 + 1 + 2 + 1, "the preview too");
    assert_eq!(again.bytes, with.bytes);
    assert_eq!(entry(&again.bytes, "preview.png"), preview);
    // A model that changed does not copy the preview as it was.
    let mut changed = back.clone();
    changed.title = Arc::from("changed");
    let after = with_previous(&changed, &opened.state, &with.bytes, &opened.decoded);
    assert!(!names(&after.bytes).contains(&"preview.png".to_owned()), "a changed model is not given the old picture");
    assert_eq!(after.copied, 3 + 1 + 2, "the pages, masters and blobs still copy");
}

#[test]
fn a_save_that_changed_one_page_rewrites_that_page_and_no_other() {
    let (p, q) = (png(1, 40), png(2, 80));
    let doc = drawing_of(vec![page_showing(1, &[&p]), page_showing(2, &[&q]), page_showing(3, &[&p])]);
    let first = write(&doc, &State::default(), None, &WriteOptions::default()).unwrap();
    let (mut back, opened) = open_ok(&first.bytes);
    // A new name for page 2: its object is another allocation, and only it is written again.
    let mut page = (**back.pages.get(1).unwrap()).clone();
    page.name = Arc::from("Renamed");
    back.pages.set(1, Arc::new(page)).unwrap();
    let second = with_previous(&back, &opened.state, &first.bytes, &opened.decoded);
    assert_eq!(second.copied, 2 + 2, "two pages and two blobs are copied");
    assert_eq!(entry(&second.bytes, "pages/page1.xml"), entry(&first.bytes, "pages/page1.xml"));
    assert_eq!(entry(&second.bytes, "pages/page3.xml"), entry(&first.bytes, "pages/page3.xml"));
    assert_ne!(entry(&second.bytes, "pages/page2.xml"), entry(&first.bytes, "pages/page2.xml"));
    let (reopened, _) = open_ok(&second.bytes);
    assert_eq!(&*reopened.pages.get(1).unwrap().name, "Renamed");
    assert_eq!(picture_of(&reopened, 1, 0).as_deref(), Some(&q[..]));
}

#[test]
fn raw_copied_page_keeps_its_pictures_after_renumbering() {
    // Review Focus 1. Blobs are numbered by first use, so a part copied as it is can name the wrong picture. Pages 1 to 3, the picture P only on page 3.
    let (p, q) = (png(7, 60), png(9, 90));
    let doc = drawing_of(vec![Page::new(1, "One"), Page::new(2, "Two"), page_showing(3, &[&p])]);
    let first = write(&doc, &State::default(), None, &WriteOptions::default()).unwrap();
    let (mut back, opened) = open_ok(&first.bytes);
    assert_eq!(picture_of(&back, 2, 0).as_deref(), Some(&p[..]));
    // Add the picture Q to page 1 after the open: Q is blob 0 now, and P is blob 1.
    let mut one = (**back.pages.get(0).unwrap()).clone();
    one.shapes = ShapeList::from_vec(vec![Arc::new(picture_shape_of(1, "media/q.png", &q))]);
    back.pages.set(0, Arc::new(one)).unwrap();
    let second = with_previous(&back, &opened.state, &first.bytes, &opened.decoded);
    // Page 2 is copied (it names no blob); page 3 is NOT: it named blob 0, which is Q's number now.
    assert_eq!(second.copied, 1, "only page 2 is copied: P is blob 1 now and was blob 0, so its entry is written again too");
    let reopened = open_ok(&second.bytes).0;
    assert_eq!(picture_of(&reopened, 0, 0).as_deref(), Some(&q[..]), "page 1 shows Q");
    assert_eq!(picture_of(&reopened, 2, 0).as_deref(), Some(&p[..]), "page 3 shows its own picture, P");
    let page_3 = String::from_utf8(entry(&second.bytes, "pages/page3.xml")).unwrap();
    assert!(page_3.contains("blob=\"1\""), "page 3 names the number P has now: {page_3}");
    // The write after it knows the new numbers, and copies everything.
    let (back, opened) = open_ok(&second.bytes);
    let third = with_previous(&back, &opened.state, &second.bytes, &opened.decoded);
    assert_eq!(third.copied, 3 + 2);
    assert_eq!(third.bytes, second.bytes);
}

#[test]
fn raw_copied_page_keeps_its_pictures_after_the_last_shape_that_showed_an_earlier_blob_is_deleted() {
    // Review Focus 1, the other way: delete the last shape that showed a blob numbered before P's. Page 1 shows R (blob 0), page 3 shows P (blob 1).
    let (r, p) = (png(3, 30), png(4, 50));
    let doc = drawing_of(vec![page_showing(1, &[&r]), Page::new(2, "Two"), page_showing(3, &[&p])]);
    let first = write(&doc, &State::default(), None, &WriteOptions::default()).unwrap();
    let (mut back, opened) = open_ok(&first.bytes);
    let mut one = (**back.pages.get(0).unwrap()).clone();
    one.shapes = ShapeList::new();
    back.pages.set(0, Arc::new(one)).unwrap();
    let second = with_previous(&back, &opened.state, &first.bytes, &opened.decoded);
    let reopened = open_ok(&second.bytes).0;
    assert_eq!(picture_of(&reopened, 2, 0).as_deref(), Some(&p[..]), "page 3 shows P and not what the number it named means now");
    assert!(reopened.pages.get(0).unwrap().shapes.is_empty());
    assert_eq!(names(&second.bytes).iter().filter(|n| n.starts_with("blobs/")).count(), 1, "R is no longer written: nothing shows it");
    // Two pages that show equal bytes in distinct allocations, one of them replaced by other bytes of the same size.
    let (a, b) = (png(5, 20), png(6, 20));
    let doc = drawing_of(vec![page_showing(1, &[&a]), page_showing(2, &[&b])]);
    let first = write(&doc, &State::default(), None, &WriteOptions::default()).unwrap();
    let (mut back, opened) = open_ok(&first.bytes);
    let swapped = png(5, 20);
    let mut one = (**back.pages.get(0).unwrap()).clone();
    one.shapes = ShapeList::from_vec(vec![Arc::new(picture_shape_of(1, "media/image1.png", &swapped))]);
    back.pages.set(0, Arc::new(one)).unwrap();
    let second = with_previous(&back, &opened.state, &first.bytes, &opened.decoded);
    let reopened = open_ok(&second.bytes).0;
    assert_eq!(picture_of(&reopened, 0, 0).as_deref(), Some(&a[..]));
    assert_eq!(picture_of(&reopened, 1, 0).as_deref(), Some(&b[..]));
}

#[test]
fn a_second_save_is_no_edit() {
    // The same model written twice gives the same bytes, and a model read back and written again gives the same bytes, with and without what the read learned.
    let (p, q) = (png(1, 40), png(2, 80));
    let docs = vec![Document::new_blank(), drawing_of(vec![page_showing(1, &[&p]), page_showing(2, &[&q, &p])]), {
        let mut d = drawing_of(vec![page_showing(4, &[&q])]);
        d.masters = diacraft_doc::SharedVec::from_vec(vec![Arc::new(master(1, "M"))]);
        d
    }];
    for doc in docs {
        let first = write_doc(&doc);
        assert_eq!(first, write_doc(&doc), "the same model, the same bytes");
        let (back, opened) = open_ok(&first);
        let again = write(&back, &opened.state, None, &WriteOptions::default()).unwrap();
        assert_eq!(again.bytes, first, "after a reopen, from nothing");
        let copied = with_previous(&back, &opened.state, &first, &opened.decoded);
        assert_eq!(copied.bytes, first, "after a reopen, copying");
    }
}

#[test]
fn the_models_the_repository_makes_come_back_from_their_package() {
    for (name, doc) in fixture_documents() {
        let bytes = write_doc(&doc);
        let (back, opened) = open_ok(&bytes);
        let (want, got) = (without_derived_theme_data(&doc), without_derived_theme_data(&back));
        assert!(want == got, "{name} comes back as it went");
        assert!(opened.notes.is_empty(), "{name}: {:?}", opened.notes);
        let again = write_doc(&back);
        let (twice, _) = open_ok(&again);
        assert_eq!(write_doc(&twice), again, "{name}: a second write is the first's");
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: std::env::var("NATIVE_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(24), ..ProptestConfig::default() })]

    #[test]
    fn a_generated_model_comes_back_from_its_package(doc in any_document(ModelSize::Small)) {
        // A part of an unknown role is written at a path of its own, which the generator's names (`/p0.bin`, for the VSDX writer) are not: those documents are the
        // tests of `versions.rs`.
        prop_assume!(doc.parts.iter().all(|p| p.format != diacraft_doc::Format::Native));
        let first = write(&doc, &State::default(), None, &WriteOptions::default());
        let first = match first {
            Ok(w) => w,
            // A model the codec cannot save is refused in words and never aborts.
            Err(e) => {
                prop_assert!(!e.to_string().is_empty());
                return Ok(());
            }
        };
        let (back, opened) = open_ok(&first.bytes);
        prop_assert!(back == doc, "the model comes back as it went: {:?}", first_difference(&back, &doc));
        prop_assert!(opened.state.content_stamp.is_none());
        // A fixed point after one read and one write (the codec's contract), and a second save is no edit.
        let second = write(&back, &opened.state, None, &WriteOptions::default()).unwrap();
        let (again, opened_again) = open_ok(&second.bytes);
        let third = write(&again, &opened_again.state, None, &WriteOptions::default()).unwrap();
        prop_assert!(second.bytes == third.bytes, "a second save is no edit");
        let copied = write(&again, &opened_again.state, Some(&Previous { bytes: &second.bytes, decoded: &opened_again.decoded }), &WriteOptions::default()).unwrap();
        prop_assert!(copied.bytes == second.bytes, "and copying changes no byte");
    }
}

#[test]
fn a_page_with_a_shape_of_a_group_keeps_the_ledger_raised_to_its_highest_id() {
    // The ledger is written raised to the highest shape ID of each page, nested shapes included.
    let mut group = Shape::new(40);
    group.children = ShapeList::from_vec(vec![Arc::new(Shape::new(41)), Arc::new(Shape::new(7))]);
    let mut page = Page::new(1, "P");
    page.shapes = ShapeList::from_vec(vec![Arc::new(Shape::new(3)), Arc::new(group)]);
    let doc = drawing_of(vec![page, Page::new(2, "Q")]);
    let bytes = write_doc(&doc);
    let state = String::from_utf8(entry(&bytes, "state.xml")).unwrap();
    assert!(state.contains("<diacraft:ledger page=\"1\" mark=\"41\"/>"), "{state}");
    assert!(!state.contains("page=\"2\""), "a page with no mark has no row: {state}");
}

#[test]
fn a_media_type_a_manifest_row_cannot_hold_is_written_as_the_record_says_it_and_comes_back() {
    // Found by the generated models: the media type of a picture is any text the model holds. The manifest row of the blob says an octet stream when it cannot hold it
    // (more than 1,024 bytes, or a character XML forbids); the record of the document part holds it exactly, and the read gives it back.
    let bytes = png(1, 30);
    for media in ["x".repeat(5_000), "image/png\u{1}\u{1f}".to_owned(), "\u{fffe}".to_owned(), String::new(), "image/x-\u{e9}-é".to_owned()] {
        let doc = drawing_of(vec![page_of_typed(1, &[(media.as_str(), bytes.clone())])]);
        let written = write_doc(&doc);
        let (back, opened) = open_ok(&written);
        assert!(back == doc, "a picture of the media type {media:?} comes back");
        assert!(opened.notes.is_empty(), "{:?}", opened.notes);
        let listed = rows(&written);
        let blob = listed.iter().find(|r| r.1 == "blob").unwrap();
        let held = media.len() <= 1_024 && !media.chars().any(|c| (c as u32) < 0x20 || c == '\u{fffe}');
        assert_eq!(blob.3.as_deref(), Some(if held { media.as_str() } else { "application/octet-stream" }));
    }
}
