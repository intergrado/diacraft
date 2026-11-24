//! The six formats through the commands: each opens and saves unchanged, a template opens as a new unsaved drawing, a Save As between
//! the types changes the type and what the new kind needs, and a macro project is kept as bytes or dropped only by the caller's choice, in words.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_engine::{EngineError, Session};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{MasterSpec, PageSpec, Pkg, VsdxBuilder, cell, shape};
use diacraft_vsdx::Variant;
use serde_json::json;

use crate::run;

const EXTENSIONS: [&str; 6] = ["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm"];
const VBA_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";
const VBA_BYTES: &[u8] = b"not code, never run";

/// A scratch folder under `TMPDIR`, removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-formats-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }

    fn write(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.path(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    pkg.parts().map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect()
}

fn variant(ext: &str) -> Variant {
    Variant::from_extension(ext).unwrap()
}

/// A package of the type `ext`: a stencil has a page row with no part, the others a page with a shape; all have a master and an unknown part;
/// the macro-enabled ones hold a project with a signature.
fn package(ext: &str) -> Vec<u8> {
    let v = variant(ext);
    let mut builder = VsdxBuilder::new().document_type(v.content_type()).master(MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))));
    builder = if v.kind == diacraft_vsdx::Kind::Stencil {
        builder.page(PageSpec::new(0, "Page-1").without_part())
    } else {
        builder.page(PageSpec::new(1, "First").shapes(&shape(5, &cell("PinX", "1"))))
    };
    builder = builder.with(Pkg::new().part_without_override("/foo/bar.dat", b"0123456789").default_type("dat", "application/x-foo"));
    if v.macro_enabled {
        builder = builder.with(
            Pkg::new()
                .default_type("bin", "application/vnd.ms-office.vbaProject")
                .part_without_override("/visio/vbaProject.bin", VBA_BYTES)
                .rel("/visio/document.xml", "rId50", VBA_REL, "vbaProject.bin")
                .part("/visio/vbaProjectSignature.bin", "application/vnd.ms-office.vbaProjectSignature", b"sig")
                .rel(
                    "/visio/vbaProject.bin",
                    "rId1",
                    "http://schemas.microsoft.com/office/2006/relationships/vbaProjectSignature",
                    "vbaProjectSignature.bin",
                ),
        );
    }
    builder.build()
}

fn open(s: &mut Session, path: &str) -> serde_json::Value {
    run(s, "file.open", json!({"path": path, "edit_template": true}))
}

fn refused(s: &mut Session, id: &str, params: serde_json::Value) -> EngineError {
    s.execute(id, &params).unwrap_err()
}

#[test]
fn every_one_of_the_six_formats_opens_and_saves_unchanged() {
    for ext in EXTENSIONS {
        let dir = Dir::new();
        let bytes = package(ext);
        let path = dir.write(&format!("in.{ext}"), &bytes);
        let mut s = Session::new();
        let opened = open(&mut s, &path);
        assert_eq!(opened["file"]["variant"], ext, "{ext}");
        assert_eq!(opened["file"]["warning_count"], 0, "{ext}: {opened}");
        assert_eq!(opened["file"]["new_from_template"], false, "{ext}: the file itself was asked for");
        let saved = run(&mut s, "file.save", json!({}));
        assert_eq!(saved["variant"], ext, "{ext}");
        assert_eq!(saved["macros_dropped"], 0, "{ext}");
        let (before, after) = (parts(&bytes), parts(&std::fs::read(&path).unwrap()));
        assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>(), "{ext}");
        let changed: Vec<&str> = after.iter().filter(|(k, v)| before.get(*k) != Some(*v)).map(|(k, _)| k.as_str()).collect();
        assert_eq!(changed, ["/docProps/app.xml"], "{ext}: nothing else differs");
        assert_eq!(dir.files(), [format!("in.{ext}")], "{ext}: no staging file is left");
    }
}

#[test]
fn a_template_opens_as_a_new_unsaved_drawing_that_has_no_file_and_the_template_is_never_written() {
    for ext in ["vstx", "vstm"] {
        let dir = Dir::new();
        let bytes = package(ext);
        let path = dir.write(&format!("t.{ext}"), &bytes);
        let mut s = Session::new();
        let opened = run(&mut s, "file.open", json!({"path": path}));
        assert_eq!(opened["file"]["variant"], ext);
        assert_eq!(opened["file"]["kind"], "template");
        assert_eq!(opened["file"]["new_from_template"], true, "{ext}");
        assert!(s.path().is_none() && !s.is_dirty(), "{ext}: a drawing with no file, not unsaved until it is edited");
        assert_eq!(run(&mut s, "file.status", json!({})), json!({"dirty": false, "has_file": false, "format": null, "variant": null, "kind": null}));
        assert_eq!(opened["pages"], json!([{"id": 1, "name": "First"}]), "{ext}: the template's content is the drawing");
        // `file.save` has nowhere to write, and says to use Save As.
        let error = refused(&mut s, "file.save", json!({}));
        assert!(error.to_string().contains("file.save_as"), "{error}");
        // The first edit makes it unsaved.
        run(&mut s, "shape.move", json!({"ids": [5], "dx": 1.0, "dy": 0.0}));
        assert!(s.is_dirty());
        // Save As a drawing: the type changes, the template's other parts come along, and the template on the disk is as it was.
        let out = dir.path("copy.vsdx");
        let result = if ext == "vstm" {
            let error = refused(&mut s, "file.save_as", json!({"path": out}));
            assert!(matches!(error, EngineError::MacrosNotAllowed { .. }), "{error:?}");
            assert_eq!(dir.files(), [format!("t.{ext}")], "nothing was written");
            assert!(s.path().is_none() && s.is_dirty(), "and the drawing is as it was");
            run(&mut s, "file.save_as", json!({"path": out, "drop_macros": true}))
        } else {
            run(&mut s, "file.save_as", json!({"path": out}))
        };
        assert_eq!(result["variant"], "vsdx");
        assert!(!s.is_dirty() && s.path().unwrap().to_str() == Some(out.as_str()));
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "{ext}: the template file is untouched");
        let made = parts(&std::fs::read(&out).unwrap());
        assert_eq!(made.get("/foo/bar.dat").map(Vec::as_slice), Some(&b"0123456789"[..]), "{ext}: a part the model does not hold comes along");
        let mut other = Session::new();
        let again = run(&mut other, "file.open", json!({"path": out}));
        assert_eq!(again["file"]["variant"], "vsdx");
        assert_eq!(again["file"]["new_from_template"], false);
        assert!(other.path().is_some());
        // Saved again, it is a file like any other.
        run(&mut s, "file.save", json!({}));
    }
}

#[test]
fn a_template_opened_to_be_changed_is_the_file_itself_and_keeps_its_type() {
    let dir = Dir::new();
    let path = dir.write("t.vstx", &package("vstx"));
    let mut s = Session::new();
    let opened = run(&mut s, "file.open", json!({"path": path, "edit_template": true}));
    assert_eq!(opened["file"]["new_from_template"], false);
    assert_eq!(s.path().unwrap().to_str(), Some(path.as_str()));
    run(&mut s, "shape.move", json!({"ids": [5], "dx": 1.0, "dy": 0.0}));
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["variant"], "vstx");
    let mut check = Session::new();
    assert_eq!(open(&mut check, &path)["file"]["variant"], "vstx");
}

#[test]
fn a_template_copy_saved_as_a_template_is_a_template_and_the_times_are_the_copys() {
    let dir = Dir::new();
    let path = dir.write("t.vstx", &package("vstx"));
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let out = dir.path("mine.vstx");
    let saved = run(&mut s, "file.save_as", json!({"path": out}));
    assert_eq!(saved["variant"], "vstx");
    let core = String::from_utf8(parts(&std::fs::read(&out).unwrap())["/docProps/core.xml"].clone()).unwrap();
    assert!(core.contains("<dcterms:created") && core.contains("<dcterms:modified"), "{core}");
}

#[test]
fn a_macro_enabled_drawing_saved_as_a_type_without_macros_is_refused_until_the_caller_chooses_and_says_so() {
    let dir = Dir::new();
    let bytes = package("vsdm");
    let path = dir.write("m.vsdm", &bytes);
    let mut s = Session::new();
    open(&mut s, &path);
    let out = dir.path("plain.vsdx");
    let error = refused(&mut s, "file.save_as", json!({"path": out}));
    match &error {
        EngineError::MacrosNotAllowed { cmd, msg } => {
            assert_eq!(cmd, "file.save_as");
            assert!(msg.contains("macro") && msg.contains("leave"), "{msg}");
            assert!(!msg.contains(&dir.path("")), "no path in the words: {msg}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(dir.files(), ["m.vsdm"], "nothing was written");
    assert_eq!(s.path().unwrap().to_str(), Some(path.as_str()), "the drawing's file is still the macro file");
    // The choice made: the project goes, the words say so, and the file reads back as a drawing.
    let saved = run(&mut s, "file.save_as", json!({"path": out, "drop_macros": true}));
    assert_eq!(saved["variant"], "vsdx");
    assert_eq!(saved["macros_dropped"], 2, "the project and its signature");
    let said =
        saved["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).find(|w| w.contains("macro")).expect("a word about the macros");
    assert!(said.contains("vbaProject.bin"), "{said}");
    let made = parts(&std::fs::read(&out).unwrap());
    assert!(!made.keys().any(|k| k.to_ascii_lowercase().contains("vba")), "{:?}", made.keys());
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "the macro file is untouched");
    let mut check = Session::new();
    assert_eq!(run(&mut check, "file.open", json!({"path": out}))["file"]["warning_count"], 0);
}

#[test]
fn a_macro_project_is_kept_as_bytes_by_every_save_that_keeps_a_macro_type() {
    for (from, to) in [("vsdm", "vsdm"), ("vsdm", "vssm"), ("vsdm", "vstm"), ("vssm", "vsdm"), ("vstm", "vsdm")] {
        let dir = Dir::new();
        let path = dir.write(&format!("m.{from}"), &package(from));
        let mut s = Session::new();
        open(&mut s, &path);
        if variant(from).kind != diacraft_vsdx::Kind::Stencil {
            run(&mut s, "shape.move", json!({"ids": [5], "dx": 1.0, "dy": 0.0}));
        }
        let out = dir.path(&format!("out.{to}"));
        // Not `drop_macros`: the type allows macros, and the project is not the caller's to lose.
        let saved = run(&mut s, "file.save_as", json!({"path": out}));
        assert_eq!(saved["macros_dropped"], 0, "{from} to {to}");
        let made = parts(&std::fs::read(&out).unwrap());
        assert_eq!(made.get("/visio/vbaProject.bin").map(Vec::as_slice), Some(VBA_BYTES), "{from} to {to}");
        assert_eq!(made.get("/visio/vbaProjectSignature.bin").map(Vec::as_slice), Some(&b"sig"[..]), "{from} to {to}");
    }
}

#[test]
fn a_drawing_saved_as_a_template_or_a_stencil_changes_the_type_and_a_stencil_is_told_what_it_holds() {
    let dir = Dir::new();
    let path = dir.write("d.vsdx", &package("vsdx"));
    let mut s = Session::new();
    open(&mut s, &path);
    let template = run(&mut s, "file.save_as", json!({"path": dir.path("t.vstx")}));
    assert_eq!(template["variant"], "vstx");
    assert_eq!(template["warnings"], json!([]));
    // The drawing is now the template's file; open the drawing again to save it as a stencil.
    let mut again = Session::new();
    open(&mut again, &path);
    let stencil = run(&mut again, "file.save_as", json!({"path": dir.path("s.vssx")}));
    assert_eq!(stencil["variant"], "vssx");
    let words: Vec<&str> = stencil["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).collect();
    assert!(words.iter().any(|w| w.contains("stencil") && w.contains("page")), "{words:?}");
    // The stencil reads back as one, with the drawing's master.
    let mut check = Session::new();
    let opened = open(&mut check, &dir.path("s.vssx"));
    assert_eq!(opened["file"]["variant"], "vssx");
    assert_eq!(opened["file"]["kind"], "stencil");
    assert_eq!(check.document().unwrap().masters.len(), 1);
}

#[test]
fn a_stencil_opens_with_a_page_that_has_no_part_and_saved_as_a_drawing_gets_one() {
    let dir = Dir::new();
    let path = dir.write("s.vssx", &package("vssx"));
    let mut s = Session::new();
    let opened = open(&mut s, &path);
    assert_eq!(opened["file"]["kind"], "stencil");
    let out = dir.path("d.vsdx");
    run(&mut s, "file.save_as", json!({"path": out}));
    let made = parts(&std::fs::read(&out).unwrap());
    assert!(made.contains_key("/visio/pages/page1.xml"), "{:?}", made.keys());
    let mut check = Session::new();
    let again = run(&mut check, "file.open", json!({"path": out}));
    assert_eq!(again["file"]["warning_count"], 0, "{again}");
}

#[test]
fn a_new_drawing_is_saved_as_any_of_the_six_by_its_extension() {
    for ext in EXTENSIONS {
        let dir = Dir::new();
        let mut s = crate::blank_session();
        let out = dir.path(&format!("n.{ext}"));
        let saved = run(&mut s, "file.save_as", json!({"path": out}));
        assert_eq!(saved["variant"], ext);
        assert_eq!(saved["macros_dropped"], 0);
        let mut check = Session::new();
        let opened = open(&mut check, &out);
        assert_eq!(opened["file"]["variant"], ext);
        assert_eq!(opened["file"]["warning_count"], 0, "{ext}: {opened}");
    }
}

#[test]
fn the_params_of_the_two_commands_are_documented_and_a_misspelling_is_refused() {
    let mut s = Session::new();
    let listing = s.commands();
    let params = |id: &str| listing.iter().find(|c| c.id == id).map(|c| c.params.to_owned()).unwrap();
    assert!(params("file.open").contains("edit_template"));
    assert!(params("file.save_as").contains("drop_macros"));
    assert!(!params("file.save").contains("drop_macros"), "file.save keeps the type, so it never drops a project");
    let error = s.execute("file.open", &json!({"path": "/x/y.vsdx", "edit_templates": true})).unwrap_err();
    assert!(matches!(error, EngineError::BadParams { .. }), "{error:?}");
    let bad = s.execute("file.open", &json!({"path": "/x/y.vsdx", "edit_template": "yes"})).unwrap_err();
    assert!(matches!(bad, EngineError::BadParams { .. }), "{bad:?}");
}

#[test]
fn a_part_name_that_holds_words_is_never_in_the_result_or_the_error_of_a_save() {
    let dir = Dir::new();
    let project = "/visio/Secret(story)~.bin";
    let bytes = VsdxBuilder::new()
        .document_type(variant("vsdm").content_type())
        .page(PageSpec::new(1, "A"))
        .with(Pkg::new().default_type("bin", "application/vnd.ms-office.vbaProject").part_without_override(project, VBA_BYTES).rel(
            "/visio/document.xml",
            "rId50",
            VBA_REL,
            "Secret(story)~.bin",
        ))
        .build();
    let path = dir.write("m.vsdm", &bytes);
    let mut s = Session::new();
    open(&mut s, &path);
    let out = dir.path("plain.vsdx");
    let refusal = refused(&mut s, "file.save_as", json!({"path": out}));
    assert!(!refusal.to_string().contains("Secret"), "{refusal}");
    let saved = run(&mut s, "file.save_as", json!({"path": out, "drop_macros": true}));
    assert_eq!(saved["macros_dropped"], 1);
    assert!(!saved.to_string().contains("Secret"), "the result and its warnings name no part a person named: {saved}");
}
