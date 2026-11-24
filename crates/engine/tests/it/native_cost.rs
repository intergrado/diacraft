//! What `.diacraft` costs the engine, counted and not timed: the passes that fingerprint the model, what a Save As does to what the session holds, the entries a save
//! copies, and a ledger that a file makes hostile. A count does not move with the load on the machine.

use diacraft_container::zip::{Archive, ArchiveWriter, Method};
use serde_json::json;

use crate::edit::drop_shape;
use crate::native_common::{Dir, made, opened, vsdx_drawing};
use crate::{blank_session, run};

#[test]
fn the_stamp_costs_one_encoding_per_foreign_open_and_is_made_when_it_is_needed() {
    let dir = Dir::new();
    let vsdx = dir.write("a.vsdx", &vsdx_drawing());
    let mut s = diacraft_engine::Session::new();
    assert_eq!(s.stamp_work().passes, 0);
    // A foreign open makes no pass (it was a third of the open): the stamp is pending, and while the drawing is the very document the baseline holds, it is unchanged
    // without a pass, an edit and its undo included (the undo puts the same document back).
    run(&mut s, "file.open", json!({ "path": vsdx }));
    assert!(s.content_stamp_pending());
    assert_eq!(s.stamp_work().passes, 0, "an open costs no pass");
    assert_eq!(s.content_unchanged(), Some(true));
    drop_shape(&mut s, "rectangle", 4.0, 4.0);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.content_unchanged(), Some(true));
    assert!(s.content_stamp_pending(), "asking whether it is unchanged did not make it");
    assert_eq!(s.stamp_work().passes, 0);
    let doc = s.document().unwrap();
    let parts = 1 + doc.pages.len() as u64 + doc.masters.len() as u64;
    // A Save As to `.diacraft` needs the stamp: one pass, over the document part, each page and each master, one at a time, of the model as it was read.
    run(&mut s, "file.save_as", json!({ "path": dir.path("x.diacraft") }));
    let work = s.stamp_work();
    assert_eq!((work.passes, work.parts), (1, parts), "one encoding of the model, a part at a time: {work:?}");
    assert!(work.items > 0 && !s.content_stamp_pending());
    // Nothing after it fingerprints again: a save moves no stamp, and an edit and its save cost none.
    run(&mut s, "file.save", json!({}));
    drop_shape(&mut s, "rectangle", 4.0, 4.0);
    run(&mut s, "file.save", json!({}));
    assert_eq!(s.stamp_work(), work, "a save moves no stamp and costs no pass");
    // The decision a model writer takes is one pass each time it is asked once the drawing is not the document it was read as: the content now, against the stamp.
    assert_eq!(s.content_unchanged(), Some(false));
    let after = s.stamp_work();
    assert_eq!(after.passes, 2, "one more pass, and only one: {after:?}");
    assert_eq!(after.parts, parts + 1 + s.document().unwrap().pages.len() as u64 + s.document().unwrap().masters.len() as u64);
    // A `.diacraft` open reads its stamp from the file: no pass.
    let mut t = diacraft_engine::Session::new();
    run(&mut t, "file.open", json!({ "path": dir.path("x.diacraft") }));
    assert_eq!(t.stamp_work().passes, 0);
    assert!(t.content_stamp().is_some() && !t.content_stamp_pending());
    // A new drawing and its save have none.
    let mut n = blank_session();
    made(&mut n);
    run(&mut n, "file.save_as", json!({ "path": dir.path("n.diacraft") }));
    assert_eq!(n.stamp_work().passes, 0);
    // A template, opened as a new drawing, is a new drawing: no stamp.
    run(&mut n, "file.save_as", json!({ "path": dir.path("t.diacraft-template") }));
    let mut tt = diacraft_engine::Session::new();
    run(&mut tt, "file.open", json!({ "path": dir.path("t.diacraft-template") }));
    assert_eq!((tt.stamp_work().passes, tt.content_stamp()), (0, None));
}

#[test]
fn the_stamp_is_armed_for_a_named_vsdx_file_or_odg_and_for_nothing_else() {
    // (a) A named VSDX file, an `.odg` package or a flat `.fodg` read by `file.open` arms it. A template file opened as a new drawing, a new drawing and its save as
    // VSDX, a built-in template and a `.diacraft` file do not.
    let dir = Dir::new();
    let mut named = diacraft_engine::Session::new();
    run(&mut named, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_drawing()) }));
    assert!(named.content_stamp_pending());
    let mut new = blank_session();
    made(&mut new);
    assert!(!new.content_stamp_pending());
    run(&mut new, "file.save_as", json!({ "path": dir.path("n.vsdx") }));
    run(&mut new, "file.save_as", json!({ "path": dir.path("t.vstx") }));
    assert!(!new.content_stamp_pending(), "a new drawing saved as VSDX is not read from a foreign file");
    assert_eq!((new.content_stamp(), new.content_unchanged()), (None, None));
    let mut from_template = diacraft_engine::Session::new();
    let opened = run(&mut from_template, "file.open", json!({ "path": dir.path("t.vstx") }));
    assert_eq!(opened["file"]["new_from_template"], true);
    assert!(!from_template.content_stamp_pending(), "a template opened as a new drawing is a new drawing");
    assert_eq!((from_template.content_stamp(), from_template.stamp_work().passes), (None, 0));
    let mut edit_template = diacraft_engine::Session::new();
    run(&mut edit_template, "file.open", json!({ "path": dir.path("t.vstx"), "edit_template": true }));
    assert!(edit_template.content_stamp_pending(), "the template itself is a named VSDX file");
    let mut builtin = diacraft_engine::Session::new();
    run(&mut builtin, "file.new", json!({ "template": "basic_diagram" }));
    assert!(!builtin.content_stamp_pending());
    // An `.odg` package is a foreign file, and its stamp is armed; the template that is opened as a new drawing is not.
    let odg = crate::odg_build::Drawing::new().page("");
    let mut package = diacraft_engine::Session::new();
    run(&mut package, "file.open", json!({ "path": dir.write("a.odg", &odg.bytes()) }));
    assert!(package.content_stamp_pending(), "an `.odg` read from a file arms the stamp");
    assert_eq!(package.stamp_work().passes, 0, "and makes no pass at the open");
    let mut otg = diacraft_engine::Session::new();
    run(&mut otg, "file.open", json!({ "path": dir.write("t.otg", &odg.bytes_of_type(crate::odg_build::TEMPLATE)) }));
    assert!(!otg.content_stamp_pending(), "a template opened as a new drawing is a new drawing");
    let mut otg_itself = diacraft_engine::Session::new();
    run(&mut otg_itself, "file.open", json!({ "path": dir.path("t.otg"), "edit_template": true }));
    assert!(otg_itself.content_stamp_pending(), "the template file itself is a foreign file");
    let mut flat = diacraft_engine::Session::new();
    run(&mut flat, "file.open", json!({ "path": dir.write("f.fodg", &odg.flat_bytes()) }));
    assert!(
        flat.content_stamp_pending(),
        "a flat drawing is a foreign file as an `.odg` is: Save As takes it to `.diacraft` and to the model writers"
    );
    assert_eq!(flat.stamp_work().passes, 0, "and makes no pass at the open");
    run(&mut new, "file.save_as", json!({ "path": dir.path("x.diacraft") }));
    let mut native = diacraft_engine::Session::new();
    run(&mut native, "file.open", json!({ "path": dir.path("x.diacraft") }));
    assert!(!native.content_stamp_pending());
}

#[test]
fn a_vsdx_save_makes_no_pass_and_a_drawing_edited_before_it_still_reads_as_edited_when_it_is_asked_later() {
    // (b), as the blocker of the re-review changed it: the stamp is made from the document the drawing was read as, which the pending stamp keeps; **a save of a VSDX
    // file makes no pass** (on a VSDX-only path nothing consumes the stamp: the first save was 30-50% slower than before it). The baseline the save replaces is no longer the thing
    // the stamp depends on, so a drawing that was edited and saved reads as edited all the same.
    let dir = Dir::new();
    let mut s = diacraft_engine::Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_drawing()) }));
    assert_eq!(s.stamp_work().passes, 0);
    drop_shape(&mut s, "rectangle", 4.0, 4.0);
    run(&mut s, "file.save", json!({}));
    assert_eq!(s.stamp_work().passes, 0, "a save of a VSDX file makes no pass");
    assert!(s.content_stamp_pending(), "and does not make the stamp");
    run(&mut s, "file.save", json!({}));
    drop_shape(&mut s, "rectangle", 5.0, 5.0);
    run(&mut s, "file.save", json!({}));
    assert_eq!((s.stamp_work().passes, s.content_stamp_pending()), (0, true), "three saves, no pass");
    // The question is answered from the document that was read, kept by the pending stamp: one pass for the stamp and one for the content now.
    assert_eq!(s.content_unchanged(), Some(false), "edited after it was read, and saved: still edited, as the file it was read from held it");
    assert_eq!((s.stamp_work().passes, s.content_stamp_pending()), (2, false), "the stamp, from the document it was read as, and the content now");
    // The content as edited is what the file holds now: a fresh session of the saved file is unedited against itself, and its stamp is not the first session's.
    let mut again = diacraft_engine::Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.vsdx") }));
    assert_eq!(again.content_unchanged(), Some(true));
    assert_ne!(again.content_stamp(), s.content_stamp(), "the edit is in the saved file, not in the stamp the first session carries");
    // A Save As to `.diacraft` after the splice saves carries the stamp of the original content, made then (one pass), and the file says edited.
    let mut e = diacraft_engine::Session::new();
    run(&mut e, "file.open", json!({ "path": dir.path("a.vsdx") }));
    drop_shape(&mut e, "rectangle", 4.0, 4.0);
    run(&mut e, "file.save", json!({}));
    assert_eq!(e.stamp_work().passes, 0);
    run(&mut e, "file.save_as", json!({ "path": dir.path("x.diacraft") }));
    assert_eq!(e.stamp_work().passes, 1, "the Save As made the stamp, once, from the document the drawing was read as");
    let reopened = opened(&dir.path("x.diacraft"));
    assert_eq!(reopened.content_unchanged(), Some(false));
    assert_eq!(reopened.content_stamp(), e.content_stamp());
    // An unedited drawing saved by the splice writer keeps its stamp as it was read, and the save costs none (the stamp is made when it is asked for).
    let mut u = diacraft_engine::Session::new();
    run(&mut u, "file.open", json!({ "path": dir.path("a.vsdx") }));
    run(&mut u, "file.save", json!({}));
    assert_eq!(u.stamp_work().passes, 0, "the save of an unedited drawing makes no pass either");
    let first = u.content_stamp();
    assert!(first.is_some());
    assert_eq!(u.content_stamp(), first);
    assert_eq!(u.stamp_work().passes, 1, "one pass for the stamp, and none for the question");
}

#[test]
fn open_save_as_diacraft_and_save_again_hold_the_model_once() {
    // The chain: a VSDX drawing opened, saved as `.diacraft`, edited, saved, opened again in a session of its own. **The save keeps no second model**: what the session holds after a
    // save is what it held before it (the baseline shares what the drawing holds), and the file's bytes and the model it was read as are what the process counts for it.
    let dir = Dir::new();
    let mut s = diacraft_engine::Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_drawing()) }));
    for k in 0..300 {
        drop_shape(&mut s, "rectangle", 1.0 + f64::from(k % 20) * 0.4, 1.0 + f64::from(k / 20) * 0.4);
    }
    let before = s.model_held().unwrap();
    run(&mut s, "file.save_as", json!({ "path": dir.path("x.diacraft") }));
    let after = s.model_held().unwrap();
    assert!(after <= before + before / 100 + 4_096, "a Save As to `.diacraft` doubled what the session holds: {before} then {after}");
    assert_eq!(s.model_held_exact(), s.model_held(), "the count is the walk of everything held");
    assert_eq!(s.published_held(), s.model_held().unwrap(), "and the process is told");
    drop_shape(&mut s, "rectangle", 9.0, 9.0);
    run(&mut s, "file.save", json!({}));
    assert!(s.model_held().unwrap() <= after + after / 20 + 8_192, "a second save adds no model: {after} then {}", s.model_held().unwrap());
    let t = opened(&dir.path("x.diacraft"));
    assert!(t.document() == s.document());
}

#[test]
fn the_memory_a_session_holds_is_the_files_bytes_and_the_model_it_read_as_and_stays_with_the_drawing_when_it_is_saved() {
    // As the VSDX file's (`vsdx_memory.rs`): the open file holds the package's bytes and the model it was read as, by the count a session keeps, and the process is told. (The process's figure
    // is shared with the tests that run in one process: the session's own figures are what is exact.)
    let dir = Dir::new();
    let mut maker = blank_session();
    made(&mut maker);
    run(&mut maker, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let bytes = dir.read("a.diacraft");
    let (doc, _) = diacraft_native::read(&bytes, &diacraft_native::Limits::default()).unwrap();
    let model = diacraft_sheetxml::model::held_bytes_of([&doc], u64::MAX);
    assert!(model > 10_000, "the model of a drawing is counted: {model}");
    let mut s = diacraft_engine::Session::new();
    assert_eq!((s.held_memory().drawing, s.held_memory().stencils), (0, 0), "a session with no drawing holds nothing");
    run(&mut s, "file.open", json!({ "path": dir.path("a.diacraft") }));
    assert_eq!((s.held_memory().drawing, s.held_memory().stencils), (bytes.len() as u64 + model, 0), "the bytes and the model");
    assert!(s.held_memory().process >= s.held_memory().drawing);
    assert_eq!(s.source().unwrap().model_bytes(), model);
    // An edit and a save: the file written is a different file, and the model is the one that was read (what it has grown by is not counted in the file's hold).
    drop_shape(&mut s, "rectangle", 5.0, 5.0);
    run(&mut s, "file.save", json!({}));
    let written = s.source().unwrap().bytes().len() as u64;
    assert_eq!(s.held_memory().drawing, written + model);
    // A template opened as a new drawing keeps no file: it holds no bytes and no file's model.
    run(&mut maker, "file.save_as", json!({ "path": dir.path("t.diacraft-template") }));
    let mut t = diacraft_engine::Session::new();
    run(&mut t, "file.open", json!({ "path": dir.path("t.diacraft-template") }));
    assert_eq!(t.held_memory().drawing, 0);
    // A new drawing holds no file and no model.
    run(&mut s, "file.new", json!({}));
    assert_eq!(s.held_memory().drawing, 0);
}

#[test]
fn a_page_of_more_than_five_thousand_shapes_is_saved_with_no_preview_and_one_of_five_thousand_with_one() {
    // The picture of the first page is for a file manager, 256 pixels at the most: a page of thousands of shapes is a blur, and the render is a pass over every shape. The most is 5,000.
    let has_preview = |bytes: &[u8]| Archive::read(bytes, &diacraft_container::Limits::default()).unwrap().position("preview.png").is_some();
    let dir = Dir::new();
    for (count, wanted) in [(5_000usize, true), (5_001, false)] {
        let mut s = blank_session();
        let id = drop_shape(&mut s, "rectangle", 2.0, 2.0);
        s.edit("Test edit", |doc| {
            let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let template = page.shapes.iter().find(|x| x.id == id).unwrap().clone();
            let first = page.shapes.len();
            for k in first..count {
                page.shapes.push(std::sync::Arc::new(diacraft_doc::Shape { id: id + u32::try_from(k).unwrap(), ..(*template).clone() }));
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(s.document().unwrap().pages.iter().next().unwrap().shapes.len(), count);
        let saved = run(&mut s, "file.save_as", json!({ "path": dir.path(&format!("p{count}.diacraft")) }));
        assert_eq!(saved["warning_count"], 0, "no word for a picture that is not made: {saved}");
        assert_eq!(has_preview(&dir.read(&format!("p{count}.diacraft"))), wanted, "{count} shapes");
        let again = opened(&dir.path(&format!("p{count}.diacraft")));
        assert_eq!(again.document().unwrap().pages.iter().next().unwrap().shapes.len(), count);
    }
}

#[test]
fn a_save_that_changed_a_page_copies_every_other_entry_and_one_that_changed_nothing_copies_them_all() {
    let dir = Dir::new();
    let mut s = blank_session();
    made(&mut s);
    run(&mut s, "page.add", json!({ "name": "Third" }));
    let first = run(&mut s, "file.save_as", json!({ "path": dir.path("x.diacraft") }));
    assert_eq!(first["copied"], 0, "nothing to copy from: {first}");
    let again = run(&mut s, "file.save", json!({}));
    let entries = {
        let bytes = dir.read("x.diacraft");
        let archive = Archive::read(&bytes, &diacraft_container::Limits::default()).unwrap();
        archive.entries().len()
    };
    // `mimetype`, the manifest, the document part and `state.xml` are written, and every page, master, blob and the preview are copied.
    assert_eq!(again["copied"].as_u64().unwrap() as usize, entries - 4, "{again}");
    // An edit of one page: that page's part is written, the rest copied.
    let page = s.document().unwrap().pages.iter().next().unwrap().id;
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": page, "x": 3.0, "y": 3.0}));
    let edited = run(&mut s, "file.save", json!({}));
    // The changed page and the picture of the first page, which shows it, are written (and `mimetype`, the manifest, the document part and `state.xml` as always); the other two pages, the
    // master and the theme are copied as they were.
    assert_eq!(edited["copied"].as_u64().unwrap() as usize, entries - 4 - 2, "{edited}");
    // A reopen and a save of what it read: every entry but the four is copied, the theme included (its bytes are the allocation the model read them as).
    let mut t = opened(&dir.path("x.diacraft"));
    let saved = run(&mut t, "file.save", json!({}));
    assert_eq!(saved["copied"].as_u64().unwrap() as usize, entries - 4, "{saved}");
}

#[test]
fn a_ledger_of_a_hundred_thousand_rows_seeds_only_the_pages_the_drawing_has() {
    let dir = Dir::new();
    let mut s = blank_session();
    drop_shape(&mut s, "rectangle", 1.0, 1.0);
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let rows: String = (2..100_002u32).map(|p| format!("<diacraft:ledger page=\"{p}\" mark=\"{p}\"/>")).collect();
    let state = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<diacraft:state xmlns:diacraft=\"urn:app.diacraft:extension\"><diacraft:ledger page=\"1\" mark=\"3\"/>{rows}</diacraft:state>"
    );
    let bytes = dir.read("a.diacraft");
    let mut archive = Archive::read(&bytes, &diacraft_container::Limits::default()).unwrap();
    let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
    let mut w = ArchiveWriter::new();
    for (k, name) in names.iter().enumerate() {
        let data = archive.inflate(k, &mut diacraft_container::Work::default()).unwrap();
        let data = if name == "state.xml" { state.clone().into_bytes() } else { data };
        w.add(name, &data, if name == "mimetype" { Method::Stored } else { Method::Deflated }).unwrap();
    }
    let path = dir.write("hostile.diacraft", &w.finish().unwrap());
    let t = opened(&path);
    for page in [2u32, 3, 77, 100_001] {
        assert_eq!(t.id_floor(page), 0, "a row for page {page}, which the drawing does not have, seeds nothing");
    }
    assert_eq!(t.id_floor(1), 3);
}

/// **The yardstick** (the plan's D1): opening and saving a `.vsdx` of the same size. A `.diacraft` drawing of 20,000 shapes on a page of a size is opened, one shape moved and saved, beside the
/// VSDX file it came from, each in a session of its own, one after the other seven times and each the best; the figures are printed, and the `.diacraft` pair must not be more than **two and a
/// half times** the VSDX pair. **Measured on beast (2026-11-21, load 16 to 20): 1.8 to 2.1 times** (open 153 to 201 ms against 69 to 95, save 250 to 368 ms against 147 to 178), **and it was 1.4 to 1.6
/// against a VSDX pair that paid the stamp's pass in its save** (the first save was 30 to 50% slower than before the stamp; the pass is 46 ms of the 20,000 shapes, and the save no longer pays it: the
/// `.diacraft` times did not move, and the yardstick got shorter). Where the time goes is the codec's and not the
/// engine's: the open is `diacraft_native::read` (147 ms for the part, against 91 ms for the VSDX reader: the decode charges every element to the budget and shares what is equal), and the
/// save is `save_parts`, which makes every part twice (to measure it, then to hand it over) and decodes it once to prove an open would take it, about 220 ms of the 20,000 shapes whether
/// the entry is then copied or not (a save of a drawing that changed nothing costs about as much as a fresh one: 222 ms against 263). **The picture of the first page cost 83 ms of it**
/// (the review's suspicion, measured): a page of more than 5,000 shapes has none now (`nativefile::save::PREVIEW_MOST_SHAPES`). The VSDX open is at its old time: the stamp pass is not made at
/// the open. Run by hand, alone: `cargo nextest run -p diacraft-engine --test it --run-ignored only a_diacraft_open_and_save --no-capture`.
#[test]
#[ignore = "timed: run by hand, alone"]
fn a_diacraft_open_and_save_cost_no_more_than_two_and_a_half_times_a_vsdx_open_and_save() {
    use std::time::Instant;
    let shapes: String = (1..=20_000u32)
        .map(|k| {
            diacraft_testkit::package::shape(
                k,
                &format!(
                    r##"<Cell N="PinX" V="{}"/><Cell N="PinY" V="{}"/><Cell N="Width" V="0.5"/><Cell N="Height" V="0.25"/><Cell N="FillForegnd" V="#aabbcc"/>"##,
                    1 + k % 200,
                    1 + k / 200
                ),
            )
        })
        .collect();
    let vsdx = diacraft_testkit::package::VsdxBuilder::new()
        .page(diacraft_testkit::package::PageSpec::new(1, "P").sheet(r#"<Cell N="PageWidth" V="11"/><Cell N="PageHeight" V="8.5"/>"#).shapes(&shapes))
        .build();
    let dir = Dir::new();
    let (v, d) = (dir.write("a.vsdx", &vsdx), dir.path("a.diacraft"));
    let mut first = diacraft_engine::Session::new();
    run(&mut first, "file.open", json!({ "path": v }));
    run(&mut first, "file.save_as", json!({ "path": d }));
    drop(first);
    // **The VSDX open is at the reader's time** (the stamp pass is not made at the open): `file.open` against `diacraft_vsdx::read` of the same bytes, one after the other. It was
    // 1.37 times the reader's with the pass in it (287 ms against 209, the review's measure), and the command's own work (the file read, the model's count, the holds) is the rest.
    let reader_alone = (0..7)
        .map(|_| {
            let started = Instant::now();
            let read = diacraft_vsdx::read(&vsdx, &diacraft_vsdx::Limits::default()).unwrap();
            let took = started.elapsed();
            drop(read);
            took
        })
        .min()
        .unwrap();
    let open_alone = (0..7)
        .map(|_| {
            let mut s = diacraft_engine::Session::new();
            let started = Instant::now();
            run(&mut s, "file.open", json!({ "path": v }));
            assert_eq!(s.stamp_work().passes, 0, "an open makes no pass");
            started.elapsed()
        })
        .min()
        .unwrap();
    println!(
        "TIMED vsdx reader alone {reader_alone:?}, file.open {open_alone:?}, ratio {:.2}",
        open_alone.as_secs_f64() / reader_alone.as_secs_f64()
    );
    assert!(open_alone <= reader_alone * 5 / 4, "the open is the reader's time and the command's own work: {open_alone:?} against {reader_alone:?}");
    // **The first save of a VSDX file pays for no stamp** (the blocker of the re-review: with the pass moved to the save it was 30 to 50% slower than before the stamp, 488 to
    // 528 ms against 375 to 421 at 20,000 shapes): a session opens the file, moves one shape and saves, the first save timed, seven times and the best; and **what one pass of the stamp costs**
    // on the same model, which is what the save would have paid. The pass is a part of the cost, not of the save: the figures are printed, and the count asserts that no pass was made.
    let first_save = (0..7)
        .map(|_| {
            let mut s = diacraft_engine::Session::new();
            run(&mut s, "file.open", json!({ "path": v }));
            let id = s.document().unwrap().pages.iter().next().unwrap().shapes.get(0).unwrap().id;
            run(&mut s, "shape.move", json!({"ids": [id], "dx": 0.5, "dy": 0.0}));
            let started = Instant::now();
            run(&mut s, "file.save", json!({}));
            let took = started.elapsed();
            (took, s.stamp_work().passes)
        })
        .min_by_key(|(took, _)| *took)
        .unwrap();
    let one_pass = (0..7)
        .map(|_| {
            let mut s = diacraft_engine::Session::new();
            run(&mut s, "file.open", json!({ "path": v }));
            let started = Instant::now();
            assert!(s.content_stamp().is_some());
            started.elapsed()
        })
        .min()
        .unwrap();
    println!(
        "TIMED first VSDX save {:?} (passes {}), one pass of the stamp {one_pass:?}: the save would have been {:?}",
        first_save.0,
        first_save.1,
        first_save.0 + one_pass
    );
    assert_eq!(first_save.1, 0, "the first save of a VSDX file makes no pass");
    // One round of a session: the open, the move of the first shape and the save, each timed (and a second save, in which nothing changed).
    let round = |path: &str| {
        let mut s = diacraft_engine::Session::new();
        let started = Instant::now();
        run(&mut s, "file.open", json!({ "path": path }));
        let opened = started.elapsed();
        let id = s.document().unwrap().pages.iter().next().unwrap().shapes.get(0).unwrap().id;
        let started = Instant::now();
        run(&mut s, "shape.move", json!({"ids": [id], "dx": 0.5, "dy": 0.0}));
        let moved = started.elapsed();
        let started = Instant::now();
        run(&mut s, "file.save", json!({}));
        let saved = started.elapsed();
        let started = Instant::now();
        run(&mut s, "file.save", json!({}));
        (opened, moved, saved, started.elapsed())
    };
    // The two are run **one after the other, seven times**, and each keeps its best by the sum of the open, the move and the save: a load on the machine that comes and goes falls on both.
    let (mut vsdx_best, mut native_best) = (None, None);
    for _ in 0..7 {
        let (a, b) = (round(&v), round(&d));
        if vsdx_best.is_none_or(|(o, m, s, _): (std::time::Duration, std::time::Duration, std::time::Duration, std::time::Duration)| {
            a.0 + a.1 + a.2 < o + m + s
        }) {
            vsdx_best = Some(a);
        }
        if native_best.is_none_or(|(o, m, s, _): (std::time::Duration, std::time::Duration, std::time::Duration, std::time::Duration)| {
            b.0 + b.1 + b.2 < o + m + s
        }) {
            native_best = Some(b);
        }
    }
    let ((vo, vm, vs, vc), (no, nm, ns, nc)) = (vsdx_best.unwrap(), native_best.unwrap());
    let (yardstick, case) = (vo + vm + vs, no + nm + ns);
    println!("TIMED vsdx    open {vo:?} move {vm:?} save {vs:?} (again {vc:?}) = {yardstick:?}");
    println!("TIMED diacraft open {no:?} move {nm:?} save {ns:?} (again {nc:?}) = {case:?}");
    println!("TIMED ratio {:.2}", case.as_secs_f64() / yardstick.as_secs_f64());
    assert!(case <= yardstick * 5 / 2, "a .diacraft pair costs {case:?} against {yardstick:?} for a VSDX pair");
}

/// The gap GATE.e142.F1 found and did not test: what a `.diacraft` read may hold, counted, against the memory it is given. The read's pools are shared out of the memory
/// the process leaves it (`READ_BUDGET` at the most, the smaller of the VSDX reader's figure and the package's own), and the document part's tree is held beside them: the budget was
/// shared out whole, so that a read short of memory held 41 MB more than it was given. It is counted from the pools (`Limits::read_peak`), at the budget the engine gives a read and at
/// what is left of it beside a drawing that is open.
#[test]
fn a_diacraft_read_holds_no_more_than_the_budget_it_was_given_at_the_engines_budget_and_below_it() {
    let dir = Dir::new();
    let mut s = diacraft_engine::Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_drawing()) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let bytes = dir.read("a.diacraft");
    // `nativefile::open::READ_BUDGET`: the process's budget (the VSDX reader's) or the package's own, whichever is the smaller.
    let budget = diacraft_native::MAX_MEMORY.min(diacraft_vsdx::MAX_MEMORY);
    for held_elsewhere in [0u64, 100 << 20, 400 << 20, 1 << 30, 3 << 29, 2 << 30] {
        let reserved = budget.saturating_sub(held_elsewhere);
        let limits = diacraft_native::Limits { max_memory: reserved, ..diacraft_native::Limits::default() };
        let peak = limits.read_peak(&bytes).unwrap_or_else(|e| panic!("{held_elsewhere} held elsewhere: {e}"));
        assert!(peak <= reserved, "a read given {reserved} bytes ({held_elsewhere} are held elsewhere) may hold {peak}, {} more", peak - reserved);
    }
    // The process as a whole: the budget and what a process has before it opens anything stay under the address space with a twentieth to spare.
    let address_space = diacraft_sheetxml::model::ADDRESS_SPACE_BYTES;
    assert!(budget + diacraft_sheetxml::model::cost::BASE_BYTES <= address_space - address_space / 20);
}
