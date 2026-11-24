//! Reopen equivalence (the design's section 10): a save in the session equals, byte for byte, the same save after a `.diacraft` save and a reopen in a new session, so that
//! the bytes a session happened to read never change what it writes. The clock is pinned, since a model writer's modified time comes from the caller.
//!
//! A hand mutant that decides bookkeeping by `!s.is_dirty()` in place of the stamp comparison (`Session::content_unchanged`) must fail
//! `an_edit_made_before_the_save_is_an_edit_to_the_foreign_file_after_the_reopen`, and, since the VSDX model writer is wired,
//! `the_stamp_not_the_dirty_flag_decides_the_bookkeeping` and `a_vsdx_save_from_the_model_equals_the_same_after_a_reopen`.

use serde_json::json;

use crate::edit::drop_shape;
use crate::native_common::{Dir, PINNED, made, opened, pinned, status, vsdx_drawing};
use crate::{blank_session, run};

/// Saves the drawing in `first` as `one.<ext>`, opens that in a session of its own and saves it as `two.<ext>`: the same bytes. The session of the reopen saves again over its own file
/// and over a new name, and none of them moves a byte. `ext` says the kind.
fn assert_the_save_after_a_reopen_is_the_save(what: &str, mut first: diacraft_engine::Session, ext: &str) {
    let dir = Dir::new();
    run(&mut first, "file.save_as", json!({ "path": dir.path(&format!("one.{ext}")) }));
    let written = dir.read(&format!("one.{ext}"));
    // A second save of the same drawing in the same session is no edit.
    run(&mut first, "file.save", json!({}));
    assert_eq!(dir.read(&format!("one.{ext}")), written, "{what}: a second save in the session changed the file");
    // A new session reads it and writes the same bytes, as a new file and over its own.
    let mut second = opened(&dir.path(&format!("one.{ext}")));
    run(&mut second, "file.save_as", json!({ "path": dir.path(&format!("two.{ext}")) }));
    assert_eq!(dir.read(&format!("two.{ext}")), written, "{what}: the save after a reopen is not the save");
    run(&mut second, "file.save", json!({}));
    assert_eq!(dir.read(&format!("two.{ext}")), written, "{what}: a save over the file after a reopen changed it");
    // And once more, through a third generation.
    let mut third = opened(&dir.path(&format!("two.{ext}")));
    run(&mut third, "file.save_as", json!({ "path": dir.path(&format!("three.{ext}")) }));
    assert_eq!(dir.read(&format!("three.{ext}")), written, "{what}: the third generation differs");
}

#[test]
fn a_foreign_save_as_equals_the_same_after_a_reopen() {
    // A drawing opened from a VSDX file, saved as `.diacraft`.
    let dir = Dir::new();
    let mut v = pinned();
    run(&mut v, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_drawing()) }));
    assert_the_save_after_a_reopen_is_the_save("a VSDX drawing", v, "diacraft");
    // A new blank drawing, and one a person made.
    assert_the_save_after_a_reopen_is_the_save("a blank drawing", pinned(), "diacraft");
    let mut m = pinned();
    made(&mut m);
    assert_the_save_after_a_reopen_is_the_save("a made drawing", m, "diacraft");
    // Each built-in template, as a new drawing.
    let listed = run(&mut blank_session(), "template.list", json!({}));
    for id in listed["templates"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_owned()) {
        let mut t = diacraft_engine::Session::new();
        t.pin_clock(Some(PINNED));
        run(&mut t, "file.new", json!({ "template": id }));
        assert_the_save_after_a_reopen_is_the_save(&format!("the template {id}"), t, "diacraft");
    }
    // A stencil and a template of DiaCraft's own.
    let mut st = pinned();
    drop_shape(&mut st, "rectangle", 2.0, 2.0);
    assert_the_save_after_a_reopen_is_the_save("a stencil", st, "diacraft-stencil");
    let mut tp = pinned();
    made(&mut tp);
    assert_the_save_after_a_reopen_is_the_save("a template", tp, "diacraft-template");
}

#[test]
fn an_edit_made_before_the_save_is_an_edit_to_the_foreign_file_after_the_reopen() {
    // The case the dirty flag cannot tell: edited after it was read from a VSDX file, saved as `.diacraft`, and clean in the session that opens that.
    let dir = Dir::new();
    let mut v = pinned();
    run(&mut v, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_drawing()) }));
    assert_eq!(v.content_unchanged(), Some(true));
    drop_shape(&mut v, "rectangle", 4.0, 4.0);
    assert_eq!(v.content_unchanged(), Some(false));
    run(&mut v, "file.save_as", json!({ "path": dir.path("x.diacraft") }));
    assert_eq!(v.content_unchanged(), Some(false), "a save moves no stamp");
    let t = opened(&dir.path("x.diacraft"));
    assert!(!t.is_dirty());
    assert_eq!(t.content_unchanged(), Some(false), "clean, and not what the VSDX file held");
    // The same drawing without the edit is what the VSDX file held, in any number of sessions.
    let mut u = pinned();
    run(&mut u, "file.open", json!({ "path": dir.path("a.vsdx") }));
    run(&mut u, "file.save_as", json!({ "path": dir.path("y.diacraft") }));
    assert_eq!(opened(&dir.path("y.diacraft")).content_unchanged(), Some(true));
    // An edit and its undo, saved, is as unedited as the file it came from.
    let mut w = pinned();
    run(&mut w, "file.open", json!({ "path": dir.path("a.vsdx") }));
    drop_shape(&mut w, "rectangle", 4.0, 4.0);
    run(&mut w, "edit.undo", json!({}));
    run(&mut w, "file.save_as", json!({ "path": dir.path("z.diacraft") }));
    let z = opened(&dir.path("z.diacraft"));
    assert_eq!(z.content_unchanged(), Some(true));
    assert!(z.document() == opened(&dir.path("y.diacraft")).document(), "the same content is the same drawing");
}

#[test]
fn the_pinned_clock_is_what_a_save_writes_and_the_system_clock_is_not_read() {
    let dir = Dir::new();
    let mut a = pinned();
    let mut b = pinned();
    run(&mut a, "file.save_as", json!({ "path": dir.path("a.vsdx") }));
    run(&mut b, "file.save_as", json!({ "path": dir.path("b.vsdx") }));
    assert_eq!(dir.read("a.vsdx"), dir.read("b.vsdx"), "two new drawings saved at the pinned time are the same file");
    assert_eq!(a.now(), Some(PINNED));
    a.pin_clock(None);
    let now = a.now().expect("the system's clock");
    assert!(now > PINNED - 400_000_000, "the system's clock is read again: {now}");
    assert_eq!(status(&mut b)["format"], "vsdx");
}

/// Whether a VSDX file has its thumbnail, and the `Application` its extended properties name.
fn bookkeeping_of(bytes: &[u8]) -> (bool, Option<String>) {
    let pkg = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
    let thumbnail = pkg.part("/docProps/thumbnail.emf").is_some();
    let app = pkg.part("/docProps/app.xml").map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
    let application = app.split_once("<Application>").and_then(|(_, rest)| rest.split_once("</Application>")).map(|(name, _)| name.to_owned());
    (thumbnail, application)
}

/// A VSDX file with a thumbnail and the name of the application that wrote it.
fn thumbnailed() -> Vec<u8> {
    use diacraft_testkit::package::{PK, PageSpec, Pkg, VsdxBuilder, cell, shape};
    let app = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>Some Writer</Application></Properties>";
    VsdxBuilder::new()
        .app(app)
        .page(PageSpec::new(1, "P").shapes(&shape(1, &cell("PinX", "1"))))
        .with(Pkg::new().part_without_override("/docProps/thumbnail.emf", b"emf-bytes").default_type("emf", "image/x-emf").rel(
            "/",
            "rId8",
            &format!("{PK}thumbnail"),
            "docProps/thumbnail.emf",
        ))
        .build()
}

#[test]
fn the_stamp_not_the_dirty_flag_decides_the_bookkeeping() {
    // The plan's test: a VSDX file with a thumbnail goes Save As `.diacraft`, opens in a new session (clean, whatever was done before) and goes Save As `.vsdx`. The
    // thumbnail is kept and `Application` is the file's while the drawing is the one the file held; after an edit and its undo the same holds; after an edit the thumbnail goes and the
    // application is DiaCraft (the design's 4.4).
    let dir = Dir::new();
    let source = thumbnailed();
    assert_eq!(bookkeeping_of(&source), (true, Some("Some Writer".to_owned())));
    let mut v = pinned();
    run(&mut v, "file.open", json!({ "path": dir.write("a.vsdx", &source) }));
    run(&mut v, "file.save_as", json!({ "path": dir.path("x.diacraft") }));
    let mut t = opened(&dir.path("x.diacraft"));
    run(&mut t, "file.save_as", json!({ "path": dir.path("b.vsdx") }));
    assert_eq!(bookkeeping_of(&dir.read("b.vsdx")), (true, Some("Some Writer".to_owned())), "the drawing is the file's: its bookkeeping is kept");
    let mut u = opened(&dir.path("x.diacraft"));
    drop_shape(&mut u, "rectangle", 4.0, 4.0);
    run(&mut u, "edit.undo", json!({}));
    run(&mut u, "file.save_as", json!({ "path": dir.path("c.vsdx") }));
    assert_eq!(bookkeeping_of(&dir.read("c.vsdx")), (true, Some("Some Writer".to_owned())), "an edit and its undo are no edit");
    let mut w = opened(&dir.path("x.diacraft"));
    drop_shape(&mut w, "rectangle", 4.0, 4.0);
    run(&mut w, "file.save_as", json!({ "path": dir.path("d.vsdx") }));
    assert_eq!(bookkeeping_of(&dir.read("d.vsdx")), (false, Some("DiaCraft".to_owned())), "an edit is an edited save");
    // The drawing edited before its `.diacraft` save is edited in the session that reads it, clean as that session is.
    let mut e = pinned();
    run(&mut e, "file.open", json!({ "path": dir.path("a.vsdx") }));
    drop_shape(&mut e, "rectangle", 4.0, 4.0);
    run(&mut e, "file.save_as", json!({ "path": dir.path("y.diacraft") }));
    let mut f = opened(&dir.path("y.diacraft"));
    assert!(!f.is_dirty());
    run(&mut f, "file.save_as", json!({ "path": dir.path("e.vsdx") }));
    assert_eq!(bookkeeping_of(&dir.read("e.vsdx")), (false, Some("DiaCraft".to_owned())), "clean in the session, and not what the VSDX file held");
}

#[test]
fn a_vsdx_save_from_the_model_equals_the_same_after_a_reopen() {
    // A drawing whose working file is a `.diacraft` one is saved as `.vsdx` from the model; a session that opens the `.diacraft` file writes the same bytes.
    let dir = Dir::new();
    let mut from_vsdx = pinned();
    run(&mut from_vsdx, "file.open", json!({ "path": dir.write("v.vsdx", &thumbnailed()) }));
    let mut made_one = pinned();
    made(&mut made_one);
    let cases = vec![("a VSDX drawing", from_vsdx), ("a blank drawing", pinned()), ("a made drawing", made_one)];
    for (n, (what, mut first)) in cases.into_iter().enumerate() {
        let native = dir.path(&format!("w{n}.diacraft"));
        run(&mut first, "file.save_as", json!({ "path": native }));
        run(&mut first, "file.save_as", json!({ "path": dir.path(&format!("one{n}.vsdx")) }));
        let mut second = opened(&native);
        run(&mut second, "file.save_as", json!({ "path": dir.path(&format!("two{n}.vsdx")) }));
        assert_eq!(dir.read(&format!("two{n}.vsdx")), dir.read(&format!("one{n}.vsdx")), "{what}: the VSDX save after a reopen is not the save");
    }
}

/// **An `.odg` written from the model equals the same after a reopen** (the design's 4.1: the path depends only on the working file, so a drawing writes the same file whether or
/// not it was reopened). A blank drawing, one a person made, each built-in template's drawing (which holds the VSDX skeleton the writer is given a copy without) and a drawing read from an
/// `.odg`, saved as `.odg` from the session, and after a `.diacraft` save and a reopen in a session of its own: the same bytes, with the clock pinned.
#[test]
fn an_odg_save_from_the_model_equals_the_same_after_a_reopen() {
    let dir = Dir::new();
    let mut from_odg = pinned();
    run(&mut from_odg, "file.open", json!({ "path": dir.write("o.odg", &crate::odg_build::Drawing::new().page("").bytes()) }));
    let mut made_one = pinned();
    made(&mut made_one);
    let mut cases = vec![("a blank drawing".to_owned(), pinned()), ("a made drawing".to_owned(), made_one), ("an .odg".to_owned(), from_odg)];
    let listed = run(&mut blank_session(), "template.list", json!({}));
    for id in listed["templates"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_owned()) {
        let mut t = diacraft_engine::Session::new();
        t.pin_clock(Some(PINNED));
        run(&mut t, "file.new", json!({ "template": id }));
        cases.push((format!("the template {id}"), t));
    }
    for (n, (what, mut first)) in cases.into_iter().enumerate() {
        let native = dir.path(&format!("r{n}.diacraft"));
        run(&mut first, "file.save_as", json!({ "path": native }));
        run(&mut first, "file.save_as", json!({ "path": dir.path(&format!("one{n}.odg")) }));
        let mut second = opened(&native);
        run(&mut second, "file.save_as", json!({ "path": dir.path(&format!("two{n}.odg")) }));
        assert!(dir.read(&format!("two{n}.odg")) == dir.read(&format!("one{n}.odg")), "{what}: the `.odg` save after a reopen is not the save");
    }
}

// ----------------------------------------------------------------------------------------: what a visit through `.diacraft` keeps

fn package_of(bytes: &[u8]) -> diacraft_opc::Package {
    diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap()
}

fn part_text(bytes: &[u8], name: &str) -> String {
    String::from_utf8(package_of(bytes).part(name).unwrap_or_else(|| panic!("no part {name}")).to_vec()).unwrap()
}

/// The rows from a part of a VSDX file, as `(type word after the last slash, the part it leads to)`, sorted.
fn rows_from(bytes: &[u8], source: &str) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = package_of(bytes)
        .rels(source)
        .unwrap()
        .items
        .iter()
        .map(|r| (r.ty.rsplit('/').next().unwrap_or("").to_owned(), r.target.clone().unwrap_or_else(|| r.target_raw.clone())))
        .collect();
    rows.sort();
    rows
}

/// A drawing whose document part has no `FaceNames`, and whose page "Front" has a row to a master no shape on it uses and one to a background page though it has no `BackPage`.
fn no_table_and_rows_of_its_own() -> Vec<u8> {
    use diacraft_testkit::package::{MasterSpec, PageSpec, VIS, VsdxBuilder, cell, shape};
    VsdxBuilder::new()
        .master(MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))))
        .page(PageSpec::new(1, "Back").attrs(r#" Background="1""#).shapes(&shape(1, &cell("PinX", "9"))))
        .page(PageSpec::new(2, "Front").shapes(&shape(1, &cell("PinX", "1"))).rel("rId5", &format!("{VIS}master"), "../masters/master1.xml").rel(
            "rId6",
            &format!("{VIS}page"),
            "page1.xml",
        ))
        .pkg()
        .map_parts(|name, bytes| {
            if name == "/visio/document.xml" {
                *bytes = String::from_utf8(bytes.clone()).unwrap().replace("<FaceNames></FaceNames>", "").into_bytes();
            }
        })
        .build()
}

#[test]
fn a_visit_through_diacraft_gives_no_font_table_to_a_file_that_had_none_and_keeps_every_row_of_its_pages() {
    // The full gate's sample-file report: 26 files with no `FaceNames` came back with an empty one, which the schema refuses, and rows to masters were not written in 7, on a visit that
    // edited nothing. The `.diacraft` file carries what the reader saw, and the content stamp says the drawing is the one the file held.
    let dir = Dir::new();
    let source = no_table_and_rows_of_its_own();
    let mut v = pinned();
    run(&mut v, "file.open", json!({ "path": dir.write("a.vsdx", &source) }));
    run(&mut v, "file.save_as", json!({ "path": dir.path("x.diacraft") }));
    let mut t = opened(&dir.path("x.diacraft"));
    run(&mut t, "file.save_as", json!({ "path": dir.path("b.vsdx") }));
    let written = dir.read("b.vsdx");
    let same = diacraft_vsdx::compare_content(&source, &written, &diacraft_vsdx::Limits::default()).unwrap();
    assert!(same.same(), "the visit changed the content: {:?}", same.shown);
    assert!(!part_text(&written, "/visio/document.xml").contains("FaceNames"), "{}", part_text(&written, "/visio/document.xml"));
    // After an edit, the rows the model does not explain are made from it: the page uses no master and has no background page.
    let mut w = opened(&dir.path("x.diacraft"));
    drop_shape(&mut w, "rectangle", 4.0, 4.0);
    run(&mut w, "file.save_as", json!({ "path": dir.path("c.vsdx") }));
    let edited = dir.read("c.vsdx");
    let had = [("master".to_owned(), "/visio/masters/master1.xml".to_owned()), ("page".to_owned(), "/visio/pages/page1.xml".to_owned())];
    assert_eq!(rows_from(&source, "/visio/pages/page2.xml"), had);
    assert!(rows_from(&edited, "/visio/pages/page2.xml").is_empty(), "{:?}", rows_from(&edited, "/visio/pages/page2.xml"));
    assert!(!part_text(&edited, "/visio/document.xml").contains("FaceNames"));
}

/// A template with a thumbnail, the name of the application that wrote it and the times it was made and last changed.
fn template_with_bookkeeping() -> Vec<u8> {
    use diacraft_testkit::package::{PK, PageSpec, Pkg, VsdxBuilder, cell, shape};
    let app = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>Some Writer</Application></Properties>";
    let core = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>T</dc:title><dcterms:created xsi:type=\"dcterms:W3CDTF\">2020-01-02T03:04:05Z</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">2021-02-03T04:05:06Z</dcterms:modified></cp:coreProperties>";
    VsdxBuilder::new()
        .document_type("application/vnd.ms-visio.template.main+xml") // brand-ok: a content type the format defines
        .app(app)
        .core(core)
        .page(PageSpec::new(1, "P").shapes(&shape(1, &cell("PinX", "1"))))
        .with(Pkg::new().part_without_override("/docProps/thumbnail.emf", b"emf-bytes").default_type("emf", "image/x-emf").rel(
            "/",
            "rId8",
            &format!("{PK}thumbnail"),
            "docProps/thumbnail.emf",
        ))
        .build()
}

#[test]
fn a_template_opened_as_itself_and_visited_through_diacraft_keeps_its_thumbnail_and_its_times() {
    // The design's 4.4 for the template types: the template file itself (`edit_template`) is read from a foreign file, so its content stamp is made, and an unedited visit writes the
    // thumbnail, `Application` and `modified` as they were read. (A template opened as a new drawing is a new drawing, with no stamp: its first foreign save is a new file's.)
    let dir = Dir::new();
    let source = template_with_bookkeeping();
    let modified = ">2021-02-03T04:05:06Z</dcterms:modified>";
    let mut v = pinned();
    run(&mut v, "file.open", json!({ "path": dir.write("a.vstx", &source), "edit_template": true }));
    run(&mut v, "file.save_as", json!({ "path": dir.path("x.diacraft-template") }));
    let mut t = pinned();
    run(&mut t, "file.open", json!({ "path": dir.path("x.diacraft-template"), "edit_template": true }));
    assert_eq!(t.content_unchanged(), Some(true));
    run(&mut t, "file.save_as", json!({ "path": dir.path("b.vstx") }));
    let written = dir.read("b.vstx");
    assert_eq!(bookkeeping_of(&written), (true, Some("Some Writer".to_owned())), "the thumbnail and the application are the file's");
    assert!(part_text(&written, "/docProps/core.xml").contains(modified), "{}", part_text(&written, "/docProps/core.xml"));
    let same = diacraft_vsdx::compare_content(&source, &written, &diacraft_vsdx::Limits::default()).unwrap();
    assert!(same.same(), "the visit changed the content: {:?}", same.shown);
    // An edit is an edited save: the thumbnail goes, and the time is the save's.
    let mut e = pinned();
    run(&mut e, "file.open", json!({ "path": dir.path("x.diacraft-template"), "edit_template": true }));
    drop_shape(&mut e, "rectangle", 4.0, 4.0);
    run(&mut e, "file.save_as", json!({ "path": dir.path("c.vstx") }));
    let edited = dir.read("c.vstx");
    assert_eq!(bookkeeping_of(&edited), (false, Some("DiaCraft".to_owned())));
    assert!(!part_text(&edited, "/docProps/core.xml").contains(modified), "{}", part_text(&edited, "/docProps/core.xml"));
}
