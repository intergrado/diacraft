//! The ShapeSheet window, editing: the cell editor, the formula bar, the keyboard, and what the window says of a commit.
//!
//! The design is the ShapeSheet window's (5.8 for the editing, 5.9 for the keys, 5.11 for the feedback) and the spec of the window is
//! (for how a sheet is laid out, for what a typed text does, R38 for a cut text, for
//! the keys). The window edits only through the engine's `sheet.set_cell`, one undo step each; what a test asks of the engine's own rules is what
//! `crates/engine/tests/it/sheet_write.rs` holds, and these tests hold what the window sends, shows and keeps.

use diacraft_doc::RowKey;
use diacraft_testkit::build::ShapeBuilder;
use egui::{Event, Key, Modifiers, Rect};
use serde_json::{Value, json};

use crate::sheet_window::{
    data_shape, flow_window, geometry_shape, is_drawn, labels, one_page, open_shape, overlaps, painted, rect_of, right_click, sheet, window_on,
};
use crate::support::{Rest, Window, click, drag, elements_named, fast_window, move_to, press, run_command};

// ---- the windows these tests open and the keys they press ------------------------------------------------------------------------------

/// A shape with a Width of `2 in` that is a plain value, a Height, a pin, and a LocPinX that reads the Width.
fn boxed() -> ShapeBuilder {
    ShapeBuilder::new(10).cell_u("Width", "2", "IN").cell_u("Height", "1", "IN").cell("PinX", "4").cell("PinY", "4").formula(
        "LocPinX",
        "Width*0.5",
        "1",
    )
}

/// A window on a drawing with `shape` as shape 10, its sheet open and the sheet's focus.
fn sheet_of(shape: ShapeBuilder) -> Window {
    let mut w = window_on(one_page(shape), [1400.0, 900.0]);
    open_shape(&mut w, 10);
    w
}

fn select(w: &mut Window, cell: &str) {
    run_command(w, "window.sheetSelect", json!({"cell": cell}));
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

/// Text typed as a keyboard sends it: the text event of a printable key.
fn typed(w: &mut Window, text: &str) {
    w.input_mut().events.push(Event::Text(text.to_owned()));
    w.step();
    w.rest();
}

fn edit(w: &Window) -> Value {
    sheet(w)["edit"].clone()
}

fn selected(w: &Window) -> Value {
    sheet(w)["selected"].clone()
}

/// The cell the selection is on, by its name.
fn on(w: &Window) -> String {
    selected(w)["cell"].as_str().unwrap_or("").to_owned()
}

/// The formula a cell box shows, as the last frame painted it.
fn shown(w: &Window, name: &str) -> String {
    painted(&sheet(w), name)["text"].as_str().unwrap_or("").to_owned()
}

/// What the window sent the engine to write a cell, in order.
fn sent(w: &Window) -> Vec<Value> {
    w.state().session().journal().iter().filter(|(id, _)| id == "sheet.set_cell").map(|(_, params)| params.clone()).collect()
}

/// The steps of the drawing's history that an undo goes back over.
fn steps(w: &Window) -> usize {
    w.state().session().history().map_or(0, |h| h.undo_len())
}

fn footer(w: &Window) -> String {
    sheet(w)["footer"].as_str().unwrap_or("").to_owned()
}

/// The texts egui was asked to put on the system clipboard in the last frame.
fn system_copies(w: &Window) -> Vec<String> {
    w.output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// A window whose frames are 1/60 s apart (two presses in a row are a double click), on `shape` as shape 10.
fn fast_sheet_of(shape: ShapeBuilder) -> Window {
    use diacraft_engine::Session;
    use diacraft_ui_egui::DiacraftApp;
    use diacraft_ui_egui::render_worker::bundled_drawer;
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let doc = one_page(shape);
    session
        .edit("build", |d| {
            *d = doc;
            Ok(())
        })
        .unwrap();
    let mut w = fast_window(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()), [1400.0, 900.0]);
    w.rest();
    open_shape(&mut w, 10);
    w
}

// ---- typing a formula (design 5.8; of the window spec) -----------------------------------------------------------------------

/// V1 of the inventory: typing `3` and Enter in a Width holding `2 in` sends **one** `sheet.set_cell` with the tab's object, the cell's reference, the text and `force: true`
/// (a formula typed in the sheet replaces a guarded one), as one undo step, and the cell shows `3 in`.
#[test]
fn typing_3_and_enter_in_a_width_holding_2_in_sends_one_set_cell_with_force_and_shows_3_in() {
    let mut w = sheet_of(boxed());
    select(&mut w, "Width");
    assert_eq!(shown(&w, "Width"), "2 in");
    let (before, undo) = (sent(&w).len(), steps(&w));
    typed(&mut w, "3");
    assert_eq!(edit(&w)["text"], json!("3"), "the key began an edit with its character: {}", edit(&w));
    assert_eq!(edit(&w)["typing"], json!(true), "an edit begun by typing (the arrows then commit)");
    assert_eq!(sent(&w).len(), before, "nothing is sent while the text is typed");
    tap(&mut w, Key::Enter);
    let sent_now = sent(&w);
    assert_eq!(sent_now.len(), before + 1, "one command for the commit");
    assert_eq!(sent_now.last(), Some(&json!({"id": 10, "page": 1, "cell": "Width", "formula": "3", "force": true})));
    assert_eq!(steps(&w), undo + 1, "one undo step");
    assert_eq!(shown(&w, "Width"), "3 in", "the unit of the cell is kept");
    assert_eq!(edit(&w), Value::Null, "the editor closed");
    assert_eq!(on(&w), "Height", "Enter moves down, as in a spreadsheet");
}

/// Escape and an unchanged Enter send nothing: no command, no undo step (`original` of GridCraft's editor).
#[test]
fn escape_and_an_unchanged_enter_send_nothing() {
    let mut w = sheet_of(boxed());
    select(&mut w, "Width");
    let (before, undo, revision) = (sent(&w).len(), steps(&w), w.state().session().revision());
    tap(&mut w, Key::F2);
    assert_eq!(edit(&w)["text"], json!("2 in"), "F2 edits the formula's own text");
    assert_eq!(edit(&w)["typing"], json!(false), "an edit begun by F2: the arrows move the caret");
    tap(&mut w, Key::Escape);
    assert_eq!(edit(&w), Value::Null, "Escape cancels");
    assert_eq!(sheet(&w)["focus"], json!(true), "and keeps the sheet's focus");
    tap(&mut w, Key::F2);
    tap(&mut w, Key::Enter);
    assert_eq!(edit(&w), Value::Null);
    typed(&mut w, "9");
    tap(&mut w, Key::Escape);
    assert_eq!((sent(&w).len(), steps(&w), w.state().session().revision()), (before, undo, revision), "nothing was sent, no step, no revision");
    assert_eq!(shown(&w, "Width"), "2 in");
}

/// V2 of the inventory: Delete on an instance's Width opens an edit with no text and Enter clears it; the cell follows its master again.
#[test]
fn delete_and_enter_clear_an_instances_width_which_follows_the_master_again() {
    let (mut w, process, _, _) = flow_window([1400.0, 900.0]);
    open_shape(&mut w, process);
    let looks = |w: &Window| {
        let cell = painted(&sheet(w), "Width").clone();
        (cell["text"].clone(), cell["shown"].clone())
    };
    let master_width = looks(&w);
    w.state_mut().execute("shape.resize", &json!({"id": process, "x": 3.0, "y": 3.0, "width": 3.5, "height": 1.0})).unwrap();
    w.rest();
    assert_eq!(painted(&sheet(&w), "Width")["origin"], json!("local"), "a resize made the Width the shape's own");
    select(&mut w, "Width");
    tap(&mut w, Key::Delete);
    assert_eq!(edit(&w)["text"], json!(""), "Delete opens an edit with no text");
    let before = sent(&w).len();
    tap(&mut w, Key::Enter);
    let last = sent(&w).last().cloned().unwrap();
    assert_eq!(sent(&w).len(), before + 1);
    assert_eq!(last["formula"], json!(""), "{last}");
    assert_eq!(last["cell"], json!("Width"));
    let width = painted(&sheet(&w), "Width").clone();
    assert_eq!(width["origin"], json!("inherited"), "the instance follows its master again: {width}");
    assert_eq!(looks(&w), master_width, "with the master's formula and value");
    // Backspace opens the same edit.
    select(&mut w, "Height");
    tap(&mut w, Key::Backspace);
    assert_eq!(edit(&w)["text"], json!(""));
    tap(&mut w, Key::Escape);
}

// ---- a refused formula (design 5.8: the fault is marked, the words are said) ----------------------------------------------------------

/// `1/` and Enter keeps the editor open with the fault marked and the words; the mark goes when the text changes, and the text that then parses is written.
#[test]
fn a_formula_that_does_not_parse_keeps_the_editor_open_with_the_fault_marked_and_the_words() {
    let mut w = sheet_of(boxed());
    select(&mut w, "Height");
    let (undo, revision) = (steps(&w), w.state().session().revision());
    typed(&mut w, "1/");
    tap(&mut w, Key::Enter);
    let e = edit(&w);
    assert_ne!(e, Value::Null, "the editor stays open");
    assert_eq!(e["text"], json!("1/"), "with the text as it was typed");
    let fault = e["fault"].clone();
    assert_eq!((fault["start"].as_u64(), fault["end"].as_u64()), (Some(1), Some(2)), "the text ended early: the last character is marked: {fault}");
    let words = fault["words"].as_str().unwrap_or("");
    assert!(words.contains("something is due at character 3"), "the words of the fault: {words}");
    assert!(footer(&w).contains("the formula has an error"), "the footer says what the command said: {}", footer(&w));
    assert!(is_drawn(&w, "sheet.bar.fault"), "a line under the bar says what is wrong");
    assert_eq!((steps(&w), w.state().session().revision()), (undo, revision), "a refusal changes nothing");
    assert_eq!(on(&w), "Height", "and the selection does not move");

    typed(&mut w, "2");
    let e = edit(&w);
    assert_eq!(e["text"], json!("1/2"));
    assert_eq!(e["fault"], Value::Null, "the mark goes when the text changes");
    assert!(!is_drawn(&w, "sheet.bar.fault"));
    tap(&mut w, Key::Enter);
    assert_eq!(edit(&w), Value::Null);
    assert_eq!(sent(&w).last().unwrap()["formula"], json!("1/2"));
    assert_eq!(shown(&w, "Height"), "1/2");
}

/// The words of the command's own refusal are shown to the person, in the footer, and the editor stays open: a shape on a locked layer.
#[test]
fn the_refusal_of_the_command_is_said_in_the_footer_and_the_editor_stays_open() {
    let (mut w, process, _, _) = flow_window([1400.0, 900.0]);
    open_shape(&mut w, process);
    run_command(&mut w, "layer.add", json!({"name": "Held"}));
    run_command(&mut w, "layer.assign", json!({"ids": [process], "layers": ["Held"]}));
    run_command(&mut w, "layer.set", json!({"layer": "Held", "lock": true}));
    select(&mut w, "PinX");
    let undo = steps(&w);
    typed(&mut w, "9");
    tap(&mut w, Key::Enter);
    assert!(footer(&w).contains("which is locked"), "{}", footer(&w));
    assert_ne!(edit(&w), Value::Null, "the editor stays open");
    assert_eq!(edit(&w)["fault"], Value::Null, "the refusal is no fault of the text");
    assert_eq!(steps(&w), undo);
    assert_eq!(sheet(&w)["last"]["ok"], json!(false));
    tap(&mut w, Key::Escape);
    assert_eq!(edit(&w), Value::Null);
}

// ---- the result while typing (design 5.8) ------------------------------------------------------------------------------------------------

/// The bar's value changes as the text does, each change of the text asks `sheet.evaluate`, and nothing is written (the revision is as it was).
#[test]
fn the_bars_live_value_changes_as_the_text_does_and_the_revision_does_not() {
    let mut w = sheet_of(boxed());
    select(&mut w, "Width");
    let revision = w.state().session().revision();
    typed(&mut w, "Height*3");
    assert_eq!(edit(&w)["live"]["value"], json!("3.0000 in"), "{}", edit(&w));
    assert!(is_drawn(&w, "sheet.bar.value"), "the value is at the right of the bar");
    typed(&mut w, "+1");
    assert_eq!(edit(&w)["text"], json!("Height*3+1"));
    assert_eq!(edit(&w)["live"]["value"], json!("4.0000 in"), "{}", edit(&w));
    typed(&mut w, "/0");
    assert_eq!(edit(&w)["live"]["error"], json!(true), "an error the text works out to: {}", edit(&w));
    assert_eq!(edit(&w)["live"]["value"], json!("#DIV/0!"));
    assert_eq!(edit(&w)["fault"], Value::Null, "a live result is no fault mark");
    typed(&mut w, "(");
    assert!(
        edit(&w)["live"]["fault"].as_str().is_some_and(|f| f.contains("character")),
        "a text that does not parse says so at the right of the bar: {}",
        edit(&w)
    );
    assert_eq!(w.state().session().revision(), revision, "a query writes nothing");
    assert_eq!(sent(&w).len(), 0);
    // A frame with the text as it was asks nothing again.
    let work = sheet(&w)["work"]["evaluations"].as_u64().unwrap();
    for _ in 0..3 {
        w.step();
    }
    assert_eq!(sheet(&w)["work"]["evaluations"].as_u64().unwrap(), work, "a still frame does not evaluate again");
}

// ---- copy and paste a formula (design 5.8) -----------------------------------------------------------------------------------------------

#[test]
fn ctrl_c_and_ctrl_v_copy_a_formula_from_one_cell_to_another() {
    let mut w = sheet_of(boxed());
    select(&mut w, "LocPinX");
    w.input_mut().events.push(Event::Copy);
    w.step();
    assert_eq!(system_copies(&w), ["Width*0.5"], "Ctrl+C puts the formula's text on the clipboard");
    assert!(w.state().session().clipboard().is_none(), "and copies no shape of the drawing");
    w.rest();
    select(&mut w, "Height");
    let undo = steps(&w);
    w.input_mut().events.push(Event::Paste("Width*0.5".to_owned()));
    w.step();
    w.rest();
    assert_eq!(shown(&w, "Height"), "Width*0.5", "Ctrl+V commits the clipboard's text as the cell's formula");
    assert_eq!(steps(&w), undo + 1, "as one step");
    assert_eq!(sent(&w).last().unwrap()["force"], json!(true));
    // Inside an edit a paste goes in at the caret and sends nothing.
    select(&mut w, "PinY");
    let pin = shown(&w, "PinY");
    tap(&mut w, Key::F2);
    let before = sent(&w).len();
    w.input_mut().events.push(Event::Paste("+1".to_owned()));
    w.step();
    w.rest();
    assert_eq!(edit(&w)["text"], json!(format!("{pin}+1")), "{}", edit(&w));
    assert_eq!(sent(&w).len(), before);
    tap(&mut w, Key::Escape);
}

/// Ctrl+Z with the sheet's focus undoes the drawing's last step, and the sheet shows the formula as it was.
#[test]
fn ctrl_z_shows_the_old_formula_and_ctrl_y_the_new_one() {
    let mut w = sheet_of(boxed());
    select(&mut w, "Width");
    typed(&mut w, "3");
    tap(&mut w, Key::Enter);
    assert_eq!(shown(&w, "Width"), "3 in");
    press_key(&mut w, Key::Z, Modifiers::COMMAND);
    assert_eq!(shown(&w, "Width"), "2 in", "the old formula is shown");
    press_key(&mut w, Key::Y, Modifiers::COMMAND);
    assert_eq!(shown(&w, "Width"), "3 in");
}

// ---- the marks of what a commit worked out again (design 5.11) ------------------------------------------------------

/// `LocPinX` reads Width: it has the changed mark after a write of Width, the footer says how many cells were worked out again, and the mark goes at the next commit. No timer.
#[test]
fn a_cell_a_commit_worked_out_again_has_the_changed_mark_until_the_next_commit() {
    let mut w = sheet_of(boxed());
    assert_eq!(painted(&sheet(&w), "LocPinX")["changed"], json!(false), "no mark before a commit");
    select(&mut w, "Width");
    typed(&mut w, "3");
    tap(&mut w, Key::Enter);
    assert_eq!(painted(&sheet(&w), "LocPinX")["changed"], json!(true), "LocPinX was worked out again");
    assert_eq!(painted(&sheet(&w), "Width")["changed"], json!(false), "the cell typed over is no mark: it is the person's own");
    assert_eq!(painted(&sheet(&w), "PinX")["changed"], json!(false));
    assert!(footer(&w).contains("Worked out again: "), "{}", footer(&w));
    assert!(footer(&w).contains("on this sheet"), "{}", footer(&w));
    assert_eq!(sheet(&w)["changed"], json!(["LocPinX"]));
    // Still frames keep it: no timer takes it away.
    for _ in 0..5 {
        w.step();
    }
    assert_eq!(painted(&sheet(&w), "LocPinX")["changed"], json!(true));
    assert!(w.state().pending_input() == 0);
    // The next commit takes it away: PinX is read by nobody.
    select(&mut w, "PinX");
    typed(&mut w, "5");
    tap(&mut w, Key::Enter);
    assert_eq!(painted(&sheet(&w), "LocPinX")["changed"], json!(false), "the next commit loses it");
    assert_eq!(sheet(&w)["changed"], json!([]));
}

/// A commit that makes a cell read an error says which in the footer ("Width now reads #DIV/0!"), and a function the evaluator does not have is said too.
#[test]
fn the_footer_names_a_cell_that_now_reads_an_error() {
    let mut w = sheet_of(boxed());
    select(&mut w, "Width");
    typed(&mut w, "1/0");
    tap(&mut w, Key::Enter);
    assert!(footer(&w).contains("Width now reads #DIV/0!"), "{}", footer(&w));
    assert_eq!(painted(&sheet(&w), "Width")["shown"], json!("#DIV/0!"));
    assert_eq!(sheet(&w)["last"]["ok"], json!(true), "the formula parsed: the command did not refuse it");
    assert_eq!(sheet(&w)["last"]["answer"]["error"], json!("#DIV/0!"));
    // R56 (calc) wins over the design's V4: a cell that reads the failing cell reads its last good value, with no error.
    let loc = painted(&sheet(&w), "LocPinX").clone();
    assert_ne!(loc["valueInk"], json!("error"), "LocPinX reads Width's last good value: {loc}");
    assert_eq!(loc["origin"], json!("local"));
    // The selection moved, and so the footer was said by the commit and stays until the person moves again.
    select(&mut w, "PinX");
    assert!(!footer(&w).contains("Width now reads"), "a move of the selection says where the cell comes from instead: {}", footer(&w));
}

// ---- the keyboard (design 5.9) ------------------------------------------------------------------------------------------

/// A plain shape's Shape Transform lays its ten cells out as name and value pairs in three columns filled down each in turn: Width, Height, Angle; PinX, PinY, LocPinX,
/// LocPinY; FlipX, FlipY, ResizeMode. The arrows move one cell, up and down in a column, left and right in a line, and on across the sections.
#[test]
fn the_arrows_move_one_cell_in_the_pairs_of_a_group_and_on_across_the_sections() {
    let mut w = sheet_of(boxed());
    select(&mut w, "Width");
    tap(&mut w, Key::ArrowDown);
    assert_eq!(on(&w), "Height");
    tap(&mut w, Key::ArrowDown);
    assert_eq!(on(&w), "Angle");
    tap(&mut w, Key::ArrowRight);
    assert_eq!(on(&w), "LocPinX", "the second column's third cell");
    tap(&mut w, Key::ArrowRight);
    assert_eq!(on(&w), "ResizeMode");
    tap(&mut w, Key::ArrowRight);
    assert_eq!(on(&w), "ResizeMode", "the end of the line");
    tap(&mut w, Key::ArrowLeft);
    tap(&mut w, Key::ArrowLeft);
    assert_eq!(on(&w), "Angle");
    tap(&mut w, Key::ArrowUp);
    tap(&mut w, Key::ArrowUp);
    assert_eq!(on(&w), "Width");
    // Up from the first cell of a section is the section's header; Down from the last cell of a column is the next section's header, and then its first cell.
    tap(&mut w, Key::ArrowUp);
    assert_eq!(selected(&w)["header"], json!(true), "{}", selected(&w));
    assert_eq!(selected(&w)["section"], json!("Shape Transform"));
    tap(&mut w, Key::ArrowDown);
    assert_eq!(on(&w), "Width");
    for _ in 0..2 {
        tap(&mut w, Key::ArrowDown);
    }
    assert_eq!(on(&w), "Angle");
    tap(&mut w, Key::ArrowDown);
    assert_eq!(selected(&w)["header"], json!(true), "the column ends: the next section's header: {}", selected(&w));
    assert_eq!(selected(&w)["section"].as_str(), Some(labels(&w)[1].as_str()));
    tap(&mut w, Key::ArrowDown);
    assert_eq!(selected(&w)["header"], json!(false));
    assert_eq!(selected(&w)["section"].as_str(), Some(labels(&w)[1].as_str()), "the first cell of the next section");
}

#[test]
fn tab_and_shift_tab_go_by_reading_order_and_home_and_end_to_the_ends_of_the_line() {
    let mut w = sheet_of(boxed());
    select(&mut w, "Width");
    let mut path = vec![on(&w)];
    for _ in 0..4 {
        tap(&mut w, Key::Tab);
        path.push(on(&w));
    }
    assert_eq!(path, ["Width", "PinX", "FlipX", "Height", "PinY"], "across a line, then on to the next");
    press_key(&mut w, Key::Tab, Modifiers::SHIFT);
    assert_eq!(on(&w), "Height", "Shift+Tab goes back");
    select(&mut w, "FlipY");
    tap(&mut w, Key::Home);
    assert_eq!(on(&w), "Height", "Home: the first cell of the line");
    tap(&mut w, Key::End);
    assert_eq!(on(&w), "FlipY", "End: the last");
    press_key(&mut w, Key::Home, Modifiers::COMMAND);
    assert_eq!(on(&w), "Width", "Ctrl+Home: the first cell of the sheet");
    press_key(&mut w, Key::End, Modifiers::COMMAND);
    let last = selected(&w);
    let sections = sheet(&w)["sections"].clone();
    let last_section = sections.as_array().unwrap().last().unwrap()["name"].clone();
    assert_eq!(last["section"], last_section, "Ctrl+End: the last cell of the sheet is in the last section: {last}");
}

#[test]
fn enter_moves_down_f2_and_a_printable_key_edit_and_the_menu_key_opens_the_menu() {
    let mut w = sheet_of(boxed());
    select(&mut w, "Width");
    tap(&mut w, Key::Enter);
    assert_eq!(on(&w), "Height", "Enter: the cell below");
    tap(&mut w, Key::F2);
    assert_eq!(edit(&w)["cell"], json!("Height"));
    tap(&mut w, Key::Escape);
    typed(&mut w, "7");
    assert_eq!(edit(&w)["text"], json!("7"), "a printable key edits, starting with its character");
    tap(&mut w, Key::Escape);
    press_key(&mut w, Key::F10, Modifiers::SHIFT);
    assert!(is_drawn(&w, "sheet.menu.Formulas") && is_drawn(&w, "sheet.menu.Values"), "Shift+F10: the cell's right-click menu holds the two toggles");
    tap(&mut w, Key::Escape);
    assert!(!is_drawn(&w, "sheet.menu.Formulas"), "Escape closes the menu");
    assert_eq!(sheet(&w)["focus"], json!(true), "and no more than that");
}

/// Space or Enter on a section header folds the section or opens it; Escape gives the focus back to the drawing.
#[test]
fn space_and_enter_on_a_header_fold_it_and_escape_gives_the_focus_back_to_the_drawing() {
    let mut w = sheet_of(boxed());
    select(&mut w, "Width");
    tap(&mut w, Key::ArrowUp);
    assert_eq!(selected(&w)["header"], json!(true));
    let open = |w: &Window| sheet(w)["sections"][0]["open"].clone();
    assert_eq!(open(&w), json!(true));
    tap(&mut w, Key::Space);
    assert_eq!(open(&w), json!(false), "Space on the header folds the section");
    assert_eq!(edit(&w), Value::Null, "and the space begins no edit");
    tap(&mut w, Key::Enter);
    assert_eq!(open(&w), json!(true), "Enter opens it");
    tap(&mut w, Key::Escape);
    assert_eq!(sheet(&w)["focus"], json!(false), "Escape: the focus goes back to the drawing");
    assert!(!is_drawn(&w, "tab.ShapeSheet Design"), "and the contextual tab goes with it");
}

/// Page Up and Page Down scroll a screen and move the selection with it; Ctrl with them scrolls sideways.
#[test]
fn page_up_and_page_down_scroll_a_screen_and_ctrl_with_them_scrolls_sideways() {
    let mut w = window_on(one_page(geometry_shape(10, 400)), [1400.0, 900.0]);
    open_shape(&mut w, 10);
    run_command(&mut w, "window.sheetSelect", json!({"section": "Geometry1", "row": 1, "cell": "X"}));
    let (first, page) = {
        let s = sheet(&w);
        (s["first"].as_u64().unwrap(), s["visible"][1].as_u64().unwrap() - s["visible"][0].as_u64().unwrap())
    };
    assert!(page > 5, "{page}");
    tap(&mut w, Key::PageDown);
    let moved = sheet(&w)["first"].as_u64().unwrap() - first;
    assert!(moved + 2 >= page && moved <= page, "a screen: moved {moved} lines of {page}");
    assert_eq!(selected(&w)["row"], json!(1 + moved), "the selection moves with the screen: {}", selected(&w));
    tap(&mut w, Key::PageUp);
    assert_eq!(sheet(&w)["first"].as_u64().unwrap(), first, "a screen back");
    assert_eq!(selected(&w)["row"], json!(1));

    // Sideways: a table wider than the list.
    let mut w = window_on(one_page(data_shape(10)), [640.0, 800.0]);
    open_shape(&mut w, 10);
    run_command(&mut w, "window.sheetSelect", json!({"section": "Prop", "row": "Cost", "cell": "Label"}));
    assert_eq!(sheet(&w)["hscroll"].as_f64(), Some(0.0));
    press_key(&mut w, Key::PageDown, Modifiers::COMMAND);
    assert!(sheet(&w)["hscroll"].as_f64().unwrap() > 100.0, "Ctrl+Page Down: a screen to the right: {}", sheet(&w)["hscroll"]);
    press_key(&mut w, Key::PageUp, Modifiers::COMMAND);
    assert_eq!(sheet(&w)["hscroll"].as_f64(), Some(0.0), "and back");
}

/// While the sheet has the focus its keys are its own: Delete does not delete the selected shape, the arrows do not nudge it and Ctrl+C does not copy it. Escape gives
/// the focus back, and then the drawing's keys work again.
#[test]
fn the_drawings_keys_stand_aside_while_the_sheet_has_the_focus() {
    let (mut w, process, _, _) = flow_window([1400.0, 900.0]);
    run_command(&mut w, "edit.select", json!({"ids": [process]}));
    open_shape(&mut w, process);
    select(&mut w, "PinX");
    let pin = painted(&sheet(&w), "PinX")["text"].clone();
    tap(&mut w, Key::ArrowRight);
    tap(&mut w, Key::Delete);
    tap(&mut w, Key::Escape);
    assert_eq!(edit(&w), Value::Null, "Escape closed the edit that Delete began");
    assert_eq!(sheet(&w)["focus"], json!(true));
    tap(&mut w, Key::Escape);
    assert!(w.state().session().document().unwrap().find_shape(diacraft_doc::Owner::Page(1), process).is_some(), "the shape is still there");
    assert_eq!(painted(&sheet(&w), "PinX")["text"], pin, "and it did not move");
    // The sheet gave the focus back: Delete is the drawing's again.
    assert_eq!(sheet(&w)["focus"], json!(false));
    tap(&mut w, Key::Delete);
    assert!(w.state().session().document().unwrap().find_shape(diacraft_doc::Owner::Page(1), process).is_none(), "now Delete deleted the shape");
}

// ---- the keys inside an edit (design 5.9) ----------------------------------------------------------------------------------------------

/// In typing mode the arrows commit and move; in F2 mode they move the caret; Tab commits and moves right; Enter commits; Escape cancels.
#[test]
fn in_typing_mode_the_arrows_commit_and_move_and_in_f2_mode_they_move_the_caret() {
    let mut w = sheet_of(boxed());
    select(&mut w, "Width");
    typed(&mut w, "3");
    tap(&mut w, Key::ArrowRight);
    assert_eq!(sent(&w).last().map(|s| s["formula"].clone()), Some(json!("3")), "an arrow in typing mode commits");
    assert_eq!(on(&w), "PinX", "and moves");
    assert_eq!(edit(&w), Value::Null);

    select(&mut w, "Height");
    tap(&mut w, Key::F2);
    let before = sent(&w).len();
    tap(&mut w, Key::ArrowLeft);
    assert_ne!(edit(&w), Value::Null, "an arrow in F2 mode moves the caret and commits nothing");
    assert_eq!(on(&w), "Height");
    assert_eq!(sent(&w).len(), before);
    let mut expected = shown(&w, "Height");
    expected.insert(expected.len() - 1, 'x');
    typed(&mut w, "x");
    assert_eq!(edit(&w)["text"], json!(expected), "the caret moved one character left of the end before the character was typed: {}", edit(&w));
    tap(&mut w, Key::Escape);

    // Tab commits and moves right.
    select(&mut w, "PinY");
    typed(&mut w, "6");
    tap(&mut w, Key::Tab);
    assert_eq!(sent(&w).last().map(|s| s["cell"].clone()), Some(json!("PinY")));
    assert_eq!(on(&w), "FlipY", "Tab commits and moves right: PinY's neighbour in its line");
}

// ---- the bar and its buttons (design 5.2, 5.8) --------------------------------------------------------------------------------------

/// The buttons before the bar's text are greyed while nothing is edited and work while a cell is: Accept commits and stays on the cell, Cancel sends nothing. A press in the bar
/// begins an edit of the formula's text there.
#[test]
fn the_bars_buttons_work_while_a_cell_is_edited_and_a_press_in_the_bar_begins_an_edit() {
    let mut w = sheet_of(boxed());
    select(&mut w, "Width");
    assert!(!w.state().element("sheet.bar.accept").unwrap().enabled && !w.state().element("sheet.bar.cancel").unwrap().enabled, "nothing is edited");
    typed(&mut w, "3");
    assert!(w.state().element("sheet.bar.accept").unwrap().enabled && w.state().element("sheet.bar.cancel").unwrap().enabled, "an edit is open");
    press(&mut w, "sheet.bar.cancel");
    w.rest();
    assert_eq!((edit(&w), sent(&w).len()), (Value::Null, 0), "Cancel sends nothing");
    typed(&mut w, "3");
    press(&mut w, "sheet.bar.accept");
    w.rest();
    assert_eq!(sent(&w).len(), 1, "Accept sends the text");
    assert_eq!((on(&w).as_str(), shown(&w, "Width").as_str()), ("Width", "3 in"), "and the selection stays on the cell");

    // A press in the bar: an edit of the formula's text, in the bar.
    press(&mut w, "sheet.bar.formula");
    w.rest();
    let e = edit(&w);
    assert_eq!((e["text"].clone(), e["inBar"].clone(), e["typing"].clone()), (json!("3 in"), json!(true), json!(false)), "{e}");
    typed(&mut w, "x");
    assert_eq!(edit(&w)["text"], json!("3 inx"), "the text is typed in the bar");
    tap(&mut w, Key::Escape);
    assert_eq!(edit(&w), Value::Null);
}

/// A double click on a cell edits its text in the cell; a press on another cell commits the edit and selects that cell.
#[test]
fn a_double_click_edits_in_the_cell_and_a_press_on_another_cell_commits() {
    let mut w = fast_sheet_of(boxed());
    let at = rect_of(&w, "sheet.cell.Width").center();
    click(&mut w, at);
    click(&mut w, at);
    w.rest();
    let e = edit(&w);
    assert_eq!((e["cell"].clone(), e["text"].clone(), e["inBar"].clone()), (json!("Width"), json!("2 in"), json!(false)), "{e}");
    let editor = rect_of(&w, "sheet.edit");
    let cell = rect_of(&w, "sheet.cell.Width");
    assert!(editor.contains(cell.center()) && editor.left() <= cell.left() + 1.0, "the editor is over the cell: {editor:?} {cell:?}");
    typed(&mut w, "+*");
    let target = rect_of(&w, "sheet.cell.PinY").center();
    click(&mut w, target);
    w.rest();
    // `2 inx` is not a formula: the press committed it, the command refused it, so the editor stays open and the selection stays.
    assert_ne!(edit(&w), Value::Null);
    assert_eq!(on(&w), "Width");
    assert_eq!(sheet(&w)["last"]["ok"], json!(false));
    assert!(footer(&w).contains("the formula has an error"), "{}", footer(&w));
}

#[test]
fn a_press_on_another_cell_commits_a_good_text_and_selects_that_cell() {
    let mut w = sheet_of(boxed());
    select(&mut w, "Width");
    typed(&mut w, "3");
    press(&mut w, "sheet.cell.PinY");
    w.rest();
    assert_eq!(edit(&w), Value::Null);
    assert_eq!(sent(&w).last().map(|s| s["cell"].clone()), Some(json!("Width")));
    assert_eq!(on(&w), "PinY");
    assert_eq!(shown(&w, "Width"), "3 in");
}

// ---- a cut text, the width of a column, pairs, red names --------------------------------------------------------------

/// A shape with a Shape Data row whose prompt is a text longer than a column can be.
fn long_prompt_shape() -> ShapeBuilder {
    let long = "long ".repeat(60);
    let mut shape = ShapeBuilder::new(10).cell("Width", "2").cell("Height", "1").cell("PinX", "4").cell("PinY", "4");
    let key = RowKey::name("Cost");
    shape = shape
        .row_cell("Property", None, key.clone(), "Label", "Unit cost")
        .row_cell("Property", None, key.clone(), "Prompt", &long)
        .row_cell("Property", None, key.clone(), "Value", "12")
        .row_cell("Property", None, key, "Format", "@");
    shape
}

/// The text of every text shape egui painted in the last frame, with the clip it was painted in and its own width.
fn text_shapes(w: &Window) -> Vec<(String, Rect, f32)> {
    text_places(w).into_iter().map(|(text, clip, width, _)| (text, clip, width)).collect()
}

/// As [`text_shapes`], with where the text begins: its left edge.
fn text_places(w: &Window) -> Vec<(String, Rect, f32, f32)> {
    w.output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(t) => Some((t.galley.text().to_owned(), clipped.clip_rect, t.galley.size().x, t.pos.x)),
            _ => None,
        })
        .collect()
}

/// A formula too long for its cell is cut at the cell's right edge with no ellipsis, and the whole formula is in the tooltip.
#[test]
fn a_cell_text_is_cut_at_the_edge_with_no_ellipsis_and_the_tooltip_holds_the_whole_text() {
    let mut w = sheet_of(long_prompt_shape());
    run_command(&mut w, "window.sheetSelect", json!({"cell": "Prop.Cost.Prompt"}));
    let cell = rect_of(&w, "sheet.cell.Prop.Cost.Prompt");
    let list = rect_of(&w, "sheet.list");
    let texts = text_shapes(&w);
    let (text, clip, width) = texts
        .iter()
        .find(|(t, clip, _)| t.contains("long long long") && clip.top() >= list.top() - 0.5)
        .cloned()
        .expect("the prompt's text was painted in the list (and not only in the bar)");
    assert!(!text.contains('\u{2026}'), "no ellipsis: {text}");
    assert!(width > cell.width(), "the text is wider than its cell: {width} against {}", cell.width());
    assert!(clip.right() <= cell.right() + 0.5 && clip.left() >= cell.left() - 0.5, "and is cut at the cell's edge: {clip:?} {cell:?}");
    // The tooltip: rest the pointer on the cell.
    let at = cell.center();
    for _ in 0..4 {
        move_to(&mut w, at);
    }
    w.rest();
    let tip: Vec<String> = text_shapes(&w).into_iter().map(|(t, _, _)| t).filter(|t| t.contains("Formula:")).collect();
    assert!(tip.iter().any(|t| t.contains(&"long ".repeat(60).trim_end().to_owned()[..250])), "the whole text is in the tooltip: {tip:?}");
    assert_eq!(painted(&sheet(&w), "Prompt")["text"].as_str().map(|t| t.chars().count()), Some(120), "the record keeps the first 120 characters");
}

/// A column's edge can be dragged (tab state, not saved): the column is as wide as the drag makes it, down to a least, and a double click on the edge makes it fit its text again.
#[test]
fn a_columns_edge_can_be_dragged_and_a_double_click_fits_it_again() {
    let mut w = fast_sheet_of(data_shape(10));
    run_command(&mut w, "window.sheetSelect", json!({"section": "Prop", "row": "Cost", "cell": "Label"}));
    let width_of = |w: &Window, name: &str| rect_of(w, name).width();
    let (label, prompt) = (width_of(&w, "sheet.title.Prop.Label"), width_of(&w, "sheet.title.Prop.Prompt"));
    let title = rect_of(&w, "sheet.title.Prop.Label");
    let edge = egui::pos2(title.right() - 1.0, title.center().y);
    drag(&mut w, edge, egui::vec2(60.0, 0.0));
    w.rest();
    let wider = width_of(&w, "sheet.title.Prop.Label");
    assert!((wider - label - 60.0).abs() < 4.0, "the column is 60 points wider: {label} then {wider}");
    assert!((width_of(&w, "sheet.title.Prop.Prompt") - prompt).abs() < 1.0, "the next column keeps its width");
    // Narrower, but no narrower than a least.
    let title = rect_of(&w, "sheet.title.Prop.Label");
    let edge = egui::pos2(title.right() - 1.0, title.center().y);
    drag(&mut w, edge, egui::vec2(-400.0, 0.0));
    w.rest();
    let least = width_of(&w, "sheet.title.Prop.Label");
    assert!((20.0..=40.0).contains(&least), "a least width stays: {least}");
    // A double click on the edge: the column fits its content again.
    let title = rect_of(&w, "sheet.title.Prop.Label");
    let edge = egui::pos2(title.right() - 1.0, title.center().y);
    click(&mut w, edge);
    click(&mut w, edge);
    w.rest();
    assert!((width_of(&w, "sheet.title.Prop.Label") - label).abs() < 1.0, "fitted again: {}", width_of(&w, "sheet.title.Prop.Label"));
    assert!(sheet(&w)["columns"].is_object(), "the widths are in the window state: {}", sheet(&w)["columns"]);
}

/// A display group is name and value pairs in several columns, filled down each column in turn, each name right-aligned beside its value. A column takes at least 300
/// points: at 1400 and at 1920 points wide the sheet has three columns, and a narrower list has fewer; nothing overlaps or leaves the list.
#[test]
fn a_display_group_is_name_and_value_pairs_in_columns_at_wide_and_narrow_sizes() {
    for (width, wanted) in [(1920.0, Some(3)), (1400.0, Some(3)), (1024.0, None), (640.0, None)] {
        let mut w = window_on(one_page(boxed()), [width, 900.0]);
        open_shape(&mut w, 10);
        let list = rect_of(&w, "sheet.list");
        let columns = sheet(&w)["pairColumns"].as_u64().unwrap() as usize;
        let fits = (((list.width() - 12.0) / 300.0).floor() as usize).clamp(1, 3);
        assert_eq!(columns, fits, "{width}: a column of pairs takes at least 300 points of a list {} wide", list.width());
        if let Some(wanted) = wanted {
            assert_eq!(columns, wanted, "{width}");
        }
        let r = |name: &str| rect_of(&w, &format!("sheet.cell.{name}"));
        // Shape Transform's ten cells down each column in turn: 3, 4 and 3 in three columns, 5 and 5 in two, all ten in one.
        let heads: &[&str] = match columns {
            3 => &["Width", "PinX", "FlipX"],
            2 => &["Width", "LocPinX"],
            _ => &["Width"],
        };
        let tops: Vec<f32> = heads.iter().map(|n| r(n).top()).collect();
        assert!(tops.iter().all(|t| *t == tops[0]), "{width}: {heads:?} head the columns of one line: {tops:?}");
        let lefts: Vec<f32> = heads.iter().map(|n| r(n).left()).collect();
        assert!(lefts.windows(2).all(|p| p[0] < p[1]), "{width}: from the left: {lefts:?}");
        assert!(r("Height").top() > r("Width").top() && r("Height").left() == r("Width").left(), "{width}: Height is under Width");
        if columns == 1 {
            assert!(r("PinX").top() > r("Height").top(), "{width}: one column: a line a cell");
        }
        let cells = elements_named(&w, "sheet.cell.");
        for (name, rect, _) in &cells {
            assert!(
                list.contains_rect(*rect) || rect.intersect(list).area() >= rect.area() - 1.0,
                "{width}: {name} {rect:?} is inside the list {list:?}"
            );
        }
        for (i, (na, a, _)) in cells.iter().enumerate() {
            for (nb, b, _) in &cells[i + 1..] {
                assert!(!overlaps(*a, *b), "{width}: {na} {a:?} overlaps {nb} {b:?}");
            }
        }
        // A cell's name is drawn beside its value.
        assert!(text_shapes(&w).iter().any(|(t, _, _)| t == "Width"), "{width}: the name of Width is drawn");
        w.rest();
    }
}

/// A row's name shows in red when the row's formula has an error.
#[test]
fn a_row_name_is_red_when_the_rows_formula_has_an_error() {
    use diacraft_doc::{Cell, CellAddr, Value as Held};
    let mut broken = Cell::with_formula("1/0", Some(Held::from_text("0")));
    broken.error = Some("#DIV/0!".into());
    let key = RowKey::name("Bad");
    let shape = ShapeBuilder::new(10)
        .cell("Width", "2")
        .cell("Height", "1")
        .row_cell("Property", None, RowKey::name("Good"), "Value", "12")
        .set(CellAddr::in_row("Property", None, key, "Value"), broken);
    let w = sheet_of(shape);
    let rows = sheet(&w)["rowNames"].clone();
    let ink = |label: &str| rows.as_array().unwrap().iter().find(|r| r["label"] == label).map(|r| r["ink"].clone());
    assert_eq!(ink("Prop.Bad"), Some(json!("error")), "{rows}");
    assert_ne!(ink("Prop.Good"), Some(json!("error")), "{rows}");
    assert!(ink("Prop.Good").is_some(), "{rows}");
}

// ---- the window command (design 7.8) ---------------------------------------------------------------------------------------------------

/// `window.sheetEdit` drives the editor as a person would, from the control channel: it begins an edit on the selected cell, types the text, commits or cancels, and the answer
/// says what the command answered.
#[test]
fn window_sheet_edit_drives_the_editor_and_the_answer_says_what_the_command_answered() {
    use crate::support::{call, window as support_window};
    use diacraft_engine::Session;
    use diacraft_ui_egui::DiacraftApp;
    use diacraft_ui_egui::render_worker::bundled_drawer;
    let (tx, rx) = std::sync::mpsc::channel();
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let doc = one_page(boxed());
    session
        .edit("build", |d| {
            *d = doc;
            Ok(())
        })
        .unwrap();
    let mut w = support_window(DiacraftApp::new(session, Some(rx)).with_inline_pictures(bundled_drawer()), [1400.0, 900.0]);
    w.rest();
    let ask = |w: &mut Window, command: &str, params: Value| call(w, &tx, "engine.execute", json!({"command": command, "params": params}));
    ask(&mut w, "window.shapesheet", json!({"of": "shape", "id": 10, "page": 1})).unwrap();
    ask(&mut w, "window.sheetSelect", json!({"cell": "Width"})).unwrap();
    let state = ask(&mut w, "window.sheetEdit", json!({"text": "3"})).unwrap();
    assert_eq!(state["sheet"]["edit"]["text"], json!("3"), "{state}");
    let state = ask(&mut w, "window.sheetEdit", json!({"cancel": true})).unwrap();
    assert_eq!(state["sheet"]["edit"], Value::Null);
    let state = ask(&mut w, "window.sheetEdit", json!({"text": "3", "commit": true})).unwrap();
    assert_eq!(state["sheet"]["last"]["ok"], json!(true), "{state}");
    assert_eq!(state["sheet"]["last"]["answer"]["shown"], json!("3.0000 in"));
    let listed = state["sheet"]["last"]["answer"]["recalculated"]["listed"].clone();
    assert!(listed.as_array().unwrap().iter().any(|c| c["cell"] == json!("LocPinX")), "LocPinX was worked out again: {listed}");
    // A refusal is the answer, with its words, and the editor stays open.
    let state = ask(&mut w, "window.sheetEdit", json!({"text": "1/", "commit": true})).unwrap();
    assert_eq!(state["sheet"]["last"]["ok"], json!(false));
    assert!(state["sheet"]["last"]["error"].as_str().unwrap().contains("the formula has an error"), "{state}");
    assert_eq!(state["sheet"]["edit"]["fault"]["start"], json!(1));
    // A program does not leave an edit by selecting another cell: the command says so.
    let e = ask(&mut w, "window.sheetSelect", json!({"cell": "Height"})).unwrap_err();
    assert!(e.contains("an edit is open: commit or cancel it first"), "{e}");
    // Commit with no edit open, and both of cancel and commit, are refused in words.
    ask(&mut w, "window.sheetEdit", json!({"cancel": true})).unwrap();
    let e = ask(&mut w, "window.sheetEdit", json!({"commit": true})).unwrap_err();
    assert!(e.contains("no edit is open"), "{e}");
    let e = ask(&mut w, "window.sheetEdit", json!({"commit": true, "cancel": true})).unwrap_err();
    assert!(e.contains("commit or cancel, not both"), "{e}");
    // The command is listed with one entry a key.
    let spec = diacraft_ui_egui::commands::find_ui_command("window.sheetEdit").unwrap();
    assert_eq!(diacraft_engine::param_keys(spec.params).unwrap(), ["text", "commit", "cancel"]);
}

// ---- the window comes to rest with an edit open, at narrow and wide sizes ---------------------------------------------------------------

/// An open edit comes to rest at once and asks for no timer; at 640, 1024 and 1920 points the editor, the bar and the fault line stay inside the window and over no other part.
#[test]
fn an_open_edit_comes_to_rest_and_stays_inside_the_window_at_every_width() {
    for (width, height) in [(640.0, 720.0), (1024.0, 800.0), (1920.0, 1000.0)] {
        let mut w = window_on(one_page(boxed()), [width, height]);
        open_shape(&mut w, 10);
        select(&mut w, "Height");
        typed(&mut w, "1/");
        tap(&mut w, Key::Enter);
        let frames = w.rest();
        assert!(frames <= 8, "{width}: settled in {frames} frames");
        assert_eq!(w.state().pending_input(), 0);
        let (editor, list, bar) = (rect_of(&w, "sheet.edit"), rect_of(&w, "sheet.list"), rect_of(&w, "sheet.bar"));
        assert!(
            list.contains_rect(editor) || editor.intersect(list).area() >= editor.area() - 1.0,
            "{width}: the editor {editor:?} is inside the list {list:?}"
        );
        let fault = rect_of(&w, "sheet.bar.fault");
        assert!(
            fault.left() >= bar.left() - 0.5 && fault.right() <= bar.right() + 0.5 && fault.top() >= bar.bottom() - 0.5,
            "{width}: the fault line sits under the bar: {fault:?} {bar:?}"
        );
        let parts: Vec<Rect> =
            ["sheet.bar.name", "sheet.bar.cancel", "sheet.bar.accept", "sheet.bar.formula"].iter().map(|n| rect_of(&w, n)).collect();
        for pair in parts.windows(2) {
            assert!(pair[0].right() <= pair[1].left() + 0.5, "{width}: the bar's parts overlap: {pair:?}");
        }
        assert!(w.state().element("sheet.bar.accept").unwrap().enabled, "{width}: the buttons work while an edit is open");
        tap(&mut w, Key::Escape);
    }
}

// ---- the sheet's menu (design 5.12: Formulas and Values) --------------------------------------------------------------------------------

/// A right click on a cell selects it and opens the sheet's menu; its two check boxes turn the View group's toggles through `window.sheetView`, the menu closes on an item, and the
/// last one on cannot go off (the command says so in the notice).
#[test]
fn a_right_click_on_a_cell_selects_it_and_opens_a_menu_whose_boxes_are_the_two_toggles() {
    let mut w = sheet_of(boxed());
    let at = rect_of(&w, "sheet.cell.PinY").center();
    right_click(&mut w, at);
    assert_eq!(on(&w), "PinY", "the cell was selected");
    assert_eq!(sheet(&w)["menu"], json!(true));
    assert!(is_drawn(&w, "sheet.menu.Formulas") && is_drawn(&w, "sheet.menu.Values"));
    press(&mut w, "sheet.menu.Values");
    w.rest();
    assert_eq!((sheet(&w)["formulas"].clone(), sheet(&w)["values"].clone()), (json!(true), json!(false)), "a box turned its toggle");
    assert_eq!(sheet(&w)["menu"], json!(false), "an item closes the menu");
    press_key(&mut w, Key::F10, Modifiers::SHIFT);
    press(&mut w, "sheet.menu.Formulas");
    w.rest();
    assert_eq!(sheet(&w)["formulas"], json!(true), "the last toggle on stays on");
    assert!(w.state().notice().is_some_and(|n| n.contains("at least one of Formulas and Values stays on")), "{:?}", w.state().notice());
}

/// A named row's name is right-aligned in its column and, too long for it, cut at the left; a number sits at the left.
#[test]
fn a_row_name_too_long_for_its_column_is_cut_at_the_left_and_a_short_one_sits_at_the_right() {
    let long = format!("Row_{}_tail", "x".repeat(110));
    let key = RowKey::name(&long);
    let short = RowKey::name("Cost");
    let shape = ShapeBuilder::new(10)
        .cell("Width", "2")
        .cell("Height", "1")
        .row_cell("Property", None, key, "Value", "1")
        .row_cell("Property", None, short, "Value", "2");
    let mut w = sheet_of(shape);
    run_command(&mut w, "window.sheetSelect", json!({"section": "Prop", "row": long.as_str(), "cell": "Value"}));
    let column = rect_of(&w, "sheet.title.Prop.Row");
    let places = text_places(&w);
    let tail = places.iter().find(|(t, ..)| t.ends_with("_tail") && t.len() > 20).cloned().expect("the long name was painted");
    assert!(tail.0.ends_with("_tail"), "the end of the name is the part that shows: {}", tail.0);
    assert!(tail.2 > column.width(), "the name is wider than its column: {} against {}", tail.2, column.width());
    assert!(tail.3 < column.left(), "and begins left of the column, where the clip cuts it: {} against {}", tail.3, column.left());
    let cost = places.iter().find(|(t, ..)| t == "Prop.Cost").cloned().expect("the short name was painted");
    assert!(column.right() - (cost.3 + cost.2) < 12.0, "a short name ends at the column's right edge: {cost:?} in {column:?}");
}

// ---- the drawing and the sheet together (design 5.11) ------------------------------------------------------------------------------------

/// A move in the drawing changes `PinX` on the open sheet at once, and a cell typed in the sheet redraws the drawing: both read the one document.
#[test]
fn the_drawing_and_the_sheet_update_together() {
    let (mut w, process, _, _) = flow_window([1400.0, 900.0]);
    open_shape(&mut w, process);
    assert_eq!(shown(&w, "PinX"), "3 in");
    run_command(&mut w, "shape.move", json!({"ids": [process], "dx": 1.0, "dy": 0.0}));
    assert_eq!(shown(&w, "PinX"), "4 in", "the move shows on the sheet");
    select(&mut w, "PinX");
    typed(&mut w, "6");
    tap(&mut w, Key::Enter);
    let doc = w.state().session().document().unwrap();
    let sheet = diacraft_doc::SheetRef::Shape(doc.find_shape(diacraft_doc::Owner::Page(1), process).unwrap());
    let pin = diacraft_doc::Resolver::new(doc)
        .resolve(&sheet, &diacraft_doc::CellAddr::direct("PinX"))
        .unwrap()
        .value
        .cell
        .unwrap()
        .value
        .as_ref()
        .unwrap()
        .number();
    assert_eq!(pin, Some(6.0), "the drawing's shape is where the sheet put it");
}

/// A selection whose section was folded is on the section's header, where a key begins: Down goes on to the next section's header.
#[test]
fn a_key_after_a_fold_begins_at_the_header_of_the_selections_section() {
    let mut w = sheet_of(boxed());
    select(&mut w, "Width");
    run_command(&mut w, "window.sheetCollapse", json!({"section": "Shape Transform", "open": false}));
    tap(&mut w, Key::ArrowDown);
    assert_eq!(selected(&w)["header"], json!(true), "{}", selected(&w));
    assert_eq!(selected(&w)["section"].as_str(), Some(labels(&w)[1].as_str()), "the next section's header: {}", selected(&w));
}

// ---- the room a formula and its value share ------------------------------------------------------------------------------------------

/// A column is wide enough for a formula and its error value, and the error shows whole: `1/0` and `#DIV/0!` both fit the Value column of a row.
#[test]
fn a_column_is_wide_enough_for_a_formula_and_its_error_value_and_the_error_shows_whole() {
    use diacraft_doc::{Cell, CellAddr, Value as Held};
    let mut broken = Cell::with_formula("1/0", Some(Held::from_text("0")));
    broken.error = Some("#DIV/0!".into());
    let shape = ShapeBuilder::new(10)
        .cell("Width", "2")
        .cell("Height", "1")
        .row_cell("Property", None, RowKey::name("Good"), "Value", "12")
        .set(CellAddr::in_row("Property", None, RowKey::name("Bad"), "Value"), broken);
    let mut w = sheet_of(shape);
    run_command(&mut w, "window.sheetSelect", json!({"cell": "Prop.Bad.Value"}));
    let list = rect_of(&w, "sheet.list");
    let (_, clip, width, x) = text_places(&w)
        .into_iter()
        .find(|(t, clip, ..)| t == "#DIV/0!" && clip.top() >= list.top() - 0.5)
        .expect("the error value was painted in the list");
    assert!(
        x >= clip.left() - 0.5 && x + width <= clip.right() + 0.5,
        "the whole of `#DIV/0!` is inside the room it was given: text {x}..{} in {clip:?}",
        x + width
    );
}

/// The bar gives a long value at most two fifths of its box, cut at its edge, and the formula keeps the rest (at least 120 points).
#[test]
fn a_long_value_is_cut_in_the_bar_and_the_formula_keeps_its_room() {
    let mut w = sheet_of(long_prompt_shape());
    run_command(&mut w, "window.sheetSelect", json!({"cell": "Prop.Cost.Prompt"}));
    let (formula, value) = (rect_of(&w, "sheet.bar.formula"), rect_of(&w, "sheet.bar.value"));
    assert!(value.width() <= formula.width() * 0.4 + 1.0, "the value takes at most two fifths: {value:?} in {formula:?}");
    assert!(formula.width() - value.width() >= 120.0, "the formula keeps its room: {formula:?} {value:?}");
    let list_texts = text_places(&w);
    let shown = list_texts
        .iter()
        .find(|(t, clip, ..)| t.starts_with("= long long") && clip.height() < 40.0)
        .cloned()
        .expect("the value was drawn from its start");
    assert!(shown.3 >= value.left() - 0.5, "from the left of its slot, and cut at the right: {shown:?} {value:?}");
}
