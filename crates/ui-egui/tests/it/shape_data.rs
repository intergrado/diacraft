//! In the window: the Shape Data pane (Data, Show/Hide, Shape Data Window), the Define Shape Data dialog and the question a drop asks. The pointer and the keyboard are given to egui
//! as a person's would be, and what is checked is the document the commands changed and what the window drew.

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};

use diacraft_ui_egui::work;

use crate::editing::{at, drag_tile_to, edit_window, key, shape_ids, type_text};
use crate::saving::ctrl;
use crate::support::{Rest, Window, click, move_to, press, run_command};

fn rect_of(w: &Window, name: &str) -> egui::Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

fn click_on(w: &mut Window, name: &str) {
    let to = rect_of(w, name).center();
    click(w, to);
    w.rest();
}

fn panes(w: &Window) -> Value {
    w.state().window_state()["panes"].clone()
}

/// A rectangle dropped at (4, 5) and selected.
fn window_with_a_shape() -> (Window, u32) {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 4.0, 5.0);
    let id = shape_ids(&w)[0];
    (w, id)
}

fn define(w: &mut Window, id: u32, fields: Value) {
    run_command(w, "shapedata.define", json!({"id": id, "fields": fields}));
}

fn open_pane(w: &mut Window) {
    run_command(w, "window.pane", json!({"pane": "shape_data", "shown": true}));
}

/// The fields `shapedata.list` gives, hidden ones too.
fn fields(w: &mut Window, id: u32) -> Vec<Value> {
    w.state_mut().execute("shapedata.list", &json!({"id": id, "hidden": true})).unwrap()["fields"].as_array().unwrap().clone()
}

fn value_of(w: &mut Window, id: u32, row: &str) -> String {
    fields(w, id).iter().find(|f| f["row"] == row).unwrap()["value"].as_str().unwrap().to_owned()
}

fn undo_steps(w: &Window) -> usize {
    w.state().session().history().unwrap().undo_len()
}

/// Types `text` in the field `name` and ends it with Enter: one value, as a person types it.
fn type_in(w: &mut Window, name: &str, text: &str) {
    click_on(w, name);
    key(w, Key::A, ctrl());
    type_text(w, text);
    key(w, Key::Enter, Modifiers::NONE);
}

fn right_click(w: &mut Window, pos: Pos2) {
    for _ in 0..3 {
        move_to(w, pos);
    }
    for pressed in [true, false] {
        w.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        w.step();
    }
    w.rest();
}

fn typed() -> (Window, u32) {
    let (mut w, id) = window_with_a_shape();
    define(
        &mut w,
        id,
        json!([
            {"label": "Owner", "name": "Owner", "type": "string", "prompt": "Who owns it"},
            {"label": "Qty", "name": "Qty", "type": "number", "format": "0.00"},
            {"label": "Pick", "name": "Pick", "type": "fixed_list", "format": "Red;Green;Blue"},
            {"label": "Done", "name": "Done", "type": "boolean"},
            {"label": "Tag", "name": "Tag", "type": "variable_list", "format": "Red;Green"},
            {"label": "Secret", "name": "Secret", "type": "string", "invisible": true},
        ]),
    );
    (w, id)
}

// ---- the pane ----------------------------------------------------------------------------------------------------------------------------

/// Data, Show/Hide, Shape Data Window and View, Task Panes, Shape Data open the pane, titled with the name of the shape selected; a click on the check box again closes it.
#[test]
fn the_data_tab_and_the_view_menu_open_the_pane_titled_with_the_shapes_name() {
    let (mut w, id) = window_with_a_shape();
    run_command(&mut w, "window.tab", json!({"tab": "Data"}));
    assert!(w.state().element("ribbon.Data.ShapeDataWindow").is_some_and(|e| e.enabled), "the check box works");
    press(&mut w, "ribbon.Data.ShapeDataWindow");
    assert_eq!(panes(&w), json!([["shape_data"]]));
    w.get_by_label(&format!("Shape Data - Rectangle.{id}"));
    press(&mut w, "ribbon.Data.ShapeDataWindow");
    assert_eq!(panes(&w), json!([]), "the same box closes it");
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    press(&mut w, "ribbon.View.TaskPanesMenu");
    assert!(w.state().element("menu.ShapeDataWindowShowHide").is_some_and(|e| e.enabled), "View, Task Panes lists it");
    click_on(&mut w, "menu.ShapeDataWindowShowHide");
    assert_eq!(panes(&w), json!([["shape_data"]]));
    assert_eq!(w.state().ui_panics(), 0);
}

/// With no shape selected the title is "Shape Data - ThePage", with several it is "Shape Data", and the body says to select one.
#[test]
fn with_no_shape_or_several_selected_the_pane_lists_nothing() {
    let (mut w, id) = typed();
    open_pane(&mut w);
    assert!(w.state().element("pane.data.Owner").is_some(), "one shape selected: its fields");
    run_command(&mut w, "edit.select", json!({"ids": []}));
    w.get_by_label("Shape Data - ThePage");
    assert!(w.state().element("pane.data.Owner").is_none());
    drag_tile_to(&mut w, "Ellipse", 7.0, 3.0);
    let other = *shape_ids(&w).last().unwrap();
    run_command(&mut w, "edit.select", json!({"ids": [id, other]}));
    w.get_by_label("Shape Data");
    assert!(w.state().element("pane.data.Owner").is_none(), "several shapes: nothing is listed");
    run_command(&mut w, "edit.select", json!({"ids": [other]}));
    assert!(w.state().element("pane.data.Owner").is_none(), "the other shape has no shape data");
    assert!(w.state().element("pane.shape_data.define").is_some_and(|e| e.enabled), "but its pane offers Define Shape Data");
}

/// A value typed in a text box is one `shapedata.set` (one undo step) when Enter is pressed; a field shows the value through its format; a value that is no value of the type is refused in words under the
/// field and changes nothing.
#[test]
fn a_value_typed_in_a_field_is_one_undo_step_and_a_refusal_is_said_under_the_field() {
    let (mut w, id) = typed();
    open_pane(&mut w);
    let steps = undo_steps(&w);
    type_in(&mut w, "pane.data.Owner", "Ann Lee");
    assert_eq!(value_of(&mut w, id, "Owner"), "Ann Lee");
    assert_eq!(undo_steps(&w), steps + 1, "one undo step");
    type_in(&mut w, "pane.data.Qty", "12.5");
    assert_eq!(value_of(&mut w, id, "Qty"), "12.5");
    assert_eq!(w.state().pane_field_text("pane.data.Qty"), Some("12.50"), "shown through the format 0.00");
    assert_eq!(w.state().pane_field_text("pane.data.Owner"), Some("Ann Lee"));
    // Refused: the words are under the field, and the drawing is as it was.
    let steps = undo_steps(&w);
    type_in(&mut w, "pane.data.Qty", "twelve");
    let words = w.state().pane_field_refusal("pane.data.Qty").unwrap_or_default().to_owned();
    assert!(words.contains("twelve") && words.contains("number"), "{words}");
    assert_eq!(value_of(&mut w, id, "Qty"), "12.5");
    assert_eq!(undo_steps(&w), steps);
    // Undo takes the value typed before it back, and the pane follows.
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(value_of(&mut w, id, "Qty"), "");
    assert_eq!(w.state().pane_field_text("pane.data.Qty"), Some(""));
    assert_eq!(w.state().ui_panics(), 0);
}

/// A field that is not changed by typing in it sends nothing: a box passed through, or one left with the same text.
#[test]
fn a_box_passed_through_or_left_as_it_was_sends_nothing() {
    let (mut w, id) = typed();
    open_pane(&mut w);
    type_in(&mut w, "pane.data.Owner", "Ann");
    let steps = undo_steps(&w);
    click_on(&mut w, "pane.data.Owner");
    key(&mut w, Key::Tab, Modifiers::NONE);
    type_in(&mut w, "pane.data.Owner", "Ann");
    assert_eq!(undo_steps(&w), steps, "the same value is no step");
    assert_eq!(value_of(&mut w, id, "Owner"), "Ann");
}

/// A fixed list and a boolean are drop-downs; a variable list's drop-down has New value..., which turns the row into a text box that takes the keys at once.
#[test]
fn lists_and_booleans_are_drop_downs_and_a_variable_list_takes_a_new_value() {
    let (mut w, id) = typed();
    open_pane(&mut w);
    click_on(&mut w, "pane.data.Pick");
    for n in 0..4 {
        assert!(w.state().element(&format!("pane.data.Pick.item.{n}")).is_some(), "item {n}: the three items and (blank)");
    }
    click_on(&mut w, "pane.data.Pick.item.1");
    assert_eq!(value_of(&mut w, id, "Pick"), "Green");
    click_on(&mut w, "pane.data.Done");
    click_on(&mut w, "pane.data.Done.item.0");
    assert_eq!(value_of(&mut w, id, "Done"), "1");
    click_on(&mut w, "pane.data.Done");
    click_on(&mut w, "pane.data.Done.item.1");
    assert_eq!(value_of(&mut w, id, "Done"), "0");
    // A variable list: an item, and then a value it does not have.
    click_on(&mut w, "pane.data.Tag");
    click_on(&mut w, "pane.data.Tag.item.0");
    assert_eq!(value_of(&mut w, id, "Tag"), "Red");
    click_on(&mut w, "pane.data.Tag");
    click_on(&mut w, "pane.data.Tag.new");
    assert!(w.state().pane_field_text("pane.data.Tag").is_some(), "the row is a text box now");
    type_text(&mut w, "Violet");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert_eq!(value_of(&mut w, id, "Tag"), "Violet");
    let tag = fields(&mut w, id).into_iter().find(|f| f["row"] == "Tag").unwrap();
    assert_eq!(tag["format"], "Red;Green;Violet", "the new value joined the list");
    assert!(w.state().pane_field_text("pane.data.Tag").is_none(), "and the row is a drop-down again");
}

/// A field with the Hidden box is left out of the window; the list is read once for a revision, not on every frame, and again when the drawing changes.
#[test]
fn hidden_fields_are_left_out_and_the_list_is_read_once_for_a_revision() {
    let (mut w, id) = typed();
    work::reset();
    open_pane(&mut w);
    assert!(w.state().element("pane.data.Secret").is_none(), "hidden");
    let rows: Vec<String> = ["Owner", "Qty", "Pick", "Done", "Tag"].iter().map(|r| format!("pane.data.{r}")).collect();
    for row in &rows {
        assert!(w.state().element(row).is_some(), "{row}");
    }
    let asked = work::work().data_queries;
    assert!(asked >= 1);
    for _ in 0..5 {
        w.step();
    }
    assert_eq!(work::work().data_queries, asked, "the same drawing: no new question");
    run_command(&mut w, "shapedata.set", json!({"id": id, "row": "Owner", "value": "x"}));
    assert!(work::work().data_queries > asked, "the drawing changed: asked again");
}

/// The pane follows the selection and a new drawing replaces what it kept.
#[test]
fn the_pane_follows_the_selection() {
    let (mut w, id) = typed();
    drag_tile_to(&mut w, "Ellipse", 7.0, 3.0);
    let other = *shape_ids(&w).last().unwrap();
    define(&mut w, other, json!([{"label": "Site", "name": "Site"}]));
    open_pane(&mut w);
    assert!(w.state().element("pane.data.Site").is_some() && w.state().element("pane.data.Owner").is_none());
    run_command(&mut w, "edit.select", json!({"ids": [id]}));
    assert!(w.state().element("pane.data.Owner").is_some() && w.state().element("pane.data.Site").is_none());
    run_command(&mut w, "file.new", json!({}));
    assert!(w.state().element("pane.data.Owner").is_none(), "a new drawing: nothing of the old one");
}

// ---- the Define Shape Data dialog -----------------------------------------------------------------------------------------------------------

#[test]
fn define_shape_data_needs_one_shape_selected_and_opens_from_the_pane_and_the_menu() {
    let (mut w, id) = typed();
    run_command(&mut w, "edit.select", json!({"ids": []}));
    assert!(w.state().command_enabled("window.defineShapeData").is_err(), "nothing is selected");
    run_command(&mut w, "edit.select", json!({"ids": [id]}));
    assert_eq!(w.state().command_enabled("window.defineShapeData"), Ok(()));
    // From the pane's button.
    open_pane(&mut w);
    click_on(&mut w, "pane.shape_data.define");
    let state = w.state().window_state()["shapeData"]["define"].clone();
    assert_eq!(state["id"], id);
    let rows: Vec<&str> = state["fields"].as_array().unwrap().iter().map(|f| f["row"].as_str().unwrap()).collect();
    assert_eq!(rows, ["Owner", "Qty", "Pick", "Done", "Tag", "Secret"], "every field, the hidden one too");
    click_on(&mut w, "data.cancel");
    assert!(w.state().window_state()["shapeData"]["define"].is_null());
    // From the shape's right-click menu: Data, then Define Shape Data...
    run_command(&mut w, "window.pane", json!({"pane": "shape_data", "shown": false}));
    let on_shape = at(&w, 4.0, 5.0);
    right_click(&mut w, on_shape);
    assert!(w.state().element("menu.CanvasData").is_some(), "Data is a row of the shape's menu");
    click_on(&mut w, "menu.CanvasData");
    assert!(w.state().element("menu.CanvasShapeData").is_some() && w.state().element("menu.CanvasDefineShapeData").is_some());
    click_on(&mut w, "menu.CanvasShapeData");
    assert_eq!(panes(&w), json!([["shape_data"]]), "Shape Data... opens the pane");
}

/// New adds a field: its Label, Name and Type are typed and chosen, OK sends one `shapedata.define` (one undo step), and the field is there with the name made of the label.
#[test]
fn new_adds_a_field_and_ok_sends_one_define_in_one_undo_step() {
    let (mut w, id) = window_with_a_shape();
    run_command(&mut w, "window.defineShapeData", json!({}));
    assert_eq!(w.state().window_state()["shapeData"]["define"]["fields"], json!([]), "a shape with no shape data starts with an empty list");
    click_on(&mut w, "data.new");
    type_in_box(&mut w, "data.label", "Unit cost");
    click_on(&mut w, "data.type");
    click_on(&mut w, "data.type.currency");
    type_in_box(&mut w, "data.value", "$12.5");
    type_in_box(&mut w, "data.prompt", "Cost of one");
    click_on(&mut w, "data.ask");
    let steps = undo_steps(&w);
    click_on(&mut w, "data.ok");
    assert!(w.state().window_state()["shapeData"]["define"].is_null(), "the dialog closed");
    assert_eq!(undo_steps(&w), steps + 1, "one undo step");
    let listed = fields(&mut w, id);
    assert_eq!(listed.len(), 1);
    let f = &listed[0];
    assert_eq!(
        (f["row"].as_str(), f["label"].as_str(), f["type"].as_str(), f["shown"].as_str()),
        (Some("Unit_cost"), Some("Unit cost"), Some("currency"), Some("$12.50"))
    );
    assert_eq!((f["prompt"].as_str(), f["ask"].as_bool()), (Some("Cost of one"), Some(true)));
    run_command(&mut w, "edit.undo", json!({}));
    assert!(fields(&mut w, id).is_empty());
}

/// Types in a box of the dialog.
fn type_in_box(w: &mut Window, name: &str, text: &str) {
    click_on(w, name);
    key(w, Key::A, ctrl());
    type_text(w, text);
}

/// Delete takes a field out of the list; Move Up and Move Down change the order by renumbering the sort keys; Cancel sends nothing.
#[test]
fn delete_and_move_change_the_list_and_cancel_sends_nothing() {
    let (mut w, id) = typed();
    run_command(&mut w, "window.defineShapeData", json!({}));
    let steps = undo_steps(&w);
    click_on(&mut w, "data.field.1");
    click_on(&mut w, "data.up");
    let state = w.state().window_state()["shapeData"]["define"].clone();
    let order: Vec<&str> = state["fields"].as_array().unwrap().iter().map(|f| f["row"].as_str().unwrap()).collect();
    assert_eq!(order[..2], ["Qty", "Owner"], "{state}");
    assert_eq!(state["selected"], 0);
    let keys: Vec<&str> = state["fields"].as_array().unwrap().iter().map(|f| f["sort_key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["0001", "0002", "0003", "0004", "0005", "0006"], "every field's key is its place");
    click_on(&mut w, "data.field.4");
    click_on(&mut w, "data.delete");
    assert_eq!(w.state().window_state()["shapeData"]["define"]["fields"].as_array().unwrap().len(), 5);
    click_on(&mut w, "data.cancel");
    assert_eq!(undo_steps(&w), steps, "Cancel sends nothing");
    assert_eq!(fields(&mut w, id).len(), 6);
    // OK sends the order and the deletion in one step.
    run_command(&mut w, "window.defineShapeData", json!({}));
    click_on(&mut w, "data.field.1");
    click_on(&mut w, "data.up");
    click_on(&mut w, "data.field.4");
    click_on(&mut w, "data.delete");
    click_on(&mut w, "data.ok");
    assert_eq!(undo_steps(&w), steps + 1);
    let rows: Vec<String> = fields(&mut w, id).iter().map(|f| f["row"].as_str().unwrap().to_owned()).collect();
    assert_eq!(rows, ["Qty", "Owner", "Pick", "Done", "Secret"], "Tag is gone and Qty is first");
}

/// A refusal of the engine is said in the dialog, which stays open and sends nothing.
#[test]
fn a_refusal_is_said_in_the_dialog_which_stays_open() {
    let (mut w, id) = typed();
    run_command(&mut w, "window.defineShapeData", json!({}));
    click_on(&mut w, "data.new");
    type_in_box(&mut w, "data.name", "Owner");
    let steps = undo_steps(&w);
    click_on(&mut w, "data.ok");
    let state = w.state().window_state()["shapeData"]["define"].clone();
    assert!(!state.is_null(), "the dialog stays");
    assert!(state["error"].as_str().unwrap().contains("already used"), "{state}");
    assert!(w.state().element("data.error").is_some());
    assert_eq!(undo_steps(&w), steps);
    assert_eq!(fields(&mut w, id).len(), 6);
    // A language that is no language tag is the engine's refusal (the cell holds a tag such as en-US).
    type_in_box(&mut w, "data.name", "Fresh");
    type_in_box(&mut w, "data.lang_id", "en US");
    click_on(&mut w, "data.ok");
    let state = w.state().window_state()["shapeData"]["define"].clone();
    assert!(state["error"].as_str().unwrap().contains("language tag"), "{state}");
}

/// Hidden and Ask on drop are boxes of the dialog; a hidden field leaves the pane at once.
#[test]
fn the_hidden_box_takes_a_field_out_of_the_pane() {
    let (mut w, id) = typed();
    open_pane(&mut w);
    assert!(w.state().element("pane.data.Owner").is_some());
    run_command(&mut w, "window.defineShapeData", json!({}));
    click_on(&mut w, "data.field.0");
    click_on(&mut w, "data.hidden");
    click_on(&mut w, "data.ok");
    assert!(w.state().element("pane.data.Owner").is_none(), "hidden now");
    assert_eq!(fields(&mut w, id).iter().filter(|f| f["invisible"] == true).count(), 2);
}

// ---- Ask on drop ----------------------------------------------------------------------------------------------------------------------------

/// A shape dropped from a master that holds a field with Ask on drop has the card asked: its editor is that of the type, OK sends one `shapedata.set` of the values typed and Cancel leaves the shape as it was dropped.
#[test]
fn a_drop_of_a_shape_with_a_field_that_asks_opens_the_card() {
    let (mut w, id) = window_with_a_shape();
    define(&mut w, id, json!([{"label": "Owner", "name": "Owner", "ask": true}, {"label": "Site", "name": "Site"}]));
    // The window asks about a shape it has just dropped: here the shape that has the field is the one asked about.
    run_command(&mut w, "window.askShapeData", json!({"id": id}));
    let state = w.state().window_state()["shapeData"]["ask"].clone();
    let rows: Vec<&str> = state["fields"].as_array().unwrap().iter().map(|f| f["row"].as_str().unwrap()).collect();
    assert_eq!(rows, ["Owner"], "only the field that asks");
    let steps = undo_steps(&w);
    click_on(&mut w, "ask.cancel");
    assert!(w.state().window_state()["shapeData"]["ask"].is_null());
    assert_eq!(undo_steps(&w), steps);
    // The window command closes the card too, with no shape named.
    run_command(&mut w, "window.askShapeData", json!({"id": id}));
    assert!(!w.state().window_state()["shapeData"]["ask"].is_null());
    run_command(&mut w, "window.askShapeData", json!({"close": true}));
    assert!(w.state().window_state()["shapeData"]["ask"].is_null());
    run_command(&mut w, "window.askShapeData", json!({"id": id}));
    type_in_box(&mut w, "ask.Owner", "Ann");
    click_on(&mut w, "ask.ok");
    assert!(w.state().window_state()["shapeData"]["ask"].is_null());
    assert_eq!(value_of(&mut w, id, "Owner"), "Ann");
    assert_eq!(undo_steps(&w), steps + 1, "one undo step");
    // A shape none of whose fields asks is not asked.
    define(&mut w, id, json!([{"row": "Owner", "ask": false}, {"row": "Site"}]));
    run_command(&mut w, "window.askShapeData", json!({"id": id}));
    assert!(w.state().window_state()["shapeData"]["ask"].is_null());
}

/// The draft holds at most the 512 fields the engine holds: New is greyed out at that many, and a press on it does nothing.
#[test]
fn the_dialog_holds_no_more_fields_than_the_engine_does() {
    let (mut w, id) = window_with_a_shape();
    let many: Vec<Value> = (0..512).map(|n| json!({"label": format!("F{n}")})).collect();
    define(&mut w, id, Value::Array(many));
    run_command(&mut w, "window.defineShapeData", json!({}));
    assert_eq!(w.state().window_state()["shapeData"]["define"]["fields"].as_array().unwrap().len(), 512);
    assert!(w.state().element("data.new").is_some_and(|e| !e.enabled), "New is greyed out at 512 fields");
    click_on(&mut w, "data.new");
    assert_eq!(w.state().window_state()["shapeData"]["define"]["fields"].as_array().unwrap().len(), 512);
    click_on(&mut w, "data.field.0");
    click_on(&mut w, "data.delete");
    assert!(w.state().element("data.new").is_some_and(|e| e.enabled), "one gone: New works again");
}

// ----: the forms files hold ----------------------------------------------------------------------------------------------------

/// A window on a shape whose `Property` rows hold forms a file holds and the dialog must keep: `Odd` a `Type` of 8, `Lang` a `LangID` of 1033
/// and `Due` a date never set, a plain 0 with no unit, with the calendar 0. The shape is selected.
fn window_with_file_forms() -> (Window, u32) {
    let mut session = diacraft_engine::Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let id = session.execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": 4.0, "y": 5.0})).unwrap()["id"].as_u64().unwrap() as u32;
    let rows: [(&str, &[(&str, &str)]); 3] = [
        ("Odd", &[("Label", "Odd"), ("Type", "8"), ("Value", "a")]),
        ("Lang", &[("Label", "Lang"), ("Type", "0"), ("Value", "b"), ("LangID", "1033")]),
        ("Due", &[("Label", "Due"), ("Type", "5"), ("Value", "0"), ("Calendar", "0")]),
    ];
    session
        .edit("rows as a file holds them", |d| {
            let at = d.find_shape(diacraft_doc::Owner::Page(1), id).unwrap();
            let page = std::sync::Arc::make_mut(d.pages.get_mut(0).unwrap());
            let shape = std::sync::Arc::make_mut(page.shapes.get_mut(at.path[0] as usize).unwrap());
            let mut section = diacraft_doc::Section::new("Property", None);
            for (name, cells) in rows {
                let mut row = diacraft_doc::Row { name: Some(std::sync::Arc::from(name)), ..diacraft_doc::Row::default() };
                for (cell, text) in cells {
                    row.cells.insert((*cell).into(), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text(text)));
                }
                section.rows.push(std::sync::Arc::new(row));
            }
            std::sync::Arc::make_mut(&mut shape.sheet).sections.push(std::sync::Arc::new(section));
            Ok(())
        })
        .unwrap();
    let mut w = crate::support::fast_window(
        diacraft_ui_egui::DiacraftApp::new(session, None).with_inline_pictures(diacraft_ui_egui::render_worker::bundled_drawer()),
        [1280.0, 900.0],
    );
    w.rest();
    run_command(&mut w, "edit.select", json!({"ids": [id]}));
    (w, id)
}

/// What the drop-down labelled `label` shows.
fn combo_text(w: &Window, label: &str) -> Option<String> {
    w.query_by_role_and_label(egui::accesskit::Role::ComboBox, label).and_then(|n| n.value())
}

fn listed_field(w: &mut Window, id: u32, row: &str) -> Value {
    fields(w, id).into_iter().find(|f| f["row"] == row).unwrap()
}

/// The dialog lists a `Type` it does not know as Unknown and **keeps it as written** when OK sends the list, unless a type is chosen.
#[test]
fn the_dialog_keeps_a_type_it_does_not_know() {
    let (mut w, id) = window_with_file_forms();
    run_command(&mut w, "window.defineShapeData", json!({}));
    let state = w.state().window_state()["shapeData"]["define"].clone();
    assert_eq!(state["fields"][0]["type"], "unknown", "{state}");
    click_on(&mut w, "data.field.0");
    assert_eq!(combo_text(&w, "Type").as_deref(), Some("Unknown (8)"), "the Type box says the type is unknown, and its number");
    type_in_box(&mut w, "data.label", "Odd one");
    click_on(&mut w, "data.ok");
    assert!(w.state().window_state()["shapeData"]["define"].is_null(), "the dialog closed");
    let odd = listed_field(&mut w, id, "Odd");
    assert_eq!((odd["label"].as_str(), odd["type"].as_str(), odd["type_code"].as_i64()), (Some("Odd one"), Some("unknown"), Some(8)), "{odd}");
    let due = listed_field(&mut w, id, "Due");
    assert_eq!((due["value"].as_str(), due["unit"].is_null()), (Some("0"), true), "a date never set stays the plain 0: {due}");
}

/// The Language box takes a tag, and a number a file holds is kept when the box is not typed in; a text that is no tag is the engine's refusal, said in the dialog.
#[test]
fn the_dialog_takes_a_language_tag() {
    let (mut w, id) = window_with_file_forms();
    run_command(&mut w, "window.defineShapeData", json!({}));
    click_on(&mut w, "data.field.0");
    type_in_box(&mut w, "data.label", "Odd one");
    click_on(&mut w, "data.ok");
    assert_eq!(listed_field(&mut w, id, "Lang")["lang_id"], "1033", "a box not typed in sends nothing");
    run_command(&mut w, "window.defineShapeData", json!({}));
    click_on(&mut w, "data.field.1");
    type_in_box(&mut w, "data.lang_id", "en US");
    click_on(&mut w, "data.ok");
    let state = w.state().window_state()["shapeData"]["define"].clone();
    assert!(state["error"].as_str().is_some_and(|e| e.contains("language tag")), "{state}");
    type_in_box(&mut w, "data.lang_id", "de-DE");
    click_on(&mut w, "data.ok");
    assert!(w.state().window_state()["shapeData"]["define"].is_null(), "the dialog closed");
    assert_eq!(listed_field(&mut w, id, "Lang")["lang_id"], "de-DE");
}

/// A date field's Calendar is a drop-down of the fourteen calendars by name, and the one chosen is its number.
#[test]
fn the_dialog_names_the_calendars_of_a_date_field() {
    let (mut w, id) = window_with_file_forms();
    run_command(&mut w, "window.defineShapeData", json!({}));
    click_on(&mut w, "data.field.2");
    assert_eq!(combo_text(&w, "Calendar").as_deref(), Some("Western"));
    click_on(&mut w, "data.calendar");
    assert!(w.state().element("data.calendar.13").is_some(), "the fourteen of MS-VSDX's table");
    click_on(&mut w, "data.calendar.2");
    click_on(&mut w, "data.ok");
    let due = listed_field(&mut w, id, "Due");
    assert_eq!((due["calendar"].as_u64(), due["calendar_name"].as_str()), (Some(2), Some("Hebrew Lunar")), "{due}");
    // A field that is no date has no calendar to choose.
    run_command(&mut w, "window.defineShapeData", json!({}));
    click_on(&mut w, "data.field.1");
    assert!(w.state().element("data.calendar").is_none(), "only a date reads its calendar");
}
