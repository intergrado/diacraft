//! The window opens and saves `.diacraft`, its default (the switch, window half).
//!
//! The Save dialog and the Open dialog are stood in for by pickers that remember what they were asked and answer a path of the test's: the OS's own dialogs are the desktop
//! app's (`dialogs.rs`, `rfd`), and their screenshots are taken by hand. What these tests drive is the window's own part: which kinds the window asks the Save dialog to offer
//! and in what order, the name it starts with, the type a name with none is given, the title after an open and after a Save As, and the view settings a `.diacraft` drawing
//! gets back from the window part of the VSDX file it came from. Every file is built in code.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use diacraft_container::zip::{Archive, ArchiveWriter, Method};
use diacraft_engine::Session;
use diacraft_ui_egui::{FilePicker, SavePicker, SaveRequest};
use egui::Key;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::editing::{drag_tile_to, edit_window_with, key, key_without_waiting};
use crate::fixture;
use crate::saving::{ctrl, ctrl_shift};
use crate::stencil_files::{open_from_the_flyout, opened_ids, tile_names, write_stencil};
use crate::support::{Rest, TempDir, Window, run_command};

/// What the Save dialog was asked, each time it was.
type Asked = Arc<Mutex<Vec<SaveRequest>>>;

/// A Save dialog that remembers what it was asked and answers `answer` (`None`: the person cancels).
fn save_picker(answer: Option<PathBuf>, asked: &Asked) -> SavePicker {
    let asked = Arc::clone(asked);
    Arc::new(move |request| {
        asked.lock().unwrap().push(request);
        answer.clone()
    })
}

fn open_picker(path: PathBuf) -> FilePicker {
    Arc::new(move || Some(path.clone()))
}

/// A window whose Open dialog answers `file` and whose Save dialog answers `answer`, with `file` opened by Ctrl+O.
fn window_on(file: &Path, answer: Option<PathBuf>, asked: &Asked) -> Window {
    let (open, save) = (open_picker(file.to_path_buf()), save_picker(answer, asked));
    let mut w = edit_window_with([1280.0, 900.0], |app| app.with_file_picker(open).with_save_picker(save));
    key_without_waiting(&mut w, Key::O, ctrl());
    w.rest();
    assert!(w.state().open_error().is_none(), "{} did not open: {:?}", file.display(), w.state().open_error());
    w
}

/// A window on a new drawing whose Save dialog answers `answer`.
fn new_window(answer: Option<PathBuf>, asked: &Asked) -> Window {
    let save = save_picker(answer, asked);
    edit_window_with([1280.0, 900.0], |app| app.with_save_picker(save))
}

/// The kinds of the last request, as extensions.
fn offered(asked: &Asked) -> Vec<&'static str> {
    asked.lock().unwrap().last().expect("the Save dialog was asked").kinds.iter().map(|k| k.extension).collect()
}

/// The name the last request started with.
fn started_with(asked: &Asked) -> String {
    asked.lock().unwrap().last().expect("the Save dialog was asked").name.clone()
}

fn save_as(w: &mut Window) {
    key(w, Key::S, ctrl_shift());
}

/// A `.diacraft` drawing of nothing, made by the engine as a new drawing saved to `name`.
fn diacraft_file(dir: &TempDir, name: &str) -> PathBuf {
    let path = dir.file(name);
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    session.execute("file.save_as", &json!({"path": path.to_str().unwrap()})).unwrap();
    path
}

/// A drawing with the demonstration page, as a VSDX file called `name`.
fn vsdx_file(dir: &TempDir, name: &str) -> PathBuf {
    fixture::write(dir.path(), name, &fixture::demo_vsdx())
}

/// A drawing package of one rectangle, `.odg`, by hand: the media type first and stored, the content, the styles and the manifest.
fn odg_file(dir: &TempDir, name: &str) -> PathBuf {
    const DRAWING: &str = "application/vnd.oasis.opendocument.graphics";
    const NS: &str = concat!(
        r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
        r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
        r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0""#
    );
    let content = format!(
        "<?xml version=\"1.0\"?><office:document-content {NS} office:version=\"1.3\"><office:automatic-styles/><office:body><office:drawing><draw:page draw:name=\"p\" draw:master-page-name=\"Default\"><draw:rect draw:name=\"R\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"2cm\" svg:height=\"1cm\"/></draw:page></office:drawing></office:body></office:document-content>"
    );
    let styles = format!(
        "<?xml version=\"1.0\"?><office:document-styles {NS} office:version=\"1.3\"><office:styles/><office:automatic-styles><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\"/></style:page-layout></office:automatic-styles><office:master-styles><draw:layer-set><draw:layer draw:name=\"layout\"/></draw:layer-set><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles></office:document-styles>"
    );
    let manifest = format!(
        "<?xml version=\"1.0\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{DRAWING}\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/></manifest:manifest>"
    );
    let mut w = ArchiveWriter::new();
    w.add("mimetype", DRAWING.as_bytes(), Method::Stored).unwrap();
    w.add("content.xml", content.as_bytes(), Method::Deflated).unwrap();
    w.add("styles.xml", styles.as_bytes(), Method::Deflated).unwrap();
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).unwrap();
    fixture::write(dir.path(), name, &w.finish().unwrap())
}

/// `bytes`, a `.diacraft` package, with the version its manifest states changed to `version`.
fn with_format(bytes: &[u8], version: &str) -> Vec<u8> {
    let mut archive = Archive::read(bytes, &diacraft_container::Limits::default()).unwrap();
    let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
    let mut w = ArchiveWriter::new();
    let mut changed = false;
    for (k, name) in names.iter().enumerate() {
        let inflated = archive.inflate(k, &mut diacraft_container::Work::default()).unwrap();
        let data = if name == "manifest.xml" {
            let text = String::from_utf8(inflated).unwrap();
            let at = text.find("format=\"").expect("the manifest states its version") + "format=\"".len();
            let end = at + text[at..].find('"').unwrap();
            changed = true;
            format!("{}{version}{}", &text[..at], &text[end..]).into_bytes()
        } else {
            inflated
        };
        w.add(name, &data, if name == "mimetype" { Method::Stored } else { Method::Deflated }).unwrap();
    }
    assert!(changed, "the package has a manifest");
    w.finish().unwrap()
}

/// An opened VSDX drawing is offered its own type first, then DiaCraft's drawing and template, then the other VSDX types; the `.odg` the engine does not write for it is not
/// offered, and the dialog starts with the file's own name.
#[test]
fn save_as_for_an_opened_vsdx_lists_its_own_type_first_then_diacraft() {
    let dir = TempDir::new("native-vsdx");
    let asked = Asked::default();
    let mut w = window_on(&vsdx_file(&dir, "plans.vsdx"), None, &asked);
    save_as(&mut w);
    assert_eq!(started_with(&asked), "plans.vsdx");
    assert_eq!(offered(&asked), ["vsdx", "diacraft", "diacraft-template", "odg", "otg", "vsdm", "vssx", "vstx"]);
    let kinds = asked.lock().unwrap().last().unwrap().kinds.clone();
    assert_eq!(kinds[1].label, "DiaCraft drawing (.diacraft)", "the dialog shows what each kind is");
    assert_eq!(w.state().ui_panics(), 0);
}

/// A new drawing is a `.diacraft` drawing: the dialog starts with `Drawing1.diacraft` and selects `.diacraft` first, and a name chosen with no type is given it.
#[test]
fn save_as_for_a_new_drawing_preselects_diacraft() {
    let dir = TempDir::new("native-new");
    let asked = Asked::default();
    let mut w = new_window(Some(dir.file("plan")), &asked);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    save_as(&mut w);
    assert_eq!(started_with(&asked), "Drawing1.diacraft");
    assert_eq!(
        offered(&asked),
        ["diacraft", "diacraft-template", "odg", "otg", "vsdx", "vsdm", "vssx", "vstx"],
        "DiaCraft's own first, then the OpenDocument drawing and its template that the engine writes from the model, then the VSDX types"
    );
    assert!(dir.file("plan.diacraft").exists() && !dir.file("plan").exists(), "a name with no type is a .diacraft drawing");
    assert_eq!(w.state().window_title(), "plan.diacraft - DiaCraft");
    assert!(!w.state().is_dirty());
    // What was written is a drawing of one shape, as DiaCraft's own format.
    let mut other = Session::new();
    other.execute("file.open", &json!({"path": dir.file("plan.diacraft").to_str().unwrap()})).unwrap();
    assert_eq!(other.document().unwrap().pages.get(0).unwrap().shapes.len(), 1);
    assert_eq!(other.source().and_then(|f| f.extension()), Some("diacraft"));
}

/// Ctrl+S on a new drawing asks for a file once, starts with `.diacraft`, and from then on saves in place with no dialog.
#[test]
fn save_on_a_new_drawing_writes_diacraft_and_then_saves_in_place() {
    let dir = TempDir::new("native-save");
    let asked = Asked::default();
    let target = dir.file("first.diacraft");
    let mut w = new_window(Some(target.clone()), &asked);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key(&mut w, Key::S, ctrl());
    assert_eq!(started_with(&asked), "Drawing1.diacraft");
    assert!(target.exists());
    drag_tile_to(&mut w, "Diamond", 7.0, 4.0);
    assert!(w.state().is_dirty());
    key(&mut w, Key::S, ctrl());
    assert_eq!(asked.lock().unwrap().len(), 1, "no second dialog");
    assert!(!w.state().is_dirty());
    let mut again = Session::new();
    again.execute("file.open", &json!({"path": target.to_str().unwrap()})).unwrap();
    assert_eq!(again.document().unwrap().pages.get(0).unwrap().shapes.len(), 2, "the second save was a .diacraft save too");
}

/// A new drawing made from a template (File > New's tiles) is as new: it has no file, and is a `.diacraft` drawing from the start.
#[test]
fn a_drawing_made_from_a_template_is_offered_diacraft_first_too() {
    let asked = Asked::default();
    let mut w = new_window(None, &asked);
    run_command(&mut w, "file.new", json!({"template": "basic_flowchart"}));
    w.rest();
    save_as(&mut w);
    assert!(started_with(&asked).ends_with(".diacraft"), "{}", started_with(&asked));
    assert_eq!(offered(&asked)[0], "diacraft");
}

/// A stencil is offered `.diacraft-stencil` and the VSDX stencil types, its own type first, and no `.odg` (OpenDocument has no stencil format); saved as `.diacraft-stencil`
/// it is that from then on.
#[test]
fn save_as_for_a_stencil_offers_diacraft_stencil_and_the_vsdx_stencil_types() {
    let dir = TempDir::new("native-stencil");
    let asked = Asked::default();
    let target = dir.file("Mine.diacraft-stencil");
    let mut w = window_on(&write_stencil(&dir, "Mine.vssx"), Some(target.clone()), &asked);
    save_as(&mut w);
    assert_eq!(started_with(&asked), "Mine.vssx", "the stencil's own name, with its own type");
    assert_eq!(offered(&asked), ["vssx", "diacraft-stencil", "vssm"]);
    assert!(target.exists(), "saved as the stencil kind the dialog answered");
    assert_eq!(w.state().window_title(), "Mine.diacraft-stencil - DiaCraft");
    save_as(&mut w);
    assert_eq!(started_with(&asked), "Mine.diacraft-stencil");
    assert_eq!(offered(&asked), ["diacraft-stencil", "vssx", "vssm"], "now its own type is DiaCraft's");
}

/// A `.diacraft` drawing is offered `.diacraft` first, then the `.odg` and the VSDX types the engine writes from the model (wired the `.odg` model writer, and the dialog
/// offers what the engine allows with no change of its own).
#[test]
fn save_as_for_a_diacraft_drawing_lists_diacraft_first() {
    let dir = TempDir::new("native-diacraft");
    let asked = Asked::default();
    let mut w = window_on(&diacraft_file(&dir, "plans.diacraft"), None, &asked);
    save_as(&mut w);
    assert_eq!(started_with(&asked), "plans.diacraft");
    assert_eq!(offered(&asked), ["diacraft", "diacraft-template", "odg", "otg", "vsdx", "vsdm", "vssx", "vstx"]);
}

/// An opened `.odg` is offered `.odg` first, then `.diacraft` and its template: a VSDX type is not offered, since the engine does not write one for it.
#[test]
fn save_as_for_an_opened_odg_lists_odg_first_then_diacraft() {
    let dir = TempDir::new("native-odg");
    let asked = Asked::default();
    let mut w = window_on(&odg_file(&dir, "plan.odg"), None, &asked);
    save_as(&mut w);
    assert_eq!(started_with(&asked), "plan.odg");
    assert_eq!(offered(&asked), ["odg", "diacraft", "diacraft-template", "otg", "vsdx", "vsdm", "vssx", "vstx"]);
}

/// **`.otg` is a kind**: the OpenDocument drawing template is offered after the `.odg`, because the engine writes it (`Session::save_as_allows`), a name chosen with that type is
/// written as the template it names, and from then on it is the drawing's own type and the dialog selects it first.
#[test]
fn save_as_otg_writes_a_template_and_it_is_then_the_own_type() {
    let dir = TempDir::new("native-otg");
    let asked = Asked::default();
    let target = dir.file("mine.otg");
    let mut w = new_window(Some(target.clone()), &asked);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    save_as(&mut w);
    assert!(offered(&asked).contains(&"otg"), "{:?}", offered(&asked));
    let kinds = asked.lock().unwrap().last().unwrap().kinds.clone();
    let otg = kinds.iter().find(|k| k.extension == "otg").expect("`.otg` is a kind");
    assert_eq!(otg.label, "OpenDocument drawing template (.otg)");
    assert!(target.exists(), "the template was written");
    assert_eq!(w.state().window_title(), "mine.otg - DiaCraft");
    assert!(w.state().session().save_as_allows("otg"), "the engine allows it");
    let bytes = std::fs::read(&target).unwrap();
    assert!(bytes.windows(b"graphics-template".len()).any(|window| window == b"graphics-template"), "a drawing template, by its media type");
    save_as(&mut w);
    assert_eq!(offered(&asked)[0], "otg", "its own type, selected first");
    assert_eq!(started_with(&asked), "mine.otg");
}

/// The window keeps no table of its own: a kind is offered because the engine says it writes it. Every kind offered, for each kind of drawing, is one `file.save_as` writes.
#[test]
fn every_kind_offered_is_one_the_engine_writes() {
    let dir = TempDir::new("native-agree");
    let sources: [(&str, PathBuf); 4] = [
        ("a VSDX drawing", vsdx_file(&dir, "plans.vsdx")),
        ("a .diacraft drawing", diacraft_file(&dir, "plans.diacraft")),
        ("an .odg drawing", odg_file(&dir, "plan.odg")),
        ("a VSDX stencil", write_stencil(&dir, "Mine.vssx")),
    ];
    for (what, source) in sources {
        let asked = Asked::default();
        let mut w = window_on(&source, None, &asked);
        save_as(&mut w);
        let offered = offered(&asked);
        assert!(offered.len() >= 3, "{what}: {offered:?}");
        for extension in offered {
            let mut session = Session::new();
            session.execute("file.open", &json!({"path": source.to_str().unwrap()})).unwrap();
            let target = dir.file(&format!("agree.{extension}"));
            let saved = session.execute("file.save_as", &json!({"path": target.to_str().unwrap()}));
            // The table offers a type; what a drawing holds may be refused in words (the demo drawing has a background page, which an `.odg` the model writer makes does not hold), never
            // for want of a way: a refusal says "Nothing was written" and names what the drawing holds.
            if let Err(e) = &saved {
                assert!(
                    e.to_string().contains("Nothing was written") && e.to_string().contains("background page"),
                    "{what}: .{extension} is offered, and the engine refuses it: {e}"
                );
            }
            std::fs::remove_file(&target).ok();
        }
    }
}

/// The title is the name of the `.diacraft` file after it is opened, and after a Save As, which switches the drawing to the file written (Call 15), whatever it is written as.
#[test]
fn the_title_shows_the_diacraft_name() {
    let dir = TempDir::new("native-title");
    let asked = Asked::default();
    // After an open.
    let opened = diacraft_file(&dir, "plans.diacraft");
    let w = window_on(&opened, None, &asked);
    assert_eq!(w.state().window_title(), "plans.diacraft - DiaCraft");
    assert!(!w.state().is_dirty());
    w.get_by_label("plans.diacraft - DiaCraft");
    // After a Save As of a VSDX drawing to `.diacraft`, and the star while it is changed.
    let second = dir.file("second.diacraft");
    let mut from_vsdx = window_on(&vsdx_file(&dir, "from.vsdx"), Some(second.clone()), &asked);
    assert_eq!(from_vsdx.state().window_title(), "from.vsdx - DiaCraft");
    save_as(&mut from_vsdx);
    assert_eq!(from_vsdx.state().window_title(), "second.diacraft - DiaCraft", "the drawing is the file it was saved as");
    assert!(second.exists());
    drag_tile_to(&mut from_vsdx, "Rectangle", 4.0, 4.0);
    assert_eq!(from_vsdx.state().window_title(), "*second.diacraft - DiaCraft");
    // Save writes the `.diacraft` file again, in place.
    key(&mut from_vsdx, Key::S, ctrl());
    assert_eq!(from_vsdx.state().window_title(), "second.diacraft - DiaCraft");
    assert_eq!(asked.lock().unwrap().len(), 1, "Save asked for no file: it has one now");
    // And a Save As of the `.diacraft` drawing to a VSDX type switches to that file.
    let third = dir.file("third.vsdx");
    let mut back = window_on(&second, Some(third.clone()), &asked);
    save_as(&mut back);
    assert_eq!(back.state().window_title(), "third.vsdx - DiaCraft", "{:?}", back.state().open_error());
    assert!(third.exists());
}

/// A drawing that came from a VSDX file keeps its window's view settings through `.diacraft`: they are read from the part the model holds, and not from the bytes of the
/// file, which are no VSDX package now.
#[test]
fn window_settings_come_from_the_held_windows_part() {
    let dir = TempDir::new("native-view");
    // The grid and the rulers on (the demonstration drawing's windows part), and then guides off and the connection points on, which are each other than what no value gives.
    let windows = r#"<?xml version="1.0" encoding="utf-8"?><Windows ClientWidth="1280" ClientHeight="720"><Window ID="0" WindowType="Drawing"><ShowGrid>1</ShowGrid><ShowGuides>0</ShowGuides><ShowConnectionPoints>1</ShowConnectionPoints></Window></Windows>"#;
    for (name, bytes, expected) in [
        ("grid", fixture::demo_vsdx(), (true, true, true, false)),
        ("flags", fixture::demo_builder(Some(windows)).build(), (true, false, false, true)),
    ] {
        let vsdx = fixture::write(dir.path(), &format!("{name}.vsdx"), &bytes);
        let diacraft = dir.file(&format!("{name}.diacraft"));
        let asked = Asked::default();
        let first = window_on(&vsdx, Some(diacraft.clone()), &asked);
        let state = |w: &Window| {
            let s = w.state().window_state();
            (s["grid"].as_bool().unwrap(), s["rulers"].as_bool().unwrap(), s["guides"].as_bool().unwrap(), s["connectionPoints"].as_bool().unwrap())
        };
        assert_eq!(state(&first), expected, "{name}: the VSDX file as it was");
        let mut first = first;
        save_as(&mut first);
        assert!(diacraft.exists(), "{name}");
        // A second window opens the `.diacraft` file: the settings come back.
        let second = window_on(&diacraft, None, &asked);
        assert_eq!(state(&second), expected, "{name}: the .diacraft file the VSDX drawing was saved as");
    }
}

/// A `.diacraft` drawing that came from no VSDX file, and whose settings nothing set, holds no windows part, so it is opened as a new drawing shows (plan Ruling
///): the grid, the rulers and the guides on, the connection points off. **A new drawing saved as `.diacraft` comes back as it was** (it was the concern 1: the
/// grid and the rulers came back off).
#[test]
fn a_diacraft_drawing_with_no_windows_part_opens_as_a_new_drawing_shows() {
    let dir = TempDir::new("native-noview");
    let asked = Asked::default();
    let w = window_on(&diacraft_file(&dir, "plain.diacraft"), None, &asked);
    let s = w.state().window_state();
    assert_eq!(
        (s["grid"].as_bool(), s["rulers"].as_bool(), s["guides"].as_bool(), s["connectionPoints"].as_bool()),
        (Some(true), Some(true), Some(true), Some(false))
    );
}

/// A file made by a newer version of the format is told in words that name the version and say it is not damaged, and the drawing that was open is still open: the engine reports
/// it as a file of no known kind, and the window says what it is.
#[test]
fn a_diacraft_file_of_a_newer_major_version_is_told_as_one() {
    let dir = TempDir::new("native-newer");
    let good = diacraft_file(&dir, "good.diacraft");
    let newer = fixture::write(dir.path(), "newer.diacraft", &with_format(&std::fs::read(&good).unwrap(), "2.0"));
    let asked = Asked::default();
    let open = open_picker(newer);
    let mut w = edit_window_with([1280.0, 900.0], |app| app.with_file_picker(open).with_save_picker(save_picker(None, &asked)));
    key_without_waiting(&mut w, Key::O, ctrl());
    w.rest();
    let said = w.state().open_error().expect("the refusal is shown").to_owned();
    assert!(said.contains("newer.diacraft") && said.contains("made by a newer DiaCraft (format 2.0)"), "{said}");
    assert!(said.contains("not damaged") && said.contains("still open"), "{said}");
    assert!(!said.contains("not a drawing file") && !said.contains("is damaged"), "{said}");
    assert!(!said.contains("newer_major") && !said.contains("::"), "no class word: {said}");
    assert_eq!(w.state().window_title(), "Drawing1 - DiaCraft", "the drawing that was open is as it was");
}

/// A `.diacraft` file chosen in the Open dialog is opened as any drawing is, with no word of trouble, and a file that is no drawing is told in words that now name DiaCraft's own types.
#[test]
fn a_diacraft_file_opens_and_the_words_for_a_file_that_is_no_drawing_name_the_new_types() {
    let dir = TempDir::new("native-open");
    let asked = Asked::default();
    let w = window_on(&diacraft_file(&dir, "plans.diacraft"), None, &asked);
    assert!(w.state().open_error().is_none(), "{:?}", w.state().open_error());
    let text = dir.file("notes.diacraft");
    std::fs::write(&text, b"nothing of a drawing").unwrap();
    let mut other = edit_window_with([1280.0, 900.0], |app| app.with_file_picker(open_picker(text)));
    key_without_waiting(&mut other, Key::O, ctrl());
    other.rest();
    let said = other.state().open_error().expect("the refusal is shown").to_owned();
    assert!(
        said.contains("notes.diacraft") && said.contains(".diacraft-stencil") && said.contains(".diacraft-template") && said.contains(".odg"),
        "{said}"
    );
}

/// Open Stencil takes a `.diacraft-stencil` as it takes a VSDX stencil: it has a row, is the one shown, and its masters are tiles.
#[test]
fn open_stencil_takes_a_diacraft_stencil() {
    let dir = TempDir::new("native-stencil-open");
    let vssx = write_stencil(&dir, "Mine.vssx");
    let target = dir.file("Mine.diacraft-stencil");
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": vssx.to_str().unwrap()})).unwrap();
    session.execute("file.save_as", &json!({"path": target.to_str().unwrap()})).unwrap();
    let mut w = crate::stencil_files::window(target);
    open_from_the_flyout(&mut w);
    assert_eq!(opened_ids(&w), ["file-1"]);
    assert_eq!(
        tile_names(&w),
        ["Box", "Triangle", "Bad icon", "Empty"],
        "the masters in the file's order, and the hidden one left out: {:?}",
        w.state().notice()
    );
    assert_eq!(w.state().ui_panics(), 0);
}
