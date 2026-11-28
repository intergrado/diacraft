//! A window shortcut runs for the press that names it and for no other. egui's own match lets an extra Alt or Shift stand, and AltGr can arrive as Ctrl and Alt held
//! together (a browser on Windows sends it so; a Windows desktop sends Ctrl with the left Alt key so, and the AltGr key with no modifier, `win_altgr.rs`), so Ctrl+Alt+Shift+K was Small Caps through Ctrl+Shift+K, Ctrl+Alt+K was Link through Ctrl+K, and a Polish, Czech or German
//! person who typed a letter with AltGr (ś, ł, €, @) on a selected shape or in a field ran the Ctrl+key command too, or had the text box's own Ctrl+K cut the rest of the line.
//! These tests give egui the press as a person's keyboard does (the modifiers held, the key, and the character the key typed in the same frame) and look at the document, the
//! dialogs and the text that came out.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_engine::formatting::{Case, ShapeLook, look_of};
use diacraft_testkit::build::ShapeBuilder;
use diacraft_ui_egui::shortcuts::{KeyTaker, bound_keys, parse, press_matches, types_text};
use egui::os::OperatingSystem;
use egui::{Event, Key, KeyboardShortcut, Modifiers};
use serde_json::{Value, json};

use crate::editing::{edit_window, edit_window_with, hold, key, shape_ids, three_shapes, type_text};
use crate::saving::ctrl;
use crate::sheet_window::{one_page, open_shape, sheet, window_on};
use crate::support::{Rest, Window, click, press, run_command};

const WIDE: [f32; 2] = [1280.0, 900.0];

/// What AltGr is to egui from a browser on Windows (and what Ctrl with the left Alt key is on a Windows desktop): Ctrl (which is also the command key there) and Alt.
const CTRL_ALT: Modifiers = Modifiers { alt: true, ctrl: true, shift: false, mac_cmd: false, command: true };
const CTRL_ALT_SHIFT: Modifiers = Modifiers { shift: true, ..CTRL_ALT };
const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: true, shift: true, mac_cmd: false, command: true };
/// Command and Option on a Mac.
const CMD_OPTION: Modifiers = Modifiers { alt: true, ctrl: false, shift: false, mac_cmd: true, command: true };
const COMMAND_MAC: Modifiers = Modifiers { alt: false, ctrl: false, shift: false, mac_cmd: true, command: true };

fn event(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
}

/// An AltGr press: the keys held, then the key and the character it typed in one frame (`text_first`: the character before the key, as a browser sends them), then the release.
fn altgr_with(w: &mut Window, key: Key, text: &str, modifiers: Modifiers, text_first: bool) {
    hold(w, modifiers);
    if text_first {
        w.input_mut().events.push(Event::Text(text.to_owned()));
        w.input_mut().events.push(event(key, true, modifiers));
    } else {
        w.input_mut().events.push(event(key, true, modifiers));
        w.input_mut().events.push(Event::Text(text.to_owned()));
    }
    w.step();
    w.input_mut().events.push(event(key, false, modifiers));
    w.step();
    hold(w, Modifiers::NONE);
    w.rest();
}

fn altgr(w: &mut Window, key: Key, text: &str) {
    altgr_with(w, key, text, CTRL_ALT, false);
}

fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

fn look(w: &Window, id: u32) -> ShapeLook {
    look_of(w.state().session().document().unwrap(), page_id(w), &[id]).unwrap().remove(0)
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

/// A window with the three shapes of the editing tests, the ellipse selected and holding two lines of text.
fn lines() -> (Window, u32) {
    let mut w = edit_window(WIDE);
    let ids = three_shapes(&mut w);
    run_command(&mut w, "shape.set_text", json!({"id": ids[2], "text": "one\ntwo"}));
    (w, ids[2])
}

// ---- the press names its shortcut and no other ---------------------------------------------------------------------------------------------

/// Ctrl+Alt+Shift+K was Small Caps: egui let the extra Alt stand. Ctrl+Shift+K still is.
#[test]
fn ctrl_alt_shift_k_is_no_ctrl_shift_k_and_ctrl_shift_k_still_toggles_small_caps() {
    let (mut w, id) = lines();
    let revision = w.state().session().revision();
    key(&mut w, Key::K, CTRL_ALT_SHIFT);
    assert_eq!(look(&w, id).case, Case::Normal, "Ctrl+Alt+Shift+K is not Ctrl+Shift+K");
    assert_eq!(w.state().session().revision(), revision, "and it changed nothing");
    key(&mut w, Key::K, CTRL_SHIFT);
    assert_eq!(look(&w, id).case, Case::SmallCaps, "Ctrl+Shift+K is Small Caps");
    key(&mut w, Key::K, CTRL_SHIFT);
    assert_eq!(look(&w, id).case, Case::Normal, "and on and off");
    assert_eq!(w.state().ui_panics(), 0);
}

/// Ctrl+Alt+K ran Ctrl+K, Link. Ctrl+K still opens it.
#[test]
fn ctrl_alt_k_is_no_ctrl_k_and_ctrl_k_still_links() {
    let mut w = edit_window(WIDE);
    three_shapes(&mut w);
    key(&mut w, Key::K, CTRL_ALT);
    assert!(state(&w)["hyperlinks"].is_null(), "Ctrl+Alt+K is not Ctrl+K");
    key(&mut w, Key::K, CTRL_ALT_SHIFT);
    assert!(state(&w)["hyperlinks"].is_null(), "nor is Ctrl+Alt+Shift+K");
    key(&mut w, Key::K, ctrl());
    assert!(state(&w)["hyperlinks"].is_object(), "Ctrl+K opens the Link dialog");
}

/// A chord with an extra Alt, and Delete with Alt or Shift added, run no command: Rotate, Flip, Select All, Undo, Bold and Delete each run for their own press only.
#[test]
fn a_chord_with_an_extra_alt_or_shift_runs_no_command_and_the_chord_alone_does() {
    let mut w = edit_window(WIDE);
    let ids = three_shapes(&mut w);
    let revision = w.state().session().revision();
    for (k, with) in [
        (Key::L, CTRL_ALT),
        (Key::R, CTRL_ALT),
        (Key::J, CTRL_ALT),
        (Key::A, CTRL_ALT),
        (Key::Z, CTRL_ALT),
        (Key::B, CTRL_ALT),
        (Key::Delete, Modifiers::ALT),
        (Key::Delete, Modifiers::SHIFT),
    ] {
        key(&mut w, k, with);
        assert_eq!(w.state().session().revision(), revision, "{k:?} with {with:?} changed the drawing");
        assert_eq!(shape_ids(&w), ids, "{k:?} with {with:?} deleted or undid something");
        assert_eq!(w.state().selection(), &ids[2..], "{k:?} with {with:?} changed the selection");
    }
    // The chords alone still run.
    key(&mut w, Key::A, ctrl());
    assert_eq!(w.state().selection().len(), 3, "Ctrl+A selects all");
    key(&mut w, Key::Delete, Modifiers::NONE);
    assert!(shape_ids(&w).is_empty(), "Delete deletes");
    key(&mut w, Key::Z, ctrl());
    assert_eq!(shape_ids(&w).len(), 3, "Ctrl+Z undoes");
}

/// Ctrl+= zooms in, and on a keyboard that types `=` with Shift the press arrives with Shift held: that is still Zoom In (the one extra Shift the window allows, on a key that is a
/// character and not a letter), while Alt added is not.
#[test]
fn an_extra_shift_on_a_symbol_key_is_the_symbol_and_an_extra_alt_is_not() {
    let mut w = edit_window(WIDE);
    let zoom = |w: &Window| w.state().zoom();
    let start = zoom(&w);
    key(&mut w, Key::Equals, CTRL_ALT);
    assert_eq!(zoom(&w), start, "Ctrl+Alt+= is not Ctrl+=");
    key(&mut w, Key::Equals, CTRL_SHIFT);
    assert_ne!(zoom(&w), start, "Ctrl+Shift+= is Zoom In on a keyboard where `=` is Shift and 0");
    let zoomed = zoom(&w);
    key(&mut w, Key::Minus, CTRL_ALT);
    assert_eq!(zoom(&w), zoomed, "Ctrl+Alt+- is not Ctrl+-");
    key(&mut w, Key::Minus, ctrl());
    assert_ne!(zoom(&w), zoomed, "Ctrl+- is Zoom Out");
}

// ---- a press that typed a character is the text's ------------------------------------------------------------------------------------------

/// A selected shape and an AltGr press that typed a character (€ on many layouts, @ on German, ł on Czech, Hungarian and Slovak, ś, ą and ż on Polish): the character starts the
/// shape's text, in either order the events come, and **no Ctrl+key command runs** (Link, Rotate, Select All, Undo, Bold and the rest).
#[test]
fn altgr_on_the_canvas_types_the_character_and_runs_nothing() {
    for text_first in [false, true] {
        for (k, typed) in [
            (Key::E, "€"),
            (Key::Q, "@"),
            (Key::K, "ł"),
            (Key::L, "ł"),
            (Key::A, "ą"),
            (Key::Z, "ż"),
            (Key::S, "ś"),
            (Key::N, "ń"),
            (Key::O, "ó"),
            (Key::B, "{"),
            (Key::R, "®"),
        ] {
            let mut w = edit_window(WIDE);
            let ids = three_shapes(&mut w);
            let revision = w.state().session().revision();
            altgr_with(&mut w, k, typed, CTRL_ALT, text_first);
            let why = format!("AltGr+{k:?} typing {typed} (text first: {text_first})");
            assert!(editing(&w), "{why}: the character starts the shape's text");
            assert_eq!(w.state().edited_text(), Some(typed), "{why}");
            assert_eq!(w.state().session().revision(), revision, "{why}: no command ran");
            assert_eq!(shape_ids(&w), ids, "{why}");
            assert!(state(&w)["hyperlinks"].is_null() && !w.state().save_dialog_open(), "{why}: no dialog opened");
            assert_eq!(w.state().selection(), &ids[2..], "{why}: the selection is the shape's");
            assert_eq!(w.state().ui_panics(), 0, "{why}");
        }
    }
}

/// A key of the window that has Ctrl and Alt in it still runs for its own press: Ctrl+Alt+P opens Reorder Pages. The same keys as an AltGr press that typed a character do not.
#[test]
fn a_ctrl_alt_key_of_the_window_runs_for_its_press_and_not_for_a_press_that_typed_a_character() {
    let mut w = edit_window(WIDE);
    run_command(&mut w, "page.add", json!({}));
    run_command(&mut w, "page.select", json!({"id": 1}));
    three_shapes(&mut w);
    let reordering = |w: &Window| state(w)["reorderPages"].as_bool().unwrap();
    altgr(&mut w, Key::P, "¶");
    assert!(!reordering(&w), "AltGr+P typed a character: no Reorder Pages");
    assert_eq!(w.state().edited_text(), Some("¶"), "it started the shape's text");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(!editing(&w));
    key(&mut w, Key::P, CTRL_ALT);
    assert!(reordering(&w), "Ctrl+Alt+P with no character typed is Reorder Pages");
}

/// AltGr+Shift types the capital (Ł): it is no Ctrl+Shift+K either.
#[test]
fn altgr_with_shift_types_the_capital_and_runs_nothing() {
    let (mut w, id) = lines();
    altgr_with(&mut w, Key::K, "Ł", CTRL_ALT_SHIFT, false);
    assert_eq!(w.state().edited_text(), Some("Ł"));
    assert_eq!(look(&w, id).case, Case::Normal, "Ctrl+Alt+Shift+K is not Small Caps");
}

/// With the shape's text box open, the AltGr character goes into the text where the caret is: nothing else of the press acts on it (egui's own Ctrl+K cuts the rest of the line, Ctrl+U
/// the line before the caret, Ctrl+H a character and Ctrl+W a word, and the window's Ctrl+B and Ctrl+I format the characters selected).
#[test]
fn altgr_in_the_shape_text_box_types_the_character_and_runs_nothing() {
    for (k, typed) in [(Key::K, "ł"), (Key::E, "€"), (Key::B, "{"), (Key::U, "€"), (Key::I, "í"), (Key::H, "ħ"), (Key::W, "|")] {
        let (mut w, id) = lines();
        key(&mut w, Key::F2, Modifiers::NONE);
        assert!(editing(&w));
        // The caret between `on` and `e` of the first line.
        key(&mut w, Key::Home, ctrl());
        key(&mut w, Key::ArrowRight, Modifiers::NONE);
        key(&mut w, Key::ArrowRight, Modifiers::NONE);
        let revision = w.state().session().revision();
        altgr(&mut w, k, typed);
        assert_eq!(w.state().edited_text(), Some(format!("on{typed}e\ntwo").as_str()), "AltGr+{k:?}: the character goes in and the text stays");
        assert_eq!(w.state().session().revision(), revision, "AltGr+{k:?}: nothing was sent");
        assert!(editing(&w), "AltGr+{k:?}: the box is still open");
        key(&mut w, Key::Escape, Modifiers::NONE);
        assert_eq!(look(&w, id).case, Case::Normal);
        assert_eq!(w.state().ui_panics(), 0);
    }
}

/// Ctrl+Alt+Enter is not Ctrl+Enter, which ends the text box.
#[test]
fn ctrl_alt_enter_does_not_end_the_text_box_and_ctrl_enter_does() {
    let (mut w, _) = lines();
    key(&mut w, Key::F2, Modifiers::NONE);
    assert!(editing(&w));
    key(&mut w, Key::Enter, CTRL_ALT);
    assert!(editing(&w), "Ctrl+Alt+Enter leaves the box open");
    key(&mut w, Key::Enter, ctrl());
    assert!(!editing(&w), "Ctrl+Enter ends it");
}

/// A field of the Shape Data pane: the Polish ś is AltGr+S, which was Ctrl+S, a shortcut of the files that runs while a field has the keyboard. It types, and saves nothing.
#[test]
fn altgr_in_a_shape_data_field_types_the_character_and_saves_nothing() {
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
    key(&mut w, Key::A, ctrl());
    type_text(&mut w, "Ann");
    altgr(&mut w, Key::S, "ś");
    altgr(&mut w, Key::K, "ł");
    key(&mut w, Key::Enter, Modifiers::NONE);
    let owner = w.state_mut().execute("shapedata.list", &json!({"id": id})).unwrap()["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["row"] == "Owner")
        .map(|f| f["value"].as_str().unwrap().to_owned());
    assert_eq!(owner.as_deref(), Some("Annśł"), "the characters were typed into the field");
    w.rest();
    assert_eq!(asked.load(Ordering::SeqCst), 0, "AltGr+S did not Save");
    assert!(!w.state().save_dialog_open());
    assert!(state(&w)["hyperlinks"].is_null(), "AltGr+K did not open Link");
    assert_eq!(w.state().ui_panics(), 0);
}

/// A field of a dialog (the Link dialog's address): AltGr+K types ł where the caret is, and egui's Ctrl+K does not cut the rest of the address.
#[test]
fn altgr_in_a_dialog_field_types_the_character_and_cuts_nothing() {
    let mut w = edit_window(WIDE);
    three_shapes(&mut w);
    run_command(&mut w, "window.hyperlinks", json!({}));
    press(&mut w, "links.address");
    type_text(&mut w, "abc");
    key(&mut w, Key::Home, Modifiers::NONE);
    altgr(&mut w, Key::K, "ł");
    assert_eq!(state(&w)["hyperlinks"]["rows"][0]["address"], json!("łabc"), "{}", state(&w)["hyperlinks"]);
    altgr(&mut w, Key::E, "€");
    assert_eq!(state(&w)["hyperlinks"]["rows"][0]["address"], json!("ł€abc"));
    assert!(state(&w)["hyperlinks"].is_object(), "the dialog is still open");
}

/// The ShapeSheet: the AltGr character types into the cell editor begun by a character, in the formula bar, and begins an edit from a selected cell.
#[test]
fn altgr_in_the_shapesheet_grid_and_formula_bar_types_the_character() {
    let shape = ShapeBuilder::new(10).cell_u("Width", "2", "IN").cell_u("Height", "1", "IN").cell("PinX", "4").cell("PinY", "4");
    let mut w = window_on(one_page(shape), [1400.0, 900.0]);
    open_shape(&mut w, 10);
    run_command(&mut w, "window.sheetSelect", json!({"cell": "Width"}));
    let edit = |w: &Window| sheet(w)["edit"].clone();
    let steps = w.state().session().history().map_or(0, |h| h.undo_len());
    altgr(&mut w, Key::E, "€");
    assert_eq!(edit(&w)["text"], json!("€"), "the character began an edit of the cell: {}", edit(&w));
    key(&mut w, Key::Home, Modifiers::NONE);
    altgr(&mut w, Key::K, "ł");
    assert_eq!(edit(&w)["text"], json!("ł€"), "and the formula bar took the next one without cutting the first");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(edit(&w), Value::Null);
    assert_eq!(w.state().session().history().map_or(0, |h| h.undo_len()), steps, "nothing was written");
}

/// Ctrl+Alt+Home is not Ctrl+Home in the sheet.
#[test]
fn ctrl_alt_home_in_the_shapesheet_is_not_ctrl_home() {
    let shape = ShapeBuilder::new(10).cell_u("Width", "2", "IN").cell_u("Height", "1", "IN").cell("PinX", "4").cell("PinY", "4");
    let mut w = window_on(one_page(shape), [1400.0, 900.0]);
    open_shape(&mut w, 10);
    run_command(&mut w, "window.sheetSelect", json!({"cell": "PinY"}));
    let on = |w: &Window| sheet(w)["selected"].clone();
    let before = on(&w);
    key(&mut w, Key::Home, CTRL_ALT);
    assert_eq!(on(&w), before, "Ctrl+Alt+Home moves nothing");
    key(&mut w, Key::Home, ctrl());
    assert_ne!(on(&w), before, "Ctrl+Home goes to the first cell");
}

// ---- the Mac -------------------------------------------------------------------------------------------------------------------------------

/// On a Mac Option with a key types a character (å, ç, a dead key's accent): with a shape selected it starts the shape's text, as it does in a text field. Command and Option keys
/// stay the window's: Command+Option+J flips vertically, and Command+Option+K is no Command+K.
#[test]
fn on_a_mac_option_types_characters_and_option_command_keys_still_run() {
    let mut w = edit_window(WIDE);
    let ids = three_shapes(&mut w);
    w.ctx.set_os(OperatingSystem::Mac);
    // Option+A types å.
    hold(&mut w, Modifiers::ALT);
    w.input_mut().events.push(event(Key::A, true, Modifiers::ALT));
    w.input_mut().events.push(Event::Text("å".to_owned()));
    w.step();
    hold(&mut w, Modifiers::NONE);
    w.rest();
    assert_eq!(w.state().edited_text(), Some("å"), "Option+A starts the text with å");
    key(&mut w, Key::Escape, Modifiers::NONE);
    key(&mut w, Key::Escape, Modifiers::NONE);
    run_command(&mut w, "edit.select", json!({"ids": [ids[2]]}));
    // Command+Option+K is not Command+K (Link).
    key(&mut w, Key::K, CMD_OPTION);
    assert!(state(&w)["hyperlinks"].is_null(), "Command+Option+K is not Command+K");
    key(&mut w, Key::K, COMMAND_MAC);
    assert!(state(&w)["hyperlinks"].is_object(), "Command+K opens Link");
    run_command(&mut w, "window.hyperlinks", json!({"close": true}));
    // Command+Option+J is the Mac's Flip Vertical, and still runs.
    let revision = w.state().session().revision();
    key(&mut w, Key::J, CMD_OPTION);
    assert!(w.state().session().revision() > revision, "Command+Option+J flips vertically");
    // A text box on a Mac takes Option's characters too.
    key(&mut w, Key::F2, Modifiers::NONE);
    hold(&mut w, Modifiers::ALT);
    w.input_mut().events.push(event(Key::E, true, Modifiers::ALT));
    w.input_mut().events.push(Event::Text("€".to_owned()));
    w.step();
    hold(&mut w, Modifiers::NONE);
    w.rest();
    assert_eq!(w.state().edited_text(), Some("€"));
    assert_eq!(w.state().ui_panics(), 0);
}

/// What leaves a typed character to the text: no modifier, Shift, and AltGr (Ctrl and Alt together) off a Mac; Option on a Mac. Ctrl alone, Alt alone off a Mac, and Command are the window's keys.
#[test]
fn a_character_is_the_texts_with_no_chord_with_altgr_and_with_option_on_a_mac() {
    for os in [OperatingSystem::Windows, OperatingSystem::Nix] {
        assert!(types_text(os, &Modifiers::NONE) && types_text(os, &Modifiers::SHIFT), "{os:?}: a plain key types");
        assert!(types_text(os, &CTRL_ALT) && types_text(os, &CTRL_ALT_SHIFT), "{os:?}: AltGr types");
        assert!(!types_text(os, &ctrl()) && !types_text(os, &CTRL_SHIFT), "{os:?}: Ctrl alone is a chord");
        assert!(!types_text(os, &Modifiers::ALT), "{os:?}: Alt alone is the window's");
    }
    let mac = OperatingSystem::Mac;
    assert!(types_text(mac, &Modifiers::NONE) && types_text(mac, &Modifiers::ALT) && types_text(mac, &Modifiers { shift: true, ..Modifiers::ALT }));
    assert!(!types_text(mac, &COMMAND_MAC) && !types_text(mac, &CMD_OPTION), "Command makes a chord, with Option or without");
    assert!(!types_text(mac, &Modifiers { ctrl: true, ..Modifiers::NONE }), "Control makes a chord");
}

// ---- the table of keys ---------------------------------------------------------------------------------------------------------------------

/// The keys that type a sign or a digit, which a layout may type with Shift held (the one extra Shift the window allows).
fn types_a_sign(key: Key) -> bool {
    matches!(
        key,
        Key::Num0
            | Key::Num1
            | Key::Num2
            | Key::Num3
            | Key::Num4
            | Key::Num5
            | Key::Num6
            | Key::Num7
            | Key::Num8
            | Key::Num9
            | Key::Equals
            | Key::Minus
            | Key::Plus
            | Key::Period
            | Key::Comma
            | Key::Colon
            | Key::Semicolon
            | Key::Slash
            | Key::Backslash
            | Key::Pipe
            | Key::Questionmark
            | Key::Exclamationmark
            | Key::OpenBracket
            | Key::CloseBracket
            | Key::OpenCurlyBracket
            | Key::CloseCurlyBracket
            | Key::Backtick
    )
}

/// The modifiers a press of `shortcut` carries on `os`: the shortcut's own, with `Cmd` as the Command key on a Mac and as Ctrl (which is egui's command there) elsewhere.
fn press_of_shortcut(os: OperatingSystem, shortcut: &KeyboardShortcut) -> Modifiers {
    let m = shortcut.modifiers;
    let mac = os == OperatingSystem::Mac;
    let held = m.command || m.ctrl || m.mac_cmd;
    Modifiers { alt: m.alt, ctrl: if mac { m.ctrl } else { held }, shift: m.shift, mac_cmd: mac && (m.command || m.mac_cmd), command: held }
}

/// For every key of the window on every system: its own press runs it; the same press with Alt added, or Ctrl or Command added, or Shift added to a key that is not a character,
/// runs no key the table does not list with that very combination. **No Ctrl or Command key runs for an AltGr press** (Ctrl and Alt held, whatever the letter).
#[test]
fn no_key_of_the_window_runs_for_a_press_with_other_modifiers() {
    for os in [OperatingSystem::Mac, OperatingSystem::Windows, OperatingSystem::Nix] {
        let all = bound_keys(os, true);
        assert!(all.len() > 40, "{os:?}: the table is the window's");
        for b in &all {
            let own = press_of_shortcut(os, b);
            assert!(press_matches(os, b, b.logical_key, own), "{os:?}: {b:?} runs for its own press");
            let mut others = Vec::new();
            if !own.alt {
                others.push(Modifiers { alt: true, ..own });
            }
            if !(own.command || own.ctrl || own.mac_cmd) {
                others.push(Modifiers { ctrl: true, command: os != OperatingSystem::Mac, ..own });
            }
            if !own.shift && !types_a_sign(b.logical_key) {
                others.push(Modifiers { shift: true, ..own });
            }
            for press in others {
                for a in &all {
                    if press_matches(os, a, b.logical_key, press) {
                        let (word_a, word_p) = (press_of_shortcut(os, a), press);
                        assert!(
                            word_a.alt == press.alt
                                && (word_a.command || word_a.ctrl) == (press.command || press.ctrl)
                                && word_a.shift == press.shift,
                            "{os:?}: {a:?} runs for {b:?} with {word_p:?}"
                        );
                    }
                }
            }
        }
        // AltGr (Ctrl and Alt) with any letter, digit or symbol key: only a key that lists Ctrl and Alt both may match.
        let altgr_press =
            |os: OperatingSystem| Modifiers { alt: true, ctrl: true, shift: false, mac_cmd: false, command: os != OperatingSystem::Mac };
        for b in &all {
            let press = altgr_press(os);
            for a in all.iter().filter(|a| a.logical_key == b.logical_key) {
                if press_matches(os, a, b.logical_key, press) {
                    assert!(a.modifiers.alt, "{os:?}: {a:?} runs for AltGr+{:?}", b.logical_key);
                }
            }
        }
    }
    // The documented allowance: one extra Shift, on a key that is a character and not a letter.
    let os = OperatingSystem::Nix;
    let zoom = parse("Cmd+=").unwrap();
    assert!(press_matches(os, &zoom, Key::Equals, CTRL_SHIFT) && press_matches(os, &zoom, Key::Equals, ctrl()));
    assert!(!press_matches(os, &zoom, Key::Equals, CTRL_ALT));
    let link = parse("Cmd+K").unwrap();
    assert!(!press_matches(os, &link, Key::K, CTRL_SHIFT) && !press_matches(os, &link, Key::K, CTRL_ALT) && press_matches(os, &link, Key::K, ctrl()));
    let small_caps = parse("Cmd+Shift+K").unwrap();
    assert!(press_matches(os, &small_caps, Key::K, CTRL_SHIFT) && !press_matches(os, &small_caps, Key::K, CTRL_ALT_SHIFT));
}

/// Only a key that types a sign or a digit takes the one extra Shift: every key egui lists, as a Ctrl shortcut, with Ctrl and Shift held; and none takes an extra Alt.
#[test]
fn only_a_sign_or_a_digit_takes_an_extra_shift_and_no_key_takes_an_extra_alt() {
    for os in [OperatingSystem::Mac, OperatingSystem::Windows, OperatingSystem::Nix] {
        for key in Key::ALL {
            let shortcut = KeyboardShortcut::new(Modifiers::COMMAND, *key);
            let with_shift = press_of_shortcut(os, &KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, *key));
            assert_eq!(press_matches(os, &shortcut, *key, with_shift), types_a_sign(*key), "{os:?}: {key:?} with an extra Shift");
            let with_alt = press_of_shortcut(os, &KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::ALT, *key));
            assert!(!press_matches(os, &shortcut, *key, with_alt), "{os:?}: {key:?} with an extra Alt");
        }
    }
}

/// The browser version's page takes a `keydown` only for a key the window runs, now that the window runs a key for its own press only: Ctrl+Alt+N is no New. A press where AltGr typed a
/// character (the `key` of the event is the character, the `code` the key's own) is the character's, and the page leaves it to the browser so that it types.
#[test]
fn the_page_takes_a_keydown_for_its_own_press_only_and_leaves_altgr_characters_to_the_browser() {
    let windows =
        KeyTaker::for_user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/158.0.0.0 Safari/537.36");
    assert!(windows.takes("n", "KeyN", true, false, false, false), "Ctrl+N");
    assert!(!windows.takes("n", "KeyN", true, false, false, true), "Ctrl+Alt+N is no New");
    assert!(!windows.takes("N", "KeyN", true, false, true, true), "nor Ctrl+Alt+Shift+N");
    // Ctrl+Alt+P is a key of the window.
    assert!(windows.takes("p", "KeyP", true, false, false, true), "Ctrl+Alt+P");
    // AltGr+P on a layout that types a character there: the browser's `key` is that character.
    assert!(!windows.takes("¶", "KeyP", true, false, false, true), "AltGr+P typing a character is left to the browser");
    assert!(!windows.takes("€", "KeyE", true, false, false, true));
    assert!(!windows.takes("ł", "KeyK", true, false, false, true));
    // A press with Ctrl alone on a layout of another script still names its key by the code.
    assert!(windows.takes("\u{ff}", "KeyN", true, false, false, false), "the code names the key when the character does not");
}
