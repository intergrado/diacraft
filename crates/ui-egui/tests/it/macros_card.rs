//! The card for a save the engine refused because the drawing holds a macro project and the type chosen cannot hold one:
//! "Leave the macros out?" with Save without and Cancel. The words are the engine's refusal (`EngineError::MacrosNotAllowed`); Save without runs the same
//! `file.save_as` again with `drop_macros`; Cancel leaves the drawing and its file as they were and ends what was waiting on the save (a New, an Open or a close
//! does not happen).
//!
//! Only `file.save_as` is refused for this reason on main (`file.save` keeps the type, so it never drops a project), and no engine command refuses a save
//! for embedded objects with a parameter to leave them out (no `drop_objects`), so there is no card for objects.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::channel;

use diacraft_engine::{EngineError, Session};
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder, shape};
use diacraft_ui_egui::render_worker::bundled_drawer;
use diacraft_ui_egui::{DiacraftApp, SavePicker};
use egui::{Key, Modifiers, ViewportCommand, ViewportEvent, ViewportId};
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::editing::key;
use crate::fixture;
use crate::saving::ctrl_shift;
use crate::support::{Rest, TempDir, Window, call, click, commands, fast_window, press, run_command, window};

const VBA_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";
const DRAWING_WITH_MACROS: &str = "application/vnd.ms-visio.drawing.macroEnabled.main+xml"; // brand-ok: a content type the format defines
const TEMPLATE_WITH_MACROS: &str = "application/vnd.ms-visio.template.macroEnabled.main+xml"; // brand-ok: a content type the format defines
const CHANGED: &[u8] = b"changed by something else since the drawing was opened";

/// A package of the type `content_type` with one shape (ID 1) and a macro project (one part, never run).
fn with_macros(content_type: &str) -> Vec<u8> {
    let one = shape(1, &[fixture::placed(2.0, 5.5, 2.0, 1.0), fixture::rectangle(2.0, 1.0)].concat());
    VsdxBuilder::new()
        .document_type(content_type)
        .page(PageSpec::new(1, "Plan").shapes(&one))
        .with(Pkg::new().default_type("bin", "application/vnd.ms-office.vbaProject").part_without_override("/visio/vbaProject.bin", b"not code").rel(
            "/visio/document.xml",
            "rId50",
            VBA_REL,
            "vbaProject.bin",
        ))
        .build()
}

fn picker(path: PathBuf) -> SavePicker {
    Arc::new(move |_| Some(path.clone()))
}

/// The window on the file `name` of `dir`, holding `bytes`, and the Save dialog answering `answer`.
fn window_on(dir: &TempDir, name: &str, bytes: &[u8], answer: PathBuf) -> (Window, PathBuf) {
    window_choosing(dir, name, bytes, picker(answer))
}

/// The window on the file `name` of `dir`, holding `bytes`, and `chooser` for the Save dialog.
fn window_choosing(dir: &TempDir, name: &str, bytes: &[u8], chooser: SavePicker) -> (Window, PathBuf) {
    let path = fixture::write(dir.path(), name, bytes);
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let app = DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()).with_save_picker(chooser);
    let mut w = fast_window(app, [1280.0, 900.0]);
    w.rest();
    (w, path)
}

/// A Save dialog that answers the paths it is given, one for each time it is asked (the first first), and none when they are used up.
fn picker_answering(paths: Vec<PathBuf>) -> SavePicker {
    let queue = Arc::new(std::sync::Mutex::new(paths.into_iter().collect::<std::collections::VecDeque<_>>()));
    Arc::new(move |_| queue.lock().unwrap().pop_front())
}

/// The names of the parts of the package at `path`.
fn parts(path: &Path) -> Vec<String> {
    let bytes = std::fs::read(path).unwrap();
    let package = diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
    package.parts().map(str::to_owned).collect()
}

/// What the engine says to a program that asks the same save: the refusal's own words.
fn engines_words(bytes: &[u8]) -> String {
    let dir = TempDir::new("macros-words");
    let path = fixture::write(dir.path(), "in.vsdm", bytes);
    let mut s = Session::new();
    s.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    match s.execute("file.save_as", &json!({"path": dir.file("out.vsdx").to_str().unwrap()})) {
        Err(EngineError::MacrosNotAllowed { msg, .. }) => msg,
        other => panic!("not refused for macros: {other:?}"),
    }
}

fn save_as(w: &mut Window) {
    key(w, Key::S, ctrl_shift());
    w.rest();
}

/// Save As to a macro-free type of a drawing that holds a macro project: the person is asked, in the engine's words, and nothing is written yet.
#[test]
fn a_save_refused_for_macros_is_a_card_in_the_engines_words_and_writes_nothing() {
    let dir = TempDir::new("macros-card");
    let bytes = with_macros(DRAWING_WITH_MACROS);
    let out = dir.file("out.vsdx");
    let (mut w, path) = window_on(&dir, "plan.vsdm", &bytes, out.clone());
    save_as(&mut w);
    assert!(w.state().macros_prompt_open(), "the person is asked");
    assert!(w.state().is_modal_open(), "and nothing under the card takes keys or the pointer");
    assert!(w.state().open_error().is_none(), "the banner says nothing: the card is the answer");
    assert!(!out.exists(), "nothing is written before the person answers");
    w.rest();
    for name in ["macros.save_without", "macros.cancel"] {
        let element = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn"));
        assert!(element.enabled, "{name}");
    }
    w.get_by_label("Leave the macros out?");
    // The words under the heading are the engine's, **the whole of them**: its message with the first letter capital and a full stop at the end.
    let said = engines_words(&bytes);
    assert!(said.contains("macro project") && !said.ends_with('.') && said.starts_with("the "), "the engine's message is not yet a sentence: {said}");
    let sentence = format!("T{}.", said.get(1..).unwrap());
    w.get_by_label(&sentence);
    assert!(w.query_by_label_contains(said.as_str()).is_none(), "no label holds the message as it came, in its lower case");
    assert!(w.query_by_label_contains("file.save_as").is_none(), "no command id");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

/// Cancel (and so Escape) leaves the file, the drawing and its path as they were.
#[test]
fn cancel_leaves_the_document_and_its_path_as_they_were() {
    for escape in [false, true] {
        let dir = TempDir::new("macros-cancel");
        let bytes = with_macros(DRAWING_WITH_MACROS);
        let out = dir.file("out.vsdx");
        let (mut w, path) = window_on(&dir, "plan.vsdm", &bytes, out.clone());
        run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
        let revision = w.state().session().revision();
        save_as(&mut w);
        assert!(w.state().macros_prompt_open());
        if escape {
            key(&mut w, Key::Escape, Modifiers::NONE);
        } else {
            press(&mut w, "macros.cancel");
        }
        w.rest();
        assert!(!w.state().macros_prompt_open() && !w.state().is_modal_open(), "escape: {escape}");
        assert!(!out.exists(), "nothing was written (escape: {escape})");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(w.state().session().path(), Some(path.as_path()), "the drawing still has its own file");
        assert_eq!(w.state().window_title(), "*plan.vsdm - DiaCraft", "still the drawing, still unsaved (escape: {escape})");
        assert_eq!(w.state().session().revision(), revision);
        assert!(w.state().open_error().is_none());
    }
}

/// Save without runs the same save again with `drop_macros`: the file has no macro project, says so in the status bar, and is the drawing's file now.
#[test]
fn save_without_writes_the_file_with_no_macro_project_and_says_so() {
    let dir = TempDir::new("macros-without");
    let bytes = with_macros(DRAWING_WITH_MACROS);
    let out = dir.file("out.vsdx");
    let (mut w, path) = window_on(&dir, "plan.vsdm", &bytes, out.clone());
    assert!(parts(&path).iter().any(|p| p.to_ascii_lowercase().contains("vba")), "the drawing holds a project to begin with");
    save_as(&mut w);
    press(&mut w, "macros.save_without");
    w.rest();
    assert!(!w.state().macros_prompt_open() && !w.state().is_modal_open());
    assert!(out.exists(), "the file was written");
    assert!(!parts(&out).iter().any(|p| p.to_ascii_lowercase().contains("vba")), "{:?}", parts(&out));
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "the macro-enabled file is untouched");
    assert_eq!(w.state().window_title(), "out.vsdx - DiaCraft", "the drawing is named for its new file");
    assert!(!w.state().is_dirty());
    let notice = w.state().notice().unwrap_or_default().to_owned();
    assert!(notice.starts_with("Saved out.vsdx. The macro project was left out of the new file"), "a sentence after the full stop: {notice}");
    assert!(notice.ends_with(").") && !notice.contains(".."), "and a full stop at its end: {notice}");
    assert!(w.state().open_error().is_none());
    // It reads back as a drawing of one shape.
    let mut other = Session::new();
    other.execute("file.open", &json!({"path": out.to_str().unwrap()})).unwrap();
    assert_eq!(other.document().unwrap().pages.get(0).unwrap().shapes.len(), 1);
}

/// A type that holds macros is saved without a question, and so is Save in place: neither is refused.
#[test]
fn a_save_that_is_not_refused_asks_nothing() {
    let dir = TempDir::new("macros-not-asked");
    let bytes = with_macros(DRAWING_WITH_MACROS);
    let (mut w, path) = window_on(&dir, "plan.vsdm", &bytes, dir.file("again.vsdm"));
    run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    key(&mut w, Key::S, crate::saving::ctrl());
    w.rest();
    assert!(!w.state().macros_prompt_open() && !w.state().is_dirty(), "Save keeps the type and the project");
    assert!(parts(&path).iter().any(|p| p.to_ascii_lowercase().contains("vba")));
    run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    save_as(&mut w);
    assert!(!w.state().macros_prompt_open(), "a macro-enabled type holds the project");
    assert!(dir.file("again.vsdm").exists() && parts(&dir.file("again.vsdm")).iter().any(|p| p.to_ascii_lowercase().contains("vba")));
}

/// A close that saves goes through the card: the window closes after Save without, and not at Cancel. The drawing is a copy made from a macro-enabled
/// template, which has no file of its own to save to.
#[test]
fn a_close_that_saves_waits_for_the_answer_about_the_macros() {
    for save_without in [true, false] {
        let dir = TempDir::new("macros-close");
        let out = dir.file("out.vsdx");
        let (mut w, template) = window_on(&dir, "plan.vstm", &with_macros(TEMPLATE_WITH_MACROS), out.clone());
        assert!(w.state().session().path().is_none(), "a template opens as a new drawing with no file");
        run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
        w.input_mut().viewports.entry(ViewportId::ROOT).or_default().events.push(ViewportEvent::Close);
        w.step();
        assert!(w.state().prompt_open());
        w.rest();
        press(&mut w, "prompt.save");
        w.rest();
        assert!(w.state().macros_prompt_open(), "the save met the macros (save without: {save_without})");
        assert!(!commands(&w).iter().any(|c| matches!(c, ViewportCommand::Close)), "and the window has not closed");
        w.rest();
        let button = if save_without { "macros.save_without" } else { "macros.cancel" };
        let at = w.state().element(button).unwrap().rect.center();
        click(&mut w, at);
        let closed = commands(&w).iter().any(|c| matches!(c, ViewportCommand::Close));
        assert_eq!(closed, save_without, "the window closes once the file is saved, and not when the person cancels");
        assert_eq!(out.exists(), save_without);
        if save_without {
            assert!(!parts(&out).iter().any(|p| p.to_ascii_lowercase().contains("vba")));
        } else {
            assert!(w.state().session().path().is_none(), "a Cancel leaves a drawing with no file with none");
            assert!(w.state().is_dirty() && !w.state().prompt_open() && !w.state().macros_prompt_open());
        }
        assert!(template.exists());
    }
}

/// A New that was to follow the save does not happen at Cancel.
#[test]
fn cancelling_the_card_cancels_the_new_that_waited_for_the_save() {
    let dir = TempDir::new("macros-new");
    let (mut w, _) = window_on(&dir, "plan.vstm", &with_macros(TEMPLATE_WITH_MACROS), dir.file("out.vsdx"));
    run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    key(&mut w, Key::N, crate::saving::ctrl());
    press(&mut w, "prompt.save");
    w.rest();
    assert!(w.state().macros_prompt_open());
    press(&mut w, "macros.cancel");
    assert!(!w.state().macros_prompt_open() && !w.state().prompt_open());
    assert!(w.state().is_dirty() && w.state().session().path().is_none(), "still the template's drawing, unsaved");
    assert!(!dir.file("out.vsdx").exists());
}

/// A file that changed on the disk is asked about first, and the macros after Overwrite: Save without keeps the choice to overwrite (`force`).
#[test]
fn the_choice_to_overwrite_a_changed_file_is_kept_when_the_macros_are_asked_about() {
    let dir = TempDir::new("macros-changed");
    let bytes = with_macros(DRAWING_WITH_MACROS);
    // A file called `.vsdx` whose package is of a type with macros: saving it as `.vsdx` changes its type.
    let own = dir.file("plan.vsdx");
    let (mut w, path) = window_on(&dir, "plan.vsdx", &bytes, own.clone());
    assert_eq!(path, own);
    std::fs::write(&own, CHANGED).unwrap();
    save_as(&mut w);
    assert!(w.state().changed_prompt_open() && !w.state().macros_prompt_open(), "the changed file comes first");
    w.rest();
    press(&mut w, "changed.overwrite");
    w.rest();
    assert!(!w.state().changed_prompt_open() && w.state().macros_prompt_open(), "then the macros");
    assert_eq!(std::fs::read(&own).unwrap(), CHANGED, "nothing is written yet");
    w.rest();
    press(&mut w, "macros.save_without");
    w.rest();
    assert!(!w.state().changed_prompt_open() && !w.state().macros_prompt_open());
    assert!(w.state().open_error().is_none(), "{:?}", w.state().open_error());
    assert!(!parts(&own).iter().any(|p| p.to_ascii_lowercase().contains("vba")), "the file was overwritten, without the project");
    assert!(!w.state().is_dirty());
}

/// A Cancel ends what was waiting on the save for good: a later save that works does not bring the New back.
#[test]
fn what_waited_on_a_cancelled_save_does_not_come_back_with_a_later_one() {
    let dir = TempDir::new("macros-later");
    let chooser = picker_answering(vec![dir.file("out.vsdx"), dir.file("kept.vstm")]);
    let (mut w, _) = window_choosing(&dir, "plan.vstm", &with_macros(TEMPLATE_WITH_MACROS), chooser);
    run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    key(&mut w, Key::N, crate::saving::ctrl());
    press(&mut w, "prompt.save");
    w.rest();
    assert!(w.state().macros_prompt_open());
    press(&mut w, "macros.cancel");
    // Save As to a type that holds the macros: it works, and nothing else happens after it.
    save_as(&mut w);
    assert!(!w.state().macros_prompt_open() && dir.file("kept.vstm").exists());
    assert_eq!(w.state().window_title(), "kept.vstm - DiaCraft", "the drawing was saved and not replaced by a new one");
}

/// A close the desktop asks for while the card is up is held back and not asked about on top of it.
#[test]
fn a_close_while_the_card_is_up_is_held_back_and_the_card_stays() {
    let dir = TempDir::new("macros-close-over");
    let (mut w, _) = window_on(&dir, "plan.vsdm", &with_macros(DRAWING_WITH_MACROS), dir.file("out.vsdx"));
    run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    save_as(&mut w);
    assert!(w.state().macros_prompt_open());
    w.input_mut().viewports.entry(ViewportId::ROOT).or_default().events.push(ViewportEvent::Close);
    w.step();
    assert!(commands(&w).iter().any(|c| matches!(c, ViewportCommand::CancelClose)), "the close was cancelled");
    assert!(!w.state().prompt_open(), "and the question about unsaved changes is not put over the card");
    w.rest();
    assert!(w.state().macros_prompt_open(), "the card is still the question");
}

/// The choice to leave the macros out outlives a file that changes while the card is up: the second question is the overwrite, and no third one comes.
#[test]
fn a_file_that_changes_under_the_card_is_asked_about_and_the_choice_about_the_macros_is_kept() {
    let dir = TempDir::new("macros-changed-under");
    let own = dir.file("plan.vsdx");
    let (mut w, _) = window_on(&dir, "plan.vsdx", &with_macros(DRAWING_WITH_MACROS), own.clone());
    save_as(&mut w);
    assert!(w.state().macros_prompt_open() && !w.state().changed_prompt_open(), "the file is as it was opened: the macros are asked about first");
    std::fs::write(&own, CHANGED).unwrap();
    press(&mut w, "macros.save_without");
    w.rest();
    assert!(w.state().changed_prompt_open() && !w.state().macros_prompt_open(), "the file changed meanwhile");
    assert_eq!(std::fs::read(&own).unwrap(), CHANGED);
    w.rest();
    press(&mut w, "changed.overwrite");
    w.rest();
    assert!(!w.state().changed_prompt_open() && !w.state().macros_prompt_open(), "not asked about the macros again");
    assert!(w.state().open_error().is_none(), "{:?}", w.state().open_error());
    assert!(!parts(&own).iter().any(|p| p.to_ascii_lowercase().contains("vba")));
}

// ---- the drawing under the card is replaced ----------------------------------------------------------------------------

/// A window with a control inbox on the macro-enabled file `name` of type `content_type`, the Save dialog answering `answers` one after the other.
fn controlled_on_macros(
    dir: &TempDir,
    (name, content_type): (&str, &str),
    answers: Vec<PathBuf>,
) -> (Window, std::sync::mpsc::Sender<diacraft_ui_egui::ControlRequest>, PathBuf) {
    let path = fixture::write(dir.path(), name, &with_macros(content_type));
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let (tx, rx) = channel();
    let app = DiacraftApp::new(session, Some(rx)).with_inline_pictures(bundled_drawer()).with_save_picker(picker_answering(answers));
    let mut w = window(app, [1280.0, 900.0]);
    w.rest();
    (w, tx, path)
}

/// What a program that replaces the drawing leaves: no card and nothing to press, and the new drawing is not written under words about the old one.
fn assert_no_macros_card(w: &mut Window, dir: &TempDir, what: &str) {
    w.rest();
    assert!(!w.state().macros_prompt_open() && !w.state().is_modal_open(), "{what}: the card is gone");
    assert!(w.state().element("macros.save_without").is_none() && w.state().element("macros.cancel").is_none(), "{what}: and so are its buttons");
    assert!(!dir.file("out.vsdx").exists(), "{what}: nothing was written");
}

/// A program that makes a new drawing under the card (`file.new` over the control channel) takes the card away: Save without would have written the empty
/// new drawing under words about a macro project it does not hold.
#[test]
fn a_new_drawing_made_by_a_program_closes_the_card() {
    let dir = TempDir::new("macros-replaced-new");
    let (mut w, tx, _) = controlled_on_macros(&dir, ("plan.vsdm", DRAWING_WITH_MACROS), vec![dir.file("out.vsdx")]);
    save_as(&mut w);
    assert!(w.state().macros_prompt_open());
    call(&mut w, &tx, "engine.execute", json!({"command": "file.new", "params": {}})).unwrap();
    assert_no_macros_card(&mut w, &dir, "file.new");
    assert!(!w.state().window_title().contains("plan.vsdm"), "{}", w.state().window_title());
}

/// So does a file opened by a program: the drawing the card was about is gone.
#[test]
fn a_file_opened_by_a_program_closes_the_card() {
    let dir = TempDir::new("macros-replaced-open");
    let other = fixture::write(dir.path(), "other.vsdx", &fixture::labelled_vsdx());
    let (mut w, tx, _) = controlled_on_macros(&dir, ("plan.vsdm", DRAWING_WITH_MACROS), vec![dir.file("out.vsdx")]);
    save_as(&mut w);
    assert!(w.state().macros_prompt_open());
    let answer = crate::support::send(&tx, "engine.execute", json!({"command": "file.open", "params": {"path": other.to_str().unwrap()}}));
    w.rest();
    answer.try_recv().expect("the file was read").unwrap();
    assert_no_macros_card(&mut w, &dir, "file.open");
    assert_eq!(w.state().window_title(), "other.vsdx - DiaCraft");
}

/// What waited on the save goes with the card: a later save that works does not bring back the New (or the close) the old question was asked for.
#[test]
fn what_waited_on_the_save_goes_when_the_drawing_is_replaced_under_the_card() {
    let dir = TempDir::new("macros-replaced-after");
    let (mut w, tx, _) = controlled_on_macros(&dir, ("plan.vstm", TEMPLATE_WITH_MACROS), vec![dir.file("out.vsdx"), dir.file("kept.vsdx")]);
    run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    key(&mut w, Key::N, crate::saving::ctrl());
    press(&mut w, "prompt.save");
    w.rest();
    assert!(w.state().macros_prompt_open(), "the save met the macros, and the New waits for it");
    call(&mut w, &tx, "engine.execute", json!({"command": "file.new", "params": {}})).unwrap();
    assert_no_macros_card(&mut w, &dir, "file.new");
    let title = w.state().window_title();
    // The new drawing is saved by the person, to a name the dialog gives: it is saved, and no other new drawing follows it.
    save_as(&mut w);
    assert!(dir.file("kept.vsdx").exists(), "the second answer of the dialog was used");
    assert_eq!(w.state().window_title(), "kept.vsdx - DiaCraft", "saved, and not replaced by a New that waited for the old save (it was {title})");
}

/// A save that fails after the choice is told in the banner, and the card is not asked a second time.
#[test]
fn a_save_that_fails_after_the_choice_is_told_in_the_banner_and_not_asked_again() {
    let dir = TempDir::new("macros-fails");
    let sub = dir.file("sub");
    std::fs::create_dir(&sub).unwrap();
    let out = sub.join("out.vsdx");
    let (mut w, path) = window_on(&dir, "plan.vsdm", &with_macros(DRAWING_WITH_MACROS), out.clone());
    save_as(&mut w);
    assert!(w.state().macros_prompt_open());
    std::fs::remove_dir(&sub).unwrap();
    press(&mut w, "macros.save_without");
    w.rest();
    assert!(!w.state().macros_prompt_open(), "not asked again");
    let words = w.state().open_error().expect("the refusal is shown in the banner").to_owned();
    assert!(words.contains("not saved"), "{words}");
    assert!(!out.exists() && path.exists());
    assert_eq!(w.state().window_title(), "plan.vsdm - DiaCraft");
}

/// A program's save is the engine's: the refusal is an error, and no card opens (a command never asks); with `drop_macros` it saves.
#[test]
fn a_program_that_asks_for_the_save_gets_the_error_and_no_card() {
    let dir = TempDir::new("macros-program");
    let path = fixture::write(dir.path(), "plan.vsdm", &with_macros(DRAWING_WITH_MACROS));
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let (tx, rx) = channel();
    let mut w = window(DiacraftApp::new(session, Some(rx)).with_inline_pictures(bundled_drawer()), [900.0, 600.0]);
    let out = dir.file("out.vsdx");
    let asked = json!({"command": "file.save_as", "params": {"path": out.to_str().unwrap()}});
    let refused = call(&mut w, &tx, "engine.execute", asked).unwrap_err();
    assert!(refused.contains("macro project"), "{refused}");
    assert!(!w.state().macros_prompt_open() && !out.exists());
    let leave = json!({"command": "file.save_as", "params": {"path": out.to_str().unwrap(), "drop_macros": true}});
    call(&mut w, &tx, "engine.execute", leave).unwrap();
    assert!(out.exists() && !w.state().macros_prompt_open());
}

/// The buttons stay on the screen and apart from each other at a narrow window and at a wide one, and the card keeps the middle.
#[test]
fn the_card_fits_a_narrow_window_and_a_wide_one() {
    let dir = TempDir::new("macros-fit");
    let (mut w, _) = window_on(&dir, "plan.vsdm", &with_macros(DRAWING_WITH_MACROS), dir.file("out.vsdx"));
    save_as(&mut w);
    assert!(w.state().macros_prompt_open());
    for size in [[300.0, 420.0], [480.0, 320.0], [480.0, 520.0], [1280.0, 900.0]] {
        w.set_size(egui::vec2(size[0], size[1]));
        w.rest();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
        let first = w.state().element("macros.save_without").unwrap().rect;
        let second = w.state().element("macros.cancel").unwrap().rect;
        for (name, rect) in [("macros.save_without", first), ("macros.cancel", second)] {
            assert!(screen.contains_rect(rect), "{name} {rect:?} is off the screen at {size:?}");
        }
        assert!(!first.intersects(second), "the buttons overlap at {size:?}: {first:?} {second:?}");
        let heading = w.get_by_label("Leave the macros out?").rect();
        assert!(screen.contains_rect(heading), "the heading is off the screen at {size:?}: {heading:?}");
    }
}

/// Writes a drawing with a macro project where the run of the real window can open it: `DIACRAFT_DEMO_DIR` names the folder. Not a test unless asked for.
#[test]
#[ignore = "writes plan.vsdm to the folder in DIACRAFT_DEMO_DIR, for opening it in the running window"]
fn write_the_drawing_with_macros() {
    let dir = PathBuf::from(std::env::var("DIACRAFT_DEMO_DIR").expect("DIACRAFT_DEMO_DIR names the folder to write to"));
    std::fs::create_dir_all(&dir).unwrap();
    let bytes = fixture::demo_builder(None)
        .document_type(DRAWING_WITH_MACROS)
        .with(Pkg::new().default_type("bin", "application/vnd.ms-office.vbaProject").part_without_override("/visio/vbaProject.bin", b"not code").rel(
            "/visio/document.xml",
            "rId50",
            VBA_REL,
            "vbaProject.bin",
        ))
        .build();
    fixture::write(&dir, "plan.vsdm", &bytes);
}

// ---- the choice to keep the macros: save as the macro-enabled type -------------------------------------------------------

/// The card has a second way out besides leaving the macros out: **save as the macro-enabled type** of the one that was chosen (`.vsdx` to `.vsdm`, `.vstx` to `.vstm`). Pressing it
/// runs `file.save_as` onto that name, with no `drop_macros`: the project is in the file, the macro-free name was never written, and the drawing is the new file.
#[test]
fn saving_as_the_macro_enabled_type_keeps_the_project_and_writes_no_macro_free_file() {
    for (name, content_type, chosen, twin) in
        [("plan.vsdm", DRAWING_WITH_MACROS, "out.vsdx", "out.vsdm"), ("plan.vstm", TEMPLATE_WITH_MACROS, "out.vstx", "out.vstm")]
    {
        let dir = TempDir::new("macros-switch");
        let bytes = with_macros(content_type);
        let (mut w, path) = window_on(&dir, name, &bytes, dir.file(chosen));
        run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
        save_as(&mut w);
        assert!(w.state().macros_prompt_open(), "{name}: the person is asked");
        let label = format!("Save as .{}", &twin[twin.len() - 4..]);
        w.get_by_label(&label);
        assert!(w.state().element("macros.save_as_enabled").is_some_and(|e| e.enabled), "{name}: the second way out is a button that can be pressed");
        w.get_by_label("Save without macros");
        assert!(!dir.file(twin).exists() && !dir.file(chosen).exists(), "{name}: nothing is written before the answer");
        press(&mut w, "macros.save_as_enabled");
        w.rest();
        assert!(!w.state().macros_prompt_open() && !w.state().is_modal_open(), "{name}");
        assert!(!dir.file(chosen).exists(), "{name}: the macro-free name is never written");
        assert!(dir.file(twin).exists(), "{name}: the macro-enabled one is");
        assert!(
            parts(&dir.file(twin)).iter().any(|p| p.to_ascii_lowercase().contains("vba")),
            "{name}: the project is in it: {:?}",
            parts(&dir.file(twin))
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "{name}: the file the drawing came from is untouched");
        assert_eq!(w.state().window_title(), format!("{twin} - DiaCraft"), "{name}: the drawing is the file written");
        assert!(!w.state().is_dirty() && w.state().open_error().is_none(), "{name}: {:?}", w.state().open_error());
        assert!(w.state().notice().is_some_and(|n| n.starts_with(&format!("Saved {twin}."))), "{name}: {:?}", w.state().notice());
        assert!(w.state().save_note().is_none(), "{name}: a VSDX file saved as VSDX carries nothing across, so there is no note");
    }
}

/// The other way out still leaves the macros out, and Cancel still cancels, with the second button there.
#[test]
fn the_two_other_answers_are_as_they_were_with_the_new_button_beside_them() {
    let dir = TempDir::new("macros-three");
    let bytes = with_macros(DRAWING_WITH_MACROS);
    let (mut w, path) = window_on(&dir, "plan.vsdm", &bytes, dir.file("out.vsdx"));
    save_as(&mut w);
    let names: Vec<String> = ["macros.save_without", "macros.save_as_enabled", "macros.cancel"]
        .iter()
        .map(|n| {
            assert!(w.state().element(n).is_some_and(|e| e.enabled), "{n} is drawn and can be pressed");
            (*n).to_owned()
        })
        .collect();
    // Left to right: leave out, keep, cancel.
    let x = |n: &str| w.state().element(n).unwrap().rect.center().x;
    assert!(x(&names[0]) < x(&names[1]) && x(&names[1]) < x(&names[2]), "{names:?}");
    press(&mut w, "macros.save_without");
    w.rest();
    assert!(dir.file("out.vsdx").exists() && !dir.file("out.vsdm").exists());
    assert!(!parts(&dir.file("out.vsdx")).iter().any(|p| p.to_ascii_lowercase().contains("vba")));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

/// The macro-enabled name may be a file already: it is asked about before it is replaced, as any file the window names is, in words that say why the name is that one; Replace writes it
/// and Cancel leaves it, the drawing and its path as they were.
#[test]
fn a_macro_enabled_name_that_is_there_is_asked_about_before_it_is_replaced() {
    for replace in [true, false] {
        let dir = TempDir::new("macros-switch-there");
        let bytes = with_macros(DRAWING_WITH_MACROS);
        let (mut w, path) = window_on(&dir, "plan.vsdm", &bytes, dir.file("out.vsdx"));
        std::fs::write(dir.file("out.vsdm"), b"somebody else's file").unwrap();
        run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
        save_as(&mut w);
        press(&mut w, "macros.save_as_enabled");
        w.rest();
        assert!(w.state().replace_prompt_open() && !w.state().macros_prompt_open(), "replace: {replace}: the file that is there is asked about");
        assert_eq!(std::fs::read(dir.file("out.vsdm")).unwrap(), b"somebody else's file", "nothing is written before the answer");
        w.get_by_label("out.vsdm already exists. Do you want to replace it?");
        assert!(w.query_by_label_contains("added .").is_none(), "the words are not those of a name the window gave a type to");
        press(&mut w, if replace { "replace.replace" } else { "replace.cancel" });
        w.rest();
        assert!(!w.state().replace_prompt_open() && !w.state().is_modal_open());
        if replace {
            assert!(
                parts(&dir.file("out.vsdm")).iter().any(|p| p.to_ascii_lowercase().contains("vba")),
                "the project is in the file that replaced the other"
            );
            assert_eq!(w.state().window_title(), "out.vsdm - DiaCraft");
        } else {
            assert_eq!(std::fs::read(dir.file("out.vsdm")).unwrap(), b"somebody else's file", "Cancel leaves it");
            assert_eq!(w.state().window_title(), "*plan.vsdm - DiaCraft", "still the drawing, still unsaved");
            assert_eq!(w.state().session().path(), Some(path.as_path()));
        }
        assert!(!dir.file("out.vsdx").exists());
    }
}

/// A close that saves goes through the choice too: keeping the macros saves the drawing as the macro-enabled type, and then the window closes.
#[test]
fn a_close_that_saves_goes_on_after_the_macros_are_kept() {
    let dir = TempDir::new("macros-close-keep");
    let (mut w, _) = window_on(&dir, "plan.vstm", &with_macros(TEMPLATE_WITH_MACROS), dir.file("out.vstx"));
    run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    w.input_mut().viewports.entry(ViewportId::ROOT).or_default().events.push(ViewportEvent::Close);
    w.step();
    w.rest();
    press(&mut w, "prompt.save");
    w.rest();
    assert!(w.state().macros_prompt_open());
    assert!(!commands(&w).iter().any(|c| matches!(c, ViewportCommand::Close)), "not closed while the question is up");
    let at = w.state().element("macros.save_as_enabled").unwrap().rect.center();
    click(&mut w, at);
    assert!(commands(&w).iter().any(|c| matches!(c, ViewportCommand::Close)), "closed once the file is saved");
    assert!(dir.file("out.vstm").exists() && !dir.file("out.vstx").exists());
}
