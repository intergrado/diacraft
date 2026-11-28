//! `file.save`, `file.save_as` and the unsaved state: a drawing is written whole beside its file and renamed over it, so a failed save
//! leaves the old file; what an edit did not touch is the file's own; the drawing is unsaved from the first edit to the next save, and undo
//! back to what was saved makes it clean again.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_engine::{EngineError, Session};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder, ZipItem, shape, zip};
use serde_json::json;

use crate::{blank_session, check_session, run, snapshot};

/// A scratch folder under `TMPDIR`, removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-save-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        // Put back the permissions a test took away, so that the folder can go.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    pkg.parts().map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect()
}

fn drawing() -> Vec<u8> {
    VsdxBuilder::new()
        .page(PageSpec::new(1, "First").shapes(&shape(5, r#"<Cell N="Width" V="2.50"/>"#)))
        .page(PageSpec::new(2, "Second"))
        .with(Pkg::new().part_without_override("/foo/bar.dat", b"0123456789").default_type("dat", "application/x-foo"))
        .build()
}

fn status(s: &mut Session) -> serde_json::Value {
    run(s, "file.status", json!({}))
}

#[test]
fn a_new_drawing_is_clean_until_it_is_edited_and_saves_as_a_file_that_opens() {
    let dir = Dir::new();
    let mut s = blank_session();
    assert!(!s.is_dirty() && s.path().is_none(), "a drawing from file.new is not unsaved until it is edited");
    assert_eq!(status(&mut s), json!({"dirty": false, "has_file": false, "format": null, "variant": null, "kind": null}));
    run(&mut s, "page.add", json!({"name": "Second"}));
    assert!(s.is_dirty());
    let path = dir.path("new.vsdx");
    let result = run(&mut s, "file.save_as", json!({"path": path}));
    assert_eq!(result["variant"], "vsdx");
    assert_eq!(result["dirty"], false);
    assert!(result["bytes"].as_u64().unwrap() > 1000);
    assert!(!s.is_dirty());
    assert_eq!(s.path().unwrap().to_str().unwrap(), path);
    assert_eq!(status(&mut s), json!({"dirty": false, "has_file": true, "format": "vsdx", "variant": "vsdx", "kind": "drawing"}));
    assert_eq!(dir.files(), ["new.vsdx"], "no temporary file is left");
    // It opens, with the same pages.
    let mut other = Session::new();
    let opened = run(&mut other, "file.open", json!({"path": path}));
    assert_eq!(opened["pages"], json!([{"id": 1, "name": "Page-1"}, {"id": 2, "name": "Second"}]));
    assert_eq!(opened["file"]["warning_count"], 0, "{opened}");
    assert!(!other.is_dirty());
}

#[test]
fn the_drawing_is_unsaved_from_the_first_edit_to_the_next_save_and_undo_back_to_the_saved_state_is_clean() {
    let dir = Dir::new();
    let mut s = blank_session();
    let path = dir.path("a.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    assert!(!s.is_dirty());
    run(&mut s, "page.add", json!({}));
    assert!(s.is_dirty());
    run(&mut s, "edit.undo", json!({}));
    assert!(!s.is_dirty(), "undo to the saved document is clean");
    run(&mut s, "edit.redo", json!({}));
    assert!(s.is_dirty());
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["dirty"], false);
    assert!(!s.is_dirty());
    run(&mut s, "edit.undo", json!({}));
    assert!(s.is_dirty(), "after a save, undo goes back past what the file holds");
    // A failed command does not make the drawing unsaved.
    let mut t = blank_session();
    run(&mut t, "file.save_as", json!({"path": dir.path("b.vsdx")}));
    t.execute("page.rename", &json!({"id": 99, "name": "x"})).unwrap_err();
    assert!(!t.is_dirty());
}

#[test]
fn save_needs_a_file_and_names_save_as_when_there_is_none() {
    let dir = Dir::new();
    let mut s = blank_session();
    let before = snapshot(&s);
    let error = s.execute("file.save", &json!({})).unwrap_err();
    assert!(matches!(error, EngineError::Failed { .. }), "{error:?}");
    assert!(error.to_string().contains("file.save_as"), "{error}");
    assert_eq!(snapshot(&s), before);
    assert!(Session::new().execute("file.save", &json!({})).is_err(), "no document, no save");
    // Nothing was written anywhere.
    assert!(dir.files().is_empty());
}

#[test]
fn save_as_wants_a_drawing_extension_and_writes_nothing_for_another() {
    let dir = Dir::new();
    let mut s = blank_session();
    for name in ["x", "x.txt", "x.vsd", "x.vsdx.bak", ".vsdx", "x.VSDX.zip"] {
        let before = snapshot(&s);
        let error = s.execute("file.save_as", &json!({"path": dir.path(name)})).unwrap_err();
        let accepted = name == ".vsdx";
        assert!(accepted || matches!(error, EngineError::BadParams { .. }), "{name}: {error:?}");
        assert_eq!(snapshot(&s), before, "{name}");
        assert!(!s.is_dirty());
    }
    assert!(!dir.files().iter().any(|f| f.ends_with("txt") || f == "x"), "{:?}", dir.files());
    // Upper case is fine, and the kind follows the extension.
    let result = run(&mut s, "file.save_as", json!({"path": dir.path("x.VSSX")}));
    assert_eq!(result["variant"], "vssx");
    let result = run(&mut s, "file.save_as", json!({"path": dir.path("y.vstm")}));
    assert_eq!(result["variant"], "vstm");
}

#[test]
fn saving_an_opened_file_keeps_every_part_the_edits_did_not_touch_and_save_as_leaves_the_original_alone() {
    let dir = Dir::new();
    let original = drawing();
    let a = dir.path("a.vsdx");
    std::fs::write(&a, &original).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": a}));
    assert!(!s.is_dirty());
    run(&mut s, "page.rename", json!({"id": 2, "name": "Renamed"}));
    // Save As to another name: the original file is not touched, and the new one has the unknown part.
    let b = dir.path("b.vsdx");
    let result = run(&mut s, "file.save_as", json!({"path": b}));
    assert!(result["parts"]["kept"].as_u64().unwrap() >= 6, "{result}");
    assert_eq!(std::fs::read(&a).unwrap(), original, "Save As does not touch the file it was opened from");
    let before = parts(&original);
    let after = parts(&std::fs::read(&b).unwrap());
    assert_eq!(after.get("/foo/bar.dat"), before.get("/foo/bar.dat"));
    assert_eq!(after.get("/visio/pages/page1.xml"), before.get("/visio/pages/page1.xml"), "a page the edit did not reach is the original's");
    assert_eq!(s.path().unwrap().to_str().unwrap(), b, "the new file is the drawing's file now");
    // Save again: patched from what was just written, to the same path.
    run(&mut s, "page.add", json!({"name": "Third"}));
    run(&mut s, "file.save", json!({}));
    let again = parts(&std::fs::read(&b).unwrap());
    assert_eq!(again.get("/foo/bar.dat"), before.get("/foo/bar.dat"));
    assert!(again.contains_key("/visio/pages/page3.xml"));
    let mut reopened = Session::new();
    let opened = run(&mut reopened, "file.open", json!({"path": b}));
    assert_eq!(opened["pages"].as_array().unwrap().len(), 3);
    assert_eq!(opened["pages"][1]["name"], "Renamed");
}

#[test]
fn a_save_over_the_file_it_was_opened_from_keeps_its_permissions_and_a_link() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = Dir::new();
        let real = dir.path("real.vsdx");
        std::fs::write(&real, drawing()).unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
        let link = dir.path("link.vsdx");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let mut s = Session::new();
        run(&mut s, "file.open", json!({"path": link}));
        run(&mut s, "page.add", json!({}));
        run(&mut s, "file.save", json!({}));
        assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o640, "permissions are kept");
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link is still a link");
        let pages = parts(&std::fs::read(&real).unwrap()).keys().filter(|k| k.starts_with("/visio/pages/page") && !k.ends_with("pages.xml")).count();
        assert_eq!(pages, 3, "the file the link names was replaced");
    }
}

#[test]
fn a_save_that_cannot_be_written_leaves_the_old_file_and_the_drawing_unsaved() {
    let dir = Dir::new();
    let original = drawing();
    let a = dir.path("a.vsdx");
    std::fs::write(&a, &original).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": a}));
    run(&mut s, "page.add", json!({}));
    let before = snapshot(&s);
    // A folder that is not there.
    let error = s.execute("file.save_as", &json!({"path": dir.path("no/such/folder/x.vsdx")})).unwrap_err();
    assert!(matches!(error, EngineError::Failed { .. }) && error.to_string().contains("folder does not exist"), "{error}");
    assert!(!error.to_string().contains("diacraft-engine-save"), "no path in the message: {error}");
    // A folder where the file should be.
    std::fs::create_dir(dir.path("d.vsdx")).unwrap();
    let error = s.execute("file.save_as", &json!({"path": dir.path("d.vsdx")})).unwrap_err();
    assert!(error.to_string().contains("stands where the file should be"), "{error}");
    assert_eq!(snapshot(&s), before, "a failed save changes nothing");
    assert!(s.is_dirty());
    assert_eq!(std::fs::read(&a).unwrap(), original);
    // A folder that cannot be written (not as the superuser, who can write anywhere).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let locked = dir.path("locked");
        std::fs::create_dir(&locked).unwrap();
        let target = format!("{locked}/x.vsdx");
        std::fs::write(&target, &original).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::write(format!("{locked}/probe"), b"x").is_err() {
            let error = s.execute("file.save_as", &json!({"path": target})).unwrap_err();
            assert!(matches!(error, EngineError::Failed { .. }), "{error:?}");
            assert_eq!(std::fs::read(&target).unwrap(), original, "the old file is as it was");
            assert_eq!(std::fs::read_dir(&locked).unwrap().count(), 1, "no temporary file");
            assert_eq!(snapshot(&s), before);
        }
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    check_session(&s).unwrap();
}

#[test]
fn an_edit_the_writer_cannot_write_is_an_error_and_the_disk_is_untouched() {
    let dir = Dir::new();
    let a = dir.path("a.vsdx");
    std::fs::write(&a, drawing()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": a}));
    // A page name the writer cannot put in XML: a control character cannot get past the page name rule, so the document is changed
    // through the model directly.
    s.edit("Break the title", |doc| {
        doc.title = std::sync::Arc::from("a\u{1}b");
        Ok(())
    })
    .unwrap();
    let before = std::fs::read(&a).unwrap();
    let error = s.execute("file.save", &json!({})).unwrap_err();
    assert!(matches!(error, EngineError::Failed { .. }), "{error:?}");
    assert_eq!(std::fs::read(&a).unwrap(), before);
    assert!(s.is_dirty());
    assert_eq!(dir.files(), ["a.vsdx"]);
}

#[test]
fn saving_inside_a_batch_is_refused() {
    let dir = Dir::new();
    let mut s = blank_session();
    let path = dir.path("a.vsdx");
    let error = s
        .execute("command.batch", &json!({"commands": [{"command": "page.add"}, {"command": "file.save_as", "params": {"path": path}}]}))
        .unwrap_err();
    assert!(error.to_string().contains("batch") || error.to_string().contains("file.save_as"), "{error}");
    assert!(dir.files().is_empty());
    assert!(!s.is_dirty(), "the batch was put back");
}

// ------------------------------------------------------------------------------------------------ what the editing commands make

/// The document with what only the file knows (the relationship ids, the times, the document's unknown elements, the font table the writer
/// lists for a drawing that names no face) cleared.
fn as_file_holds(doc: &diacraft_doc::Document) -> diacraft_doc::Document {
    let mut doc = doc.clone();
    doc.props = std::sync::Arc::new(diacraft_doc::DocProps::default());
    doc.passthrough = diacraft_doc::Passthrough::new();
    doc.fonts = std::sync::Arc::new(Vec::new());
    // What a file holds of its own: the names of the parts of pages and masters, what the roots and the rows of their parts hold that no field does,
    // the parts of the package the model does not rebuild, and what the document's own root and tables hold. A drawing made in a session has none of it.
    doc.ext = Default::default();
    doc.parts = diacraft_doc::SharedVec::new();
    doc.vsdx = None;
    for i in 0..doc.pages.len() {
        if let Some(slot) = doc.pages.get_mut(i) {
            let page = std::sync::Arc::make_mut(slot);
            page.rel_id = None;
            page.part_name = None;
            page.ext = Default::default();
        }
    }
    for i in 0..doc.masters.len() {
        if let Some(slot) = doc.masters.get_mut(i) {
            let master = std::sync::Arc::make_mut(slot);
            master.rel_id = None;
            master.part_name = None;
            master.ext = Default::default();
        }
    }
    doc
}

fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    s
}

#[test]
fn shapes_dropped_and_connected_are_saved_and_read_back_as_the_model() {
    let dir = Dir::new();
    let mut s = blank_session();
    let a = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 3.0}));
    let b = run(&mut s, "shape.drop", json!({"master": "ellipse", "page": 1, "x": 6.0, "y": 3.0}));
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 4.0, "y": 6.0}));
    run(&mut s, "connector.add", json!({"from": {"id": a["id"]}, "to": {"id": b["id"]}}));
    run(&mut s, "shape.set_text", json!({"id": a["id"], "text": "Start & <end>"}));
    run(&mut s, "shape.move", json!({"ids": [b["id"]], "dx": 1.0, "dy": -0.5}));
    let path = dir.path("drawn.vsdx");
    let saved = run(&mut s, "file.save_as", json!({"path": path}));
    assert_eq!(saved["warning_count"], 0, "{saved}");
    let bytes = std::fs::read(&path).unwrap();
    // The file reads with no warning and holds the masters, the instances and the Connect rows the commands made.
    let (read, report) = diacraft_vsdx::read(&bytes, &diacraft_vsdx::Limits::default()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let model = s.document().unwrap();
    assert_eq!(as_file_holds(&read), as_file_holds(model));
    assert_eq!(read.masters.len(), 2, "rectangle and ellipse");
    assert!(read.masters.iter().all(|m| m.rel_id.is_some() && m.unique_id.is_some()));
    let page = read.page(1).unwrap();
    assert_eq!(page.shapes.len(), 4, "three shapes and a connector");
    assert!(page.connects.len() >= 2, "the connector's two ends are glued: {:?}", page.connects);
    assert_eq!(page.connects, model.page(1).unwrap().connects, "the Connect rows survive as the model has them");
    assert!(read.masters.iter().all(|m| m.raw.get("BaseID").is_some_and(|g| g.starts_with("{D1AC0001-"))), "the masters keep their fixed BaseIDs");
    let instances = page.shapes.iter().filter(|s| s.master.is_some()).count();
    assert_eq!(instances, 3, "the three dropped shapes are instances of masters");
    // The package has the parts and the relationships a file needs for them.
    let p = parts(&bytes);
    assert!(
        p.contains_key("/visio/masters/masters.xml") && p.contains_key("/visio/masters/master1.xml") && p.contains_key("/visio/masters/master2.xml")
    );
    let pkg = Package::read(&bytes, &Limits::default()).unwrap();
    assert!(pkg.warnings().is_empty(), "{:?}", pkg.warnings());
    let rels = pkg.rels("/visio/pages/page1.xml").unwrap();
    assert_eq!(rels.of_type("http://schemas.microsoft.com/visio/2010/relationships/master").count(), 2, "{rels:?}");
    // Opened again, the drawing is the same, and it can be edited and saved again.
    let mut t = reopened(&path);
    assert_eq!(as_file_holds(t.document().unwrap()), as_file_holds(model));
    run(&mut t, "shape.move", json!({"ids": [a["id"]], "dx": 0.5, "dy": 0.0}));
    run(&mut t, "file.save", json!({}));
    let again = reopened(&path);
    assert_eq!(as_file_holds(again.document().unwrap()), as_file_holds(t.document().unwrap()));
    // A drawing saved, edited, undone and saved: the second file is the first's model again.
    run(&mut t, "edit.undo", json!({}));
    run(&mut t, "file.save", json!({}));
    assert_eq!(as_file_holds(reopened(&path).document().unwrap()), as_file_holds(model));
}

#[test]
fn editing_a_drawing_that_came_from_a_file_saves_into_that_file() {
    let dir = Dir::new();
    let original = drawing();
    let a = dir.path("a.vsdx");
    std::fs::write(&a, &original).unwrap();
    let mut s = reopened(&a);
    let dropped = run(&mut s, "shape.drop", json!({"master": "hexagon", "page": 1, "x": 4.0, "y": 4.0}));
    let other = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 2, "x": 3.0, "y": 3.0}));
    run(&mut s, "connector.add", json!({"from": {"id": dropped["id"]}, "to": {"x": 9.0, "y": 9.0}}));
    run(&mut s, "shape.set_text", json!({"id": other["id"], "page": 2, "text": "hi"}));
    run(&mut s, "file.save", json!({}));
    let after = parts(&std::fs::read(&a).unwrap());
    let before = parts(&original);
    assert_eq!(after.get("/foo/bar.dat"), before.get("/foo/bar.dat"), "the unknown part is the file's own");
    assert_ne!(after.get("/visio/pages/page1.xml"), before.get("/visio/pages/page1.xml"));
    let page1 = String::from_utf8(after["/visio/pages/page1.xml"].clone()).unwrap();
    assert!(page1.contains("<Cell N=\"Width\" V=\"2.50\"/>"), "the shape the file had keeps its text: {page1}");
    let t = reopened(&a);
    assert_eq!(as_file_holds(t.document().unwrap()), as_file_holds(s.document().unwrap()));
    assert!(!t.is_dirty());
}

// -------------------------------------------------------------------------------------------------------- links (a security rule)

/// `file.save_as` is reachable from the control channel and MCP, where a program names the path. A link must never make a save write a file
/// the caller did not name.
#[cfg(unix)]
mod links {
    use super::*;
    use std::os::unix::fs::symlink;

    fn is_link(path: &str) -> bool {
        std::fs::symlink_metadata(path).unwrap().file_type().is_symlink()
    }

    #[test]
    fn a_link_to_a_file_with_another_extension_is_refused_and_the_target_is_unchanged() {
        let dir = Dir::new();
        let precious = dir.path("bashrc.txt");
        std::fs::write(&precious, b"export PATH=precious\n").unwrap();
        let link = dir.path("innocent.vsdx");
        symlink(&precious, &link).unwrap();
        for opened in [false, true] {
            let mut s = blank_session();
            if opened {
                // A drawing with a file of its own makes no difference: the link does not lead to that file.
                run(&mut s, "file.save_as", json!({"path": dir.path("mine.vsdx")}));
            }
            run(&mut s, "page.add", json!({}));
            let before = snapshot(&s);
            let error = s.execute("file.save_as", &json!({"path": link})).unwrap_err();
            assert!(matches!(error, EngineError::Failed { .. }) && error.to_string().contains("link"), "{error}");
            assert!(!error.to_string().contains("bashrc") && !error.to_string().contains("diacraft-engine-save"), "no path in the message: {error}");
            assert_eq!(snapshot(&s), before);
            assert!(s.is_dirty());
        }
        assert_eq!(std::fs::read(&precious).unwrap(), b"export PATH=precious\n", "the target is as it was");
        assert!(is_link(&link), "the link is still a link");
        assert_eq!(dir.files(), ["bashrc.txt", "innocent.vsdx", "mine.vsdx"], "no temporary file");
    }

    #[test]
    fn a_link_to_a_drawing_that_is_not_the_drawings_own_is_refused_too() {
        let dir = Dir::new();
        let other = dir.path("other.vsdx");
        std::fs::write(&other, drawing()).unwrap();
        let link = dir.path("alias.vsdx");
        symlink(&other, &link).unwrap();
        let mut s = blank_session();
        let error = s.execute("file.save_as", &json!({"path": link})).unwrap_err();
        assert!(error.to_string().contains("link"), "{error}");
        assert_eq!(std::fs::read(&other).unwrap(), drawing(), "even a file with a drawing's extension is not written through a link");
        // A link to a folder.
        let folder = dir.path("folder");
        std::fs::create_dir(&folder).unwrap();
        let to_folder = dir.path("folder.vsdx");
        symlink(&folder, &to_folder).unwrap();
        assert!(s.execute("file.save_as", &json!({"path": to_folder})).is_err());
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
    }

    #[test]
    fn a_link_to_the_open_file_saves_it_and_stays_a_link() {
        let dir = Dir::new();
        let real = dir.path("real.vsdx");
        std::fs::write(&real, drawing()).unwrap();
        let mut s = Session::new();
        run(&mut s, "file.open", json!({"path": real}));
        run(&mut s, "page.add", json!({"name": "Third"}));
        // A link whose own name has no drawing extension: the extension that counts is the file it leads to.
        let link = dir.path("alias.dat");
        symlink(&real, &link).unwrap();
        let result = run(&mut s, "file.save_as", json!({"path": link}));
        assert_eq!(result["variant"], "vsdx");
        assert!(is_link(&link), "the link is still a link");
        assert!(!s.is_dirty());
        let reopened = reopened(&real);
        assert_eq!(reopened.document().unwrap().pages.len(), 3, "the open file was saved");
        assert_eq!(dir.files(), ["alias.dat", "real.vsdx"]);
        // The drawing's file is now the name that was given, and saving again goes through it to the same file.
        run(&mut s, "page.add", json!({}));
        run(&mut s, "file.save", json!({}));
        assert_eq!(super::reopened(&real).document().unwrap().pages.len(), 4);
        assert!(is_link(&link));
    }

    #[test]
    fn a_drawing_opened_through_a_link_saves_through_it_and_a_link_to_a_link_to_it_does_too() {
        let dir = Dir::new();
        let real = dir.path("real.vsdx");
        std::fs::write(&real, drawing()).unwrap();
        let first = dir.path("first.vsdx");
        let second = dir.path("second.vsdx");
        symlink(&real, &first).unwrap();
        symlink(&first, &second).unwrap();
        let mut s = Session::new();
        run(&mut s, "file.open", json!({"path": first}));
        run(&mut s, "page.add", json!({}));
        run(&mut s, "file.save_as", json!({"path": second}));
        assert!(is_link(&first) && is_link(&second));
        assert_eq!(super::reopened(&real).document().unwrap().pages.len(), 3);
    }

    #[test]
    fn a_dangling_link_and_a_circle_of_links_are_refused_and_nothing_is_made() {
        let dir = Dir::new();
        let dangling = dir.path("dangling.vsdx");
        let nowhere = dir.path("nowhere.txt");
        symlink(&nowhere, &dangling).unwrap();
        let a = dir.path("a.vsdx");
        let b = dir.path("b.vsdx");
        symlink(&b, &a).unwrap();
        symlink(&a, &b).unwrap();
        let mut s = blank_session();
        run(&mut s, "page.add", json!({}));
        let before = snapshot(&s);
        for link in [&dangling, &a] {
            let error = s.execute("file.save_as", &json!({"path": link})).unwrap_err();
            assert!(error.to_string().contains("leads nowhere"), "{error}");
        }
        assert_eq!(snapshot(&s), before);
        assert!(!std::path::Path::new(&nowhere).exists(), "the file the dangling link names was not created");
        assert_eq!(dir.files(), ["a.vsdx", "b.vsdx", "dangling.vsdx"]);
    }

    #[test]
    fn a_link_in_a_folder_of_the_path_is_the_users_own_choice_and_a_link_at_the_end_is_not() {
        // A folder that is a link is a place the caller named, and a file in it is the file named. The end of the path is what is judged.
        let dir = Dir::new();
        let real_folder = dir.path("real");
        std::fs::create_dir(&real_folder).unwrap();
        symlink(&real_folder, dir.path("linked")).unwrap();
        let mut s = blank_session();
        run(&mut s, "file.save_as", json!({"path": format!("{}/x.vsdx", dir.path("linked"))}));
        assert!(std::path::Path::new(&format!("{real_folder}/x.vsdx")).is_file());
    }
}

// ------------------------------------------------------------------------------------------------------------------------ edit and save, again and again

/// One round of the sequence a person does: drop a shape, save in place. Every round must save.
fn drop_and_save(s: &mut Session, round: usize) {
    run(s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0 + round as f64, "y": 3.0}));
    assert!(s.is_dirty(), "round {round}");
    s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("round {round}: file.save: {e}"));
    assert!(!s.is_dirty(), "round {round}");
}

fn shapes_on_first_page(s: &Session) -> usize {
    s.document().unwrap().pages.get(0).unwrap().shapes.len()
}

fn document_part(path: &str) -> Vec<u8> {
    parts(&std::fs::read(path).unwrap()).get("/visio/document.xml").cloned().unwrap()
}

/// `rounds` rounds of [`drop_and_save`] on the drawing in `s`, saved at `path`. Between them the document part is the file's own (an edit of shapes
/// does not rewrite the document's tables, so the font table and the unknown elements the file holds are not taken for edits), and every shape
/// dropped is on disk.
fn rounds_keep_the_document_part(s: &mut Session, path: &str, rounds: usize, what: &str) {
    let before = shapes_on_first_page(s);
    let first = document_part(path);
    for round in 1..=rounds {
        drop_and_save(s, round);
        assert_eq!(document_part(path), first, "{what}: round {round} rewrote the document part");
    }
    let mut other = Session::new();
    run(&mut other, "file.open", json!({"path": path}));
    assert_eq!(shapes_on_first_page(&other), before + rounds, "{what}");
}

#[test]
fn a_drawing_saved_as_a_file_and_edited_can_be_saved_in_place_again_and_again() {
    // `file.new`, `file.save_as`, then a shape dropped and a save, over and over: the second in-place save used to fail with "a change to the unknown
    // elements of the document", because the file the first one read back held the skeleton's `DocumentSettings`, which the drawing's model does not.
    let dir = Dir::new();
    let mut s = blank_session();
    let path = dir.path("loop.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    rounds_keep_the_document_part(&mut s, &path, 6, "vsdx");
    assert_eq!(dir.files(), ["loop.vsdx"], "no temporary file is left");
}

#[test]
fn a_new_drawing_saved_as_each_of_the_six_types_can_be_saved_in_place_again_and_again() {
    let dir = Dir::new();
    for ext in ["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm"] {
        let mut s = blank_session();
        let path = dir.path(&format!("six.{ext}"));
        run(&mut s, "file.save_as", json!({"path": path}));
        rounds_keep_the_document_part(&mut s, &path, 4, ext);
    }
}

#[test]
fn a_file_that_was_opened_can_be_saved_in_place_again_and_again_and_so_can_one_saved_as_each_type() {
    // A file with what a real one holds that the model does not: unknown elements of the document, a font table, colours, a theme, an unknown part.
    let bytes = VsdxBuilder::new()
        .colors(r##"<ColorEntry IX="0" RGB="#000000"/>"##)
        .face_names(r#"<FaceName ID="1" Name="Arial"/><FaceName ID="2" Name="Segoe Print"/>"#)
        .document_extra(
            r#"<DocumentSettings TopPage="0"><DefaultTextStyle>0</DefaultTextStyle></DocumentSettings><x:note xmlns:x="urn:example:x">kept</x:note>"#,
        )
        .theme(b"<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"T\"/>")
        .page(PageSpec::new(1, "First").shapes(&shape(5, r#"<Cell N="Width" V="2.50"/>"#)))
        .with(Pkg::new().part_without_override("/foo/bar.dat", b"0123456789").default_type("dat", "application/x-foo"))
        .build();
    let dir = Dir::new();
    let path = dir.path("real.vsdx");
    std::fs::write(&path, &bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    // The first save of an opened file may write the document part (the writer's name is not in it, so it does not); the later ones must not.
    drop_and_save(&mut s, 1);
    rounds_keep_the_document_part(&mut s, &path, 4, "opened");
    assert_eq!(
        parts(&std::fs::read(&path).unwrap()).get("/foo/bar.dat").map(Vec::as_slice),
        Some(&b"0123456789"[..]),
        "an unknown part is the file's"
    );
    assert_eq!(document_part(&path), parts(&bytes).get("/visio/document.xml").cloned().unwrap(), "and so is the document part, byte for byte");
    // The same file saved as each type, and then edited and saved in place there.
    for ext in ["vsdm", "vssx", "vssm", "vstx", "vstm"] {
        let mut s = Session::new();
        run(&mut s, "file.open", json!({"path": path}));
        let there = dir.path(&format!("real.{ext}"));
        run(&mut s, "file.save_as", json!({"path": there}));
        rounds_keep_the_document_part(&mut s, &there, 3, ext);
    }
}

#[test]
fn applying_a_theme_to_a_single_quoted_crlf_file_adds_its_row_and_keeps_every_other_rows_bytes() {
    // A file whose content types are laid out as a tool lays them out: single quotes, CR LF, an indented row to a line.
    let pkg = VsdxBuilder::new()
        .page(PageSpec::new(1, "First").shapes(&shape(5, r#"<Cell N="Width" V="2.50"/>"#)))
        .with(Pkg::new().part_without_override("/foo/bar.dat", b"0123456789").default_type("dat", "application/x-foo"))
        .pkg();
    let rows = |bytes: &[u8]| {
        let p = Package::read(bytes, &Limits::default()).unwrap();
        (p.content_types().defaults.clone(), p.content_types().overrides.clone())
    };
    let (defaults, overrides) = rows(&pkg.build());
    let mut single =
        String::from("<?xml version='1.0' encoding='UTF-8'?>\r\n<Types xmlns='http://schemas.openxmlformats.org/package/2006/content-types'>");
    for (extension, ct) in &defaults {
        single.push_str(&format!("\r\n  <Default Extension='{extension}' ContentType='{ct}'/>"));
    }
    for (part, ct) in &overrides {
        single.push_str(&format!("\r\n  <Override PartName='{part}' ContentType='{ct}'/>"));
    }
    single.push_str("\r\n</Types>\r\n");
    let mut items = pkg.items();
    items[0] = ZipItem::deflated("[Content_Types].xml", single.as_bytes());
    let dir = Dir::new();
    let path = dir.path("themed.vsdx");
    std::fs::write(&path, zip(&items)).unwrap();

    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("file.save: {e}"));

    let saved = std::fs::read(&path).unwrap();
    let (defaults_after, overrides_after) = rows(&saved);
    assert_eq!(defaults_after, defaults, "no Default changed");
    let added: Vec<&(String, String)> = overrides_after.iter().filter(|row| !overrides.contains(row)).collect();
    assert_eq!(added.len(), 1, "one row, for the theme part: {added:?}");
    assert!(added[0].1.ends_with("theme+xml"), "{added:?}");
    let stream = String::from_utf8(Package::read(&saved, &Limits::default()).unwrap().content_types_bytes().to_vec()).unwrap();
    let want =
        single.replace("\r\n</Types>\r\n", &format!("\r\n  <Override PartName='{}' ContentType='{}'/>\r\n</Types>\r\n", added[0].0, added[0].1));
    assert_eq!(stream, want, "every other row is the file's bytes, and the new one is laid out as they are");
}

/// An unedited save of a file whose document part has two `StyleSheet` rows of one `Name` (with `ID`s 4 and 5, beside the root style's 0) says nothing, as its open does:
/// the styles carry no data of the other format, so none is left out. A save of such a file once said "the data of 2 pages, masters or styles was left out".
#[test]
fn an_unedited_save_of_a_file_whose_styles_share_a_name_says_nothing_as_its_open_does() {
    let dir = Dir::new();
    let bytes = VsdxBuilder::new()
        .more_styles(
            r#"<StyleSheet ID="4" Name="Twin" NameU="Twin"><Cell N="LineWeight" V="0.01"/></StyleSheet><StyleSheet ID="5" Name="Twin" NameU="Twin"><Cell N="LineWeight" V="0.02"/></StyleSheet>"#,
        )
        .page(PageSpec::new(1, "P").shapes(&shape(1, r#"<Cell N="PinX" V="1"/>"#)))
        .build();
    let path = dir.path("twins.vsdx");
    std::fs::write(&path, &bytes).unwrap();
    let mut s = Session::new();
    let opened = run(&mut s, "file.open", json!({"path": path}));
    assert_eq!(opened["file"]["warning_count"], 0, "{opened}");
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["warning_count"], 0, "{saved}");
    assert_eq!(saved["warnings"], json!([]), "{saved}");
    // The file holds no DiaCraft part after the save: nothing rides.
    let after = std::fs::read(&path).unwrap();
    assert!(Package::read(&after, &Limits::default()).unwrap().parts().all(|p| !p.contains("solution")), "no solution part is made");
    check_session(&s).unwrap();
}
