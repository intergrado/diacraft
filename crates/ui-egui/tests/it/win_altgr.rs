//! What egui is given on Windows when a person types a character with AltGr, and that the window types it. The sequences are the ones the pinned winit and egui-winit
//! produce, read from their code (not pressed on a Windows machine):
//!
//! * **The AltGr key** (the right Alt key of a layout that has an AltGr character on it). Windows sends a Ctrl key down that the keyboard makes up (`LCtrl`), and the right Alt key down. winit's
//!   Windows backend tells `ModifiersChanged` before each key (`event_loop.rs`, `update_modifiers`), so the made-up Ctrl is first told as held (the right Alt key is not down yet), its own key
//!   event is dropped (`keyboard.rs`, `is_current_fake`), and the right Alt key's press then takes **both Ctrl and Alt out of the modifiers while AltGr is down** (`keyboard_layout.rs`,
//!   `get_agnostic_mods`: `filter_out_altgr`). The character's key then arrives with **no modifier at all**, and its text is the character (`keyboard.rs`: `ctrl_on` is off for a layout with
//!   AltGr while Alt is down). egui-winit turns that into a press of the key's own letter (the Windows virtual-key letter) and an `Event::Text`.
//! * **Ctrl and the left Alt key** types the same character on a layout with AltGr (Windows reads Ctrl+Alt as AltGr when it turns a key into a character), and winit reports it with Ctrl and Alt
//!   held. The egui-winit of the pin keeps its text on Windows (`typed_text_event`, the one-bit fork's; upstream's `is_cmd` drops the text of every press with Ctrl held, and
//!   the pin did until then). The key press and the text come in one frame, and Ctrl and Alt held in the frames before. These tests feed egui the events that conversion gives: winit's key
//!   event cannot be built outside winit, so the rule itself is held by the fork's unit test `typed_text_is_text_and_a_chord_is_not`.
//!
//! The window must type the character in both, whatever frame the modifier events fall in, and run no command for it.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_testkit::build::ShapeBuilder;
use egui::os::OperatingSystem;
use egui::{Event, Key, Modifiers};
use serde_json::{Value, json};

use crate::editing::{edit_window, edit_window_with, key, shape_ids, three_shapes, type_text};
use crate::sheet_window::{one_page, open_shape, sheet, window_on};
use crate::support::{Rest, Window, click, press, run_command};

const WIDE: [f32; 2] = [1280.0, 900.0];

const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };
const CTRL_ALT: Modifiers = Modifiers { alt: true, ..CTRL };

/// The two ways a person types a character with AltGr on Windows.
#[derive(Clone, Copy, Debug)]
enum Typed {
    /// The AltGr key: Ctrl told as held for the made-up key, then no modifier, with the key's press and the text.
    TheAltGrKey,
    /// Ctrl and the left Alt key.
    CtrlAndLeftAlt,
}

const BOTH: [Typed; 2] = [Typed::TheAltGrKey, Typed::CtrlAndLeftAlt];

fn key_event(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers }
}

/// The events of the press: those about the modifier keys (each is told before its key), and then the character's key with its text, which egui-winit pushes in one call.
fn press_events(how: Typed, character: Key, text: &str, shift: bool) -> (Vec<Event>, Vec<Event>) {
    let held = Modifiers { shift, ..Modifiers::NONE };
    let with_shift = |m: Modifiers| Modifiers { shift, ..m };
    match how {
        Typed::TheAltGrKey => (
            vec![
                // The made-up Ctrl key: told as held (the right Alt key is not down yet), and no key event of its own.
                Event::ModifiersChanged(with_shift(CTRL)),
                // The right Alt key: Ctrl and Alt are both taken out while AltGr is down. egui-winit gives no egui key for AltGraph, so the physical key stands in.
                Event::ModifiersChanged(held),
                key_event(Key::AltRight, true, held),
            ],
            vec![key_event(character, true, held), Event::Text(text.to_owned())],
        ),
        Typed::CtrlAndLeftAlt => (
            vec![
                Event::ModifiersChanged(with_shift(CTRL)),
                key_event(Key::ControlLeft, true, with_shift(CTRL)),
                Event::ModifiersChanged(with_shift(CTRL_ALT)),
                key_event(Key::AltLeft, true, with_shift(CTRL_ALT)),
            ],
            vec![key_event(character, true, with_shift(CTRL_ALT)), Event::Text(text.to_owned())],
        ),
    }
}

/// The events of the release, in the order the keys come up (the character's, then the modifiers).
fn release_events(how: Typed, character: Key, shift: bool) -> Vec<Event> {
    let held = Modifiers { shift, ..Modifiers::NONE };
    let with_shift = |m: Modifiers| Modifiers { shift, ..m };
    match how {
        Typed::TheAltGrKey => vec![key_event(character, false, held), key_event(Key::AltRight, false, held)],
        Typed::CtrlAndLeftAlt => vec![
            key_event(character, false, with_shift(CTRL_ALT)),
            key_event(Key::AltLeft, false, with_shift(CTRL_ALT)),
            Event::ModifiersChanged(with_shift(CTRL)),
            key_event(Key::ControlLeft, false, with_shift(CTRL)),
            Event::ModifiersChanged(held),
        ],
    }
}

/// A person types `text` on the key `character` of a Windows layout with AltGr. `together`: every event reaches egui in one frame (a Windows event loop that has all the key messages queued);
/// else each modifier event has a frame of its own and the character's key and text come in the next one (a frame drawn between two key messages).
fn type_with_altgr(w: &mut Window, how: Typed, character: Key, text: &str, shift: bool, together: bool) {
    w.ctx.set_os(OperatingSystem::Windows);
    let (modifiers, typed) = press_events(how, character, text, shift);
    if together {
        w.input_mut().events.extend(modifiers);
        w.input_mut().events.extend(typed);
        w.step();
    } else {
        for event in modifiers {
            w.input_mut().events.push(event);
            w.step();
        }
        w.input_mut().events.extend(typed);
        w.step();
    }
    for event in release_events(how, character, shift) {
        w.input_mut().events.push(event);
        w.step();
    }
    w.rest();
}

fn state(w: &Window) -> Value {
    w.state().window_state()
}

fn editing(w: &Window) -> bool {
    state(w)["editingText"].as_bool().unwrap()
}

fn click_on(w: &mut Window, name: &str) {
    let to = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    click(w, to);
    w.rest();
}

/// Characters on keys of a German, a Polish, a Czech and a Portuguese layout: the key and what AltGr types on it.
const CHARACTERS: [(Key, &str); 8] =
    [(Key::E, "€"), (Key::Q, "@"), (Key::L, "ł"), (Key::S, "ś"), (Key::A, "ą"), (Key::Num7, "{"), (Key::Num8, "["), (Key::Num0, "}")];

/// A selected shape and a character typed with AltGr: it starts the shape's text, and no Ctrl+key command of the key runs (Link for K and L, Save for S, Select All for A, Undo for Z, Bold for B).
#[test]
fn a_character_typed_with_altgr_on_windows_starts_the_text_of_the_selected_shape_and_runs_nothing() {
    for how in BOTH {
        for together in [true, false] {
            for (character, text) in CHARACTERS {
                let mut w = edit_window(WIDE);
                let ids = three_shapes(&mut w);
                let revision = w.state().session().revision();
                type_with_altgr(&mut w, how, character, text, false, together);
                let why = format!("{how:?}, AltGr+{character:?} typing {text} (events together: {together})");
                assert!(editing(&w), "{why}: the character starts the shape's text");
                assert_eq!(w.state().edited_text(), Some(text), "{why}");
                assert_eq!(w.state().session().revision(), revision, "{why}: no command ran");
                assert_eq!(shape_ids(&w), ids, "{why}");
                assert!(state(&w)["hyperlinks"].is_null() && !w.state().save_dialog_open(), "{why}: no dialog opened");
                assert_eq!(w.state().selection(), &ids[2..], "{why}: the selection is the shape's");
                assert_eq!(w.state().ui_panics(), 0, "{why}");
            }
        }
    }
}

/// AltGr and Shift type the capital, and the selected shape's text takes it.
#[test]
fn altgr_with_shift_on_windows_types_the_capital() {
    for how in BOTH {
        let mut w = edit_window(WIDE);
        three_shapes(&mut w);
        type_with_altgr(&mut w, how, Key::L, "Ł", true, true);
        assert_eq!(w.state().edited_text(), Some("Ł"), "{how:?}");
    }
}

/// With the text box open: the character goes in where the caret is, one after another, and nothing else acts on the press (egui's own Ctrl+K cuts the rest of the line).
#[test]
fn altgr_on_windows_types_into_the_open_text_box_one_character_after_another() {
    for how in BOTH {
        for together in [true, false] {
            let mut w = edit_window(WIDE);
            let ids = three_shapes(&mut w);
            run_command(&mut w, "shape.set_text", json!({"id": ids[2], "text": "one\ntwo"}));
            key(&mut w, Key::F2, Modifiers::NONE);
            assert!(editing(&w));
            key(&mut w, Key::Home, CTRL);
            key(&mut w, Key::ArrowRight, Modifiers::NONE);
            key(&mut w, Key::ArrowRight, Modifiers::NONE);
            let revision = w.state().session().revision();
            type_with_altgr(&mut w, how, Key::L, "ł", false, together);
            type_with_altgr(&mut w, how, Key::E, "€", false, together);
            type_with_altgr(&mut w, how, Key::Q, "@", false, together);
            let why = format!("{how:?} (events together: {together})");
            assert_eq!(w.state().edited_text(), Some("onł€@e\ntwo"), "{why}: each went in where the caret was, and the line stayed");
            assert_eq!(w.state().session().revision(), revision, "{why}: nothing was sent");
            assert!(editing(&w), "{why}: the box is still open");
            assert_eq!(w.state().ui_panics(), 0, "{why}");
        }
    }
}

/// What egui-winit gives for Ctrl and the left Alt key on a key that the layout types nothing on (a layout with no AltGr character there, or none at all): winit gives no text for a Ctrl+Alt
/// chord that types nothing, so the frame has the key press with Ctrl and Alt and **no text**, and nothing is typed and no Ctrl+key command runs. The control for the tests above, which have the
/// text. Before the pin held the fork's E22 this was the frame of every Ctrl+Alt press, the AltGr characters too (gaps P4, fixed): that frame is now only a chord's.
#[test]
fn ctrl_and_left_alt_with_no_text_types_nothing_and_runs_nothing() {
    for (character, _) in CHARACTERS {
        let mut w = edit_window(WIDE);
        let ids = three_shapes(&mut w);
        let revision = w.state().session().revision();
        w.ctx.set_os(OperatingSystem::Windows);
        let (modifiers, typed) = press_events(Typed::CtrlAndLeftAlt, character, "€", false);
        w.input_mut().events.extend(modifiers);
        // The key press, as egui-winit pushes it for a chord that typed nothing, and no `Event::Text` after it.
        w.input_mut().events.extend(typed.into_iter().filter(|e| !matches!(e, Event::Text(_))));
        w.step();
        for event in release_events(Typed::CtrlAndLeftAlt, character, false) {
            w.input_mut().events.push(event);
            w.step();
        }
        w.rest();
        let why = format!("Ctrl and the left Alt key on {character:?}, no text");
        assert!(!editing(&w), "{why}: nothing is typed");
        assert_eq!(w.state().session().revision(), revision, "{why}: and no command ran");
        assert!(state(&w)["hyperlinks"].is_null() && !w.state().save_dialog_open(), "{why}: no dialog opened");
        assert_eq!(w.state().selection(), &ids[2..], "{why}");
        assert_eq!(w.state().ui_panics(), 0, "{why}");
    }
}

/// A field of the Shape Data pane: Polish ś is AltGr+S (Ctrl+S saves) and ł is AltGr+L: they type, and save nothing.
#[test]
fn altgr_on_windows_types_into_a_shape_data_field_and_saves_nothing() {
    for how in BOTH {
        let asked = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&asked);
        let picker: diacraft_ui_egui::SavePicker = Arc::new(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            None
        });
        let mut w = edit_window_with(WIDE, |app| app.with_save_picker(picker));
        let shapes = three_shapes(&mut w);
        let id = shapes[2];
        run_command(
            &mut w,
            "shapedata.define",
            json!({"id": id, "fields": [{"label": "Owner", "name": "Owner", "type": "string"}, {"label": "Qty", "name": "Qty", "type": "string"}]}),
        );
        run_command(&mut w, "window.pane", json!({"pane": "shape_data", "shown": true}));
        click_on(&mut w, "pane.data.Owner");
        key(&mut w, Key::A, CTRL);
        type_text(&mut w, "Ann");
        type_with_altgr(&mut w, how, Key::S, "ś", false, true);
        type_with_altgr(&mut w, how, Key::L, "ł", false, false);
        key(&mut w, Key::Enter, Modifiers::NONE);
        let owner = w.state_mut().execute("shapedata.list", &json!({"id": id})).unwrap()["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["row"] == "Owner")
            .map(|f| f["value"].as_str().unwrap().to_owned());
        assert_eq!(owner.as_deref(), Some("Annśł"), "{how:?}: the characters were typed into the field");
        w.rest();
        assert_eq!(asked.load(Ordering::SeqCst), 0, "{how:?}: AltGr+S did not Save");
        assert!(!w.state().save_dialog_open(), "{how:?}");
        assert!(state(&w)["hyperlinks"].is_null(), "{how:?}: AltGr+L did not open Link");
        assert_eq!(w.state().ui_panics(), 0, "{how:?}");
    }
}

/// A field of a dialog (the Link dialog's address) and the ShapeSheet's cell editor take the character too.
#[test]
fn altgr_on_windows_types_into_a_dialog_field_and_the_shapesheet() {
    for how in BOTH {
        let mut w = edit_window(WIDE);
        three_shapes(&mut w);
        run_command(&mut w, "window.hyperlinks", json!({}));
        press(&mut w, "links.address");
        type_text(&mut w, "abc");
        key(&mut w, Key::Home, Modifiers::NONE);
        type_with_altgr(&mut w, how, Key::L, "ł", false, true);
        type_with_altgr(&mut w, how, Key::E, "€", false, false);
        assert_eq!(state(&w)["hyperlinks"]["rows"][0]["address"], json!("ł€abc"), "{how:?}: {}", state(&w)["hyperlinks"]);
        assert!(state(&w)["hyperlinks"].is_object(), "{how:?}: the dialog is still open");

        let shape = ShapeBuilder::new(10).cell_u("Width", "2", "IN").cell_u("Height", "1", "IN").cell("PinX", "4").cell("PinY", "4");
        let mut w = window_on(one_page(shape), [1400.0, 900.0]);
        open_shape(&mut w, 10);
        run_command(&mut w, "window.sheetSelect", json!({"cell": "Width"}));
        let steps = w.state().session().history().map_or(0, |h| h.undo_len());
        type_with_altgr(&mut w, how, Key::E, "€", false, true);
        assert_eq!(sheet(&w)["edit"]["text"], json!("€"), "{how:?}: the character began an edit of the cell: {}", sheet(&w)["edit"]);
        key(&mut w, Key::Home, Modifiers::NONE);
        type_with_altgr(&mut w, how, Key::L, "ł", false, false);
        assert_eq!(sheet(&w)["edit"]["text"], json!("ł€"), "{how:?}: and the formula bar took the next one without cutting the first");
        key(&mut w, Key::Escape, Modifiers::NONE);
        assert_eq!(w.state().session().history().map_or(0, |h| h.undo_len()), steps, "{how:?}: nothing was written");
    }
}

/// What Windows gives for Ctrl and the left Alt key when the layout types nothing there (a chord, no text) still runs a key of the window: Ctrl+Alt+P is Reorder Pages. The window has no other
/// check than the text: a key press with Ctrl and Alt that comes with no text is a command's.
#[test]
fn ctrl_and_left_alt_on_windows_with_no_character_typed_still_runs_the_window_key() {
    let mut w = edit_window(WIDE);
    run_command(&mut w, "page.add", json!({}));
    run_command(&mut w, "page.select", json!({"id": 1}));
    three_shapes(&mut w);
    w.ctx.set_os(OperatingSystem::Windows);
    let reordering = |w: &Window| state(w)["reorderPages"].as_bool().unwrap();
    let (modifiers, _) = press_events(Typed::CtrlAndLeftAlt, Key::P, "", false);
    w.input_mut().events.extend(modifiers);
    w.input_mut().events.push(key_event(Key::P, true, CTRL_ALT));
    w.step();
    for event in release_events(Typed::CtrlAndLeftAlt, Key::P, false) {
        w.input_mut().events.push(event);
        w.step();
    }
    w.rest();
    assert!(reordering(&w), "Ctrl+Alt+P with no text is Reorder Pages");
    assert!(!editing(&w), "and typed nothing");
}
