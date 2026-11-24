//! 2026-11-14: what a drawing saved as an `.odg` could not carry, now carried. Each case opens an `.odg` made by the writer (a hand-made
//! package that the writer then saved with what the case needs), applies the command, saves, **opens the file in a session of its own** and reads what came back, and
//! saves again, which must be no edit.

use std::sync::Arc;

use diacraft_doc::{CellName, Document, Master, Shape, ShapeList};
use diacraft_engine::Session;
use serde_json::json;

use crate::odg_build::{Dir, id_of, names_of, open_bytes_in, open_drawing, part_bytes, part_of, read_back};
use crate::odg_file::{assert_saves_again_as_no_edit, four, save, shift};
use crate::run;

/// A master of `id` with one shape and one cell of its own: what a file that carries masters holds.
fn master(id: u32) -> Master {
    let mut m = Master { id, name: Some(Arc::from(format!("Carried {id}"))), name_u: Some(Arc::from(format!("Carried.{id}"))), ..Master::default() };
    let mut shape = Shape::new(id * 10 + 1);
    let sheet = Arc::make_mut(&mut shape.sheet);
    sheet.cells.insert(CellName::new("Width"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text(&format!("{id}"))));
    m.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    m
}

/// The bytes of `four()` saved by the writer with `masters` masters in its model: an `.odg` that carries them in DiaCraft's document file.
fn four_with_masters(masters: u32) -> Vec<u8> {
    let bytes = four().bytes();
    let limits = diacraft_odg::Limits::default();
    let (mut doc, report) = diacraft_odg::read(&bytes, &limits).expect("the package reads");
    for id in 1..=masters {
        doc.masters.push(Arc::new(master(id)));
    }
    let original = diacraft_odg::OriginalPackage { bytes: &bytes, index: &report.index };
    let written = diacraft_odg::write_with(&doc, Some(&original), &diacraft_odg::WriteOptions::default()).expect("the writer saves the masters");
    written.bytes
}

fn masters_of(doc: &Document) -> Vec<(u32, String)> {
    doc.masters.iter().map(|m| (m.id, m.name.as_deref().unwrap_or("").to_owned())).collect()
}

fn reopened(dir: &Dir, name: &str) -> Session {
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path(name) }));
    again
}

/// Item 4: `flatten` took every master out of the copy of every `.odg` save, the file's own included, so a file of three carried masters held none after one edit
/// and one save. The masters the file held are kept, by their ids, by every save after it too.
#[test]
fn a_save_after_shape_move_keeps_the_masters_the_file_carries() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &four_with_masters(3));
    let held = masters_of(s.document().unwrap());
    assert_eq!(held.len(), 3, "the file carries three masters and the reader reads them: {held:?}");
    shift(&mut s, "A", 1.0, 0.0);
    assert_eq!(save(&mut s)["edited"], true);
    let (doc, report) = read_back(&dir.read("a.odg"));
    assert_eq!(masters_of(&doc), held, "the file still carries its masters");
    assert_eq!(report.warnings, Vec::<String>::new());
    let again = reopened(&dir, "a.odg");
    assert_eq!(masters_of(again.document().unwrap()), held, "and a session of its own reads them");
    assert_eq!(
        again.document().unwrap().masters.iter().map(|m| (**m).clone()).collect::<Vec<_>>(),
        s.document().unwrap().masters.iter().map(|m| (**m).clone()).collect::<Vec<_>>()
    );
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // A second edit and a second save keep them too (the state a save leaves holds the ids as well).
    shift(&mut s, "B", 1.0, 0.0);
    assert_eq!(save(&mut s)["edited"], true);
    assert_eq!(masters_of(&read_back(&dir.read("a.odg")).0), held);
}

/// What a drop adds to the document is the file's too (NEWSAVE left it out): the master the drop brings is written beside the three the
/// file held, the shape it makes keeps its link to it, and a file read again has the four masters, equal to the session's, and the shape linked to the fourth.
#[test]
fn shape_drop_brings_a_master_the_save_carries_beside_the_masters_the_file_held() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &four_with_masters(3));
    let held = masters_of(s.document().unwrap());
    let made = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    let in_session = masters_of(s.document().unwrap());
    assert!(in_session.len() > held.len() && in_session[..held.len()] == held[..], "the drop brought its master into the model: {in_session:?}");
    let linked = |doc: &Document| doc.pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap().master;
    let master = linked(s.document().unwrap()).expect("the dropped shape is an instance");
    save(&mut s);
    let (doc, _) = read_back(&dir.read("a.odg"));
    assert_eq!(masters_of(&doc), in_session, "the file holds the three it had and the drop's");
    assert_eq!(linked(&doc), Some(master), "and the dropped shape is linked to the master the drop brought");
    let again = reopened(&dir, "a.odg");
    assert_eq!(masters_of(again.document().unwrap()), in_session);
    assert_eq!(
        again.document().unwrap().masters.iter().map(|m| (**m).clone()).collect::<Vec<_>>(),
        s.document().unwrap().masters.iter().map(|m| (**m).clone()).collect::<Vec<_>>(),
        "each master is what the session has, body and all"
    );
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    let _ = id_of;
}

/// A file with no masters gets the one a drop brings (it was left with none until a later change): the file has a document file, the shape is linked, and the file read
/// again holds the master and saves again as no edit.
#[test]
fn shape_drop_in_a_file_that_carries_no_master_gives_it_the_master() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &four().bytes());
    run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    save(&mut s);
    let (doc, _) = read_back(&dir.read("a.odg"));
    assert_eq!(masters_of(&doc), masters_of(s.document().unwrap()));
    assert_eq!(doc.masters.len(), 1);
    assert_eq!(doc.pages.get(0).unwrap().shapes.iter().filter(|x| x.master == Some(doc.masters.get(0).unwrap().id)).count(), 1);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// Item 2: `snap.set` changes the words of `DocumentSettings`, which the model keeps among the unknown nodes of the document, and a save of a package compared
/// them as it compared the rest of those nodes: refused. DiaCraft's document file carries them now (`words`), so the change is saved, read back by a session of its
/// own, and a second save is no edit.
#[test]
fn snap_set_is_saved_and_the_file_reads_back_the_same_options() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let before = run(&mut s, "snap.settings", json!({}));
    run(&mut s, "snap.set", json!({ "grid": false, "glue_vertices": true, "ext_curved": true, "dynamic_grid": false }));
    let held = run(&mut s, "snap.settings", json!({}));
    assert_ne!(held, before, "the command changed the options");
    assert!(!held["words"]["SnapSettings"].is_null() && !held["words"]["GlueSettings"].is_null(), "and the words that hold them: {held}");
    assert_eq!(save(&mut s)["edited"], true);
    let file = part_of(&dir.read("a.odg"), "DiaCraft/document.xml");
    assert!(
        file.contains("<diacraft:words ") && file.contains("snap=\"") && file.contains("glue=\""),
        "the words are in DiaCraft's document file: {file}"
    );
    let mut again = reopened(&dir, "a.odg");
    assert_eq!(run(&mut again, "snap.settings", json!({})), held, "a session of its own reads the options and the words as they were");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // The reopened drawing, opened again now that the first session has saved twice: saved with no change it is no edit, and with another change it is saved and
    // read once more.
    let mut again = reopened(&dir, "a.odg");
    assert_saves_again_as_no_edit(&dir, &mut again, "a.odg");
    run(&mut again, "snap.set", json!({ "guides": false }));
    let second = run(&mut again, "snap.settings", json!({}));
    assert_eq!(save(&mut again)["edited"], true);
    assert_eq!(run(&mut reopened(&dir, "a.odg"), "snap.settings", json!({})), second);
}

/// The switches (`snap`, `glue`) are DiaCraft's own data, which the document file carried before this task: they are saved with the words.
#[test]
fn snap_set_of_the_switches_is_saved() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    run(&mut s, "snap.set", json!({ "snap": false, "glue": false }));
    assert_eq!(save(&mut s)["edited"], true);
    let mut again = reopened(&dir, "a.odg");
    let read = run(&mut again, "snap.settings", json!({}));
    assert_eq!((read["snap"].clone(), read["glue"].clone()), (json!(false), json!(false)));
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// The words are the drawing's and `snap.set` is no undo step, so an undo does not take them out of the file. An edit, the options turned
/// and the edit's undo (which goes back to a drawing made before the words were there) leave the words in what a save writes; there is nothing to undo of `snap.set` itself.
#[test]
fn snap_set_is_kept_by_a_save_through_the_undo_of_an_edit_and_is_no_step_of_its_own() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    run(&mut s, "snap.set", json!({ "grid": false }));
    save(&mut s);
    assert!(part_bytes(&dir.read("a.odg"), "DiaCraft/document.xml").is_some());
    assert!(s.execute("edit.undo", &json!({})).is_err(), "no undo step: nothing to undo");
    let words = s.document().map(|doc| doc.snap_words()).unwrap_or_default();
    assert_ne!(words, diacraft_doc::SnapWords::default());
    // An edit, and the edit's undo: the drawing the undo goes to was made before the options were turned, and keeps them.
    run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.document().map(|doc| doc.snap_words()), Some(words.clone()), "the undo of an edit leaves the words");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    assert!(part_bytes(&dir.read("a.odg"), "DiaCraft/document.xml").is_some(), "the words stay in the file");
    let (doc, report) = read_back(&dir.read("a.odg"));
    assert_eq!(doc.snap_words(), words);
    assert_eq!(report.warnings, Vec::<String>::new());
}

fn guides(s: &mut Session) -> serde_json::Value {
    run(s, "guide.list", json!({ "page": 0 }))
}

fn guide_shapes(doc: &Document) -> Vec<Shape> {
    doc.pages.get(0).unwrap().shapes.iter().filter(|s| s.kind == diacraft_doc::ShapeType::Guide).map(|s| (**s).clone()).collect()
}

/// Item 3: `guide.add` was refused as "a new shape that is turned or flipped" (a vertical guide is turned a quarter turn). A guide is saved now, in the file as a shape of
/// no size whose style says it is a guide (`diacraft:type`), and a session of its own reads it back as a guide with its cells; it moves, is deleted and is saved again
/// as any shape is, and a second save is no edit.
#[test]
fn guide_add_is_saved_and_read_back_as_a_guide() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let across = run(&mut s, "guide.add", json!({ "page": 0, "orientation": "vertical", "at": 2.0 }));
    let along = run(&mut s, "guide.add", json!({ "page": 0, "orientation": "horizontal", "at": 3.5 }));
    let held = guides(&mut s);
    assert_eq!(held["guides"].as_array().unwrap().len(), 2, "{held}");
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    assert_eq!(saved["warning_count"], 0, "a guide says nothing of its outline: {saved}");
    let content = part_of(&dir.read("a.odg"), "content.xml");
    assert!(content.contains("draw:transform=\"rotate (1.5707963267949)"), "the vertical guide is turned a quarter turn, in radians: {content}");
    assert_eq!(content.matches("diacraft:type=\"Guide\"").count(), 2, "both guides say what they are: {content}");
    // They draw nothing in another program (a shape of no size is a dot in LibreOffice, with the root's line): each has a style of no fill and no line.
    assert!(content.matches("draw:fill=\"none\"").count() >= 2 && content.matches("draw:stroke=\"none\"").count() >= 2, "{content}");
    let mut again = reopened(&dir, "a.odg");
    assert_eq!(guides(&mut again), held, "a session of its own lists the guides as they were");
    let (mine, theirs) = (guide_shapes(s.document().unwrap()), guide_shapes(again.document().unwrap()));
    assert_eq!(mine.len(), 2);
    for (a, b) in mine.iter().zip(theirs.iter()) {
        for cell in ["NonPrinting", "ShapePermeableX", "ShapePermeableY", "ShapePermeablePlace", "Angle", "PinX", "PinY", "Width", "Height"] {
            // A cell the file leaves out is 0 (the reader writes `Angle` only when it is not 0): the cells that make a guide are 1 or a place and are not left out.
            let n = |s: &Shape| s.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0);
            assert!((n(a) - n(b)).abs() < 1e-9, "{cell}: {} in the session and {} in the file", n(a), n(b));
        }
        assert!(
            b.sheet.sections.iter().any(|s| s.name.as_str() == "Geometry" && s.rows.iter().any(|r| r.row_type.as_deref() == Some("InfiniteLine")))
        );
    }
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // The reopened drawing moves a guide, and deletes the other: saved, and read once more.
    let mut again = reopened(&dir, "a.odg");
    run(&mut again, "guide.move", json!({ "id": across["id"], "page": 0, "at": 2.5 }));
    run(&mut again, "guide.delete", json!({ "id": along["id"], "page": 0 }));
    let wanted = guides(&mut again);
    assert_eq!(wanted["guides"].as_array().unwrap().len(), 1);
    assert_eq!(save(&mut again)["edited"], true);
    let mut last = reopened(&dir, "a.odg");
    assert_eq!(guides(&mut last), wanted, "the moved guide and no other");
    assert_eq!(guide_shapes(last.document().unwrap()).len(), 1);
    assert_saves_again_as_no_edit(&dir, &mut last, "a.odg");
    // The four shapes of the page are where they were.
    assert_eq!(last.document().unwrap().pages.get(0).unwrap().shapes.iter().filter(|s| s.kind != diacraft_doc::ShapeType::Guide).count(), 4);
}

/// The bytes of `four()` saved by the writer with the theme `id` of the catalogue in its model, as the bytes of a part and nothing parsed: what a document file
/// holds of a theme, and what the `odg` crate reads it back as.
fn four_with_theme(id: &str) -> Vec<u8> {
    let bytes = four().bytes();
    let (mut doc, report) = diacraft_odg::read(&bytes, &diacraft_odg::Limits::default()).expect("the package reads");
    let theme = diacraft_engine::theme::find(id).expect("a theme of the catalogue");
    doc.themes.push(Arc::new(diacraft_doc::Theme {
        part: Arc::from("theme/theme1.xml"),
        bytes: Arc::clone(&theme.bytes),
        ..diacraft_doc::Theme::default()
    }));
    let original = diacraft_odg::OriginalPackage { bytes: &bytes, index: &report.index };
    diacraft_odg::write_with(&doc, Some(&original), &diacraft_odg::WriteOptions::default()).expect("the writer saves the theme").bytes
}

/// A theme a document file carries is bytes and nothing more when the `odg` crate reads it (that crate is no reader of DrawingML), so a model read from an `.odg`
/// had no colour scheme, no number a `ThemeIndex` selects it by and no fonts: `theme.list` said the theme was not in the drawing and a second `theme.apply` added
/// it again. The engine reads the part when it opens the file, as the VSDX reader does.
#[test]
fn theme_list_and_theme_apply_find_the_theme_a_file_carries_once_it_is_opened() {
    let dir = Dir::new();
    let fern = diacraft_engine::theme::find("fernwood").unwrap();
    let mut s = open_bytes_in(&dir, "a.odg", &four_with_theme("fernwood"));
    let doc = s.document().unwrap();
    assert_eq!(doc.themes.len(), 1);
    let found = doc.theme_by_index(fern.scheme()).expect("the theme is found by the number its components carry");
    assert!(found.colors.is_some() && found.major_font.is_some(), "its colours and fonts are read: {found:?}");
    assert_eq!(found.bytes, fern.bytes, "and its bytes are as they were");
    let listed = run(&mut s, "theme.list", json!({}));
    let mine = listed["themes"].as_array().unwrap().iter().find(|t| t["id"] == "fernwood").unwrap();
    assert_eq!(mine["in_drawing"], true, "{mine}");
    // Applying it again adds no part.
    let applied = run(&mut s, "theme.apply", json!({ "theme": "fernwood" }));
    assert_eq!(applied["theme_added"], false, "{applied}");
    assert_eq!(s.document().unwrap().themes.len(), 1);
}

/// `four()` and a fifth shape, `T`, whose automatic style sets no fill and no line: its look is what its style sheets give it, and so the theme's.
fn four_and_a_plain_shape() -> Vec<u8> {
    let mut drawing = four();
    drawing.auto.push_str(r#"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties/></style:style>"#);
    let shapes = crate::odg_build::rect_on("gr3", "T", (2.0, 13.0, 4.0, 2.0), "");
    drawing.pages = drawing.pages.replace("</draw:page>", &format!("{shapes}</draw:page>"));
    drawing.bytes()
}

fn look_of(s: &mut Session, name: &str) -> serde_json::Value {
    let at = names_of(s.document().unwrap(), 0).iter().position(|n| n == name).unwrap();
    crate::odg_file::look(s, 0, at)
}

fn links_of(doc: &Document) -> Vec<(Option<u32>, Option<u32>, Option<u32>)> {
    doc.pages.get(0).unwrap().shapes.iter().map(|s| (s.sheet.links.line, s.sheet.links.fill, s.sheet.links.text)).collect()
}

/// Item 1: `theme.apply` was refused on save ("a change of the document's own fields, styles or masters"): it adds the theme part (carried by the document file
///), the page's scheme cells (carried), the `Theme` style sheet and the shapes' links to it. The style sheet is an empty common style of `styles.xml` with
/// its sheet in the document file, and each shape that follows it names it as its parent. The drawing a session of its own opens is the one the session holds: the same
/// looks (shape `T` has no fill or line of its own, so its look is the theme's), the same style sheets and links, the same selection, and the same answers to the
/// commands that follow the theme (`theme.apply` again is no change, `theme.variant` gives the look the session that never saved gives).
#[test]
fn theme_apply_is_saved_and_the_drawing_a_session_of_its_own_opens_draws_it_the_same() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &four_and_a_plain_shape());
    let before = look_of(&mut s, "T");
    let applied = run(&mut s, "theme.apply", json!({ "theme": "fernwood", "scope": "all" }));
    assert_eq!(applied["style_added"], true, "{applied}");
    let after = look_of(&mut s, "T");
    assert_ne!(before, after, "the theme changes what T looks like, or this case shows nothing: {before} {after}");
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    let content = part_of(&dir.read("a.odg"), "content.xml");
    let styles = part_of(&dir.read("a.odg"), "styles.xml");
    assert!(styles.contains("style:name=\"Theme\"") && styles.contains("style:family=\"graphic\""), "the style sheet is a common style: {styles}");
    assert!(content.contains("style:parent-style-name=\"Theme\""), "the shapes name it as their parent: {content}");
    let file = part_of(&dir.read("a.odg"), "DiaCraft/document.xml");
    assert!(file.contains("<diacraft:style name=\"Theme\"") && file.contains("THEMEVAL"), "its sheet is in the document file: {file}");
    let mut again = reopened(&dir, "a.odg");
    let (mine, theirs) = (s.document().unwrap().clone(), again.document().unwrap().clone());
    for name in ["A", "B", "C", "D", "T"] {
        assert_eq!(look_of(&mut s, name), look_of(&mut again, name), "{name}: the look after a reopen");
    }
    assert_eq!(links_of(&mine), links_of(&theirs), "the links of the shapes");
    assert_eq!(mine.styles.len(), theirs.styles.len());
    // A style sheet as read holds the `.odg` data of its slot, which the model's has not: the rest is the same.
    let bare = |s: &diacraft_doc::Style| diacraft_doc::Style { ext: Default::default(), ..s.clone() };
    for (a, b) in mine.styles.iter().zip(theirs.styles.iter()) {
        assert_eq!(bare(a), bare(b), "style sheet {}", a.id);
    }
    assert_eq!(mine.themes.len(), theirs.themes.len());
    assert_eq!(run(&mut s, "theme.list", json!({}))["page"], run(&mut again, "theme.list", json!({}))["page"], "the page's selection");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // The same theme again, in the drawing of a session of its own: no change and no undo step.
    let mut again = reopened(&dir, "a.odg");
    let steps = again.history().unwrap().undo_len();
    let once_more = run(&mut again, "theme.apply", json!({ "theme": "fernwood", "scope": "all" }));
    assert_eq!(
        (once_more["theme_added"].clone(), once_more["style_added"].clone(), once_more["changed_pages"].clone()),
        (json!(false), json!(false), json!([])),
        "{once_more}"
    );
    assert_eq!(again.history().unwrap().undo_len(), steps, "no undo step");
    // The variants: the same look in the session that never saved and in the one that opened the file.
    run(&mut s, "theme.variant", json!({ "variant": 1, "scope": "all" }));
    run(&mut again, "theme.variant", json!({ "variant": 1, "scope": "all" }));
    for name in ["A", "T"] {
        assert_eq!(look_of(&mut s, name), look_of(&mut again, name), "{name}: the look of variant 1");
    }
    assert_ne!(look_of(&mut again, "T"), after, "and the variant changed it");
}
