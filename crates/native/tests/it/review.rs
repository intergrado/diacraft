//! The findings of the full review of 5987bb75, each a test that failed on it (B1, H1, H2, M1 to M5 and the low findings). A package here is made by this code,
//! and where a test needs one that another program zipped, it is a package this writer made and the ZIP builder wrote again with the headers a tool of its own would give.
use std::sync::Arc;

use diacraft_container::Limits as ContainerLimits;
use diacraft_container::zip::{Archive, Method};
use diacraft_doc::{Document, Page, ShapeList};
use diacraft_native::{Limits, NativeError, Previous, ReadNote, State, WriteOptions, read, write};

use crate::util::*;
use crate::zipbuild::Item;

fn save_from(doc: &Document, state: &State, bytes: &[u8], decoded: &diacraft_native::Decoded) -> diacraft_native::Written {
    write(doc, state, Some(&Previous { bytes, decoded }), &WriteOptions::default()).unwrap()
}

// ------------------------------------------------------------------------------------------------------------------------------------ B1

/// Every entry of `bytes` has the headers this writer makes (2.1): the fixed date, no extra field, no comment, no data descriptor, the UTF-8 flag where the name needs it,
/// and the version, attributes and name of a header that nothing else wrote.
fn assert_ours(bytes: &[u8], what: &str) {
    let archive = Archive::read(bytes, &ContainerLimits::default()).unwrap();
    for e in archive.entries() {
        let version = if e.method == 0 { 10 } else { 20 };
        assert_eq!((e.date, e.time), (0x0021, 0), "{what}: {}", e.name);
        assert_eq!(e.local_extra_len, 0, "{what}: the local header of {}", e.name);
        assert_eq!(e.row_len, 46 + e.name.len(), "{what}: the row of {} has no extra field and no comment", e.name);
        assert_eq!(e.row_extra_and_comment(bytes), Some((&[][..], &[][..])), "{what}: {}", e.name);
        assert_eq!(e.flags, if e.name.is_ascii() { 0 } else { 0x0800 }, "{what}: the flags of {}", e.name);
        assert_eq!((e.version_made_by, e.version_needed), (version, version), "{what}: {}", e.name);
        assert_eq!((e.internal_attributes, e.external_attributes), (0, 0), "{what}: {}", e.name);
    }
}

/// The package `bytes` zipped again, as a tool of its own would: every deflated entry with the headers this writer makes (so it is one to copy) except where `touch` changes it.
fn zipped_again(bytes: &[u8], mut touch: impl FnMut(&str, &mut Item)) -> Vec<u8> {
    let methods = methods(bytes);
    let items: Vec<Item> = entries(bytes)
        .into_iter()
        .zip(methods)
        .map(|((name, data), (_, method))| {
            let mut item = if method == Some(Method::Stored) { Item::stored(&name, &data) } else { Item::deflated(&name, &data) };
            item.made_by = 20;
            touch(&name, &mut item);
            item
        })
        .collect();
    package(&items)
}

fn two_pages() -> (Document, Vec<u8>) {
    let mut doc = drawing_of(vec![Page::new(1, "One"), Page::new(2, "Two")]);
    doc.title = Arc::from("Two pages");
    let bytes = write_doc(&doc);
    (doc, bytes)
}

#[test]
fn an_entry_whose_headers_are_this_writers_is_copied_and_every_other_is_written_afresh() {
    // The control: a package zipped again with the headers this writer makes is copied, entry for entry.
    let (doc, ours) = two_pages();
    let again = zipped_again(&ours, |_, _| {});
    let (back, opened) = open_ok(&again);
    let copied = save_from(&back, &opened.state, &again, &opened.decoded);
    assert_eq!(copied.copied, 2, "both pages");
    assert_ours(&copied.bytes, "control");
    // Each header field another tool may give an entry of a page, on its own: the entry is written afresh under this writer's headers (2.1), and the other page is still copied.
    type Touch = fn(&mut Item);
    let variants: [(&str, Touch); 10] = [
        ("a date", |i| i.date = 0x5A21),
        ("a time", |i| i.time = 0x6000),
        ("a version made by", |i| i.made_by = 0x033F),
        ("an internal attribute", |i| i.internal = 1),
        ("an external attribute", |i| i.external = 0o100_644 << 16),
        ("an extra field in the row", |i| i.extra = vec![0x55, 0x54, 5, 0, 1, 0, 0, 0, 0]),
        ("an extra field in the local header", |i| i.local_extra = vec![0x55, 0x54, 5, 0, 1, 0, 0, 0, 0]),
        ("a comment", |i| i.comment = b"made elsewhere".to_vec()),
        ("the UTF-8 flag on a name that is ASCII", |i| i.flags |= 0x0800),
        ("a data descriptor flag", |i| i.flags |= 0x0008),
    ];
    for (what, touch) in variants {
        let theirs = zipped_again(&ours, |name, item| {
            if name == "pages/page1.xml" {
                touch(item);
            }
        });
        let (back, opened) = open_ok(&theirs);
        assert!(back == doc, "{what}: the package reads");
        let saved = save_from(&back, &opened.state, &theirs, &opened.decoded);
        assert_ours(&saved.bytes, what);
        assert_eq!(saved.copied, 1, "{what}: only the page whose headers are this writer's is copied");
        assert_eq!(entry(&saved.bytes, "pages/page1.xml"), entry(&ours, "pages/page1.xml"), "{what}: and the page is the same");
        assert!(open_ok(&saved.bytes).0 == doc, "{what}: and reads back");
    }
    // A file that another tool zipped whole has none of its entries copied.
    let theirs = zipped_again(&ours, |_, item| {
        item.date = 0x5A21;
        item.time = 0x6000;
        item.comment = b"made elsewhere".to_vec();
        item.extra = vec![0x55, 0x54, 5, 0, 1, 0, 0, 0, 0];
    });
    let (back, opened) = open_ok(&theirs);
    let saved = save_from(&back, &opened.state, &theirs, &opened.decoded);
    assert_eq!(saved.copied, 0);
    assert_ours(&saved.bytes, "every entry another tool's");
    // An unedited save of this writer's own file is still the file, byte for byte.
    let (back, opened) = open_ok(&ours);
    assert_eq!(save_from(&back, &opened.state, &ours, &opened.decoded).bytes, ours);
}

#[test]
fn an_entry_that_is_not_in_the_method_this_writer_picks_is_written_afresh() {
    // 2.6: a blob that starts as a PNG is stored; 2.2: an XML part is deflated. A tool that did the other way gave headers that are otherwise this writer's.
    let p = png(3, 400);
    let doc = drawing_of(vec![page_showing(1, &[&p]), Page::new(2, "Two")]);
    let ours = write_doc(&doc);
    assert_eq!(methods(&ours).iter().find(|(n, _)| n == "blobs/0.png").map(|(_, m)| *m), Some(Some(Method::Stored)));
    let theirs = package(
        &entries(&ours)
            .into_iter()
            .map(|(name, data)| {
                let mut item = match name.as_str() {
                    "mimetype" | "pages/page2.xml" => Item::stored(&name, &data),
                    _ => Item::deflated(&name, &data),
                };
                item.made_by = 20;
                item
            })
            .collect::<Vec<_>>(),
    );
    let (back, opened) = open_ok(&theirs);
    let saved = save_from(&back, &opened.state, &theirs, &opened.decoded);
    let method = |name: &str| methods(&saved.bytes).into_iter().find(|(n, _)| n == name).map(|(_, m)| m).unwrap();
    assert_eq!(method("blobs/0.png"), Some(Method::Stored), "a PNG is stored whoever zipped it");
    assert_eq!(method("pages/page2.xml"), Some(Method::Deflated), "an XML part is deflated whoever zipped it");
    assert_eq!(method("pages/page1.xml"), Some(Method::Deflated));
    assert_ours(&saved.bytes, "other methods");
    assert!(open_ok(&saved.bytes).0 == back);
}

// ------------------------------------------------------------------------------------------------------------------------------------ H1

#[test]
fn a_part_named_twice_by_the_lists_is_damage() {
    // 3.3 and 2.10: the lists are exact. The same page named twice, a page and a master that name one entry, and a master named twice.
    let mut doc = drawing_of(vec![Page::new(1, "One"), Page::new(2, "Two")]);
    doc.masters = diacraft_doc::SharedVec::from_vec(vec![Arc::new(master(1, "M")), Arc::new(master(2, "N"))]);
    let ours = write_doc(&doc);
    let with_document = |change: &dyn Fn(String) -> String| {
        let items =
            rebuilt(
                &ours,
                |name, content| {
                    if name == "document.xml" { Some(change(String::from_utf8(content).unwrap()).into_bytes()) } else { Some(content) }
                },
            );
        package(&items)
    };
    let twice_page = with_document(&|d| {
        d.replace("<diacraft:page-ref id=\"2\" part=\"pages/page2.xml\"/>", "<diacraft:page-ref id=\"2\" part=\"pages/page1.xml\"/>")
    });
    let twice_master = with_document(&|d| {
        d.replace("<diacraft:master-ref id=\"2\" part=\"masters/master2.xml\"/>", "<diacraft:master-ref id=\"2\" part=\"masters/master1.xml\"/>")
    });
    let across = with_document(&|d| {
        d.replace("<diacraft:master-ref id=\"2\" part=\"masters/master2.xml\"/>", "<diacraft:master-ref id=\"2\" part=\"pages/page1.xml\"/>")
    });
    for (what, bytes) in [("a page named twice", twice_page), ("a master named twice", twice_master), ("a name in both lists", across)] {
        let error = refused(&bytes);
        assert!(["part_listed_twice", "part_missing", "part_unlisted"].contains(&error.class()), "{what}: {error}");
    }
    // A document part that lists the same page entry twice and still names every page and master entry once is the case a set of names cannot see.
    let listed_twice = with_document(&|d| {
        d.replace(
            "<diacraft:page-ref id=\"2\" part=\"pages/page2.xml\"/>",
            "<diacraft:page-ref id=\"2\" part=\"pages/page2.xml\"/><diacraft:page-ref id=\"2\" part=\"pages/page2.xml\"/>",
        )
    });
    let error = refused(&listed_twice);
    assert_eq!(error.class(), "part_listed_twice", "{error}");
    assert!(error.to_string().contains("damaged"), "{error}");
    let twice_master = with_document(&|d| {
        d.replace(
            "<diacraft:master-ref id=\"2\" part=\"masters/master2.xml\"/>",
            "<diacraft:master-ref id=\"2\" part=\"masters/master2.xml\"/><diacraft:master-ref id=\"2\" part=\"masters/master2.xml\"/>",
        )
    });
    assert_eq!(refused(&twice_master).class(), "part_listed_twice");
    // The same entry in the page list and the master list.
    let both = with_document(&|d| {
        d.replace(
            "<diacraft:master-ref id=\"2\" part=\"masters/master2.xml\"/>",
            "<diacraft:master-ref id=\"2\" part=\"masters/master2.xml\"/><diacraft:master-ref id=\"9\" part=\"pages/page2.xml\"/>",
        )
    });
    assert_eq!(refused(&both).class(), "part_listed_twice");
}

// ------------------------------------------------------------------------------------------------------------------------------------ H2

#[test]
fn a_page_is_not_copied_when_the_picture_it_names_is_another_of_the_same_size() {
    // H02: the guard is the allocation and not the size. P and Q are one size; Q is added to page 1, so Q is blob 0 and P is blob 1, and page 3 names blob 0.
    let (p, q) = (png(7, 60), png(9, 60));
    assert_eq!(p.len(), q.len());
    let doc = drawing_of(vec![Page::new(1, "One"), Page::new(2, "Two"), page_showing(3, &[&p])]);
    let first = write(&doc, &State::default(), None, &WriteOptions::default()).unwrap();
    let (mut back, opened) = open_ok(&first.bytes);
    let mut one = (**back.pages.get(0).unwrap()).clone();
    one.shapes = ShapeList::from_vec(vec![Arc::new(picture_shape_of(1, "media/q.png", &q))]);
    back.pages.set(0, Arc::new(one)).unwrap();
    let second = save_from(&back, &opened.state, &first.bytes, &opened.decoded);
    let reopened = open_ok(&second.bytes).0;
    assert_eq!(picture_of(&reopened, 0, 0).as_deref(), Some(&q[..]), "page 1 shows Q");
    assert_eq!(picture_of(&reopened, 2, 0).as_deref(), Some(&p[..]), "page 3 shows P, which is blob 1 now");
    assert_eq!(second.copied, 1, "only page 2 is copied");
}

#[test]
fn what_a_copied_page_names_is_remembered_for_the_save_after_it() {
    // H31: save, save again copying, add Q, and save from what the copy remembered: page 3 still names blob 0, which is Q's now, and is written again.
    let (p, q) = (png(7, 60), png(9, 60));
    let doc = drawing_of(vec![Page::new(1, "One"), Page::new(2, "Two"), page_showing(3, &[&p])]);
    let first = write(&doc, &State::default(), None, &WriteOptions::default()).unwrap();
    let (back, opened) = open_ok(&first.bytes);
    let copied = save_from(&back, &opened.state, &first.bytes, &opened.decoded);
    assert_eq!(copied.bytes, first.bytes);
    assert_eq!(copied.copied, 3 + 1, "every page and the blob");
    let mut changed = back.clone();
    let mut one = (**changed.pages.get(0).unwrap()).clone();
    one.shapes = ShapeList::from_vec(vec![Arc::new(picture_shape_of(1, "media/q.png", &q))]);
    changed.pages.set(0, Arc::new(one)).unwrap();
    // From what the copy remembered, and from what the first write remembered.
    for decoded in [&copied.decoded, &first.decoded] {
        let third = save_from(&changed, &opened.state, &copied.bytes, decoded);
        let reopened = open_ok(&third.bytes).0;
        assert_eq!(picture_of(&reopened, 0, 0).as_deref(), Some(&q[..]));
        assert_eq!(picture_of(&reopened, 2, 0).as_deref(), Some(&p[..]), "page 3 shows P");
    }
}

#[test]
fn a_blob_is_not_copied_under_a_name_it_does_not_have_now() {
    // H05: the media type of the first use is the extension of the entry. P is shown by pages 1 and 2 as a PNG; page 1 is edited to show it as a JPEG, so the first use is
    // `image/jpeg` and the entry is `blobs/0.jpg`, and page 2 (unchanged, copied) still names blob 0.
    let p = png(5, 80);
    let doc = drawing_of(vec![page_of_typed(1, &[("image/png", p.clone())]), page_of_typed(2, &[("image/png", p.clone())])]);
    let first = write(&doc, &State::default(), None, &WriteOptions::default()).unwrap();
    assert!(names(&first.bytes).contains(&"blobs/0.png".to_owned()));
    let (mut back, opened) = open_ok(&first.bytes);
    let shared = picture_of(&back, 0, 0).unwrap();
    let mut one = (**back.pages.get(0).unwrap()).clone();
    one.shapes = ShapeList::from_vec(vec![Arc::new(typed_picture(1, "image/jpeg", &shared))]);
    back.pages.set(0, Arc::new(one)).unwrap();
    let second = save_from(&back, &opened.state, &first.bytes, &opened.decoded);
    let names = names(&second.bytes);
    assert!(names.contains(&"blobs/0.jpg".to_owned()) && !names.contains(&"blobs/0.png".to_owned()), "{names:?}");
    let (reopened, notes) = open_ok(&second.bytes);
    assert!(notes.notes.is_empty(), "no picture is lost: {:?}", notes.notes);
    assert_eq!(picture_of(&reopened, 0, 0).as_deref(), Some(&p[..]));
    assert_eq!(picture_of(&reopened, 1, 0).as_deref(), Some(&p[..]));
}

#[test]
fn an_extra_part_is_copied_only_while_its_bytes_are_the_allocation_it_was() {
    // H06: a part of an unknown role whose bytes were replaced by others of the same length (the same name and role) is written from the model, not copied.
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
                ("notes/review.txt", "annotation", " media-type=\"text/plain\""),
            ],
        ),
    );
    items.push(Item::deflated("notes/review.txt", b"check the margins"));
    let other = package(&items);
    // The package is this writer's own (the headers of another tool's are never copied, B1), so the part is a candidate for a raw copy.
    let (first, first_opened) = open_ok(&other);
    let own = save_from(&first, &first_opened.state, &other, &first_opened.decoded);
    let bytes = own.bytes;
    let (mut doc, opened) = open_ok(&bytes);
    let part = doc.parts.get(0).unwrap().clone();
    let new = diacraft_doc::Part { bytes: Arc::from(b"CHECK THE MARGINS".to_vec()), ..(*part).clone() };
    doc.parts = diacraft_doc::SharedVec::from_vec(vec![Arc::new(new)]);
    let saved = save_from(&doc, &opened.state, &bytes, &opened.decoded);
    assert_eq!(entry(&saved.bytes, "notes/review.txt"), b"CHECK THE MARGINS");
    // And the part that was not changed is still the entry that was read, copied as it was.
    let (back, opened) = open_ok(&bytes);
    let same = save_from(&back, &opened.state, &bytes, &opened.decoded);
    assert_eq!(entry(&same.bytes, "notes/review.txt"), b"check the margins");
    assert!(same.copied >= 1, "the part is copied once the file is this writer's own");
}

#[test]
fn two_pages_of_one_id_are_copied_by_the_entry_they_were_read_from_and_never_by_their_id() {
    // H04: duplicate page IDs get `pages/page1.xml` and `pages/page1-1.xml`. Swapped, each page is written under the name the list gives it now.
    let doc = drawing_of(vec![Page::new(1, "A"), Page::new(1, "B")]);
    let first = write(&doc, &State::default(), None, &WriteOptions::default()).unwrap();
    assert!(names(&first.bytes).contains(&"pages/page1-1.xml".to_owned()));
    let (mut back, opened) = open_ok(&first.bytes);
    let (a, b) = (back.pages.get(0).unwrap().clone(), back.pages.get(1).unwrap().clone());
    back.pages.set(0, b).unwrap();
    back.pages.set(1, a).unwrap();
    let swapped = save_from(&back, &opened.state, &first.bytes, &opened.decoded);
    let reopened = open_ok(&swapped.bytes).0;
    let order: Vec<&str> = reopened.pages.iter().map(|p| &*p.name).collect::<Vec<_>>();
    assert_eq!(order, ["B", "A"], "the pages come back in the order they were saved in");
    assert!(reopened == back);
}

// ------------------------------------------------------------------------------------------------------------------------------------ M2

#[test]
fn a_blob_that_the_stamp_loses_is_not_remembered() {
    // M2: the entry's bytes are inflated, their stamp is not the record's, so the picture is lost: nothing of it is held for a later save to copy from.
    let mut changed = PICTURE.to_vec();
    *changed.last_mut().unwrap() ^= 0xFF;
    let mut items = vector_5();
    remove(&mut items, "blobs/0.png");
    let at = items.iter().position(|i| i.name == b"state.xml").unwrap();
    items.insert(at, Item::stored("blobs/0.png", &changed));
    let (doc, opened) = open_ok(&package(&items));
    assert_eq!(opened.notes, [ReadNote::PictureLost { count: 1 }]);
    assert_eq!(opened.decoded.blobs(), 0, "a lost blob is no blob of the package");
    assert!(doc.pages.get(0).unwrap().shapes.get(0).unwrap().foreign.as_ref().unwrap().picture.is_none());
    // And the intact one is.
    assert_eq!(open_ok(&package(&vector_5())).1.decoded.blobs(), 1);
}

// ------------------------------------------------------------------------------------------------------------------------------------ M4

#[test]
fn the_directory_of_a_package_is_charged_by_what_it_holds_and_not_by_the_most_it_could() {
    // M4: a package of 5 MB with five entries (a picture of 5 MB, stored) read at the file's own length and 100 MB: the pools are what is left, the directory takes a few
    // kilobytes, and the read opens. The worst case of a file that size (50,000 entries and their names) was 120 MB.
    let picture = patterned(2, 5 << 20);
    let doc = drawing_of(vec![page_showing(1, &[&picture])]);
    let bytes = write_doc(&doc);
    assert!(bytes.len() > 5 << 20 && bytes.len() < 6 << 20);
    let limits = Limits { max_memory: bytes.len() as u64 + (100 << 20), ..Limits::default() };
    let (back, opened) = read(&bytes, &limits).unwrap_or_else(|e| panic!("a package of five entries opens at its own length and 100 MB: {e}"));
    assert!(opened.notes.is_empty(), "{:?}", opened.notes);
    assert_eq!(picture_of(&back, 0, 0).as_deref(), Some(&picture[..]));
}

// ------------------------------------------------------------------------------------------------------------------------------------ M5

#[test]
fn a_blob_whose_entry_is_not_the_size_its_record_states_is_not_inflated() {
    // H09: the check of the size comes before the inflate: the bytes of the entry are not read when the record says another size.
    let honest = open_ok(&package(&vector_5())).1.work.inflated;
    let lying = {
        let mut items = vector_5();
        let document = String::from_utf8(entry(&package(&items), "document.xml")).unwrap().replace("size=\"12\"", "size=\"13\"");
        replace(&mut items, "document.xml", document.into_bytes());
        let (_, opened) = open_ok(&package(&items));
        assert_eq!(opened.notes, [ReadNote::PictureLost { count: 1 }]);
        opened.work.inflated
    };
    assert_eq!(honest - lying, 12, "the entry of the blob is inflated for the record that is honest, and is not for the one that is not");
}

#[test]
fn a_flag_of_the_manifest_is_one_zero_true_or_false() {
    // `parse_flag`: `required` is read as `0` and `false` (not required, so an extra part is kept) and as `1` and `true` (refused naming the role); any other text is damage.
    let with = |flag: &str| {
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
                    ("notes/review.txt", "annotation", &format!(" required=\"{flag}\"")),
                ],
            ),
        );
        items.push(Item::deflated("notes/review.txt", b"x"));
        package(&items)
    };
    for flag in ["0", "false"] {
        assert_eq!(open_ok(&with(flag)).1.kept_parts, 1, "{flag}");
    }
    for flag in ["1", "true"] {
        assert_eq!(refused(&with(flag)), NativeError::RequiredRole("annotation".to_owned()), "{flag}");
    }
    for flag in ["2", "yes", "", "TRUE", "False"] {
        assert_eq!(refused(&with(flag)).class(), "manifest_row", "{flag:?}");
    }
}

#[test]
fn a_class_of_damage_that_has_no_sentence_says_that_a_part_is_not_what_the_format_says() {
    assert_eq!(NativeError::Damaged("no_such_class").to_string(), "the file is damaged: a part of it is not what the format says");
    assert_eq!(NativeError::NotNative.to_string(), "this is not a DiaCraft file");
    assert_eq!(NativeError::Unwritable("why").to_string(), "why");
    assert_eq!(NativeError::TooLarge("the file is large").to_string(), "the file is large");
}

/// The least memory in `0..=1 GiB` at which `ok` holds, by bisection (the answer only rises as the memory falls).
fn least(ok: impl Fn(u64) -> bool) -> u64 {
    let (mut low, mut high) = (0u64, 1u64 << 30);
    assert!(ok(high), "it holds at a gibibyte");
    while low < high {
        let mid = low + (high - low) / 2;
        if ok(mid) { high = mid } else { low = mid + 1 }
    }
    low
}

#[test]
fn the_directory_of_the_previous_package_is_charged_to_the_save() {
    // H26: a save that copies from a package holds its directory beside the one it makes, and asks the memory for it first.
    let (p, q) = (patterned(1, 30_000), patterned(2, 30_000));
    let doc = drawing_of((1..=20).map(|i| page_showing(i, &[if i % 2 == 0 { &p } else { &q }])).collect());
    let first = write(&doc, &State::default(), None, &WriteOptions::default()).unwrap();
    let (back, opened) = open_ok(&first.bytes);
    let try_with = |memory: u64, previous: bool| {
        let options = WriteOptions { limits: Limits { max_memory: memory, ..Limits::default() }, ..WriteOptions::default() };
        let prev = Previous { bytes: &first.bytes, decoded: &opened.decoded };
        write(&back, &opened.state, previous.then_some(&prev), &options).is_ok()
    };
    let (without, with) = (least(|m| try_with(m, false)), least(|m| try_with(m, true)));
    assert!(with > without, "a save that copies needs the memory of the directory it reads: {with} against {without}");
}

#[test]
fn the_package_as_a_whole_is_held_to_the_cap_with_the_extra_parts_the_state_and_the_preview() {
    // H29: the inflated total of a package is every entry (the parts of unknown roles, the state, the preview and the manifest too): written at that cap, refused at one byte
    // less, in words.
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
                ("notes/review.txt", "annotation", " media-type=\"text/plain\""),
            ],
        ),
    );
    items.push(Item::deflated("notes/review.txt", &[b'x'; 5_000]));
    let (doc, opened) = open_ok(&package(&items));
    let preview: Vec<u8> = [&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], &[7u8; 3_000][..]].concat();
    let write_at = |cap: u64| {
        let mut limits = Limits::default();
        limits.container.max_package_bytes = cap;
        write(&doc, &opened.state, None, &WriteOptions { preview: Some(preview.clone()), limits, ..WriteOptions::default() })
    };
    let whole = write_at(1 << 20).unwrap();
    let total: u64 = entries(&whole.bytes).iter().map(|(_, c)| c.len() as u64).sum();
    assert!(write_at(total).is_ok(), "at the total it is written");
    let error = write_at(total - 1).unwrap_err();
    assert_eq!(error.class(), "too_large", "{error}");
    assert!(error.to_string().contains("inflated"), "{error}");
}

// ------------------------------------------------------------------------------------------------------------------------------------ low

/// Four bytes that make the CRC-32 of `prefix`, them and `suffix` equal `target`: the register run back through `suffix`, and then the four bytes of the message.
fn forge_crc32(prefix: &[u8], suffix: &[u8], target: u32) -> [u8; 4] {
    let table: Vec<u32> = (0..256u32).map(|i| (0..8).fold(i, |c, _| if c & 1 == 1 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 })).collect();
    let back = |after: u32, byte: u8| -> u32 {
        let index = table.iter().position(|t| t >> 24 == after >> 24).unwrap() as u32;
        ((after ^ table[index as usize]) << 8) | (index ^ u32::from(byte))
    };
    let mut register = !target;
    for byte in suffix.iter().rev() {
        register = back(register, *byte);
    }
    // Four bytes are four steps of the register xor the message word: run the zero bytes back, and what is left is the word and the register before it.
    for _ in 0..4 {
        register = back(register, 0);
    }
    let mut before = 0xFFFF_FFFFu32;
    for byte in prefix {
        before = table[((before ^ u32::from(*byte)) & 0xFF) as usize] ^ (before >> 8);
    }
    (register ^ before).to_le_bytes()
}

#[test]
fn the_crc_forge_of_the_tests_makes_the_crc_it_is_asked_for() {
    let (prefix, suffix) = (b"<a title=\"x".as_slice(), b"\"/>".as_slice());
    let want = crc32fast::hash(&[prefix, b"ABCD", suffix].concat());
    let forged = forge_crc32(prefix, suffix, want);
    assert_eq!(forged, *b"ABCD");
    assert_eq!(crc32fast::hash(&[prefix, &forged[..], suffix].concat()), want);
}

#[test]
fn the_preview_is_not_copied_when_the_document_part_changed_to_bytes_of_the_same_length_and_checksum() {
    // The document part is held by size and CRC-32, and a CRC-32 is made to collide: two titles of eight characters that write documents of one length and one checksum
    // (the second title's last four characters are the four bytes that the CRC of the whole needs, given the first four). The preview is the picture of the drawing the first
    // title is of, and is not copied for the second.
    let png: Vec<u8> = [&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], b"first title"].concat();
    let mut rng = Rng(0x7171);
    let alphabet: Vec<u8> = (b'a'..=b'z').chain(b'A'..=b'Z').chain(b'0'..=b'9').collect();
    let pick = |rng: &mut Rng, n: usize| -> String { (0..n).map(|_| alphabet[rng.below(alphabet.len())] as char).collect() };
    let document_of = |title: &str| {
        let mut doc = drawing_of(vec![Page::new(1, "One")]);
        doc.title = Arc::from(title);
        doc
    };
    let write_with_preview =
        |doc: &Document| write(doc, &State::default(), None, &WriteOptions { preview: Some(png.clone()), ..WriteOptions::default() }).unwrap();
    let (first_title, second_title, first) = loop {
        let one = pick(&mut rng, 8);
        let other = pick(&mut rng, 4);
        let first = write_with_preview(&document_of(&one));
        let document = entry(&first.bytes, "document.xml");
        let at = String::from_utf8(document.clone()).unwrap().find(&format!("title=\"{one}\"")).unwrap() + "title=\"".len();
        let prefix = [&document[..at], other.as_bytes()].concat();
        let forged = forge_crc32(&prefix, &document[at + 8..], crc32fast::hash(&document));
        if forged.iter().all(|b| alphabet.contains(b)) && other != one[..4] {
            break (one, format!("{other}{}", String::from_utf8(forged.to_vec()).unwrap()), first);
        }
    };
    let second = write_with_preview(&document_of(&second_title));
    assert_ne!(entry(&first.bytes, "document.xml"), entry(&second.bytes, "document.xml"));
    assert_eq!(entry(&first.bytes, "document.xml").len(), entry(&second.bytes, "document.xml").len());
    assert_eq!(
        crc32fast::hash(&entry(&first.bytes, "document.xml")),
        crc32fast::hash(&entry(&second.bytes, "document.xml")),
        "{first_title} and {second_title}"
    );
    // Read the first drawing, give it the second title, and save it without a preview of its own: the old picture is not the picture of this drawing.
    let (mut doc, opened) = open_ok(&first.bytes);
    doc.title = Arc::from(second_title.as_str());
    let saved = save_from(&doc, &opened.state, &first.bytes, &opened.decoded);
    assert!(!names(&saved.bytes).contains(&"preview.png".to_owned()), "a drawing that changed is not given the picture of the one before");
    // The drawing that did not change is.
    let (same, opened) = open_ok(&first.bytes);
    assert!(names(&save_from(&same, &opened.state, &first.bytes, &opened.decoded).bytes).contains(&"preview.png".to_owned()));
}

#[test]
fn an_extra_part_written_afresh_keeps_the_method_it_had() {
    // 2.2: an extra part is stored "as it was". A part that another tool stored, or deflated, is written again by the method it had, whatever its bytes begin with.
    let part_bytes: [&[u8]; 2] = [
        b"check the margins, which is text that is deflated by the rule",
        b"\x89PNG\r\n\x1a\nnot an image but it starts as one, and the rule would store it",
    ];
    for (item, want) in
        [(Item::stored("notes/review.txt", part_bytes[0]), Method::Stored), (Item::deflated("notes/review.txt", part_bytes[1]), Method::Deflated)]
    {
        let bytes = if want == Method::Stored { part_bytes[0] } else { part_bytes[1] };
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
                    ("notes/review.txt", "annotation", " media-type=\"text/plain\""),
                ],
            ),
        );
        let mut item = item;
        item.date = 0x5A21;
        items.push(item);
        let package = package(&items);
        let (doc, opened) = open_ok(&package);
        let saved = save_from(&doc, &opened.state, &package, &opened.decoded);
        let method = methods(&saved.bytes).into_iter().find(|(n, _)| n == "notes/review.txt").map(|(_, m)| m).unwrap();
        assert_eq!(method, Some(want), "the part of {} bytes keeps its method", bytes.len());
        assert_eq!(entry(&saved.bytes, "notes/review.txt"), bytes);
        assert_ours(&saved.bytes, "a part of an unknown role");
    }
}

#[test]
fn a_blob_that_no_record_names_in_a_newer_minor_and_fails_its_checksum_is_a_lost_blob() {
    // 2.10: damage confined to blobs does not refuse the file. In a newer minor an unnamed blob is held as a part, and a part whose checksum fails is lost, and said.
    let mut bad = Item::deflated("blobs/9.bin", b"nobody names me");
    bad.crc ^= 1;
    let mut items = vector_1();
    replace(
        &mut items,
        "manifest.xml",
        manifest(
            "1.9",
            "drawing",
            &[
                ("document.xml", "document", ""),
                ("pages/page1.xml", "page", ""),
                ("blobs/9.bin", "blob", " media-type=\"application/octet-stream\""),
                ("state.xml", "state", ""),
            ],
        ),
    );
    items.push(bad);
    let (doc, opened) = open_ok(&package(&items));
    assert!(doc.parts.is_empty());
    assert_eq!(opened.kept_parts, 0);
    assert_eq!(opened.notes, [ReadNote::PartLost { count: 1 }]);
    // A part of an unknown role that fails its checksum is damage as it was.
    let mut bad = Item::deflated("notes/review.txt", b"x");
    bad.crc ^= 1;
    let mut items = vector_1();
    replace(
        &mut items,
        "manifest.xml",
        manifest(
            "1.9",
            "drawing",
            &[("document.xml", "document", ""), ("pages/page1.xml", "page", ""), ("state.xml", "state", ""), ("notes/review.txt", "annotation", "")],
        ),
    );
    items.push(bad);
    assert_eq!(refused(&package(&items)).class(), "zip_corrupt");
}

#[test]
fn an_entry_named_like_one_of_the_package_has_the_role_of_it() {
    // `state.xml`, `preview.png` and `document.xml` listed as blobs or as anything else are damage: the entry is not the state, the preview or the document otherwise.
    for (path, role) in [
        ("state.xml", "blob"),
        ("preview.png", "blob"),
        ("document.xml", "blob"),
        ("state.xml", "annotation"),
        ("document.xml", "page"),
        ("preview.png", "state"),
    ] {
        let mut items = vector_1();
        let rows: Vec<(&str, &str, &str)> =
            [("document.xml", "document"), ("pages/page1.xml", "page"), ("state.xml", "state"), ("preview.png", "preview")]
                .into_iter()
                .map(|(p, r)| if p == path { (p, role, "") } else { (p, r, "") })
                .collect();
        replace(&mut items, "manifest.xml", manifest("1.2", "drawing", &rows));
        items.push(Item::deflated("preview.png", b"\x89PNG\r\n\x1a\n"));
        let error = read(&package(&items), &limits()).err();
        assert!(error.as_ref().is_some_and(|e| e.class() == "manifest_row"), "{path} as {role}: {error:?}");
    }
}

#[test]
fn a_version_of_ten_digits_or_more_is_a_version_and_a_larger_major_is_named() {
    let with = |format: &str| {
        let mut items = vector_1();
        replace(
            &mut items,
            "manifest.xml",
            manifest(format, "drawing", &[("document.xml", "document", ""), ("pages/page1.xml", "page", ""), ("state.xml", "state", "")]),
        );
        package(&items)
    };
    assert_eq!(refused(&with("4294967295.0")).to_string(), "made by a newer DiaCraft (format 4294967295.0)");
    assert_eq!(refused(&with("9999999999.7")).to_string(), "made by a newer DiaCraft (format 9999999999.7)");
    assert_eq!(refused(&with("9999999999999999999.0")).to_string(), "made by a newer DiaCraft (format 9999999999999999999.0)");
    // A minor of that size opens, and the version says the largest it can.
    assert_eq!(open_ok(&with("1.4294967295")).1.format, (1, 4_294_967_295));
    assert_eq!(open_ok(&with("1.9999999999")).1.format, (1, u32::MAX));
    // More than nineteen digits is not a version.
    assert_eq!(refused(&with("99999999999999999999.0")).class(), "manifest_version");
}

#[test]
fn a_part_of_an_unknown_role_is_charged_to_the_pool_of_blobs_before_it_is_read() {
    // H14: the bytes of a part kept as bytes are held beside the blobs, and are taken from their pool by the size the entry states. A read that is short of memory shrinks the pool
    // (to about two megabytes here), a picture of 100 kB and a part of a megabyte fit it, and a part of three do not.
    let read_with = |extra: usize| {
        let mut doc = drawing_of(vec![page_showing(1, &[&patterned(3, 100_000)])]);
        let part = diacraft_doc::Part {
            format: diacraft_doc::Format::Native,
            name: Arc::from("notes/big.bin"),
            media_type: Arc::from("application/octet-stream"),
            role: diacraft_doc::PartRole::Other,
            manifest: diacraft_doc::RawAttrs::from_pairs([("role", "annotation")]),
            rels: Arc::new(Vec::new()),
            bytes: patterned(4, extra),
        };
        doc.parts = diacraft_doc::SharedVec::from_vec(vec![Arc::new(part)]);
        let bytes = write_doc(&doc);
        let limits = Limits { max_memory: bytes.len() as u64 + (10 << 20), ..Limits::default() };
        (read(&bytes, &limits), doc)
    };
    let (small, doc) = read_with(1 << 20);
    let (back, opened) = small.unwrap_or_else(|e| panic!("a part of a megabyte fits: {e}"));
    assert_eq!(opened.kept_parts, 1);
    assert!(back == doc);
    let (large, _) = read_with(3 << 20);
    let error = large.err().unwrap_or_else(|| panic!("a part of three megabytes does not fit the pool the memory leaves"));
    assert_eq!(error.class(), "model_too_complex", "{error}");
    assert!(error.to_string().contains("pictures and parts"), "{error}");
}

/// A preview picture: a PNG signature and `tag`.
fn preview_png(tag: &[u8]) -> Vec<u8> {
    [&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], tag].concat()
}

/// The headers another tool gives every entry: a date, a time, an extra field and a comment.
fn foreign(_: &str, item: &mut Item) {
    item.date = 0x5A21;
    item.time = 0x6000;
    item.comment = b"made elsewhere".to_vec();
    item.extra = vec![0x55, 0x54, 5, 0, 1, 0, 0, 0, 0];
}

#[test]
fn a_preview_of_a_file_another_tool_zipped_is_kept_while_the_drawing_is_unchanged() {
    // 2.8: "A writer whose drawing is unchanged since it read the file copies the preview as it was." A file whose entries another tool zipped has none of them copied (B1), and
    // the preview went with them: the first save lost it. It is written again, stored, from the bytes the file held, when the drawing is what it read.
    let preview = preview_png(b"the first page");
    let mut doc = drawing_of(vec![Page::new(1, "One"), Page::new(2, "Two")]);
    doc.title = Arc::from("Two pages");
    let ours = write(&doc, &State::default(), None, &WriteOptions { preview: Some(preview.clone()), ..WriteOptions::default() }).unwrap().bytes;
    let theirs = zipped_again(&ours, foreign);
    assert_ne!(theirs, ours);
    let (back, opened) = open_ok(&theirs);
    let saved = save_from(&back, &opened.state, &theirs, &opened.decoded);
    assert_eq!(saved.copied, 0, "an entry that another tool zipped is written afresh");
    assert_eq!(entry(&saved.bytes, "preview.png"), preview, "the preview is kept");
    assert_ours(&saved.bytes, "the first save of a foreign file");
    assert_eq!(saved.bytes, ours, "and the file is the one this writer makes of that drawing and that picture");
    assert_eq!(methods(&saved.bytes).iter().find(|(n, _)| n == "preview.png").map(|(_, m)| *m), Some(Some(Method::Stored)));
    // The save after it copies everything, the preview too, and is the same file.
    let (again, opened) = open_ok(&saved.bytes);
    let second = save_from(&again, &opened.state, &saved.bytes, &opened.decoded);
    assert_eq!(second.copied, 2 + 1, "both pages and the preview");
    assert_eq!(second.bytes, saved.bytes);
    // A preview that is the only entry another tool changed: the pages are copied and the preview is written again.
    let theirs = zipped_again(&ours, |name, item| {
        if name == "preview.png" {
            foreign(name, item);
        }
    });
    let (back, opened) = open_ok(&theirs);
    let saved = save_from(&back, &opened.state, &theirs, &opened.decoded);
    assert_eq!(saved.copied, 2, "the pages");
    // (The pages were copied as the other tool deflated them, so the bytes are not `ours` but every entry holds what it held.)
    assert_eq!(entries(&saved.bytes), entries(&ours));
    assert_ours(&saved.bytes, "a preview written again beside pages that are copied");
    // A preview that the other tool deflated is written stored (2.8).
    let theirs = zipped_again(&ours, |name, item| {
        if name == "preview.png" {
            *item = Item::deflated(name, &preview);
            item.made_by = 20;
        }
    });
    let (back, opened) = open_ok(&theirs);
    let saved = save_from(&back, &opened.state, &theirs, &opened.decoded);
    assert_eq!(entries(&saved.bytes), entries(&ours));
    assert_eq!(methods(&saved.bytes).iter().find(|(n, _)| n == "preview.png").map(|(_, m)| *m), Some(Some(Method::Stored)));
    assert_ours(&saved.bytes, "a preview that was deflated");
}

#[test]
fn a_preview_of_a_file_another_tool_zipped_is_not_kept_when_the_drawing_changed_or_it_is_no_picture() {
    let preview = preview_png(b"the first page");
    let doc = drawing_of(vec![Page::new(1, "One"), Page::new(2, "Two")]);
    let ours = write(&doc, &State::default(), None, &WriteOptions { preview: Some(preview.clone()), ..WriteOptions::default() }).unwrap().bytes;
    let theirs = zipped_again(&ours, foreign);
    // The drawing changed: the old picture is not its picture.
    let (mut back, opened) = open_ok(&theirs);
    back.title = Arc::from("changed");
    let saved = save_from(&back, &opened.state, &theirs, &opened.decoded);
    assert!(!names(&saved.bytes).contains(&"preview.png".to_owned()));
    assert!(open_ok(&saved.bytes).0 == back, "and the file reads back");
    // A page edited is a change too.
    let (mut back, opened) = open_ok(&theirs);
    let mut pages: Vec<Arc<Page>> = back.pages.iter().cloned().collect();
    let mut first = (*pages[0]).clone();
    first.name = Arc::from("Renamed");
    pages[0] = Arc::new(first);
    back.pages = diacraft_doc::SharedVec::from_vec(pages);
    let saved = save_from(&back, &opened.state, &theirs, &opened.decoded);
    assert!(!names(&saved.bytes).contains(&"preview.png".to_owned()));
    // An entry that is no picture, one that is too large for a preview, and one whose checksum fails are left out (2.8: a writer MAY leave the preview out), and the save is made.
    let not_a_picture = zipped_again(&ours, |name, item| {
        if name == "preview.png" {
            *item = Item::stored(name, b"not a picture at all");
            foreign(name, item);
        }
    });
    let too_large = zipped_again(&ours, |name, item| {
        if name == "preview.png" {
            *item = Item::stored(name, &preview_png(&vec![7u8; (4 << 20) + 1]));
            foreign(name, item);
        }
    });
    let bad_checksum = zipped_again(&ours, |name, item| {
        foreign(name, item);
        if name == "preview.png" {
            item.crc ^= 1;
        }
    });
    for (what, bytes) in [("no picture", not_a_picture), ("too large", too_large), ("a checksum that fails", bad_checksum)] {
        let (back, opened) = open_ok(&bytes);
        let saved = save_from(&back, &opened.state, &bytes, &opened.decoded);
        assert!(!names(&saved.bytes).contains(&"preview.png".to_owned()), "{what}");
        assert!(open_ok(&saved.bytes).0 == doc, "{what}: the file reads back");
        assert_ours(&saved.bytes, what);
    }
}
