//! A flat OpenDocument drawing (`.fodg`) through the engine. A flat file is read like any other, **Save writes a drawing that
//! came from one back as a flat file, in place**, and Save As does not offer `.fodg` (a drawing that came from one is saved by Save As as a
//! `.diacraft` file, an `.odg` package or a VSDX type, which `odg_flat_saveas.rs` holds). The flat drawings are written by hand from the standard (`Drawing::flat_xml`), beside the package
//! that holds the same elements: what the engine reads of the one it reads of the other. The shapes of the drawings are those of `odg_file.rs`.

use diacraft_doc::Document;
use diacraft_engine::{EngineError, OpenProblem, Session};
use serde_json::{Value, json};

use crate::odg_build::{DRAWING, Dir, Drawing, TEMPLATE, glued, names_of, rect};
use crate::odg_file::{
    assert_reopens_as_saved, assert_saves_again_as_no_edit, edit_cases, four, frame_at, glue, look, refused, save, shift, text_at,
};
use crate::{check_session, run};

/// A drawing of two pages: the four shapes of `four()` on the first, and on the second two shapes with a connector glued to both and a text.
pub(crate) fn two_pages() -> Drawing {
    let ided = |text: String, id: &str| text.replacen("<draw:custom-shape ", &format!("<draw:custom-shape xml:id=\"{id}\" draw:id=\"{id}\" "), 1);
    let first = four();
    let second = [
        ided(rect("S", (2.0, 3.0, 4.0, 2.0), "start"), "id1"),
        ided(rect("T", (12.0, 3.0, 4.0, 2.0), "end"), "id2"),
        glued("K", "id1", "id2", (6.0, 4.0, 12.0, 4.0)),
    ]
    .concat();
    Drawing { pages: format!("{}<draw:page draw:name=\"page2\" draw:master-page-name=\"Default\">{second}</draw:page>", first.pages), ..first }
}

/// A session that has opened `bytes` as `name` in `dir`.
fn opened(dir: &Dir, name: &str, bytes: &[u8]) -> (Session, Value) {
    let path = dir.write(name, bytes);
    let mut s = Session::new();
    let result = run(&mut s, "file.open", json!({ "path": path }));
    (s, result)
}

/// What a person who looks at two drawings sees: the same pages, the same shapes in the same order with the same frames, looks and texts, the same glue, and
/// the same picture of each page.
#[track_caller]
pub(crate) fn assert_same_model(dir: &Dir, a: &mut Session, b: &mut Session, what: &str) {
    let (da, db): (Document, Document) = (a.document().unwrap().clone(), b.document().unwrap().clone());
    assert_eq!(da.pages.len(), db.pages.len(), "{what}: the pages");
    for page in 0..da.pages.len() {
        let (pa, pb) = (da.pages.get(page).unwrap(), db.pages.get(page).unwrap());
        assert_eq!((pa.id, pa.name.to_string()), (pb.id, pb.name.to_string()), "{what}: page {page}");
        assert_eq!(names_of(&da, page), names_of(&db, page), "{what}: the shapes of page {page}");
        for at in 0..pa.shapes.len() {
            assert_eq!(frame_at(&da, page, at), frame_at(&db, page, at), "{what}: the frame of shape {at} of page {page}");
            assert_eq!(look(a, page, at), look(b, page, at), "{what}: the look of shape {at} of page {page}");
            assert_eq!(text_at(a, page, at), text_at(b, page, at), "{what}: the text of shape {at} of page {page}");
        }
        assert_eq!(glue(&da, page), glue(&db, page), "{what}: the glue of page {page}");
        let (ia, ib) = (pa.id, pb.id);
        run(a, "file.export", json!({ "path": dir.path("a.png"), "page": ia }));
        run(b, "file.export", json!({ "path": dir.path("b.png"), "page": ib }));
        assert_eq!(dir.read("a.png"), dir.read("b.png"), "{what}: the picture of page {page}");
    }
    check_session(a).unwrap();
    check_session(b).unwrap();
}

/// A flat drawing's `file.open` reports what a package's does, and says which form it is.
#[test]
fn odg_package_a_flat_drawing_opens_as_the_model_of_the_package_that_holds_the_same_elements() {
    for (what, drawing) in [("four rectangles", four()), ("two pages with a glued connector", two_pages())] {
        let dir = Dir::new();
        let (mut package, from_package) = opened(&dir, "a.odg", &drawing.bytes());
        let (mut flat, from_flat) = opened(&dir, "b.fodg", &drawing.flat_bytes());
        assert_same_model(&dir, &mut package, &mut flat, what);
        for key in ["variant", "kind", "new_from_template"] {
            assert_eq!(from_package["file"][key], from_flat["file"][key], "{what}: file.{key}");
        }
        // A flat drawing says first, at the open, what Save does with it; the rest of what the two files say is the same, word for word.
        let (flat_words, package_words) = (from_flat["file"]["warnings"].as_array().unwrap(), from_package["file"]["warnings"].as_array().unwrap());
        assert_eq!(flat_words.len(), package_words.len() + 1, "{what}: {flat_words:?}");
        let note = flat_words[0].as_str().unwrap();
        assert!(note.contains("flat OpenDocument file (.fodg)") && note.contains("Save writes it back as a flat file"), "{what}: {note}");
        assert!(!note.contains("cannot be saved") && !note.contains("not write it"), "{what}: it no longer says the file cannot be saved: {note}");
        assert_eq!(&flat_words[1..], package_words.as_slice(), "{what}");
        assert_eq!(from_flat["file"]["warning_count"].as_u64().unwrap(), from_package["file"]["warning_count"].as_u64().unwrap() + 1, "{what}");
        assert_eq!((from_package["file"]["variant"].clone(), from_flat["file"]["variant"].clone()), (json!("odg"), json!("odg")));
        assert_eq!(from_package["file"]["form"], "package", "{what}");
        assert_eq!(from_flat["file"]["form"], "flat", "{what}");
        assert_eq!(from_flat["pages"], from_package["pages"], "{what}: the pages of the answer");
        assert_eq!(from_flat["file"]["bytes"], drawing.flat_bytes().len(), "{what}: the size is the file's");
    }
}

/// The same edit of the same shape gives the same drawing whichever form the file was in.
#[test]
fn odg_package_an_edit_of_a_flat_drawing_is_the_edit_of_its_package_twin() {
    for (what, edit) in edit_cases() {
        let dir = Dir::new();
        let (mut package, _) = opened(&dir, "a.odg", &four().bytes());
        let (mut flat, _) = opened(&dir, "b.fodg", &four().flat_bytes());
        let (before_package, before_flat) = (package.document().unwrap().clone(), flat.document().unwrap().clone());
        edit(&mut package, &before_package);
        edit(&mut flat, &before_flat);
        assert_same_model(&dir, &mut package, &mut flat, what);
        assert!(flat.is_dirty() && package.is_dirty(), "{what}: the edit is an edit");
    }
}

/// A flat drawing is told by what it is and not by what it is called (as a package is), and a package that is called `.fodg` is a package.
#[test]
fn odg_package_the_name_of_the_file_does_not_decide_its_form() {
    let dir = Dir::new();
    for name in ["plan.fodg", "PLAN.FODG", "plan.odg", "plan.xml", "plan", "plan.vsdx", "plan.txt"] {
        let (s, result) = opened(&dir, name, &four().flat_bytes());
        assert_eq!((result["file"]["variant"].clone(), result["file"]["form"].clone()), (json!("odg"), json!("flat")), "{name}");
        assert_eq!(names_of(s.document().unwrap(), 0), ["A", "B", "C", "D"], "{name}");
    }
    for name in ["plan.fodg", "plan.odg", "plan"] {
        let (_, result) = opened(&dir, name, &four().bytes());
        assert_eq!((result["file"]["variant"].clone(), result["file"]["form"].clone()), (json!("odg"), json!("package")), "{name}");
    }
}

/// The drawing opened from a flat file is offered the type `.odg` for Save As (the window starts the dialog from it), never `.fodg`.
#[test]
fn odg_package_a_flat_drawing_is_offered_odg_and_not_fodg_for_save_as() {
    let dir = Dir::new();
    let (s, _) = opened(&dir, "plan.fodg", &four().flat_bytes());
    let file = s.source().unwrap();
    assert_eq!(file.extension(), Some("odg"));
    assert!(file.is_named());
    assert!(file.is_flat(), "the file is told to be flat from its bytes");
    let (package, _) = opened(&dir, "plan.fodg", &four().bytes());
    assert!(!package.source().unwrap().is_flat(), "a package that is called .fodg is not flat");
    assert_eq!(package.source().unwrap().extension(), Some("odg"));
}

/// A flat template is not opened (a template is `.otg`'s, and none is opened yet), in words, and the drawing that was open stays.
#[test]
fn odg_package_a_flat_template_is_refused_in_words_and_the_open_drawing_stays() {
    let dir = Dir::new();
    let (mut s, _) = opened(&dir, "keep.odg", &four().bytes());
    shift(&mut s, "A", 1.0, 0.0);
    let before = crate::snapshot(&s);
    let path = dir.write("t.fodg", four().flat_xml(TEMPLATE).as_bytes());
    match s.execute("file.open", &json!({ "path": path })) {
        Err(EngineError::CannotOpen { problem, msg, .. }) => {
            assert_eq!(problem, OpenProblem::NotAPackage);
            assert!(msg.contains("template") && !msg.contains(&path), "{msg}");
        }
        other => panic!("a flat template was not refused as a template: {other:?}"),
    }
    assert_eq!(crate::snapshot(&s), before, "the open drawing is as it was");
}

/// A hostile flat file is refused in words, by the kind of trouble, with nothing of the file in the words, and the drawing that was open stays open as it was.
#[test]
fn odg_package_a_hostile_flat_file_is_refused_in_words_and_the_open_drawing_stays() {
    let secret = "SECRET-WORDS";
    let good = four().flat_xml(DRAWING);
    // A drawing nested far deeper than the container's depth cap, in the page.
    let deep = {
        let opens = "<draw:g>".repeat(5_000);
        let closes = "</draw:g>".repeat(5_000);
        good.replacen(
            "<draw:page draw:name=\"page1\" draw:master-page-name=\"Default\">",
            &format!("<draw:page draw:name=\"page1\" draw:master-page-name=\"Default\">{opens}{closes}"),
            1,
        )
    };
    // A DOCTYPE with an internal entity, which no part of the reader expands.
    let doctype = good.replacen("?>\n", &format!("?>\n<!DOCTYPE office:document [<!ENTITY a \"{secret}\">]>\n"), 1);
    // Another kind of document: a text document in one flat file.
    let other_kind = good.replace(DRAWING, "application/vnd.oasis.opendocument.text");
    // The file cut off inside the page.
    let cut = good.get(..good.find("<draw:custom-shape").unwrap_or(0) + 40).unwrap().to_owned();
    // UTF-16 with a byte order mark, which no sniff tells for a drawing.
    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend(good.encode_utf16().flat_map(u16::to_le_bytes));
    // The first tag of a drawing and then the secret and nothing else.
    let junk = format!(
        "<office:document xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" office:mimetype=\"{DRAWING}\"><{secret} \u{1}\u{2}"
    );
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("a drawing nested past the depth cap", deep.into_bytes()),
        ("a DOCTYPE", doctype.into_bytes()),
        ("another kind of document", other_kind.into_bytes()),
        ("a file cut off", cut.into_bytes()),
        ("UTF-16", utf16),
        ("junk after the root", junk.into_bytes()),
        ("a text that is no XML", format!("{secret} is not a drawing").into_bytes()),
    ];
    for (what, bytes) in cases {
        let dir = Dir::new();
        let (mut s, _) = opened(&dir, "keep.odg", &four().bytes());
        shift(&mut s, "A", 1.0, 0.0);
        let before = crate::snapshot(&s);
        let path = dir.write("bad.fodg", &bytes);
        match s.execute("file.open", &json!({ "path": path })) {
            Err(EngineError::CannotOpen { problem, msg, .. }) => {
                assert!(matches!(problem, OpenProblem::NotAPackage | OpenProblem::TooBig | OpenProblem::Damaged), "{what}: {problem:?}");
                assert!(msg.starts_with("this "), "{what}: words, by the kind of trouble: {msg}");
                assert!(!msg.contains(secret) && !msg.contains(&path), "{what}: the words hold nothing of the file: {msg}");
                assert!(msg.chars().count() < 500, "{what}: {msg}");
            }
            other => panic!("{what}: the file was not refused as a file that cannot be opened: {other:?}"),
        }
        assert_eq!(crate::snapshot(&s), before, "{what}: the open drawing is as it was");
        check_session(&s).unwrap();
    }
}

/// Save of a drawing that came from a flat file writes it back as a flat file, in place, whatever the file is called (the file's own form is kept, as
/// for a `.vsdx` and an `.odg`). Unedited, it is no edit and the bytes are the file's own; and it is saved again, and again, as no edit.
#[test]
fn odg_package_an_unedited_save_of_a_flat_drawing_is_no_edit_and_writes_its_own_bytes() {
    for name in ["plan.fodg", "plan.odg", "plan"] {
        let dir = Dir::new();
        let input = four().flat_bytes();
        let (mut s, _) = opened(&dir, name, &input);
        let saved = save(&mut s);
        assert_eq!(
            (saved["edited"].clone(), saved["form"].clone(), saved["variant"].clone()),
            (json!(false), json!("flat"), json!("odg")),
            "{name}: {saved}"
        );
        assert_eq!(saved["warning_count"], 0, "{name}: the open's note is not a save's: {saved}");
        assert_eq!(saved["dirty"], false, "{name}");
        assert_eq!(dir.read(name), input, "{name}: the file is its own bytes");
        assert_eq!(dir.files(), [name.to_owned()], "{name}: no other file");
        assert_saves_again_as_no_edit(&dir, &mut s, name);
        assert!(s.source().unwrap().is_flat(), "{name}: the drawing is still from a flat file");
        assert_eq!(s.source().unwrap().bytes(), dir.read(name).as_slice(), "{name}: the session holds the file's bytes");
    }
}

/// An edit is spliced into the flat document (what the edit did not touch is the file's own markup, byte for byte), the file stays a flat document,
/// reads back through the real reader as the drawing the session holds, and is saved again as no edit.
#[test]
fn odg_package_an_edit_of_a_flat_drawing_is_saved_into_the_flat_file_and_reads_back() {
    let dir = Dir::new();
    let (mut s, _) = opened(&dir, "plan.fodg", &four().flat_bytes());
    shift(&mut s, "A", 1.0, 0.5);
    assert_eq!(run(&mut s, "file.status", json!({}))["dirty"], true);
    let saved = save(&mut s);
    assert_eq!((saved["edited"].clone(), saved["form"].clone(), saved["dirty"].clone()), (json!(true), json!("flat"), json!(false)), "{saved}");
    let bytes = dir.read("plan.fodg");
    assert_eq!(diacraft_odg::Form::of(&bytes), diacraft_odg::Form::Flat, "the file stays one flat document");
    let text = String::from_utf8(bytes).unwrap();
    assert_eq!(text.matches("<office:document ").count(), 1);
    for (untouched, markup) in [("B", rect("B", (12.0, 3.0, 4.0, 2.0), "")), ("D", rect("D", (12.0, 8.0, 4.0, 2.0), ""))] {
        assert!(text.contains(&markup), "{untouched}: what the edit did not touch is the file's own markup");
    }
    assert!(!text.contains(&rect("A", (2.0, 3.0, 4.0, 2.0), "hello")), "the shape that moved is written anew");
    assert!(text.contains("<meta:generator>DiaCraft/"), "the generator says who saved it");
    assert_reopens_as_saved(&dir, &mut s, "plan.fodg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "plan.fodg");
    // An undo and a redo, and a second edit, are saved as the same.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "file.status", json!({}))["dirty"], true, "the undo takes it back from what is saved");
    shift(&mut s, "B", 0.0, -1.0);
    assert_eq!(save(&mut s)["edited"], true);
    assert_reopens_as_saved(&dir, &mut s, "plan.fodg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "plan.fodg");
}

/// Tested against the package: every kind of edit the engine has, made to a flat drawing and to its package twin and saved, gives files that read
/// as one drawing, and the flat file is still flat.
#[test]
fn odg_package_every_edit_is_saved_into_a_flat_drawing_as_it_is_into_its_package_twin() {
    let mut masters_refused = Vec::new();
    for (what, edit) in edit_cases() {
        let dir = Dir::new();
        let (mut package, _) = opened(&dir, "a.odg", &four().bytes());
        let (mut flat, _) = opened(&dir, "b.fodg", &four().flat_bytes());
        let (before_package, before_flat) = (package.document().unwrap().clone(), flat.document().unwrap().clone());
        edit(&mut package, &before_package);
        edit(&mut flat, &before_flat);
        let saved_package = save(&mut package);
        // A shape dropped from a stencil brings its master into the document, and a flat file has no place for the document's masters:
        // the save is refused in words, in the way of a flat file, and the file is as it was.
        let saved_flat = match flat.execute("file.save", &json!({})) {
            Ok(saved) => saved,
            Err(refused) => {
                let words = refused.to_string();
                assert!(words.contains("masters") && words.contains("edit.undo") && words.contains("saved only as a flat file"), "{what}: {words}");
                assert_eq!(dir.read("b.fodg"), four().flat_bytes(), "{what}: the file is as it was");
                masters_refused.push(what);
                continue;
            }
        };
        assert_eq!(saved_flat["edited"], true, "{what}");
        assert_eq!(saved_package["edited"], true, "{what}");
        assert_ne!(dir.read("b.fodg"), four().flat_bytes(), "{what}: the edit was written");
        assert_eq!(diacraft_odg::Form::of(&dir.read("b.fodg")), diacraft_odg::Form::Flat, "{what}: still a flat document");
        assert_eq!(saved_flat["warnings"], saved_package["warnings"], "{what}: the writer says the same of both");
        assert_reopens_as_saved(&dir, &mut flat, "b.fodg", 0);
        let (mut again_package, _) = opened(&dir, "a2.odg", &dir.read("a.odg"));
        let (mut again_flat, _) = opened(&dir, "b2.fodg", &dir.read("b.fodg"));
        assert_same_model(&dir, &mut again_package, &mut again_flat, what);
        assert_saves_again_as_no_edit(&dir, &mut flat, "b.fodg");
    }
    assert_eq!(
        masters_refused,
        ["a new rectangle", "a new ellipse with a text"],
        "only the drops from a stencil are refused, for the master they bring"
    );
}

/// A file that something else changed since the drawing was opened is not replaced by a save, as for any other file, unless `force` says to.
#[test]
fn odg_package_a_flat_file_that_changed_on_the_disk_is_not_replaced_unless_forced() {
    let dir = Dir::new();
    let (mut s, _) = opened(&dir, "plan.fodg", &four().flat_bytes());
    shift(&mut s, "A", 1.0, 0.0);
    let other = four().page(&rect("Z", (1.0, 1.0, 1.0, 1.0), "")).flat_bytes();
    dir.write("plan.fodg", &other);
    match s.execute("file.save", &json!({})) {
        Err(EngineError::ChangedOnDisk { .. }) => {}
        other => panic!("a file changed on the disk was replaced: {other:?}"),
    }
    assert_eq!(dir.read("plan.fodg"), four().page(&rect("Z", (1.0, 1.0, 1.0, 1.0), "")).flat_bytes());
    assert_eq!(run(&mut s, "file.save", json!({ "force": true }))["edited"], true);
    assert_eq!(diacraft_odg::Form::of(&dir.read("plan.fodg")), diacraft_odg::Form::Flat);
}

/// A change the flat writer refuses (a page added) ends the save in words, with the way out said for a flat file, and the file and the drawing as
/// they were: the person takes it back and saves.
#[test]
fn odg_package_a_change_the_flat_writer_refuses_is_refused_in_words_with_the_way_out_of_a_flat_file() {
    let dir = Dir::new();
    let input = four().flat_bytes();
    let (mut s, _) = opened(&dir, "plan.fodg", &input);
    run(&mut s, "page.add", json!({}));
    let dirty = crate::snapshot(&s);
    let words = refused(&mut s);
    assert!(words.contains("page") && words.contains("edit.undo"), "{words}");
    assert!(
        words.contains("opened from a flat file is saved only as a flat file") && !words.contains(".odg"),
        "the way out is a flat file's: {words}"
    );
    assert_eq!(dir.read("plan.fodg"), input, "the file is as it was");
    assert_eq!(crate::snapshot(&s), dirty, "and the session");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(save(&mut s)["edited"], false, "taken back, the save is the file's own bytes");
}

/// Keeps `.fodg` out of Save As, for a drawing that came from a flat file as for any other (lets it be saved as a `.diacraft` file, an `.odg`
/// package and a VSDX type, `odg_flat_saveas.rs`): a Save As to a `.fodg` name is refused in words that point to the `.odg` package, whatever the flat file is called, nothing is
/// written and the drawing stays as it was; Save, the file's own, writes the flat file.
#[test]
fn odg_package_save_as_fodg_of_a_flat_drawing_is_refused_in_words_and_writes_nothing() {
    for (what, name) in [("a .fodg", "plan.fodg"), ("a flat file with an .odg name", "plan.odg"), ("a flat file with no extension", "plan")] {
        let dir = Dir::new();
        let (mut s, _) = opened(&dir, name, &four().flat_bytes());
        let on_disk = dir.read(name);
        shift(&mut s, "A", 1.0, 0.0);
        let dirty = crate::snapshot(&s);
        for target in ["other.fodg", "OTHER.FODG"] {
            for force in [false, true] {
                let words = match s.execute("file.save_as", &json!({ "path": dir.path(target), "force": force })) {
                    Err(EngineError::BadParams { cmd, msg }) => {
                        assert_eq!(cmd, "file.save_as", "{what}");
                        msg
                    }
                    other => panic!("{what}: file.save_as {target} was not refused: {other:?}"),
                };
                for part in ["flat", ".fodg", "not written by Save As", "Nothing was written", "saved by Save, as a flat file", "as an .odg"] {
                    assert!(words.contains(part), "{what}: file.save_as {target} says {part}: {words}");
                }
                assert!(words.chars().count() < 300, "{what}: {words}");
            }
        }
        assert_eq!(dir.read(name), on_disk, "{what}: the file is as it was");
        assert_eq!(dir.files(), [name.to_owned()], "{what}: no other file was made");
        assert_eq!(crate::snapshot(&s), dirty, "{what}: the drawing and the session are as they were");
        assert_eq!(run(&mut s, "file.status", json!({}))["dirty"], true, "{what}: the changes are still not saved");
        // The way: Save writes the file's own form.
        assert_eq!(save(&mut s)["edited"], true, "{what}");
        assert_eq!(diacraft_odg::Form::of(&dir.read(name)), diacraft_odg::Form::Flat, "{what}");
    }
}

/// A drawing that was not opened from a flat file is not saved as one either: Save As does not offer `.fodg`, and says so for a drawing of every other kind.
#[test]
fn odg_package_a_save_as_fodg_is_refused_in_words_for_every_other_drawing() {
    let dir = Dir::new();
    let (mut package, _) = opened(&dir, "a.odg", &four().bytes());
    let mut blank = Session::new();
    run(&mut blank, "file.new", json!({}));
    for (what, s) in [("an .odg", &mut package), ("a new drawing", &mut blank)] {
        shift_if_odg(s);
        let before = crate::snapshot(s);
        for target in ["x.fodg", "X.FODG"] {
            let path = dir.path(target);
            let error = s.execute("file.save_as", &json!({ "path": path })).expect_err("Save As does not offer .fodg");
            let words = error.to_string();
            for part in ["flat", ".fodg", "not written by Save As", "Nothing was written", "saved by Save, as a flat file", "as an .odg"] {
                assert!(words.contains(part), "{what}: {target} says {part}: {words}");
            }
            // Made a flat file writable by Save: the words never say that flat files are read and not written.
            for gone in ["does not write", "read only", "never written", "reads such files"] {
                assert!(!words.contains(gone), "{what}: {target} says {gone}: {words}");
            }
            assert!(!std::path::Path::new(&path).exists(), "{what}: nothing was written for {target}");
        }
        assert_eq!(crate::snapshot(s), before, "{what}: the drawing is as it was");
    }
    assert!(!dir.files().iter().any(|f| f.to_lowercase().ends_with(".fodg")), "{:?}", dir.files());
}

/// An edit that makes the `.odg` drawing differ, so a refusal is not mistaken for a drawing that has nothing to save.
fn shift_if_odg(s: &mut Session) {
    if s.document().is_some_and(|d| d.pages.get(0).is_some_and(|p| p.shapes.iter().any(|x| x.name_u.as_deref() == Some("A")))) {
        shift(s, "A", 1.0, 0.0);
    }
}

/// A package that a person called `.fodg` is a package: it opens, and it is saved under the name of an `.odg`, and the words for the name say so.
#[test]
fn odg_package_a_package_called_fodg_is_saved_as_an_odg_and_the_name_says_what_to_do() {
    let dir = Dir::new();
    let (mut s, _) = opened(&dir, "plan.fodg", &four().bytes());
    shift(&mut s, "A", 1.0, 0.0);
    let words = refused(&mut s);
    assert!(words.contains("must end in .odg") && words.contains("file.save_as"), "{words}");
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("plan.odg") }));
    assert_eq!((saved["variant"].clone(), saved["edited"].clone()), (json!("odg"), json!(true)));
    let bytes = dir.read("plan.odg");
    assert_eq!(diacraft_odg::Form::of(&bytes), diacraft_odg::Form::Package, "an .odg that was saved is a package");
}

/// A shape an undo put back, with a connector of the file glued to it, is saved in a flat file as in a package (the flattening used to refuse it, with the way out of a
/// flat file or of an `.odg`, a redo): the shapes and the glue are written, and the file reads back as the session holds it. (The refusal of the flattening that is left, the copy
/// of an object, says the way out of a flat file in the test after.)
#[test]
fn odg_package_a_shape_an_undo_put_back_is_saved_with_its_glue_in_both_forms() {
    let ided = |text: String, id: &str| text.replacen("<draw:custom-shape ", &format!("<draw:custom-shape xml:id=\"{id}\" draw:id=\"{id}\" "), 1);
    let shapes = [
        ided(rect("A", (2.0, 3.0, 4.0, 2.0), ""), "id1"),
        ided(rect("B", (12.0, 3.0, 4.0, 2.0), ""), "id2"),
        glued("K", "id1", "id2", (6.0, 4.0, 12.0, 4.0)),
    ]
    .concat();
    let drawing = Drawing::new().page(&shapes);
    for (name, bytes) in [("plan.fodg", drawing.flat_bytes()), ("plan.odg", drawing.bytes())] {
        let dir = Dir::new();
        let (mut s, _) = opened(&dir, name, &bytes);
        let (a, b) = (crate::odg_build::id_of(s.document().unwrap(), 0, "A"), crate::odg_build::id_of(s.document().unwrap(), 0, "B"));
        run(&mut s, "shape.delete", json!({ "ids": [a, b] }));
        assert_eq!(save(&mut s)["edited"], true, "{name}");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(save(&mut s)["edited"], true, "{name}: the shapes and the glue come back");
        crate::odg_file::assert_reopens_as_saved(&dir, &mut s, name, 0);
        assert_eq!(s.document().unwrap().pages.get(0).unwrap().connects.len(), 2, "{name}: the connector is glued at both ends");
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(save(&mut s)["edited"], true, "{name}");
        crate::odg_file::assert_reopens_as_saved(&dir, &mut s, name, 0);
    }
}

/// The copy of an object that has a picture, in a flat file (the object inline): refused in words by the flattening, with the undo as the way
/// out, in the words of a flat file; nothing is written, and the undone drawing saves.
#[test]
fn odg_package_the_copy_of_an_inline_object_in_a_flat_drawing_is_refused_with_the_way_out_of_a_flat_file() {
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&[200u8; 16]).unwrap();
    // Base64 by RFC 4648, in lines of 72 characters.
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in png.chunks(3) {
        let n = u32::from_be_bytes([0, chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)]);
        for k in 0..4usize {
            text.push(if k <= chunk.len() { char::from(alphabet[((n >> (18 - 6 * k)) & 63) as usize]) } else { '=' });
        }
    }
    let wrapped: Vec<&str> = text.as_bytes().chunks(72).map(|c| std::str::from_utf8(c).unwrap()).collect();
    let frame = format!(
        "<draw:frame draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"O\" svg:width=\"4cm\" svg:height=\"3cm\" svg:x=\"2cm\" svg:y=\"9cm\"><draw:object><office:document office:mimetype=\"{DRAWING}\" office:version=\"1.3\"><office:body><office:drawing/></office:body></office:document></draw:object><draw:image draw:mime-type=\"image/png\"><office:binary-data>{}</office:binary-data></draw:image></draw:frame>",
        wrapped.join("\n")
    );
    let dir = Dir::new();
    let input = Drawing::new().page(&format!("{}{frame}", rect("A", (2.0, 3.0, 4.0, 2.0), ""))).flat_bytes();
    let (mut s, _) = opened(&dir, "plan.fodg", &input);
    let o = crate::odg_build::id_of(s.document().unwrap(), 0, "O");
    run(&mut s, "shape.duplicate", json!({ "ids": [o] }));
    let said = refused(&mut s);
    assert!(said.contains("an object") && said.contains("not copied"), "{said}");
    assert!(
        said.contains("Undo the change (edit.undo) to save")
            && said.contains("a drawing opened from a flat file is saved only as a flat file by Save"),
        "{said}"
    );
    assert!(said.contains("use Save As to save the drawing as a .diacraft file"), "the other way out, which a flat drawing has: {said}");
    assert!(!said.contains(".odg"), "{said}");
    assert_eq!(dir.read("plan.fodg"), input, "nothing was written");
    run(&mut s, "edit.undo", json!({}));
    shift(&mut s, "A", 1.0, 0.0);
    assert_eq!(save(&mut s)["edited"], true);
    assert_eq!(diacraft_odg::Form::of(&dir.read("plan.fodg")), diacraft_odg::Form::Flat);
}

/// A flat file that makes the reader warn more than the open lists: the open lists 20, **the note of what Save does with a flat file first** and the reader's after it,
/// where it listed 21 (the review); the count is the whole.
#[test]
fn odg_package_an_open_lists_twenty_warnings_with_the_note_of_the_flat_file_first() {
    // A graphic style named twice in the one container warns once for the name: 30 names, 30 warnings.
    let styles: String = (0..30).map(|n| format!("<style:style style:name=\"dup{n}\" style:family=\"graphic\"/>")).collect::<String>().repeat(2);
    let mut drawing = Drawing::new().page(&rect("A", (2.0, 3.0, 4.0, 2.0), ""));
    drawing.auto.push_str(&styles);
    let dir = Dir::new();
    let (_, result) = opened(&dir, "many.fodg", &drawing.flat_bytes());
    let words = result["file"]["warnings"].as_array().unwrap();
    assert_eq!(words.len(), 20, "{words:?}");
    assert!(words[0].as_str().unwrap().contains("flat OpenDocument file (.fodg)"), "the note comes first: {words:?}");
    assert!(words[1..].iter().all(|w| w.as_str().unwrap().contains("dup")), "and the reader's follow it: {words:?}");
    assert_eq!(result["file"]["warning_count"], 31, "the count is the whole: the note and the reader's 30: {result}");
    // The same drawing as a package, with no note: the reader's 20, as before.
    let (_, as_package) = opened(&dir, "many.odg", &drawing.bytes());
    assert_eq!(as_package["file"]["warnings"].as_array().unwrap().len(), 20, "{as_package}");
}

/// What `engine.commands` says of a command now.
fn listed(s: &Session, id: &str) -> diacraft_engine::CommandInfo {
    s.commands().into_iter().find(|c| c.id == id).unwrap_or_else(|| panic!("no command {id}"))
}

/// `file.save_as` is enabled for a drawing that came from a flat file, as for every other drawing (item 5 had its `enabled` say no, with the
/// reason of the window's note), and says "no document open" for none; Save and Export are as usable as ever.
#[test]
fn odg_package_file_save_as_is_enabled_for_a_flat_drawing_as_for_any_other() {
    let dir = Dir::new();
    let mut none = Session::new();
    let (info, none_info) = (listed(&none, "file.save_as"), listed(&none, "file.save"));
    assert_eq!((info.enabled, info.disabled_reason.as_deref()), (false, Some("no document open")), "no document: as before");
    assert_eq!((none_info.enabled, none_info.disabled_reason.as_deref()), (false, Some("no document open")));
    run(&mut none, "file.new", json!({}));
    assert_eq!(listed(&none, "file.save_as").disabled_reason, None, "a new drawing has Save As");
    let (package, _) = opened(&dir, "a.odg", &four().bytes());
    assert_eq!(listed(&package, "file.save_as").disabled_reason, None, "an .odg has Save As");
    for name in ["plan.fodg", "plan.odg", "plan"] {
        let (flat, _) = opened(&dir, name, &four().flat_bytes());
        for command in ["file.save_as", "file.save", "file.export"] {
            let as_listed = listed(&flat, command);
            assert!(as_listed.enabled && as_listed.disabled_reason.is_none(), "{name}: {command}: {as_listed:?}");
        }
    }
}
