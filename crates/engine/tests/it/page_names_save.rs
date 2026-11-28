//! The saves: **a drawing is saved as a VSDX file with page names the program that opens the file keeps.** An `.odg` names its pages `page1`, `page2` (and a file of another
//! program, or a build before this one, may give two pages one name); the program that opens a VSDX file renames such a page and gives its `Name` and `NameU` different numbers. So a save
//! as a VSDX type gives the page the default form (`Page-N` for the lowest N no page has, in both names), says so in the save's warnings, and the drawing becomes what the file reads as.
//! **The `.odg` and the `.diacraft` writers keep the names as they are.** Every input is made by the tests' own builders and by commands.

use diacraft_doc::Document;
use diacraft_engine::Session;
use serde_json::json;

use crate::odg_build::{Dir, Drawing, open_bytes_in, open_drawing, read_back, rect};
use crate::{blank_session, check_session, page_names, run};

const SAID: &str = "given a new name in the file";

fn two_pages() -> Drawing {
    Drawing::new().page(&rect("A", (2.0, 3.0, 4.0, 2.0), "")).page(&rect("B", (2.0, 3.0, 4.0, 2.0), ""))
}

fn read_vsdx(bytes: &[u8]) -> Document {
    diacraft_vsdx::read(bytes, &diacraft_vsdx::Limits::default()).unwrap_or_else(|e| panic!("{e}")).0
}

fn names_in(doc: &Document) -> Vec<(String, String)> {
    doc.pages.iter().map(|p| (p.name.to_string(), p.name_u.as_deref().unwrap_or("").to_owned())).collect()
}

fn each(names: &[&str]) -> Vec<(String, String)> {
    names.iter().map(|n| ((*n).to_owned(), (*n).to_owned())).collect()
}

fn said(result: &serde_json::Value) -> Vec<String> {
    result["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).filter(|w| w.contains(SAID)).map(str::to_owned).collect()
}

/// A drawing whose two pages are both called `Page-1`, as a build before this one, or a file of another program, can leave it.
fn with_a_repeated_name() -> Session {
    let mut s = blank_session();
    run(&mut s, "page.add", json!({"name": "Second"}));
    s.edit("a name another program gave", |d| {
        d.rename_page(2, "Page-1").map_err(|e| diacraft_engine::EngineError::Internal { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
    assert_eq!(page_names(&s), ["Page-1", "Page-1"]);
    s
}

#[test]
fn an_odg_with_pages_page1_and_page2_saved_as_a_vsdx_file_has_page_1_and_page_2_and_says_so() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &two_pages());
    assert_eq!(page_names(&s), ["page1", "page2"]);
    let out = run(&mut s, "file.save_as", json!({ "path": dir.path("out.vsdx") }));
    let words = said(&out);
    assert_eq!(words.len(), 1, "{out}");
    assert!(words[0].starts_with("2 pages were given a new name in the file"), "{}", words[0]);
    assert!(words[0].contains("page 1 is now \"Page-1\"") && words[0].contains("page 2 is now \"Page-2\""), "{}", words[0]);
    // The file holds the names, in both.
    assert_eq!(names_in(&read_vsdx(&dir.read("out.vsdx"))), each(&["Page-1", "Page-2"]));
    // The save found nothing else to say about it: the check of the file against the drawing finds no page that differs, and the drawing is what the file reads as.
    assert_eq!(out["checked"]["classes"], json!([]), "{out}");
    assert_eq!(page_names(&s), ["Page-1", "Page-2"]);
    check_session(&s).unwrap();
}

#[test]
fn a_drawing_saved_as_an_odg_and_opened_again_and_saved_as_a_vsdx_file_is_named_the_same_way() {
    // The drawing is made by the commands: its pages are the `.odg`'s, saved as one, opened again, and saved as a VSDX file.
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &two_pages());
    run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    run(&mut s, "file.save", json!({}));
    assert_eq!(page_names(&read_back_session(&dir)), ["page1", "page2"], "the .odg writer keeps the names");
    let mut again = open_bytes_in(&dir, "again.odg", &dir.read("a.odg"));
    assert_eq!(page_names(&again), ["page1", "page2"]);
    let out = run(&mut again, "file.save_as", json!({ "path": dir.path("then.vsdx") }));
    assert_eq!(said(&out).len(), 1, "{out}");
    assert_eq!(names_in(&read_vsdx(&dir.read("then.vsdx"))), each(&["Page-1", "Page-2"]));
    // Saved into again, the file is spliced: nothing changes in the names and there is nothing to say.
    run(&mut again, "shape.drop", json!({ "master": "ellipse", "page": 1, "x": 5.0, "y": 5.0 }));
    let saved = run(&mut again, "file.save", json!({}));
    assert!(said(&saved).is_empty(), "{saved}");
    assert_eq!(names_in(&read_vsdx(&dir.read("then.vsdx"))), each(&["Page-1", "Page-2"]));
}

fn read_back_session(dir: &Dir) -> Session {
    open_bytes_in(dir, "back.odg", &dir.read("a.odg"))
}

#[test]
fn the_odg_writer_keeps_the_names_it_is_given() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &two_pages());
    run(&mut s, "page.add", json!({"name": "Notes"}));
    run(&mut s, "file.save", json!({}));
    let back = read_back(&dir.read("a.odg")).0;
    assert_eq!(back.pages.iter().map(|p| p.name.to_string()).collect::<Vec<_>>(), ["page1", "page2", "Notes"]);
    run(&mut s, "file.save_as", json!({ "path": dir.path("copy.odg") }));
    let copy = read_back(&dir.read("copy.odg")).0;
    assert_eq!(copy.pages.iter().map(|p| p.name.to_string()).collect::<Vec<_>>(), ["page1", "page2", "Notes"]);
}

#[test]
fn a_drawing_with_a_repeated_name_is_kept_as_it_is_in_a_diacraft_file_and_renamed_in_a_vsdx_file() {
    let dir = Dir::new();
    let mut s = with_a_repeated_name();
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let mut native = open_bytes_in(&dir, "b.diacraft", &dir.read("a.diacraft"));
    assert_eq!(page_names(&native), ["Page-1", "Page-1"], "the native format keeps what the drawing holds");
    let out = run(&mut native, "file.save_as", json!({ "path": dir.path("c.vsdx") }));
    assert_eq!(said(&out).len(), 1, "{out}");
    assert_eq!(names_in(&read_vsdx(&dir.read("c.vsdx"))), each(&["Page-1", "Page-2"]));
    // The drawing in the session is not turned into another by an export of it, and says "Page-1" twice still.
    assert_eq!(page_names(&native), ["Page-1", "Page-1"]);
    // A drawing never saved is written so as well.
    let out = run(&mut s, "file.save_as", json!({ "path": dir.path("d.vsdx") }));
    assert_eq!(said(&out).len(), 1, "{out}");
    assert_eq!(names_in(&read_vsdx(&dir.read("d.vsdx"))), each(&["Page-1", "Page-2"]));
}

#[test]
fn a_drawing_with_names_the_program_keeps_is_saved_as_a_vsdx_file_with_no_word_about_names() {
    let dir = Dir::new();
    let mut s = blank_session();
    run(&mut s, "page.add", json!({"name": "Overview"}));
    let out = run(&mut s, "file.save_as", json!({ "path": dir.path("fine.vsdx") }));
    assert!(said(&out).is_empty(), "{out}");
    assert_eq!(names_in(&read_vsdx(&dir.read("fine.vsdx"))), each(&["Page-1", "Overview"]));
}

#[test]
fn a_cell_that_names_a_page_the_save_renamed_names_its_new_name_keeps_its_value_and_the_save_says_so() {
    // The name `page1` is the drawing's and not the person's choice, so the save that gives the page the name `Page-1` rewrites the formula that named it, and nothing reads `#REF!` for it.
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &two_pages());
    let dropped = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 1, "x": 4.0, "y": 2.0 }));
    let id = u32::try_from(dropped["id"].as_u64().unwrap()).unwrap();
    run(&mut s, "sheet.set_cell", json!({"page": 1, "id": id, "cell": "Width", "formula": "Pages[page1]!PageWidth*0+1"}));
    let before = crate::page_readers::full(&mut s, 1, id, "Width");
    assert_eq!(before["error"], serde_json::Value::Null, "the name finds the page: {before}");
    let out = run(&mut s, "file.save_as", json!({ "path": dir.path("out.vsdx") }));
    let words = said(&out);
    assert_eq!(words.len(), 1, "{out}");
    assert!(
        words[0].ends_with(". 1 formula that named such a page by its old name now names it by its new name and keeps its value"),
        "{}",
        words[0]
    );
    let mut reopened = Session::new();
    reopened.execute("file.open", &json!({ "path": dir.path("out.vsdx") })).unwrap();
    for (what, session) in [("the file", &mut reopened), ("the drawing the save left", &mut s)] {
        let after = crate::page_readers::full(session, 1, id, "Width");
        assert_eq!(after["error"], serde_json::Value::Null, "{what}: no `#REF!` for the page the save renamed: {after}");
        assert_eq!(after["value"], before["value"], "{what}: the value is kept");
        assert_eq!(after["formula"], json!("Pages[Page-1]!PageWidth*0+1"), "{what}: the formula names the page by its new name");
        crate::sheet_cross_pages::settled(session.document().unwrap());
    }
}

#[test]
fn many_odg_pages_saved_as_a_vsdx_file_are_each_named_by_their_place_and_a_save_after_it_is_no_edit() {
    // Pages page1 to page17, as an `.odg` of seventeen pages names them: saved as a VSDX file, then saved into again with nothing edited.
    for count in [1usize, 2, 3, 10, 17] {
        let dir = Dir::new();
        let mut drawing = Drawing::new();
        for at in 0..count {
            drawing = drawing.page(&rect(&format!("R{at}"), (2.0, 3.0, 4.0, 2.0), ""));
        }
        let mut s = open_drawing(&dir, &drawing);
        assert_eq!(page_names(&s).first().map(String::as_str), Some("page1"));
        let out = run(&mut s, "file.save_as", json!({ "path": dir.path("out.vsdx") }));
        let expected: Vec<String> = (1..=count).map(|n| format!("Page-{n}")).collect();
        let wanted: Vec<(String, String)> = expected.iter().map(|n| (n.clone(), n.clone())).collect();
        assert_eq!(names_in(&read_vsdx(&dir.read("out.vsdx"))), wanted, "{count} pages");
        assert_eq!(said(&out).len(), 1, "{count} pages: {out}");
        assert_eq!(out["checked"]["classes"], json!([]), "{count} pages: {out}");
        assert_eq!(page_names(&s), expected, "{count} pages: the drawing is what the file reads as");
        // A save into the file with nothing edited writes what it holds and has nothing to say about names.
        let before = dir.read("out.vsdx");
        let again = run(&mut s, "file.save", json!({}));
        assert!(said(&again).is_empty(), "{count} pages: {again}");
        assert_eq!(dir.read("out.vsdx"), before, "{count} pages: no edit, no change");
        check_session(&s).unwrap();
    }
}

/// Writes a VSDX file of an `.odg` of three pages to the folder `DIACRAFT_LO_OUT`, for opening in LibreOffice Draw by hand (`soffice --headless --convert-to odg`): its pages read as `Page-1`,
/// `Page-2` and `Page-3`.
#[test]
#[ignore = "writes a file for the LibreOffice check by hand"]
fn lo_page_names_write() {
    let Some(out) = std::env::var_os("DIACRAFT_LO_OUT") else { return };
    let dir = Dir::new();
    let drawing = Drawing::new().page(&rect("A", (2.0, 3.0, 4.0, 2.0), "one")).page(&rect("B", (2.0, 3.0, 4.0, 2.0), "two")).page(&rect(
        "C",
        (2.0, 3.0, 4.0, 2.0),
        "three",
    ));
    let mut s = open_drawing(&dir, &drawing);
    run(&mut s, "file.save_as", json!({ "path": dir.path("pages.vsdx") }));
    std::fs::write(std::path::Path::new(&out).join("pages.vsdx"), dir.read("pages.vsdx")).unwrap();
}
