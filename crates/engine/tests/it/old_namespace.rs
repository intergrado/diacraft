//! APPID: a drawing that a build made before the id moved to `diacraft.app` wrote (DiaCraft's own namespace is `urn:io.github.one_bit.diacraft:extension` in it) opens as it did in every format,
//! is written in the current namespace only by every save that writes the file from the model, and costs the check after a save nothing it did not cost before. The five files are the ones `main`
//! at 3f696318 wrote (`diacraft_testkit::old_namespace`), not files this code built by hand; the "current" file of each is the same parts with the namespace put in its current form.
//!
//! A save into the file a drawing was opened from (a splice) writes what an edit changed and keeps the rest as the file had it, so what it keeps stays in the namespace it was written in: **a
//! drawing that carries both reads as one**, and the tests that follow an edit hold that (no page, shape or setting twice).

use diacraft_container::zip::Archive;
use diacraft_container::{Limits, Work};
use diacraft_doc::DIACRAFT_URN;
use diacraft_engine::Session;
use diacraft_testkit::old_namespace::{DRAWING_DIACRAFT, DRAWING_ODG, DRAWING_VSDX, NAMES, ODG_AS_VSDX, OLD_NAMESPACE, VSDX_AS_ODG, file, parts};
use serde_json::{Value, json};

use crate::native_common::{Dir, opened};
use crate::run;

/// The file `name` with the old namespace put in its current form in every part that holds it.
fn current(name: &str) -> Vec<u8> {
    let mut archive = diacraft_container::zip::ArchiveWriter::new();
    for (entry, bytes) in parts(name) {
        let method = if entry == "mimetype" { diacraft_container::zip::Method::Stored } else { diacraft_container::zip::Method::Deflated };
        let bytes = match String::from_utf8(bytes) {
            Ok(text) => text.replace(OLD_NAMESPACE, DIACRAFT_URN).into_bytes(),
            Err(bytes) => bytes.into_bytes(),
        };
        archive.add(&entry, &bytes, method).unwrap();
    }
    archive.finish().unwrap()
}

/// The text of every part of the archive in `bytes`, with its name (a picture reads as lossy text, which holds no namespace).
pub fn texts(bytes: &[u8]) -> Vec<(String, String)> {
    let mut archive = Archive::read(bytes, &Limits::default()).unwrap();
    let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
    names
        .into_iter()
        .enumerate()
        .map(|(at, n)| (n, String::from_utf8_lossy(&archive.inflate(at, &mut Work::default()).unwrap()).into_owned()))
        .collect()
}

/// The parts of the archive that name the old namespace, by name.
fn parts_with_the_old_namespace(bytes: &[u8]) -> Vec<String> {
    texts(bytes).into_iter().filter(|(_, t)| t.contains("one_bit")).map(|(n, _)| n).collect()
}

/// A session that has opened `bytes` as `file` in `dir`.
fn open_bytes(dir: &Dir, file: &str, bytes: &[u8]) -> Session {
    opened(&dir.write(file, bytes))
}

/// The extension a drawing of this fixture is saved under to be in the same format as it.
fn own_extension(name: &str) -> &'static str {
    if name.ends_with(".diacraft") {
        "diacraft"
    } else if name.ends_with(".vsdx") {
        "vsdx"
    } else {
        "odg"
    }
}

/// What the open drawing holds, as a value that two sessions can be told apart by: the pages (id, name) with their shapes (id, where each is), and DiaCraft's own Snap and Glue settings.
fn drawing_of(s: &mut Session) -> Value {
    let pages: Vec<Value> = s
        .document()
        .unwrap()
        .pages
        .iter()
        .map(|page| {
            let shapes: Vec<Value> = page
                .shapes
                .iter()
                .map(|shape| {
                    let at = |cell: &str| shape.sheet.cell(cell).and_then(|c| c.value.as_ref().map(|v| v.text().to_owned()));
                    json!({ "id": shape.id, "pin": [at("PinX"), at("PinY")] })
                })
                .collect();
            json!({ "id": page.id, "name": &*page.name, "shapes": shapes })
        })
        .collect();
    json!({ "pages": pages, "snap": run(s, "snap.settings", json!({})) })
}

#[test]
fn the_five_files_are_what_a_build_before_the_move_wrote() {
    for name in NAMES {
        let held = parts_with_the_old_namespace(&file(name));
        assert!(!held.is_empty(), "{name} holds the old namespace somewhere");
        assert!(texts(&file(name)).iter().all(|(_, t)| !t.contains("urn:app.diacraft")), "{name} holds no current namespace");
        assert!(parts_with_the_old_namespace(&current(name)).is_empty(), "{name} in its current form holds no old one");
    }
}

#[test]
fn every_file_opens_as_the_drawing_of_its_current_form() {
    let dir = Dir::new();
    for name in NAMES {
        let ext = own_extension(name);
        let mut before = open_bytes(&dir, &format!("old.{ext}"), &file(name));
        let mut after = open_bytes(&dir, &format!("new.{ext}"), &current(name));
        assert_eq!(drawing_of(&mut before), drawing_of(&mut after), "{name}: the drawing, whichever namespace the file says");
        let (a, b) = (before.document().unwrap(), after.document().unwrap());
        assert!(a == b, "{name}: the model of the one is the model of the other");
        assert_eq!(a.pages.len(), 2, "{name}");
        assert_eq!(
            run(&mut before, "snap.settings", json!({}))["strength_grid"],
            json!(40),
            "{name}: DiaCraft's own Snap and Glue setting comes with the file"
        );
    }
}

#[test]
fn a_save_from_the_model_into_another_format_writes_the_current_namespace_only_and_the_check_says_nothing_it_did_not_say_before() {
    let dir = Dir::new();
    let mut crossings = 0;
    for name in NAMES {
        let own = own_extension(name);
        for target in ["diacraft", "vsdx", "odg"] {
            if target == own {
                continue;
            }
            let mut before = open_bytes(&dir, &format!("old.{own}"), &file(name));
            let mut after = open_bytes(&dir, &format!("new.{own}"), &current(name));
            let out_old = dir.path(&format!("from-old.{target}"));
            let out_new = dir.path(&format!("from-new.{target}"));
            let saved_old = run(&mut before, "file.save_as", json!({ "path": out_old }));
            let saved_new = run(&mut after, "file.save_as", json!({ "path": out_new }));
            let bytes_old = dir.read(&format!("from-old.{target}"));
            assert!(
                parts_with_the_old_namespace(&bytes_old).is_empty(),
                "{name} as {target}: the old namespace is written nowhere: {:?}",
                parts_with_the_old_namespace(&bytes_old)
            );
            assert!(
                texts(&bytes_old).iter().any(|(_, t)| t.contains(DIACRAFT_URN)) || target == "vsdx",
                "{name} as {target}: the file holds DiaCraft's own markup"
            );
            // The check after the save: it finished, it names nothing, and it says what it said of the same drawing in the current namespace.
            assert_eq!(
                saved_old["checked"], saved_new["checked"],
                "{name} as {target}: a file of before costs the check nothing a file of now does not"
            );
            assert_eq!(saved_old["checked"]["done"], json!(true), "{name} as {target}: {}", saved_old["checked"]);
            // The check names nothing: nothing that is DiaCraft's own data, and no font (the one row of a new drawing's font table, which no cell names and no `.odg` declares, is no
            // missing font).
            let named: Vec<&str> = saved_old["checked"]["classes"].as_array().unwrap().iter().filter_map(|c| c["class"].as_str()).collect();
            assert!(named.is_empty(), "{name} as {target}: {named:?}");
            // And what was written is the drawing again.
            let mut again = opened(&out_old);
            assert_eq!(drawing_of(&mut again), drawing_of(&mut before), "{name} as {target}");
            crossings += 1;
        }
    }
    assert_eq!(crossings, 10, "five files, two other formats each");
}

#[test]
fn a_save_in_the_files_own_format_after_an_edit_of_the_data_writes_it_in_the_current_namespace_and_the_drawing_reads_as_one() {
    let dir = Dir::new();
    for name in NAMES {
        let ext = own_extension(name);
        let mut s = open_bytes(&dir, &format!("old.{ext}"), &file(name));
        let before = drawing_of(&mut s);
        run(&mut s, "snap.set", json!({ "strength_grid": 41 }));
        let out = dir.path(&format!("edited.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": out }));
        let written = dir.read(&format!("edited.{ext}"));
        // What the save wrote is in the current namespace: DiaCraft's data of the format (the solution part of a VSDX file, the document file of an `.odg`), or the whole file (`.diacraft`).
        let current_holders: Vec<String> = texts(&written).into_iter().filter(|(_, t)| t.contains(DIACRAFT_URN)).map(|(n, _)| n).collect();
        assert!(!current_holders.is_empty(), "{name}: the save wrote something of its own in the current namespace");
        // Reopened, the drawing is the one that was edited: each page and shape once, the setting the edit made, nothing of the old data beside it.
        let mut again = opened(&out);
        let mut expected = before.clone();
        expected["snap"] = run(&mut again, "snap.settings", json!({}));
        assert_eq!(expected["snap"]["strength_grid"], json!(41), "{name}: the setting the edit made");
        assert_eq!(drawing_of(&mut again), expected, "{name}: the same pages and shapes, each once");
        // A VSDX file's solution part holds one `Data` and no old namespace in it or in its relationships; a `.diacraft` file holds none anywhere that it wrote again.
        if ext == "vsdx" {
            assert!(parts_with_the_old_namespace(&written).is_empty(), "{name}: {:?}", parts_with_the_old_namespace(&written));
        }
    }
}

/// The ids of the shapes of each page of a drawing as `drawing_of` says it, page by page.
fn shape_ids(drawing: &Value) -> Vec<Vec<Value>> {
    drawing["pages"].as_array().unwrap().iter().map(|p| p["shapes"].as_array().unwrap().iter().map(|s| s["id"].clone()).collect()).collect()
}

#[test]
fn an_odg_file_of_the_old_namespace_that_an_edit_touches_keeps_the_rest_and_still_reads_as_one_drawing() {
    let dir = Dir::new();
    // Each edit, the shapes it adds (or takes), and whether the save writes DiaCraft's own markup of the file again, so that the file then holds both namespaces.
    let edits: [(&str, Value, i64, bool); 5] = [
        ("shape.move", json!({ "ids": [1], "dx": 0.5, "dy": 0.25 }), 0, false),
        ("shape.format", json!({ "ids": [1], "fill": "#ff0000" }), 0, false),
        ("shape.set_text", json!({ "id": 1, "text": "Changed" }), 0, false),
        ("shape.drop", json!({ "master": "circle", "page": 1, "x": 2.0, "y": 2.0 }), 1, true),
        ("page.rename", json!({ "id": 1, "name": "Renamed" }), 0, true),
    ];
    for name in [DRAWING_ODG, VSDX_AS_ODG] {
        for (command, params, added, both) in &edits {
            let mut s = open_bytes(&dir, "old.odg", &file(name));
            let before = drawing_of(&mut s);
            run(&mut s, command, params.clone());
            let out = dir.path("edited.odg");
            run(&mut s, "file.save_as", json!({ "path": out }));
            let written = dir.read("edited.odg");
            let (mut old, mut new) = (0, 0);
            for (_, text) in texts(&written) {
                old += text.matches("one_bit").count();
                new += text.matches(DIACRAFT_URN).count();
            }
            assert!(old > 0 || *both, "{name} {command}: what the edit did not touch is as the file had it");
            if *both {
                assert!(old > 0 && new > 0, "{name} {command}: the file carries both namespaces now (old {old}, new {new})");
            }
            // Reopened, it is one drawing: each page once, each shape once, the shapes the edit added or took, and DiaCraft's own settings as they were.
            let mut again = opened(&out);
            let after = drawing_of(&mut again);
            assert_eq!(after["snap"], before["snap"], "{name} {command}: DiaCraft's own settings");
            let (ids_before, ids_after) = (shape_ids(&before), shape_ids(&after));
            assert_eq!(ids_after.len(), ids_before.len(), "{name} {command}: no page twice, none gone");
            let shapes = |ids: &[Vec<Value>]| ids.iter().map(Vec::len).sum::<usize>() as i64;
            assert_eq!(shapes(&ids_after), shapes(&ids_before) + added, "{name} {command}");
            for page in &ids_after {
                let mut sorted: Vec<String> = page.iter().map(Value::to_string).collect();
                sorted.sort();
                sorted.dedup();
                assert_eq!(sorted.len(), page.len(), "{name} {command}: a shape twice on a page: {page:?}");
            }
            if *command == "shape.move" {
                assert_ne!(before["pages"][0]["shapes"][0]["pin"], after["pages"][0]["shapes"][0]["pin"], "{name}: the edit is in the file");
                assert_eq!(before["pages"][0]["shapes"][1], after["pages"][0]["shapes"][1], "{name}: a shape the edit did not touch is where it was");
            }
            // A second edit of the file the first made, which holds both namespaces, is saved and reads as one drawing again.
            run(&mut again, "shape.move", json!({ "ids": [1], "dx": 0.25, "dy": 0.0 }));
            run(&mut again, "file.save_as", json!({ "path": dir.path("twice.odg") }));
            let mut third = opened(&dir.path("twice.odg"));
            assert_eq!(shape_ids(&drawing_of(&mut third)), ids_after, "{name} {command}: the second save");
        }
    }
}

#[test]
fn a_save_of_a_vsdx_file_of_the_old_namespace_that_changed_nothing_of_the_data_writes_the_part_as_it_read_it_but_for_the_declaration_and_the_file_reads_as_before()
 {
    let dir = Dir::new();
    for name in [DRAWING_VSDX, ODG_AS_VSDX] {
        let mut s = open_bytes(&dir, "old.vsdx", &file(name));
        let before = drawing_of(&mut s);
        run(&mut s, "file.save_as", json!({ "path": dir.path("same.vsdx") }));
        let written = dir.read("same.vsdx");
        // A Save As writes the file again, so the part of a build before 0.2.4, which starts with an XML declaration, is written without it; nothing else of it changes, and the namespace stays the
        // old one (the data did not change).
        let part_of = |bytes: &[u8]| texts(bytes).into_iter().find(|(n, _)| n.ends_with("solution1.xml")).map(|(_, t)| t);
        let read = part_of(&file(name)).map(|text| match text.split_once("?>\r\n") {
            Some((head, rest)) if head.starts_with("<?xml") => rest.to_owned(),
            _ => text,
        });
        assert_eq!(part_of(&written), read, "{name}: the part is the file's, with no declaration");
        let mut again = opened(&dir.path("same.vsdx"));
        assert_eq!(drawing_of(&mut again), before, "{name}");
    }
}

#[test]
fn the_names_of_the_two_vsdx_fixtures_and_the_other_three_are_those_the_tests_ask_for() {
    assert_eq!((own_extension(DRAWING_DIACRAFT), own_extension(DRAWING_VSDX), own_extension(DRAWING_ODG)), ("diacraft", "vsdx", "odg"));
    assert_eq!((own_extension(ODG_AS_VSDX), own_extension(VSDX_AS_ODG)), ("vsdx", "odg"));
}

/// A `.diacraft` file as a build before the move wrote it, with the picture of its first page: the drawing is made and saved by this engine, and every XML part of the file is then put in
/// the old namespace and nothing else (the picture is stored, the parts deflated, the headers are those of the writer).
fn an_old_file_with_a_preview(dir: &Dir, name: &str) -> String {
    let mut s = crate::blank_session();
    run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 1, "x": 2.0, "y": 3.0 }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("made.diacraft") }));
    let mut archive = diacraft_container::zip::ArchiveWriter::new();
    for (entry, bytes) in texts_and_bytes(&dir.read("made.diacraft")) {
        let method = if entry == "mimetype" || entry == "preview.png" {
            diacraft_container::zip::Method::Stored
        } else {
            diacraft_container::zip::Method::Deflated
        };
        let bytes = if entry.ends_with(".xml") { String::from_utf8(bytes).unwrap().replace(DIACRAFT_URN, OLD_NAMESPACE).into_bytes() } else { bytes };
        archive.add(&entry, &bytes, method).unwrap();
    }
    dir.write(name, &archive.finish().unwrap())
}

/// The entries of an archive, each with its inflated bytes.
fn texts_and_bytes(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = Archive::read(bytes, &Limits::default()).unwrap();
    let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
    names.into_iter().enumerate().map(|(at, n)| (n, archive.inflate(at, &mut Work::default()).unwrap())).collect()
}

#[test]
fn a_save_as_a_save_and_a_convert_of_an_old_diacraft_file_that_changed_nothing_keep_its_preview() {
    let dir = Dir::new();
    let path = an_old_file_with_a_preview(&dir, "old.diacraft");
    let held = texts_and_bytes(&dir.read("old.diacraft"));
    let picture = held.iter().find(|(n, _)| n == "preview.png").map(|(_, b)| b.clone()).expect("the file holds its picture");
    assert!(
        held.iter().filter(|(n, _)| n.ends_with(".xml")).all(|(_, b)| !String::from_utf8_lossy(b).contains(DIACRAFT_URN)),
        "every XML part is in the old namespace"
    );
    let before = drawing_of(&mut opened(&path));
    for (how, command, params) in [("a Save As", "file.save_as", json!({ "path": dir.path("as.diacraft") })), ("a Save", "file.save", json!({}))] {
        let mut s = opened(&path);
        run(&mut s, command, params);
        let out = if command == "file.save" { "old.diacraft" } else { "as.diacraft" };
        let written = texts_and_bytes(&dir.read(out));
        let kept = written.iter().find(|(n, _)| n == "preview.png").map(|(_, b)| b);
        assert!(
            kept.is_some(),
            "{how} of an unedited file keeps the picture it had: the file holds {:?}",
            written.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
        );
        assert!(kept == Some(&picture), "{how}: the picture it had, and no other");
        assert!(
            written.iter().filter(|(n, _)| n == "manifest.xml" || n == "document.xml").all(|(_, b)| !String::from_utf8_lossy(b).contains("one_bit")),
            "{how}: the current namespace"
        );
        // The file as it is written now is read as the drawing it was.
        assert_eq!(drawing_of(&mut opened(&dir.path(out))), before, "{how}");
    }
}
