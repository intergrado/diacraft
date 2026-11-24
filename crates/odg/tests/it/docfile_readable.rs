//! Item 3 (the report, concern 5): **the writer of DiaCraft's document file refuses, in words, a file its own reader would refuse** for its items, its
//! counts, its depth or its bytes, and writes nothing. A model of masters that passed the writer's node check was a file the reader refused for its items, so a save could write what
//! DiaCraft could not open again.
//!
//! Each boundary is tested both ways, through the real reader: the file that the reader has just enough room for is saved and read back, and the same model one step over it is refused
//! by the save in words, and the file that the save made one step earlier is refused by the reader at the same step. The two cannot differ.

use std::sync::Arc;

use diacraft_doc::{ColorEntry, Document, MAX_COLOR_ENTRIES, MAX_MASTERS, Master};
use diacraft_odg::{Limits, OriginalPackage, WriteError, WriteOptions, WriteWork, read, write_counted};

use crate::docfile::{a_master, base};
use crate::write_core::{Open, part_bytes};

const PATH: &str = "DiaCraft/document.xml";

/// A save of `doc` over `o`'s file at `limits`: the bytes, or the refusal, and what the save counted.
fn save(doc: &Document, o: &Open, limits: Limits) -> (Result<Vec<u8>, WriteError>, WriteWork) {
    let mut work = WriteWork::default();
    let saved = write_counted(
        doc,
        Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }),
        &WriteOptions { limits, ..WriteOptions::default() },
        &mut work,
    );
    (saved.map(|w| w.bytes), work)
}

/// The refusal of a save, in words that name what was too much; a test that did not get one fails.
fn refused(saved: Result<Vec<u8>, WriteError>, words: &str) -> WriteError {
    match saved {
        Ok(_) => panic!("the save wrote a file; it should have refused one, in words that say `{words}`"),
        Err(e) => {
            assert!(e.to_string().contains(words), "the refusal does not say `{words}`: {e}");
            e
        }
    }
}

/// The document's own items, as the reader counts them: what a read of the file costs (`items`), and the part of it that DiaCraft's document file cost.
fn items_of(bytes: &[u8], limits: &Limits) -> (u64, u64) {
    let (_, report) = read(bytes, limits).unwrap_or_else(|e| panic!("the file does not read: {} ({e})", e.class()));
    (report.work.items, report.work.docfile_items)
}

/// **The items.** The reader has a budget of items for a drawing, and the document file's share of it is what is left once the rest of the file has had its own. A package that has no
/// document file gains the manifest row that lists it, which the reader charges (a few items): the save counts that row above what it is, so that it errs towards refusing. The file the
/// save makes reads at the items it costs the reader, and at one fewer the reader refuses it, and so does the save, in words.
#[test]
fn a_document_file_the_reader_has_no_items_for_is_refused_in_words_and_one_it_has_items_for_is_written() {
    let mut o = base();
    let (plain, none) = items_of(&o.bytes, &Limits::default());
    assert_eq!(none, 0, "a file with no document file of DiaCraft's costs the reader none of it");
    for id in 1..=30 {
        o.doc.masters.push(Arc::new(a_master(id)));
    }
    let (saved, _) = save(&o.doc, &o, Limits::default());
    let bytes = saved.unwrap_or_else(|e| panic!("not saved: {e}"));
    let (needs, docfile) = items_of(&bytes, &Limits::default());
    assert!(docfile > 300, "thirty masters cost the reader items: {docfile}");
    // What the rest of the file costs after the save: the markup it had, and the row of the manifest that lists the document file.
    let row = needs - plain - docfile;
    assert!(row > 0 && row <= 64, "the manifest row the document file gains costs the reader {row} items");
    let needs = usize::try_from(needs).unwrap();
    // The reader: exactly these items, and not one fewer.
    assert!(read(&bytes, &Limits { max_items: needs, ..Limits::default() }).is_ok());
    let error = read(&bytes, &Limits { max_items: needs - 1, ..Limits::default() }).unwrap_err();
    assert_eq!(error.class(), "too_many_items", "{error}");
    // The save: refused at the items that are too few, in words, and written at the items that hold the file and the row's allowance, and the file it makes reads at them.
    let (saved, _) = save(&o.doc, &o, Limits { max_items: needs - 1, ..Limits::default() });
    let error = refused(saved, "items to read DiaCraft's document file with");
    assert_eq!(error.class(), "write_limit");
    let (saved, work) = save(&o.doc, &o, Limits { max_items: needs + 64, ..Limits::default() });
    let made = saved.unwrap_or_else(|e| panic!("a file the reader has the items for was refused: {e}"));
    assert!(read(&made, &Limits { max_items: needs + 64, ..Limits::default() }).is_ok(), "and the reader reads it at those items");
    assert_eq!(made, bytes, "the budget is no part of what is written");
    assert!(work.docfile_items > 0);
}

/// The room of the document file is what the rest of the file leaves, **whatever the file it replaces held**: a file that already has a document file is saved again over it, and
/// what the old one cost the reader is no part of the rest.
#[test]
fn what_the_old_document_file_cost_is_not_counted_against_the_new_one() {
    let mut o = base();
    for id in 1..=30 {
        o.doc.masters.push(Arc::new(a_master(id)));
    }
    let first = save(&o.doc, &o, Limits::default()).0.unwrap_or_else(|e| panic!("not saved: {e}"));
    let (needs, docfile) = items_of(&first, &Limits::default());
    // The same model over the file that holds it already, with one master more.
    let again = crate::write_core::open_bytes(first.clone());
    let mut doc = again.doc.clone();
    doc.masters.push(Arc::new(a_master(99)));
    let (bigger, _) = save(&doc, &again, Limits::default());
    let bigger = bigger.unwrap_or_else(|e| panic!("not saved: {e}"));
    let (needs_bigger, docfile_bigger) = items_of(&bigger, &Limits::default());
    assert!(docfile_bigger > docfile, "{docfile_bigger} against {docfile}");
    assert_eq!(needs_bigger - docfile_bigger, needs - docfile, "the rest of the file is the same");
    // At the items that hold the bigger file the save is made, though they are fewer than the old file and the new one cost together.
    let at = usize::try_from(needs_bigger).unwrap();
    assert!(at < usize::try_from(needs + docfile_bigger).unwrap());
    let (saved, _) = save(&doc, &again, Limits { max_items: at, ..Limits::default() });
    assert!(saved.is_ok(), "{:?}", saved.err().map(|e| e.to_string()));
    let (saved, _) = save(&doc, &again, Limits { max_items: at - 1, ..Limits::default() });
    refused(saved, "items to read DiaCraft's document file with");
}

/// **The masters.** The reader reads at most `MAX_MASTERS` of them and refuses a file of more: the save refuses a model of more in words, before any tree of the file is made. A model of
/// exactly that many is saved and read back.
#[test]
fn the_most_masters_the_reader_reads_are_written_and_one_more_is_refused_before_a_tree_is_made() {
    let mut o = base();
    let bare = Arc::new(Master::default());
    for _ in 0..MAX_MASTERS {
        o.doc.masters.push(Arc::clone(&bare));
    }
    let (saved, _) = save(&o.doc, &o, Limits::default());
    let bytes = saved.unwrap_or_else(|e| panic!("{MAX_MASTERS} masters were refused: {e}"));
    let (back, _) = read(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("the file does not read: {} ({e})", e.class()));
    assert_eq!(back.masters.len(), MAX_MASTERS);
    o.doc.masters.push(bare);
    let (saved, work) = save(&o.doc, &o, Limits::default());
    refused(saved, "masters in DiaCraft's document file");
    assert_eq!(work.docfile_trees, 0, "no tree of a master was made");
}

/// **The colours.** The reader reads at most `MAX_COLOR_ENTRIES` rows of the colour table and refuses a file of more.
#[test]
fn the_most_colours_the_reader_reads_are_written_and_one_more_is_refused() {
    let colours = |n: usize| -> Arc<Vec<ColorEntry>> {
        Arc::new((0..n).map(|ix| ColorEntry { ix: u32::try_from(ix).unwrap(), rgb: Arc::from("#102030") }).collect())
    };
    let mut o = base();
    o.doc.colors = colours(MAX_COLOR_ENTRIES);
    let (saved, _) = save(&o.doc, &o, Limits::default());
    let bytes = saved.unwrap_or_else(|e| panic!("{MAX_COLOR_ENTRIES} colours were refused: {e}"));
    let (back, _) = read(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("the file does not read: {} ({e})", e.class()));
    assert_eq!(back.colors.len(), MAX_COLOR_ENTRIES);
    o.doc.colors = colours(MAX_COLOR_ENTRIES + 1);
    let (saved, _) = save(&o.doc, &o, Limits::default());
    refused(saved, "colours in DiaCraft's document file");
}

/// **The bytes.** A caller that lowers the container's cap of a part has a reader that refuses a document file of more: the save refuses it too, and one of exactly the cap is saved and
/// read back.
#[test]
fn a_document_file_of_the_most_bytes_a_part_may_be_is_written_and_one_byte_more_is_refused() {
    let mut o = base();
    for id in 1..=30 {
        o.doc.masters.push(Arc::new(a_master(id)));
    }
    let bytes = save(&o.doc, &o, Limits::default()).0.unwrap_or_else(|e| panic!("not saved: {e}"));
    let len = part_bytes(&bytes, PATH).len() as u64;
    let capped = |cap: u64| {
        let mut limits = Limits::default();
        limits.container.max_part_bytes = cap;
        limits
    };
    // The cap is the file's length: the reader takes it, and the save writes it.
    assert!(read(&bytes, &capped(len)).is_ok(), "the reader reads a part of exactly the cap");
    let (saved, _) = save(&o.doc, &o, capped(len));
    assert!(saved.is_ok(), "{:?}", saved.err().map(|e| e.to_string()));
    // One byte less: the reader refuses the file, and the save refuses to make it.
    assert!(read(&bytes, &capped(len - 1)).is_err(), "the reader refuses a part of one byte more than the cap");
    let (saved, _) = save(&o.doc, &o, capped(len - 1));
    let error = refused(saved, "bytes in DiaCraft's document file");
    assert_eq!(error.class(), "write_limit");
}
