//! The window shows the save note (the native-format design's 5.6).
//!
//! The note is **the engine's words** for the save result's `carried` field (`diacraft_engine::across::words`): the window keeps no copy of them and never reads a warning. Each
//! test saves a real file made in code, and the oracle is a second session that makes the same saves and gives the `carried` field the engine answered; the note on the screen is
//! `words` of that.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use diacraft_container::zip::{ArchiveWriter, Method};
use diacraft_engine::Session;
use diacraft_engine::across::{Carried, Target, words};
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder, shape};
use diacraft_ui_egui::render_worker::bundled_drawer;
use diacraft_ui_egui::{DiacraftApp, SavePicker};
use egui::Key;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};

use crate::editing::key;
use crate::fixture;
use crate::saving::{ctrl, ctrl_shift};
use crate::support::{Rest, TempDir, Window, fast_window, press};

const VBA_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";
const SIG: &str = "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/";
/// The page's size: a drawing saved as an `.odg` is made for a page that has one.
const PAGE_SIZE: &str = r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#;
const DRAWING_WITH_MACROS: &str = "application/vnd.ms-visio.drawing.macroEnabled.main+xml"; // brand-ok: a content type the format defines

// ------------------------------------------------------------------------------------------------------------------------------- the files

const ODG: &str = "application/vnd.oasis.opendocument.graphics";
const NS: &str = concat!(
    r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
    r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
    r#"xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" "#,
    r#"xmlns:script="urn:oasis:names:tc:opendocument:xmlns:script:1.0""#
);
const LAYERS: &str = r#"<draw:layer draw:name="layout"/><draw:layer draw:name="background"/><draw:layer draw:name="backgroundobjects"/><draw:layer draw:name="controls"/><draw:layer draw:name="measurelines"/>"#;
const GR1: &str = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#3465a4" draw:stroke="solid" svg:stroke-width="0.05cm" svg:stroke-color="#000000"/></style:style>"##;
const RECT: &str = r#"<draw:rect draw:style-name="gr1" draw:layer="layout" draw:name="R" svg:x="2cm" svg:y="2cm" svg:width="4cm" svg:height="2cm"/>"#;

/// An `.odg` of one rectangle, with `common` among its common styles and `shapes` after the rectangle, and each of `files` in its package.
fn odg(common: &str, shapes: &str, files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content {NS} office:version=\"1.3\"><office:font-face-decls/><office:automatic-styles>{GR1}</office:automatic-styles><office:body><office:drawing><draw:page draw:name=\"page1\" draw:master-page-name=\"Default\">{RECT}{shapes}</draw:page></office:drawing></office:body></office:document-content>"
    );
    let styles = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles {NS} office:version=\"1.3\"><office:font-face-decls/><office:styles>{common}</office:styles><office:automatic-styles><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:margin-top=\"1cm\" fo:margin-bottom=\"1cm\" fo:margin-left=\"1cm\" fo:margin-right=\"1cm\" fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" style:print-orientation=\"portrait\"/></style:page-layout></office:automatic-styles><office:master-styles><draw:layer-set>{LAYERS}</draw:layer-set><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles></office:document-styles>"
    );
    let meta = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta {NS} office:version=\"1.3\"><office:meta><meta:generator>TestWriter/1</meta:generator></office:meta></office:document-meta>"
    );
    let mut parts: Vec<(String, Vec<u8>)> =
        vec![("content.xml".into(), content.into_bytes()), ("styles.xml".into(), styles.into_bytes()), ("meta.xml".into(), meta.into_bytes())];
    parts.extend(files.iter().map(|(name, bytes)| ((*name).to_owned(), bytes.clone())));
    let mut manifest = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\">\n",
    );
    manifest.push_str(&format!(" <manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"{ODG}\"/>\n"));
    for (name, _) in &parts {
        let media = if name.ends_with(".png") { "image/png" } else { "text/xml" };
        manifest.push_str(&format!(" <manifest:file-entry manifest:full-path=\"{name}\" manifest:media-type=\"{media}\"/>\n"));
    }
    manifest.push_str("</manifest:manifest>\n");
    let mut w = ArchiveWriter::new();
    w.add("mimetype", ODG.as_bytes(), Method::Stored).unwrap();
    for (name, data) in &parts {
        w.add(name, data, if name.ends_with(".png") { Method::Stored } else { Method::Deflated }).unwrap();
    }
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).unwrap();
    w.finish().unwrap()
}

/// What an `.odg` holds that a VSDX file has no place for: a line end, a gradient and a hatch.
const DEFINITIONS: &str = concat!(
    r##"<draw:gradient draw:name="g1" draw:style="linear" draw:start-color="#729fcf" draw:end-color="#ffffff" draw:angle="0deg" draw:border="0%"/>"##,
    r##"<draw:hatch draw:name="h1" draw:style="single" draw:color="#3465a4" draw:distance="0.1cm" draw:rotation="0"/>"##,
    r#"<draw:marker draw:name="m1" svg:viewBox="0 0 10 10" svg:d="M0 0h10v10z"/>"#
);

/// An `.odg` with definitions the VSDX types have no place for.
fn odg_with_definitions() -> Vec<u8> {
    odg(DEFINITIONS, "", &[])
}

/// An `.odg` with a macro project (two parts, never run).
fn odg_with_macros() -> Vec<u8> {
    odg("", "", &[("Basic/Standard/Module1.xml", b"<script/>".to_vec()), ("Basic/script-lc.xml", b"<libraries/>".to_vec())])
}

/// An `.odg` whose second shape is an embedded sub-document (a chart's own folder), which a VSDX file has no embedding for.
fn odg_with_a_subdocument() -> Vec<u8> {
    let frame = concat!(
        r#"<draw:frame draw:style-name="gr1" draw:layer="layout" draw:name="A" svg:width="4cm" svg:height="2cm" svg:x="8cm" svg:y="2cm">"#,
        r#"<draw:object xlink:href="./Object 1" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"#,
        r#"<draw:image xlink:href="./ObjectReplacements/Object 1" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/></draw:frame>"#
    );
    let mut picture = Vec::new();
    let mut encoder = png::Encoder::new(&mut picture, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&[10; 16]).unwrap();
    odg(
        "",
        frame,
        &[("Object 1/content.xml", b"<chart/>".to_vec()), ("Object 1/styles.xml", b"<styles/>".to_vec()), ("ObjectReplacements/Object 1", picture)],
    )
}

/// An `.odg` with all three of what a note tells besides classes: a macro project, an embedded sub-document and a digital signature.
fn odg_with_everything() -> Vec<u8> {
    let frame = concat!(
        r#"<draw:frame draw:style-name="gr1" draw:layer="layout" draw:name="A" svg:width="4cm" svg:height="2cm" svg:x="8cm" svg:y="2cm">"#,
        r#"<draw:object xlink:href="./Object 1" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/></draw:frame>"#
    );
    odg(
        DEFINITIONS,
        frame,
        &[
            ("Basic/Standard/Module1.xml", b"<script/>".to_vec()),
            ("Object 1/content.xml", b"<chart/>".to_vec()),
            ("META-INF/documentsignatures.xml", b"<sig/>".to_vec()),
        ],
    )
}

/// A `.vsdx` whose one shape holds shape data (a `Property` row), which an `.odg` has no place for and a LibreOffice save would drop: the ODF decision-8 note and the save note both apply.
fn vsdx_with_shape_data() -> Vec<u8> {
    let data = r#"<Section N="Property"><Row N="Cost"><Cell N="Value" V="5"/><Cell N="Label" V="Cost" U="STR"/></Row></Section>"#;
    let one = shape(1, &[fixture::placed(2.0, 5.5, 2.0, 1.0), fixture::rectangle(2.0, 1.0), data.to_owned()].concat());
    VsdxBuilder::new().page(PageSpec::new(1, "Plan").sheet(PAGE_SIZE).shapes(&one)).build()
}

/// A `.vsdm` with a VBA project beside one rectangle.
fn vsdm_with_vba() -> Vec<u8> {
    let one = shape(1, &[fixture::placed(2.0, 5.5, 2.0, 1.0), fixture::rectangle(2.0, 1.0)].concat());
    VsdxBuilder::new()
        .document_type(DRAWING_WITH_MACROS)
        .page(PageSpec::new(1, "Plan").sheet(PAGE_SIZE).shapes(&one))
        .with(Pkg::new().default_type("bin", "application/vnd.ms-office.vbaProject").part_without_override("/visio/vbaProject.bin", b"not code").rel(
            "/visio/document.xml",
            "rId50",
            VBA_REL,
            "vbaProject.bin",
        ))
        .build()
}

/// A `.vsdx` with a digital signature (the package's signature origin and one signature), which no longer matches a file written from the model.
fn signed_vsdx() -> Vec<u8> {
    let one = shape(1, &[fixture::placed(2.0, 5.5, 2.0, 1.0), fixture::rectangle(2.0, 1.0)].concat());
    VsdxBuilder::new()
        .page(PageSpec::new(1, "Plan").sheet(PAGE_SIZE).shapes(&one))
        .with(
            Pkg::new()
                .default_type("sigs", "application/vnd.openxmlformats-package.digital-signature-origin")
                .part_without_override("/_xmlsignatures/origin.sigs", [4u8])
                .rel("/", "rId12", &format!("{SIG}origin"), "_xmlsignatures/origin.sigs")
                .part("/_xmlsignatures/sig1.xml", "application/vnd.openxmlformats-package.digital-signature-xmlsignature+xml", "<Signature/>")
                .rel("/_xmlsignatures/origin.sigs", "rId1", &format!("{SIG}signature"), "sig1.xml"),
        )
        .build()
}

// ------------------------------------------------------------------------------------------------------------------------------- the window

/// A Save dialog that answers each path it is given in turn (the first first), and none when they are used up.
fn picker_answering(paths: Vec<PathBuf>) -> SavePicker {
    let queue = Arc::new(std::sync::Mutex::new(paths.into_iter().collect::<std::collections::VecDeque<_>>()));
    Arc::new(move |_| queue.lock().unwrap().pop_front())
}

/// The window on the file `name` of `dir` holding `bytes`, its Save dialog answering `answers` in turn, `size` points wide and high.
fn window_of(dir: &TempDir, name: &str, bytes: &[u8], answers: Vec<PathBuf>, size: [f32; 2]) -> Window {
    let path = fixture::write(dir.path(), name, bytes);
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let app = DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()).with_save_picker(picker_answering(answers));
    let mut w = fast_window(app, size);
    w.rest();
    w
}

fn save_as(w: &mut Window) {
    key(w, Key::S, ctrl_shift());
    w.rest();
}

/// What the engine says the saves of `source` carry, one `carried` field for each of `targets`, made by a session of its own that opens the file and saves it as each in turn: the
/// same commands the window runs, so the same answers.
fn engine_carried(source: &Path, targets: &[&Path]) -> Vec<Carried> {
    let mut s = Session::new();
    s.execute("file.open", &json!({"path": source.to_str().unwrap()})).unwrap();
    targets
        .iter()
        .map(|to| {
            let out: Value = s.execute("file.save_as", &json!({"path": to.to_str().unwrap()})).unwrap_or_else(|e| panic!("{}: {e}", to.display()));
            Carried::from_json(&out["carried"]).unwrap_or_else(|| panic!("a save result has a `carried` field: {out}"))
        })
        .collect()
}

/// The note on the screen, as the window keeps it.
fn note(w: &Window) -> Vec<String> {
    w.state().save_note().map(<[String]>::to_vec).unwrap_or_default()
}

/// How many labels on the screen hold `text`.
fn shown(w: &Window, text: &str) -> usize {
    w.query_all_by_label_contains(text).count()
}

// ------------------------------------------------------------------------------------------------------------------------------- the plan's two

/// §5.6: a save across the formats that carries data is told **in the engine's words, once for the save**: the window reads the structured `carried` field (never a warning), draws
/// the note once however many frames follow, the person dismisses it, and a later save with nothing to say takes none away or brings none back.
#[test]
fn the_note_is_shown_in_words_once_per_save() {
    let dir = TempDir::new("note-once");
    let (vsdx, odg_out) = (dir.file("out.vsdx"), dir.file("out.odg"));
    let mut w = window_of(&dir, "plan.odg", &odg_with_definitions(), vec![vsdx.clone(), odg_out.clone()], [1280.0, 900.0]);
    assert!(w.state().save_note().is_none(), "nothing before a save");
    save_as(&mut w);
    assert!(vsdx.exists(), "the file was written");
    let expected = words(Target::Vsdx, &engine_carried(&dir.file("plan.odg"), &[&dir.file("oracle.vsdx")])[0]);
    assert_eq!(expected.len(), 1, "one part: data a VSDX file has no place for");
    assert!(expected[0].starts_with("This drawing has data a VSDX file has no place for (line ends, gradients, hatches"), "{}", expected[0]);
    assert_eq!(note(&w), expected, "the note is the engine's words for the `carried` field, exactly");
    // On the screen, once: more frames draw the same card and add none.
    assert_eq!(shown(&w, &expected[0]), 1);
    for _ in 0..3 {
        w.step();
    }
    assert_eq!(shown(&w, &expected[0]), 1, "still one card");
    // It is the note, and not the error banner or the status bar's notice: they say what happened to the save, and this says what the file carries.
    assert!(w.state().open_error().is_none(), "{:?}", w.state().open_error());
    assert!(w.state().notice().is_some_and(|n| n.starts_with("Saved out.vsdx.") && !n.contains("no place for")), "{:?}", w.state().notice());
    // Dismissed, it is gone, and a frame does not bring it back.
    press(&mut w, "save-note.dismiss");
    assert!(w.state().save_note().is_none() && shown(&w, &expected[0]) == 0);
    w.step();
    assert!(w.state().save_note().is_none(), "dismissed for good");
    // A save into the file has nothing to say.
    key(&mut w, Key::S, ctrl());
    w.rest();
    assert!(w.state().save_note().is_none(), "Save keeps the file's own type: no note");
    // The next save across says its own: the words are the OpenDocument ones, for what that save carries (the oracle makes the same two saves).
    save_as(&mut w);
    assert!(odg_out.exists());
    let second = words(Target::Odg, &engine_carried(&dir.file("plan.odg"), &[&dir.file("oracle1.vsdx"), &dir.file("oracle2.odg")])[1]);
    assert!(!second.is_empty() && second[0].starts_with("This drawing has data an OpenDocument drawing has no place for"), "{second:?}");
    assert_eq!(note(&w), second, "the note of this save, and not the last one");
    for part in &second {
        assert_eq!(shown(&w, part), 1, "each part once: {part}");
    }
    assert_eq!(shown(&w, "a VSDX file has no place for"), 0, "nothing of the last note is left");
}

/// With nothing carried there is **no note**: a save into a file of the same format, a `.diacraft` (which holds everything), and a new drawing as an `.odg`, whose `carried` is empty.
#[test]
fn no_note_when_carried_is_empty() {
    let dir = TempDir::new("note-none");
    // The control: a save that carries data shows a note, and the save that follows it with nothing carried takes it away and brings none.
    let answers = vec![dir.file("first.vsdx"), dir.file("second.diacraft")];
    let mut c = window_of(&dir, "defs.odg", &odg_with_definitions(), answers, [1280.0, 900.0]);
    save_as(&mut c);
    assert!(!note(&c).is_empty(), "the save that carries data has a note");
    save_as(&mut c);
    assert!(dir.file("second.diacraft").exists());
    assert!(engine_carried(&dir.file("defs.odg"), &[&dir.file("o1.diacraft")])[0].is_empty(), "the engine says a .diacraft carries nothing");
    assert!(
        c.state().save_note().is_none() && c.query_by_label_contains("no place for").is_none(),
        "a .diacraft carries nothing: the last note is gone"
    );
    // A VSDX drawing saved as VSDX: the carried field is empty.
    let mut w = window_of(&dir, "plan.vsdx", &fixture::labelled_vsdx(), vec![dir.file("copy.vsdx"), dir.file("copy.diacraft")], [1280.0, 900.0]);
    assert!(engine_carried(&dir.file("plan.vsdx"), &[&dir.file("oracle.vsdx")])[0].is_empty(), "the engine says nothing is carried");
    save_as(&mut w);
    assert!(dir.file("copy.vsdx").exists());
    assert!(w.state().save_note().is_none(), "the same format: nothing carried, no note");
    assert!(w.query_by_label_contains("no place for").is_none());
    // A `.diacraft` holds the whole model.
    save_as(&mut w);
    assert!(dir.file("copy.diacraft").exists());
    assert!(w.state().save_note().is_none(), ".diacraft carries nothing");
    // An `.odg` of definitions saved as `.diacraft`: the same.
    let mut o = window_of(&dir, "defs.odg", &odg_with_definitions(), vec![dir.file("defs.diacraft")], [1280.0, 900.0]);
    save_as(&mut o);
    assert!(dir.file("defs.diacraft").exists());
    assert!(o.state().save_note().is_none(), "what an `.odg` holds is the model's: nothing is carried into a .diacraft");
    // A new drawing saved as an `.odg`.
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let app = DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()).with_save_picker(picker_answering(vec![dir.file("new.odg")]));
    let mut n = fast_window(app, [1280.0, 900.0]);
    n.rest();
    save_as(&mut n);
    assert!(dir.file("new.odg").exists());
    assert!(n.state().save_note().is_none(), "a new drawing has nothing the `.odg` cannot say");
}

// ------------------------------------------------------------------------------------------------------------------------------- each part of the note

/// The note on the screen after one Save As of `bytes` (a file called `name`) to `to`: the parts, which are the engine's words for the `carried` field of that save (`target` says which).
fn note_of(dir: &TempDir, name: &str, bytes: &[u8], to: &str, target: Target) -> (Window, Carried, Vec<String>) {
    let mut w = window_of(dir, name, bytes, vec![dir.file(to)], [1280.0, 900.0]);
    let carried = engine_carried(&dir.file(name), &[&dir.file(&format!("oracle-{to}"))]).remove(0);
    save_as(&mut w);
    assert!(dir.file(to).exists(), "{to} was written");
    // The engine's words for what the save carried, and after them its words for the check (a drawing written across the formats has the values group at least): a second session says them.
    let expected = [words(target, &carried), engine_check_words(&dir.file(name), &dir.file(&format!("oracle-check-{to}")))].concat();
    assert_eq!(note(&w), expected, "the note is the engine's words for what the save carried and for what its check found: {carried:?}");
    for part in &expected {
        assert_eq!(shown(&w, part), 1, "each part is on the screen once: {part}");
    }
    (w, carried, expected)
}

/// **Part one, in a VSDX file:** data the type has no place for, with its classes in plain words, and that another program that saves the file may leave it out.
#[test]
fn part_one_names_the_classes_a_vsdx_file_carries() {
    let dir = TempDir::new("note-p1-vsdx");
    let (w, carried, parts) = note_of(&dir, "plan.odg", &odg_with_definitions(), "out.vsdx", Target::Vsdx);
    assert_eq!(parts.len(), 1);
    for class in ["line ends", "gradients", "hatches", "layer settings", "style names", "document properties"] {
        assert!(carried.classes.iter().any(|c| c == class) && parts[0].contains(class), "{class}: {}", parts[0]);
    }
    assert!(parts[0].starts_with("This drawing has data a VSDX file has no place for ("), "{}", parts[0]);
    assert!(
        parts[0].ends_with("DiaCraft keeps it inside the file and reads it back, but another program that saves the file may leave it out."),
        "{}",
        parts[0]
    );
    assert!(w.query_by_label_contains("macro project").is_none() && w.query_by_label_contains("digital signature").is_none(), "no other part");
}

/// **Part one, in an `.odg`:** the same, with what another program may do with the data as it was measured (a LibreOffice save kept DiaCraft's own entries, so the note says
/// another program may keep or leave out the data, and that its changes win where DiaCraft can tell).
#[test]
fn part_one_says_what_another_program_may_do_with_the_data_of_an_odg() {
    let dir = TempDir::new("note-p1-odg");
    let (_w, carried, parts) = note_of(&dir, "plan.vsdx", &fixture::labelled_vsdx(), "out.odg", Target::Odg);
    assert!(carried.classes.iter().any(|c| c == "shape data") && carried.classes.iter().any(|c| c == "document data"), "{carried:?}");
    // The note, and after it the check's one line for this drawing: the values an `.odg` holds in place of the VSDX file's.
    assert_eq!(parts.len(), 2, "{parts:?}");
    assert!(parts[1].starts_with("Written as values the file format holds:"), "{}", parts[1]);
    assert!(parts[0].starts_with("This drawing has data an OpenDocument drawing has no place for (shape data"), "{}", parts[0]);
    assert!(
        parts[0].ends_with(
            "DiaCraft keeps it inside the file and reads it back. Another program that saves the file may keep this data or leave it out, and what that program changes in the drawing wins where DiaCraft can tell."
        ),
        "{}",
        parts[0]
    );
}

/// **Part two:** a macro project that the other format's programs may not show, in each direction, and that DiaCraft never runs macros.
#[test]
fn part_two_tells_of_a_macro_project_in_each_direction() {
    let dir = TempDir::new("note-p2-macros");
    // A `.vsdm` saved as an `.odg`: the project rides in the file as bytes.
    let (_w, carried, parts) = note_of(&dir, "plan.vsdm", &vsdm_with_vba(), "out.odg", Target::Odg);
    assert!(carried.macros && !carried.objects, "{carried:?}");
    assert_eq!(
        parts.last().map(String::as_str),
        Some(
            "This file carries a macro project that OpenDocument programs may not show. DiaCraft keeps them so that they come back when the file is opened in DiaCraft. DiaCraft never runs macros."
        )
    );
    // An `.odg` with a macro project saved as a macro-enabled VSDX type.
    let (_w, carried, parts) = note_of(&dir, "plan.odg", &odg_with_macros(), "out.vsdm", Target::Vsdx);
    assert!(carried.macros && !carried.objects, "{carried:?}");
    assert_eq!(
        parts.last().map(String::as_str),
        Some(
            "This file carries a macro project that VSDX programs may not show. DiaCraft keeps them so that they come back when the file is opened in DiaCraft. DiaCraft never runs macros."
        )
    );
}

/// **Part two, objects:** embedded objects that ride in the file, and (with a macro project too) both in one sentence.
#[test]
fn part_two_tells_of_embedded_objects_and_of_both() {
    let dir = TempDir::new("note-p2-objects");
    let (_w, carried, parts) = note_of(&dir, "plan.odg", &odg_with_a_subdocument(), "out.vsdx", Target::Vsdx);
    assert!(carried.objects && !carried.macros, "{carried:?}");
    assert_eq!(
        parts.last().map(String::as_str),
        Some(
            "This file carries embedded objects that VSDX programs may not show. DiaCraft keeps them so that they come back when the file is opened in DiaCraft. DiaCraft never runs macros."
        )
    );
    let (_w, carried, parts) = note_of(&dir, "both.odg", &odg_with_everything(), "both.vsdm", Target::Vsdx);
    assert!(carried.objects && carried.macros, "{carried:?}");
    assert!(
        parts.iter().any(|p| p.starts_with("This file carries a macro project and embedded objects that VSDX programs may not show.")),
        "{parts:?}"
    );
}

/// **Part three:** the digital signatures that no longer match the file. A signed VSDX drawing that went through `.diacraft` and is saved as VSDX again has the signature part, and says so.
#[test]
fn part_three_counts_the_signatures_that_no_longer_match() {
    let dir = TempDir::new("note-p3");
    let mut w = window_of(&dir, "signed.vsdx", &signed_vsdx(), vec![dir.file("visit.diacraft"), dir.file("back.vsdx")], [1280.0, 900.0]);
    save_as(&mut w);
    assert!(w.state().save_note().is_none(), "into .diacraft: nothing is carried and no signature is written");
    let carried = engine_carried(&dir.file("signed.vsdx"), &[&dir.file("o.diacraft"), &dir.file("o.vsdx")]).remove(1);
    assert_eq!(carried.signatures_stale, 1, "{carried:?}");
    save_as(&mut w);
    assert!(dir.file("back.vsdx").exists());
    assert_eq!(note(&w), ["1 digital signature(s) in this file no longer match it."], "the whole note, and the design's words");
    assert_eq!(shown(&w, "1 digital signature(s) in this file no longer match it."), 1);
}

/// All three parts, in the design's order, each once.
#[test]
fn the_three_parts_come_in_the_designs_order() {
    let dir = TempDir::new("note-all");
    let (_w, carried, parts) = note_of(&dir, "all.odg", &odg_with_everything(), "all.vsdm", Target::Vsdx);
    assert!(!carried.classes.is_empty() && carried.macros && carried.objects && carried.signatures_stale == 1, "{carried:?}");
    assert_eq!(parts.len(), 3);
    assert!(parts[0].starts_with("This drawing has data a VSDX file has no place for ("), "{}", parts[0]);
    assert!(parts[1].starts_with("This file carries a macro project and embedded objects"), "{}", parts[1]);
    assert_eq!(parts[2], "1 digital signature(s) in this file no longer match it.");
}

// ------------------------------------------------------------------------------------------------------------------------------- where it is

/// The note is about the drawing's file: a new drawing, or another file opened, takes it away.
#[test]
fn a_new_drawing_takes_the_note_away() {
    let dir = TempDir::new("note-new");
    let mut w = window_of(&dir, "plan.odg", &odg_with_definitions(), vec![dir.file("out.vsdx")], [1280.0, 900.0]);
    save_as(&mut w);
    assert!(w.state().save_note().is_some());
    crate::support::run_command(&mut w, "file.new", json!({}));
    assert!(w.state().save_note().is_none() && w.query_by_label_contains("no place for").is_none(), "the drawing it was about is gone");
}

/// The note never blocks: the save is done before it shows, the window takes keys and the pointer under it (it is no modal), and it can stay while the person works.
#[test]
fn the_note_blocks_nothing() {
    let dir = TempDir::new("note-nonmodal");
    let mut w = window_of(&dir, "plan.odg", &odg_with_definitions(), vec![dir.file("out.vsdx")], [1280.0, 900.0]);
    save_as(&mut w);
    assert!(w.state().save_note().is_some());
    assert!(!w.state().is_modal_open(), "the note is no question");
    assert!(!w.state().is_dirty(), "the save was done");
    crate::support::run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 0.5, "dy": 0.0}));
    assert!(w.state().save_note().is_some(), "work goes on under it");
}

/// **The whole note is on the screen**, and not the first lines of it: where the window has the room, every part's words are inside the card, one under the other, and the card is inside the window,
/// at narrow and wide sizes. (A card that was sized by a scroll area's first frame showed four lines of a note of three parts, which the first screenshot of it found.)
#[test]
fn the_whole_note_is_on_the_screen_at_narrow_and_wide_windows() {
    for size in [[640.0, 700.0], [1024.0, 700.0], [1280.0, 800.0], [1920.0, 1080.0]] {
        let dir = TempDir::new("note-fit");
        let mut w = window_of(&dir, "all.odg", &odg_with_everything(), vec![dir.file("all.vsdm")], size);
        save_as(&mut w);
        let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
        let card = w.state().element("save-note").unwrap_or_else(|| panic!("{size:?}: the card is not drawn")).rect;
        assert!(window.contains_rect(card), "{size:?}: the card {card:?} is outside the window {window:?}");
        let dismiss = w.state().element("save-note.dismiss").unwrap_or_else(|| panic!("{size:?}: no dismiss button")).rect;
        assert!(card.contains_rect(dismiss) && window.contains_rect(dismiss), "{size:?}: the dismiss button {dismiss:?} is not on the card {card:?}");
        // The words of each part sit inside the card, and a part does not overlap the one before it.
        let mut last_bottom = f32::MIN;
        for part in note(&w) {
            let rect = w.get_by_label_contains(&part[..40]).rect();
            assert!(rect.min.y >= last_bottom - 0.5, "{size:?}: a part overlaps the one before it ({rect:?} after {last_bottom})");
            assert!(card.contains_rect(rect.shrink(0.5)), "{size:?}: the words {rect:?} are not all inside the card {card:?}");
            last_bottom = rect.max.y;
        }
        // Dismissing leaves the drawing as it was.
        press(&mut w, "save-note.dismiss");
        assert!(w.state().save_note().is_none(), "{size:?}");
    }
}

/// The note of ODF decision 8 (what a LibreOffice save would drop, once in a session, in the banner) and the save note are two things, and both show: the card stands under the banner, and the
/// banner is dismissed without taking the card.
#[test]
fn the_card_stands_under_the_banner_when_both_are_up() {
    let dir = TempDir::new("note-banner");
    let mut w = window_of(&dir, "data.vsdx", &vsdx_with_shape_data(), vec![dir.file("out.odg")], [1280.0, 900.0]);
    save_as(&mut w);
    let banner = w.state().open_error().map(str::to_owned).expect("the decision-8 note is in the banner");
    assert!(banner.contains("a LibreOffice save would drop"), "{banner}");
    let parts = note(&w);
    assert!(!parts.is_empty() && parts.iter().all(|p| !p.contains("LibreOffice save would drop")), "the two notes say their own: {parts:?}");
    let card = w.state().element("save-note").expect("the card is drawn").rect;
    let dismiss = w.state().element("open-error.dismiss").expect("the banner is drawn").rect;
    assert!(card.min.y > dismiss.max.y, "the card {card:?} is under the banner, whose button is at {dismiss:?}");
    press(&mut w, "open-error.dismiss");
    assert!(w.state().open_error().is_none() && w.state().save_note().is_some(), "the banner goes, the card stays");
    let card = w.state().element("save-note").expect("the card is still drawn").rect;
    assert!(card.min.y < dismiss.max.y, "and the card moves up to the top: {card:?}");
}

/// **A note taller than the room the window has scrolls, and every part can be reached**: the card stays inside the window (and so over the drawing area), its words are cut at its bottom edge, and
/// the wheel over it brings the last part into the card.
#[test]
fn a_note_taller_than_the_window_scrolls_and_the_last_part_can_be_reached() {
    let dir = TempDir::new("note-scrolls");
    let size = [560.0, 420.0];
    let mut w = window_of(&dir, "all.odg", &odg_with_everything(), vec![dir.file("all.vsdm")], size);
    save_as(&mut w);
    let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
    let card = w.state().element("save-note").expect("the card is drawn").rect;
    assert!(window.contains_rect(card), "the card {card:?} is outside the window {window:?}");
    let parts = note(&w);
    let last = parts.last().expect("a note of three parts").clone();
    let before = w.get_by_label_contains(&last[..30]).rect();
    assert!(before.max.y > card.max.y, "the last part is below the card, so it needs the wheel: {before:?} against {card:?}");
    crate::support::move_to(&mut w, card.center());
    for _ in 0..30 {
        w.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -60.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
            source: egui::MouseWheelSource::Unknown,
        });
        w.step();
    }
    let after = w.get_by_label_contains(&last[..30]).rect();
    assert!(after.max.y <= card.max.y + 0.5 && after.min.y >= card.min.y, "the last part is in the card after the wheel: {after:?} in {card:?}");
    assert_eq!(w.state().element("save-note").unwrap().rect, card, "the card is as it was, and only its words moved");
}

/// **The whole note at the scale of a real screen**: the window's own `ui.screenshot` of the first version showed four lines of a note of three parts, and the harness at a scale of 1 did not
/// (the card sized by a scroll area that is in a row has a height of its own to find). A window at 1.15 points to the pixel, which is a common screen, shows each part whole, one under the
/// other, inside the card.
#[test]
fn the_whole_note_is_on_the_screen_on_a_scaled_screen() {
    for scale in [1.0_f32, 1.15, 1.5, 2.0] {
        let dir = TempDir::new("note-scaled");
        let path = fixture::write(dir.path(), "all.odg", &odg_with_everything());
        let mut session = Session::new();
        session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
        let app = DiacraftApp::new(session, None)
            .with_inline_pictures(bundled_drawer())
            .with_clock(diacraft_ui_egui::clock::Clock::manual())
            .with_save_picker(picker_answering(vec![dir.file("all.vsdm")]));
        let mut w =
            egui_kittest::Harness::builder().with_size([1280.0, 800.0]).with_pixels_per_point(scale).with_step_dt(1.0 / 60.0).build_eframe(|_| app);
        w.rest();
        save_as(&mut w);
        let card = w.state().element("save-note").unwrap_or_else(|| panic!("scale {scale}: the card is not drawn")).rect;
        let mut last_bottom = f32::MIN;
        for part in note(&w) {
            let words = w.get_by_label_contains(&part[..40]).rect();
            assert!(words.min.y >= last_bottom - 0.5, "scale {scale}: a part overlaps the one before it ({words:?} after {last_bottom})");
            assert!(card.contains_rect(words.shrink(0.5)), "scale {scale}: the words {words:?} are not all inside the card {card:?}");
            last_bottom = words.max.y;
        }
    }
}

/// **A second note is shown whole as well**: the card of one save is drawn again for the next, which is longer, and it grows to hold it (a card that kept the last one's height showed the first
/// lines of the second). A signed VSDX drawing goes through `.diacraft` to VSDX (a note of one line), and then to an `.odg` (a note of several).
#[test]
fn a_longer_note_after_a_shorter_one_is_shown_whole() {
    let dir = TempDir::new("note-grows");
    let answers = vec![dir.file("visit.diacraft"), dir.file("back.vsdx"), dir.file("out.odg")];
    let mut w = window_of(&dir, "signed.vsdx", &signed_vsdx(), answers, [1280.0, 800.0]);
    save_as(&mut w);
    save_as(&mut w);
    assert_eq!(note(&w), ["1 digital signature(s) in this file no longer match it."], "a short note first");
    let short = w.state().element("save-note").unwrap().rect;
    save_as(&mut w);
    let longer = note(&w);
    assert!(!longer.is_empty() && longer.iter().all(|p| !p.contains("digital signature")), "{longer:?}");
    let card = w.state().element("save-note").unwrap().rect;
    assert!(card.height() > short.height(), "{card:?} against {short:?}");
    for part in &longer {
        let words = w.get_by_label_contains(&part[..40]).rect();
        assert!(card.contains_rect(words.shrink(0.5)), "the words {words:?} are not all inside the card {card:?} (the last card was {short:?})");
    }
}

/// A program that watches the window reads the note in `ui.inspect`: `saveNote` is null, and after a save across the formats it is the list of the parts, the engine's words.
#[test]
fn the_window_state_tells_the_note() {
    let dir = TempDir::new("note-inspect");
    let source = fixture::write(dir.path(), "plan.odg", &odg_with_definitions());
    let answer = dir.file("out.vsdx");
    let (mut w, tx) = crate::support::controlled_with(move |app| app.with_save_picker(picker_answering(vec![answer])));
    w.state_mut().execute("file.open", &json!({"path": source.to_str().unwrap()})).unwrap();
    w.rest();
    let before = crate::support::call(&mut w, &tx, "ui.inspect", json!({})).unwrap();
    assert_eq!(before["saveNote"], Value::Null, "{before}");
    save_as(&mut w);
    let expected = words(Target::Vsdx, &engine_carried(&source, &[&dir.file("oracle.vsdx")])[0]);
    assert_eq!(expected.len(), 1);
    let after = crate::support::call(&mut w, &tx, "ui.inspect", json!({})).unwrap();
    assert_eq!(after["saveNote"], json!(expected), "{after}");
    assert!(after["openError"].is_null(), "the note is not the error banner: {after}");
    w.state_mut().dismiss_save_note();
    let gone = crate::support::call(&mut w, &tx, "ui.inspect", json!({})).unwrap();
    assert_eq!(gone["saveNote"], Value::Null);
}

// ------------------------------------------------------------------------------------------------------------------------------- the files of the screenshots

/// Writes the files the screenshots are taken from, into the folder `DIACRAFT_DEMO_DIR`: one for each part of the note, for the real window to open and Save As. Run by hand.
#[test]
#[ignore = "writes the files of the screenshots into the folder in DIACRAFT_DEMO_DIR, for opening in the running window"]
fn write_the_files_of_the_screenshots() {
    let dir = PathBuf::from(std::env::var("DIACRAFT_DEMO_DIR").expect("DIACRAFT_DEMO_DIR names a folder"));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, bytes) in [
        ("definitions.odg", odg_with_definitions()),
        ("labelled.vsdx", fixture::labelled_vsdx()),
        ("macros.vsdm", vsdm_with_vba()),
        ("macros.odg", odg_with_macros()),
        ("objects.odg", odg_with_a_subdocument()),
        ("signed.vsdx", signed_vsdx()),
        ("everything.odg", odg_with_everything()),
        ("data.vsdx", vsdx_with_shape_data()),
        ("formulas.vsdx", vsdx_with_formulas()),
        ("arrows.vsdx", vsdx_with_formulas_and_arrows()),
    ] {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
}

// ------------------------------------------------------------------------------------------------------------------------------- the check after the save

/// A VSDX drawing whose formulas an `.odg` cannot give back: the places of the members of a group, which the reader works out from the group's frame (a shape's own `RGB()` and
/// `Sheet.N!` come back).
fn vsdx_with_formulas() -> Vec<u8> {
    let member = |id: u32, x: f64, k: f64| {
        format!(
            r#"<Shape ID="{id}" Type="Shape"><Cell N="PinX" V="{x}" F="Sheet.1!Width*{k}"/><Cell N="PinY" V="0.5"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/></Shape>"#
        )
    };
    let shapes = format!(
        r#"<Shape ID="1" Type="Group" NameU="Pair"><Cell N="PinX" V="4"/><Cell N="PinY" V="6"/><Cell N="Width" V="4"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="2" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/><Shapes>{}{}</Shapes></Shape>"#,
        member(2, 1.0, 0.25),
        member(3, 3.0, 0.75)
    );
    VsdxBuilder::new().page(PageSpec::new(1, "Plan").sheet(PAGE_SIZE).shapes(&shapes)).build()
}

/// [`vsdx_with_formulas`] with the arrows of a line on a third shape: a line end is lost in an `.odg` as well.
fn vsdx_with_formulas_and_arrows() -> Vec<u8> {
    let shapes = concat!(
        r##"<Shape ID="1" Type="Shape"><Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="FillForegnd" V="#ff0000" F="RGB(255,0,0)"/></Shape>"##,
        r#"<Shape ID="2" Type="Shape"><Cell N="PinX" V="5"/><Cell N="PinY" V="2"/><Cell N="Width" V="2" F="Sheet.1!Width"/><Cell N="Height" V="1"/></Shape>"#,
        r#"<Shape ID="3" Type="Shape"><Cell N="PinX" V="3"/><Cell N="PinY" V="5"/><Cell N="Width" V="2"/><Cell N="Height" V="0"/><Cell N="BeginArrow" V="3"/><Cell N="EndArrow" V="4"/></Shape>"#
    );
    VsdxBuilder::new().page(PageSpec::new(1, "Plan").sheet(PAGE_SIZE).shapes(shapes)).build()
}

/// What the engine says the check found for the save of `source` as `target`: the words of the `checked` field, made by a session of its own that does what the window does.
fn engine_check_words(source: &Path, target: &Path) -> Vec<String> {
    let mut s = Session::new();
    s.execute("file.open", &json!({"path": source.to_str().unwrap()})).unwrap();
    let out: Value = s.execute("file.save_as", &json!({"path": target.to_str().unwrap()})).unwrap();
    let outcome = diacraft_engine::check::Outcome::from_json(&out["checked"]).unwrap_or_else(|| panic!("the save has a `checked` field: {out}"));
    diacraft_engine::check::words(&outcome)
}

/// **The card says what the file read back differs in**, after the note's own sentences: a VSDX drawing saved as an `.odg` loses formulas (its group members' places), and the card names the class
/// with its count, in the engine's words, once for the save, and says the values group.
#[test]
fn the_card_names_what_the_file_read_back_differs_in_after_the_notes_sentences() {
    let dir = TempDir::new("note-checked");
    let out = dir.file("out.odg");
    let mut w = window_of(&dir, "plan.vsdx", &vsdx_with_formulas(), vec![out.clone()], [1280.0, 900.0]);
    save_as(&mut w);
    assert!(out.exists(), "the file was written");
    let expected = engine_check_words(&dir.file("plan.vsdx"), &dir.file("oracle.odg"));
    assert!(expected.iter().any(|part| part == "formulas: 2 formulas are lost"), "{expected:?}");
    let shown_parts = note(&w);
    assert!(shown_parts.ends_with(&expected), "the card ends with the engine's words for `checked`: {shown_parts:?}");
    for part in &expected {
        assert_eq!(shown(&w, part), 1, "each part once: {part}");
    }
}

/// A difference in a `.diacraft` file written from a drawing of another format is a card too (it carries nothing, so the card is the check's alone), and a file that differs in nothing has none.
#[test]
fn a_diacraft_file_that_differs_has_a_card_of_the_check_alone_and_one_that_does_not_has_none() {
    let dir = TempDir::new("note-checked-native");
    let out = dir.file("out.diacraft");
    let mut w = window_of(&dir, "plan.vsdx", &fixture::labelled_vsdx(), vec![out.clone(), dir.file("clean.diacraft")], [1280.0, 900.0]);
    diacraft_engine::check::with_tamper(
        |doc| {
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            page.name = Arc::from("Renamed");
        },
        || save_as(&mut w),
    );
    let parts = note(&w);
    assert_eq!(parts.len(), 2, "{parts:?}");
    assert!(parts[0].starts_with("DiaCraft read the file back and compared it with the drawing, and the file differs in 1 way."), "{}", parts[0]);
    assert_eq!(parts[1], "pages: 1 page differs");
    assert_eq!(shown(&w, "pages: 1 page differs"), 1);
    // The next save, with nothing planted, finds nothing, and the card goes.
    save_as(&mut w);
    assert!(w.state().save_note().is_none(), "nothing differs: no card");
}
