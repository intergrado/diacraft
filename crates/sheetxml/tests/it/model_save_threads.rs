//! **A save hands over the same parts at any number of threads.** `save_parts_owned` makes the pages and masters of the first pass together, each by a thread with an encoder of
//! its own, and holds the parts that fit for the second pass, which does not make them again. What it hands over (each part's role, name and bytes, in order), what it numbers (the blobs, by
//! first use), and every refusal are those of the single-threaded save, at 2, 7 and 31 threads, over pages that use no picture, pages that use pictures (so that the numbers of the
//! blobs depend on the order of the pages), a picture that two pages share, parts that do not fit what they are expected to be, and a hold of memory too small to keep a part.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, Document, ForeignData, Master, Media, Page, Shape, ShapeList, Value};
use diacraft_sheetxml::model::{BlobId, CodecError, MemBlobs, PartRole, SaveThreads, SavedOwned, open_limits, save_parts_owned};

const MIB: u64 = 1 << 20;

type Handed = (PartRole, String, Vec<u8>);

/// What a save did: how it ended, what it handed over, and the blobs it numbered (media type and size of each, in the order of their numbers).
type Done = (Result<(), CodecError>, Vec<Handed>, Vec<(String, usize)>);

fn cell(v: &str) -> Cell {
    Cell::constant(Value::from_text(v))
}

fn picture(len: usize, seed: u8) -> Arc<Media> {
    Arc::new(Media { part: Arc::from("p"), content_type: Arc::from("image/png"), bytes: Arc::from(vec![seed; len]) })
}

/// A shape with a cell of its own, and the picture `media` when there is one.
fn shape(id: u32, media: Option<&Arc<Media>>) -> Arc<Shape> {
    let mut s = Shape::new(id);
    Arc::make_mut(&mut s.sheet).set_cell(&CellAddr::direct("PinX"), cell(&format!("{id}.5"))).unwrap();
    if let Some(m) = media {
        s.foreign = Some(Arc::new(ForeignData { picture: Some(Arc::clone(m)), ..ForeignData::default() }));
    }
    Arc::new(s)
}

fn page(id: u32, shapes: u32, pictures: &[&Arc<Media>]) -> Arc<Page> {
    let mut page = Page::new(id, &format!("page {id}"));
    page.shapes =
        ShapeList::from_vec((0..shapes).map(|i| shape(id * 1000 + i + 1, pictures.get(i as usize % (pictures.len() + 1)).copied())).collect());
    Arc::new(page)
}

fn master(id: u32, shapes: u32, pictures: &[&Arc<Media>]) -> Arc<Master> {
    let mut m = Master { id, ..Master::default() };
    m.shapes = ShapeList::from_vec((0..shapes).map(|i| shape(i + 1, pictures.get(i as usize % (pictures.len() + 1)).copied())).collect());
    Arc::new(m)
}

/// What a save hands over, how it ended, and the blobs it numbered (path and size of each, in the order of their numbers).
fn save(doc: &Document, par: SaveThreads) -> Done {
    let mut blobs = MemBlobs::new();
    let mut parts: Vec<Handed> = Vec::new();
    let out = save_parts_owned(doc, &mut blobs, &open_limits(), par, &mut |p: SavedOwned<'_>| {
        parts.push((p.role, p.name.to_owned(), p.bytes));
        Ok(())
    });
    let held: Vec<(String, usize)> =
        (0..blobs.len() as u32).filter_map(|n| blobs.entry(BlobId(n))).map(|(bytes, media)| (media.to_owned(), bytes.len())).collect();
    (out, parts, held)
}

/// The save at one thread and at each count, with `keep` bytes held: the same parts, the same blobs, the same end.
fn same_at_every_count(doc: &Document, keep: u64) {
    let (out, parts, blobs) = save(doc, SaveThreads::ONE);
    assert!(out.is_ok(), "{out:?}");
    assert!(parts.len() >= 2);
    for threads in [2, 7, 31] {
        let (got, handed, held) = save(doc, SaveThreads { threads, keep });
        assert_eq!(got, out, "{threads} threads, a hold of {keep}");
        assert!(handed == parts, "{threads} threads, a hold of {keep}: the parts differ");
        assert_eq!(held, blobs, "{threads} threads, a hold of {keep}: the blobs differ");
    }
}

#[test]
fn pages_that_use_no_picture_are_the_same_parts_at_any_count_and_any_hold() {
    let mut doc = Document::empty();
    for id in 1..=9 {
        doc.pages.push(page(id, 40 + id, &[]));
    }
    doc.masters.push(master(10, 5, &[]));
    doc.masters.push(master(11, 7, &[]));
    for keep in [0, 1, 300_000, 4 * MIB, 512 * MIB] {
        same_at_every_count(&doc, keep);
    }
}

#[test]
fn the_blobs_are_numbered_by_first_use_whatever_order_the_pages_were_made_in() {
    // Picture A is first used by page 3, picture B by page 2 and again by page 5, picture C by page 4; a master uses A and C. The numbers follow the pages in order, whichever thread was done first.
    let (a, b, c) = (picture(5_000, 1), picture(6_000, 2), picture(7_000, 3));
    let mut doc = Document::empty();
    doc.pages.push(page(1, 30, &[]));
    doc.pages.push(page(2, 30, &[&b]));
    doc.pages.push(page(3, 30, &[&a, &b]));
    doc.pages.push(page(4, 30, &[&c]));
    doc.pages.push(page(5, 30, &[&b, &a]));
    doc.pages.push(page(6, 30, &[]));
    doc.masters.push(master(10, 6, &[&a, &c]));
    for keep in [0, 64 * MIB] {
        same_at_every_count(&doc, keep);
    }
    // And the numbers are the ones the pages in order give: B before A before C.
    let (_, _, blobs) = save(&doc, SaveThreads { threads: 31, keep: 64 * MIB });
    let sizes: Vec<usize> = blobs.iter().map(|(_, n)| *n).collect();
    assert_eq!(sizes, vec![6_000, 5_000, 7_000]);
}

#[test]
fn parts_that_are_more_than_they_were_expected_to_be_are_made_as_a_single_thread_makes_them() {
    // A top-level shape is expected to be 4 KiB, and a group of many children is not: the part passes what a thread may make of it and is made again in its place.
    let mut doc = Document::empty();
    for id in 1..=4 {
        let mut p = Page::new(id, &format!("page {id}"));
        let mut group = Shape::new(1);
        let kids: Vec<Arc<Shape>> = (0..3_000).map(|i| shape(i + 2, None)).collect();
        group.children = ShapeList::from_vec(kids);
        p.shapes = ShapeList::from_vec(vec![Arc::new(group)]);
        doc.pages.push(Arc::new(p));
    }
    for keep in [2 * MIB, 64 * MIB] {
        same_at_every_count(&doc, keep);
    }
}

#[test]
fn a_part_the_first_pass_held_is_the_part_the_second_pass_would_have_made() {
    // The second pass makes the parts again when nothing was held (a hold of 0 is the single-threaded save): the two are the same bytes.
    let mut doc = Document::empty();
    for id in 1..=6 {
        doc.pages.push(page(id, 120, &[]));
    }
    let (_, rebuilt, _) = save(&doc, SaveThreads { threads: 4, keep: 0 });
    let (_, held, _) = save(&doc, SaveThreads { threads: 4, keep: 256 * MIB });
    assert!(held == rebuilt);
}

#[test]
fn a_save_that_is_refused_is_refused_in_the_same_words_at_any_count() {
    // A package past what the limits allow: refused before the first part is handed over, at any count.
    let mut doc = Document::empty();
    for id in 1..=6 {
        doc.pages.push(page(id, 200, &[]));
    }
    let mut limits = open_limits();
    limits.max_part_bytes = 20_000;
    let one = {
        let mut parts = 0usize;
        let out = save_parts_owned(&doc, &mut MemBlobs::new(), &limits, SaveThreads::ONE, &mut |_| {
            parts += 1;
            Ok(())
        });
        (out, parts)
    };
    assert!(one.0.is_err(), "the pages are past 20,000 bytes");
    for threads in [2, 7, 31] {
        let mut parts = 0usize;
        let out = save_parts_owned(&doc, &mut MemBlobs::new(), &limits, SaveThreads { threads, keep: 64 * MIB }, &mut |_| {
            parts += 1;
            Ok(())
        });
        assert_eq!((out, parts), one, "{threads} threads");
    }
}
