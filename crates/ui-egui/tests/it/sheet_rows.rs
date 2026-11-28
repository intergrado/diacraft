//! The ShapeSheet window, rows, sections and the Editing group: the Sections, Rows and Editing groups of the ShapeSheet Design tab, the sheet's right-click menu, the Insert
//! Section card and the Edit Formula card.
//!
//! The design is the ShapeSheet window's (5.8 for Edit Formula, 5.12 for the rows and sections, 5.13 for the tab) and the spec of the window is
//! (for what Insert Section lists and where Insert Row puts a row, R24 for the Editing group, R42 for Ctrl+E). The window adds and
//! deletes rows and sections only through the engine's `sheet.add_row`, `sheet.delete_row`, `sheet.add_section` and `sheet.delete_section`; what a test asks of the engine's own
//! rules is what `crates/engine/tests/it/sheet_rows.rs` holds, and these tests hold what the window sends, greys, shows and says.

use diacraft_doc::{DISPLAY_ORDER, RowKey};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use egui::{Event, Key, Modifiers, Rect};
use serde_json::{Value, json};

use crate::sheet_window::{geometry_shape, is_drawn, one_page, open_shape, overlaps, rect_of, right_click, sheet, window_on};
use crate::support::{Rest, Window, press, run_command};

// ---- the windows these tests open and the keys they press ------------------------------------------------------------------------------

/// A shape with two User rows, a Scratch of three rows and a Geometry of four rows.
fn rich() -> ShapeBuilder {
    let mut shape =
        geometry_shape(10, 4).row_cell("User", None, RowKey::name("Row_1"), "Value", "1").row_cell("User", None, RowKey::name("Row_2"), "Value", "2");
    for k in 0..3_u32 {
        shape = shape.row_cell("Scratch", None, RowKey::Ix(k), "A", &(10 * (k + 1)).to_string());
    }
    shape
}

/// A window on a drawing with `shape` as shape 10, its sheet open and the ShapeSheet Design tab on top.
fn sheet_of(shape: ShapeBuilder, size: [f32; 2]) -> Window {
    let mut w = window_on(one_page(shape), size);
    open_shape(&mut w, 10);
    run_command(&mut w, "window.tab", json!({"tab": "ShapeSheet Design"}));
    w
}

fn select_cell(w: &mut Window, cell: &str) {
    run_command(w, "window.sheetSelect", json!({"cell": cell}));
}

/// Selects a row by the number a reference spells it with.
fn select_row(w: &mut Window, section: &str, row: u32) {
    run_command(w, "window.sheetSelect", json!({"section": section, "row": row}));
}

fn press_key(w: &mut Window, key: Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        w.input_mut().events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        w.step();
    }
    w.rest();
}

fn tap(w: &mut Window, key: Key) {
    press_key(w, key, Modifiers::NONE);
}

fn typed(w: &mut Window, text: &str) {
    w.input_mut().events.push(Event::Text(text.to_owned()));
    w.step();
    w.rest();
}

/// The ribbon's own control of the ShapeSheet Design tab.
fn control(id: &str) -> String {
    format!("ribbon.ShapeSheet Design.{id}")
}

fn enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

/// What the window says of a control it can grey: `{ok, reason}`, by the name the window state gives it.
fn op(w: &Window, name: &str) -> Value {
    let ops = sheet(w)["ops"].clone();
    ops.get(name).cloned().unwrap_or_else(|| panic!("the window state has no op {name}: {ops}"))
}

fn reason(w: &Window, name: &str) -> String {
    op(w, name)["reason"].as_str().unwrap_or("").to_owned()
}

fn footer(w: &Window) -> String {
    sheet(w)["footer"].as_str().unwrap_or("").to_owned()
}

fn selected(w: &Window) -> Value {
    sheet(w)["selected"].clone()
}

/// The steps of the drawing's history that an undo goes back over.
fn steps(w: &Window) -> usize {
    w.state().session().history().map_or(0, |h| h.undo_len())
}

/// What the window sent the engine, in order, for the commands whose id starts with `prefix`.
fn sent(w: &Window, prefix: &str) -> Vec<(String, Value)> {
    w.state().session().journal().iter().filter(|(id, _)| id.starts_with(prefix)).cloned().collect()
}

/// The keys of the rows of a section of shape 10, as `sheet.read` lists them.
fn row_keys(w: &mut Window, section: &str) -> Vec<Value> {
    let read = w.state_mut().execute("sheet.read", &json!({"id": 10, "sections": [section], "count": 2000})).unwrap();
    read["lines"].as_array().unwrap().iter().filter(|l| l["kind"] == "row").map(|l| l["row"]["key"].clone()).collect()
}

/// The labels of the sections the sheet on top shows, in order.
fn labels(w: &Window) -> Vec<String> {
    sheet(w)["sections"].as_array().unwrap().iter().map(|s| s["label"].as_str().unwrap().to_owned()).collect()
}

/// The name of the section the sheet on top shows under this label.
fn section_named(w: &Window, label: &str) -> String {
    sheet(w)["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["label"] == label)
        .map(|s| s["name"].as_str().unwrap().to_owned())
        .unwrap_or_else(|| panic!("the sheet shows no section {label}: {:?}", labels(w)))
}

fn card(w: &Window) -> Value {
    sheet(w)["card"].clone()
}

// ---- Insert Section (design 5.12) ------------------------------------------------------------------------------------------

/// The labels a card of Insert Section lists for any sheet: every section a sheet of some kind can take, with a Geometry section as the three kinds it comes in.
fn insertable_somewhere() -> Vec<String> {
    let mut labels = Vec::new();
    for entry in DISPLAY_ORDER.iter().filter(|d| d.rules.insertable) {
        if entry.element == Some("Geometry") {
            labels.extend(["Geometry", "Ellipse", "Infinite Line"].map(str::to_owned));
        } else {
            labels.push(entry.label.to_owned());
        }
    }
    labels
}

/// Insert (Sections) opens a card that lists what `sheet.sections` offers and greys every section the sheet has; a Geometry section can be added again.
#[test]
fn insert_section_lists_what_sheet_sections_offers_and_greys_what_the_sheet_has() {
    let mut w = sheet_of(rich(), [1400.0, 900.0]);
    select_cell(&mut w, "Width");
    assert!(enabled(&w, &control("ShapeSheetInsertSection")), "Insert is there with any selection");
    assert_eq!(card(&w), Value::Null, "no card before the press");
    press(&mut w, &control("ShapeSheetInsertSection"));
    w.rest();
    assert_eq!(card(&w)["kind"], json!("insertSection"), "{}", card(&w));

    let answer = w.state_mut().execute("sheet.sections", &json!({"id": 10, "page": 1})).unwrap();
    let offered: Vec<String> = answer["can_add"].as_array().unwrap().iter().map(|o| o["label"].as_str().unwrap().to_owned()).collect();
    assert!(offered.iter().any(|l| l == "Scratch") || w.state().element("sheet.card.insert.Scratch").is_some(), "a Scratch section is listed");
    let listed = insertable_somewhere();
    for label in &listed {
        let name = format!("sheet.card.insert.{label}");
        let on = enabled(&w, &name);
        assert_eq!(on, offered.contains(label), "{label}: a box is on exactly when sheet.sections offers it ({offered:?})");
    }
    // What the sheet has is greyed: User-defined Cells; a Geometry section is offered again.
    assert!(!enabled(&w, "sheet.card.insert.User-defined Cells"), "the sheet has User-defined Cells already");
    assert!(enabled(&w, "sheet.card.insert.Geometry"), "a Geometry section can be added again");
    assert!(enabled(&w, "sheet.card.insert.Ellipse") && enabled(&w, "sheet.card.insert.Infinite Line"));
    let items: Vec<String> = card(&w)["items"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap().to_owned()).collect();
    assert_eq!(items, listed, "the card's own account lists the same sections in the same order");
    assert!(!enabled(&w, "sheet.card.insert.ok"), "OK waits for a box to be ticked");
    assert!(enabled(&w, "sheet.card.insert.cancel"));
}

/// The ticked boxes are added as one undo step with one `command.batch` of `sheet.add_section`; Cancel adds nothing.
#[test]
fn ticked_sections_are_added_as_one_undo_step_and_cancel_adds_nothing() {
    let mut w = sheet_of(rich(), [1400.0, 900.0]);
    select_cell(&mut w, "Width");
    let before = (steps(&w), w.state().session().revision());
    press(&mut w, &control("ShapeSheetInsertSection"));
    w.rest();
    press(&mut w, "sheet.card.insert.cancel");
    w.rest();
    assert_eq!(card(&w), Value::Null, "Cancel closes the card");
    assert_eq!((steps(&w), w.state().session().revision()), before, "and adds nothing");

    press(&mut w, &control("ShapeSheetInsertSection"));
    w.rest();
    press(&mut w, "sheet.card.insert.Controls");
    press(&mut w, "sheet.card.insert.Shape Data");
    w.rest();
    assert!(enabled(&w, "sheet.card.insert.ok"));
    assert_eq!(steps(&w), before.0, "nothing is sent while the boxes are ticked");
    press(&mut w, "sheet.card.insert.ok");
    w.rest();
    assert_eq!(card(&w), Value::Null, "OK closes the card");
    assert_eq!(steps(&w), before.0 + 1, "two sections, one undo step");
    let batches = sent(&w, "command.batch");
    assert_eq!(batches.len(), 1, "one batch: {batches:?}");
    let commands = batches[0].1["commands"].as_array().unwrap().clone();
    let ids: Vec<&str> = commands.iter().map(|c| c["command"].as_str().unwrap()).collect();
    assert_eq!(ids, ["sheet.add_section", "sheet.add_section"], "{commands:?}");
    assert_eq!(
        commands[0]["params"],
        json!({"id": 10, "page": 1, "section": "Prop"}),
        "in the card's order, which is the window's: Shape Data comes before Controls"
    );
    assert_eq!(commands[1]["params"], json!({"id": 10, "page": 1, "section": "Controls"}));
    let shown = labels(&w);
    assert!(shown.iter().any(|l| l == "Controls") && shown.iter().any(|l| l == "Shape Data"), "both sections are on the sheet: {shown:?}");
    assert!(footer(&w).contains("Inserted"), "the footer says what was done: {}", footer(&w));
    assert_eq!(selected(&w)["header"], json!(true), "the first section added is selected: {}", selected(&w));
    run_command(&mut w, "edit.undo", json!({}));
    let shown = labels(&w);
    assert!(!shown.iter().any(|l| l == "Controls") && !shown.iter().any(|l| l == "Shape Data"), "one undo took both away: {shown:?}");
}

/// A Geometry card entry adds the kind it names: Ellipse is a Geometry section with the variant.
#[test]
fn the_ellipse_and_infinite_line_boxes_add_a_geometry_section_of_that_kind() {
    let mut w = sheet_of(ShapeBuilder::new(10).cell("Width", "2"), [1400.0, 900.0]);
    select_cell(&mut w, "Width");
    run_command(&mut w, "window.sheetInsertSection", json!({}));
    press(&mut w, "sheet.card.insert.Ellipse");
    press(&mut w, "sheet.card.insert.ok");
    w.rest();
    let batch = sent(&w, "command.batch");
    assert_eq!(batch[0].1["commands"][0]["params"], json!({"id": 10, "page": 1, "section": "Geometry", "variant": "Ellipse"}), "{batch:?}");
    assert!(labels(&w).iter().any(|l| l == "Geometry 1"), "{:?}", labels(&w));
}

/// A card that is open with nothing to add says so and offers nothing, and a refusal of the engine is said in the card, which stays.
#[test]
fn the_card_says_what_the_engine_refused_and_stays_open() {
    let mut w = sheet_of(ShapeBuilder::new(10).cell("Width", "2"), [1400.0, 900.0]);
    select_cell(&mut w, "Width");
    run_command(&mut w, "window.sheetInsertSection", json!({}));
    press(&mut w, "sheet.card.insert.Scratch");
    // Another way adds the section between the card opening and OK: the engine then refuses a second Scratch.
    w.state_mut().execute("sheet.add_section", &json!({"id": 10, "page": 1, "section": "Scratch"})).unwrap();
    w.rest();
    let steps_before = steps(&w);
    press(&mut w, "sheet.card.insert.ok");
    w.rest();
    assert_eq!(card(&w)["kind"], json!("insertSection"), "the card stays open on a refusal");
    let error = card(&w)["error"].as_str().unwrap_or("").to_owned();
    assert!(error.contains("already has Scratch"), "the engine's words: {error}");
    assert_eq!(steps(&w), steps_before, "a refusal changes nothing");
    assert!(is_drawn(&w, "sheet.card.insert.error"), "the words are drawn in the card");
}

/// `window.sheetInsertSection` with `sections` adds them with no card (a program does what a person does), and `close` closes the card.
#[test]
fn window_sheetinsertsection_adds_named_sections_and_closes_its_card() {
    let mut w = sheet_of(ShapeBuilder::new(10).cell("Width", "2"), [1400.0, 900.0]);
    run_command(&mut w, "window.sheetInsertSection", json!({"sections": ["Scratch", "Hyperlinks"]}));
    assert_eq!(card(&w), Value::Null, "no card for a program that names the sections");
    assert!(labels(&w).iter().any(|l| l == "Scratch") && labels(&w).iter().any(|l| l == "Hyperlinks"), "{:?}", labels(&w));
    run_command(&mut w, "window.sheetInsertSection", json!({}));
    assert_eq!(card(&w)["kind"], json!("insertSection"));
    run_command(&mut w, "window.sheetInsertSection", json!({"close": true}));
    assert_eq!(card(&w), Value::Null);
    let e = w.state_mut().execute("window.sheetInsertSection", &json!({"sections": ["Nothing"]})).unwrap_err().to_string();
    assert!(e.contains("Nothing") && e.contains("sheet.sections"), "a name the card does not offer is refused in words: {e}");
}

// ---- Delete Section --------------------------------------------------------------------------------------------------------------

/// Delete (Sections) is greyed on Shape Transform, with the engine's words; on a section that may go it deletes the section of the selected cell with `sheet.delete_section`.
#[test]
fn delete_section_is_greyed_on_shape_transform_and_deletes_the_section_of_the_selected_cell() {
    let mut w = sheet_of(rich(), [1400.0, 900.0]);
    assert!(!enabled(&w, &control("ShapeSheetDeleteSection")), "nothing is selected");
    select_cell(&mut w, "Width");
    assert!(!enabled(&w, &control("ShapeSheetDeleteSection")), "Shape Transform cannot be deleted");
    assert_eq!(op(&w, "deleteSection")["ok"], json!(false));
    assert!(reason(&w, "deleteSection").contains("Shape Transform cannot be inserted or deleted"), "{}", reason(&w, "deleteSection"));
    let e = w.state_mut().execute("window.sheetDeleteSection", &json!({})).unwrap_err().to_string();
    assert!(e.contains("Shape Transform cannot be inserted or deleted"), "the command says it too: {e}");

    select_cell(&mut w, "Scratch.A2");
    assert!(enabled(&w, &control("ShapeSheetDeleteSection")), "a Scratch section may go");
    let before = steps(&w);
    press(&mut w, &control("ShapeSheetDeleteSection"));
    w.rest();
    assert_eq!(steps(&w), before + 1, "one undo step");
    assert_eq!(sent(&w, "sheet.delete_section").len(), 1);
    assert_eq!(sent(&w, "sheet.delete_section")[0].1, json!({"id": 10, "page": 1, "section": "Scratch"}));
    assert!(!labels(&w).iter().any(|l| l == "Scratch"), "the section is gone: {:?}", labels(&w));
    assert!(footer(&w).contains("Deleted"), "{}", footer(&w));
    assert_eq!(selected(&w), Value::Null, "the cell that was selected went with its section");
    run_command(&mut w, "edit.undo", json!({}));
    assert!(labels(&w).iter().any(|l| l == "Scratch"), "undo brings it back");
}

// ---- Insert Row and Delete Row ------------------------------------------------------------------------------------------------

/// Insert (Rows) adds a row after the selected one with `sheet.add_row`, selects the new row, and says what it did.
#[test]
fn insert_row_on_the_ribbon_adds_a_row_after_the_selected_one() {
    let mut w = sheet_of(rich(), [1400.0, 900.0]);
    assert!(!enabled(&w, &control("ShapeSheetInsertRow")), "no row is selected");
    assert!(reason(&w, "insertRow").contains("select"), "{}", reason(&w, "insertRow"));
    select_row(&mut w, "Scratch", 2);
    assert!(enabled(&w, &control("ShapeSheetInsertRow")));
    let before = steps(&w);
    press(&mut w, &control("ShapeSheetInsertRow"));
    w.rest();
    assert_eq!(steps(&w), before + 1, "one undo step");
    let calls = sent(&w, "sheet.add_row");
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].1, json!({"id": 10, "page": 1, "section": "Scratch", "after": 2}), "the ribbon's Insert goes after the selected row");
    assert_eq!(row_keys(&mut w, "Scratch"), [json!(1), json!(2), json!(3), json!(4)], "the new row is the third; the old third is the fourth");
    assert_eq!(selected(&w)["row"], json!(2), "the new row is selected: {}", selected(&w));
    assert_eq!(selected(&w)["cell"], Value::Null);
    assert!(footer(&w).contains("Inserted row 3"), "{}", footer(&w));
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(row_keys(&mut w, "Scratch"), [json!(1), json!(2), json!(3)]);
}

/// A cell of a row selected is in the row: Insert goes after that row too. A header selected adds at the end.
#[test]
fn insert_row_works_from_a_cell_of_a_row_and_from_a_header() {
    let mut w = sheet_of(rich(), [1400.0, 900.0]);
    select_cell(&mut w, "User.Row_1.Value");
    press(&mut w, &control("ShapeSheetInsertRow"));
    w.rest();
    assert_eq!(sent(&w, "sheet.add_row")[0].1, json!({"id": 10, "page": 1, "section": "User", "after": "Row_1"}));
    assert_eq!(row_keys_named(&mut w, "User"), ["Row_1", "Row_3", "Row_2"], "the next free name, in the place after Row_1");
    // A header is selected by a press on it (which folds the section): the row goes at the end.
    select_cell(&mut w, "Scratch.A1");
    press(&mut w, "sheet.section.Scratch");
    w.rest();
    assert_eq!(selected(&w)["header"], json!(true));
    run_command(&mut w, "window.sheetInsertRow", json!({}));
    assert_eq!(sent(&w, "sheet.add_row")[1].1, json!({"id": 10, "page": 1, "section": "Scratch"}), "a header selected: no row to go beside");
    assert_eq!(row_keys(&mut w, "Scratch"), [json!(1), json!(2), json!(3), json!(4)]);
    select_cell(&mut w, "Scratch.A1");
    run_command(&mut w, "window.sheetInsertRow", json!({"at": "before"}));
    assert_eq!(sent(&w, "sheet.add_row")[2].1, json!({"id": 10, "page": 1, "section": "Scratch", "before": 1}));
}

/// The names of the rows of a named section of shape 10.
fn row_keys_named(w: &mut Window, section: &str) -> Vec<String> {
    row_keys(w, section).into_iter().map(|k| k.as_str().unwrap_or("?").to_owned()).collect()
}

/// The right-click menu: Insert Row adds the row before the selected one, and in Geometry Insert Row After adds it after.
#[test]
fn the_right_click_menu_inserts_before_and_after_in_geometry() {
    let mut w = sheet_of(rich(), [1400.0, 900.0]);
    select_cell(&mut w, "Geometry1.X2");
    let at = rect_of(&w, "sheet.cell.Geometry1.X2").center();
    right_click(&mut w, at);
    for item in ["Insert Row", "Insert Row After", "Delete Row", "Insert Section", "Delete Section", "Formulas", "Values"] {
        assert!(is_drawn(&w, &format!("sheet.menu.{item}")), "the menu of a Geometry cell holds {item}");
        assert!(enabled(&w, &format!("sheet.menu.{item}")), "{item} works on a Geometry cell");
    }
    press(&mut w, "sheet.menu.Insert Row");
    w.rest();
    assert_eq!(
        sent(&w, "sheet.add_row")[0].1,
        json!({"id": 10, "page": 1, "section": "Geometry1", "before": 2}),
        "Insert Row goes before the selected row"
    );
    assert_eq!(
        row_keys(&mut w, "Geometry1"),
        [json!(1), json!(2), json!(3), json!(4), json!(5)],
        "the new row is row 2; the old rows 2 to 4 are 3 to 5"
    );
    assert_eq!(sheet(&w)["menu"], json!(false), "an item closes the menu");

    select_cell(&mut w, "Geometry1.X3");
    let at = rect_of(&w, "sheet.cell.Geometry1.X3").center();
    right_click(&mut w, at);
    press(&mut w, "sheet.menu.Insert Row After");
    w.rest();
    assert_eq!(sent(&w, "sheet.add_row")[1].1, json!({"id": 10, "page": 1, "section": "Geometry1", "after": 3}));
    assert_eq!(row_keys(&mut w, "Geometry1").len(), 6);
}

/// Insert Row After is for Geometry: the menu of a Scratch cell has no such item; a cell of a display group has the rows' items greyed.
#[test]
fn the_menu_has_insert_row_after_in_geometry_only_and_greys_what_a_group_of_cells_cannot_do() {
    let mut w = sheet_of(rich(), [1400.0, 900.0]);
    select_cell(&mut w, "Scratch.A1");
    let at = rect_of(&w, "sheet.cell.Scratch.A1").center();
    right_click(&mut w, at);
    assert!(is_drawn(&w, "sheet.menu.Insert Row") && enabled(&w, "sheet.menu.Insert Row"));
    assert!(!is_drawn(&w, "sheet.menu.Insert Row After"), "Insert Row After is Geometry's alone");
    tap(&mut w, Key::Escape);
    select_cell(&mut w, "Width");
    let at = rect_of(&w, "sheet.cell.Width").center();
    right_click(&mut w, at);
    assert!(!enabled(&w, "sheet.menu.Insert Row"), "Shape Transform has no rows");
    assert!(!enabled(&w, "sheet.menu.Delete Row"));
    assert!(!enabled(&w, "sheet.menu.Delete Section"), "Shape Transform cannot be deleted");
    assert!(enabled(&w, "sheet.menu.Insert Section"));
    assert!(enabled(&w, "sheet.menu.Formulas") && enabled(&w, "sheet.menu.Values"));
}

/// A right click on a row's number selects the row and opens the menu; on a section's header it opens the menu for the section.
#[test]
fn a_right_click_on_a_row_number_or_a_header_opens_the_menu_for_it() {
    let mut w = sheet_of(rich(), [1400.0, 900.0]);
    select_cell(&mut w, "Scratch.A1");
    let at = rect_of(&w, "sheet.row.Scratch.2").center();
    right_click(&mut w, at);
    assert_eq!(selected(&w)["row"], json!(1), "the row was selected: {}", selected(&w));
    assert_eq!(selected(&w)["cell"], Value::Null);
    assert!(is_drawn(&w, "sheet.menu.Delete Row") && enabled(&w, "sheet.menu.Delete Row"));
    press(&mut w, "sheet.menu.Delete Row");
    w.rest();
    assert_eq!(sent(&w, "sheet.delete_section").len() + sent(&w, "sheet.delete_row").len(), 1);
    assert_eq!(sent(&w, "sheet.delete_row")[0].1, json!({"id": 10, "page": 1, "section": "Scratch", "row": 2}));

    let at = rect_of(&w, "sheet.section.Scratch").center();
    right_click(&mut w, at);
    assert_eq!(selected(&w)["header"], json!(true), "{}", selected(&w));
    assert!(enabled(&w, "sheet.menu.Delete Section") && enabled(&w, "sheet.menu.Insert Section"));
    assert!(!enabled(&w, "sheet.menu.Delete Row"), "a header is no row");
}

/// Delete (Rows) deletes the selected row with `sheet.delete_row`: the rows below move up, the selection stays in its place, and one undo brings the row back.
#[test]
fn delete_row_deletes_the_selected_row() {
    let mut w = sheet_of(rich(), [1400.0, 900.0]);
    select_row(&mut w, "Scratch", 1);
    press(&mut w, "sheet.row.Scratch.2");
    w.rest();
    assert_eq!(selected(&w)["row"], json!(1), "a press on a row's number selects the row");
    assert!(enabled(&w, &control("ShapeSheetDeleteRow")));
    let before = steps(&w);
    press(&mut w, &control("ShapeSheetDeleteRow"));
    w.rest();
    assert_eq!(steps(&w), before + 1);
    assert_eq!(sent(&w, "sheet.delete_row")[0].1, json!({"id": 10, "page": 1, "section": "Scratch", "row": 2}));
    assert_eq!(row_keys(&mut w, "Scratch"), [json!(1), json!(2)], "the rows below moved up");
    assert!(footer(&w).contains("Deleted row 2"), "{}", footer(&w));
    assert_eq!(selected(&w)["row"], json!(1), "the selection stays where the row was: {}", selected(&w));
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(row_keys(&mut w, "Scratch").len(), 3);
}

/// A cell of a row selected is in the row, and a cell of a display group or a header is not a row.
#[test]
fn delete_row_needs_a_row() {
    let mut w = sheet_of(rich(), [1400.0, 900.0]);
    select_cell(&mut w, "Width");
    assert!(!enabled(&w, &control("ShapeSheetDeleteRow")));
    assert!(reason(&w, "deleteRow").contains("select a row"), "{}", reason(&w, "deleteRow"));
    select_cell(&mut w, "User.Row_2.Value");
    assert!(enabled(&w, &control("ShapeSheetDeleteRow")), "a cell of a row is in the row");
    press(&mut w, &control("ShapeSheetDeleteRow"));
    w.rest();
    assert_eq!(sent(&w, "sheet.delete_row")[0].1, json!({"id": 10, "page": 1, "section": "User", "row": "Row_2"}));
    assert_eq!(row_keys_named(&mut w, "User"), ["Row_1"]);
}

/// A text that is being typed is committed first, as a press on another cell commits it, and one the engine refuses stops the row from being inserted and stays in the editor.
#[test]
fn an_edit_that_is_open_is_committed_before_a_row_is_inserted_and_a_refused_one_stops_it() {
    let mut w = sheet_of(rich(), [1400.0, 900.0]);
    select_cell(&mut w, "Scratch.A1");
    typed(&mut w, "1/");
    let undo = steps(&w);
    press(&mut w, &control("ShapeSheetInsertRow"));
    w.rest();
    assert_eq!(sent(&w, "sheet.add_row").len(), 0, "a refused edit: no row was inserted");
    assert_ne!(sheet(&w)["edit"], Value::Null, "the edit stays open with its words");
    assert_eq!(sheet(&w)["edit"]["text"], json!("1/"));
    assert!(footer(&w).contains("the formula has an error"), "{}", footer(&w));
    assert_eq!(steps(&w), undo);
    typed(&mut w, "2");
    assert_eq!(sheet(&w)["edit"]["text"], json!("1/2"));
    press(&mut w, &control("ShapeSheetInsertRow"));
    w.rest();
    assert_eq!(sheet(&w)["edit"], Value::Null, "the text was committed");
    let sets = sent(&w, "sheet.set_cell");
    assert_eq!(sets.last().unwrap().1["formula"], json!("1/2"), "the text was sent first");
    assert_eq!(sent(&w, "sheet.add_row").len(), 1, "then the row was inserted");
    assert_eq!(steps(&w), undo + 2, "two steps: the formula, then the row");
    assert_eq!(row_keys(&mut w, "Scratch"), [json!(1), json!(2), json!(3), json!(4)]);
}

// ---- refusals (the engine's own) ---------------------------------------------------------------------------------

/// Each refusal of greys its control, with the engine's words: the rows the text makes, a section that cannot go.
#[test]
fn each_refusal_of_sheet_rows_1_greys_its_control_with_the_engines_words() {
    let shape = ShapeBuilder::new(10)
        .cell("Width", "2")
        .row_cell("Character", None, RowKey::Ix(0), "Size", "0.1")
        .row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "1")
        .row_cell("Tabs", None, RowKey::Ix(0), "Position", "1")
        .row_cell("Field", None, RowKey::Ix(0), "Format", "0");
    let mut w = sheet_of(shape, [1400.0, 900.0]);
    for (label, words) in
        [("Character", "made by the text"), ("Paragraph", "made by the text"), ("Tabs", "made by the text"), ("Text Fields", "made by the text")]
    {
        let name = section_named(&w, label);
        run_command(&mut w, "window.sheetSelect", json!({"section": name, "row": 1}));
        assert!(!enabled(&w, &control("ShapeSheetInsertRow")), "{label}: Insert Row is greyed");
        assert!(!enabled(&w, &control("ShapeSheetDeleteRow")), "{label}: Delete Row is greyed");
        assert!(reason(&w, "insertRow").contains(words), "{label}: {}", reason(&w, "insertRow"));
        assert!(reason(&w, "deleteRow").contains(words), "{label}: {}", reason(&w, "deleteRow"));
        assert!(!enabled(&w, &control("ShapeSheetDeleteSection")), "{label}: its section cannot go");
        let e = w.state_mut().execute("window.sheetInsertRow", &json!({})).unwrap_err().to_string();
        assert!(e.contains(words), "{label}: the command says it: {e}");
    }
    // A display group: no rows, and only Text Transform and Layer Membership may go.
    select_cell(&mut w, "Width");
    assert!(reason(&w, "insertRow").contains("group of cells"), "{}", reason(&w, "insertRow"));
    let mut tried = 0;
    for (label, cell) in
        [("Protection", "LockWidth"), ("Miscellaneous", "NoObjHandles"), ("Line Format", "LineWeight"), ("Fill Format", "FillForegnd")]
    {
        if !labels(&w).iter().any(|l| l == label) {
            continue;
        }
        select_cell(&mut w, cell);
        assert!(!enabled(&w, &control("ShapeSheetDeleteSection")), "{label} cannot be deleted");
        assert!(reason(&w, "deleteSection").contains("cannot be inserted or deleted"), "{label}: {}", reason(&w, "deleteSection"));
        tried += 1;
    }
    assert!(tried >= 2, "the sheet shows some of the display groups: {:?}", labels(&w));
}

/// A Geometry section that holds an Ellipse row holds that row alone, so Insert Row is greyed on it, and a Geometry section with a path is not.
#[test]
fn insert_row_is_greyed_on_a_geometry_section_that_holds_an_ellipse_row() {
    let shape = geometry_shape(10, 3)
        .row_type("Geometry", Some(1), RowKey::Ix(1), "Ellipse")
        .row_cell("Geometry", Some(1), RowKey::Ix(1), "X", "1")
        .row_cell("Geometry", Some(1), RowKey::Ix(1), "Y", "1")
        .row_cell("Geometry", Some(1), RowKey::Ix(1), "A", "2")
        .row_cell("Geometry", Some(1), RowKey::Ix(1), "B", "1")
        .row_cell("Geometry", Some(1), RowKey::Ix(1), "C", "1")
        .row_cell("Geometry", Some(1), RowKey::Ix(1), "D", "2");
    let mut w = sheet_of(shape, [1400.0, 900.0]);
    select_row(&mut w, "Geometry1", 2);
    assert!(enabled(&w, &control("ShapeSheetInsertRow")), "a path takes rows");
    select_row(&mut w, "Geometry2", 1);
    assert!(!enabled(&w, &control("ShapeSheetInsertRow")), "an Ellipse is alone in its section");
    assert!(reason(&w, "insertRow").contains("Ellipse") && reason(&w, "insertRow").contains("alone"), "{}", reason(&w, "insertRow"));
    let e = w.state_mut().execute("window.sheetInsertRow", &json!({})).unwrap_err().to_string();
    assert!(e.contains("alone"), "{e}");
    assert!(enabled(&w, &control("ShapeSheetDeleteRow")), "the row can still go");
}

/// A refusal of the engine that the window cannot know before it asks is shown in words in the footer and in `sheet.last`, and changes nothing: an insert in the middle of a master's
/// own rows.
#[test]
fn a_refusal_of_the_engine_is_said_in_the_footer_and_changes_nothing() {
    let master_shape = (0..3_u32).fold(ShapeBuilder::new(5), |s, k| s.row_cell("Scratch", None, RowKey::Ix(k), "A", &k.to_string()));
    let doc = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(master_shape))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let mut w = window_on(doc, [1400.0, 900.0]);
    run_command(&mut w, "window.shapesheet", json!({"of": "master", "master": 1, "id": 5}));
    run_command(&mut w, "window.tab", json!({"tab": "ShapeSheet Design"}));
    select_row(&mut w, "Scratch", 1);
    let anchor = (steps(&w), w.state().session().revision());
    press(&mut w, &control("ShapeSheetInsertRow"));
    w.rest();
    assert_eq!((steps(&w), w.state().session().revision()), anchor, "a refusal changes nothing");
    assert!(footer(&w).contains("add the row at the end"), "the footer says what the engine said: {}", footer(&w));
    assert_eq!(sheet(&w)["last"]["ok"], json!(false));
    assert!(sheet(&w)["last"]["error"].as_str().unwrap_or("").contains("add the row at the end"), "{}", sheet(&w)["last"]);
}

// ---- Edit Formula --------------------------------------------------------------------------------------------------------

fn edit_card_text(w: &Window) -> String {
    sheet(w)["edit"]["text"].as_str().unwrap_or("").to_owned()
}

/// Ctrl+E on a selected cell opens the Edit Formula card on that cell's formula; OK commits as Enter does (one `sheet.set_cell` with `force`), Cancel sends nothing.
#[test]
fn edit_formula_card_commits_as_enter_does() {
    let mut w =
        sheet_of(ShapeBuilder::new(10).cell_u("Width", "2", "IN").cell_u("Height", "1", "IN").cell("PinX", "4").cell("PinY", "4"), [1400.0, 900.0]);
    assert!(!enabled(&w, &control("ShapeSheetEditFormula")), "no cell is selected");
    select_cell(&mut w, "Width");
    assert!(enabled(&w, &control("ShapeSheetEditFormula")));
    press_key(&mut w, Key::E, Modifiers::COMMAND);
    assert_eq!(card(&w)["kind"], json!("editFormula"), "Ctrl+E opens the card: {}", card(&w));
    assert_eq!(card(&w)["cell"], json!("Width"));
    assert_eq!(edit_card_text(&w), "2 in", "on the cell's formula");
    for name in ["sheet.card.formula.text", "sheet.card.formula.ok", "sheet.card.formula.cancel"] {
        assert!(is_drawn(&w, name), "{name}");
    }
    // Cancel sends nothing.
    let (before, undo) = (sent(&w, "sheet.set_cell").len(), steps(&w));
    press(&mut w, "sheet.card.formula.cancel");
    w.rest();
    assert_eq!(card(&w), Value::Null);
    assert_eq!(sheet(&w)["edit"], Value::Null, "an edit the card began goes with it");
    assert_eq!((sent(&w, "sheet.set_cell").len(), steps(&w)), (before, undo), "Cancel sends nothing");

    // OK commits the text as Enter does.
    press(&mut w, &control("ShapeSheetEditFormula"));
    w.rest();
    run_command(&mut w, "window.sheetEditFormula", json!({"text": "3"}));
    press(&mut w, "sheet.card.formula.ok");
    w.rest();
    let calls = sent(&w, "sheet.set_cell");
    assert_eq!(calls.len(), before + 1, "one command for the commit");
    assert_eq!(calls.last().unwrap().1, json!({"id": 10, "page": 1, "cell": "Width", "formula": "3", "force": true}));
    assert_eq!(steps(&w), undo + 1, "one undo step");
    assert_eq!(card(&w), Value::Null, "the card closed");
    let width = sheet(&w)["painted"].as_array().unwrap().iter().find(|c| c["name"] == "Width").cloned().unwrap();
    assert_eq!(width["text"], json!("3 in"), "the unit of the cell is kept");
}

/// The card takes the keys of a text of several lines: typed text is added at the end, Enter adds a line and Ctrl+Enter is OK, Escape is Cancel.
#[test]
fn the_card_takes_typing_enter_adds_a_line_ctrl_enter_commits_and_escape_cancels() {
    let mut w = sheet_of(ShapeBuilder::new(10).cell_u("Width", "2", "IN").cell_u("Height", "1", "IN"), [1400.0, 900.0]);
    select_cell(&mut w, "Height");
    press_key(&mut w, Key::E, Modifiers::COMMAND);
    typed(&mut w, "+Width");
    assert_eq!(edit_card_text(&w), "1 in+Width", "the text was typed at the end of the formula");
    let undo = steps(&w);
    tap(&mut w, Key::Enter);
    assert!(edit_card_text(&w).contains('\n'), "Enter adds a line (a formula may have line breaks): {:?}", edit_card_text(&w));
    assert_eq!(card(&w)["kind"], json!("editFormula"), "and does not commit");
    assert_eq!(steps(&w), undo);
    typed(&mut w, "*2");
    press_key(&mut w, Key::Enter, Modifiers::COMMAND);
    assert_eq!(card(&w), Value::Null, "Ctrl+Enter is OK");
    assert_eq!(steps(&w), undo + 1);
    assert!(sent(&w, "sheet.set_cell").last().unwrap().1["formula"].as_str().unwrap().contains("*2"));

    select_cell(&mut w, "Width");
    press_key(&mut w, Key::E, Modifiers::COMMAND);
    typed(&mut w, "9");
    tap(&mut w, Key::Escape);
    assert_eq!(card(&w), Value::Null, "Escape is Cancel");
    assert_eq!(steps(&w), undo + 1, "and sends nothing");
}

/// A text the engine refuses keeps the card open with the fault marked and the words, and changes nothing; the mark goes when the text changes.
#[test]
fn a_refused_formula_keeps_the_card_open_with_the_words() {
    let mut w = sheet_of(ShapeBuilder::new(10).cell_u("Width", "2", "IN").cell_u("Height", "1", "IN"), [1400.0, 900.0]);
    select_cell(&mut w, "Height");
    run_command(&mut w, "window.sheetEditFormula", json!({"text": "1/"}));
    let (undo, revision) = (steps(&w), w.state().session().revision());
    press(&mut w, "sheet.card.formula.ok");
    w.rest();
    assert_eq!(card(&w)["kind"], json!("editFormula"), "the card stays open");
    assert_eq!((steps(&w), w.state().session().revision()), (undo, revision), "a refusal changes nothing");
    let fault = sheet(&w)["edit"]["fault"].clone();
    assert_eq!((fault["start"].as_u64(), fault["end"].as_u64()), (Some(1), Some(2)), "the fault is marked: {fault}");
    assert!(is_drawn(&w, "sheet.card.formula.words"), "the words are in the card");
    assert!(!is_drawn(&w, "sheet.card.formula.value"), "what is wrong is said once: the value waits until the text is right");
    assert!(!is_drawn(&w, "sheet.bar.fault"), "and the card says it, so the line under the bar does not");
    assert!(footer(&w).contains("the formula has an error"), "{}", footer(&w));
    typed(&mut w, "2");
    assert_eq!(sheet(&w)["edit"]["fault"], Value::Null, "the mark goes when the text changes");
    press(&mut w, "sheet.card.formula.ok");
    w.rest();
    assert_eq!(card(&w), Value::Null);
    assert_eq!(sent(&w, "sheet.set_cell").last().unwrap().1["formula"], json!("1/2"));
}

/// Ctrl+E while an edit is open takes the text typed so far to the card, and Cancel gives the text back to the cell's editor, which stays open.
#[test]
fn ctrl_e_in_an_open_edit_takes_the_text_to_the_card_and_cancel_gives_it_back() {
    let mut w = sheet_of(ShapeBuilder::new(10).cell_u("Width", "2", "IN"), [1400.0, 900.0]);
    select_cell(&mut w, "Width");
    tap(&mut w, Key::F2);
    typed(&mut w, "+1");
    assert_eq!(edit_card_text(&w), "2 in+1");
    press_key(&mut w, Key::E, Modifiers::COMMAND);
    assert_eq!(card(&w)["kind"], json!("editFormula"));
    assert_eq!(edit_card_text(&w), "2 in+1", "the card opens on the text typed so far");
    typed(&mut w, "+2");
    tap(&mut w, Key::Escape);
    assert_eq!(card(&w), Value::Null);
    assert_ne!(sheet(&w)["edit"], Value::Null, "the cell's edit is still open");
    assert_eq!(edit_card_text(&w), "2 in+1", "with the text it had when the card opened");
    tap(&mut w, Key::Escape);
}

// ---- the Editing group ------------------------------------------------------------------------------------------------------------

/// Shape Data adds a Shape Data row to the sheet (the section too, when the sheet has none), Link adds a Hyperlink row, and Shape Action adds a row to the selected Actions section.
#[test]
fn shape_data_link_and_shape_action_add_rows_to_the_sheet() {
    let mut w = sheet_of(ShapeBuilder::new(10).cell("Width", "2").cell("Height", "1"), [1400.0, 900.0]);
    for id in ["ShapeData", "HyperlinkInsert"] {
        assert!(enabled(&w, &control(id)), "{id} works on a shape's sheet");
    }
    assert!(!enabled(&w, &control("ShapeSheetShapeAction")), "Shape Action waits for an Actions section to be selected (dimmed in the image)");
    assert!(reason(&w, "shapeAction").contains("Actions"), "{}", reason(&w, "shapeAction"));

    let undo = steps(&w);
    press(&mut w, &control("ShapeData"));
    w.rest();
    assert_eq!(steps(&w), undo + 1);
    assert_eq!(
        sent(&w, "sheet.add_section")[0].1,
        json!({"id": 10, "page": 1, "section": "Prop"}),
        "the sheet had no Shape Data: the section is made, with its one row"
    );
    assert_eq!(row_keys_named(&mut w, "Prop"), ["Row_1"], "an unnamed Shape Data row is Row_n");
    press(&mut w, &control("ShapeData"));
    w.rest();
    assert_eq!(sent(&w, "sheet.add_row")[0].1, json!({"id": 10, "page": 1, "section": "Prop"}), "the section is there: a row at its end");
    assert_eq!(row_keys_named(&mut w, "Prop"), ["Row_1", "Row_2"]);
    assert_eq!(selected(&w)["row"], json!("Row_2"), "the new row is selected: {}", selected(&w));

    press(&mut w, &control("HyperlinkInsert"));
    w.rest();
    assert_eq!(sent(&w, "sheet.add_section")[1].1, json!({"id": 10, "page": 1, "section": "Hyperlink"}));
    assert!(labels(&w).iter().any(|l| l == "Hyperlinks"));

    // An Actions section, and a cell of it selected: Shape Action adds a row to it.
    w.state_mut().execute("sheet.add_section", &json!({"id": 10, "page": 1, "section": "Actions"})).unwrap();
    w.rest();
    assert!(!enabled(&w, &control("ShapeSheetShapeAction")), "a selection outside Actions leaves it dimmed");
    run_command(&mut w, "window.sheetSelect", json!({"section": "Actions", "row": "Row_1"}));
    assert!(enabled(&w, &control("ShapeSheetShapeAction")));
    press(&mut w, &control("ShapeSheetShapeAction"));
    w.rest();
    assert_eq!(sent(&w, "sheet.add_row").last().unwrap().1, json!({"id": 10, "page": 1, "section": "Actions"}));
    assert_eq!(row_keys_named(&mut w, "Actions").len(), 2);
}

/// The Editing group works on the other sheets too (a page, the document, a style hold Shape Data and Hyperlinks), and `window.sheetAddRow` refuses a section the group does not add to.
#[test]
fn the_editing_group_works_on_a_pages_sheet_and_the_command_refuses_other_sections() {
    let mut w = window_on(one_page(ShapeBuilder::new(10).cell("Width", "2")), [1400.0, 900.0]);
    run_command(&mut w, "window.shapesheet", json!({"of": "page"}));
    run_command(&mut w, "window.tab", json!({"tab": "ShapeSheet Design"}));
    assert!(enabled(&w, &control("ShapeData")));
    press(&mut w, &control("ShapeData"));
    w.rest();
    assert_eq!(sent(&w, "sheet.add_section")[0].1, json!({"page": 1, "section": "Prop"}));
    assert!(labels(&w).iter().any(|l| l == "Shape Data"), "{:?}", labels(&w));
    let e = w.state_mut().execute("window.sheetAddRow", &json!({"section": "Geometry1"})).unwrap_err().to_string();
    assert!(e.contains("Prop, Hyperlink or Actions"), "{e}");
    let e = w.state_mut().execute("window.sheetAddRow", &json!({})).unwrap_err().to_string();
    assert!(e.contains("missing `section`"), "{e}");
}

// ---- the commands, from the control channel ---------------------------------------------------------------------------------------------

/// The window's commands refuse in words, take no key they do not list, and every control of the three groups is a command of the registry.
#[test]
fn the_row_and_section_commands_refuse_in_words() {
    let mut w = window_on(one_page(rich()), [1400.0, 900.0]);
    for command in [
        "window.sheetInsertRow",
        "window.sheetDeleteRow",
        "window.sheetInsertSection",
        "window.sheetDeleteSection",
        "window.sheetEditFormula",
        "window.sheetAddRow",
    ] {
        let e = w.state_mut().execute(command, &json!({"section": "Prop"})).unwrap_err().to_string();
        assert!(e.contains("no ShapeSheet is open") || e.contains("unknown parameter"), "{command} with no sheet: {e}");
    }
    open_shape(&mut w, 10);
    let e = w.state_mut().execute("window.sheetInsertRow", &json!({})).unwrap_err().to_string();
    assert!(e.contains("select"), "nothing is selected: {e}");
    let e = w.state_mut().execute("window.sheetDeleteRow", &json!({})).unwrap_err().to_string();
    assert!(e.contains("select a row"), "{e}");
    let e = w.state_mut().execute("window.sheetEditFormula", &json!({})).unwrap_err().to_string();
    assert!(e.contains("select a cell"), "{e}");
    select_row(&mut w, "Scratch", 1);
    let e = w.state_mut().execute("window.sheetInsertRow", &json!({"at": "sideways"})).unwrap_err().to_string();
    assert!(e.contains("`at` must be after or before"), "{e}");
    let e = w.state_mut().execute("window.sheetInsertRow", &json!({"banana": 1})).unwrap_err().to_string();
    assert!(e.contains("unknown parameter"), "{e}");
    for id in [
        "window.sheetInsertRow",
        "window.sheetDeleteRow",
        "window.sheetInsertSection",
        "window.sheetDeleteSection",
        "window.sheetEditFormula",
        "window.sheetAddRow",
    ] {
        let spec = diacraft_ui_egui::commands::find_ui_command(id).unwrap_or_else(|| panic!("{id} is a command of the window's registry"));
        assert!(diacraft_engine::param_keys(spec.params).is_ok(), "{id}: the params doc is `name: description` entries");
    }
}

/// `window.sheetInsertRow` and `window.sheetDeleteRow` do what the ribbon's controls do, and name their answer in `sheet.last`.
#[test]
fn the_window_commands_do_what_the_controls_do_and_the_state_names_the_answer() {
    let mut w = sheet_of(rich(), [1400.0, 900.0]);
    select_row(&mut w, "Scratch", 3);
    run_command(&mut w, "window.sheetInsertRow", json!({"at": "after"}));
    assert_eq!(row_keys(&mut w, "Scratch").len(), 4);
    assert_eq!(sheet(&w)["last"]["ok"], json!(true));
    assert_eq!(sheet(&w)["last"]["answer"]["section"], json!("Scratch"));
    run_command(&mut w, "window.sheetDeleteRow", json!({}));
    assert_eq!(row_keys(&mut w, "Scratch").len(), 3);
    run_command(&mut w, "window.sheetSelect", json!({"section": "Scratch", "row": 1}));
    run_command(&mut w, "window.sheetDeleteSection", json!({}));
    assert!(!labels(&w).iter().any(|l| l == "Scratch"));
}

// ---- no clipping, no overlap, no repaint loop -------------------------------------------------------------------------------------------

/// The cards and the menu fit the window at 360, 640, 1024 and 1920 points: every control is inside the window, the buttons do not overlap, and the window comes to rest.
#[test]
fn the_cards_and_the_menu_fit_the_window_at_every_width() {
    for (width, height) in [(360.0_f32, 640.0_f32), (640.0, 720.0), (1024.0, 800.0), (1920.0, 1000.0)] {
        let mut w = sheet_of(rich(), [width, height]);
        let screen = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
        select_cell(&mut w, "Width");
        run_command(&mut w, "window.sheetInsertSection", json!({}));
        let names: Vec<String> = w.state().elements().iter().filter(|e| e.name.starts_with("sheet.card.insert.")).map(|e| e.name.clone()).collect();
        assert!(names.len() > 10, "{width}: the card lists the sections: {names:?}");
        for name in &names {
            let rect = rect_of(&w, name);
            assert!(screen.contains_rect(rect), "{width}: {name} is inside the window: {rect:?} in {screen:?}");
        }
        let (ok, cancel) = (rect_of(&w, "sheet.card.insert.ok"), rect_of(&w, "sheet.card.insert.cancel"));
        assert!(!overlaps(ok, cancel), "{width}: OK and Cancel overlap: {ok:?} {cancel:?}");
        let boxes: Vec<Rect> =
            names.iter().filter(|n| !n.ends_with(".ok") && !n.ends_with(".cancel") && !n.ends_with(".error")).map(|n| rect_of(&w, n)).collect();
        for (i, a) in boxes.iter().enumerate() {
            for b in &boxes[i + 1..] {
                assert!(!overlaps(*a, *b), "{width}: two boxes overlap: {a:?} {b:?}");
            }
            assert!(!overlaps(*a, ok) && !overlaps(*a, cancel), "{width}: a box overlaps a button: {a:?}");
        }
        run_command(&mut w, "window.sheetInsertSection", json!({"close": true}));

        run_command(&mut w, "window.sheetEditFormula", json!({"text": &"Width*2+".repeat(40)}));
        for name in ["sheet.card.formula.text", "sheet.card.formula.ok", "sheet.card.formula.cancel"] {
            let rect = rect_of(&w, name);
            assert!(screen.contains_rect(rect), "{width}: {name} is inside the window: {rect:?}");
        }
        let (ok, cancel) = (rect_of(&w, "sheet.card.formula.ok"), rect_of(&w, "sheet.card.formula.cancel"));
        assert!(!overlaps(ok, cancel), "{width}: OK and Cancel overlap");
        assert!(!overlaps(rect_of(&w, "sheet.card.formula.text"), ok), "{width}: the text and the buttons overlap");
        run_command(&mut w, "window.sheetEditFormula", json!({"close": true}));
        assert_eq!(card(&w), Value::Null);

        // The menu, opened by the key, over a cell near the right edge.
        select_cell(&mut w, "Geometry1.X2");
        press_key(&mut w, Key::F10, Modifiers::SHIFT);
        for item in ["Insert Row", "Insert Row After", "Delete Row", "Insert Section", "Delete Section", "Formulas", "Values"] {
            let rect = rect_of(&w, &format!("sheet.menu.{item}"));
            assert!(screen.contains_rect(rect), "{width}: the menu's {item} is inside the window: {rect:?}");
        }
        w.rest();
    }
}

/// The three groups of the tab hold at 640, 1024 and 1920 points: each control is named and drawn (the ribbon folds a group that does not fit into one button).
#[test]
fn the_sections_rows_and_editing_groups_are_drawn_and_named() {
    let mut w = sheet_of(rich(), [1400.0, 900.0]);
    select_row(&mut w, "Scratch", 1);
    for id in [
        "ShapeSheetInsertSection",
        "ShapeSheetDeleteSection",
        "ShapeSheetInsertRow",
        "ShapeSheetDeleteRow",
        "ShapeSheetEditFormula",
        "ShapeData",
        "HyperlinkInsert",
        "ShapeSheetShapeAction",
    ] {
        assert!(is_drawn(&w, &control(id)), "{id} is on the tab");
    }
    let enabled_now: Vec<(&str, bool)> = [
        "ShapeSheetInsertSection",
        "ShapeSheetDeleteSection",
        "ShapeSheetInsertRow",
        "ShapeSheetDeleteRow",
        "ShapeSheetEditFormula",
        "ShapeData",
        "HyperlinkInsert",
        "ShapeSheetShapeAction",
    ]
    .into_iter()
    .map(|id| (id, enabled(&w, &control(id))))
    .collect();
    assert_eq!(
        enabled_now,
        [
            ("ShapeSheetInsertSection", true),
            ("ShapeSheetDeleteSection", true),
            ("ShapeSheetInsertRow", true),
            ("ShapeSheetDeleteRow", true),
            ("ShapeSheetEditFormula", false),
            ("ShapeData", true),
            ("HyperlinkInsert", true),
            ("ShapeSheetShapeAction", false),
        ],
        "a Scratch row selected: a row, not a cell"
    );
}
