//! What a read and a write cost, counted in the units their loops walk, for the crafted inputs of the plan's step 7: a count that does not move with the load on the
//! machine, with a bound near the measured value. The `#[ignore]` tests time each beside a yardstick, a package of the same size made of plain pages, and fail when the case is
//! slower; no default-run test asserts elapsed time.
use std::sync::Arc;
use std::time::Instant;

use diacraft_doc::Page;
use diacraft_native::{Limits, NativeError, ReadNote, State, WriteOptions, read, write};
use diacraft_sheetxml::model::stamp_of_bytes;

use crate::util::*;
use crate::zipbuild::Item;

/// A package of Vector 1's kind whose document part holds `document_extra` after its page references, and whose pages are `pages` (`(name, text)`), and one blob entry.
fn package_of(pages: &[(String, Vec<u8>)], document_extra: &str, blob: Option<(&str, &[u8])>) -> Vec<u8> {
    let page_refs: String =
        pages.iter().enumerate().map(|(i, (name, _))| format!("<diacraft:page-ref id=\"{}\" part=\"{name}\"/>\n", i + 1)).collect();
    let document = part(&format!(
        "<diacraft:document xmlns:diacraft=\"{NS}\" title=\"Untitled\">\n{STYLE_BLOCK}{page_refs}{document_extra}</diacraft:document>"
    ));
    let mut rows: Vec<(String, &str, String)> = vec![("document.xml".to_owned(), "document", String::new())];
    rows.extend(pages.iter().map(|(n, _)| (n.clone(), "page", String::new())));
    if let Some((name, _)) = blob {
        rows.push((name.to_owned(), "blob", " media-type=\"image/png\"".to_owned()));
    }
    rows.push(("state.xml".to_owned(), "state", String::new()));
    let borrowed: Vec<(&str, &str, &str)> = rows.iter().map(|(a, b, c)| (a.as_str(), *b, c.as_str())).collect();
    let mut items = vec![
        Item::stored("mimetype", MEDIA_DRAWING.as_bytes()),
        Item::deflated("manifest.xml", &manifest("1.0", "drawing", &borrowed)),
        Item::deflated("document.xml", &document),
    ];
    items.extend(pages.iter().map(|(n, t)| Item::deflated(n, t)));
    if let Some((name, bytes)) = blob {
        items.push(Item::stored(name, bytes));
    }
    items.push(Item::deflated("state.xml", &state_empty()));
    package(&items)
}

fn small_pages(n: usize) -> Vec<(String, Vec<u8>)> {
    (1..=n)
        .map(|i| {
            let text = format!("<diacraft:page xmlns:diacraft=\"{NS}\" id=\"{i}\" name=\"P{i}\"><diacraft:shape id=\"1\"/></diacraft:page>");
            (format!("pages/page{i}.xml"), part(&text))
        })
        .collect()
}

#[test]
fn many_small_parts_cost_in_proportion_to_their_number() {
    let (small, big) = (open_ok(&package_of(&small_pages(500), "", None)).1.work, open_ok(&package_of(&small_pages(1_000), "", None)).1.work);
    // Rows of the directory read, entries inflated, and the events the XML reader made: linear in the number of parts, with the fixed few entries beside them.
    assert_eq!((small.rows, big.rows), (500 + 4, 1_000 + 4));
    assert!(big.xml_events <= 2 * small.xml_events + 100, "{} against {}", big.xml_events, small.xml_events);
    assert!(big.xml_events as f64 >= 1.9 * small.xml_events as f64, "and every part is read: {} against {}", big.xml_events, small.xml_events);
    assert!(big.inflated <= 2 * small.inflated + 4_000, "{} against {}", big.inflated, small.inflated);
    assert!(big.local_headers <= 2 * small.local_headers + 10, "{} against {}", big.local_headers, small.local_headers);
    assert!(small.name_comparisons < 500 * 40, "the judge's sort: {}", small.name_comparisons);
    // The pages' count is the model's own cap: 1,001 pages are refused in words.
    let error = read(&package_of(&small_pages(1_001), "", None), &Limits::default()).unwrap_err();
    assert_eq!(error.class(), "model_too_complex");
    assert!(error.to_string().contains("1000"), "{error}");
}

/// A package whose `records` blob records all name `blobs/0.png` (12 bytes) and whose page has `records` shapes, each showing one of them.
fn records_naming_one_entry(records: usize) -> Vec<u8> {
    let stamp = stamp_of_bytes(&PICTURE).text();
    let list: String = (0..records)
        .map(|i| format!("<diacraft:blob id=\"{i}\" path=\"blobs/0.png\" media-type=\"image/png\" size=\"12\" stamp=\"{stamp}\"/>\n"))
        .collect();
    let shapes: String = (0..records)
        .map(|i| format!("<diacraft:shape id=\"{i}\"><diacraft:foreign><diacraft:picture blob=\"{i}\"/></diacraft:foreign></diacraft:shape>"))
        .collect();
    let page = part(&format!("<diacraft:page xmlns:diacraft=\"{NS}\" id=\"1\" name=\"P1\">{shapes}</diacraft:page>"));
    package_of(&[("pages/page1.xml".to_owned(), page)], &list, Some(("blobs/0.png", &PICTURE)))
}

#[test]
fn fifty_thousand_blob_records_naming_one_entry_inflate_it_once_for_each_blob_named_and_no_more() {
    let crafted = records_naming_one_entry(50_000);
    let base: u64 = entries(&crafted).iter().filter(|(n, _)| n != "blobs/0.png").map(|(_, c)| c.len() as u64).sum();
    let (doc, opened) = open_ok(&crafted);
    assert_eq!(doc.pages.get(0).unwrap().shapes.len(), 50_000);
    assert!(opened.notes.is_empty(), "{:?}", opened.notes);
    // Each blob named is fetched once: the entry's 12 bytes inflated for each, and nothing more is read for the records that name the same path.
    assert!(opened.work.inflated >= base + 12 * 50_000 - 12, "{} of {}", opened.work.inflated, base + 12 * 50_000);
    assert!(opened.work.inflated <= base + 12 * 50_000, "{} of {}", opened.work.inflated, base + 12 * 50_000);
    assert_eq!(opened.decoded.blobs(), 50_000);
    // And a package whose records name an entry that is no blob row at all fetches nothing: the records are lost blobs.
    let mut items = vector_5();
    let document =
        String::from_utf8(entry(&package(&items), "document.xml")).unwrap().replace("blobs/0.png\" media-type", "pages/page1.xml\" media-type");
    replace(&mut items, "document.xml", document.into_bytes());
    let (_, lost) = open_ok(&package(&items));
    assert_eq!(
        lost.notes,
        [ReadNote::PictureLost { count: 1 }, ReadNote::UnusedBlob { count: 1 }],
        "the record names an entry that is no blob, so the blob entry is named by none"
    );
}

#[test]
fn a_blob_named_by_every_shape() {
    // 200,000 shapes on five pages show one picture of 8 MiB. Its bytes are charged to the pool of blobs once (a charge for each shape would be 1.6 TB, and the pool is
    // 384 MiB), inflated once, and held once.
    let blob = patterned(11, 8 << 20);
    let stamp = stamp_of_bytes(&blob).text();
    let list = format!("<diacraft:blob path=\"blobs/0.png\" media-type=\"image/png\" size=\"{}\" stamp=\"{stamp}\"/>\n", blob.len());
    let shapes: String = (0..40_000)
        .map(|i| format!("<diacraft:shape id=\"{i}\"><diacraft:foreign><diacraft:picture blob=\"0\"/></diacraft:foreign></diacraft:shape>"))
        .collect();
    let pages: Vec<(String, Vec<u8>)> = (1..=5)
        .map(|i| {
            (format!("pages/page{i}.xml"), part(&format!("<diacraft:page xmlns:diacraft=\"{NS}\" id=\"{i}\" name=\"P{i}\">{shapes}</diacraft:page>")))
        })
        .collect();
    let bytes = package_of(&pages, &list, Some(("blobs/0.png", &blob)));
    let (doc, opened) = open_ok(&bytes);
    let shown: Vec<Arc<[u8]>> = doc
        .pages
        .iter()
        .flat_map(|p| p.shapes.iter().filter_map(|s| s.foreign.as_ref()?.picture.as_ref().map(|m| Arc::clone(&m.bytes))).collect::<Vec<_>>())
        .collect();
    assert_eq!(shown.len(), 200_000);
    assert!(shown.iter().all(|s| Arc::ptr_eq(s, &shown[0])), "one copy of the bytes");
    assert!(opened.work.inflated < 8 * (1 << 20) + 40 * (1 << 20), "inflated once: {}", opened.work.inflated);
    assert_eq!(opened.decoded.blobs(), 1);
}

#[test]
fn read_write_read_under_one_budget() {
    // A save that passes is a package an open takes: under every limit that is tried, a model that was read is written and read again, or is refused in words, and
    // what a write made is never refused by a read of the same limits.
    let doc = drawing_of((1..=3).map(|i| page_showing(i, &[&patterned(i as u64, 20_000), &patterned(9, 20_000)])).collect());
    let bytes = write_doc(&doc);
    let mut passed = 0;
    for package_cap in [1_000u64, 10_000, 50_000, 100_000, 1 << 20, 1 << 30] {
        for memory in [0u64, 1 << 20, 64 << 20, 1 << 30] {
            let mut l = Limits::default();
            l.container.max_package_bytes = package_cap;
            l.max_memory = memory;
            let Ok((model, opened)) = read(&bytes, &l) else { continue };
            let written = write(&model, &opened.state, None, &WriteOptions { limits: l.clone(), ..WriteOptions::default() });
            match written {
                Ok(w) => {
                    assert!(read(&w.bytes, &l).is_ok(), "package {package_cap}, memory {memory}: a save that passed is a package the open takes");
                    passed += 1;
                }
                Err(e) => assert!(!e.to_string().is_empty()),
            }
        }
    }
    assert!(passed > 0);
}

#[test]
fn a_save_over_the_cap_is_refused_in_words_naming_what_is_large() {
    let doc = drawing_of(vec![page_showing(1, &[&patterned(1, 50_000)]), page_showing(2, &[&patterned(2, 50_000)])]);
    let with = |f: &dyn Fn(&mut Limits)| {
        let mut l = Limits::default();
        f(&mut l);
        write(&doc, &State::default(), None, &WriteOptions { limits: l, ..WriteOptions::default() })
    };
    // The file: a package longer than the cap of a file.
    let error = with(&|l| l.max_file_bytes = 1_000).unwrap_err();
    assert_eq!(error, NativeError::TooLarge("the package would be larger than a DiaCraft file may be"));
    // The parts: one past the cap of an XML part as written, which the codec names.
    let error = with(&|l| l.container.max_part_bytes = 400).unwrap_err();
    assert_eq!(error.class(), "model_too_complex");
    assert!(error.to_string().contains("bytes in one part as written"), "{error}");
    // The package: the parts and the blobs together, inflated, past the cap.
    let error = with(&|l| l.container.max_package_bytes = 60_000).unwrap_err();
    assert!(error.to_string().contains("bytes in the package"), "{error}");
    // The entries: more than a package may hold.
    let error = with(&|l| l.container.max_parts = 5).unwrap_err();
    assert!(error.to_string().contains("parts in the package") || error.to_string().contains("more entries"), "{error}");
    // The memory: the package as it grows is charged before it is made.
    let error = with(&|l| l.max_memory = 1_000).unwrap_err();
    assert_eq!(error.class(), "too_large");
    assert!(error.to_string().contains("memory"), "{error}");
    // Nothing a refusal made is given back: there is no half-written package, since there is no value.
    assert!(with(&|_| {}).is_ok());
    // A part of an unknown role larger than an entry may be, and one too many.
    let (mut model, opened) = open_ok(&vector_6_package());
    let part = model.parts.get(0).unwrap().clone();
    let mut l = Limits::default();
    l.container.max_part_bytes = 10;
    let error = write(&model, &opened.state, None, &WriteOptions { limits: l, ..WriteOptions::default() }).unwrap_err();
    assert_eq!(error.class(), "too_large");
    let many: Vec<_> = (0..12).map(|i| Arc::new(diacraft_doc::Part { name: Arc::from(format!("n/{i}").as_str()), ..(*part).clone() })).collect();
    model.parts = diacraft_doc::SharedVec::from_vec(many);
    let mut l = Limits::default();
    l.container.max_parts = 10;
    let error = write(&model, &opened.state, None, &WriteOptions { limits: l, ..WriteOptions::default() }).unwrap_err();
    assert_eq!(error.class(), "too_large");
    assert!(error.to_string().contains("entries"), "{error}");
}

fn vector_6_package() -> Vec<u8> {
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
    items.push(Item::stored("notes/review.txt", b"check the margins"));
    package(&items)
}

#[test]
fn a_save_of_a_drawing_of_many_pages_is_charged_for_each_part_as_it_is_made() {
    // The package grows part by part: 300 pages of a page each, written, then read, with the cost of each side counted.
    let doc = drawing_of((1..=300).map(|i| Page::new(i, "P")).collect());
    let bytes = write_doc(&doc);
    let (back, opened) = open_ok(&bytes);
    assert!(back == doc);
    assert_eq!(opened.work.rows, 300 + 4);
    assert_eq!(opened.decoded.pages(), 300);
}

// ----------------------------------------------------------------------------------------------------------------------------- the yardstick

/// The time `read` takes of `bytes`, the least of three.
fn time_of(bytes: &[u8]) -> std::time::Duration {
    (0..3)
        .map(|_| {
            let start = Instant::now();
            let _ = read(bytes, &Limits::default());
            start.elapsed()
        })
        .min()
        .unwrap_or_default()
}

/// A package of plain pages of about `size` inflated bytes: the yardstick of a crafted input of that size.
fn plain_of(size: usize) -> Vec<u8> {
    let page = |i: usize| {
        let shapes: String = (0..40)
            .map(|k| {
                format!(
                    "<diacraft:shape id=\"{k}\" name=\"S{k}\"><diacraft:sheet><diacraft:Cell N=\"PinX\" V=\"{k}\"/></diacraft:sheet></diacraft:shape>"
                )
            })
            .collect();
        part(&format!("<diacraft:page xmlns:diacraft=\"{NS}\" id=\"{i}\" name=\"P{i}\">{shapes}</diacraft:page>"))
    };
    let each = page(1).len();
    let n = (size / each).clamp(1, 1_000);
    package_of(&(1..=n).map(|i| (format!("pages/page{i}.xml"), page(i))).collect::<Vec<_>>(), "", None)
}

#[test]
#[ignore = "timed: run beside its yardstick on a quiet machine"]
fn timed_fifty_thousand_records_naming_one_entry_against_plain_pages_of_the_same_size() {
    let crafted = records_naming_one_entry(50_000);
    let inflated: usize = entries(&crafted).iter().map(|(_, c)| c.len()).sum();
    let (case, yardstick) = (time_of(&crafted), time_of(&plain_of(inflated)));
    eprintln!("crafted {case:?}, yardstick {yardstick:?}");
    assert!(case <= yardstick * 3, "the crafted input costs {case:?}, the yardstick {yardstick:?}");
}

#[test]
#[ignore = "timed: run beside its yardstick on a quiet machine"]
fn timed_many_small_parts_against_plain_pages_of_the_same_size() {
    let crafted = package_of(&small_pages(1_000), "", None);
    let inflated: usize = entries(&crafted).iter().map(|(_, c)| c.len()).sum();
    let (case, yardstick) = (time_of(&crafted), time_of(&plain_of(inflated)));
    eprintln!("crafted {case:?}, yardstick {yardstick:?}");
    assert!(case <= yardstick * 3, "the crafted input costs {case:?}, the yardstick {yardstick:?}");
}
