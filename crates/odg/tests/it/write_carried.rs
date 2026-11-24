//! The writer's carried work: what the reviews of left for it: (a tag that holds `svg:x` and an `x` in the W3C's SVG
//! namespace moves cleanly), the words of a page change, and the memory a save counts for what a caller did to the model after the read (L2, whose
//! child-process case is in `write_memory.rs`).

use diacraft_odg::WriteError;

use crate::drawing::{Drawing, cm};
use crate::write_core::{move_by, num_of, open, open_bytes, part, real, shape_mut};

#[test]
fn a_tag_with_svg_x_and_an_x_in_the_w3c_namespace_moves_cleanly() {
    // The two are two names in two namespaces (ODF's and the W3C's), though the reader folds both into `svg:x`. The check of the bytes made
    // once called them one name written twice and refused the save of a file nothing was wrong with.
    let mut d = Drawing::a4().page(
        r#"<draw:custom-shape draw:name="s" svg:x="2cm" w:x="9cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#,
    );
    d.ns.push_str(r#" xmlns:w="http://www.w3.org/2000/svg""#);
    let mut o = open(&d);
    // The reader reads the first `x` there is: the file's `svg:x`, 2 cm.
    close_to(num_of(o.doc.pages.get(0).unwrap().shapes.get(0).unwrap(), "PinX"), cm(4.0));
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {} ({e})", e.class()));
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains(r#"svg:x="3cm""#) && after.contains(r#"w:x="9cm""#), "the first `x` is moved and the other is the file's own: {after}");
    let back = open_bytes(saved.bytes);
    close_to(num_of(back.doc.pages.get(0).unwrap().shapes.get(0).unwrap(), "PinX"), cm(5.0));
    // The other way round, the W3C's `x` first: it is the one the reader reads and the one that moves, and the save is as clean.
    let mut d = Drawing::a4().page(
        r#"<draw:custom-shape draw:name="s" w:x="2cm" svg:x="9cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#,
    );
    d.ns.push_str(r#" xmlns:w="http://www.w3.org/2000/svg""#);
    let mut o = open(&d);
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let after = part(&real(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml");
    assert!(after.contains(r#"w:x="3cm""#) && after.contains(r#"svg:x="9cm""#), "{after}");
}

#[track_caller]
fn close_to(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} is not {expected}");
}

#[test]
fn an_index_of_other_pages_than_the_files_is_refused_and_a_model_of_other_pages_than_the_files_is_saved() {
    // An index that does not hold the pages of the file is no index of it: the words say so, and name the class a harness reads. A model of more pages than the file (a page added)
    // is not an error of the index's: the save writes the pages the model added.
    let pages = |n: usize| {
        let mut d = Drawing::a4();
        for k in 0..n {
            d = d.page(&format!("<draw:rect draw:name=\"p{k}\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"1cm\" svg:height=\"1cm\"/>"));
        }
        open(&d)
    };
    let (two, three) = (pages(2), pages(3));
    match real(&two.doc, &two.bytes, &three.index) {
        Err(e @ WriteError::Mismatch(words)) => {
            assert_eq!(e.class(), "write_mismatch");
            assert!(words.contains("index") && words.contains("pages"), "{words}");
        }
        other => panic!("an index of more pages than the file: {other:?}"),
    }
    // The third page of another file holds that file's markup (the attributes of its element), which a new page of this writer cannot: refused, in words that name it.
    match real(&three.doc, &two.bytes, &two.index) {
        Err(WriteError::Unsupported(words)) => assert!(words.contains("new page") && words.contains("markup"), "{words}"),
        other => panic!("a page that holds another file's markup: {other:?}"),
    }
    // A page as the engine adds one is written.
    let mut doc = two.doc.clone();
    let mut page = diacraft_doc::Page::new(2, "Page-3");
    page.sheet = std::sync::Arc::clone(&doc.pages.get(0).unwrap().sheet);
    doc.pages.push(std::sync::Arc::new(page));
    let saved = real(&doc, &two.bytes, &two.index).unwrap_or_else(|e| panic!("a page added was refused: {e}"));
    assert!(saved.edited && saved.index.pages() == 3);
}

#[test]
fn what_a_model_has_grown_by_since_the_read_is_counted_from_the_model_itself() {
    // The model of an unedited read has grown by nothing: the gate charges nothing for growth, and the figures the walk takes of it equal the read's.
    let o = open(&Drawing::a4().page(
        r#"<draw:rect draw:name="a" svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/><draw:rect draw:name="b" svg:x="3cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/>"#,
    ));
    let mut doc = o.doc.clone();
    move_by(&mut doc, 0, 1, 1.0, 0.0);
    let mut work = diacraft_odg::WriteWork::default();
    let options = diacraft_odg::WriteOptions::default();
    let saved = diacraft_odg::write_counted(&doc, Some(&diacraft_odg::OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work);
    assert!(saved.is_ok_and(|w| w.edited));
    assert_eq!(work.grown_bytes, 0, "a move grows nothing");
    // 2 shapes, and the cells and text each holds, are walked once (a step each).
    assert!(work.model_walk_steps >= 2, "{}", work.model_walk_steps);
    // A cell given text after the read is charged, byte for byte.
    {
        let shape = shape_mut(&mut doc, 0, 2);
        let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
        sheet.cells.insert(diacraft_doc::CellName::new("Comment"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text(&"c".repeat(1_000))));
    }
    let mut work = diacraft_odg::WriteWork::default();
    let _ = diacraft_odg::write_counted(&doc, Some(&diacraft_odg::OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work);
    // One cell (4 items of 64 bytes) and its 1,000 bytes of text, and the 7 of its name and the unit nothing: at least what it holds.
    assert!(work.grown_bytes >= 1_000 + 4 * 64 && work.grown_bytes < 1_000 + 4 * 64 + 100, "{}", work.grown_bytes);
}

/// Review LOW: the index a save gives carries the cost of the read with what the model grew by since taken in as items (the bytes it grew by over
/// what the reader charges for an item), so that the next save's gate counts the model as it is. Not the growth times the charge.
#[test]
fn the_index_a_save_gives_counts_the_growth_of_the_model_in_items_at_what_the_reader_charges_for_one() {
    let mut o = open(&Drawing::a4().page(
        r#"<draw:rect draw:name="a" svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"><text:p>x</text:p></draw:rect><draw:rect draw:name="b" svg:x="3cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"><text:p>y</text:p></draw:rect>"#,
    ));
    // 100,000 characters of text in the first shape: written, and the model's growth.
    shape_mut(&mut o.doc, 0, 1).text = Some(std::sync::Arc::new(diacraft_doc::Text {
        parts: vec![diacraft_doc::TextPart::Chars(std::sync::Arc::from("c".repeat(100_000)))],
        ..Default::default()
    }));
    move_by(&mut o.doc, 0, 2, 1.0, 0.0);
    let mut work = diacraft_odg::WriteWork::default();
    let saved = diacraft_odg::write_counted(
        &o.doc,
        Some(&diacraft_odg::OriginalPackage { bytes: &o.bytes, index: &o.index }),
        &diacraft_odg::WriteOptions::default(),
        &mut work,
    )
    .unwrap();
    assert!(work.grown_bytes >= 99_000, "{}", work.grown_bytes);
    // 64 bytes an item (the reader's charge; 54 before a change).
    assert_eq!(saved.index.read_cost().items - o.index.read_cost().items, work.grown_bytes / 64);
}
