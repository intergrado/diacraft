//! **A save is the same bytes at any number of threads.** `write_with_threads` patches the parts of the pages and masters that an edit touched on workers, and compresses the
//! parts of the package on workers; the parts are put back in the order they have, so the package is the one a single thread makes, and so is the report of what the save did, at 1, 2, 7 and
//! 31 threads, over a drawing that holds many pages, masters, pictures and parts nothing models, and over a drawing made new.
//!
//! Every input is built in code.

use diacraft_doc::{CellAddr, CellEdit, Document, Owner, ShapeRef, SheetRef, Value, WriteOrigin};
use diacraft_testkit::package::{MasterSpec, PageSpec, Pkg, VsdxBuilder, shape, typical_shape};
use diacraft_vsdx::{OriginalBytes, WriteError, WriteOptions, Written, write_with, write_with_threads};

use crate::read_ok;

const NOW: i64 = 1_700_000_000;

fn options() -> WriteOptions {
    WriteOptions { now: Some(NOW), ..WriteOptions::default() }
}

/// Bytes that deflate to a part of their size: four bits in a byte.
fn picture(len: usize, mut seed: u64) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::with_capacity(len);
    while bytes.len() < len {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        bytes.extend(seed.to_le_bytes().iter().map(|b| b & 0x0F));
    }
    bytes.truncate(len);
    bytes
}

/// A drawing of `pages` pages of `per` shapes (each of its own), three masters, two pictures (one stored, one deflated) and a part of unknown kind.
fn drawing(pages: u32, per: u32) -> Vec<u8> {
    let mut builder = VsdxBuilder::new();
    for m in 1..=3u32 {
        builder = builder.master(MasterSpec::new(m, &format!("Master {m}")).shapes(&shape(4, &format!(r#"<Cell N="Width" V="{m}"/>"#))));
    }
    for page in 1..=pages {
        let body: String = (0..per).map(|i| typical_shape((page - 1) * per + i + 1)).collect();
        builder = builder.page(PageSpec::new(page, &format!("Page {page}")).shapes(&body));
    }
    let extras = Pkg::new()
        .default_type("bmp", "image/bmp")
        .default_type("png", "image/png")
        .default_type("dat", "application/x-foo")
        .part_without_override("/visio/media/image1.bmp", picture(600_000, 3))
        .part_without_override("/visio/media/image2.png", picture(300_000, 4))
        .part_without_override("/foo/bar.dat", picture(70_000, 5));
    builder.with(extras).build()
}

/// The first shape of every page and master moved: every part with shapes is rewritten.
fn move_everywhere(doc: &mut Document) {
    let owners: Vec<Owner> = doc.pages.iter().map(|p| Owner::Page(p.id)).chain(doc.masters.iter().map(|m| Owner::Master(m.id))).collect();
    for owner in owners {
        let sheet = SheetRef::Shape(ShapeRef::top(owner, 0));
        doc.write_cell(&sheet, &CellAddr::direct("PinX"), CellEdit::Value(Value::from_text("7.25")), WriteOrigin::Drawing).unwrap();
    }
}

fn save(doc: &Document, original: &[u8], baseline: &Document, threads: usize) -> Result<Written, WriteError> {
    write_with_threads(doc, Some(&OriginalBytes { bytes: original, baseline: Some(baseline) }), &options(), threads)
}

/// Everything a save says of itself but the document it read back.
fn report(w: &Written) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>, usize) {
    (w.warnings.clone(), w.rewritten.clone(), w.added.clone(), w.removed.clone(), w.kept)
}

#[test]
fn a_save_into_a_file_is_the_same_bytes_and_the_same_report_at_any_number_of_threads() {
    let original = drawing(9, 60);
    let (base, _) = read_ok(&original);
    let mut doc = base.clone();
    move_everywhere(&mut doc);
    let one = save(&doc, &original, &base, 1).unwrap();
    assert!(one.rewritten.len() >= 9, "every page part was patched: {:?}", one.rewritten);
    assert_eq!(
        write_with(&doc, Some(&OriginalBytes { bytes: &original, baseline: Some(&base) }), &options()).unwrap().bytes,
        one.bytes,
        "write_with is one thread"
    );
    for threads in [2, 7, 31] {
        let many = save(&doc, &original, &base, threads).unwrap();
        assert!(
            many.bytes == one.bytes,
            "{threads} threads: the package differs from the single-threaded one ({} and {} bytes)",
            many.bytes.len(),
            one.bytes.len()
        );
        assert_eq!(report(&many), report(&one), "{threads} threads: the report");
    }
}

#[test]
fn a_save_of_a_drawing_made_new_is_the_same_bytes_at_any_number_of_threads() {
    use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
    let doc = DocBuilder::new()
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(1).cell("PinX", "1")))
        .page(2, PageBuilder::new().shape(ShapeBuilder::new(1).cell("PinX", "2")))
        .page(3, PageBuilder::new().shape(ShapeBuilder::new(1).cell("PinX", "3")))
        .build();
    let one = write_with(&doc, None, &options()).unwrap();
    for threads in [2, 7, 31] {
        let many = write_with_threads(&doc, None, &options(), threads).unwrap();
        assert!(many.bytes == one.bytes, "{threads} threads");
        assert_eq!(report(&many), report(&one), "{threads} threads: the report");
    }
}

#[test]
fn a_save_is_the_same_bytes_every_time_it_is_made_on_threads() {
    let original = drawing(6, 40);
    let (base, _) = read_ok(&original);
    let mut doc = base.clone();
    move_everywhere(&mut doc);
    let first = save(&doc, &original, &base, 7).unwrap().bytes;
    for run in 0..6 {
        assert!(save(&doc, &original, &base, 7).unwrap().bytes == first, "run {run}");
    }
}
