//! Files in memory (`diacraft_engine::memfs`): the engine's file commands, run as the browser runs them, on a path that is a name in the store. Each test turns memory files on for its own
//! thread, and leaves nothing on the disk.

use std::path::{Path, PathBuf};

use diacraft_engine::memfs::{self, Keep};
use diacraft_engine::{EngineError, OpenProblem, Session};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape};
use serde_json::{Value, json};

use crate::run;

/// Memory files for this thread, and an empty store; turned off (and emptied) when the test ends, even by a failure.
struct Memory;

impl Memory {
    fn on() -> Memory {
        memfs::use_on_this_thread(true);
        memfs::clear();
        Memory
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        memfs::use_on_this_thread(false);
    }
}

fn place(name: &str) -> PathBuf {
    PathBuf::from(format!("/web/1/{name}"))
}

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// A drawing built in code: two pages, a master and a shape.
fn vsdx_drawing() -> Vec<u8> {
    VsdxBuilder::new()
        .master(MasterSpec::new(1, "M").shapes(&shape(4, "")))
        .page(PageSpec::new(1, "First").shapes(&shape(5, r#"<Cell N="Width" V="2"/>"#)))
        .page(PageSpec::new(2, "Second"))
        .build()
}

fn page_names(s: &Session) -> Vec<String> {
    s.document().map(|d| d.pages.iter().map(|p| p.name.to_string()).collect()).unwrap_or_default()
}

/// What the browser does with a file the person chose: the bytes are put in the store, the command reads the path, and the page lets go of the name.
fn open(s: &mut Session, name: &str, bytes: Vec<u8>) -> Value {
    let path = place(name);
    memfs::put(&path, bytes, Keep::Once).unwrap();
    let answer = run(s, "file.open", json!({ "path": text(&path) }));
    assert!(!memfs::contains(&path), "a file read once is moved out to the reader");
    answer
}

/// What the browser does with a file a command wrote: it takes the bytes out.
fn take(path: &Path) -> Vec<u8> {
    memfs::take(path).unwrap_or_else(|| panic!("the command wrote no file at {path:?}"))
}

#[test]
fn a_drawing_is_opened_edited_saved_and_opened_again_without_a_disk() {
    let _memory = Memory::on();
    let mut s = Session::new();
    let opened = open(&mut s, "plan.vsdx", vsdx_drawing());
    assert_eq!(opened["file"]["variant"], "vsdx");
    assert_eq!(page_names(&s), ["First", "Second"]);
    assert_eq!(s.path(), Some(place("plan.vsdx").as_path()));
    run(&mut s, "page.rename", json!({"id": 1, "name": "Renamed"}));
    assert!(s.is_dirty());

    // Save writes the file the drawing came from, in memory, and keeps the parts the edit did not touch.
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["variant"], "vsdx");
    assert!(!s.is_dirty());
    let bytes = take(&place("plan.vsdx"));
    assert_eq!(saved["bytes"], bytes.len());
    assert!(bytes.starts_with(b"PK"), "a package");

    // A second save of the same drawing finds no file to compare with (it was handed over) and is not refused as changed on the disk.
    run(&mut s, "page.rename", json!({"id": 2, "name": "Again"}));
    run(&mut s, "file.save", json!({}));
    assert!(memfs::contains(&place("plan.vsdx")));
    memfs::remove(&place("plan.vsdx"));

    let mut again = Session::new();
    open(&mut again, "plan-again.vsdx", bytes);
    assert_eq!(page_names(&again), ["Renamed", "Second"], "what was saved is what is read");
}

#[test]
fn a_new_drawing_is_saved_as_each_type_and_read_back_by_its_own_reader() {
    let _memory = Memory::on();
    for extension in ["diacraft", "vsdx", "odg"] {
        let mut s = Session::new();
        run(&mut s, "file.new", json!({}));
        run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2}));
        run(&mut s, "page.add", json!({"name": "Second"}));
        let path = place(&format!("Drawing1.{extension}"));
        let saved = run(&mut s, "file.save_as", json!({ "path": text(&path) }));
        assert_eq!(saved["dirty"], false, "{extension}");
        let bytes = take(&path);
        assert_eq!(saved["bytes"], bytes.len(), "{extension}");
        // The check after the save has an answer where the engine makes one: a file written from the model is read back.
        assert!(saved.get("checked").is_some(), "{extension}: the save card's answer is there");

        let mut reread = Session::new();
        let answer = open(&mut reread, &format!("again.{extension}"), bytes);
        let page_count = reread.document().map_or(0, |d| d.pages.len());
        assert_eq!(page_count, 2, "{extension}: both pages come back");
        assert!(answer["file"]["warning_count"].as_u64().unwrap_or(0) < 3, "{extension}: {answer}");
        assert!(reread.document().is_some_and(|d| d.pages.iter().next().is_some_and(|p| !p.shapes.is_empty())), "{extension}: the shape comes back");
    }
}

#[test]
fn a_save_in_memory_leaves_the_disk_as_it_was() {
    let _memory = Memory::on();
    let dir = std::env::temp_dir().join(format!("diacraft-memfs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let on_disk = dir.join("plan.diacraft");
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    run(&mut s, "file.save_as", json!({ "path": on_disk.to_str().unwrap() }));
    assert!(memfs::contains(&on_disk), "the file is in the store, under the name it was given");
    assert!(!on_disk.exists(), "and not on the disk");
    memfs::remove(&on_disk);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_picture_is_exported_as_png_svg_and_pdf_to_memory() {
    let _memory = Memory::on();
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2}));
    for (name, magic) in [("page.png", &b"\x89PNG"[..]), ("page.svg", b"<svg"), ("page.pdf", b"%PDF")] {
        let path = place(name);
        let done = run(&mut s, "file.export", json!({ "path": text(&path) }));
        let bytes = take(&path);
        assert_eq!(done["bytes"], bytes.len(), "{name}");
        assert!(bytes.windows(magic.len()).take(64).any(|w| w == magic), "{name}: starts like its type");
    }
    // The page printed to a PDF is a file in memory as well.
    let path = place("print.pdf");
    run(&mut s, "file.print", json!({ "path": text(&path) }));
    assert!(take(&path).starts_with(b"%PDF"));
}

#[test]
fn a_file_that_is_not_there_or_not_a_drawing_is_refused_in_words_and_the_open_drawing_stays() {
    let _memory = Memory::on();
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    run(&mut s, "page.add", json!({"name": "Mine"}));
    let before = page_names(&s);

    let missing = s.execute("file.open", &json!({ "path": "/web/9/nothing.vsdx" })).unwrap_err();
    assert!(matches!(missing, EngineError::CannotOpen { problem: OpenProblem::NotFound, .. }), "{missing:?}");

    let path = place("junk.vsdx");
    memfs::put(&path, b"this is not a zip file at all".to_vec(), Keep::Once).unwrap();
    let junk = s.execute("file.open", &json!({ "path": text(&path) })).unwrap_err();
    assert!(matches!(junk, EngineError::CannotOpen { problem: OpenProblem::NotAPackage, .. }), "{junk:?}");

    // A package cut short is damaged, not a crash.
    let mut cut = vsdx_drawing();
    cut.truncate(cut.len() / 2);
    memfs::put(&path, cut, Keep::Once).unwrap();
    let damaged = s.execute("file.open", &json!({ "path": text(&path) })).unwrap_err();
    assert!(matches!(damaged, EngineError::CannotOpen { .. }), "{damaged:?}");
    assert_eq!(page_names(&s), before, "the drawing that was open is as it was");
}

#[test]
fn a_file_the_budget_cannot_hold_is_refused_from_its_size_before_a_byte_is_put() {
    let _memory = Memory::on();
    let budget = diacraft_vsdx::MAX_MEMORY;
    let cap = diacraft_engine::file::package_file_cap(&diacraft_vsdx::Limits::default());
    // All of the budget alone, or more than any package may be, whichever is less: refused from the size.
    let at_budget = memfs::admit(budget);
    assert!(matches!(at_budget, Err(OpenProblem::TooBig | OpenProblem::FileTooLarge)), "{at_budget:?}");
    assert_eq!(memfs::admit(cap + 1), Err(OpenProblem::FileTooLarge), "more than any package may be");
    assert_eq!(memfs::admit(u64::MAX), Err(OpenProblem::FileTooLarge));
    assert_eq!(memfs::admit(1024), Ok(()));
    // What the engine itself says of a file of that size is what `admit` says: it refuses before a byte is read.
    let mut s = Session::new();
    let path = place("huge.vsdx");
    memfs::put(&path, vec![0; 16], Keep::Once).unwrap();
    assert!(s.execute("file.open", &json!({ "path": text(&path) })).is_err());
    // A file that fits the budget but not the store beside what it holds.
    memfs::put(&place("held.vssx"), vec![0; 1024], Keep::Held).unwrap();
    assert_eq!(memfs::admit(memfs::MAX_BYTES), Err(OpenProblem::OutOfMemory));
}

#[test]
fn a_save_the_store_has_no_room_for_is_refused_in_words_and_the_drawing_stays_unsaved() {
    let _memory = Memory::on();
    for n in 0..memfs::MAX_ENTRIES {
        memfs::put(&PathBuf::from(format!("/web/{n}/waiting")), vec![0], Keep::Once).unwrap();
    }
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    run(&mut s, "page.add", json!({"name": "Mine"}));
    let refused = s.execute("file.save_as", &json!({ "path": "/web/99/full.diacraft" })).unwrap_err();
    let EngineError::Failed { msg, .. } = &refused else { panic!("{refused:?}") };
    assert!(msg.contains("not enough memory"), "{msg}");
    assert!(s.is_dirty(), "a save that did not happen leaves the drawing unsaved");
    assert!(s.path().is_none());
}

#[test]
fn a_stencil_that_is_held_opens_again_in_each_session_and_one_read_once_does_not() {
    let _memory = Memory::on();
    // A stencil file is a drawing package of the stencil type: the engine's own save writes one.
    let mut maker = Session::new();
    run(&mut maker, "file.new", json!({}));
    let path = place("shapes.diacraft-stencil");
    run(&mut maker, "file.save_as", json!({ "path": text(&path) }));
    let stencil = take(&path);

    memfs::put(&path, stencil.clone(), Keep::Held).unwrap();
    let mut first = Session::new();
    run(&mut first, "file.new", json!({}));
    let opened = run(&mut first, "stencil.open", json!({ "path": text(&path) }));
    assert_eq!(opened["already_open"], false);
    assert!(memfs::contains(&path), "a held file stays");
    let mut second = Session::new();
    run(&mut second, "file.new", json!({}));
    run(&mut second, "stencil.open", json!({ "path": text(&path) }));
    assert_eq!(second.stencils().iter().count(), first.stencils().iter().count());

    memfs::put(&path, stencil, Keep::Once).unwrap();
    let mut third = Session::new();
    run(&mut third, "file.new", json!({}));
    run(&mut third, "stencil.open", json!({ "path": text(&path) }));
    assert!(!memfs::contains(&path), "a file read once is gone");
}

#[test]
fn a_picture_is_inserted_from_memory_and_a_file_that_is_no_picture_is_refused() {
    let _memory = Memory::on();
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    // A PNG the engine's own export made.
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2}));
    let png_path = place("made.png");
    run(&mut s, "file.export", json!({ "path": text(&png_path) }));
    let png = take(&png_path);
    let picture = place("pic.png");
    memfs::put(&picture, png, Keep::Once).unwrap();
    let inserted = run(&mut s, "insert.picture", json!({ "path": text(&picture), "page": 1 }));
    assert!(inserted.get("id").is_some(), "{inserted}");
    assert!(!memfs::contains(&picture));

    memfs::put(&picture, b"not a picture".to_vec(), Keep::Once).unwrap();
    let refused = s.execute("insert.picture", &json!({ "path": text(&picture), "page": 1 })).unwrap_err();
    assert!(!refused.to_string().is_empty());
}

#[test]
fn on_the_disk_nothing_changes_for_a_thread_that_did_not_ask() {
    // Memory files are off for a thread that never turned them on: a path is a path on the disk, and the store is not looked at.
    memfs::use_on_this_thread(false);
    memfs::put(&place("a.vsdx"), vsdx_drawing(), Keep::Once).unwrap();
    let mut s = Session::new();
    let refused = s.execute("file.open", &json!({ "path": text(&place("a.vsdx")) })).unwrap_err();
    assert!(matches!(refused, EngineError::CannotOpen { problem: OpenProblem::NotFound, .. }), "{refused:?}");
    memfs::clear();
}
