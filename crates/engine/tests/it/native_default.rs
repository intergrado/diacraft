//! `.diacraft` is the default (the engine half of the switch): a new drawing keeps its model and no file's bytes (NP10), its first save is a `.diacraft` file, a Save As to a VSDX
//! type writes it from the model, an opened `.vsdx` keeps saving as VSDX with nothing said, and the refusals that stay name `.diacraft` as a way out.
//!
//! The eight tests of the plan's step 1 are here, by their names (the two that said "until ODFTAG" say the opposite now: `.odg` to `.diacraft` is lifted).

use diacraft_doc::Document;
use diacraft_engine::{EngineError, Session};
use diacraft_sheetxml::model::{Budget, content_stamp, open_limits};
use serde_json::{Value, json};

use crate::edit::drop_shape;
use crate::native_common::{Dir, PINNED, made, opened, status, vsdx_drawing};
use crate::odg_build::Drawing;
use crate::{check_session, run};

/// The built-in templates a new drawing may start from, and the blank drawing (`None`).
const STARTS: [Option<&str>; 3] = [None, Some("basic_diagram"), Some("basic_flowchart")];

/// A new drawing as `file.new` makes it, with the clock pinned.
fn new_drawing(template: Option<&str>) -> Session {
    let mut s = Session::new();
    s.pin_clock(Some(PINNED));
    match template {
        None => run(&mut s, "file.new", json!({})),
        Some(id) => run(&mut s, "file.new", json!({ "template": id })),
    };
    s
}

/// The fingerprint of the model a drawing holds, **with the property times cleared** (a clock is no part of the model a new drawing starts with): the content stamp, which is the hash of the
/// model's canonical encoding and so of everything the model holds (held slots and DiaCraft data included).
fn fingerprint(doc: &Document) -> String {
    let mut doc = doc.clone();
    let props = std::sync::Arc::make_mut(&mut doc.props);
    props.created = None;
    props.modified = None;
    props.last_printed = None;
    let mut budget = Budget::for_open(&open_limits());
    content_stamp(&doc, &mut budget).expect("the model of a new drawing encodes").text()
}

fn refusal(s: &mut Session, command: &str, params: Value) -> String {
    match s.execute(command, &params) {
        Err(EngineError::Failed { msg, .. } | EngineError::BadParams { msg, .. }) => msg,
        other => panic!("{command} {params}: expected a refusal, got {other:?}"),
    }
}

/// What a reader of the drawing sees of its content: each page's name and its top-level shapes' `ID`, master and frame cells, and the masters by name.
type Seen = (Vec<(String, Vec<(u32, Option<u32>, Vec<Option<diacraft_doc::Cell>>)>)>, Vec<Option<String>>);

fn content_of(doc: &Document) -> Seen {
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

fn entry_text(bytes: &[u8], name: &str) -> String {
    let pkg = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
    String::from_utf8(pkg.part(name).unwrap_or_else(|| panic!("no part {name}")).to_vec()).unwrap()
}

// ---- the model a new drawing starts with ------------------------------------------------------------------------------------------------

/// **The trap fell into**: the model a new drawing starts with must stay exactly as it was (changing it moved page 1 to page 0, and 677 engine tests failed). For the blank drawing and
/// each built-in template, the model after `file.new` is the model of the commit before this task (the pins were taken on main at 0bf05612, which holds what the commit the plan names,
/// ae19066, did not: the held VSDX slots and the DiaCraft data, so the pins are of that model and not of the older one), with the property times cleared; and page 1's `ID` is 1 and the
/// page's name is the one a new drawing has.
#[test]
fn a_new_drawings_model_is_exactly_as_before() {
    let pins: [(Option<&str>, &str); 3] =
        [(None, "d60ea187d92bb3b5"), (Some("basic_diagram"), "86c97b0377bff463"), (Some("basic_flowchart"), "f97d0c44d5e76a27")];
    let mut seen: Vec<(Option<&str>, String)> = Vec::new();
    for (template, _) in pins {
        let s = new_drawing(template);
        let doc = s.document().unwrap();
        let ids: Vec<u32> = doc.pages.iter().map(|p| p.id).collect();
        assert_eq!(ids, [1], "{template:?}: page 1 is still page 1");
        assert_eq!(doc.pages.get(0).map(|p| p.name.to_string()).as_deref(), Some("Page-1"), "{template:?}");
        assert!(!s.is_dirty(), "{template:?}: a new drawing is not unsaved until it is edited");
        seen.push((template, fingerprint(doc)));
    }
    let want: Vec<(Option<&str>, String)> = pins.iter().map(|(template, pin)| (*template, (*pin).to_owned())).collect();
    assert_eq!(seen, want, "the model a new drawing starts with moved");
}

// ---- no bytes ---------------------------------------------------------------------------------------------------------------------------

/// **NP10**: a new drawing keeps its model and no bytes, whether it came from a built-in template, a template file of either format, or nothing. (A template opened to be changed is the file
/// itself, and keeps it.) The session holds no file for it and no memory for one.
#[test]
fn a_new_drawing_keeps_no_template_bytes() {
    let dir = Dir::new();
    let vstx = dir.write("t.vstx", &crate::template::template_with_list(None));
    // A `.diacraft-template` made by the engine, from a drawing that has a theme and shapes.
    let mut maker = new_drawing(None);
    made(&mut maker);
    run(&mut maker, "file.save_as", json!({ "path": dir.path("t.diacraft-template") }));
    let native = dir.path("t.diacraft-template");
    for template in STARTS {
        let s = new_drawing(template);
        assert!(s.source().is_none(), "{template:?}: a new drawing has no file's bytes");
        assert_eq!(s.held_memory().drawing, 0, "{template:?}: and holds none");
        assert!(s.path().is_none(), "{template:?}");
    }
    // The template files, started from by `file.new {template: path}` and opened by `file.open`, which makes a new drawing of one.
    for path in [vstx.as_str(), native.as_str()] {
        let mut s = Session::new();
        run(&mut s, "file.new", json!({ "template": path }));
        assert!(s.source().is_none(), "{path}: file.new of a template file keeps no bytes");
        assert_eq!(s.held_memory().drawing, 0, "{path}");
        let mut t = Session::new();
        let opened = run(&mut t, "file.open", json!({ "path": path }));
        assert_eq!(opened["file"]["new_from_template"], true, "{opened}");
        assert!(t.source().is_none(), "{path}: file.open of a template makes a new drawing that keeps no bytes");
        assert_eq!(t.held_memory().drawing, 0, "{path}");
        assert_eq!(status(&mut t), json!({"dirty": false, "has_file": false, "format": null, "variant": null, "kind": null}));
        // The same template opened to be changed is the file itself: it keeps its bytes and its name.
        let mut e = Session::new();
        run(&mut e, "file.open", json!({ "path": path, "edit_template": true }));
        assert!(e.source().is_some_and(|f| f.is_named() && !f.bytes().is_empty()), "{path}: the template itself keeps its file");
    }
    // The drawing is as it was: the template's page, theme and stencils are the model's.
    let s = new_drawing(Some("basic_flowchart"));
    assert_eq!(s.document().unwrap().pages.len(), 1);
    assert_eq!(s.stencils().open_builtins(), ["flowchart", "basic"]);
    check_session(&s).unwrap();
}

// ---- the first save ---------------------------------------------------------------------------------------------------------------------

/// A new drawing's first save is `.diacraft`: it is written from the model alone, copies nothing (there is no package to copy from), is a drawing of the kind its name says, reads back as the
/// same model, and is the same bytes when saved again. `file.save` of a drawing with no file says to give it one with Save As, and writes nothing.
#[test]
fn a_new_drawing_saves_as_diacraft_first() {
    for template in STARTS {
        let dir = Dir::new();
        let mut s = new_drawing(template);
        made(&mut s);
        // A drawing that has no file has nowhere to save to; Save As gives it one.
        let words = refusal(&mut s, "file.save", json!({}));
        assert!(words.contains("file.save_as"), "{template:?}: {words}");
        assert!(dir.files().is_empty());
        let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
        assert_eq!((saved["variant"].as_str(), saved["kind"].as_str()), (Some("diacraft"), Some("drawing")), "{template:?}: {saved}");
        assert_eq!(saved["copied"], 0, "{template:?}: a first save copies nothing");
        assert_eq!(saved["warning_count"], 0, "{template:?}: {saved}");
        assert_eq!(saved["dirty"], false);
        let bytes = dir.read("a.diacraft");
        assert!(diacraft_native::sniff(&bytes).is_some(), "{template:?}: a DiaCraft package, not a VSDX one");
        assert_eq!(
            status(&mut s),
            json!({"dirty": false, "has_file": true, "format": "diacraft", "variant": "diacraft", "kind": "drawing"}),
            "{template:?}"
        );
        // It reads back as the same model, and the same again.
        let mut t = Session::new();
        t.pin_clock(Some(PINNED));
        run(&mut t, "file.open", json!({ "path": dir.path("a.diacraft") }));
        assert!(s.document() == t.document(), "{template:?}: the model read back is the model saved");
        run(&mut t, "file.save_as", json!({ "path": dir.path("b.diacraft") }));
        assert_eq!(dir.read("b.diacraft"), bytes, "{template:?}: the save after a reopen is the save");
        // And the drawing is saved to its own file from now on, in its own format.
        drop_shape(&mut s, "rectangle", 4.0, 4.0);
        assert_eq!(run(&mut s, "file.save", json!({}))["variant"], "diacraft", "{template:?}");
    }
}

// ---- Save As to a VSDX type -------------------------------------------------------------------------------------------------------------

/// A new drawing saved as a VSDX type is written from the model by the model writer (there is no file to splice into: the template's bytes are not kept), and the file reads back as the
/// drawing. A drawing that has a template's parts (a part nothing models, in a `.vstx`) writes them from the model's held parts. The times of a copy made from a template are the save's.
/// Written from `.diacraft` and from the file, the content is the same: the second generation of the file is the first one's content.
#[test]
fn a_new_drawing_save_as_vsdx_uses_the_model_writer_and_reads_back_equal() {
    let dir = Dir::new();
    for template in STARTS {
        let mut s = new_drawing(template);
        made(&mut s);
        let drawn = content_of(s.document().unwrap());
        let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("a.vsdx") }));
        assert_eq!(saved["variant"], "vsdx", "{template:?}: {saved}");
        // The model writer makes every part of a drawing that holds a template's model, and carries only the parts the model holds as bytes: no part of a file is taken over, for there is none (the
        // splice into the template's bytes kept six). A blank drawing holds nothing of a package, and its parts are the writer's skeleton, as they always were.
        if template.is_some() {
            let held = s.document().unwrap().parts.iter().filter(|p| p.format == diacraft_doc::Format::Vsdx).count();
            assert_eq!(saved["parts"]["kept"], held, "{template:?}: written from the model alone: {saved}");
            assert_eq!(saved["warning_count"], 0, "{template:?}: a drawing only made and saved is told nothing: {saved}");
        }
        assert_eq!(status(&mut s)["format"], "vsdx", "{template:?}: the working file is the VSDX file now");
        let bytes = dir.read("a.vsdx");
        assert!(diacraft_native::sniff(&bytes).is_none());
        let mut t = Session::new();
        t.pin_clock(Some(PINNED));
        run(&mut t, "file.open", json!({ "path": dir.path("a.vsdx") }));
        assert_eq!(content_of(t.document().unwrap()), drawn, "{template:?}: the file holds the drawing");
        assert_eq!(t.document().unwrap().themes.len(), s.document().unwrap().themes.len(), "{template:?}: and its themes");
        check_session(&t).unwrap();
        // The second generation, by way of `.diacraft`, has the content of the first.
        run(&mut t, "file.save_as", json!({ "path": dir.path("b.diacraft") }));
        let mut u = Session::new();
        u.pin_clock(Some(PINNED));
        run(&mut u, "file.open", json!({ "path": dir.path("b.diacraft") }));
        run(&mut u, "file.save_as", json!({ "path": dir.path("c.vsdx") }));
        let same = diacraft_vsdx::compare_content(&bytes, &dir.read("c.vsdx"), &diacraft_vsdx::Limits::default()).unwrap();
        assert!(same.same(), "{template:?}: the content of the VSDX file is what it was: {:?}", same.shown);
    }
    // A template file's parts and the times of a copy made from it, for each of the three VSDX template types.
    for ext in ["vsdx", "vstx"] {
        let mut s = Session::new();
        s.pin_clock(Some(PINNED));
        run(&mut s, "file.open", json!({ "path": dir.write("t.vstx", &crate::template::template_with_list(Some("basic"))) }));
        assert!(s.source().is_none());
        let out = dir.path(&format!("copy.{ext}"));
        let saved = run(&mut s, "file.save_as", json!({ "path": out }));
        assert_eq!(saved["variant"], ext, "{saved}");
        assert_eq!(saved["warning_count"], 0, "{ext}: {saved}");
        let core = entry_text(&dir.read(&format!("copy.{ext}")), "/docProps/core.xml");
        assert_eq!(core.matches("2030-03-17T17:46:40Z").count(), 2, "{ext}: created and modified are the save's: {core}");
        let mut t = Session::new();
        run(&mut t, "file.open", json!({ "path": out, "edit_template": true }));
        assert_eq!(content_of(t.document().unwrap()), content_of(s.document().unwrap()), "{ext}");
    }
}

// ---- an opened VSDX file ----------------------------------------------------------------------------------------------------------------

/// An opened `.vsdx` keeps saving as VSDX, silently (Call 16): Save, and Save As to another VSDX name, splice into the file's bytes (a part nothing models comes through byte for byte, and
/// the parts the edit did not reach are the file's own), no note or warning names `.diacraft`, and the format the status says is still VSDX.
#[test]
fn an_opened_vsdx_save_keeps_vsdx_silently() {
    let dir = Dir::new();
    let mut s = opened(&dir.write("a.vsdx", &vsdx_drawing()));
    drop_shape(&mut s, "rectangle", 4.0, 4.0);
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["variant"], "vsdx", "{saved}");
    assert_eq!((saved["warning_count"].as_u64(), saved["macros_dropped"].as_u64()), (Some(0), Some(0)), "silently: {saved}");
    assert!(saved["parts"]["kept"].as_u64().is_some_and(|kept| kept > 0), "spliced into the file's own parts: {saved}");
    assert!(!saved.to_string().to_lowercase().contains("diacraft"), "nothing is said of .diacraft: {saved}");
    let file = dir.read("a.vsdx");
    assert!(diacraft_native::sniff(&file).is_none(), "an OPC package, not a .diacraft file");
    let pkg = diacraft_opc::Package::read(&file, &diacraft_opc::Limits::default()).unwrap();
    assert_eq!(pkg.part("/foo/bar.dat"), Some(&b"0123456789"[..]), "a part nothing models is the file's, byte for byte");
    assert_eq!(status(&mut s)["format"], "vsdx");
    assert_eq!(dir.files(), ["a.vsdx"], "no other file was made");
    // Save As to another VSDX name splices too, and does not turn the drawing into a `.diacraft` one.
    let again = run(&mut s, "file.save_as", json!({ "path": dir.path("b.vsdx") }));
    assert_eq!(again["variant"], "vsdx");
    assert!(again["parts"]["kept"].as_u64().is_some_and(|kept| kept > 0), "{again}");
    assert_eq!(status(&mut s)["format"], "vsdx");
    // It opens as the drawing it was.
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": dir.path("b.vsdx") }));
    assert_eq!(content_of(t.document().unwrap()), content_of(s.document().unwrap()));
}

// ---- the refusals that stay -------------------------------------------------------------------------------------------------------------

/// **Call 18's refusal is lifted** (which wires the `.odg` model writer built): a `.diacraft` drawing, one opened from a `.diacraft` file and a new drawing (which is
/// one from the start, from a built-in template too) are saved as an `.odg`, and the file becomes the drawing's own. (`native_odg.rs` holds the rest: the model writer's file, the stamp,
/// the visits.)
#[test]
fn save_as_odg_from_diacraft_is_no_longer_refused() {
    let dir = Dir::new();
    let mut working = new_drawing(None);
    made(&mut working);
    run(&mut working, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let mut reopened = opened(&dir.path("a.diacraft"));
    let mut brand_new = new_drawing(Some("basic_flowchart"));
    for (n, (what, s)) in
        [("saved as .diacraft", &mut working), ("opened from .diacraft", &mut reopened), ("new, from a built-in template", &mut brand_new)]
            .into_iter()
            .enumerate()
    {
        let name = format!("out{n}.odg");
        let saved = run(s, "file.save_as", json!({ "path": dir.path(&name) }));
        assert_eq!((saved["variant"].as_str(), saved["kind"].as_str()), (Some("odg"), Some("drawing")), "{what}: {saved}");
        assert!(dir.files().contains(&name), "{what}");
        assert_eq!(status(s)["format"], "odg", "{what}: the file written is the drawing's own");
    }
}

/// The opposite of what the plan said before ODFTAG: a drawing opened from an `.odg` is saved as `.diacraft` (the model holds what the `.odg` holds), and the file reopens with the same model.
#[test]
fn save_as_diacraft_of_an_odg_opened_drawing_is_saved_and_reopens_the_same() {
    let dir = Dir::new();
    let mut s = Session::new();
    s.pin_clock(Some(PINNED));
    let rect = r#"<draw:rect draw:name="R" draw:style-name="gr1" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><text:p>hello</text:p></draw:rect>"#;
    run(&mut s, "file.open", json!({ "path": dir.write("a.odg", &Drawing::new().page(rect).bytes()) }));
    run(&mut s, "page.add", json!({"name": "Edited"}));
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    assert_eq!((saved["variant"].as_str(), saved["kind"].as_str()), (Some("diacraft"), Some("drawing")), "{saved}");
    assert_eq!(status(&mut s)["format"], "diacraft", "the working file is the package now");
    let mut t = opened(&dir.path("a.diacraft"));
    assert_eq!(status(&mut t)["format"], "diacraft");
    assert!(s.document() == t.document(), "the model read back is the model of the `.odg` drawing");
    // The `.odg` file is as it was, and the drawing goes on in its new file.
    assert_eq!(dir.files(), ["a.diacraft", "a.odg"]);
    run(&mut t, "page.add", json!({"name": "Again"}));
    assert_eq!(run(&mut t, "file.save", json!({}))["variant"], "diacraft");
}

/// A save across the VSDX and OpenDocument formats is written, and the result says what the file carries; a name of no type is still refused in words that name the types,
/// and `.diacraft` saves as it did.
#[test]
fn the_crossings_are_written_and_a_name_of_no_type_names_the_types() {
    let dir = Dir::new();
    let mut v = opened(&dir.write("a.vsdx", &vsdx_drawing()));
    // (This fixture's first page has no size, which a package of an `.odg` is made for: the refusal says so, in words, and writes nothing.)
    let words = refusal(&mut v, "file.save_as", json!({ "path": dir.path("out.odg") }));
    assert!(words.contains("first page") && words.contains("Nothing was written"), "{words}");
    let mut o = opened(&dir.write("a.odg", &Drawing::new().page("").bytes()));
    let saved = run(&mut o, "file.save_as", json!({ "path": dir.path("out.vsdx") }));
    assert!(saved["carried"]["classes"].is_array(), "{saved}");
    let words = refusal(&mut o, "file.save_as", json!({ "path": dir.path("out.txt") }));
    assert!(words.contains(".diacraft") && words.contains(".odg") && words.contains(".vsdx"), "{words}");
    assert_eq!(dir.files(), ["a.odg", "a.vsdx", "out.vsdx"], "nothing else is written");
    // `.diacraft` is a way out of every refusal: each saves as `.diacraft`.
    assert_eq!(run(&mut v, "file.save_as", json!({ "path": dir.path("v.diacraft") }))["variant"], "diacraft");
    assert_eq!(run(&mut o, "file.save_as", json!({ "path": dir.path("o.diacraft") }))["variant"], "diacraft");
}
