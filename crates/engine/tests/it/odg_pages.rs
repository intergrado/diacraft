//! The pages of a drawing opened from an `.odg`, added, renamed, taken away and moved through the commands, and saved: the writer matches the model's pages with the
//! file's by `ID` and writes each change into the file, and the engine's flattening of the new shapes finds each page's entry of the index by the page's `ID`, not by its place.

use diacraft_doc::Document;
use serde_json::json;

use crate::odg_build::{Dir, Drawing, content_of, names_of, open_drawing, read_back, rect};
use crate::odg_file::{assert_saves_again_as_no_edit, refused, save};
use crate::{check_session, run};

/// A drawing of two pages: `A` and `B` on the first, `C` on the second.
fn two_pages() -> Drawing {
    Drawing::new().page(&[rect("A", (2.0, 3.0, 4.0, 2.0), ""), rect("B", (12.0, 3.0, 4.0, 2.0), "")].concat()).page(&rect(
        "C",
        (2.0, 8.0, 4.0, 2.0),
        "",
    ))
}

fn page_names(doc: &Document) -> Vec<String> {
    doc.pages.iter().map(|p| p.name.to_string()).collect()
}

fn page_ids(doc: &Document) -> Vec<u32> {
    doc.pages.iter().map(|p| p.id).collect()
}

/// The drawing the saved file holds, read by the crate's reader.
fn reopened(dir: &Dir) -> Document {
    read_back(&dir.read("a.odg")).0
}

#[test]
fn a_page_added_and_renamed_and_a_shape_dropped_on_it_are_saved_and_the_file_reads_as_the_drawing_is() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &two_pages());
    let added = run(&mut s, "page.add", json!({}));
    let new = u32::try_from(added["id"].as_u64().unwrap()).unwrap();
    run(&mut s, "page.rename", json!({ "id": new, "name": "Notes" }));
    let dropped = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": new, "x": 4.0, "y": 2.0 }));
    assert!(dropped["id"].as_u64().is_some());
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let model = s.document().unwrap().clone();
    assert_eq!(page_names(&model), ["page1", "page2", "Notes"]);
    let back = reopened(&dir);
    assert_eq!(page_names(&back), ["page1", "page2", "Notes"]);
    assert_eq!(page_ids(&back), page_ids(&model), "the pages keep the IDs the session gave them");
    assert_eq!(back.pages.get(2).unwrap().shapes.len(), 1, "the shape dropped on the new page is in the file");
    assert_eq!(names_of(&back, 0), ["A", "B"], "and the pages that were there are as they were");
    let (_, report) = read_back(&dir.read("a.odg"));
    assert_eq!(report.warnings, Vec::<String>::new(), "the file reads with no warning");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    check_session(&s).unwrap();
}

#[test]
fn a_page_taken_away_and_a_page_moved_are_saved_and_the_next_edit_of_the_pages_that_moved_is_too() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &two_pages().page(&rect("D", (2.0, 8.0, 4.0, 2.0), "")));
    let doc = s.document().unwrap().clone();
    let third = doc.pages.get(2).unwrap().id;
    // The last page first, and the second page taken away.
    run(&mut s, "page.move", json!({ "id": third, "to": 0 }));
    let second = s.document().unwrap().pages.get(2).unwrap().id;
    run(&mut s, "page.delete", json!({ "id": second }));
    // A shape dropped on the page that moved, before anything is saved: the page now before the one that has two shapes holds one, so a flattening that looked the page up
    // by its place would read the entry of the page that stands there in the file (whose shapes have the `ID` the new shape has) and leave the new shape as it is.
    let moved = s.document().unwrap().pages.get(0).unwrap().id;
    let dropped = run(&mut s, "shape.drop", json!({ "master": "ellipse", "page": moved, "x": 6.0, "y": 2.0 }));
    // With a thing the save has no attribute for, which the flattening of a new shape carries in DiaCraft's own data (it does it of a shape it takes for the file's no more): the
    // file read again holds the rounding only if the new shape was flattened as a new shape is.
    run(&mut s, "shape.format", json!({ "ids": [dropped["id"].clone()], "page": moved, "rounding": 0.3 }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let model = s.document().unwrap().clone();
    let back = reopened(&dir);
    let rounded = back
        .pages
        .get(0)
        .unwrap()
        .shapes
        .iter()
        .any(|x| x.sheet.cell("Rounding").and_then(|c| c.value.as_ref()?.number()).is_some_and(|r| (r - 0.3).abs() < 1e-6));
    assert!(rounded, "the new shape on the page that moved was flattened as a new shape is, and its rounding is in the file: {saved}");
    assert_eq!(page_names(&back), page_names(&model));
    assert_eq!(page_ids(&back), page_ids(&model));
    assert_eq!(back.pages.get(0).unwrap().shapes.len(), 2, "the shape of the page that moved is with it, and so is the one dropped on it");
    assert_eq!(names_of(&back, 0)[0], "D");
    assert_eq!(back.pages.get(1).unwrap().shapes.len(), 2, "the page that did not move is as it was");
    assert!(!content_of(&dir.read("a.odg")).contains("draw:name=\"C\""), "the shapes of the page taken away are gone");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // And the same after the save, from the index it gave.
    run(&mut s, "shape.drop", json!({ "master": "ellipse", "page": moved, "x": 8.0, "y": 2.0 }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    assert_eq!(reopened(&dir).pages.get(0).unwrap().shapes.len(), 3);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    check_session(&s).unwrap();
}

#[test]
fn a_page_added_with_the_setup_of_another_is_saved_on_the_master_page_of_that_size() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &two_pages());
    let first = s.document().unwrap().pages.get(0).unwrap().id;
    run(&mut s, "page.size", json!({ "page": first, "width": 11.0, "height": 17.0 }));
    run(&mut s, "page.add", json!({ "like": first }));
    save(&mut s);
    let back = reopened(&dir);
    let size = |page: usize| {
        let p = back.pages.get(page).unwrap();
        let n = |cell: &str| p.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).unwrap();
        ((n("PageWidth") * 100.0).round() / 100.0, (n("PageHeight") * 100.0).round() / 100.0)
    };
    assert_eq!(size(0), (11.0, 17.0));
    assert_eq!(size(2), (11.0, 17.0), "the page added is the size of the page it copies");
    let content = content_of(&dir.read("a.odg"));
    let masters: Vec<&str> = content.split("draw:master-page-name=\"").skip(1).filter_map(|t| t.split('"').next()).collect();
    assert_eq!(masters[0], masters[2], "and names the master page that page names: {masters:?}");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

#[test]
fn the_last_page_cannot_be_taken_away_and_a_name_two_pages_would_share_is_refused_in_words() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &two_pages());
    let (first, second) = (s.document().unwrap().pages.get(0).unwrap().id, s.document().unwrap().pages.get(1).unwrap().id);
    // The engine's own rule: a drawing keeps a page.
    run(&mut s, "page.delete", json!({ "id": second }));
    assert!(s.execute("page.delete", &json!({ "id": first })).is_err(), "the engine keeps the only page");
    // The commands refuse a name another page has: the second page called `page1` comes into the model as it may from another program, and the save refuses two pages under
    // one name, in words that say what to do.
    assert!(s.execute("page.add", &json!({ "name": "page1" })).is_err(), "the command refuses the name");
    s.edit("a name another program gave", |d| {
        d.add_page(Some("page1")).map(|_| ()).map_err(|e| diacraft_engine::EngineError::Internal { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
    let words = refused(&mut s);
    assert!(words.contains("under one name") && words.contains("rename"), "{words}");
    // Renamed, the save is made.
    let ids = page_ids(s.document().unwrap());
    run(&mut s, "page.rename", json!({ "id": ids[1], "name": "page1 again" }));
    save(&mut s);
    assert_eq!(page_names(&reopened(&dir)), ["page1", "page1 again"]);
}

/// A drawing whose pages are called `names`, each holding one rectangle.
fn named_pages(names: &[Option<&str>]) -> Drawing {
    let mut d = Drawing::new();
    for (k, name) in names.iter().enumerate() {
        let shapes = rect(&format!("R{k}"), (2.0, 3.0, 4.0, 2.0), "");
        match name {
            Some(n) => d.pages.push_str(&format!("<draw:page draw:name=\"{n}\" draw:master-page-name=\"Default\">{shapes}</draw:page>")),
            None => d.pages.push_str(&format!("<draw:page draw:master-page-name=\"Default\">{shapes}</draw:page>")),
        }
    }
    d
}

#[test]
fn a_page_added_without_a_name_is_called_page_n_for_the_first_n_no_page_has_and_saves() {
    // A file read from an `.odg` numbers its pages from 0, so `Page-{ID}` would name the second page `Page-1` beside a first page that has the name: the name is the
    // next one that is free. A drawing whose pages are called otherwise, and one made new, name the page as they did.
    for (names, wanted) in [
        (vec![Some("Page-1")], "Page-2"),
        (vec![None, None], "Page-3"),
        (vec![Some("Page-2"), Some("Page-1")], "Page-3"),
        (vec![Some("page1")], "Page-1"),
        (vec![Some("Page-1"), Some("Page-2"), Some("Page-3")], "Page-4"),
    ] {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &named_pages(&names));
        let added = run(&mut s, "page.add", json!({}));
        assert_eq!(added["name"], wanted, "{names:?}");
        let saved = save(&mut s);
        assert_eq!(saved["edited"], true, "{names:?}: {saved}");
        let back = reopened(&dir);
        assert_eq!(page_names(&back).last().map(String::as_str), Some(wanted), "{names:?}");
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
    let mut s = diacraft_engine::Session::new();
    run(&mut s, "file.new", json!({}));
    let first = s.document().unwrap().pages.get(0).unwrap().name.to_string();
    let added = run(&mut s, "page.add", json!({}));
    assert_ne!(added["name"], first.as_str(), "a new drawing's second page has a name of its own");
}

#[test]
fn a_page_put_back_by_an_undo_after_a_save_took_it_away_is_refused_with_a_redo_as_the_way_out() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &two_pages());
    let second = s.document().unwrap().pages.get(1).unwrap().id;
    run(&mut s, "page.delete", json!({ "id": second }));
    save(&mut s);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.document().unwrap().pages.len(), 2, "the undo put the page back");
    let words = refused(&mut s);
    assert!(words.contains("put back by an undo") && words.contains("edit.redo"), "{words}");
    // Redone, the drawing is the one the file holds.
    run(&mut s, "edit.redo", json!({}));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], false, "the file already holds the drawing: {saved}");
}

#[test]
fn a_page_renamed_and_then_put_back_by_an_undo_is_saved_under_its_old_name_and_a_page_saved_again_after_a_rename_is_no_edit() {
    // Three saves, the first of a change that is not the page's name, the second of a rename, the third of the rename taken back.
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &two_pages());
    let second = s.document().unwrap().pages.get(1).unwrap().id;
    run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": second, "x": 4.0, "y": 2.0 }));
    assert_eq!(save(&mut s)["edited"], true);
    run(&mut s, "page.rename", json!({ "id": second, "name": "R" }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    assert_eq!(page_names(&reopened(&dir)), ["page1", "R"]);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(page_names(s.document().unwrap()), ["page1", "page2"], "the undo took the rename back");
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "the rename taken back is a change of the file: {saved}");
    assert_eq!(page_names(&reopened(&dir)), ["page1", "page2"], "and the file says so");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    check_session(&s).unwrap();
}

#[test]
fn a_page_added_in_one_save_and_renamed_in_the_next_is_renamed_back_in_a_third() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &two_pages());
    let added = run(&mut s, "page.add", json!({}));
    let new = u32::try_from(added["id"].as_u64().unwrap()).unwrap();
    run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": new, "x": 4.0, "y": 2.0 }));
    assert_eq!(save(&mut s)["edited"], true);
    let first_name = page_names(s.document().unwrap())[2].clone();
    run(&mut s, "page.rename", json!({ "id": new, "name": "Notes" }));
    assert_eq!(save(&mut s)["edited"], true);
    assert_eq!(page_names(&reopened(&dir))[2], "Notes");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    run(&mut s, "page.rename", json!({ "id": new, "name": first_name.clone() }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    assert_eq!(page_names(&reopened(&dir))[2], first_name);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    check_session(&s).unwrap();
}
