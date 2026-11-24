//! `.diacraft` in the engine: open, save and Save As, the two save paths, the working file, the identifier ledger, the content stamp, templates,
//! stencils and themes. The package is made by the engine's own commands; the VSDX and OpenDocument files are built in code.

use std::collections::BTreeSet;

use diacraft_container::zip::{Archive, ArchiveWriter, Method};
use diacraft_doc::stamp::Stamp;
use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use crate::edit::{doc, drop_shape};
use crate::native_common::{Dir, made, opened, status, vsdx_drawing};
use crate::odg_build::Drawing;
use crate::{blank_session, check_session, run};

/// `bytes` as a package with its entry `name` replaced by `data` (the other entries as they were), written the way the container writes.
fn with_entry(bytes: &[u8], name: &str, data: &[u8]) -> Vec<u8> {
    let mut archive = Archive::read(bytes, &diacraft_container::Limits::default()).unwrap();
    let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
    let mut w = ArchiveWriter::new();
    for (k, n) in names.iter().enumerate() {
        let inflated = archive.inflate(k, &mut diacraft_container::Work::default()).unwrap();
        let method = if n == "mimetype" { Method::Stored } else { Method::Deflated };
        w.add(n, if n == name { data } else { &inflated }, method).unwrap();
    }
    w.finish().unwrap()
}

/// `bytes` with a part `name` of the role `role` added to the package and its manifest, required (`required="1"`) or not.
fn with_part(bytes: &[u8], name: &str, role: &str, required: bool) -> Vec<u8> {
    let mut archive = Archive::read(bytes, &diacraft_container::Limits::default()).unwrap();
    let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
    let mut w = ArchiveWriter::new();
    for (k, n) in names.iter().enumerate() {
        let inflated = archive.inflate(k, &mut diacraft_container::Work::default()).unwrap();
        let data = if n == "manifest.xml" {
            let text = String::from_utf8(inflated).unwrap();
            let required = if required { " required=\"1\"" } else { "" };
            let row = format!("<diacraft:part path=\"{name}\" role=\"{role}\"{required}/></diacraft:manifest>");
            assert!(text.contains("</diacraft:manifest>"), "{text}");
            text.replace("</diacraft:manifest>", &row).into_bytes()
        } else {
            inflated
        };
        w.add(n, &data, if n == "mimetype" { Method::Stored } else { Method::Deflated }).unwrap();
    }
    w.add(name, b"the bytes of a part a later version wrote", Method::Deflated).unwrap();
    w.finish().unwrap()
}

/// The text of the entry `name` of a package.
fn entry_text(bytes: &[u8], name: &str) -> String {
    let mut archive = Archive::read(bytes, &diacraft_container::Limits::default()).unwrap();
    let at = archive.position(name).unwrap_or_else(|| panic!("no entry {name}"));
    String::from_utf8(archive.inflate(at, &mut diacraft_container::Work::default()).unwrap()).unwrap()
}

/// A `state.xml` whose ledger is `rows` (page, mark) and whose content stamp is `stamp`.
fn state_xml(rows: &[(u32, u32)], stamp: Option<&str>) -> String {
    let stamp = stamp.map(|s| format!(" content-stamp=\"{s}\"")).unwrap_or_default();
    let rows: String = rows.iter().map(|(p, m)| format!("<diacraft:ledger page=\"{p}\" mark=\"{m}\"/>")).collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<diacraft:state xmlns:diacraft=\"urn:app.diacraft:extension\"{stamp}>{rows}</diacraft:state>"
    )
}

fn pages_of(s: &Session) -> Vec<String> {
    s.document().unwrap().pages.iter().map(|p| p.name.to_string()).collect()
}

// ---- the same model -------------------------------------------------------------------------------------------------------------------

/// Saves the drawing in `a` as `name`, opens that in a session of its own, and says the two drawings are one.
fn assert_same_after_a_reopen(what: &str, a: &mut Session, name: &str) {
    let dir = Dir::new();
    let path = dir.path(name);
    run(a, "file.save_as", json!({ "path": path }));
    let mut b = Session::new();
    run(&mut b, "file.open", json!({ "path": path }));
    assert!(a.document() == b.document(), "{what}: the model read back is not the model saved");
    assert!(!b.is_dirty(), "{what}: a drawing just opened is not unsaved");
    check_session(&b).unwrap();
}

#[test]
fn open_save_reopen_is_the_same_model() {
    // A blank drawing.
    assert_same_after_a_reopen("a blank drawing", &mut blank_session(), "a.diacraft");
    // A drawing a person made, with a theme and shapes.
    let mut s = blank_session();
    made(&mut s);
    assert_same_after_a_reopen("a made drawing", &mut s, "b.diacraft");
    // Each built-in template.
    let listed = run(&mut blank_session(), "template.list", json!({}));
    let ids: Vec<String> = listed["templates"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_owned()).collect();
    assert!(!ids.is_empty());
    for id in ids {
        let mut s = Session::new();
        run(&mut s, "file.new", json!({ "template": id }));
        assert_same_after_a_reopen(&format!("the template {id}"), &mut s, "t.diacraft");
    }
    // A drawing built by the testkit's VSDX builder, saved as `.diacraft` from the file it came from.
    let dir = Dir::new();
    let mut v = Session::new();
    run(&mut v, "file.open", json!({ "path": dir.write("v.vsdx", &vsdx_drawing()) }));
    assert_same_after_a_reopen("a VSDX drawing", &mut v, "v.diacraft");
    // A stencil, and a template: the kind comes from the extension.
    let mut st = blank_session();
    made(&mut st);
    assert_same_after_a_reopen("a stencil", &mut st, "s.diacraft-stencil");
    assert_same_after_a_reopen("a template", &mut st, "t.diacraft-template");
}

#[test]
fn what_the_open_says_is_the_kind_the_version_and_the_extension() {
    let dir = Dir::new();
    let mut s = blank_session();
    made(&mut s);
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft-stencil") }));
    assert_eq!(saved["variant"], "diacraft-stencil", "{saved}");
    assert_eq!(saved["dirty"], false);
    assert_eq!(saved["warning_count"], 0, "{saved}");
    assert!(saved["bytes"].as_u64().unwrap() > 500);
    let mut t = Session::new();
    let opened = run(&mut t, "file.open", json!({ "path": dir.path("a.diacraft-stencil") }));
    assert_eq!(opened["file"]["variant"], "diacraft-stencil");
    assert_eq!(opened["file"]["kind"], "stencil");
    assert_eq!(opened["file"]["format"], "1.1");
    assert_eq!(opened["file"]["new_from_template"], false);
    assert_eq!(opened["history"], json!({"undo": 0, "redo": 0}));
}

// ---- the save paths -------------------------------------------------------------------------------------------------------------------

/// A session whose working file is a `.diacraft` that it just wrote.
fn working_diacraft(dir: &Dir, name: &str) -> Session {
    let mut s = blank_session();
    made(&mut s);
    run(&mut s, "file.save_as", json!({ "path": dir.path(name) }));
    s
}

fn refusal(s: &mut Session, command: &str, params: Value) -> String {
    match s.execute(command, &params) {
        Err(EngineError::Failed { msg, .. } | EngineError::BadParams { msg, .. }) => msg,
        other => panic!("{command} {params}: expected a refusal, got {other:?}"),
    }
}

/// A top-level shape as [`content_of`] sees it: its `ID`, its master and its frame cells.
type ShapeSeen = (u32, Option<u32>, Vec<Option<diacraft_doc::Cell>>);
/// What [`content_of`] gives: each page's name with its shapes, and the masters by name.
type ContentSeen = (Vec<(String, Vec<ShapeSeen>)>, Vec<Option<String>>);

/// What a reader of the drawing sees of its content: each page's name, and each top-level shape's `ID`, master and frame cells; the masters by name.
fn content_of(doc: &diacraft_doc::Document) -> ContentSeen {
    let pages = doc
        .pages
        .iter()
        .map(|p| {
            let shapes = p
                .shapes
                .iter()
                .map(|x| (x.id, x.master, ["PinX", "PinY", "Width", "Height"].iter().map(|c| x.sheet.cell(c).cloned()).collect()))
                .collect();
            (p.name.to_string(), shapes)
        })
        .collect();
    (pages, doc.masters.iter().map(|m| m.name_u.as_deref().map(str::to_owned)).collect())
}

/// **A `.diacraft` drawing saved as a VSDX type is written from the model alone** (`write_with(doc, None, ..)`: a `.diacraft` file keeps no VSDX file to write into), and
/// the refusal of it is gone. For each of the six types the file is written, the working file becomes that type, a session of its own reads it with the drawing's content, and a
/// save after an edit splices into the file the model writer made.
#[test]
fn a_diacraft_drawing_saved_as_each_vsdx_type_is_written_from_the_model_and_reopens_with_its_content() {
    let dir = Dir::new();
    for ext in ["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm"] {
        let mut s = working_diacraft(&dir, &format!("a-{ext}.diacraft"));
        let drawn = content_of(s.document().unwrap());
        let path = dir.path(&format!("out.{ext}"));
        let saved = run(&mut s, "file.save_as", json!({ "path": path }));
        assert_eq!(saved["variant"], ext, "{saved}");
        assert_eq!(saved["dirty"], false);
        assert_eq!(
            (status(&mut s)["format"].clone(), status(&mut s)["variant"].clone()),
            (json!("vsdx"), json!(ext)),
            "the working file is the {ext} file"
        );
        let mut t = Session::new();
        run(&mut t, "file.open", json!({ "path": path }));
        assert_eq!(content_of(t.document().unwrap()), drawn, "{ext}: the file holds the drawing");
        check_session(&t).unwrap();
        if ext.starts_with("vst") {
            // A template opens as a new drawing made from it, which has no file of its own to save into.
            assert_eq!(status(&mut t)["has_file"], false, "{ext}");
            continue;
        }
        // The next save is into the file the model writer made: an edit is written and read back.
        let first = t.document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().id;
        run(&mut t, "shape.move", json!({ "ids": [first], "dx": 1.0, "dy": 0.0 }));
        let edited = content_of(t.document().unwrap());
        run(&mut t, "file.save", json!({}));
        let mut u = Session::new();
        run(&mut u, "file.open", json!({ "path": path }));
        assert_eq!(content_of(u.document().unwrap()), edited, "{ext}: the edit saved into the file is read back");
    }
}

#[test]
fn a_drawing_read_from_a_vsdx_file_saved_as_diacraft_and_reopened_is_saved_as_a_vsdx_file_of_the_same_content() {
    // The content check of the VSDX files (`diacraft_vsdx::compare_content`): the file the drawing was read from, and the one a session of its own writes from the `.diacraft` file, with the
    // bookkeeping as the file had it (the content stamp says the drawing is the one the file held). An edit made there is a difference.
    let dir = Dir::new();
    let original = crate::native_common::vsdx_drawing();
    let mut v = Session::new();
    run(&mut v, "file.open", json!({ "path": dir.write("a.vsdx", &original) }));
    run(&mut v, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": dir.path("a.diacraft") }));
    run(&mut t, "file.save_as", json!({ "path": dir.path("b.vsdx") }));
    let limits = diacraft_vsdx::Limits::default();
    let same = diacraft_vsdx::compare_content(&original, &dir.read("b.vsdx"), &limits).unwrap();
    assert!(same.same(), "the content of the VSDX file is what it was: {:?}", same.shown);
    let mut e = Session::new();
    run(&mut e, "file.open", json!({ "path": dir.path("a.diacraft") }));
    let first = e.document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().id;
    run(&mut e, "shape.move", json!({ "ids": [first], "dx": 1.0, "dy": 0.0 }));
    run(&mut e, "file.save_as", json!({ "path": dir.path("c.vsdx") }));
    let moved = diacraft_vsdx::compare_content(&original, &dir.read("c.vsdx"), &limits).unwrap();
    assert!(!moved.same(), "an edit is a difference");
    let mut r = Session::new();
    run(&mut r, "file.open", json!({ "path": dir.path("c.vsdx") }));
    assert_eq!(content_of(r.document().unwrap()), content_of(e.document().unwrap()), "the file holds the edit");
}

/// A `.diacraft` drawing is saved as an `.odg` (Call 18's refusal is lifted), and a drawing the `.odg` cannot say is refused in words that name the ways out.
#[test]
fn a_diacraft_drawing_is_saved_as_an_odg_and_what_an_odg_cannot_say_is_refused_naming_diacraft_and_the_vsdx_types() {
    let dir = Dir::new();
    let mut s = working_diacraft(&dir, "a.diacraft");
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("out.odg") }));
    assert_eq!(saved["variant"], "odg", "{saved}");
    assert!(dir.files().contains(&"out.odg".to_owned()));
    // A drawing that holds a page the `.odg` has no place for (a background page) is refused in words, with the ways out, and nothing is written.
    let mut background = working_diacraft(&dir, "b.diacraft");
    run(&mut background, "page.add", json!({ "name": "Back", "background": true }));
    let words = refusal(&mut background, "file.save_as", json!({ "path": dir.path("bad.odg") }));
    assert!(words.contains(".diacraft") && words.contains(".vsdx") && words.contains("Nothing was written"), "{words}");
    assert!(!dir.files().contains(&"bad.odg".to_owned()));
}

/// A `settings.xml` with a view setting.
const SETTINGS: &str = r#"<?xml version="1.0" encoding="UTF-8"?><office:document-settings xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:config="urn:oasis:names:tc:opendocument:xmlns:config:1.0" office:version="1.3"><office:settings><config:config-item-set config:name="ooo:view-settings"><config:config-item config:name="ZoomValue" config:type="int">100</config:config-item></config:config-item-set></office:settings></office:document-settings>"#;

/// The shape of the drawings below: a rectangle with a text.
const RECT: &str =
    r#"<draw:rect draw:name="R" draw:style-name="gr1" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><text:p>hello</text:p></draw:rect>"#;

/// A drawing opened from an `.odg` is saved as `.diacraft`, its own format still saves, as it did, and a VSDX type is a save across the formats.
#[test]
fn an_odg_drawing_is_saved_as_diacraft_as_an_odg_and_as_a_vsdx_type() {
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("a.odg", &Drawing::new().page(RECT).bytes()) }));
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("out.diacraft") }));
    assert_eq!((saved["variant"].as_str(), saved["kind"].as_str()), (Some("diacraft"), Some("drawing")), "{saved}");
    assert_eq!(status(&mut s)["format"], "diacraft", "the working file is the package now");
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.path("a.odg") }));
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("b.odg") }));
    assert_eq!(saved["variant"], "odg");
    let across = run(&mut s, "file.save_as", json!({ "path": dir.path("out.vsdx") }));
    assert_eq!(across["variant"], "vsdx", "{across}");
}

/// What a drawing holds that a `.diacraft` file must keep, each by a word only it says: the values a common style's cells only approximate (a hatch, a dash and a
/// marker the drawing does not define, a shadow, a padding, a writing mode), a value no cell can show, a paragraph style's alignment, tab leader and underline, a heading, a list and
/// its style, a hyperlink, a field the text has no cell for, a paragraph's own attribute, a bookmark in an otherwise empty text, and what the document's containers hold that no
/// field reads (a script, an attribute of the body, a child of the drawing, of the master styles and of a master page that holds no shape).
const TOKENS: [&str; 22] = [
    "HatchRefTok",
    "DashRefTok",
    "MarkerRefTok",
    "37%",
    "0.17cm",
    "tb-rl",
    "one parsec",
    "LeaderTok",
    "wave",
    "outline-level",
    "ListStyleTok",
    "example.org/LinkTok",
    "TitleFieldTok",
    "ClassTok",
    "BookmarkTok",
    "ScriptTok",
    "BodyAttrTok",
    "DrawKidTok",
    "MasterStylesKidTok",
    "MasterKidTok",
    "AutoLeaderTok",
    "PageLayerTok",
];

/// **The plan's `odg_to_diacraft_and_reopen_is_the_same_model`** (and 4): a drawing opened from an `.odg` that holds what the `.odg` holds beside its drawing (a font
/// face with its source, `meta.xml` with its fields, `settings.xml`, the entries nothing reads, the manifest's rows) and what it holds that the drawing's cells do not say (see
/// [`TOKENS`]) is saved as `.diacraft` and opened in a session of its own: the model is the same, so nothing the `.odg` held is lost by the trip.
#[test]
fn odg_to_diacraft_and_reopen_is_the_same_model() {
    let dir = Dir::new();
    let mut d = Drawing::new()
        .common(concat!(
            r##"<style:style style:name="Cs" style:family="graphic"><style:graphic-properties draw:fill="hatch" draw:fill-hatch-name="HatchRefTok" draw:stroke="dash" draw:stroke-dash="DashRefTok" "##,
            r##"draw:marker-start="MarkerRefTok" draw:marker-start-width="0.77cm" draw:shadow="visible" draw:shadow-opacity="37%" fo:padding="0.17cm" style:writing-mode="tb-rl" "##,
            r##"svg:stroke-width="one parsec"/></style:style>"##,
            r##"<style:style style:name="Ps" style:family="paragraph"><style:paragraph-properties fo:text-align="start"><style:tab-stops><style:tab-stop style:position="3cm" style:leader-text="LeaderTok"/></style:tab-stops></style:paragraph-properties>"##,
            r##"<style:text-properties style:text-underline-style="wave"/></style:style>"##
        ))
        .page(RECT)
        .page(concat!(
            r##"<draw:rect draw:name="T" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"><text:h text:outline-level="2">Title</text:h>"##,
            r##"<text:list text:style-name="ListStyleTok"><text:list-item><text:p>one</text:p></text:list-item></text:list>"##,
            r##"<text:p text:class-names="ClassTok">see <text:a xlink:href="http://example.org/LinkTok">link</text:a> <text:title>TitleFieldTok</text:title></text:p></draw:rect>"##,
            r##"<draw:rect draw:name="B" svg:x="1cm" svg:y="4cm" svg:width="4cm" svg:height="2cm"><text:p><text:bookmark text:name="BookmarkTok"/></text:p></draw:rect>"##,
            r##"<draw:rect draw:name="A" draw:style-name="gr3" svg:x="1cm" svg:y="7cm" svg:width="4cm" svg:height="2cm"/>"##
        ))
        .file("Fonts/f.ttf", b"abc")
        .file("Basic/Standard/m.xml", b"<script/>")
        .file("Configurations2/accelerator/current.xml", b"<a/>")
        .file("settings.xml", SETTINGS.as_bytes());
    d.generator = "LibreOffice/7.3".to_owned();
    d.auto.push_str(concat!(
        r##"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties fo:padding="0.17cm"/></style:style>"##,
        r##"<style:style style:name="P3" style:family="paragraph"><style:paragraph-properties><style:tab-stops><style:tab-stop style:position="2cm" style:leader-text="AutoLeaderTok"/></style:tab-stops></style:paragraph-properties></style:style>"##
    ));
    let bytes = d.bytes();
    let content = crate::odg_build::part_of(&bytes, "content.xml")
        .replace("<office:body>", r#"<office:scripts><ext:s a="ScriptTok"/></office:scripts><office:body ext:ba="BodyAttrTok">"#)
        .replace("</office:drawing>", r#"<ext:drawchild a="DrawKidTok"/></office:drawing>"#)
        .replace(
            r#"<draw:page draw:name="page2" draw:master-page-name="Default">"#,
            r#"<draw:page draw:name="page2" draw:master-page-name="Default"><draw:layer-set><draw:layer draw:name="PageLayerTok"/></draw:layer-set>"#,
        );
    let styles = crate::odg_build::part_of(&bytes, "styles.xml")
        .replace("<office:master-styles>", r#"<office:master-styles><ext:mschild a="MasterStylesKidTok"/>"#)
        .replace(r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#, r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/><style:master-page style:name="Empty" style:page-layout-name="PM1"><ext:mpkid a="MasterKidTok"/></style:master-page>"#);
    assert!(
        content.contains("ScriptTok") && content.contains("PageLayerTok") && styles.contains("MasterStylesKidTok") && styles.contains("MasterKidTok"),
        "the markup is as the test says"
    );
    let bytes = with_entry(&with_entry(&bytes, "content.xml", content.as_bytes()), "styles.xml", styles.as_bytes());
    let mut s = Session::new();
    s.pin_clock(Some(crate::native_common::PINNED));
    run(&mut s, "file.open", json!({ "path": dir.write("a.odg", &bytes) }));
    let before = s.document().cloned().expect("a drawing");
    assert!(
        before.parts.len() >= 3,
        "the entries nothing reads are the model's: {:?}",
        before.parts.iter().map(|p| p.name.to_string()).collect::<Vec<_>>()
    );
    // What the `.odg` holds is held by the model, each word where a field reads it: **the codec's words are the model's** (a `.diacraft` keeps no original).
    let held = format!("{before:?}");
    for token in TOKENS {
        assert!(held.contains(token), "the model does not hold {token:?}");
    }
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let t = opened(&dir.path("a.diacraft"));
    let mut t = t;
    assert_eq!(status(&mut t)["format"], "diacraft");
    let after = t.document().cloned().expect("a drawing");
    assert!(before == after, "the drawing is the same model after the trip");
    // The same after the encoding: each word is in the package's parts.
    let file = dir.read("a.diacraft");
    let package = Archive::read(&file, &diacraft_container::Limits::default()).unwrap();
    let mut written = String::new();
    for (k, e) in package.entries().iter().enumerate() {
        if e.name.ends_with(".xml") {
            let mut archive = Archive::read(&file, &diacraft_container::Limits::default()).unwrap();
            written.push_str(&String::from_utf8_lossy(&archive.inflate(k, &mut diacraft_container::Work::default()).unwrap()));
        }
    }
    for token in TOKENS {
        assert!(written.contains(token), "the `.diacraft` file does not say {token:?}");
    }
    // And the same again: the package made from the reopened drawing is the package that was made.
    run(&mut t, "file.save_as", json!({ "path": dir.path("b.diacraft") }));
    assert_eq!(dir.read("a.diacraft"), dir.read("b.diacraft"), "the save after a reopen is the save");
}

#[test]
fn a_name_of_no_type_names_every_type_for_a_vsdx_and_an_odg_drawing() {
    let dir = Dir::new();
    let mut v = Session::new();
    run(&mut v, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_drawing()) }));
    let words = refusal(&mut v, "file.save_as", json!({ "path": dir.path("out.txt") }));
    assert!(words.contains(".diacraft") && words.contains(".vsdx") && words.contains(".odg"), "{words}");
    let mut o = Session::new();
    run(&mut o, "file.open", json!({ "path": dir.write("a.odg", &Drawing::new().page("").bytes()) }));
    let words = refusal(&mut o, "file.save_as", json!({ "path": dir.path("out.txt") }));
    assert!(words.contains(".diacraft") && words.contains(".vsdx") && words.contains(".odg"), "{words}");
    assert!(!dir.files().contains(&"out.txt".to_owned()));
}

#[test]
fn a_stencil_is_not_saved_as_an_odg_because_odf_has_no_stencil_format() {
    let dir = Dir::new();
    let mut s = blank_session();
    made(&mut s);
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft-stencil") }));
    let words = refusal(&mut s, "file.save_as", json!({ "path": dir.path("out.odg") }));
    assert!(words.contains("stencil") && words.contains("OpenDocument"), "{words}");
    assert!(!dir.files().contains(&"out.odg".to_owned()));
}

#[test]
fn a_new_drawing_saves_as_diacraft_as_a_vsdx_type_and_as_an_odg() {
    let dir = Dir::new();
    let mut s = blank_session();
    made(&mut s);
    // (the `.odg` model writer is wired, so a new drawing is saved as an `.odg` too, and the file written is the drawing's own.)
    let odg = run(&mut s, "file.save_as", json!({ "path": dir.path("out.odg") }));
    assert_eq!(odg["variant"], "odg");
    assert_eq!(status(&mut s)["format"], "odg");
    let mut s = blank_session();
    made(&mut s);
    let vsdx = run(&mut s, "file.save_as", json!({ "path": dir.path("a.vsdx") }));
    assert_eq!(vsdx["variant"], "vsdx");
    assert_eq!(status(&mut s)["format"], "vsdx");
    // The same drawing, as `.diacraft`, from the working file that is a VSDX one.
    let native = run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    assert_eq!(native["variant"], "diacraft");
    assert_eq!(status(&mut s)["format"], "diacraft");
}

#[test]
fn a_name_that_is_no_type_is_refused_listing_the_types() {
    let dir = Dir::new();
    let mut s = blank_session();
    let words = refusal(&mut s, "file.save_as", json!({ "path": dir.path("out.txt") }));
    for ext in [".diacraft", ".diacraft-stencil", ".diacraft-template", ".vsdx", ".odg"] {
        assert!(words.contains(ext), "{ext} is not listed: {words}");
    }
    assert!(dir.files().is_empty());
}

// ---- the working file -----------------------------------------------------------------------------------------------------------------

#[test]
fn save_as_switches_the_working_file() {
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_drawing()) }));
    assert_eq!(status(&mut s)["format"], "vsdx");
    run(&mut s, "file.save_as", json!({ "path": dir.path("x.diacraft") }));
    let now = status(&mut s);
    assert_eq!(now["format"], "diacraft", "{now}");
    assert_eq!(now["variant"], "diacraft");
    assert_eq!(now["kind"], "drawing");
    assert_eq!(now["has_file"], true);
    assert_eq!(now["dirty"], false);
    assert_eq!(s.path().unwrap().file_name().unwrap(), "x.diacraft");
    // Save writes x.diacraft: an edit reaches it, and the VSDX file is not touched.
    let vsdx_before = dir.read("a.vsdx");
    let before = dir.read("x.diacraft");
    drop_shape(&mut s, "rectangle", 4.0, 4.0);
    assert_eq!(status(&mut s)["dirty"], true);
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["variant"], "diacraft", "{saved}");
    assert_ne!(dir.read("x.diacraft"), before, "the edit is in the file");
    assert_eq!(dir.read("a.vsdx"), vsdx_before);
    assert_eq!(status(&mut s)["dirty"], false);
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": dir.path("x.diacraft") }));
    assert!(s.document() == t.document());
}

#[test]
fn save_keeps_an_opened_vsdx_format_silently() {
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_drawing()) }));
    drop_shape(&mut s, "rectangle", 4.0, 4.0);
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["variant"], "vsdx", "{saved}");
    assert!(diacraft_native::sniff(&dir.read("a.vsdx")).is_none(), "an OPC package, not a .diacraft file");
    assert!(diacraft_opc::Package::read(&dir.read("a.vsdx"), &diacraft_opc::Limits::default()).is_ok());
    assert_eq!(status(&mut s)["format"], "vsdx");
    assert_eq!(dir.files(), ["a.vsdx"], "no other file was made");
}

#[test]
fn a_save_of_a_diacraft_file_whose_name_is_another_type_writes_a_diacraft_file() {
    let dir = Dir::new();
    let _first = working_diacraft(&dir, "a.diacraft");
    let bytes = dir.read("a.diacraft");
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("odd.vsdx", &bytes) }));
    drop_shape(&mut s, "rectangle", 4.0, 4.0);
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["variant"], "diacraft", "{saved}");
    assert!(diacraft_native::sniff(&dir.read("odd.vsdx")).is_some(), "the file is what the drawing is, whatever it is called");
}

#[test]
fn save_keeps_the_format_the_file_has_and_a_vsdx_package_called_diacraft_is_not_turned_into_one() {
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("odd.diacraft", &vsdx_drawing()) }));
    assert_eq!(status(&mut s)["format"], "vsdx", "the bytes decide");
    drop_shape(&mut s, "rectangle", 4.0, 4.0);
    let before = dir.read("odd.diacraft");
    let words = refusal(&mut s, "file.save", json!({}));
    assert!(words.contains("file name must end in .vsdx") && words.contains("file.save_as"), "{words}");
    assert_eq!(dir.read("odd.diacraft"), before, "nothing was written");
    assert_eq!(status(&mut s)["dirty"], true);
    // Save As is the way to the other format, and names it.
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("now.diacraft") }));
    assert_eq!(saved["variant"], "diacraft");
}

#[test]
fn save_never_turns_a_file_into_another_format_by_its_name() {
    // `file.save` keeps the format the file has: a name that is another format's is no name for it, whatever the table of saves says, so the guard holds once the writers across the
    // formats are there (`.odg` to `.diacraft`; VSDX to `.odg` and back).
    let dir = Dir::new();
    let own_words = |words: &str| words.contains("use file.save_as");
    // An `.odg` called `x.diacraft`.
    let mut o = Session::new();
    run(&mut o, "file.open", json!({ "path": dir.write("x.diacraft", &Drawing::new().page("").bytes()) }));
    assert_eq!(status(&mut o)["format"], "odg", "the bytes decide");
    run(&mut o, "page.add", json!({"name": "Edited"}));
    let before = dir.read("x.diacraft");
    let words = refusal(&mut o, "file.save", json!({}));
    assert!(own_words(&words) && words.contains(".odg"), "{words}");
    assert_eq!(dir.read("x.diacraft"), before, "nothing was written");
    // A VSDX package called `x.odg`.
    let mut v = Session::new();
    run(&mut v, "file.open", json!({ "path": dir.write("x.odg", &vsdx_drawing()) }));
    assert_eq!(status(&mut v)["format"], "vsdx");
    drop_shape(&mut v, "rectangle", 4.0, 4.0);
    let before = dir.read("x.odg");
    let words = refusal(&mut v, "file.save", json!({}));
    assert!(own_words(&words) && words.contains(".vsdx"), "{words}");
    assert_eq!(dir.read("x.odg"), before, "nothing was written");
    // A VSDX package called `x.diacraft`, an `.odg` called `x.vsdx`, and the stencil of a VSDX file called `x.odg`: the same.
    let mut w = Session::new();
    run(&mut w, "file.open", json!({ "path": dir.write("y.diacraft", &vsdx_drawing()) }));
    assert!(own_words(&refusal(&mut w, "file.save", json!({}))), "clean, and still not saved as another format");
    drop_shape(&mut w, "rectangle", 4.0, 4.0);
    assert!(own_words(&refusal(&mut w, "file.save", json!({}))));
    let mut z = Session::new();
    run(&mut z, "file.open", json!({ "path": dir.write("y.vsdx", &Drawing::new().page("").bytes()) }));
    run(&mut z, "page.add", json!({"name": "Edited"}));
    assert!(own_words(&refusal(&mut z, "file.save", json!({}))));
    // Save As is the way, and names the format.
    assert_eq!(run(&mut v, "file.save_as", json!({ "path": dir.path("z.vsdx") }))["variant"], "vsdx");
    // A `.diacraft` file keeps being saved as one, whatever it is called.
    let _first = working_diacraft(&dir, "d.diacraft");
    let bytes = dir.read("d.diacraft");
    let mut e = Session::new();
    run(&mut e, "file.open", json!({ "path": dir.write("d.odg", &bytes) }));
    drop_shape(&mut e, "rectangle", 4.0, 4.0);
    assert_eq!(run(&mut e, "file.save", json!({}))["variant"], "diacraft");
}

#[test]
fn a_diacraft_save_refuses_a_file_changed_on_disk() {
    let dir = Dir::new();
    let mut s = working_diacraft(&dir, "a.diacraft");
    drop_shape(&mut s, "rectangle", 4.0, 4.0);
    // Something else rewrites the file (another size, so that the stamp differs whatever the clock's grain is).
    let mut other = dir.read("a.diacraft");
    other.extend_from_slice(b"trailing");
    dir.write("a.diacraft", &other);
    let error = s.execute("file.save", &json!({})).unwrap_err();
    assert!(matches!(error, EngineError::ChangedOnDisk { .. }), "{error:?}");
    assert_eq!(dir.read("a.diacraft"), other, "nothing was written");
    assert_eq!(status(&mut s)["dirty"], true);
    let saved = run(&mut s, "file.save", json!({"force": true}));
    assert_eq!(saved["variant"], "diacraft");
    assert!(diacraft_native::read(&dir.read("a.diacraft"), &diacraft_native::Limits::default()).is_ok());
}

#[test]
fn nothing_but_the_file_is_written_and_a_failed_save_leaves_the_file_as_it_was() {
    let dir = Dir::new();
    let mut s = working_diacraft(&dir, "a.diacraft");
    let before = dir.read("a.diacraft");
    // A folder where the file is to be.
    std::fs::create_dir(dir.path("sub")).unwrap();
    assert!(s.execute("file.save_as", &json!({"path": dir.path("sub")})).is_err());
    assert_eq!(dir.read("a.diacraft"), before);
    assert_eq!(dir.files(), ["a.diacraft", "sub"], "no temporary file is left");
}

// ---- the identifier ledger ------------------------------------------------------------------------------------------------------------

#[test]
fn the_ledger_survives_a_reopen() {
    let dir = Dir::new();
    let mut s = blank_session();
    let ids: Vec<u32> = (0..7).map(|k| drop_shape(&mut s, "rectangle", 1.0 + f64::from(k), 2.0)).collect();
    assert_eq!(ids, [1, 2, 3, 4, 5, 6, 7]);
    run(&mut s, "shape.delete", json!({"ids": [7]}));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let mut t = opened(&dir.path("a.diacraft"));
    assert_eq!(drop_shape(&mut t, "rectangle", 9.0, 2.0), 8, "the id of the deleted shape is not given again after a reopen");
    // The same through a second save and a second reopen: the mark is kept, not lost with the shape.
    run(&mut t, "shape.delete", json!({"ids": [8]}));
    run(&mut t, "file.save", json!({}));
    let mut u = opened(&dir.path("a.diacraft"));
    assert_eq!(drop_shape(&mut u, "rectangle", 9.0, 2.0), 9);
}

#[test]
fn a_hostile_ledger_never_lowers_a_mark() {
    let dir = Dir::new();
    let mut s = blank_session();
    for k in 0..5 {
        drop_shape(&mut s, "rectangle", 1.0 + f64::from(k), 2.0);
    }
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    // A ledger that says 2 for a page that holds shape 5, and rows for a page that is not there and for the page twice.
    let hostile = state_xml(&[(1, 2), (1, 1), (77, 900)], None);
    let path = dir.write("h.diacraft", &with_entry(&dir.read("a.diacraft"), "state.xml", hostile.as_bytes()));
    let mut t = opened(&path);
    assert_eq!(drop_shape(&mut t, "rectangle", 9.0, 2.0), 6, "a mark is raised to the highest id of its page and never lowered below it");
    // A ledger that says more raises: ids 6 to 49 are not given.
    let high = state_xml(&[(1, 49)], None);
    let path = dir.write("high.diacraft", &with_entry(&dir.read("a.diacraft"), "state.xml", high.as_bytes()));
    let mut u = opened(&path);
    assert_eq!(drop_shape(&mut u, "rectangle", 9.0, 2.0), 50);
}

#[test]
fn a_ledger_at_the_most_an_id_can_be_opens_and_says_no_ids_are_left() {
    let dir = Dir::new();
    let mut s = blank_session();
    drop_shape(&mut s, "rectangle", 1.0, 2.0);
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let state = state_xml(&[(1, u32::MAX)], None);
    let path = dir.write("max.diacraft", &with_entry(&dir.read("a.diacraft"), "state.xml", state.as_bytes()));
    let mut t = opened(&path);
    let error = t.execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": 1.0, "y": 1.0})).unwrap_err();
    assert!(error.to_string().contains("no shape ids are left on the page"), "{error}");
    check_session(&t).unwrap();
    // It is kept: a save writes the mark it read.
    run(&mut t, "file.save_as", json!({ "path": dir.path("b.diacraft") }));
    let (_, read) = diacraft_native::read(&dir.read("b.diacraft"), &diacraft_native::Limits::default()).unwrap();
    assert_eq!(read.state.ledger.0.get(&1), Some(&u32::MAX));
}

#[test]
fn a_new_session_from_a_vsdx_file_starts_its_ledger_at_what_the_file_holds() {
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_drawing()) }));
    let id = drop_shape(&mut s, "rectangle", 4.0, 4.0);
    assert_eq!(id, 6, "shape 5 is the page's highest");
    run(&mut s, "shape.delete", json!({"ids": [id]}));
    run(&mut s, "file.save_as", json!({ "path": dir.path("x.diacraft") }));
    let (_, read) = diacraft_native::read(&dir.read("x.diacraft"), &diacraft_native::Limits::default()).unwrap();
    assert_eq!(read.state.ledger.0.get(&1), Some(&6), "the mark the session gave is in the file, though the shape is gone");
}

// ---- the content stamp ----------------------------------------------------------------------------------------------------------------

#[test]
fn the_content_stamp_is_kept_through_diacraft() {
    let dir = Dir::new();
    // A drawing built in code whose file holds a part that nothing models (the reader holds it once has landed).
    let mut v = Session::new();
    run(&mut v, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_drawing()) }));
    let stamp: Stamp = v.content_stamp().expect("a drawing read from a VSDX file is stamped when it is read");
    assert_eq!(v.content_unchanged(), Some(true));
    run(&mut v, "file.save_as", json!({ "path": dir.path("x.diacraft") }));
    assert_eq!(v.content_stamp(), Some(stamp), "a Save As does not move the stamp");
    // The file says it.
    let (_, read) = diacraft_native::read(&dir.read("x.diacraft"), &diacraft_native::Limits::default()).unwrap();
    assert_eq!(read.state.content_stamp, Some(stamp));
    // A new session reads it back: equal, and unedited.
    let mut t = opened(&dir.path("x.diacraft"));
    assert_eq!(t.content_stamp(), Some(stamp));
    assert_eq!(t.content_unchanged(), Some(true));
    // What the VSDX reader holds beside the model (a part nothing models, once holds it) is the same after the trip, so the stamp is the fingerprint of the same model.
    let held = |s: &Session| s.document().unwrap().parts.iter().map(|p| p.name.to_string()).collect::<Vec<_>>();
    assert_eq!(held(&v), held(&t));
    assert!(v.document() == t.document());
    // An edit changes the content, and an undo brings it back.
    drop_shape(&mut t, "rectangle", 4.0, 4.0);
    assert_eq!(t.content_unchanged(), Some(false));
    assert_eq!(t.content_stamp(), Some(stamp), "the stamp is the content as it was read, not as it is");
    run(&mut t, "edit.undo", json!({}));
    assert_eq!(t.content_unchanged(), Some(true));
}

#[test]
fn a_drawing_that_never_came_from_a_foreign_file_has_no_stamp() {
    let dir = Dir::new();
    let mut s = blank_session();
    made(&mut s);
    assert_eq!(s.content_stamp(), None);
    assert_eq!(s.content_unchanged(), None);
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    assert_eq!(s.content_stamp(), None);
    let (_, read) = diacraft_native::read(&dir.read("a.diacraft"), &diacraft_native::Limits::default()).unwrap();
    assert_eq!(read.state.content_stamp, None, "a file with none is a drawing that never came from a foreign file");
}

#[test]
fn bookkeeping_follows_the_stamp_not_the_dirty_flag() {
    // The decision: the content is unchanged when there is a stamp and the content's own equals it.
    use diacraft_engine::nativefile::content_unchanged;
    let (a, b) = (Stamp(1), Stamp(2));
    assert_eq!(content_unchanged(Some(a), Some(a)), Some(true));
    assert_eq!(content_unchanged(Some(a), Some(b)), Some(false));
    assert_eq!(content_unchanged(None, Some(a)), None, "no stamp: nothing to compare with, the bookkeeping of a new file");
    assert_eq!(content_unchanged(Some(a), None), None, "a content that cannot be fingerprinted is not known to be unchanged");
    // And through a reopen, where the dirty flag is clean but the content is not what the foreign file held.
    let dir = Dir::new();
    let mut v = Session::new();
    run(&mut v, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_drawing()) }));
    drop_shape(&mut v, "rectangle", 4.0, 4.0);
    run(&mut v, "file.save_as", json!({ "path": dir.path("x.diacraft") }));
    assert!(!v.is_dirty(), "saved");
    let t = opened(&dir.path("x.diacraft"));
    assert!(!t.is_dirty(), "a drawing just opened is clean");
    assert_eq!(t.content_unchanged(), Some(false), "the content was edited after it was read from the VSDX file, whatever the dirty flag says now");
}

// ---- templates and stencils ----------------------------------------------------------------------------------------------------------

#[test]
fn a_diacraft_template_opens_as_a_new_unnamed_drawing() {
    let dir = Dir::new();
    let mut s = blank_session();
    made(&mut s);
    run(&mut s, "file.save_as", json!({ "path": dir.path("t.diacraft-template") }));
    let mut t = Session::new();
    let result = run(&mut t, "file.open", json!({ "path": dir.path("t.diacraft-template") }));
    assert_eq!(result["file"]["new_from_template"], true, "{result}");
    assert_eq!(result["file"]["kind"], "template");
    assert!(
        t.path().is_none() && t.source().is_none(),
        "NP10: a new drawing keeps no file's bytes, whether it came from a template file or a built-in one"
    );
    assert_eq!(pages_of(&t), pages_of(&s));
    assert_eq!(status(&mut t)["has_file"], false);
    assert_eq!(status(&mut t)["dirty"], false);
    let error = t.execute("file.save", &json!({})).unwrap_err();
    assert!(error.to_string().contains("file.save_as"), "{error}");
    // Its first save is a file of its own, and the template is not written.
    let template_bytes = dir.read("t.diacraft-template");
    let saved = run(&mut t, "file.save_as", json!({ "path": dir.path("new.diacraft") }));
    assert_eq!(saved["variant"], "diacraft");
    assert_eq!(dir.read("t.diacraft-template"), template_bytes);
    assert_eq!(diacraft_native::sniff(&dir.read("new.diacraft")), Some(diacraft_native::Kind::Drawing));
}

#[test]
fn a_template_saved_again_with_other_stencils_open_keeps_its_preview() {
    // The template is written from a copy of the drawing that holds the list of the stencils open, so what is written is not the model the file holds, though the
    // drawing has no change: the picture of the first page is made again and the file does not lose it.
    let has_preview = |bytes: &[u8]| Archive::read(bytes, &diacraft_container::Limits::default()).unwrap().position("preview.png").is_some();
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.new", json!({ "template": "basic_diagram" }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("t.diacraft-template") }));
    assert!(has_preview(&dir.read("t.diacraft-template")));
    let opened_before = run(&mut s, "stencil.list", json!({}))["stencils"].as_array().unwrap().len();
    // Open a built-in stencil the template does not list.
    let available = run(&mut s, "stencil.list", json!({}))["available"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let other = available.first().expect("the template opens one built-in stencil and leaves the other available");
    run(&mut s, "stencil.open", json!({ "id": other }));
    assert!(run(&mut s, "stencil.list", json!({}))["stencils"].as_array().unwrap().len() > opened_before);
    assert!(!s.is_dirty(), "opening a stencil is no edit of the drawing");
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["warning_count"], 0, "{saved}");
    let bytes = dir.read("t.diacraft-template");
    assert!(has_preview(&bytes), "the preview is still in the file");
    let (read, _) = diacraft_native::read(&bytes, &diacraft_native::Limits::default()).map(|(d, o)| (o, d)).unwrap();
    assert_eq!(read.kind, diacraft_native::Kind::Template);
}

#[test]
fn edit_template_opens_the_template_itself() {
    let dir = Dir::new();
    let mut s = blank_session();
    made(&mut s);
    run(&mut s, "file.save_as", json!({ "path": dir.path("t.diacraft-template") }));
    let mut t = Session::new();
    let result = run(&mut t, "file.open", json!({ "path": dir.path("t.diacraft-template"), "edit_template": true }));
    assert_eq!(result["file"]["new_from_template"], false, "{result}");
    assert_eq!(t.path().unwrap().file_name().unwrap(), "t.diacraft-template");
    assert_eq!(status(&mut t)["kind"], "template");
    drop_shape(&mut t, "rectangle", 4.0, 4.0);
    let saved = run(&mut t, "file.save", json!({}));
    assert_eq!(saved["variant"], "diacraft-template", "{saved}");
    assert_eq!(diacraft_native::sniff(&dir.read("t.diacraft-template")), Some(diacraft_native::Kind::Template));
}

#[test]
fn damage_confined_to_a_blob_opens_with_a_warning_and_damage_to_the_state_is_refused_in_words() {
    let dir = Dir::new();
    let mut s = blank_session();
    made(&mut s);
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let good = dir.read("a.diacraft");
    // The theme's bytes are another length than the record says: the theme is lost, the rest of the drawing opens, and the warning counts and names nothing.
    let path = dir.write("lost.diacraft", &with_entry(&good, "blobs/0.xml", b"not what the record says"));
    let mut t = Session::new();
    let result = run(&mut t, "file.open", json!({ "path": path }));
    let words: Vec<&str> = result["file"]["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect();
    assert!(words.iter().any(|w| w.contains("theme") && w.contains("lost")), "{words:?}");
    assert_eq!(result["file"]["warning_count"], 1);
    assert!(t.document().unwrap().themes.is_empty(), "the theme is left out");
    assert_eq!(pages_of(&t), pages_of(&s), "and the pages are the file's");
    // A state part that is not what the format says is damage in the model's bookkeeping, and refuses the file.
    let path = dir.write("state.diacraft", &with_entry(&good, "state.xml", b"<diacraft:state xmlns:diacraft=\"urn:other\"/>"));
    let before = crate::snapshot(&t);
    let error = t.execute("file.open", &json!({ "path": path })).unwrap_err();
    assert!(error.to_string().contains("damaged") && error.to_string().contains("state_malformed"), "{error}");
    assert!(!error.to_string().contains(&path), "no path in the words: {error}");
    assert_eq!(crate::snapshot(&t), before, "a file that cannot be opened leaves the open drawing as it is");
}

#[test]
fn file_new_takes_a_diacraft_template_by_its_path_and_refuses_the_other_kinds_by_their_content() {
    let dir = Dir::new();
    let mut s = blank_session();
    made(&mut s);
    run(&mut s, "file.save_as", json!({ "path": dir.path("t.diacraft-template") }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("d.diacraft") }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("s.diacraft-stencil") }));
    let mut n = Session::new();
    let result = run(&mut n, "file.new", json!({ "template": dir.path("t.diacraft-template") }));
    assert_eq!(result["template"], json!({"id": null, "name": null, "builtin": false}), "{result}");
    assert_eq!((result["file"]["variant"].as_str(), result["file"]["kind"].as_str()), (Some("diacraft-template"), Some("template")), "{result}");
    assert_eq!(pages_of(&n), pages_of(&s));
    assert!(n.source().is_none(), "a new drawing keeps no file's bytes");
    assert_eq!(status(&mut n)["has_file"], false);
    // A drawing or a stencil is no template, by what the file is and not what it is called.
    for (name, kind) in [("d.diacraft", "drawing"), ("s.diacraft-stencil", "stencil")] {
        let before = crate::snapshot(&n);
        let error = n.execute("file.new", &json!({ "template": dir.path(name) })).unwrap_err();
        assert!(error.to_string().contains(&format!("a DiaCraft {kind}, not a template")), "{error}");
        assert_eq!(crate::snapshot(&n), before, "a file that is no template leaves the open drawing as it is");
    }
}

#[test]
fn stencil_open_reads_a_diacraft_stencil() {
    let dir = Dir::new();
    let mut s = blank_session();
    drop_shape(&mut s, "rectangle", 2.0, 2.0);
    drop_shape(&mut s, "circle", 4.0, 2.0);
    run(&mut s, "file.save_as", json!({ "path": dir.path("s.diacraft-stencil") }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("d.diacraft") }));
    let masters = s.document().unwrap().masters.len();
    assert!(masters >= 2, "the drawing holds the masters it used");
    let mut t = blank_session();
    let opened = run(&mut t, "stencil.open", json!({ "path": dir.path("s.diacraft-stencil") }));
    assert_eq!(opened["masters_total"], masters, "{opened}");
    assert_eq!(opened["file"]["variant"], "diacraft-stencil");
    assert_eq!(opened["already_open"], false);
    let again = run(&mut t, "stencil.open", json!({ "path": dir.path("s.diacraft-stencil") }));
    assert_eq!(again["already_open"], true);
    // A drawing is not a stencil, by its content and not its name.
    let error = t.execute("stencil.open", &json!({ "path": dir.path("d.diacraft") })).unwrap_err();
    assert!(error.to_string().contains("not a stencil") && error.to_string().contains("drawing"), "{error}");
    // The masters can be dropped from it.
    let listed = run(&mut t, "stencil.list", json!({}));
    assert!(listed.to_string().contains("s"), "{listed}");
}

#[test]
fn themes_are_derived_at_open() {
    let dir = Dir::new();
    let mut s = blank_session();
    made(&mut s);
    assert!(!s.document().unwrap().themes.is_empty(), "the drawing has a theme");
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let t = opened(&dir.path("a.diacraft"));
    let (a, b) = (s.document().unwrap(), t.document().unwrap());
    assert!(a.themes.iter().zip(b.themes.iter()).all(|(x, y)| x.colors == y.colors && x.part == y.part), "the colours are derived from the bytes");
    assert!(b.themes.iter().any(|theme| theme.colors.is_some()), "a theme with colours");
    assert!(a == b, "the drawing is equal, theme colours included");
    // The theme functions answer as they do for a drawing that was never saved.
    let listed = |s: &mut Session| run(s, "theme.list", json!({}));
    let (mut s, mut t) = (s, t);
    assert_eq!(listed(&mut s), listed(&mut t));
}

#[test]
fn a_vsdx_theme_read_into_the_model_is_the_theme_a_diacraft_file_derives_at_open() {
    // The VSDX reader reads a theme part into the model; a `.diacraft` file keeps its bytes only, and the open derives the rest by the same reader: the two drawings are equal,
    // theme colours, fonts and scheme numbers included.
    let dir = Dir::new();
    let vsdx = diacraft_testkit::package::VsdxBuilder::new()
        .page(diacraft_testkit::package::PageSpec::new(1, "First"))
        .theme(diacraft_testkit::theme::ThemeXml::default().xml().as_bytes())
        .build();
    let mut v = Session::new();
    run(&mut v, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx) }));
    let read = v.document().unwrap();
    assert!(read.themes.iter().any(|theme| theme.colors.is_some() && theme.color_scheme_id.is_some()), "the VSDX reader derived the theme");
    run(&mut v, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let t = opened(&dir.path("a.diacraft"));
    assert!(v.document() == t.document(), "the drawing is equal to the VSDX-read model, theme colours included");
    let listed = |s: &mut Session| run(s, "theme.list", json!({}));
    let mut t = t;
    assert_eq!(listed(&mut v), listed(&mut t), "the theme functions answer the same");
}

#[test]
fn pages_and_the_file_are_the_ones_the_session_had() {
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx_drawing()) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let t = opened(&dir.path("a.diacraft"));
    assert_eq!(pages_of(&t), ["First", "Second"]);
    let first: BTreeSet<u32> = doc(&t).page(1).unwrap().shapes.iter().map(|x| x.id).collect();
    assert_eq!(first, BTreeSet::from([5]));
}

#[test]
fn a_file_that_needs_a_part_this_build_does_not_know_is_refused_naming_it_and_an_optional_one_is_kept() {
    let dir = Dir::new();
    let mut s = blank_session();
    made(&mut s);
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let good = dir.read("a.diacraft");
    // A part of a role this build does not know, that the file says it requires: refused, naming the role, and the words do not call the file something it is not.
    let required = dir.write("required.diacraft", &with_part(&good, "notes/review.txt", "layers", true));
    let mut t = blank_session();
    let before = crate::snapshot(&t);
    let error = t.execute("file.open", &json!({ "path": required })).unwrap_err();
    let words = error.to_string();
    assert!(words.contains("required_role") && words.contains("\"layers\""), "{words}");
    assert!(words.contains("this DiaCraft cannot open it") && !words.contains("not a drawing file"), "it is a drawing file: {words}");
    assert!(matches!(&error, EngineError::CannotOpen { problem, .. } if problem.word() == "not_a_package"), "{error:?}");
    assert_eq!(crate::snapshot(&t), before);
    // The same part that is not required is kept: counted, never read, and written back at its own path.
    let optional = dir.write("optional.diacraft", &with_part(&good, "notes/review.txt", "layers", false));
    let opened = run(&mut t, "file.open", json!({ "path": optional }));
    assert_eq!(opened["file"]["skipped_count"], 1, "{opened}");
    run(&mut t, "file.save_as", json!({ "path": dir.path("again.diacraft") }));
    assert_eq!(entry_text(&dir.read("again.diacraft"), "notes/review.txt"), "the bytes of a part a later version wrote");
}

#[test]
fn a_file_that_is_cut_short_after_its_first_entry_is_damaged_in_words_with_its_class() {
    let dir = Dir::new();
    let mut s = blank_session();
    made(&mut s);
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let mut bytes = dir.read("a.diacraft");
    bytes.truncate(120);
    assert!(diacraft_native::sniff(&bytes).is_some(), "the first entry is still there: it is a `.diacraft` file by its bytes");
    let path = dir.write("cut.diacraft", &bytes);
    let error = Session::new().execute("file.open", &json!({ "path": path })).unwrap_err();
    let words = error.to_string();
    assert!(words.contains("this file is damaged (zip_corrupt)") && words.contains("no end record"), "{words}");
    assert!(!words.contains(&path));
    assert!(matches!(&error, EngineError::CannotOpen { problem, .. } if problem.word() == "damaged"), "{error:?}");
}

#[test]
fn the_manifest_says_who_wrote_the_file_and_a_stencil_has_no_preview() {
    let dir = Dir::new();
    let mut s = blank_session();
    made(&mut s);
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let manifest = entry_text(&dir.read("a.diacraft"), "manifest.xml");
    assert!(manifest.contains(&format!("written-by=\"DiaCraft {}\"", env!("CARGO_PKG_VERSION"))), "{manifest}");
    assert!(manifest.contains("format=\"1.1\""), "{manifest}");
    let has_preview = |name: &str| Archive::read(&dir.read(name), &diacraft_container::Limits::default()).unwrap().position("preview.png").is_some();
    assert!(has_preview("a.diacraft"), "a drawing has the picture of its first page");
    // A stencil has no page to show: none is made, and no word is said of it, whatever the pages it has.
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("s.diacraft-stencil") }));
    assert_eq!(saved["warning_count"], 0, "{saved}");
    assert!(!has_preview("s.diacraft-stencil"));
    // ... and a template has one, being a drawing meant to be copied.
    run(&mut s, "file.save_as", json!({ "path": dir.path("t.diacraft-template") }));
    assert!(has_preview("t.diacraft-template"));
}

#[test]
fn a_stencil_of_a_vsdx_file_is_not_saved_as_an_odg_either() {
    let dir = Dir::new();
    let mut s = blank_session();
    made(&mut s);
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("s.vssx") }));
    assert_eq!(saved["variant"], "vssx");
    assert_eq!(status(&mut s)["kind"], "stencil");
    let words = refusal(&mut s, "file.save_as", json!({ "path": dir.path("out.odg") }));
    assert!(words.contains("OpenDocument has no stencil format") && words.contains(".diacraft-stencil"), "{words}");
    assert!(!dir.files().contains(&"out.odg".to_owned()));
    // The same stencil, opened again, is still a stencil.
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": dir.path("s.vssx") }));
    assert!(refusal(&mut t, "file.save_as", json!({ "path": dir.path("out.odg") })).contains("no stencil format"));
}

#[cfg(unix)]
#[test]
fn a_save_through_a_link_writes_the_file_the_link_led_to_and_a_link_to_another_file_is_refused() {
    use std::os::unix::fs::symlink;
    let dir = Dir::new();
    let mut s = blank_session();
    made(&mut s);
    run(&mut s, "file.save_as", json!({ "path": dir.path("real.diacraft") }));
    let link = dir.path("link.diacraft");
    symlink(dir.path("real.diacraft"), &link).unwrap();
    // Opened through the link, saved through the link: the file it leads to is written, and the link stays a link.
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": link }));
    let before = dir.read("real.diacraft");
    drop_shape(&mut t, "rectangle", 5.0, 5.0);
    let saved = run(&mut t, "file.save", json!({}));
    assert_eq!(saved["variant"], "diacraft");
    assert_ne!(dir.read("real.diacraft"), before, "the file the link leads to was written");
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link is still a link");
    let reopened = opened(&dir.path("real.diacraft"));
    assert!(t.document() == reopened.document());
    // A link that leads to another file is never followed by a Save As: nothing is written through it.
    std::fs::write(dir.path("other.diacraft"), b"another file").unwrap();
    symlink(dir.path("other.diacraft"), dir.path("elsewhere.diacraft")).unwrap();
    let words = refusal(&mut t, "file.save_as", json!({ "path": dir.path("elsewhere.diacraft") }));
    assert!(words.contains("a link stands where the file should be"), "{words}");
    assert_eq!(dir.read("other.diacraft"), b"another file", "the file the link leads to is as it was");
}

#[test]
fn the_names_a_template_gives_that_are_not_stencils_are_told_when_it_opens_as_a_new_drawing() {
    // A VSDX template whose list names one stencil of ours and two names that are not: opened to be changed and saved as `.diacraft-template`, the list is kept as the file had it (the
    // open stencils are what it names), so the `.diacraft` template says the same, and a new drawing from it opens the one and says it ignored the two.
    let dir = Dir::new();
    let vstx = dir.write("list.vstx", &crate::template::template_with_list(Some("flowchart;one;two")));
    let mut t = Session::new();
    let opened = run(&mut t, "file.open", json!({ "path": vstx, "edit_template": true }));
    assert_eq!(opened["stencils"], json!({"open": ["flowchart"], "ignored": 2}), "{opened}");
    run(&mut t, "file.save_as", json!({ "path": dir.path("list.diacraft-template") }));
    for how in ["open", "new"] {
        let mut n = Session::new();
        let result = if how == "open" {
            run(&mut n, "file.open", json!({ "path": dir.path("list.diacraft-template") }))
        } else {
            run(&mut n, "file.new", json!({ "template": dir.path("list.diacraft-template") }))
        };
        let stencils = &result["stencils"];
        assert_eq!(stencils["ignored"], 2, "{how}: {result}");
        let file_warnings = if how == "open" { &result["file"]["warnings"] } else { &result["warnings"] };
        let said = file_warnings
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().is_some_and(|w| w.contains("2 stencil names in the template were not opened")));
        assert!(said, "{how}: the ignored names are told: {result}");
        if how == "open" {
            assert!(result["file"]["warning_count"].as_u64().unwrap() >= 1);
        }
        assert_eq!(n.stencils().open_builtins(), ["flowchart"]);
    }
}

#[test]
fn a_template_opened_as_a_new_drawing_keeps_the_ledger_of_the_template() {
    // The design's 2.7: the ledger of the file still stands when it is a template. A shape of the template that was deleted before it was saved does not give its `ID` to a shape of the
    // new drawing, by either way a template is started from.
    let dir = Dir::new();
    let mut s = blank_session();
    let ids: Vec<u32> = (0..4).map(|k| drop_shape(&mut s, "rectangle", 1.0 + f64::from(k), 2.0)).collect();
    assert_eq!(ids, [1, 2, 3, 4]);
    run(&mut s, "shape.delete", json!({"ids": [4]}));
    run(&mut s, "file.save_as", json!({ "path": dir.path("t.diacraft-template") }));
    let mut a = Session::new();
    run(&mut a, "file.open", json!({ "path": dir.path("t.diacraft-template") }));
    assert!(a.source().is_none());
    assert_eq!(drop_shape(&mut a, "rectangle", 9.0, 2.0), 5, "by file.open");
    let mut b = Session::new();
    run(&mut b, "file.new", json!({ "template": dir.path("t.diacraft-template") }));
    assert_eq!(drop_shape(&mut b, "rectangle", 9.0, 2.0), 5, "by file.new");
    // The template itself, opened to be changed, has it too.
    let mut c = Session::new();
    run(&mut c, "file.open", json!({ "path": dir.path("t.diacraft-template"), "edit_template": true }));
    assert_eq!(drop_shape(&mut c, "rectangle", 9.0, 2.0), 5);
}
