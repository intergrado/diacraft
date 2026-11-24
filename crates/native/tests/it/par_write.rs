//! **A `.diacraft` package is the same bytes at any number of threads.** `write_threads` deflates the entries on workers while the next part is made, writes them in the
//! order they were given, and has the codec make the parts of its first pass together: the package, and the count of what was copied, are those of the single-threaded write at 1, 2, 7
//! and 31 threads, over a drawing of many pages, masters, pictures (stored and deflated), parts nothing models and a preview, with and without a previous package to copy from, and under a
//! meter that is generous and one that is not. A refusal is the single-threaded refusal, in the same words.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Format, Part, PartRole, RawAttrs, Shape, ShapeList, Value};
use diacraft_native::{Limits, Previous, State, WriteOptions, Written, write, write_threads};

use crate::util::*;

/// Bytes of a kind that deflate: four bits in each.
fn compressible(len: usize, mut seed: u64) -> Arc<[u8]> {
    let mut bytes: Vec<u8> = Vec::with_capacity(len);
    while bytes.len() < len {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        bytes.extend(seed.to_le_bytes().iter().map(|b| b & 0x0F));
    }
    bytes.truncate(len);
    Arc::from(bytes)
}

fn cell(v: &str) -> Cell {
    let mut cell = Cell::new();
    cell.value = Some(Value::from_text(v));
    cell
}

/// A shape with cells of its own.
fn plain(id: u32) -> Shape {
    let mut shape = Shape::new(id);
    shape.name = Some(Arc::from(format!("Shape {id}").as_str()));
    let mut sheet = diacraft_doc::Sheet::default();
    for (n, v) in [("PinX", format!("{id}.25")), ("PinY", (id % 97).to_string()), ("Width", "1".to_owned()), ("Height", "0.5".to_owned())] {
        sheet.cells.insert(CellName::new(n), cell(&v));
    }
    shape.sheet = Arc::new(sheet);
    shape
}

/// A drawing of `pages` pages of `per` shapes, a picture here and there (some of them shared between pages, some deflated and some stored), two masters, and parts of unknown roles.
fn drawing(pages: u32, per: u32) -> Document {
    let stored = [png(1, 3_000), png(2, 4_000)];
    let deflated = [compressible(120_000, 5), compressible(90_000, 6), compressible(150_000, 7)];
    let mut built = Vec::new();
    for p in 1..=pages {
        let mut page = diacraft_doc::Page::new(p, &format!("Page-{p}"));
        let mut shapes = Vec::new();
        for i in 1..=per {
            let id = p * 1000 + i;
            let shape = match i % 9 {
                0 => picture_shape_of(id, "media/image1.png", &stored[(p as usize + i as usize) % 2]),
                4 => picture_shape_of(id, "media/image2.bmp", &deflated[(p as usize + i as usize) % 3]),
                _ => plain(id),
            };
            shapes.push(Arc::new(shape));
        }
        page.shapes = ShapeList::from_vec(shapes);
        built.push(page);
    }
    let mut doc = drawing_of(built);
    doc.masters = diacraft_doc::SharedVec::from_vec(vec![Arc::new(master(1, "Box")), Arc::new(master(2, "Diamond"))]);
    for (k, bytes) in [compressible(200_000, 11), png(9, 5_000), compressible(40_000, 12)].into_iter().enumerate() {
        doc.parts.push(Arc::new(Part {
            format: Format::Native,
            name: Arc::from(format!("embeddings/part{k}.bin")),
            media_type: Arc::from("application/octet-stream"),
            role: PartRole::Other,
            manifest: RawAttrs::default(),
            rels: Arc::new(Vec::new()),
            bytes,
        }));
    }
    doc
}

fn options() -> WriteOptions {
    WriteOptions { written_by: "DiaCraft test".to_owned(), preview: Some(png(3, 2_000).to_vec()), ..WriteOptions::default() }
}

fn saved(doc: &Document, previous: Option<&Previous<'_>>, options: &WriteOptions, threads: usize) -> Result<Written, diacraft_native::NativeError> {
    if threads == 1 { write(doc, &State::default(), previous, options) } else { write_threads(doc, &State::default(), previous, options, threads) }
}

#[test]
fn a_package_is_the_same_bytes_at_any_number_of_threads() {
    let doc = drawing(10, 60);
    let one = saved(&doc, None, &options(), 1).unwrap();
    for threads in [2, 7, 31] {
        let many = saved(&doc, None, &options(), threads).unwrap();
        assert!(
            many.bytes == one.bytes,
            "{threads} threads: the package differs from the single-threaded one ({} and {} bytes)",
            many.bytes.len(),
            one.bytes.len()
        );
        assert_eq!(many.copied, one.copied, "{threads} threads: what was copied");
    }
    // The drawing is a package that opens, with every page and master in it.
    let (back, _) = open_ok(&one.bytes);
    assert_eq!((back.pages.len(), back.masters.len(), back.parts.len()), (10, 2, 3));
}

#[test]
fn a_write_that_copies_from_the_previous_package_is_the_same_bytes_at_any_number_of_threads() {
    let doc = drawing(8, 50);
    let first = saved(&doc, None, &options(), 1).unwrap();
    let (mut again, opened) = open_ok(&first.bytes);
    // A page changed, one picture replaced in another, and a master left as it was: the rest is copied.
    {
        let page = Arc::make_mut(again.pages.get_mut(2).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("PinX"), cell("42.75"));
    }
    let previous = Previous { bytes: &first.bytes, decoded: &opened.decoded };
    let one = saved(&again, Some(&previous), &options(), 1).unwrap();
    assert!(one.copied > 0, "the entries that did not change are copied");
    for threads in [2, 7, 31] {
        let many = saved(&again, Some(&previous), &options(), threads).unwrap();
        assert!(many.bytes == one.bytes, "{threads} threads: the package differs from the single-threaded one");
        assert_eq!(many.copied, one.copied, "{threads} threads: what was copied");
    }
    // A save that changed nothing writes the file it read, at any count.
    let (same, same_opened) = open_ok(&first.bytes);
    let previous = Previous { bytes: &first.bytes, decoded: &same_opened.decoded };
    let kept = saved(&same, Some(&previous), &options(), 1).unwrap();
    for threads in [2, 31] {
        let many = saved(&same, Some(&previous), &options(), threads).unwrap();
        assert!(many.bytes == kept.bytes && many.copied == kept.copied, "{threads} threads");
    }
}

#[test]
fn a_package_is_the_same_bytes_every_time_it_is_made_on_threads() {
    let doc = drawing(6, 40);
    let first = saved(&doc, None, &options(), 7).unwrap().bytes;
    for run in 0..6 {
        assert!(saved(&doc, None, &options(), 7).unwrap().bytes == first, "run {run}");
    }
}

#[test]
fn a_meter_that_is_not_generous_gives_the_single_threaded_package_or_the_single_threaded_refusal() {
    let doc = drawing(6, 40);
    // From a meter that holds nothing to one that holds a drawing twice over: each is either the same package or the same refusal, in the same words, at any count.
    for max_memory in [0u64, 1 << 16, 1 << 20, 3 << 20, 8 << 20, 32 << 20, 256 << 20, 3 << 30] {
        let options = WriteOptions { limits: Limits { max_memory, ..Limits::default() }, ..options() };
        let one = saved(&doc, None, &options, 1);
        for threads in [2, 7, 31] {
            let many = saved(&doc, None, &options, threads);
            match (&one, &many) {
                (Ok(a), Ok(b)) => assert!(a.bytes == b.bytes, "{threads} threads, a meter of {max_memory}: the package differs"),
                (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string(), "{threads} threads, a meter of {max_memory}"),
                (a, b) => panic!(
                    "{threads} threads, a meter of {max_memory}: {:?} and {:?}",
                    a.as_ref().map(|w| w.bytes.len()),
                    b.as_ref().map(|w| w.bytes.len())
                ),
            }
        }
    }
}

#[test]
fn a_drawing_of_no_page_or_one_is_the_same_bytes_at_any_count() {
    for doc in [drawing_of(Vec::new()), drawing(1, 20)] {
        let one = saved(&doc, None, &options(), 1).unwrap();
        for threads in [2, 31] {
            assert!(saved(&doc, None, &options(), threads).unwrap().bytes == one.bytes, "{threads} threads");
        }
    }
}
