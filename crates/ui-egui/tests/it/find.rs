//! The Find and Replace dialogs (Home > Editing > Find). Every control is a press on what the window drew, and what is
//! checked is the drawing's own text, the undo history and the window's state: the dialog is a floating window that does not shade the drawing area, Find Next selects the
//! shape of a match and shows its page, and Replace and Replace All are the engine's `edit.replace`.

use diacraft_ui_egui::DiacraftApp;
use egui::{Key, Modifiers, vec2};
use serde_json::{Value, json};

use crate::editing::{edit_window, key, type_text};
use crate::support::{Rest, Window, click, press, run_command};

/// A window with a drawing of three shapes on page 1 (`alpha one`, `beta`, `alpha two`) and one on page 2 (`alpha three`).
fn drawing() -> (Window, Vec<u32>, u32) {
    let mut w = edit_window([1280.0, 900.0]);
    let page1 = w.state().session().document().unwrap().pages.get(0).unwrap().id;
    let mut ids = Vec::new();
    for (i, text) in ["alpha one", "beta", "alpha two"].into_iter().enumerate() {
        let x = 2.0 + 3.0 * i as f64;
        let dropped = w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": page1, "x": x, "y": 6.0})).unwrap();
        let id = dropped["id"].as_u64().unwrap() as u32;
        w.state_mut().execute("shape.set_text", &json!({"id": id, "page": page1, "text": text})).unwrap();
        ids.push(id);
    }
    run_command(&mut w, "page.add", json!({}));
    let page2 = w.state().session().document().unwrap().pages.get(1).unwrap().id;
    let dropped = w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": page2, "x": 4.0, "y": 4.0})).unwrap();
    let id = dropped["id"].as_u64().unwrap() as u32;
    w.state_mut().execute("shape.set_text", &json!({"id": id, "page": page2, "text": "alpha three"})).unwrap();
    ids.push(id);
    run_command(&mut w, "page.select", json!({"id": page1}));
    run_command(&mut w, "edit.select", json!({"ids": []}));
    (w, ids, page2)
}

fn state(w: &Window) -> Value {
    w.state().window_state()
}

fn find_state(w: &Window) -> Value {
    state(w)["find"].clone()
}

fn text_of(w: &Window, page: usize, id: u32) -> String {
    let doc = w.state().session().document().unwrap();
    let page = doc.pages.get(page).unwrap().id;
    diacraft_engine::draw::text_view(doc, page, id).unwrap().chars
}

fn undo_steps(w: &Window) -> usize {
    w.state().session().history().map_or(0, diacraft_engine::History::undo_len)
}

/// Clicks the box named `name`, and types `text` into it.
fn type_into(w: &mut Window, name: &str, text: &str) {
    let at = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    click(w, at);
    w.rest();
    type_text(w, text);
}

fn line(w: &Window) -> String {
    find_state(w)["line"].as_str().unwrap_or_default().to_owned()
}

fn selection(w: &Window) -> Vec<u32> {
    w.state().selection().to_vec()
}

fn open(w: &mut Window, mode: &str) {
    run_command(w, "window.find", json!({ "mode": mode }));
}

// ---------------------------------------------------------------------------------------------------------------------- the menu

/// Home > Editing > Find is a menu of Find... and Replace..., both wired: each opens its dialog.
#[test]
fn the_find_menu_opens_the_find_dialog_and_the_replace_dialog() {
    let (mut w, _, _) = drawing();
    assert!(w.state().element("ribbon.Home.FindMenu").unwrap().enabled, "the Find menu works with a drawing open");
    assert!(find_state(&w).is_null(), "closed to begin with");
    press(&mut w, "ribbon.Home.FindMenu");
    w.rest();
    for item in ["menu.FindDialog", "menu.ReplaceDialog"] {
        assert!(w.state().element(item).unwrap_or_else(|| panic!("{item} is not drawn")).enabled, "{item}");
    }
    press(&mut w, "menu.FindDialog");
    w.rest();
    assert_eq!(find_state(&w)["mode"], "find");
    for name in [
        "find.what",
        "find.next",
        "find.close",
        "find.scope.selection",
        "find.scope.page",
        "find.scope.all",
        "find.in.text",
        "find.in.data",
        "find.in.name",
        "find.in.user",
        "find.match_case",
        "find.whole_words",
        "find.found_in",
    ] {
        assert!(w.state().element(name).is_some(), "{name} is not drawn in the Find dialog");
    }
    for name in ["find.with", "find.replace", "find.replace_all"] {
        assert!(w.state().element(name).is_none(), "{name} is the Replace dialog's");
    }
    assert!(!w.state().element("find.next").unwrap().enabled, "Find Next needs something to find");
    press(&mut w, "find.close");
    w.rest();
    assert!(find_state(&w).is_null(), "Close closes it");

    press(&mut w, "ribbon.Home.FindMenu");
    w.rest();
    press(&mut w, "menu.ReplaceDialog");
    w.rest();
    assert_eq!(find_state(&w)["mode"], "replace");
    for name in [
        "find.what",
        "find.with",
        "find.replace",
        "find.replace_all",
        "find.next",
        "find.close",
        "find.scope.page",
        "find.match_case",
        "find.whole_words",
    ] {
        assert!(w.state().element(name).is_some(), "{name} is not drawn in the Replace dialog");
    }
    for name in ["find.in.text", "find.in.data", "find.in.name", "find.in.user"] {
        assert!(w.state().element(name).is_none(), "{name}: the Replace dialog lists no kind of place");
    }
}

/// Ctrl+F opens the Find dialog with the keyboard in its box; typing goes there, and Escape closes it. (The specification gives no key for Find: Ctrl+F is ours, and
/// Ctrl+H stays Flip Horizontal, the specification's.)
#[test]
fn ctrl_f_opens_the_find_dialog_and_ctrl_h_is_still_flip_horizontal() {
    let (mut w, ids, _) = drawing();
    key(&mut w, Key::F, Modifiers::COMMAND);
    assert_eq!(find_state(&w)["mode"], "find");
    type_text(&mut w, "beta");
    assert_eq!(find_state(&w)["what"], "beta", "the keyboard was in the box");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(find_state(&w).is_null(), "Escape closes it");
    // Ctrl+H is the specification's Flip Horizontal: with a shape selected it flips it, and opens no dialog.
    run_command(&mut w, "edit.select", json!({"ids": [ids[0]]}));
    key(&mut w, Key::H, Modifiers::COMMAND);
    assert!(find_state(&w).is_null(), "Ctrl+H does not open Replace");
}

// ------------------------------------------------------------------------------------------------------------------- Find Next

/// Find Next selects the shape of the match, one after the other, and goes round to the first.
#[test]
fn find_next_selects_the_shape_of_each_match_and_goes_round() {
    let (mut w, ids, _) = drawing();
    let steps = undo_steps(&w);
    open(&mut w, "find");
    assert!(!w.state().element("find.next").unwrap().enabled);
    type_into(&mut w, "find.what", "alpha");
    assert!(w.state().element("find.next").unwrap().enabled);
    assert_eq!(selection(&w), Vec::<u32>::new());
    press(&mut w, "find.next");
    w.rest();
    assert_eq!(selection(&w), [ids[0]], "the first match: `alpha one`");
    assert!(line(&w).starts_with("Found in: text block of"), "{}", line(&w));
    press(&mut w, "find.next");
    w.rest();
    assert_eq!(selection(&w), [ids[2]], "the second: `alpha two`");
    press(&mut w, "find.next");
    w.rest();
    assert_eq!(selection(&w), [ids[0]], "the page has two matches, so it goes round");
    assert!(line(&w).contains("went round"), "{}", line(&w));
    // A text that is not there.
    run_command(&mut w, "window.find", json!({"what": "zzz"}));
    press(&mut w, "find.next");
    w.rest();
    assert!(line(&w).starts_with("Can’t find “zzz”"), "{}", line(&w));
    assert!(find_state(&w)["problem"].as_bool().unwrap());
    assert_eq!(undo_steps(&w), steps, "Find changes nothing: no undo step");
}

/// Enter in Find what is Find Next, and the box keeps the keyboard.
#[test]
fn enter_in_find_what_finds_the_next_match() {
    let (mut w, ids, _) = drawing();
    open(&mut w, "find");
    type_into(&mut w, "find.what", "beta");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert_eq!(selection(&w), [ids[1]]);
    type_text(&mut w, "x");
    assert_eq!(find_state(&w)["what"], "betax", "the box kept the keyboard");
}

/// All pages: the next match may be on another page, which is shown.
#[test]
fn find_next_across_pages_shows_the_page_of_the_match() {
    let (mut w, ids, page2) = drawing();
    let page1 = state(&w)["page"].as_u64().unwrap();
    open(&mut w, "find");
    press(&mut w, "find.scope.all");
    w.rest();
    assert_eq!(find_state(&w)["scope"], "all");
    type_into(&mut w, "find.what", "alpha");
    for expect in [(page1, ids[0]), (page1, ids[2]), (u64::from(page2), ids[3]), (page1, ids[0])] {
        press(&mut w, "find.next");
        w.rest();
        assert_eq!((state(&w)["page"].as_u64().unwrap(), selection(&w)[0]), expect, "{}", line(&w));
    }
    assert!(line(&w).contains("page"), "{}", line(&w));
}

/// Match case and Find whole words only change what is found; Special puts the special characters in the box.
#[test]
fn the_options_and_the_special_list_change_what_is_found() {
    let (mut w, ids, _) = drawing();
    open(&mut w, "find");
    type_into(&mut w, "find.what", "ALPHA");
    press(&mut w, "find.next");
    assert_eq!(selection(&w), [ids[0]], "case is not matched by default");
    press(&mut w, "find.match_case");
    w.rest();
    assert_eq!(find_state(&w)["matchCase"], true);
    press(&mut w, "find.next");
    w.rest();
    assert!(line(&w).starts_with("Can’t find"), "{}", line(&w));
    // Whole words.
    run_command(&mut w, "window.find", json!({"what": "alph", "match_case": false}));
    press(&mut w, "find.next");
    w.rest();
    assert_eq!(selection(&w), [ids[0]]);
    press(&mut w, "find.whole_words");
    w.rest();
    press(&mut w, "find.next");
    w.rest();
    assert!(line(&w).starts_with("Can’t find"), "`alph` is not a whole word");
    // The Special list adds its characters to Find what.
    run_command(&mut w, "window.find", json!({"what": "alp"}));
    press(&mut w, "find.special");
    w.rest();
    press(&mut w, "find.special.^?");
    w.rest();
    assert_eq!(find_state(&w)["what"], "alp^?");
}

/// What to search: the Find dialog lists four kinds of place and Shape text is the one ticked.
#[test]
fn shape_data_and_names_and_user_cells_are_searched_when_ticked() {
    let (mut w, ids, _) = drawing();
    open(&mut w, "find");
    assert_eq!(find_state(&w)["in"], json!(["text"]));
    // A shape's name: the shapes dropped are called Rectangle.<id>.
    run_command(&mut w, "window.find", json!({"what": "Rectangle"}));
    press(&mut w, "find.next");
    w.rest();
    assert!(line(&w).starts_with("Can’t find"), "names are not searched by default");
    press(&mut w, "find.in.name");
    w.rest();
    assert_eq!(find_state(&w)["in"], json!(["text", "shape_name"]));
    press(&mut w, "find.next");
    w.rest();
    assert_eq!(selection(&w), [ids[0]]);
    assert!(line(&w).starts_with("Found in: shape name of"), "{}", line(&w));
    // Nothing ticked is said.
    run_command(&mut w, "window.find", json!({"in": []}));
    assert!(line(&w).is_empty() || find_state(&w)["in"] == json!([]), "{}", line(&w));
}

// ---------------------------------------------------------------------------------------------------------------- the selection

/// The Selection scope is enabled when shapes are selected, and searches only them.
#[test]
fn the_selection_scope_needs_a_selection_and_looks_only_in_it() {
    let (mut w, ids, _) = drawing();
    open(&mut w, "find");
    assert!(!w.state().element("find.scope.selection").unwrap().enabled, "nothing is selected");
    run_command(&mut w, "edit.select", json!({"ids": [ids[2]]}));
    assert!(w.state().element("find.scope.selection").unwrap().enabled);
    press(&mut w, "find.scope.selection");
    w.rest();
    type_into(&mut w, "find.what", "alpha");
    press(&mut w, "find.next");
    w.rest();
    assert_eq!(selection(&w), [ids[2]], "only `alpha two` is in the selection");
    run_command(&mut w, "window.find", json!({"what": "beta"}));
    press(&mut w, "find.next");
    w.rest();
    assert!(line(&w).starts_with("Can’t find"), "{}", line(&w));
}

// -------------------------------------------------------------------------------------------------------------------- non-modal

/// The dialog is not a modal: the drawing under it takes the pointer and the keys while it is open, and a press on the dialog does not reach the drawing.
#[test]
fn the_dialog_does_not_shade_the_window_and_the_drawing_stays_usable() {
    let (mut w, ids, _) = drawing();
    open(&mut w, "find");
    assert!(!w.state().is_modal_open(), "a Find dialog is not a modal");
    // A shape is picked on the page by the pointer while the dialog is open.
    run_command(&mut w, "tool.pointer", json!({}));
    let page_rect = state(&w)["pageRect"].clone();
    let ppi = state(&w)["pointsPerInch"].as_f64().unwrap() as f32;
    let (left, top, _, height) =
        (page_rect[0].as_f64().unwrap() as f32, page_rect[1].as_f64().unwrap() as f32, 0.0, page_rect[3].as_f64().unwrap() as f32);
    // Shape 1 is dropped at (2, 6) inches from the lower left of the page.
    let at = egui::pos2(left + 2.0 * ppi, top + height - 6.0 * ppi);
    click(&mut w, at);
    w.rest();
    assert_eq!(selection(&w), [ids[0]], "a click on the drawing selected the shape");
    assert!(!find_state(&w).is_null(), "and the dialog is still open");
    // A press on the dialog's own window does not clear the selection of the drawing.
    let window = w.state().element("find.window").unwrap().rect;
    click(&mut w, window.left_top() + vec2(12.0, 40.0));
    w.rest();
    assert_eq!(selection(&w), [ids[0]], "the dialog took the press");
    // Keys go to the drawing when the box has not the keyboard.
    assert!(!find_state(&w).is_null());
}

// ------------------------------------------------------------------------------------------------------------------- Replace

/// Replace finds the first match, and then replaces the one it is at and finds the next: each press is its own undo step, and Replace All is one.
#[test]
fn replace_and_replace_all_are_one_undo_step_each() {
    let (mut w, ids, _) = drawing();
    open(&mut w, "replace");
    type_into(&mut w, "find.what", "alpha");
    type_into(&mut w, "find.with", "omega");
    let steps = undo_steps(&w);
    press(&mut w, "find.replace");
    w.rest();
    assert_eq!(selection(&w), [ids[0]], "with no match to be at, Replace finds one");
    assert_eq!(undo_steps(&w), steps, "and replaces nothing");
    assert_eq!(text_of(&w, 0, ids[0]), "alpha one");
    press(&mut w, "find.replace");
    w.rest();
    assert_eq!(text_of(&w, 0, ids[0]), "omega one", "the match it was at is replaced");
    assert_eq!(selection(&w), [ids[2]], "and the next is found");
    assert_eq!(undo_steps(&w), steps + 1, "one step for the one");
    assert_eq!(w.state().session().history().unwrap().next_undo_label(), Some("Replace"));
    press(&mut w, "find.replace");
    w.rest();
    assert_eq!(text_of(&w, 0, ids[2]), "omega two");
    assert_eq!(undo_steps(&w), steps + 2);
    // Replace All of what is left on the page: nothing on page 1, and the answer says so.
    press(&mut w, "find.replace_all");
    w.rest();
    assert!(line(&w).starts_with("Nothing to replace"), "{}", line(&w));
    assert_eq!(undo_steps(&w), steps + 2, "no step for nothing");
    // All pages, All at once: one step.
    press(&mut w, "find.scope.all");
    w.rest();
    run_command(&mut w, "window.find", json!({"what": "omega", "with": "sigma"}));
    press(&mut w, "find.replace_all");
    w.rest();
    assert_eq!(line(&w), "Replaced 2 matches in 2 shapes.");
    assert_eq!(text_of(&w, 0, ids[0]), "sigma one");
    assert_eq!(undo_steps(&w), steps + 3, "one step for all");
    assert_eq!(w.state().session().history().unwrap().next_undo_label(), Some("Replace All"));
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(text_of(&w, 0, ids[0]), "omega one");
    assert_eq!(text_of(&w, 0, ids[2]), "omega two");
}

/// `window.find` with an action does what the button does, so a program can drive the dialog.
#[test]
fn window_find_runs_the_buttons_for_a_program() {
    let (mut w, ids, _) = drawing();
    run_command(&mut w, "window.find", json!({"mode": "find", "what": "beta", "action": "next"}));
    assert_eq!(selection(&w), [ids[1]]);
    assert_eq!(find_state(&w)["current"]["id"], ids[1]);
    run_command(&mut w, "window.find", json!({"mode": "replace", "with": "gamma", "action": "replace"}));
    assert_eq!(text_of(&w, 0, ids[1]), "gamma");
    assert_eq!(find_state(&w)["mode"], "replace", "the dialog changed to Replace and kept the text");
    assert_eq!(find_state(&w)["what"], "beta");
    run_command(&mut w, "window.find", json!({"close": true}));
    assert!(find_state(&w).is_null());
    // Bad parameters are refused in words.
    for (params, words) in [
        (json!({"mode": "search"}), "`mode` must be find or replace"),
        (json!({"scope": "somewhere"}), "`scope` must be selection, page or all"),
        (json!({"in": ["notes"]}), "`in` holds text"),
        (json!({"action": "explode"}), "`action` must be next, replace or replace_all"),
        (json!({"what": 3}), "`what` must be text"),
        (json!({"match_case": "yes"}), "`match_case` must be true or false"),
    ] {
        let message = w.state_mut().execute("window.find", &params).unwrap_err().to_string();
        assert!(message.contains(words), "{params}: {message}");
    }
}

/// The dialog needs a drawing, and another drawing replaces it.
#[test]
fn the_dialog_needs_a_drawing_and_is_forgotten_with_it() {
    let (mut w, _, _) = drawing();
    open(&mut w, "find");
    run_command(&mut w, "file.new", json!({}));
    assert!(find_state(&w).is_null(), "a new drawing has no dialog open");
    let mut app = DiacraftApp::new(diacraft_engine::Session::new(), None);
    assert!(app.execute("window.find", &json!({})).is_err(), "no drawing, no dialog");
}

/// The dialog fits the window at every size the window may be, in both themes, and its parts stand apart.
#[test]
fn the_dialog_fits_every_window_size_in_both_themes_and_its_parts_stand_apart() {
    for theme in ["light", "dark"] {
        for size in [[1280.0, 900.0], [900.0, 600.0], [700.0, 520.0]] {
            let mut w = edit_window(size);
            run_command(&mut w, "window.theme", json!({ "theme": theme }));
            for mode in ["find", "replace"] {
                open(&mut w, mode);
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(size[0], size[1]));
                let window = w.state().element("find.window").unwrap().rect;
                assert!(screen.contains_rect(window), "{theme} {size:?} {mode}: {window:?} is not inside the window {screen:?}");
                let parts: Vec<(String, egui::Rect, bool)> = crate::support::elements_named(&w, "find.")
                    .into_iter()
                    .filter(|(name, _, _)| {
                        matches!(
                            name.as_str(),
                            "find.what"
                                | "find.with"
                                | "find.next"
                                | "find.replace"
                                | "find.replace_all"
                                | "find.close"
                                | "find.found_in"
                                | "find.match_case"
                                | "find.whole_words"
                        )
                    })
                    .collect();
                for (i, (a, ra, _)) in parts.iter().enumerate() {
                    assert!(window.contains_rect(*ra), "{theme} {size:?} {mode}: {a} {ra:?} is outside its window {window:?}");
                    for (b, rb, _) in &parts[i + 1..] {
                        assert!(!ra.intersects(*rb), "{theme} {size:?} {mode}: {a} {ra:?} overlaps {b} {rb:?}");
                    }
                }
            }
        }
    }
}

/// Where the shape `id` of the page shown is on the screen, from the window's state and the engine's corners.
fn screen_rect(w: &Window, id: u32) -> egui::Rect {
    let state = state(w);
    let page = state["page"].as_u64().unwrap() as u32;
    let rect = &state["pageRect"];
    let (left, top, height) = (rect[0].as_f64().unwrap() as f32, rect[1].as_f64().unwrap() as f32, rect[3].as_f64().unwrap() as f32);
    let ppi = state["pointsPerInch"].as_f64().unwrap() as f32;
    let corners = diacraft_engine::draw::shape_corners(w.state().session().document().unwrap(), page, id).unwrap();
    let points: Vec<egui::Pos2> = corners.iter().map(|&(x, y)| egui::pos2(left + x as f32 * ppi, top + height - y as f32 * ppi)).collect();
    egui::Rect::from_points(&points)
}

/// Find Next brings the shape of a match into view and not under the dialog, which floats over the drawing area.
#[test]
fn a_match_under_the_dialog_is_brought_out_from_under_it() {
    let (mut w, ids, _) = drawing();
    run_command(&mut w, "view.zoom", json!({"percent": 150}));
    open(&mut w, "find");
    type_into(&mut w, "find.what", "a");
    let canvas = w.state().element("canvas").unwrap().rect;
    let mut seen = Vec::new();
    for _ in 0..3 {
        press(&mut w, "find.next");
        w.rest();
        let shape = screen_rect(&w, selection(&w)[0]);
        let dialog = w.state().element("find.window").unwrap().rect;
        assert!(!dialog.intersects(shape), "the match {:?} is under the dialog {dialog:?}", selection(&w));
        assert!(canvas.contains_rect(shape.shrink(1.0)) || canvas.intersects(shape), "{shape:?} is out of the drawing area {canvas:?}");
        seen.push(selection(&w)[0]);
    }
    assert_eq!(seen, [ids[0], ids[0], ids[1]], "`alpha one` holds two a, and then `beta`");
}

/// The earlier searches are kept, offered by the button beside the box, and a pick fills the box.
#[test]
fn the_earlier_searches_are_offered_again() {
    let (mut w, _, _) = drawing();
    open(&mut w, "find");
    for what in ["alpha", "beta"] {
        run_command(&mut w, "window.find", json!({"what": what, "action": "next"}));
    }
    assert_eq!(find_state(&w)["history"], json!(["beta", "alpha"]), "the newest first");
    press(&mut w, "find.what.earlier");
    w.rest();
    assert_eq!(w.state().element("find.what.history.0").map(|_| ()), Some(()), "the list is open");
    press(&mut w, "find.what.history.1");
    w.rest();
    assert_eq!(find_state(&w)["what"], "alpha", "the entry picked fills Find what");
}

/// The same when the command that opens the dialog is the one that finds: the dialog has not been drawn yet, and the shape is brought clear of where it will stand.
#[test]
fn a_match_found_by_the_command_that_opens_the_dialog_is_clear_of_it_too() {
    let (mut w, ids, _) = drawing();
    run_command(&mut w, "view.zoom", json!({"percent": 150}));
    for what in ["alpha", "two"] {
        run_command(&mut w, "window.find", json!({"close": true}));
        run_command(&mut w, "window.find", json!({"mode": "find", "what": what, "action": "next"}));
        let shape = screen_rect(&w, selection(&w)[0]);
        let dialog = w.state().element("find.window").unwrap().rect;
        assert!(!dialog.intersects(shape), "{what}: the match {:?} is under the dialog {dialog:?}", selection(&w));
    }
    assert_eq!(selection(&w), [ids[2]]);
}

/// Replace All passes over a shape on a locked layer, and the dialog's line says how many matches it left there apart from the others.
#[test]
fn replace_all_says_the_matches_it_left_on_locked_layers() {
    let (mut w, ids, _) = drawing();
    run_command(&mut w, "layer.add", json!({"name": "Frozen"}));
    run_command(&mut w, "layer.assign", json!({"ids": [ids[0]], "layers": ["Frozen"]}));
    run_command(&mut w, "layer.set", json!({"layer": "Frozen", "lock": true}));
    open(&mut w, "replace");
    type_into(&mut w, "find.what", "alpha");
    type_into(&mut w, "find.with", "omega");
    press(&mut w, "find.replace_all");
    w.rest();
    assert_eq!(line(&w), "Replaced 1 match. 1 match on locked layers left.");
    assert_eq!((text_of(&w, 0, ids[0]), text_of(&w, 0, ids[2])), ("alpha one".to_owned(), "omega two".to_owned()));
}
