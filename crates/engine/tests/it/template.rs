//! Templates through `Session::execute`, as the control channel and the window call them: `template.list`, `file.new {template}`, a template
//! file started from by its path and by `file.open`, the stencils a template opens, and what a save of the new drawing carries.
//!
//! Every package is made by the engine's own writer or built in code from the specs' XML (`diacraft_testkit::package`); no template from another
//! application is used (AGENTS.md).

use std::sync::Arc;

use diacraft_doc::{DIACRAFT_URN, Document, Owner, RawAttrs, RawElement, RawNode};
use diacraft_engine::template::{self, STENCILS_CHILD};
use diacraft_engine::{EngineError, OpenProblem, Session};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder};
use diacraft_vsdx::{Kind, Variant, WriteOptions, write_with};
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape};
use crate::stencil_open::{Dir, refusal, with_stencil};
use crate::{check_session, run, snapshot};

const CT_TEMPLATE: &str = "application/vnd.ms-visio.template.main+xml"; // brand-ok: a content type the format defines
const WINDOWS_TYPE: &str = "http://schemas.microsoft.com/visio/2010/relationships/windows";
const WINDOWS_CT: &str = "application/vnd.ms-visio.windows+xml";
const CT_TEMPLATE_MACRO: &str = "application/vnd.ms-visio.template.macroEnabled.main+xml"; // brand-ok: a content type the format defines
const VBA_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";
const VBA_SIGNATURE_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProjectSignature";
const VBA_BYTES: &[u8] = b"not code, never run";

fn ids_of(list: &Json, key: &str) -> Vec<String> {
    list[key].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap().to_owned()).collect()
}

/// The ids of the stencils the Shapes window holds, in its order.
fn open_stencils(s: &mut Session) -> Vec<String> {
    ids_of(&run(s, "stencil.list", json!({})), "stencils")
}

/// The ids of the built-in stencils that are not open.
fn available_stencils(s: &mut Session) -> Vec<String> {
    ids_of(&run(s, "stencil.list", json!({})), "available")
}

fn new_from(s: &mut Session, template: &str) -> Json {
    run(s, "file.new", json!({ "template": template }))
}

/// The bytes of a template of the document `doc` (the root style and one page, plus whatever the test put in).
fn template_bytes(doc: &Document) -> Vec<u8> {
    let options = WriteOptions { variant: Some(Variant { kind: Kind::Template, macro_enabled: false }), ..WriteOptions::default() };
    write_with(doc, None, &options).unwrap().bytes
}

/// A template that holds `list` as the `open` attribute of the `stencils` child of DiaCraft's part (no such child for `None`), as a file somebody sent
/// could: the text is put there as it is, which `template::set_stencil_names` would not do (it writes built-in ids and nothing else).
pub(crate) fn template_with_list(list: Option<&str>) -> Vec<u8> {
    let mut doc = Document::new_blank();
    if let Some(text) = list {
        assert!(doc.set_diacraft_child(STENCILS_CHILD, Some(&[("open", text)])));
    }
    template_bytes(&doc)
}

/// DiaCraft data of a version this build does not read, with a `stencils` child that names `basic` only.
fn later_data() -> RawElement {
    let child = RawElement { name: "dc:stencils".into(), attrs: RawAttrs::from_pairs([("open", "basic")]), children: Vec::new() };
    RawElement {
        name: "dc:Data".into(),
        attrs: RawAttrs::from_pairs([("xmlns:dc", DIACRAFT_URN), ("dc:version", "99")]),
        children: vec![RawNode::Element(child)],
    }
}

fn parts_of(bytes: &[u8]) -> Vec<String> {
    Package::read(bytes, &Limits::default()).unwrap().parts().map(str::to_owned).collect()
}

/// Every part of a package by name with its bytes.
fn parts(bytes: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    pkg.parts().map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect()
}

// ---- the list ---------------------------------------------------------------------------------------------------------------------------

#[test]
fn template_list_tells_the_four_templates_with_their_page_theme_and_stencils_and_needs_no_document() {
    let mut s = Session::new();
    let r = run(&mut s, "template.list", json!({}));
    let list = r["templates"].as_array().unwrap();
    assert_eq!(list.len(), 4);
    assert_eq!(r["categories"], json!(["General", "Flowchart", "Business"]));

    let diagram = &list[0];
    assert_eq!(
        (diagram["id"].as_str(), diagram["name"].as_str(), diagram["category"].as_str()),
        (Some("basic_diagram"), Some("Basic Diagram"), Some("General"))
    );
    assert!(diagram["description"].as_str().is_some_and(|d| d.len() > 20));
    assert_eq!(diagram["page"], json!({"size": "letter", "size_name": "Letter", "orientation": "landscape", "width": 11.0, "height": 8.5}));
    assert_eq!(diagram["theme"], Json::Null);
    assert_eq!(diagram["stencils"], json!([{"id": "basic", "name": "Basic Shapes"}, {"id": "arrows", "name": "Arrow Shapes"}]));

    let flowchart = &list[1];
    assert_eq!(
        (flowchart["id"].as_str(), flowchart["name"].as_str(), flowchart["category"].as_str()),
        (Some("basic_flowchart"), Some("Basic Flowchart"), Some("Flowchart"))
    );
    assert_eq!(flowchart["page"], json!({"size": "letter", "size_name": "Letter", "orientation": "portrait", "width": 8.5, "height": 11.0}));
    assert_eq!(flowchart["theme"], json!({"id": "fernwood", "name": "Fernwood", "variant": 0}));
    assert_eq!(flowchart["stencils"], json!([{"id": "flowchart", "name": "Basic Flowchart"}, {"id": "basic", "name": "Basic Shapes"}]));

    // A query: nothing is journaled, and it sits under File, New.
    assert!(s.journal().is_empty());
    let info = s.commands().into_iter().find(|c| c.id == "template.list").unwrap();
    assert!(info.enabled);
    assert_eq!(info.menu, ["File", "New"]);
    // The stencil and theme ids it names are the ones the other lists give.
    let stencil_ids = open_stencils(&mut s);
    for t in list {
        for st in t["stencils"].as_array().unwrap() {
            assert!(stencil_ids.contains(&st["id"].as_str().unwrap().to_owned()));
        }
    }
    let themes = run(&mut s, "theme.list", json!({}));
    assert!(themes["themes"].as_array().unwrap().iter().any(|t| t["id"] == "fernwood"));
}

// ---- starting from a built-in template --------------------------------------------------------------------------------------------------

#[test]
fn a_new_drawing_from_basic_flowchart_has_its_page_theme_and_stencils_and_no_file() {
    let mut s = Session::new();
    let r = new_from(&mut s, "basic_flowchart");
    assert_eq!(r["template"], json!({"id": "basic_flowchart", "name": "Basic Flowchart", "builtin": true}));
    assert_eq!(r["stencils"], json!({"open": ["flowchart", "basic"], "ignored": 0}));
    assert_eq!(r["pages"], json!([{"id": 1, "name": "Page-1"}]));
    assert_eq!(r["history"], json!({"undo": 0, "redo": 0}));
    assert_eq!(r["warnings"], json!([]));
    assert_eq!(r["title"], "Untitled");
    // The page: Letter, upright.
    let page = run(&mut s, "page.setup", json!({}));
    assert_eq!(page["orientation"], "portrait", "{page}");
    assert_eq!((page["width"].as_f64(), page["height"].as_f64()), (Some(8.5), Some(11.0)), "{page}");
    // The theme, its part and its style.
    let theme = run(&mut s, "theme.list", json!({}));
    assert_eq!(theme["page"]["theme"], "fernwood");
    assert_eq!(theme["themes"].as_array().unwrap().iter().find(|t| t["id"] == "fernwood").unwrap()["in_drawing"], true);
    assert!(diacraft_engine::theme::theme_style_id(doc(&s)).is_some());
    // The Shapes window: the flowchart stencil first, then the basic one, and nothing left to fetch.
    assert_eq!(open_stencils(&mut s), ["flowchart", "basic"]);
    assert_eq!(available_stencils(&mut s), ["decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"]);
    // A drawing with no file yet, clean until it is edited.
    assert_eq!(run(&mut s, "file.status", json!({})), json!({"dirty": false, "has_file": false, "format": null, "variant": null, "kind": null}));
    assert!(s.path().is_none());
    assert!(s.source().is_none(), "a new drawing keeps no file's bytes (NP10): the model holds the template, and there is no path");
    let e = s.execute("file.save", &json!({})).unwrap_err().to_string();
    assert!(e.contains("file.save_as"), "{e}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_new_drawing_from_basic_diagram_is_a_plain_landscape_page_with_the_basic_shapes_and_the_arrows_open_and_the_flowchart_ones_a_click_away() {
    let mut s = Session::new();
    let r = new_from(&mut s, "basic_diagram");
    assert_eq!(r["template"]["id"], "basic_diagram");
    assert_eq!(r["stencils"], json!({"open": ["basic", "arrows"], "ignored": 0}));
    let page = run(&mut s, "page.setup", json!({}));
    assert_eq!(page["orientation"], "landscape", "{page}");
    assert_eq!((page["width"].as_f64(), page["height"].as_f64()), (Some(11.0), Some(8.5)), "{page}");
    assert_eq!(run(&mut s, "theme.list", json!({}))["page"]["theme"], "none");
    assert!(doc(&s).themes.is_empty());
    assert_eq!(open_stencils(&mut s), ["basic", "arrows"]);
    assert_eq!(available_stencils(&mut s), ["flowchart", "decorative", "gane-sarson", "cross-functional", "orgchart"]);
    let available = &run(&mut s, "stencil.list", json!({}))["available"][0];
    assert_eq!(
        (available["name"].as_str(), available["builtin"].as_bool(), available["masters_total"].as_u64()),
        (Some("Basic Flowchart"), Some(true), Some(18))
    );
    // The other stencil is opened by a click on More Shapes, and is then second.
    let r = run(&mut s, "stencil.open", json!({"id": "flowchart"}));
    assert_eq!((r["id"].as_str(), r["already_open"].as_bool()), (Some("flowchart"), Some(false)));
    assert_eq!(open_stencils(&mut s), ["basic", "arrows", "flowchart"]);
    assert_eq!(available_stencils(&mut s), ["decorative", "gane-sarson", "cross-functional", "orgchart"]);
}

#[test]
fn a_template_is_asked_for_by_its_name_in_any_case_too() {
    let mut s = Session::new();
    for key in ["Basic Flowchart", "BASIC-FLOWCHART", " basic_flowchart "] {
        let r = new_from(&mut s, key);
        assert_eq!(r["template"]["id"], "basic_flowchart", "{key:?}");
    }
}

#[test]
fn a_blank_new_drawing_opens_every_stencil_again_and_so_does_a_drawing_read_from_a_file() {
    let dir = Dir::new();
    let mut s = Session::new();
    new_from(&mut s, "basic_diagram");
    assert_eq!(open_stencils(&mut s), ["basic", "arrows"]);
    let r = run(&mut s, "file.new", json!({}));
    assert!(r.get("template").is_none() && r.get("stencils").is_none(), "a blank drawing is answered as it always was: {r}");
    assert_eq!(open_stencils(&mut s), ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"]);
    assert!(s.source().is_none(), "a blank drawing has no file and no template behind it");
    // A drawing saved and opened again is not a template: its stencils are the default ones.
    new_from(&mut s, "basic_diagram");
    let saved = dir.path("drawing.vsdx");
    run(&mut s, "file.save_as", json!({"path": saved}));
    assert_eq!(open_stencils(&mut s), ["basic", "arrows"], "a save changes nothing about the stencils");
    let opened = run(&mut s, "file.open", json!({"path": saved}));
    assert_eq!(opened["file"]["new_from_template"], false);
    assert!(opened.get("stencils").is_none(), "only a template says which stencils it opened");
    assert_eq!(open_stencils(&mut s), ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"]);
}

#[test]
fn a_stencil_file_stays_open_when_a_template_narrows_the_built_in_ones() {
    let (mut s, _dir) = with_stencil();
    assert_eq!(open_stencils(&mut s), ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart", "file-1"]);
    new_from(&mut s, "basic_diagram");
    assert_eq!(open_stencils(&mut s), ["basic", "arrows", "file-1"], "the stencil files belong to the session and not to the drawing");
    new_from(&mut s, "basic_flowchart");
    assert_eq!(open_stencils(&mut s), ["flowchart", "basic", "file-1"]);
}

#[test]
fn a_new_shape_on_a_template_page_follows_the_page_theme_and_on_a_plain_page_follows_none() {
    let mut s = Session::new();
    new_from(&mut s, "basic_flowchart");
    let style = diacraft_engine::theme::theme_style_id(doc(&s)).unwrap();
    let id = drop_shape(&mut s, "process", 4.0, 5.0);
    let shape = doc(&s).shape(&doc(&s).find_shape(Owner::Page(1), id).unwrap()).unwrap().sheet.links;
    assert_eq!((shape.line, shape.fill, shape.text), (Some(style), Some(style), Some(style)));
    let look = &run(&mut s, "shape.formatting", json!({"ids": [id]}))["shapes"][0];
    assert_ne!(look["fill"], "#ffffff", "the theme's fill and not the plain white: {look}");

    new_from(&mut s, "basic_diagram");
    let id = drop_shape(&mut s, "rectangle", 4.0, 5.0);
    let shape = doc(&s).shape(&doc(&s).find_shape(Owner::Page(1), id).unwrap()).unwrap().sheet.links;
    assert_eq!((shape.line, shape.fill, shape.text), (None, None, None));
    assert_eq!(run(&mut s, "shape.formatting", json!({"ids": [id]}))["shapes"][0]["fill"], "#ffffff");
}

#[test]
fn the_drawing_is_clean_until_it_is_edited_and_undo_goes_back_to_the_template_and_no_further() {
    let mut s = Session::new();
    new_from(&mut s, "basic_flowchart");
    assert!(!s.is_dirty());
    drop_shape(&mut s, "process", 4.0, 5.0);
    assert!(s.is_dirty());
    run(&mut s, "edit.undo", json!({}));
    assert!(!s.is_dirty());
    assert!(s.execute("edit.undo", &json!({})).is_err(), "the template's content is not an undo step");
    assert_eq!(doc(&s).pages.len(), 1);
}

// ---- saving the new drawing -------------------------------------------------------------------------------------------------------------

#[test]
fn the_new_drawing_saves_as_a_drawing_with_the_theme_the_page_and_the_list_and_as_a_template_that_starts_the_same_drawing_again() {
    let dir = Dir::new();
    let mut s = Session::new();
    new_from(&mut s, "basic_flowchart");
    let id = drop_shape(&mut s, "process", 4.0, 5.0);
    let vsdx = dir.path("flow.vsdx");
    let saved = run(&mut s, "file.save_as", json!({"path": vsdx}));
    assert_eq!(saved["variant"], "vsdx");
    // The file is a drawing, with the theme's part, and the page's size.
    let bytes = std::fs::read(&vsdx).unwrap();
    assert!(parts_of(&bytes).iter().any(|p| p.contains("theme")), "{:?}", parts_of(&bytes));
    let mut t = Session::new();
    let opened = run(&mut t, "file.open", json!({"path": vsdx}));
    assert_eq!(opened["file"]["variant"], "vsdx");
    assert_eq!(opened["file"]["warning_count"], 0, "{opened}");
    assert_eq!(run(&mut t, "theme.list", json!({}))["page"]["theme"], "fernwood");
    assert!(doc(&t).find_shape(Owner::Page(1), id).is_some());
    let page = run(&mut t, "page.setup", json!({}));
    assert_eq!((page["width"].as_f64(), page["height"].as_f64()), (Some(8.5), Some(11.0)), "{page}");
    // The list that names the stencils came along in the drawing, and the template saved from the drawing of the template starts the same drawing.
    assert_eq!(template::stencil_names(doc(&t)).map(|n| n.ids), Some(vec!["flowchart", "basic"]));
    let vstx = dir.path("mine.vstx");
    let saved = run(&mut s, "file.save_as", json!({"path": vstx}));
    assert_eq!(saved["variant"], "vstx");
    let mut u = Session::new();
    new_from(&mut u, "basic_diagram");
    assert_eq!(open_stencils(&mut u), ["basic", "arrows"]);
    let r = run(&mut u, "file.new", json!({"template": vstx}));
    assert_eq!(r["template"], json!({"id": null, "name": null, "builtin": false}));
    assert_eq!(r["file"]["variant"], "vstx");
    assert_eq!(r["stencils"]["open"], json!(["flowchart", "basic"]));
    assert_eq!(open_stencils(&mut u), ["flowchart", "basic"]);
    assert_eq!(run(&mut u, "theme.list", json!({}))["page"]["theme"], "fernwood");
    assert!(doc(&u).find_shape(Owner::Page(1), id).is_some(), "a template made from a drawing starts with that drawing's shapes");
}

#[test]
fn the_first_save_of_the_new_drawing_gives_it_times_of_its_own_and_the_template_is_never_written() {
    let dir = Dir::new();
    let built = template::build("test", template::find("basic_diagram").unwrap()).unwrap();
    let file = dir.write("basic.vstx", &built.bytes);
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"template": file}));
    drop_shape(&mut s, "rectangle", 4.0, 5.0);
    run(&mut s, "file.save_as", json!({"path": dir.path("out.vsdx")}));
    assert_eq!(std::fs::read(&file).unwrap(), built.bytes, "the template on the disk is as it was");
    let out = std::fs::read(dir.path("out.vsdx")).unwrap();
    let package = Package::read(&out, &Limits::default()).unwrap();
    let core = String::from_utf8(package.part("/docProps/core.xml").unwrap().to_vec()).unwrap();
    assert!(core.contains("<dcterms:created") && core.contains("<dcterms:modified"), "{core}");
}

// ---- starting from a template file ------------------------------------------------------------------------------------------------------

#[test]
fn a_built_in_template_and_its_file_start_the_same_drawing() {
    let dir = Dir::new();
    for spec in template::templates() {
        let built = template::build("test", spec).unwrap();
        let file = dir.write(&format!("{}.vstx", spec.id), &built.bytes);
        let (mut a, mut b, mut c) = (Session::new(), Session::new(), Session::new());
        let from_id = run(&mut a, "file.new", json!({"template": spec.id}));
        let from_file = run(&mut b, "file.new", json!({"template": file}));
        let opened = run(&mut c, "file.open", json!({"path": file}));
        assert_eq!(a.document(), b.document(), "{}", spec.id);
        assert_eq!(a.document(), c.document(), "{}", spec.id);
        assert_eq!(open_stencils(&mut a), open_stencils(&mut b), "{}", spec.id);
        assert_eq!(open_stencils(&mut a), open_stencils(&mut c), "{}", spec.id);
        assert_eq!(from_id["stencils"], from_file["stencils"]);
        assert_eq!(from_id["stencils"], opened["stencils"]);
        assert_eq!(opened["file"]["new_from_template"], true);
        assert_eq!(from_file["template"], json!({"id": null, "name": null, "builtin": false}));
        assert!(a.source().is_none() && b.source().is_none() && c.source().is_none(), "{}: no bytes behind any of the three (NP10)", spec.id);
        assert!(b.path().is_none() && c.path().is_none());
    }
}

#[test]
fn opening_a_template_to_change_it_is_the_file_itself_and_opens_its_stencils_too_and_saving_it_back_changes_only_app_xml() {
    let dir = Dir::new();
    let built = template::build("test", template::find("basic_diagram").unwrap()).unwrap();
    let file = dir.write("basic.vstx", &built.bytes);
    let mut s = Session::new();
    let r = run(&mut s, "file.open", json!({"path": file, "edit_template": true}));
    assert_eq!(r["file"]["new_from_template"], false);
    assert!(s.path().is_some(), "the file itself");
    assert_eq!(r["stencils"]["open"], json!(["basic", "arrows"]));
    run(&mut s, "file.save", json!({}));
    let (before, after) = (parts(&built.bytes), parts(&std::fs::read(&file).unwrap()));
    let changed: Vec<&str> = after.iter().filter(|(k, v)| before.get(*k) != Some(*v)).map(|(k, _)| k.as_str()).collect();
    assert!(changed.iter().all(|part| *part == "/docProps/app.xml"), "the stencils list is as it was: {changed:?}");
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>());
}

#[test]
fn a_foreign_template_saved_in_place_is_not_given_a_stencils_list() {
    let dir = Dir::new();
    let bytes = VsdxBuilder::new().document_type(CT_TEMPLATE).page(PageSpec::new(1, "First")).build();
    let file = dir.write("foreign.vstx", &bytes);
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": file, "edit_template": true}));
    run(&mut s, "file.save", json!({}));
    let (before, after) = (parts(&bytes), parts(&std::fs::read(&file).unwrap()));
    let changed: Vec<&str> = after.iter().filter(|(k, v)| before.get(*k) != Some(*v)).map(|(k, _)| k.as_str()).collect();
    assert_eq!(changed, ["/docProps/app.xml"], "a save of a file nobody edited keeps every other part");
}

#[test]
fn a_template_opened_and_saved_as_another_with_its_stencils_as_they_opened_is_written_as_it_was() {
    // The round trip of a template (`diacraft-cli roundtrip`): the cell is written only when the open stencils are not the ones the document implies,
    // so a file that has none is not given one for nothing, and one that has it keeps it as it is.
    let dir = Dir::new();
    let foreign = VsdxBuilder::new().document_type(CT_TEMPLATE).page(PageSpec::new(1, "First")).build();
    let ours = template::build("test", template::find("basic_diagram").unwrap()).unwrap().bytes;
    for (what, bytes) in [("another application's", foreign), ("ours", ours)] {
        let file = dir.write("in.vstx", &bytes);
        let mut s = Session::new();
        run(&mut s, "file.open", json!({"path": file, "edit_template": true}));
        let out = dir.path("out.vstx");
        run(&mut s, "file.save_as", json!({"path": out}));
        let (before, after) = (parts(&bytes), parts(&std::fs::read(&out).unwrap()));
        let changed: Vec<&str> = after.iter().filter(|(k, v)| before.get(*k) != Some(*v)).map(|(k, _)| k.as_str()).collect();
        assert!(changed.iter().all(|part| *part == "/docProps/app.xml"), "{what}: {changed:?}");
        assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>(), "{what}");
    }
}

// ---- Save As a template keeps the stencils that are open --------------------------------------------------------------------------------

// (A drawing is saved twice at most in each test below: a third save of a new drawing is gap F14, which is not this task's.)

#[test]
fn a_template_saved_with_the_stencils_a_new_drawing_has_gets_no_list() {
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    let first = dir.path("first.vstx");
    run(&mut s, "file.save_as", json!({"path": first}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": first, "edit_template": true}));
    assert_eq!(template::stencil_names(doc(&t)), None, "the default stencils are open: nothing to record");
    assert_eq!(open_stencils(&mut t), ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"]);
}

#[test]
fn a_template_saved_with_save_as_keeps_the_built_in_stencils_open_at_that_moment_and_the_drawing_is_unchanged_by_it() {
    let dir = Dir::new();
    // Basic Diagram opens the basic shapes and the arrows; the flowchart ones are opened by hand, and a template saved now keeps all three.
    let mut s = Session::new();
    new_from(&mut s, "basic_diagram");
    drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(&mut s, "stencil.open", json!({"id": "flowchart"}));
    assert_eq!(open_stencils(&mut s), ["basic", "arrows", "flowchart"]);
    let before = doc(&s).clone();
    let steps = s.history().unwrap().undo_len();
    let vstx = dir.path("mine.vstx");
    run(&mut s, "file.save_as", json!({"path": vstx}));
    assert_eq!(doc(&s), &before, "the list is written into the copy that is saved and not into the drawing");
    assert_eq!(template::stencil_names(doc(&s)).map(|n| n.ids), Some(vec!["basic", "arrows"]), "the drawing keeps the list it came with");
    assert_eq!(s.history().unwrap().undo_len(), steps, "no undo step");
    assert!(!s.is_dirty());
    let mut t = Session::new();
    let r = run(&mut t, "file.new", json!({"template": vstx}));
    assert_eq!(r["stencils"]["open"], json!(["basic", "arrows", "flowchart"]));
    assert_eq!(doc(&t).page(1).unwrap().shapes.len(), 1, "with the drawing's shape");
    // This explicit subset differs from the five default stencils, so the saved template retains it.
    assert_eq!(template::stencil_names(doc(&t)).map(|n| n.ids), Some(vec!["basic", "arrows", "flowchart"]));
    assert!(doc(&t).diacraft.is_some());
    assert!(parts_of(&std::fs::read(&vstx).unwrap()).iter().any(|p| p.contains("solution")));
    // Saved again with the stencils as they are, nothing is written for them.
    run(&mut s, "file.save", json!({}));
    assert_eq!(run(&mut t, "file.new", json!({"template": vstx}))["stencils"]["open"], json!(["basic", "arrows", "flowchart"]));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_template_opened_to_be_changed_and_saved_in_place_keeps_its_list_up_to_date() {
    let dir = Dir::new();
    let file = dir.write("flow.vstx", &template_with_list(Some("flowchart")));
    let mut s = Session::new();
    let r = run(&mut s, "file.open", json!({"path": file, "edit_template": true}));
    assert_eq!(r["stencils"]["open"], json!(["flowchart"]));
    run(&mut s, "stencil.open", json!({"id": "basic"}));
    run(&mut s, "file.save", json!({}));
    let mut t = Session::new();
    assert_eq!(run(&mut t, "file.new", json!({"template": file}))["stencils"]["open"], json!(["flowchart", "basic"]));
    // The drawing itself was not changed by it.
    assert_eq!(template::stencil_names(doc(&s)).map(|n| n.ids), Some(vec!["flowchart"]));
    assert!(!s.is_dirty());
}

#[test]
fn a_drawing_made_from_a_template_saved_as_a_drawing_carries_the_list_and_reads_with_nothing_odd() {
    let dir = Dir::new();
    let mut s = Session::new();
    new_from(&mut s, "basic_diagram");
    drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let vsdx = dir.path("drawing.vsdx");
    run(&mut s, "file.save_as", json!({"path": vsdx}));
    let mut t = Session::new();
    let opened = run(&mut t, "file.open", json!({"path": vsdx}));
    assert_eq!(opened["file"]["warning_count"], 0, "{opened}");
    assert_eq!(template::stencil_names(doc(&t)).map(|n| n.ids), Some(vec!["basic", "arrows"]), "the list the template gave it");
    assert_eq!(doc(&t).page(1).unwrap().shapes.len(), 1);
    assert_eq!(
        open_stencils(&mut t),
        ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"],
        "a drawing is not a template: the stencils are the default ones"
    );
}

#[test]
fn a_built_in_template_names_its_stencils_in_diacraft_s_part_and_the_document_sheet_holds_nothing_of_it() {
    for spec in template::templates() {
        let built = template::build("test", spec).unwrap();
        let doc = &built.document;
        let data = doc.diacraft.as_deref().unwrap_or_else(|| panic!("{}: DiaCraft's part", spec.id));
        assert!(data.name.ends_with("Data"), "{}", data.name);
        let child = doc.diacraft_child(STENCILS_CHILD).unwrap_or_else(|| panic!("{}: the child", spec.id));
        assert_eq!(child.attrs.get("open"), Some(spec.stencils.join(";").as_str()), "{}", spec.id);
        assert!(doc.sheet.sections.iter().all(|s| s.name.as_str() != "User"), "{}: the document sheet has no user cells", spec.id);
        let files = parts(&built.bytes);
        let sheet_and_part: Vec<&String> = files.keys().filter(|p| p.contains("solution")).collect();
        assert!(!sheet_and_part.is_empty(), "{}: DiaCraft's part is in the package: {:?}", spec.id, files.keys().collect::<Vec<_>>());
        let document = String::from_utf8_lossy(files.get("/visio/document.xml").unwrap()).into_owned();
        assert!(!document.contains("DiaCraftStencils"), "{}: the first layout is gone", spec.id);
    }
}

#[test]
fn a_template_that_holds_diacraft_data_of_a_later_version_is_saved_without_the_stencils_and_says_so() {
    let dir = Dir::new();
    let mut s = Session::new();
    new_from(&mut s, "basic_flowchart");
    assert_eq!(open_stencils(&mut s), ["flowchart", "basic"]);
    // The drawing holds DiaCraft data this build does not read (a later build's): it is kept as it is and not interpreted, so the stencils cannot be put in it.
    s.edit("a later build", |d| {
        d.diacraft = Some(Arc::new(later_data()));
        Ok(())
    })
    .unwrap();
    assert_eq!(template::stencil_names(doc(&s)), None);
    let vstx = dir.path("later.vstx");
    let saved = run(&mut s, "file.save_as", json!({"path": vstx}));
    assert_eq!(saved["variant"], "vstx");
    assert!(saved["warnings"][0].as_str().unwrap().contains("not recorded"), "{saved}");
    assert!(saved["warning_count"].as_u64().unwrap() >= 1, "{saved}");
    // The file keeps the later build's data as it was, and the stencils it opens are the default ones.
    let mut t = Session::new();
    let opened = run(&mut t, "file.open", json!({"path": vstx, "edit_template": true}));
    assert_eq!(opened["file"]["diacraft"]["version"], "99", "{opened}");
    assert_eq!(open_stencils(&mut t), ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"]);
    assert_eq!(doc(&t).diacraft.as_deref(), Some(&later_data()));
    // Said once: the next save, with nothing changed, has the same word.
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_batch_that_holds_file_open_is_refused_before_the_file_is_read() {
    // Each of these files, opened on its own, is refused or opened in words of its own; in a batch the batch's words come first, no calculation or
    // lookup was made, and the session, its stencils included, is as it was. (The reader has no counter: the words are the proof of the order.)
    let dir = Dir::new();
    let flowchart = dir.write("flow.vstx", &template::build("test", template::find("basic_flowchart").unwrap()).unwrap().bytes);
    let cap = diacraft_engine::file::package_file_cap(&diacraft_vsdx::Limits::default());
    let huge = dir.path("huge.vsdx");
    std::fs::File::create(&huge).unwrap().set_len(cap + 1).unwrap();
    let garbage = dir.write("garbage.vsdx", b"this is not a package");
    let missing = dir.path("missing.vsdx");
    let mut s = Session::new();
    new_from(&mut s, "basic_diagram");
    drop_shape(&mut s, "rectangle", 4.0, 5.0);
    let stencils_before = run(&mut s, "stencil.list", json!({}));
    let before = snapshot(&s);
    for (path, would_say) in
        [(&flowchart, "opens as a template"), (&huge, "larger than"), (&garbage, "not a drawing file"), (&missing, "no such file")]
    {
        diacraft_calc::work::reset();
        diacraft_doc::work::reset();
        let batch = json!({"commands": [{"command": "file.open", "params": {"path": path}}]});
        let e = s.execute("command.batch", &batch).unwrap_err();
        assert!(matches!(e, EngineError::Failed { .. }), "{path}: {e:?}");
        let words = e.to_string();
        assert!(words.contains("cannot run inside a batch"), "{path}: {words}");
        for other in ["larger than", "not a drawing file", "no such file"] {
            assert!(!words.contains(other), "{path}: the file was looked at ({other}): {words}");
        }
        assert_eq!(
            (diacraft_calc::work::commits(), diacraft_calc::work::steps(), diacraft_doc::work::resolver_builds()),
            (0, 0, 0),
            "{path}: nothing was worked out"
        );
        assert_eq!(snapshot(&s), before, "{would_say}");
        assert_eq!(run(&mut s, "stencil.list", json!({})), stencils_before, "{would_say}: the stencils are as they were");
    }
    // Run alone, each does what its words said, so the batch's refusal is the check's.
    for (path, problem) in [(&huge, OpenProblem::FileTooLarge), (&garbage, OpenProblem::NotAPackage), (&missing, OpenProblem::NotFound)] {
        let e = s.execute("file.open", &json!({"path": path})).unwrap_err();
        assert!(matches!(&e, EngineError::CannotOpen { problem: p, .. } if *p == problem), "{path}: {e:?}");
    }
    let opened = run(&mut s, "file.open", json!({"path": flowchart}));
    assert_eq!(opened["stencils"]["open"], json!(["flowchart", "basic"]));
}

#[test]
fn a_stencil_file_that_is_open_is_not_kept_by_a_template_because_a_template_never_names_a_path() {
    let (mut s, dir) = with_stencil();
    let vstx = dir.path("mine.vstx");
    run(&mut s, "file.save_as", json!({"path": vstx}));
    let bytes = std::fs::read(&vstx).unwrap();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    assert!(!text.contains("shapes.vssx"), "the file's name is not in the template");
    let mut t = Session::new();
    let r = run(&mut t, "file.new", json!({"template": vstx}));
    assert_eq!(
        r["stencils"],
        json!({"open": ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"], "ignored": 0})
    );
    assert_eq!(open_stencils(&mut t), ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"], "no `file-1`");
}

#[test]
fn a_template_opens_exactly_the_stencils_it_names_and_never_follows_a_name_to_a_file() {
    let dir = Dir::new();
    let cases: [(&str, Option<&str>, Vec<&str>, usize); 8] = [
        ("one", Some("flowchart"), vec!["flowchart"], 0),
        ("order", Some("flowchart;basic"), vec!["flowchart", "basic"], 0),
        ("case and spaces", Some(" BASIC ; Flowchart "), vec!["basic", "flowchart"], 0),
        ("none wanted", Some(""), vec![], 0),
        ("a path and one that is built in", Some("C:\\Stencils\\Mine.vssx;/etc/passwd;basic"), vec!["basic"], 2),
        (
            "only names it cannot open",
            Some("/etc/passwd;file-1"),
            vec!["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"],
            2,
        ),
        ("a repeat", Some("basic;basic"), vec!["basic"], 1),
        ("no list at all", None, vec!["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"], 0),
    ];
    for (what, row, open, ignored) in cases {
        let file = dir.write("t.vstx", &template_with_list(row));
        let mut s = Session::new();
        let r = run(&mut s, "file.new", json!({"template": file}));
        assert_eq!(r["stencils"], json!({"open": open, "ignored": ignored}), "{what}");
        assert_eq!(open_stencils(&mut s), open, "{what}");
        let warnings = r["warnings"].as_array().unwrap();
        if ignored > 0 {
            assert!(warnings.iter().any(|w| w.as_str().unwrap().contains("never followed")), "{what}: {warnings:?}");
        } else {
            assert!(warnings.is_empty(), "{what}: {warnings:?}");
        }
        // The words about the names are a warning like the others and are counted with them (N3 of the re-review).
        assert_eq!(r["file"]["warning_count"].as_u64(), Some(warnings.len() as u64), "{what}: {r}");
        // `file.open` of the same file opens the same, with the same count.
        let mut t = Session::new();
        let r = run(&mut t, "file.open", json!({"path": file}));
        assert_eq!(r["stencils"], json!({"open": open, "ignored": ignored}), "{what}: file.open");
        let warnings = r["file"]["warnings"].as_array().unwrap();
        assert_eq!(r["file"]["warning_count"].as_u64(), Some(warnings.len() as u64), "{what}: file.open: {r}");
        assert_eq!(warnings.iter().any(|w| w.as_str().unwrap().contains("never followed")), ignored > 0, "{what}: file.open");
    }
}

#[test]
fn the_stencil_windows_of_a_template_from_another_application_are_not_followed_either() {
    // The windows part names a stencil by a path, as files of other applications do. It is not read for stencils.
    let windows = r#"<?xml version="1.0" encoding="utf-8"?><Windows ClientWidth="1" ClientHeight="1"><Window ID="0" WindowType="Drawing"/><Window ID="1" WindowType="Stencil" Document="/etc/passwd" ParentWindow="0"/></Windows>"#;
    let extra = Pkg::new().part("/visio/windows.xml", WINDOWS_CT, windows).rel("/visio/document.xml", "rId90", WINDOWS_TYPE, "windows.xml");
    let bytes = VsdxBuilder::new().document_type(CT_TEMPLATE).page(PageSpec::new(1, "First")).with(extra).build();
    let dir = Dir::new();
    let file = dir.write("foreign.vstx", &bytes);
    let mut s = Session::new();
    let r = run(&mut s, "file.new", json!({"template": file}));
    assert_eq!(
        r["stencils"],
        json!({"open": ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"], "ignored": 0})
    );
    assert_eq!(r["pages"][0]["name"], "First");
}

#[test]
fn a_list_of_most_of_a_megabyte_names_at_most_what_the_bound_allows() {
    let dir = Dir::new();
    let list = format!("flowchart;{}", "not a stencil;".repeat(60_000));
    let file = dir.write("big.vstx", &template_with_list(Some(&list)));
    let mut s = Session::new();
    let r = run(&mut s, "file.new", json!({"template": file}));
    assert_eq!(r["stencils"]["open"], json!(["flowchart"]));
    assert!(r["stencils"]["ignored"].as_u64().unwrap() <= template::MAX_NAMES as u64, "{r}");
}

// ---- refusals leave the session as it was -----------------------------------------------------------------------------------------------

#[test]
fn a_template_that_cannot_be_started_leaves_the_drawing_and_the_stencils_as_they_are() {
    let dir = Dir::new();
    let (mut s, _stencil_dir) = with_stencil();
    new_from(&mut s, "basic_diagram");
    drop_shape(&mut s, "rectangle", 4.0, 5.0);
    let stencils_before = run(&mut s, "stencil.list", json!({}));
    let drawing = dir.write("drawing.vsdx", &VsdxBuilder::new().page(PageSpec::new(1, "First")).build());
    let stencil = dir.write("shapes.vssx", &crate::stencil_open::stencil_bytes());
    let garbage = dir.write("garbage.vstx", b"this is not a package");
    let missing = dir.path("missing.vstx");
    let cases = [
        (json!({"template": "nope"}), "no template"),
        (json!({"template": "Basic"}), "no template"),
        (json!({"template": drawing}), "not a template"),
        (json!({"template": stencil}), "not a template"),
        (json!({"template": garbage}), "not a drawing file"),
        (json!({"template": missing}), "no such file"),
        (json!({"template": dir.path("")}), "not a file"),
        (json!({"template": 5}), "must be text"),
        (json!({"template": "x".repeat(5_000)}), "longer than"),
        (json!({"template": ""}), "no template"),
        (json!({"templat": "basic_diagram"}), "templat"),
    ];
    for (params, words) in cases {
        let msg = refusal(&mut s, "file.new", params.clone());
        assert!(msg.to_lowercase().contains(words), "{params}: {msg}");
        assert_eq!(run(&mut s, "stencil.list", json!({})), stencils_before, "{params}: the stencils are as they were");
    }
    assert!(s.is_dirty(), "the drawing with its shape is still there");
    // And the same inside a batch: file.new replaces the document, which a batch cannot hold in one step.
    let before = snapshot(&s);
    let e = s.execute("command.batch", &json!({"commands": [{"command": "file.new", "params": {"template": "basic_flowchart"}}]})).unwrap_err();
    assert!(e.to_string().contains("batch"), "{e}");
    assert_eq!(snapshot(&s), before);
    assert_eq!(run(&mut s, "stencil.list", json!({})), stencils_before);
}

#[test]
fn a_batch_that_holds_file_new_is_refused_before_the_template_is_made_or_a_file_is_read() {
    // Counted: making a template costs commits and steps and builds resolvers, and a path that is read and not there is a different refusal.
    // The batch's own words come first, and nothing was worked out.
    let mut s = Session::new();
    new_from(&mut s, "basic_diagram");
    drop_shape(&mut s, "rectangle", 4.0, 5.0);
    let stencils_before = run(&mut s, "stencil.list", json!({}));
    let before = snapshot(&s);
    for template in ["basic_flowchart", "/there/is/no/such/folder/never-read.vstx", "nope"] {
        diacraft_calc::work::reset();
        diacraft_doc::work::reset();
        let batch = json!({"commands": [{"command": "file.new", "params": {"template": template}}]});
        let e = s.execute("command.batch", &batch).unwrap_err();
        assert!(matches!(e, EngineError::Failed { .. }), "{template}: {e:?}");
        let words = e.to_string();
        assert!(words.contains("cannot run inside a batch"), "{template}: {words}");
        assert!(!words.contains("no such file") && !words.contains("no template"), "{template}: the template was looked at: {words}");
        assert_eq!(
            (diacraft_calc::work::commits(), diacraft_calc::work::steps(), diacraft_doc::work::resolver_builds()),
            (0, 0, 0),
            "{template}: nothing was worked out"
        );
        assert_eq!(snapshot(&s), before);
        assert_eq!(run(&mut s, "stencil.list", json!({})), stencils_before);
    }
    // Run alone, the same call does the work, so the zeros above are the check's.
    diacraft_calc::work::reset();
    run(&mut s, "file.new", json!({"template": "basic_flowchart"}));
    assert!(diacraft_calc::work::commits() > 0);
}

#[test]
fn a_macro_enabled_template_starts_a_drawing_that_keeps_its_project_and_will_not_lose_it_unasked() {
    // A `.vstm`: the project and its signature are bytes of the template, never run and never read. The drawing made from it carries them to a
    // macro type, and a type that cannot hold them is refused unless the caller says to leave them out.
    let dir = Dir::new();
    let bytes = VsdxBuilder::new()
        .document_type(CT_TEMPLATE_MACRO)
        .page(PageSpec::new(1, "First"))
        .with(
            Pkg::new()
                .default_type("bin", "application/vnd.ms-office.vbaProject")
                .part_without_override("/visio/vbaProject.bin", VBA_BYTES)
                .rel("/visio/document.xml", "rId50", VBA_REL, "vbaProject.bin")
                .part("/visio/vbaProjectSignature.bin", "application/vnd.ms-office.vbaProjectSignature", b"sig")
                .rel("/visio/vbaProject.bin", "rId1", VBA_SIGNATURE_REL, "vbaProjectSignature.bin"),
        )
        .build();
    let file = dir.write("macros.vstm", &bytes);
    let mut s = Session::new();
    let r = run(&mut s, "file.new", json!({"template": file}));
    assert_eq!((r["file"]["variant"].as_str(), r["file"]["kind"].as_str()), (Some("vstm"), Some("template")));
    assert_eq!(r["pages"][0]["name"], "First");
    assert!(s.path().is_none() && s.source().is_none(), "a new drawing with no file and none of the template's bytes (NP10)");
    assert_eq!(std::fs::read(&file).unwrap(), bytes, "the template is never written");
    // A type that cannot hold macros is refused, nothing is written, and the error is the typed one a window asks the person about.
    let vsdx = dir.path("out.vsdx");
    let e = s.execute("file.save_as", &json!({"path": vsdx})).unwrap_err();
    assert!(matches!(e, EngineError::MacrosNotAllowed { .. }), "{e:?}");
    assert!(!std::path::Path::new(&vsdx).exists(), "nothing was written");
    // A macro type keeps both, byte for byte; saved as a template again too.
    for name in ["out.vsdm", "out.vstm"] {
        let out = dir.path(name);
        let saved = run(&mut s, "file.save_as", json!({"path": out}));
        assert_eq!(saved["macros_dropped"], 0, "{name}");
        let kept = parts(&std::fs::read(&out).unwrap());
        assert_eq!(kept.get("/visio/vbaProject.bin").map(Vec::as_slice), Some(VBA_BYTES), "{name}");
        assert_eq!(kept.get("/visio/vbaProjectSignature.bin").map(Vec::as_slice), Some(&b"sig"[..]), "{name}");
    }
    // And with the choice made, the project is left out and the answer says so.
    let dropped = run(&mut s, "file.save_as", json!({"path": dir.path("plain.vsdx"), "drop_macros": true}));
    assert!(dropped["macros_dropped"].as_u64().unwrap() >= 2, "{dropped}");
    assert!(!parts(&std::fs::read(dir.path("plain.vsdx")).unwrap()).contains_key("/visio/vbaProject.bin"));
}

#[test]
fn a_file_over_the_most_a_package_file_may_be_is_refused_from_its_size_and_leaves_the_session_as_it_was() {
    // A sparse file one byte over the cap (`file::package_file_cap`): it is refused from its size and none of it is read, which a program that names
    // a huge file cannot make DiaCraft do.
    let dir = Dir::new();
    let cap = diacraft_engine::file::package_file_cap(&diacraft_vsdx::Limits::default());
    let path = dir.path("huge.vstx");
    std::fs::File::create(&path).unwrap().set_len(cap + 1).unwrap();
    let (mut s, _stencils) = with_stencil();
    new_from(&mut s, "basic_diagram");
    let stencils_before = run(&mut s, "stencil.list", json!({}));
    let before = snapshot(&s);
    let e = s.execute("file.new", &json!({"template": path})).unwrap_err();
    assert!(matches!(e, EngineError::CannotOpen { problem: OpenProblem::FileTooLarge, .. }), "{e:?}");
    assert!(e.to_string().contains("larger than"), "{e}");
    assert!(!e.to_string().contains("huge"), "the path is not in the words: {e}");
    assert_eq!(snapshot(&s), before);
    assert_eq!(run(&mut s, "stencil.list", json!({})), stencils_before);
    // `file.open` of it is the same refusal, so the two read one way.
    let e = s.execute("file.open", &json!({"path": path})).unwrap_err();
    assert!(matches!(e, EngineError::CannotOpen { problem: OpenProblem::FileTooLarge, .. }), "{e:?}");
}

#[test]
fn a_drawing_or_a_stencil_is_told_where_to_go() {
    let dir = Dir::new();
    let drawing = dir.write("drawing.vsdx", &VsdxBuilder::new().page(PageSpec::new(1, "First")).build());
    let mut s = Session::new();
    let e = s.execute("file.new", &json!({"template": drawing})).unwrap_err();
    assert!(matches!(e, EngineError::Failed { .. }), "{e:?}");
    assert!(e.to_string().contains("file.open"), "{e}");
}

// ---- the built-in stencils open and close by hand ---------------------------------------------------------------------------------------

#[test]
fn a_built_in_stencil_a_template_left_out_opens_by_id_and_its_masters_drop_by_name_either_way() {
    let mut s = Session::new();
    new_from(&mut s, "basic_diagram");
    assert_eq!(open_stencils(&mut s), ["basic", "arrows"]);
    // `shape.drop` finds a master by name whether or not its stencil is in the window.
    let id = drop_shape(&mut s, "process", 3.0, 3.0);
    assert!(doc(&s).find_shape(Owner::Page(1), id).is_some());
    let r = run(&mut s, "stencil.open", json!({"id": " Flowchart "}));
    assert_eq!((r["id"].as_str(), r["already_open"].as_bool(), r["masters_total"].as_u64()), (Some("flowchart"), Some(false), Some(18)));
    assert_eq!(open_stencils(&mut s), ["basic", "arrows", "flowchart"], "a stencil opened by hand goes last");
    assert_eq!(available_stencils(&mut s), ["decorative", "gane-sarson", "cross-functional", "orgchart"]);
    let again = run(&mut s, "stencil.open", json!({"id": "flowchart"}));
    assert_eq!(again["already_open"], true);
    assert_eq!(open_stencils(&mut s), ["basic", "arrows", "flowchart"]);
    // A built-in stencil still has no close: the window's own rows have no close control, and a template decides which are open.
    let msg = refusal(&mut s, "stencil.close", json!({"id": "basic"}));
    assert!(msg.contains("built-in"), "{msg}");
}

#[test]
fn the_stencil_commands_refuse_what_they_cannot_do_and_a_failed_batch_puts_the_windows_back() {
    let mut s = Session::new();
    new_from(&mut s, "basic_diagram");
    for (id, params, words) in [
        ("stencil.open", json!({"id": "nope"}), "no built-in stencil"),
        ("stencil.open", json!({"id": "x".repeat(100)}), "no built-in stencil"),
        ("stencil.open", json!({"id": "basic", "path": "/x.vssx"}), "not both"),
        ("stencil.open", json!({}), "missing"),
        ("stencil.open", json!({"id": 5}), "must be text"),
        ("stencil.close", json!({"id": "file-9"}), "no stencil"),
    ] {
        let msg = refusal(&mut s, id, params.clone());
        assert!(msg.contains(words), "{id} {params}: {msg}");
    }
    let before = run(&mut s, "stencil.list", json!({}));
    let e = s
        .execute(
            "command.batch",
            &json!({"commands": [
                {"command": "stencil.open", "params": {"id": "flowchart"}},
                {"command": "stencil.close", "params": {"id": "file-9"}},
            ]}),
        )
        .unwrap_err();
    assert!(e.to_string().contains("file-9"), "{e}");
    assert_eq!(run(&mut s, "stencil.list", json!({})), before, "the stencil the batch opened is not open");
}

// ---- Save As a stencil of the masters a drawing from a template uses ---------------------------------------------------------------------

#[test]
fn a_drawing_from_a_template_saved_as_a_stencil_offers_the_masters_it_dropped() {
    let dir = Dir::new();
    let mut s = Session::new();
    new_from(&mut s, "basic_flowchart");
    drop_shape(&mut s, "process", 4.0, 5.0);
    drop_shape(&mut s, "decision", 4.0, 7.0);
    let vssx = dir.path("mine.vssx");
    let saved = run(&mut s, "file.save_as", json!({"path": vssx}));
    assert_eq!(saved["variant"], "vssx");
    // The stencil opens as a stencil, and lists the two masters the drawing holds, by their names.
    let mut t = Session::new();
    let opened = run(&mut t, "stencil.open", json!({"path": vssx}));
    let names: Vec<&str> = opened["masters"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&"Process") && names.contains(&"Decision"), "{names:?}");
    // And it is not a template: a drawing cannot be started from it.
    let e = t.execute("file.new", &json!({"template": vssx})).unwrap_err();
    assert!(e.to_string().contains("stencil, not a template"), "{e}");
}

// ---- cost -------------------------------------------------------------------------------------------------------------------------------

#[test]
fn starting_from_a_template_costs_the_same_whatever_the_old_drawing_held() {
    // Counted, not timed: the recalculation the template's theme asks for, on its empty page, and nothing of the drawing it replaces.
    let counted = |s: &mut Session| {
        diacraft_calc::work::reset();
        diacraft_doc::work::reset();
        run(s, "file.new", json!({"template": "basic_flowchart"}));
        (diacraft_calc::work::commits(), diacraft_calc::work::steps(), diacraft_doc::work::resolver_builds())
    };
    diacraft_calc::work::reset();
    template::build("test", template::find("basic_flowchart").unwrap()).unwrap();
    let building = (diacraft_calc::work::commits(), diacraft_calc::work::steps());
    eprintln!("building the template alone: {} commits, {} steps", building.0, building.1);
    let mut fresh = Session::new();
    let on_nothing = counted(&mut fresh);
    let mut busy = crate::edit_cost::big_page();
    let on_a_big_page = counted(&mut busy);
    assert_eq!(on_a_big_page, on_nothing, "the 800 shapes of the page it replaces are not touched");
    eprintln!("file.new from a template: {} commit, {} steps, {} resolvers built", on_nothing.0, on_nothing.1, on_nothing.2);
    // All of it is making the template (its page size and turn and its theme each work out a page of no shapes); reading it back works out nothing.
    assert_eq!((on_nothing.0, on_nothing.1), building, "starting a drawing costs what making the template does and no more");
    assert!(building.0 <= 4 && building.1 <= 100, "a few commits and a few dozen steps, whatever the machine: {building:?}");
    // The plain template has no theme, so it costs less: its page size and turn only.
    diacraft_calc::work::reset();
    run(&mut busy, "file.new", json!({"template": "basic_diagram"}));
    let plain = (diacraft_calc::work::commits(), diacraft_calc::work::steps());
    assert!(plain.0 <= building.0 && plain.1 < building.1, "{plain:?} against {building:?}");
    // And a hundred of them leave the session as small as one: nothing accumulates in the journal's memory or the history.
    let journaled = busy.journal().len();
    let started = std::time::Instant::now();
    for _ in 0..100 {
        run(&mut busy, "file.new", json!({"template": "basic_flowchart"}));
    }
    // Printed and not asserted: a time moves with the load on the machine (the counts above are the test).
    eprintln!("file.new from Basic Flowchart: {:?} each, 100 in a row", started.elapsed() / 100);
    assert_eq!(check_session(&busy), Ok(()));
    assert_eq!(busy.history().unwrap().undo_len(), 0);
    // The journal records each success with its params and keeps no more than its bound: a hundred more entries, the newest the last call, and its
    // memory under the budget (`check_session` holds the entry count; this holds that the starts were recorded, and what).
    let journal = busy.journal();
    assert_eq!(journal.len(), (journaled + 100).min(diacraft_engine::JOURNAL_LIMIT), "every start is one entry");
    assert_eq!(journal.last(), Some(&("file.new".to_owned(), json!({"template": "basic_flowchart"}))));
    assert!(busy.journal_log().held_bytes() <= diacraft_engine::JOURNAL_MAX_BYTES, "{} bytes held", busy.journal_log().held_bytes());
}

/// A template file that makes the reader warn more than the answer lists, under a name that is not a template's: the answer lists 20 warnings, **the extension's note first**
/// and the reader's after it (it listed 21 when the note was put in after the cap), and `file.warning_count` is the whole (the review item 4).
#[test]
fn a_template_file_lists_twenty_warnings_with_the_extension_note_first_and_counts_them_all() {
    let dir = Dir::new();
    let mut builder = VsdxBuilder::new().document_type(CT_TEMPLATE);
    for n in 0..25 {
        builder = builder.page(PageSpec::new(1, &format!("Twin {n}")));
    }
    let file = dir.write("many.vsdx", &builder.build());
    let mut s = Session::new();
    let r = run(&mut s, "file.new", json!({"template": file}));
    let words: Vec<&str> = r["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect();
    assert_eq!(words.len(), 20, "{words:?}");
    assert!(words[0].contains("extension"), "the extension's note comes first: {words:?}");
    assert!(words[1..].iter().all(|w| w.contains("ID 1")), "and the reader's follow it: {words:?}");
    assert!(r["file"]["warning_count"].as_u64().unwrap() >= 25, "the count is the whole: {r}");
}

// ---- More Shapes: every built-in stencil, by category -------------------------------------------------------------------------

/// Our rule ("a menu of stencil categories similar to the template categories"). `stencil.list` names the category of each
/// built-in stencil, whether it is open or `available`, so that a window can group them: the words are the template categories' (General, Flowchart) and one more for the notation
/// of data flow diagrams. A stencil opened from a file has none.
#[test]
fn stencil_list_names_the_category_of_every_built_in_stencil_open_or_available() {
    let mut s = Session::new();
    new_from(&mut s, "basic_flowchart");
    let list = run(&mut s, "stencil.list", json!({}));
    let category = |group: &str, id: &str| {
        list[group].as_array().and_then(|all| all.iter().find(|x| x["id"] == id)).and_then(|x| x["category"].as_str()).map(str::to_owned)
    };
    assert_eq!(category("stencils", "flowchart").as_deref(), Some("Flowchart"));
    assert_eq!(category("stencils", "basic").as_deref(), Some("General"));
    assert_eq!(category("available", "decorative").as_deref(), Some("General"));
    assert_eq!(category("available", "arrows").as_deref(), Some("General"));
    assert_eq!(category("available", "gane-sarson").as_deref(), Some("Software and Database"));
    // Every built-in stencil, open or not, has one, and a template's categories are among them.
    let all = list["stencils"].as_array().unwrap().iter().chain(list["available"].as_array().unwrap());
    assert!(all.clone().all(|x| x["category"].as_str().is_some_and(|c| !c.is_empty())), "{list}");
    let templates = run(&mut s, "template.list", json!({}));
    let ours: Vec<&str> = all.filter_map(|x| x["category"].as_str()).collect();
    for t in templates["templates"].as_array().unwrap() {
        assert!(ours.contains(&t["category"].as_str().unwrap()), "the template category {} is a stencil category too", t["category"]);
    }
}
