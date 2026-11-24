//! The Insert tab's Pictures, Link, Field and Symbol. The pointer and the keyboard are given to egui as a person's would be;
//! what is checked is the drawing the commands changed, the state of the dialogs and what the window did.

use std::path::PathBuf;
use std::sync::Arc;

use diacraft_ui_egui::FilePicker;
use egui::{Key, Modifiers};
use serde_json::{Value as Json, json};

use crate::editing::{click_with, edit_window, edit_window_with, key, shape_ids, type_text};
use crate::support::{Rest, TempDir, Window, press, run_command};

/// A PNG of `w` by `h` pixels in `dir`.
fn png_file(dir: &TempDir, name: &str, w: u32, h: u32) -> PathBuf {
    let path = dir.file(name);
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&vec![160u8; (w * h * 4) as usize]).unwrap();
    }
    std::fs::write(&path, bytes).unwrap();
    path
}

fn state(w: &Window) -> Json {
    w.state().window_state()
}

fn enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

/// A window on a new drawing with one rectangle at (3, 6) that holds `text`, selected, on the Insert tab. → the window and the shape.
fn with_shape(text: &str) -> (Window, u32) {
    let mut w = edit_window([1400.0, 900.0]);
    let id =
        w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 6.0})).unwrap()["id"].as_u64().unwrap() as u32;
    if !text.is_empty() {
        w.state_mut().execute("shape.set_text", &json!({"id": id, "text": text})).unwrap();
    }
    w.state_mut().execute("edit.select", &json!({"ids": [id]})).unwrap();
    run_command(&mut w, "window.tab", json!({"tab": "Insert"}));
    (w, id)
}

fn links_of(w: &mut Window, id: u32) -> Vec<Json> {
    w.state_mut().execute("hyperlink.list", &json!({"id": id})).unwrap()["links"].as_array().unwrap().clone()
}

fn shown_text(w: &Window, id: u32) -> (String, Vec<String>) {
    let v = diacraft_engine::draw::text_view(w.state().session().document().unwrap(), 1, id).unwrap();
    (v.chars, v.fields.iter().map(|f| f.text.clone()).collect())
}

#[test]
fn link_field_and_symbol_need_a_shape_and_pictures_need_only_a_drawing() {
    let mut w = edit_window([1400.0, 900.0]);
    run_command(&mut w, "window.tab", json!({"tab": "Insert"}));
    assert!(!enabled(&w, "ribbon.Insert.HyperlinkInsert"), "Link is greyed out with nothing selected");
    assert!(!enabled(&w, "ribbon.Insert.FieldInsert"), "so is the Field command");
    assert!(!enabled(&w, "ribbon.Insert.SymbolInsertGallery"), "and Symbol");
    assert!(enabled(&w, "ribbon.Insert.FlyoutAnchorInsertPictures"), "Pictures is live with nothing selected");
    let id =
        w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 6.0})).unwrap()["id"].as_u64().unwrap() as u32;
    w.state_mut().execute("edit.select", &json!({"ids": [id]})).unwrap();
    w.rest();
    assert!(
        enabled(&w, "ribbon.Insert.HyperlinkInsert") && enabled(&w, "ribbon.Insert.FieldInsert") && enabled(&w, "ribbon.Insert.SymbolInsertGallery")
    );
    // Two shapes: a link and a field are for one.
    let second =
        w.state_mut().execute("shape.drop", &json!({"master": "ellipse", "page": 1, "x": 7.0, "y": 6.0})).unwrap()["id"].as_u64().unwrap() as u32;
    w.state_mut().execute("edit.select", &json!({"ids": [id, second]})).unwrap();
    w.rest();
    assert!(!enabled(&w, "ribbon.Insert.HyperlinkInsert") && !enabled(&w, "ribbon.Insert.FieldInsert"));
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- Link ----------------------------------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn the_hyperlinks_dialog_makes_a_link_with_one_hyperlink_set_and_one_undo_step() {
    let (mut w, id) = with_shape("Home");
    let steps = w.state().session().history().unwrap().undo_len();
    press(&mut w, "ribbon.Insert.HyperlinkInsert");
    w.rest();
    assert!(state(&w)["hyperlinks"].is_object(), "the dialog is open: {}", state(&w)["hyperlinks"]);
    assert_eq!(state(&w)["hyperlinks"]["rows"].as_array().unwrap().len(), 1, "a shape with no link starts with one empty row to type in");
    press(&mut w, "links.address");
    type_text(&mut w, "https://example.org/start");
    press(&mut w, "links.description");
    type_text(&mut w, "Start page");
    press(&mut w, "links.ok");
    w.rest();
    assert!(state(&w)["hyperlinks"].is_null(), "OK closes it");
    let links = links_of(&mut w, id);
    assert_eq!(links.len(), 1);
    assert_eq!(
        (links[0]["address"].as_str(), links[0]["description"].as_str(), links[0]["kind"].as_str()),
        (Some("https://example.org/start"), Some("Start page"), Some("web"))
    );
    assert_eq!(w.state().session().history().unwrap().undo_len(), steps + 1, "one step");
    // Opened again it shows the link, New adds a row, Default marks it, Delete takes one away.
    run_command(&mut w, "window.hyperlinks", json!({}));
    assert_eq!(state(&w)["hyperlinks"]["rows"][0]["address"], "https://example.org/start");
    press(&mut w, "links.new");
    press(&mut w, "links.sub_address");
    type_text(&mut w, "Page-1");
    press(&mut w, "links.default");
    assert_eq!(state(&w)["hyperlinks"]["rows"][1]["default"], true, "Default marks the row chosen");
    press(&mut w, "links.ok");
    let links = links_of(&mut w, id);
    assert_eq!(links.len(), 2);
    assert_eq!((links[1]["default"].as_bool(), links[1]["kind"].as_str()), (Some(true), Some("page")), "{links:?}");
    run_command(&mut w, "window.hyperlinks", json!({}));
    press(&mut w, "links.row.0");
    press(&mut w, "links.delete");
    press(&mut w, "links.ok");
    assert_eq!(links_of(&mut w, id).len(), 1, "the first link was taken away");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn cancel_and_escape_send_nothing_and_a_link_with_no_address_is_no_link() {
    let (mut w, id) = with_shape("");
    let revision = w.state().session().revision();
    run_command(&mut w, "window.hyperlinks", json!({}));
    press(&mut w, "links.address");
    type_text(&mut w, "https://example.org/");
    press(&mut w, "links.cancel");
    assert!(state(&w)["hyperlinks"].is_null());
    run_command(&mut w, "window.hyperlinks", json!({}));
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(state(&w)["hyperlinks"].is_null());
    // OK with the empty row sends a list of no link: nothing changes.
    run_command(&mut w, "window.hyperlinks", json!({}));
    press(&mut w, "links.ok");
    assert!(links_of(&mut w, id).is_empty());
    assert_eq!(w.state().session().revision(), revision, "nothing was changed in the drawing");
    // Ctrl+K opens the dialog.
    key(&mut w, Key::K, Modifiers::COMMAND);
    assert!(state(&w)["hyperlinks"].is_object(), "Ctrl+K");
    run_command(&mut w, "window.hyperlinks", json!({"close": true}));
    assert!(state(&w)["hyperlinks"].is_null());
}

#[test]
fn a_ctrl_click_follows_the_link_of_a_shape_when_nothing_is_selected_and_only_selects_otherwise() {
    let mut w = edit_window([1400.0, 900.0]);
    run_command(&mut w, "page.add", json!({"name": "Second"}));
    run_command(&mut w, "page.select", json!({"id": 1}));
    let page_one = w.state().session().document().unwrap().pages.get(0).unwrap().id;
    let a = w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": page_one, "x": 3.0, "y": 6.0})).unwrap()["id"]
        .as_u64()
        .unwrap() as u32;
    let b = w.state_mut().execute("shape.drop", &json!({"master": "ellipse", "page": page_one, "x": 7.0, "y": 6.0})).unwrap()["id"].as_u64().unwrap()
        as u32;
    let c = w.state_mut().execute("shape.drop", &json!({"master": "diamond", "page": page_one, "x": 5.0, "y": 3.0})).unwrap()["id"].as_u64().unwrap()
        as u32;
    w.state_mut().execute("hyperlink.set", &json!({"id": a, "links": [{"sub_address": "Second"}]})).unwrap();
    w.state_mut().execute("hyperlink.set", &json!({"id": b, "links": [{"address": "https://example.org/a b", "description": "Site"}]})).unwrap();
    w.state_mut().execute("hyperlink.set", &json!({"id": c, "links": [{"address": "file:///usr/bin/something"}]})).unwrap();
    w.state_mut().execute("edit.select", &json!({"ids": []})).unwrap();
    w.rest();
    // A web address goes to the system through OpenUrl, spaces written as %20.
    click_with(&mut w, (7.0, 6.0), Modifiers::COMMAND);
    w.rest();
    assert_eq!(state(&w)["openedUrl"], "https://example.org/a%20b");
    assert!(w.state().selection().is_empty(), "a click that follows a link selects nothing");
    // A link of this drawing shows its page.
    click_with(&mut w, (3.0, 6.0), Modifiers::COMMAND);
    w.rest();
    assert_ne!(w.state().session().document().unwrap().pages.get(1).map(|p| p.id), Some(page_one));
    assert!(state(&w)["pageRect"].is_array());
    run_command(&mut w, "page.select", json!({"id": page_one}));
    // A file is not followed: the notice says so in words and nothing is opened.
    let before = state(&w)["openedUrl"].clone();
    click_with(&mut w, (5.0, 3.0), Modifiers::COMMAND);
    w.rest();
    assert_eq!(state(&w)["openedUrl"], before);
    assert!(w.state().notice().is_some_and(|n| n.contains("not followed")), "{:?}", w.state().notice());
    // With a shape selected, a Ctrl+click only adds to the selection.
    w.state_mut().execute("edit.select", &json!({"ids": [a]})).unwrap();
    w.rest();
    let before = state(&w)["openedUrl"].clone();
    click_with(&mut w, (7.0, 6.0), Modifiers::COMMAND);
    assert_eq!(state(&w)["openedUrl"], before, "nothing was opened");
    assert_eq!(w.state().selection(), [a, b].as_slice(), "the shape joined the selection");
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- Field ---------------------------------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn the_field_dialog_lists_the_categories_shows_a_sample_and_puts_the_field_in_the_text() {
    let (mut w, id) = with_shape("Page ");
    press(&mut w, "ribbon.Insert.FieldInsert");
    w.rest();
    assert!(state(&w)["field"].is_object());
    for category in ["custom", "date_time", "document", "geometry", "object", "page", "shape_data", "user_cell"] {
        assert!(w.state().element(&format!("field.category.{category}")).is_some(), "{category}");
    }
    press(&mut w, "field.category.page");
    press(&mut w, "field.field.page_number");
    assert_eq!(
        (state(&w)["field"]["category"].as_str(), state(&w)["field"]["field"].as_str(), state(&w)["field"]["sample"].as_str()),
        (Some("page"), Some("page_number"), Some("1"))
    );
    let steps = w.state().session().history().unwrap().undo_len();
    press(&mut w, "field.ok");
    assert!(state(&w)["field"].is_null());
    assert_eq!(shown_text(&w, id), ("Page ".to_owned(), vec!["1".to_owned()]));
    assert_eq!(w.state().session().history().unwrap().undo_len(), steps + 1, "one step");
    // A custom formula shows its sample, and one that gives an error cannot be inserted.
    run_command(&mut w, "window.field", json!({}));
    press(&mut w, "field.category.custom");
    press(&mut w, "field.formula");
    type_text(&mut w, "2+3");
    assert_eq!(state(&w)["field"]["sample"], "5");
    type_text(&mut w, "/0");
    assert_eq!(state(&w)["field"]["sample"], "", "the sample of 2+3/0 is an error");
    assert!(state(&w)["field"]["sampleError"].as_str().unwrap().contains("#DIV/0!"), "{}", state(&w)["field"]);
    assert!(!enabled(&w, "field.ok"), "OK is greyed out while the sample is an error");
    press(&mut w, "field.cancel");
    assert!(state(&w)["field"].is_null());
    assert_eq!(shown_text(&w, id).1.len(), 1, "Cancel put nothing in");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn a_field_goes_where_the_caret_of_the_open_text_box_is_and_the_box_opens_again() {
    let (mut w, id) = with_shape("ab");
    key(&mut w, Key::F2, Modifiers::NONE);
    assert_eq!(state(&w)["editingText"], true);
    // The box holds the text selected: Home puts the caret before the first letter, Right one after it.
    key(&mut w, Key::Home, Modifiers::NONE);
    key(&mut w, Key::ArrowRight, Modifiers::NONE);
    run_command(&mut w, "window.field", json!({}));
    assert_eq!(state(&w)["field"]["at"], 1, "the field goes where the caret was");
    press(&mut w, "field.category.page");
    press(&mut w, "field.field.page_number");
    press(&mut w, "field.ok");
    assert_eq!(shown_text(&w, id), ("ab".to_owned(), vec!["1".to_owned()]));
    let at = diacraft_engine::draw::text_view(w.state().session().document().unwrap(), 1, id).unwrap().fields[0].at;
    assert_eq!(at, 1, "between the two letters");
    assert_eq!(state(&w)["editingText"], true, "the box is open again");
}

// ---- Symbol --------------------------------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_symbol_of_the_menu_goes_in_at_the_end_of_the_text_of_the_shape_selected() {
    let (mut w, id) = with_shape("Cost 5");
    press(&mut w, "ribbon.Insert.SymbolInsertGallery");
    w.rest();
    press(&mut w, "menu.SymbolEuro");
    assert_eq!(shown_text(&w, id).0, "Cost 5\u{20ac}");
    run_command(&mut w, "window.symbol", json!({"text": "\u{b0}"}));
    assert_eq!(shown_text(&w, id).0, "Cost 5\u{20ac}\u{b0}");
    let refused = w.state_mut().execute("window.symbol", &json!({"text": ""})).unwrap_err().to_string();
    assert!(refused.contains("text"), "{refused}");
    // A text box that is open: the characters selected are replaced.
    key(&mut w, Key::F2, Modifiers::NONE);
    key(&mut w, Key::Home, Modifiers::NONE);
    key(&mut w, Key::ArrowRight, Modifiers::SHIFT);
    run_command(&mut w, "window.symbol", json!({"text": "\u{3a9}"}));
    assert_eq!(shown_text(&w, id).0, "\u{3a9}ost 5\u{20ac}\u{b0}", "the C was replaced");
    assert_eq!(state(&w)["editingText"], true);
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn the_symbol_dialog_picks_a_character_of_a_set_and_inserts_it() {
    let (mut w, id) = with_shape("x");
    press(&mut w, "ribbon.Insert.SymbolInsertGallery");
    w.rest();
    press(&mut w, "menu.SymbolsDialog");
    assert!(state(&w)["symbols"].is_object());
    assert!(!enabled(&w, "symbol.insert"), "nothing is chosen");
    press(&mut w, "symbol.set.greek");
    press(&mut w, "symbol.char.03A9");
    assert_eq!(state(&w)["symbols"]["selected"], "\u{3a9}");
    press(&mut w, "symbol.insert");
    assert!(state(&w)["symbols"].is_null());
    assert_eq!(shown_text(&w, id).0, "x\u{3a9}");
    // Close and Escape put nothing in.
    run_command(&mut w, "window.symbols", json!({}));
    press(&mut w, "symbol.char.00A9");
    press(&mut w, "symbol.close");
    assert_eq!(shown_text(&w, id).0, "x\u{3a9}");
    run_command(&mut w, "window.symbols", json!({}));
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(state(&w)["symbols"].is_null());
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- Pictures ------------------------------------------------------------------------------------------------------------------------------------------------------------

/// A chooser that answers `path` at once, as the native dialog does when a file is picked.
fn chooser(path: Option<PathBuf>) -> FilePicker {
    Arc::new(move || path.clone())
}

#[test]
fn pictures_shows_the_chooser_and_the_file_chosen_becomes_a_selected_picture() {
    let dir = TempDir::new("insert-picture");
    let file = png_file(&dir, "logo.png", 96, 48);
    let mut w = edit_window_with([1400.0, 900.0], |app| app.with_picture_picker(chooser(Some(file.clone()))));
    run_command(&mut w, "window.tab", json!({"tab": "Insert"}));
    press(&mut w, "ribbon.Insert.FlyoutAnchorInsertPictures");
    w.rest();
    press(&mut w, "menu.PictureInsertFromFile");
    w.rest();
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 1, "the picture is a shape");
    assert_eq!(w.state().selection(), ids.as_slice(), "and the selection");
    let shape = w.state().session().document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().clone();
    assert_eq!(shape.kind, diacraft_doc::ShapeType::Foreign);
    assert!(shape.foreign.as_ref().unwrap().picture.is_some());
    assert_eq!(state(&w)["pictureDialog"], false);
    // A chooser that was cancelled inserts nothing.
    let mut w = edit_window_with([1400.0, 900.0], |app| app.with_picture_picker(chooser(None)));
    run_command(&mut w, "window.insertPicture", json!({}));
    assert!(shape_ids(&w).is_empty());
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn a_picture_by_path_needs_no_dialog_and_what_cannot_be_inserted_is_said_in_words() {
    let dir = TempDir::new("insert-picture");
    let mut w = edit_window([1400.0, 900.0]);
    run_command(&mut w, "window.insertPicture", json!({"path": png_file(&dir, "a.png", 8, 8).to_str().unwrap()}));
    assert_eq!(shape_ids(&w).len(), 1);
    // A GIF is kept and drawn as a placeholder, and the notice says so.
    let gif = dir.file("a.gif");
    std::fs::write(&gif, [b"GIF89a".as_slice(), &[8, 0, 8, 0, 0, 0, 0, 0x3b]].concat()).unwrap();
    run_command(&mut w, "window.insertPicture", json!({"path": gif.to_str().unwrap()}));
    assert_eq!(shape_ids(&w).len(), 2);
    assert!(w.state().notice().is_some_and(|n| n.contains("GIF") && n.contains("placeholder")), "{:?}", w.state().notice());
    // A file that is no picture is refused in the engine's words, and the drawing is as it was.
    let text = dir.file("notes.png");
    std::fs::write(&text, "words, not a picture").unwrap();
    let refused = w.state_mut().execute("window.insertPicture", &json!({"path": text.to_str().unwrap()})).unwrap_err().to_string();
    assert!(refused.contains("not a picture"), "{refused}");
    assert_eq!(shape_ids(&w).len(), 2);
    // With no chooser in the build, the command without a path says so.
    let refused = w.state_mut().execute("window.insertPicture", &json!({})).unwrap_err().to_string();
    assert!(refused.contains("no file dialog"), "{refused}");
    assert_eq!(w.state().ui_panics(), 0);
}
