//! The window's alt-text controls (`docs/editing-commands.md`, "Alt text and the accessibility check"): the Format Shape
//! pane's Alt Text tab, Page Setup's Alt Text tab, the shape menu's Edit Alt Text row, the Accessibility pane that Review, Accessibility, Check Accessibility opens, and the way a keyboard
//! and a screen reader meet them. The pointer and the keyboard are given to egui as a person's would be, and what is checked is the document the commands changed and what the window drew.

use std::sync::Arc;

use diacraft_engine::Session;
use diacraft_engine::doc::{Master, Section, Shape, ShapeList, Sheet, alt_text};
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::render_worker::bundled_drawer;
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, Pos2, Rect};
use egui_kittest::kittest::{By, NodeT, Queryable};
use serde_json::{Value, json};

use crate::editing::{at, drag_tile_to, edit_window, key, shape_ids, type_text};
use crate::saving::ctrl;
use crate::support::{Rest, Window, click, fast_window, move_to, press, run_command};

const TITLE: &str = "pane.alttext.title";
const DESCRIPTION: &str = "pane.alttext.description";
const PAGE_TITLE: &str = "pagesetup.alttext.title";
const PAGE_DESCRIPTION: &str = "pagesetup.alttext.description";

// ------------------------------------------------------------------------------------------------------------------------------- helpers

fn rect_of(w: &Window, name: &str) -> Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

fn is_enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

fn click_on(w: &mut Window, name: &str) {
    let to = rect_of(w, name).center();
    click(w, to);
    w.rest();
}

fn undo_steps(w: &Window) -> usize {
    w.state().session().history().unwrap().undo_len()
}

fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

/// `shape.alt_text` of the shape: what the engine says it has.
fn alt_of(w: &mut Window, id: u32) -> Value {
    w.state_mut().execute("shape.alt_text", &json!({"id": id})).unwrap()
}

fn set_alt(w: &mut Window, id: u32, params: Value) {
    let mut p = params;
    p["id"] = json!(id);
    run_command(w, "shape.alt_text", p);
}

fn page_alt(w: &mut Window) -> Value {
    w.state_mut().execute("page.alt_text", &json!({})).unwrap()
}

/// A rectangle dropped at (4, 5) and selected, the Alt Text tab of Format Shape open.
fn window_with_a_shape() -> (Window, u32) {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 4.0, 5.0);
    let id = shape_ids(&w)[0];
    run_command(&mut w, "format.options", json!({"tab": "Alt Text"}));
    (w, id)
}

/// Two shapes, both selected, the Alt Text tab open.
fn window_with_two_shapes() -> (Window, Vec<u32>) {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 3.0, 6.0);
    drag_tile_to(&mut w, "Diamond", 7.0, 6.0);
    let ids = shape_ids(&w);
    w.state_mut().select(&ids);
    run_command(&mut w, "format.options", json!({"tab": "Alt Text"}));
    (w, ids)
}

/// A drawing whose master 7 has the title "Valve" and the description "Handle", and one instance of it on the page, selected, with the Alt Text tab open.
fn instance_window() -> (Window, u32) {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    session
        .edit("test: a master with alt text", |d| {
            let mut section = Section::new("User", None);
            section.rows.push(Arc::new(alt_text::standard_row(alt_text::TITLE_ROW, "Valve")));
            section.rows.push(Arc::new(alt_text::standard_row(alt_text::DESCRIPTION_ROW, "Handle")));
            let mut shape = Shape::new(1);
            Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(section));
            d.add_master(Master {
                id: 7,
                name: Some(Arc::from("Valve")),
                shapes: ShapeList::from_vec(vec![Arc::new(shape)]),
                sheet: Arc::new(Sheet::default()),
                ..Master::default()
            })
            .unwrap();
            Ok(())
        })
        .unwrap();
    let dropped = session.execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0})).unwrap();
    let id = u32::try_from(dropped["id"].as_u64().unwrap()).unwrap();
    session
        .edit("test: an instance", |d| {
            let found = d.find_shape(diacraft_engine::doc::Owner::Page(1), id).unwrap();
            let page = Arc::make_mut(d.pages.get_mut(0).unwrap());
            let shape = Arc::make_mut(page.shapes.get_mut(usize::try_from(found.path[0]).unwrap()).unwrap());
            shape.master = Some(7);
            Arc::make_mut(&mut shape.sheet).sections.retain(|s| s.name.as_str() != "User");
            Ok(())
        })
        .unwrap();
    let mut w = fast_window(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()), [1280.0, 900.0]);
    w.rest();
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    run_command(&mut w, "edit.select", json!({"ids": [id]}));
    run_command(&mut w, "format.options", json!({"tab": "Alt Text"}));
    (w, id)
}

/// Types `text` in the one-line box `name` and ends it with Enter.
fn type_in(w: &mut Window, name: &str, text: &str) {
    click_on(w, name);
    key(w, Key::A, ctrl());
    type_text(w, text);
    key(w, Key::Enter, Modifiers::NONE);
}

/// Types `text` in the several-line box `name` and leaves it with Tab (Enter is a line break there).
fn type_in_lines(w: &mut Window, name: &str, text: &str) {
    click_on(w, name);
    key(w, Key::A, ctrl());
    type_text(w, text);
    key(w, Key::Tab, Modifiers::NONE);
}

/// What the Alt Text section reports of the two boxes (`altText` of the window state), `null` when it is not on the screen.
fn section(w: &Window) -> Value {
    w.state().window_state()["altText"].clone()
}

/// The words of the labels the last frame drew.
fn labels(w: &Window) -> Vec<String> {
    w.query_all(By::new().role(Role::Label)).filter_map(|n| n.accesskit_node().value().map(|v| v.to_string())).collect()
}

fn right_click(w: &mut Window, pos: Pos2) {
    for _ in 0..3 {
        move_to(w, pos);
    }
    for pressed in [true, false] {
        w.input_mut().events.push(Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        w.step();
    }
    w.rest();
}

fn focused(w: &Window, role: Role, label: &str) -> bool {
    w.get_by_role_and_label(role, label).accesskit_node().is_focused()
}

// ------------------------------------------------------------------------------------------------- the Format Shape pane's Alt Text tab

/// The pane has a third tab, Alt Text, with a Title and a Description for the shape selected; `format.options` opens it by either name.
#[test]
fn format_shape_has_an_alt_text_tab_with_a_title_and_a_description_for_the_shape_selected() {
    let (mut w, _) = window_with_a_shape();
    assert!(w.state().element("pane.format.tab.alt_text").is_some(), "the third tab is there");
    for name in [TITLE, DESCRIPTION, "pane.alttext.clear"] {
        assert!(w.state().element(name).is_some(), "{name} is drawn");
    }
    assert!(is_enabled(&w, TITLE) && is_enabled(&w, DESCRIPTION), "a shape is selected, so both boxes work");
    assert!(!section(&w).is_null(), "the window state says the section is on the screen");
    // The tab can be pressed, and the other two tabs leave it.
    click_on(&mut w, "pane.format.tab.fill_line");
    assert!(section(&w).is_null() && w.state().element(TITLE).is_none(), "the Fill & Line tab has no Alt Text boxes");
    click_on(&mut w, "pane.format.tab.alt_text");
    assert!(w.state().element(TITLE).is_some());
    // The command takes the tab by its name in any case, and by the name the Microsoft page gives the tab that holds the section.
    for name in ["alt text", "Size & Properties"] {
        run_command(&mut w, "format.options", json!({"tab": "Effects"}));
        assert!(section(&w).is_null());
        run_command(&mut w, "format.options", json!({"tab": name}));
        assert!(!section(&w).is_null(), "{name}");
    }
    let words = w.state_mut().execute("format.options", &json!({"tab": "Nonsense"})).unwrap_err().to_string();
    assert!(words.contains("Alt Text"), "the tabs are listed, this one among them: {words}");
}

/// The boxes are named for a screen reader, the title one line and the description several.
#[test]
fn the_alt_text_boxes_have_names_and_roles_in_the_accesskit_tree() {
    let (w, _) = window_with_a_shape();
    assert!(!w.get_by_role_and_label(Role::TextInput, "Alt text title").accesskit_node().is_disabled());
    assert!(!w.get_by_role_and_label(Role::MultilineTextInput, "Alt text description").accesskit_node().is_disabled());
    w.get_by_role_and_label(Role::Button, "Clear Alt Text");
    w.get_by_role_and_label(Role::Button, "Alt Text");
}

/// A change is written on Enter in the Title box and when the Description box loses the keyboard, **one undo step each**, and nothing else is touched.
#[test]
fn a_title_and_a_description_typed_are_one_shape_alt_text_command_and_one_undo_step_each() {
    let (mut w, id) = window_with_a_shape();
    let steps = undo_steps(&w);
    type_in(&mut w, TITLE, "Pump P-101");
    let read = alt_of(&mut w, id);
    assert_eq!((read["title"].as_str(), read["title_from"].as_str()), (Some("Pump P-101"), Some("own")));
    assert_eq!(undo_steps(&w), steps + 1, "the title is one step");
    type_in_lines(&mut w, DESCRIPTION, "Feeds the boiler");
    assert_eq!(alt_of(&mut w, id)["description"], "Feeds the boiler");
    assert_eq!(undo_steps(&w), steps + 2, "and the description is one more");
    // A line break typed in the description is kept as a line feed; the title is one line.
    click_on(&mut w, DESCRIPTION);
    key(&mut w, Key::A, ctrl());
    type_text(&mut w, "Feeds the boiler");
    key(&mut w, Key::Enter, Modifiers::NONE);
    type_text(&mut w, "and the tank");
    key(&mut w, Key::Tab, Modifiers::NONE);
    assert_eq!(alt_of(&mut w, id)["description"], "Feeds the boiler\nand the tank");
    // Undo takes one step back at a time.
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(alt_of(&mut w, id)["description"], "Feeds the boiler");
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(alt_of(&mut w, id)["description"], "");
    assert_eq!(alt_of(&mut w, id)["title"], "Pump P-101", "the title is left alone");
    assert_eq!(w.state().ui_panics(), 0);
}

/// A box left with the text it showed sends nothing (no undo step, the drawing is not made unsaved), and a text typed and left by Tab goes to the shape it was typed for.
#[test]
fn a_box_passed_through_sends_nothing_and_tab_sends_the_title_and_moves_on() {
    let (mut w, id) = window_with_a_shape();
    set_alt(&mut w, id, json!({"title": "Pump"}));
    let (steps, revision) = (undo_steps(&w), w.state().session().revision());
    click_on(&mut w, TITLE);
    key(&mut w, Key::Tab, Modifiers::NONE);
    key(&mut w, Key::Tab, Modifiers::NONE);
    assert_eq!((undo_steps(&w), w.state().session().revision()), (steps, revision), "passing through writes nothing");
    click_on(&mut w, TITLE);
    key(&mut w, Key::A, ctrl());
    type_text(&mut w, "Pump P-2");
    key(&mut w, Key::Tab, Modifiers::NONE);
    assert_eq!(alt_of(&mut w, id)["title"], "Pump P-2");
    assert!(focused(&w, Role::MultilineTextInput, "Alt text description"), "Tab moved to the Description box");
}

/// Esc puts the old text back and sends nothing.
#[test]
fn escape_puts_the_old_text_back_and_sends_nothing() {
    let (mut w, id) = window_with_a_shape();
    set_alt(&mut w, id, json!({"title": "Pump"}));
    let steps = undo_steps(&w);
    click_on(&mut w, TITLE);
    key(&mut w, Key::A, ctrl());
    type_text(&mut w, "Nope");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(alt_of(&mut w, id)["title"], "Pump");
    assert_eq!(undo_steps(&w), steps);
    assert_eq!(w.state().pane_field_text(TITLE), Some("Pump"), "the box shows the old text again");
    // The several-line box too.
    set_alt(&mut w, id, json!({"description": "Old"}));
    let steps = undo_steps(&w);
    click_on(&mut w, DESCRIPTION);
    key(&mut w, Key::A, ctrl());
    type_text(&mut w, "New");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(alt_of(&mut w, id)["description"], "Old");
    assert_eq!(undo_steps(&w), steps);
}

/// No trimming, no case change; RTL text and combining marks are kept character for character.
#[test]
fn right_to_left_and_combining_text_and_spaces_are_kept_character_for_character() {
    let (mut w, id) = window_with_a_shape();
    // The test harness panics for a letter no bundled face draws; the app draws a box for it (the policy `egui` has by default).
    w.ctx.set_missing_glyph_policy(egui::MissingGlyphPolicy::Tofu);
    for text in ["\u{5e9}\u{5dc}\u{5d5}\u{5dd} \u{5e2}\u{5d5}\u{5dc}\u{5dd}", "e\u{301}a\u{308}\u{323} \u{644}\u{627}", " Pump ", "\u{a0}"] {
        type_in(&mut w, TITLE, text);
        assert_eq!(alt_of(&mut w, id)["title"].as_str(), Some(text), "{text:?}");
        assert_eq!(alt_of(&mut w, id)["title_length"], text.chars().count());
    }
    type_in_lines(&mut w, DESCRIPTION, "\u{5e9}\u{5dc}\u{5d5}\u{5dd}\nx\u{301}");
    assert_eq!(alt_of(&mut w, id)["description"], "\u{5e9}\u{5dc}\u{5d5}\u{5dd}\nx\u{301}");
    assert_eq!(w.state().ui_panics(), 0);
}

/// Both boxes take at most 255 characters, typed or pasted, and say so in words. A paste of 10,000 characters is cut at 255.
#[test]
fn a_paste_of_ten_thousand_characters_is_cut_at_255_and_the_box_says_why() {
    let (mut w, id) = window_with_a_shape();
    click_on(&mut w, DESCRIPTION);
    key(&mut w, Key::A, ctrl());
    w.input_mut().events.push(Event::Paste("x".repeat(10_000)));
    w.step();
    w.rest();
    assert_eq!(w.state().pane_field_text(DESCRIPTION).map(|t| t.chars().count()), Some(255), "the box took 255");
    assert!(labels(&w).iter().any(|l| l.contains("255 of 255") && l.contains("no more")), "the words say the box is full: {:?}", labels(&w));
    key(&mut w, Key::Tab, Modifiers::NONE);
    let read = alt_of(&mut w, id);
    assert_eq!(read["description_length"], 255, "255 reached the shape, and no more");
    // The Title box takes the same, and a pasted line break is a space.
    click_on(&mut w, TITLE);
    w.input_mut().events.push(Event::Paste(format!("Pump\nP-101{}", "y".repeat(400))));
    w.step();
    key(&mut w, Key::Enter, Modifiers::NONE);
    let title = alt_of(&mut w, id)["title"].as_str().unwrap().to_owned();
    assert!(title.starts_with("Pump P-101y") && title.chars().count() == 255, "{title:?}");
    // The command's own limit is another: 32,768 characters, refused in words.
    let words = w.state_mut().execute("shape.alt_text", &json!({"id": id, "title": "a".repeat(32_769)})).unwrap_err().to_string();
    assert!(words.contains("title") && words.contains("32768"), "{words}");
}

/// A description of 300 characters that a file holds is shown cut at 255, with its length, in a box that takes no typing; it is saved whole, whatever else is edited.
#[test]
fn a_long_text_from_a_file_is_shown_cut_with_its_length_in_a_box_that_takes_no_typing() {
    let (mut w, id) = window_with_a_shape();
    let long = "d".repeat(300);
    set_alt(&mut w, id, json!({"description": long}));
    w.rest();
    assert!(!is_enabled(&w, DESCRIPTION), "the box is read-only");
    assert!(is_enabled(&w, TITLE), "the title is not");
    assert_eq!(w.state().pane_field_text(DESCRIPTION).map(|t| t.chars().count()), Some(255), "the first 255 characters");
    let state = section(&w);
    let note = state["description"]["readOnly"].as_str().unwrap();
    assert!(note.contains("300") && note.contains("255"), "the words say how long the text is: {note}");
    type_in(&mut w, TITLE, "Pump");
    assert_eq!(alt_of(&mut w, id)["description_length"], 300, "the title's edit leaves the 300 characters whole");
}

/// A control character cannot be held by the file: it is refused in words, and nothing changes.
#[test]
fn a_control_character_is_refused_in_words_and_changes_nothing() {
    let (mut w, id) = window_with_a_shape();
    set_alt(&mut w, id, json!({"title": "Pump"}));
    let steps = undo_steps(&w);
    click_on(&mut w, TITLE);
    key(&mut w, Key::A, ctrl());
    w.input_mut().events.push(Event::Paste("a\u{1}b".to_owned()));
    w.step();
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert_eq!(alt_of(&mut w, id)["title"], "Pump", "nothing was written");
    assert_eq!(undo_steps(&w), steps);
    let words = w.state().pane_field_refusal(TITLE).expect("the box says why");
    assert!(words.contains("title") && words.contains("control characters"), "{words}");
}

/// The inherited text is shown as ghost text in the empty box; typing replaces it in the shape's own row only, and Use master's text goes back to inheriting.
#[test]
fn an_inherited_text_is_ghost_text_and_use_masters_text_goes_back_to_inheriting() {
    let (mut w, id) = instance_window();
    let state = section(&w);
    assert_eq!((state["title"]["shown"].as_str(), state["title"]["hint"].as_str()), (Some(""), Some("Valve")), "{state}");
    assert_eq!((state["description"]["shown"].as_str(), state["description"]["hint"].as_str()), (Some(""), Some("Handle")));
    assert!(state["title"]["note"].as_str().is_some_and(|n| n.contains("master")), "the box says where the text comes from: {state}");
    assert_eq!(w.state().pane_field_text(TITLE), Some(""), "the box itself is empty");
    assert!(
        w.state().element("pane.alttext.title.inherit").is_none_or(|e| !e.enabled),
        "an instance with no row of its own has nothing to go back to"
    );
    // Typing writes the instance's own row; the master keeps its text.
    type_in(&mut w, TITLE, "Valve V-9");
    let read = alt_of(&mut w, id);
    assert_eq!((read["title"].as_str(), read["title_from"].as_str()), (Some("Valve V-9"), Some("own")));
    let master = w.state_mut().execute("master.alt_text", &json!({"id": 7})).unwrap();
    assert_eq!(master["title"], "Valve", "the master is not written");
    assert_eq!(section(&w)["title"]["shown"], "Valve V-9");
    // Use master's text: the own row goes, one undo step, and the ghost is back.
    let steps = undo_steps(&w);
    click_on(&mut w, "pane.alttext.title.inherit");
    let read = alt_of(&mut w, id);
    assert_eq!((read["title"].as_str(), read["title_from"].as_str(), read["title_master"].as_u64()), (Some("Valve"), Some("master"), Some(7)));
    assert_eq!(undo_steps(&w), steps + 1, "one undo step");
    assert_eq!(section(&w)["title"]["hint"], "Valve");
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(alt_of(&mut w, id)["title"], "Valve V-9");
    // A title cleared in the box is an empty own text: the master's is hidden, and the way back is there.
    click_on(&mut w, TITLE);
    key(&mut w, Key::A, ctrl());
    key(&mut w, Key::Backspace, Modifiers::NONE);
    key(&mut w, Key::Enter, Modifiers::NONE);
    let read = alt_of(&mut w, id);
    assert_eq!((read["title"].as_str(), read["title_from"].as_str()), (Some(""), Some("own")));
    assert!(section(&w)["title"]["hint"].is_null(), "no ghost over a text that was cleared on purpose");
    assert!(is_enabled(&w, "pane.alttext.title.inherit"));
    click_on(&mut w, "pane.alttext.title.inherit");
    assert_eq!(alt_of(&mut w, id)["title_from"], "master");
}

/// R24 (the brief of over the specification's greyed boxes): with several shapes selected a box shows "(multiple values)" where they differ, shows the text where
/// they agree, and an edit writes all of them in one undo step.
#[test]
fn several_shapes_show_multiple_values_where_they_differ_and_an_edit_writes_all_in_one_undo_step() {
    let (mut w, ids) = window_with_two_shapes();
    set_alt(&mut w, ids[0], json!({"title": "A", "description": "Same"}));
    set_alt(&mut w, ids[1], json!({"title": "B", "description": "Same"}));
    w.rest();
    let state = section(&w);
    assert_eq!(state["shapes"], 2);
    assert_eq!((state["title"]["mixed"].as_bool(), state["title"]["hint"].as_str()), (Some(true), Some("(multiple values)")), "{state}");
    assert_eq!(w.state().pane_field_text(TITLE), Some(""));
    assert_eq!((state["description"]["mixed"].as_bool(), state["description"]["shown"].as_str()), (Some(false), Some("Same")));
    assert!(is_enabled(&w, TITLE), "several shapes can be edited at once");
    let steps = undo_steps(&w);
    type_in(&mut w, TITLE, "Both");
    for id in &ids {
        assert_eq!(alt_of(&mut w, *id)["title"], "Both", "shape {id}");
    }
    assert_eq!(undo_steps(&w), steps + 1, "one undo step for both shapes");
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!((alt_of(&mut w, ids[0])["title"].clone(), alt_of(&mut w, ids[1])["title"].clone()), (json!("A"), json!("B")));
    // Clear Alt Text empties both texts of every shape, one step.
    let steps = undo_steps(&w);
    click_on(&mut w, "pane.alttext.clear");
    for id in &ids {
        let read = alt_of(&mut w, *id);
        assert_eq!((read["title"].as_str(), read["description"].as_str()), (Some(""), Some("")), "shape {id}");
    }
    assert_eq!(undo_steps(&w), steps + 1);
}

/// More shapes than a batch holds: the boxes are quiet and say how many.
#[test]
fn more_shapes_than_a_batch_holds_make_the_boxes_quiet_in_words() {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 3.0, 6.0);
    let page = page_id(&w);
    // Copy every shape of the page until there are more than 256: 1, 2, 4, ... 512.
    for _ in 0..9 {
        let ids = shape_ids(&w);
        w.state_mut().execute("shape.duplicate", &json!({"ids": ids, "page": page})).unwrap();
    }
    w.rest();
    assert!(shape_ids(&w).len() > 256, "{} shapes", shape_ids(&w).len());
    run_command(&mut w, "edit.selectAll", json!({}));
    run_command(&mut w, "format.options", json!({"tab": "Alt Text"}));
    assert!(!is_enabled(&w, TITLE), "too many shapes for one undo step");
    let note = section(&w)["blocked"].as_str().unwrap().to_owned();
    assert!(note.contains("256") && note.contains("select fewer"), "{note}");
}

/// A text a formula makes is shown as the formula, and typing replaces it with the constant; a formula a GUARD protects takes no typing and says so.
#[test]
fn a_formula_text_shows_the_formula_and_a_guarded_one_takes_no_typing() {
    let (mut w, id) = window_with_a_shape();
    set_alt(&mut w, id, json!({"title": "Pump", "description": "Long note"}));
    // The title: a formula `Prop.Name` with a result; the description: a GUARD.
    let page = page_id(&w);
    let formula = |w: &mut Window, row: &str, formula: &str| {
        run_command(w, "sheet.set_cell", json!({"id": id, "page": page, "cell": format!("User.{row}.Value"), "formula": formula}));
    };
    formula(&mut w, "visAltText", "\"Pump\"&\" 7\"");
    formula(&mut w, "visAltDescription", "GUARD(\"Long note\")");
    w.rest();
    let state = section(&w);
    assert_eq!(state["title"]["shown"], "\"Pump\"&\" 7\"", "the box shows the formula's text, as the reference application's does");
    assert!(state["title"]["readOnly"].is_null());
    assert_eq!(state["description"]["shown"], "GUARD(\"Long note\")");
    let why = state["description"]["readOnly"].as_str().unwrap();
    assert!(why.contains("GUARD") && why.contains("protected"), "{why}");
    assert!(!is_enabled(&w, DESCRIPTION) && is_enabled(&w, TITLE));
    // Typing into the formula's box replaces it with a constant.
    type_in(&mut w, TITLE, "Pump 8");
    let read = alt_of(&mut w, id);
    assert_eq!((read["title"].as_str(), read["title_formula"].is_null()), (Some("Pump 8"), true), "{read}");
    // A shape's protected text is refused in words by the command too.
    let words = w.state_mut().execute("shape.alt_text", &json!({"id": id, "description": "x"})).unwrap_err().to_string();
    assert!(words.contains("GUARD") && words.contains("protected"), "{words}");
}

/// A shape on a locked layer is not selected and not edited. A layer locked while a text is being typed takes the shape out of the selection, the box goes quiet, and the
/// text is refused in the engine's words when the edit ends (said in the status bar: the box is not there to say it under); a locked shape the window still holds (a program selected it) has quiet boxes
/// that give the same words, and the text is still shown.
#[test]
fn a_shape_on_a_locked_layer_has_quiet_boxes_and_a_refusal_in_the_engines_words() {
    let (mut w, id) = window_with_a_shape();
    set_alt(&mut w, id, json!({"title": "Pump"}));
    run_command(&mut w, "layer.add", json!({"name": "Held"}));
    run_command(&mut w, "layer.assign", json!({"ids": [id], "layers": ["Held"]}));
    // Type first, then the layer is locked: the shape is no longer selected, and the text typed is refused when a click elsewhere ends the edit.
    click_on(&mut w, TITLE);
    key(&mut w, Key::A, ctrl());
    type_text(&mut w, "Changed");
    run_command(&mut w, "layer.set", json!({"layer": "Held", "lock": true}));
    assert!(w.state().window_state()["selection"].as_array().unwrap().is_empty(), "a shape on a locked layer is not selected");
    assert!(!is_enabled(&w, TITLE), "the box went quiet");
    click_on(&mut w, "pane.format_shape.title");
    assert_eq!(alt_of_locked(&mut w, id), "Pump", "nothing was written");
    let words = w.state().notice().expect("the status bar says why").to_owned();
    assert!(words.contains("locked") && words.contains("Held"), "{words}");
    // A program selects the locked shape: the boxes are quiet, with the engine's words, and the text is shown.
    w.state_mut().select(&[id]);
    w.rest();
    assert!(!is_enabled(&w, TITLE) && !is_enabled(&w, DESCRIPTION));
    let state = section(&w);
    assert!(state["blocked"].as_str().is_some_and(|n| n.contains("locked") && n.contains("Held")), "{state}");
    assert_eq!(w.state().pane_field_text(TITLE), Some("Pump"));
    assert!(w.get_by_role_and_label(Role::Button, "Clear Alt Text").accesskit_node().is_disabled(), "Clear Alt Text is quiet too");
    let words = w.state_mut().execute("shape.alt_text", &json!({"id": id, "title": "x"})).unwrap_err().to_string();
    assert!(words.contains("locked"), "{words}");
    run_command(&mut w, "layer.set", json!({"layer": "Held", "lock": false}));
    assert!(is_enabled(&w, TITLE), "unlocked, the boxes work again");
}

/// The shape's title, read from the document and not through the command (a command on a locked layer is refused even to read).
fn alt_of_locked(w: &mut Window, id: u32) -> String {
    let doc = w.state().session().document().unwrap();
    let found = doc.find_shape(diacraft_engine::doc::Owner::Page(1), id).unwrap();
    alt_text::read_in(doc, &diacraft_engine::doc::SheetRef::Shape(found)).title.text.to_owned()
}

/// With nothing selected, or a selection on another page, the boxes are quiet and say to select a shape.
#[test]
fn with_nothing_selected_the_boxes_are_quiet_and_say_what_to_do() {
    let (mut w, _) = window_with_a_shape();
    w.state_mut().select(&[]);
    w.rest();
    assert!(!is_enabled(&w, TITLE) && !is_enabled(&w, DESCRIPTION));
    assert!(labels(&w).iter().any(|l| l.contains("Select a shape")), "{:?}", labels(&w));
    assert!(w.state_mut().execute("format.options", &json!({"tab": "Alt Text"})).is_err(), "the command needs a selection, as the other tabs do");
}

// ------------------------------------------------------------------------------------------------------------------- the shape's menu

/// A right click on a shape: the menu has Edit Alt Text, which opens the pane at the section with the Title box ready for typing.
#[test]
fn the_shape_menu_has_edit_alt_text_which_opens_the_pane_at_the_section_with_the_title_ready() {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 4.0, 5.0);
    assert!(w.state().window_state()["panes"].as_array().unwrap().is_empty());
    // With nothing selected the row is there and quiet, and says why.
    w.state_mut().select(&[]);
    w.rest();
    let on_page = at(&w, 7.0, 3.0);
    right_click(&mut w, on_page);
    assert!(
        w.state().element("menu.CanvasAltText").is_some_and(|e| !e.enabled),
        "Edit Alt Text is quiet with nothing selected: {:?} {:?}",
        w.state().window_state()["selection"],
        crate::support::elements_named(&w, "menu.")
    );
    key(&mut w, Key::Escape, Modifiers::NONE);
    let id = shape_ids(&w)[0];
    run_command(&mut w, "edit.select", json!({"ids": [id]}));
    let on_shape = at(&w, 4.0, 5.0);
    right_click(&mut w, on_shape);
    assert!(w.state().element("menu.CanvasAltText").is_some_and(|e| e.enabled), "Edit Alt Text is a row of the shape's menu");
    click_on(&mut w, "menu.CanvasAltText");
    assert_eq!(w.state().window_state()["panes"], json!([["format_shape"]]), "the Format Shape pane opened");
    assert!(!section(&w).is_null(), "at the Alt Text section");
    assert!(focused(&w, Role::TextInput, "Alt text title"), "and the Title box has the keyboard");
    type_text(&mut w, "Pump");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert_eq!(alt_of(&mut w, id)["title"], "Pump");
    // The window command does the same for a shape named, on its page.
    run_command(&mut w, "format.options", json!({"tab": "Effects"}));
    run_command(&mut w, "window.altText", json!({"id": id}));
    assert!(!section(&w).is_null());
    assert!(focused(&w, Role::TextInput, "Alt text title"));
}

/// The command refuses where there is nothing to edit, in words.
#[test]
fn edit_alt_text_with_nothing_selected_is_refused_in_words() {
    let mut w = edit_window([1280.0, 900.0]);
    let words = w.state_mut().execute("window.altText", &json!({})).unwrap_err().to_string();
    assert!(words.contains("nothing is selected"), "{words}");
    let words = w.state_mut().execute("window.altText", &json!({"id": 999_999})).unwrap_err().to_string();
    assert!(words.contains("999999"), "{words}");
}

// ----------------------------------------------------------------------------------------------------------------- Page Setup's Alt Text tab

/// Page Setup has an Alt Text tab with a one-line Title and a several-line Description for the page on view; nothing is sent until OK, and OK with other tabs' changes is one step.
#[test]
fn page_setup_alt_text_tab_works_on_a_draft_and_ok_sends_one_undo_step() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "window.pageSetup", json!({"tab": "Alt Text"}));
    assert_eq!(w.state().window_state()["pageSetup"], "Alt Text");
    assert!(w.state().element("pagesetup.tab.Alt Text").is_some_and(|e| e.enabled), "the tab works now");
    w.get_by_role_and_label(Role::TextInput, "Page alt text title");
    w.get_by_role_and_label(Role::MultilineTextInput, "Page alt text description");
    let steps = undo_steps(&w);
    click_on(&mut w, PAGE_TITLE);
    type_text(&mut w, "Plant overview");
    click_on(&mut w, PAGE_DESCRIPTION);
    type_text(&mut w, "Line one");
    key(&mut w, Key::Enter, Modifiers::NONE);
    type_text(&mut w, "Line two");
    assert_eq!(undo_steps(&w), steps, "nothing is sent before OK");
    assert!(w.state().is_modal_open(), "Enter in the Description is a line break, not OK");
    // Cancel sends nothing.
    press(&mut w, "pagesetup.cancel");
    assert!(w.state().window_state()["pageSetup"].is_null());
    assert_eq!(page_alt(&mut w)["title"], "");
    // The dialog again: the typed text and a new name on another tab go on OK, as one undo step.
    run_command(&mut w, "window.pageSetup", json!({"tab": "Alt Text"}));
    click_on(&mut w, PAGE_TITLE);
    type_text(&mut w, "Plant overview");
    click_on(&mut w, PAGE_DESCRIPTION);
    type_text(&mut w, "Line one");
    key(&mut w, Key::Enter, Modifiers::NONE);
    type_text(&mut w, "Line two");
    click_on(&mut w, "pagesetup.tab.Page Properties");
    click_on(&mut w, "pagesetup.name");
    key(&mut w, Key::A, ctrl());
    type_text(&mut w, "Plant");
    press(&mut w, "pagesetup.ok");
    assert!(w.state().window_state()["pageSetup"].is_null(), "OK closes the dialog");
    let read = page_alt(&mut w);
    assert_eq!((read["title"].as_str(), read["description"].as_str()), (Some("Plant overview"), Some("Line one\nLine two")), "{read}");
    assert_eq!(undo_steps(&w), steps + 1, "the name and the alt text are one undo step");
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(page_alt(&mut w)["title"], "");
    // Only the alt text changed: one `page.alt_text`, one step.
    run_command(&mut w, "window.pageSetup", json!({"tab": "Alt Text"}));
    click_on(&mut w, PAGE_TITLE);
    type_text(&mut w, "Plant overview");
    let steps = undo_steps(&w);
    press(&mut w, "pagesetup.ok");
    assert_eq!(undo_steps(&w), steps + 1);
    // OK with nothing changed sends nothing.
    run_command(&mut w, "window.pageSetup", json!({"tab": "Alt Text"}));
    let shown = w.get_by_role_and_label(Role::TextInput, "Page alt text title").accesskit_node().value().map(|v| v.to_string());
    assert_eq!(shown.as_deref(), Some("Plant overview"), "the dialog shows what the page holds");
    let steps = undo_steps(&w);
    press(&mut w, "pagesetup.ok");
    assert_eq!(undo_steps(&w), steps);
}

/// The tab shows the page's text as it is, takes 255 characters and refuses a text a file made longer than that.
#[test]
fn page_setup_alt_text_tab_limits_the_boxes_and_keeps_a_long_text_a_file_made() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "page.alt_text", json!({"title": "Existing", "description": "d".repeat(300)}));
    run_command(&mut w, "window.pageSetup", json!({"tab": "Alt Text"}));
    let title = w.get_by_role_and_label(Role::TextInput, "Page alt text title").accesskit_node().value().map(|v| v.to_string());
    assert_eq!(title.as_deref(), Some("Existing"), "the box shows the page's title");
    assert!(
        w.get_by_role_and_label(Role::MultilineTextInput, "Page alt text description").accesskit_node().is_disabled(),
        "300 characters: read-only"
    );
    assert!(labels(&w).iter().any(|l| l.contains("300") && l.contains("255")), "the words say how long it is: {:?}", labels(&w));
    click_on(&mut w, PAGE_TITLE);
    key(&mut w, Key::A, ctrl());
    w.input_mut().events.push(Event::Paste("t".repeat(10_000)));
    w.step();
    w.rest();
    press(&mut w, "pagesetup.ok");
    let read = page_alt(&mut w);
    assert_eq!(
        (read["title_length"].as_u64(), read["description_length"].as_u64()),
        (Some(255), Some(300)),
        "the title is cut at 255 and the description kept whole"
    );
}

// -------------------------------------------------------------------------------------------------------------- the Accessibility pane

/// A drawing of two pages: page 1 holds a rectangle and a diamond, page 2 an ellipse; none has alt text. Returns the window and the ids of the three shapes.
fn unchecked_window() -> (Window, Vec<u32>, u32) {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 3.0, 6.0);
    drag_tile_to(&mut w, "Diamond", 7.0, 6.0);
    let first = page_id(&w);
    run_command(&mut w, "page.add", json!({}));
    let second = w.state().session().document().unwrap().pages.get(1).unwrap().id;
    run_command(&mut w, "page.select", json!({"id": second}));
    drag_tile_to(&mut w, "Ellipse", 5.0, 3.0);
    let ellipse = w.state().session().document().unwrap().pages.get(1).unwrap().shapes.iter().map(|s| s.id).next().unwrap();
    run_command(&mut w, "page.select", json!({"id": first}));
    let mut ids = shape_ids(&w);
    ids.push(ellipse);
    (w, ids, second)
}

fn accessibility(w: &Window) -> Value {
    w.state().window_state()["accessibility"].clone()
}

/// Review, Accessibility, Check Accessibility opens the Accessibility pane, which lists the findings under Errors and Warnings, each with its count.
#[test]
fn check_accessibility_opens_a_pane_that_lists_the_findings_by_severity() {
    let (mut w, ids, _) = unchecked_window();
    run_command(&mut w, "window.tab", json!({"tab": "Review"}));
    assert!(w.state().element("ribbon.Review.AccessibilityChecker").is_some_and(|e| e.enabled), "the button works now");
    press(&mut w, "ribbon.Review.AccessibilityChecker");
    w.rest();
    assert_eq!(w.state().window_state()["panes"], json!([["accessibility"]]));
    let state = accessibility(&w);
    // Two pages with the default names (warnings) and no alt text on either page or on the three shapes (errors).
    assert_eq!((state["errors"].as_u64(), state["warnings"].as_u64()), (Some(5), Some(2)), "{state}");
    assert!(w.state().element("accessibility.group.errors").is_some() && w.state().element("accessibility.group.warnings").is_some());
    assert!(w.state().element("accessibility.recheck").is_some_and(|e| e.enabled));
    let findings = state["findings"].as_array().unwrap();
    let shapes: Vec<u64> = findings.iter().filter_map(|f| f["shape"].as_u64()).collect();
    for id in &ids {
        assert!(shapes.contains(&u64::from(*id)), "shape {id} has a finding: {state}");
    }
    assert!(labels(&w).iter().any(|l| l.contains("5 errors") && l.contains("2 warnings")), "the summary says how many: {:?}", labels(&w));
    assert_eq!(w.state().ui_panics(), 0);
}

/// Selecting a finding goes to its page and selects the shape, and only that.
#[test]
fn a_finding_goes_to_its_page_and_selects_its_shape() {
    let (mut w, ids, second) = unchecked_window();
    run_command(&mut w, "window.pane", json!({"pane": "accessibility", "shown": true}));
    w.rest();
    let state = accessibility(&w);
    // (A shape's ID is its page's own: the ellipse is shape 1 of page 2, as the rectangle is shape 1 of page 1.)
    let index = state["findings"]
        .as_array()
        .unwrap()
        .iter()
        .position(|f| f["shape"] == json!(ids[2]) && f["page"] == json!(second))
        .expect("the ellipse has a finding");
    assert_eq!(w.state_mut().current_page(), Some(page_id(&w)), "page 1 is on view");
    click_on(&mut w, &format!("accessibility.item.{index}"));
    assert_eq!(w.state_mut().current_page(), Some(second), "page 2 is shown");
    assert_eq!(w.state().window_state()["selection"], json!([ids[2]]), "the ellipse alone is selected");
    assert_eq!(accessibility(&w)["selected"], json!(index));
    // The same by command, and a finding that is not in the list is refused in words.
    let first = page_id(&w);
    run_command(&mut w, "window.showFinding", json!({"rule": "alt_text_missing_on_shape", "page": first, "shape": ids[0]}));
    assert_eq!(w.state().window_state()["selection"], json!([ids[0]]));
    let words = w
        .state_mut()
        .execute("window.showFinding", &json!({"rule": "alt_text_missing_on_shape", "page": first, "shape": 999_999}))
        .unwrap_err()
        .to_string();
    assert!(words.contains("no finding"), "{words}");
}

/// The pane checks again after each edit (and once for each, not each frame), and Recheck checks at once.
#[test]
fn the_pane_checks_again_after_an_edit_once_for_each_and_recheck_checks_at_once() {
    let (mut w, ids, _) = unchecked_window();
    run_command(&mut w, "window.pane", json!({"pane": "accessibility", "shown": true}));
    w.rest();
    let checks = accessibility(&w)["checks"].as_u64().unwrap();
    assert!(checks >= 1);
    for _ in 0..10 {
        w.step();
    }
    assert_eq!(accessibility(&w)["checks"].as_u64(), Some(checks), "frames of a drawing that did not change check nothing");
    set_alt(&mut w, ids[0], json!({"title": "Rectangle"}));
    w.rest();
    let after = accessibility(&w);
    assert_eq!(after["checks"].as_u64(), Some(checks + 1), "one check for the edit");
    assert_eq!(after["errors"], 4, "the finding for the rectangle is gone: {after}");
    click_on(&mut w, "accessibility.recheck");
    assert_eq!(accessibility(&w)["checks"].as_u64(), Some(checks + 2), "Recheck checks");
}

/// The fix a finding offers opens the place that puts it right: Add Alt Text the Format Shape pane at the Alt Text section (Page Setup's tab for a page).
#[test]
fn a_finding_offers_its_fix_which_opens_the_alt_text_section_or_the_page_setup_tab() {
    let (mut w, ids, _) = unchecked_window();
    run_command(&mut w, "window.pane", json!({"pane": "accessibility", "shown": true}));
    w.rest();
    let state = accessibility(&w);
    let findings = state["findings"].as_array().unwrap();
    let first = page_id(&w);
    let on_shape =
        findings.iter().position(|f| f["shape"] == json!(ids[1]) && f["page"] == json!(first) && f["rule"] == "alt_text_missing_on_shape").unwrap();
    click_on(&mut w, &format!("accessibility.item.{on_shape}"));
    assert!(w.state().element("accessibility.fix").is_some_and(|e| e.enabled), "the selected finding offers its fix");
    assert!(labels(&w).iter().any(|l| l.contains("Add Alt Text") || l.contains("alt text")), "{:?}", labels(&w));
    click_on(&mut w, "accessibility.fix");
    assert!(!section(&w).is_null(), "the Alt Text section is open");
    assert_eq!(w.state().window_state()["selection"], json!([ids[1]]));
    assert!(focused(&w, Role::TextInput, "Alt text title"), "with the Title box ready");
    type_text(&mut w, "Diamond");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert_eq!(alt_of(&mut w, ids[1])["title"], "Diamond");
    // A finding of a page opens Page Setup on its Alt Text tab.
    let on_page = accessibility(&w)["findings"]
        .as_array()
        .unwrap()
        .iter()
        .position(|f| f["shape"].is_null() && f["rule"] == "alt_text_missing_on_page")
        .unwrap();
    click_on(&mut w, &format!("accessibility.item.{on_page}"));
    click_on(&mut w, "accessibility.fix");
    assert_eq!(w.state().window_state()["pageSetup"], "Alt Text");
    // A page with a default name offers Rename Page: Page Setup on Page Properties.
    run_command(&mut w, "window.pageSetup", json!({"close": true}));
    let renamed = accessibility(&w)["findings"].as_array().unwrap().iter().position(|f| f["rule"] == "page_has_default_name").unwrap();
    click_on(&mut w, &format!("accessibility.item.{renamed}"));
    click_on(&mut w, "accessibility.fix");
    assert_eq!(w.state().window_state()["pageSetup"], "Page Properties");
}

/// A drawing with nothing to say about says so, and the commands that open the pane refuse with no drawing.
#[test]
fn a_drawing_with_alt_text_on_everything_has_no_findings_and_says_so() {
    let (mut w, ids, _) = unchecked_window();
    let pages: Vec<u32> = w.state().session().document().unwrap().pages.iter().map(|p| p.id).collect();
    // The shapes of each page by their own IDs (the rectangle and the ellipse are both shape 1, of two pages).
    let shapes: Vec<(u32, u32)> =
        w.state().session().document().unwrap().pages.iter().flat_map(|p| p.shapes.iter().map(move |s| (p.id, s.id))).collect();
    assert_eq!((shapes.len(), ids.len()), (3, 3));
    for (n, (page, id)) in shapes.iter().enumerate() {
        run_command(&mut w, "shape.alt_text", json!({"id": id, "page": page, "title": format!("Shape {n}")}));
    }
    for (n, page) in pages.iter().enumerate() {
        run_command(&mut w, "page.alt_text", json!({"page": page, "title": format!("Overview of {n}")}));
        run_command(&mut w, "page.rename", json!({"id": page, "name": format!("Plant {n}")}));
    }
    run_command(&mut w, "window.pane", json!({"pane": "accessibility", "shown": true}));
    w.rest();
    let state = accessibility(&w);
    assert_eq!((state["errors"].as_u64(), state["warnings"].as_u64()), (Some(0), Some(0)), "{state}");
    assert!(labels(&w).iter().any(|l| l.contains("No accessibility problems")), "{:?}", labels(&w));
}

// ------------------------------------------------------------------------------------------------------------ the window's own accessibility

/// The keyboard reaches the boxes in the order they are drawn: Title, Description, then the buttons; Shift+Tab goes back.
#[test]
fn tab_moves_through_the_alt_text_boxes_in_order_and_shift_tab_goes_back() {
    let (mut w, id) = window_with_a_shape();
    set_alt(&mut w, id, json!({"title": "Pump"}));
    click_on(&mut w, TITLE);
    assert!(focused(&w, Role::TextInput, "Alt text title"));
    key(&mut w, Key::Tab, Modifiers::NONE);
    assert!(focused(&w, Role::MultilineTextInput, "Alt text description"), "Tab: the Description");
    key(&mut w, Key::Tab, Modifiers::NONE);
    assert!(focused(&w, Role::Button, "Clear Alt Text"), "Tab: Clear Alt Text");
    key(&mut w, Key::Tab, Modifiers::SHIFT);
    assert!(focused(&w, Role::MultilineTextInput, "Alt text description"), "Shift+Tab: back to the Description");
    key(&mut w, Key::Tab, Modifiers::SHIFT);
    assert!(focused(&w, Role::TextInput, "Alt text title"), "and the Title");
}

/// With nothing selected the boxes are disabled for a screen reader too.
#[test]
fn the_boxes_are_disabled_in_the_accesskit_tree_when_there_is_nothing_to_edit() {
    let (mut w, _) = window_with_a_shape();
    w.state_mut().select(&[]);
    w.rest();
    assert!(w.get_by_role_and_label(Role::TextInput, "Alt text title").accesskit_node().is_disabled());
    assert!(w.get_by_role_and_label(Role::MultilineTextInput, "Alt text description").accesskit_node().is_disabled());
    assert!(w.get_by_role_and_label(Role::Button, "Clear Alt Text").accesskit_node().is_disabled());
}

/// The panes' narrow and wide windows keep the boxes inside the dock and the text out of the way of its neighbours.
#[test]
fn the_alt_text_section_fits_the_dock_in_a_narrow_window_and_a_wide_one() {
    for size in [[640.0, 700.0], [1280.0, 900.0], [1920.0, 1000.0]] {
        let mut w = edit_window(size);
        let page = page_id(&w);
        run_command(&mut w, "shape.drop", json!({"master": "rectangle", "page": page, "x": 3.0, "y": 3.0}));
        let id = shape_ids(&w)[0];
        run_command(&mut w, "edit.select", json!({"ids": [id]}));
        run_command(&mut w, "format.options", json!({"tab": "Alt Text"}));
        let dock = rect_of(&w, "panes");
        for name in [TITLE, DESCRIPTION, "pane.alttext.clear"] {
            let r = rect_of(&w, name);
            assert!(dock.contains_rect(r.shrink(0.5)), "{name} {r:?} is inside the dock {dock:?} at {size:?}");
        }
        let (a, b) = (rect_of(&w, TITLE), rect_of(&w, DESCRIPTION));
        assert!(a.max.y <= b.min.y, "the boxes do not overlap at {size:?}");
        assert_eq!(w.state().ui_panics(), 0);
    }
}

/// The Accessibility pane and Page Setup's Alt Text tab keep their words inside the dock and the dialog, one line of the list under the other, in a narrow window and a wide one.
#[test]
fn the_accessibility_pane_and_the_page_setup_tab_fit_a_narrow_window_and_a_wide_one() {
    for size in [[640.0, 700.0], [1280.0, 900.0], [1920.0, 1000.0]] {
        let mut w = edit_window(size);
        let page = page_id(&w);
        for (x, y) in [(3.0, 3.0), (5.0, 3.0), (7.0, 3.0)] {
            run_command(&mut w, "shape.drop", json!({"master": "rectangle", "page": page, "x": x, "y": y}));
        }
        run_command(&mut w, "window.pane", json!({"pane": "accessibility", "shown": true}));
        let ids = shape_ids(&w);
        let index = accessibility(&w)["findings"].as_array().unwrap().iter().position(|f| f["shape"] == json!(ids[0])).unwrap();
        click_on(&mut w, &format!("accessibility.item.{index}"));
        let dock = rect_of(&w, "panes");
        let mut items: Vec<Rect> = w.state().elements().iter().filter(|e| e.name.starts_with("accessibility.item.")).map(|e| e.rect).collect();
        items.sort_by(|a, b| a.min.y.total_cmp(&b.min.y));
        assert!(items.len() >= 4, "the three shapes and the page have findings at {size:?}");
        for pair in items.windows(2) {
            assert!(pair[0].max.y <= pair[1].min.y + 0.5, "the items do not overlap at {size:?}: {pair:?}");
        }
        for name in ["accessibility.recheck", "accessibility.group.errors", "accessibility.fix"] {
            let r = rect_of(&w, name);
            assert!(r.min.x >= dock.min.x - 0.5 && r.max.x <= dock.max.x + 0.5, "{name} {r:?} is inside the dock {dock:?} at {size:?}");
        }
        for r in &items {
            assert!(r.min.x >= dock.min.x - 0.5 && r.max.x <= dock.max.x + 0.5, "an item {r:?} is inside the dock {dock:?} at {size:?}");
        }
        run_command(&mut w, "window.pageSetup", json!({"tab": "Alt Text"}));
        let content = rect_of(&w, "pagesetup.content");
        for name in [PAGE_TITLE, PAGE_DESCRIPTION] {
            let r = rect_of(&w, name);
            assert!(
                r.min.x >= content.min.x - 0.5 && r.max.x <= content.max.x + 0.5,
                "{name} {r:?} is inside the dialog's content {content:?} at {size:?}"
            );
        }
        let (a, b) = (rect_of(&w, PAGE_TITLE), rect_of(&w, PAGE_DESCRIPTION));
        assert!(a.max.y <= b.min.y, "the boxes do not overlap at {size:?}");
        let window = Rect::from_min_size(Pos2::ZERO, egui::vec2(size[0], size[1]));
        for name in ["pagesetup.ok", "pagesetup.cancel"] {
            assert!(window.contains_rect(rect_of(&w, name)), "{name} is in the window at {size:?}");
        }
        assert_eq!(w.state().ui_panics(), 0);
    }
}
