//! A command that acts on the selection runs on the page the selection was made on, and is refused in words when another page is on view (the review
//!). A shape's `ID` is the page's own, so a drawing of two pages that both hold shapes 1 and 2 shows it: `edit.select {ids: [1, 2]}` on page 1,
//! `page.select {id: 2}` and a command, run with no frame between (the control channel serves up to 64 lines in one frame), used to act on **page 2's** shapes 1
//! and 2, which nobody had selected. Each family that takes the selection is held to it: the shape operations, formatting, arranging and the clipboard.

use diacraft_engine::Session;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::render_worker::bundled_drawer;
use serde_json::{Value, json};

use egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;

use crate::editing::{key, type_text};
use crate::fixture;
use crate::support::{Rest, TempDir, Window, click, fast_window, move_to, run_command};

/// A window on the two-page drawing, page 1 on view.
fn window(dir: &TempDir) -> Window {
    let path = fixture::write(dir.path(), "two pages.vsdx", &fixture::two_pages_vsdx());
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let mut w = fast_window(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()), [1280.0, 900.0]);
    w.rest();
    w
}

fn undo_steps(w: &Window) -> (usize, usize) {
    let h = w.state().session().history().unwrap();
    (h.undo_len(), h.redo_len())
}

/// What `command` does when the shapes `ids` of page 1 are selected and page 2 is shown, all with no frame between: it is refused in words, and **both pages, the
/// history and the clipboard are as they were**. → the words.
fn refused_on_another_page(w: &mut Window, command: &str, params: Value, ids: &[u32]) -> String {
    let (before, revision, steps) = (w.state().session().document().unwrap().clone(), w.state().session().revision(), undo_steps(w));
    let clip = w.state().session().clipboard().map_or(0, |c| usize::from(!c.is_empty()));
    w.state_mut().execute("edit.select", &json!({"ids": ids})).unwrap();
    assert_eq!(w.state().selection(), ids, "the shapes are selected on page 1");
    w.state_mut().execute("page.select", &json!({"id": 2})).unwrap();
    // An entry of the shape operations is quiet then, as the run would be refused.
    if command.starts_with("operations.") {
        assert!(w.state().command_enabled(command).unwrap_err().contains("another page"), "`{command}` is quiet for the selection on the other page");
    }
    let said = w.state_mut().execute(command, &params).expect_err(&format!("`{command}` ran on a page nobody selected on")).to_string();
    assert!(said.contains("another page"), "`{command}`: {said}");
    assert_eq!(w.state().session().document().unwrap(), &before, "`{command}` changed a page");
    assert_eq!((w.state().session().revision(), undo_steps(w)), (revision, steps), "`{command}` added a step");
    assert_eq!(w.state().session().clipboard().map_or(0, |c| usize::from(!c.is_empty())), clip, "`{command}` filled the clipboard");
    assert_eq!(w.state().ui_panics(), 0);
    said
}

/// Every command of the four families, with the shapes it is tried on, and each is refused when the page shown is not the page the selection is on.
#[test]
fn a_command_on_the_selection_is_refused_when_the_selection_is_not_on_the_page_shown() {
    let all = [1, 2];
    let cases: Vec<(&str, Value, &[u32])> = vec![
        // The shape operations (the entry would be quiet: the words are its reason).
        ("operations.union", json!({}), &all),
        ("operations.combine", json!({}), &all),
        ("operations.fragment", json!({}), &all),
        ("operations.intersect", json!({}), &all),
        ("operations.subtract", json!({}), &all),
        // Formatting.
        ("format.shape", json!({"fill": "#00FF00"}), &all),
        ("format.text", json!({"bold": true}), &all),
        ("format.toggle", json!({"what": "bold"}), &all),
        ("format.size", json!({"step": "up"}), &all),
        ("format.rotateText", json!({"by": 90}), &all),
        // Arranging.
        ("arrange.rotate", json!({"by": 90}), &all),
        ("arrange.flip", json!({"axis": "horizontal"}), &all),
        ("arrange.order", json!({"to": "front"}), &[1]),
        ("arrange.align", json!({"how": "left"}), &all),
        ("arrange.group", json!({}), &all),
        // The clipboard.
        ("clipboard.copy", json!({}), &all),
        ("clipboard.cut", json!({}), &all),
        ("clipboard.duplicate", json!({}), &all),
    ];
    for (command, params, ids) in cases {
        let dir = TempDir::new("selection-page");
        let mut w = window(&dir);
        refused_on_another_page(&mut w, command, params, ids);
    }
}

/// The page that is shown is the one the next frame shows the selection on: once the window has drawn a frame, the selection of page 1 is dropped, a command
/// is refused for having nothing selected, and a selection made on page 2 runs there and touches page 1 not at all.
#[test]
fn what_is_selected_on_the_page_shown_runs_there_and_the_other_page_is_left_alone() {
    let dir = TempDir::new("selection-page-run");
    let mut w = window(&dir);
    refused_on_another_page(&mut w, "operations.union", json!({}), &[1, 2]);
    // A frame: the window drops a selection that is not of the page on view.
    w.rest();
    assert!(w.state().selection().is_empty(), "{:?}", w.state().selection());
    let page_one = shapes_of(&w, 1);
    run_command(&mut w, "edit.select", json!({"ids": [1, 2]}));
    run_command(&mut w, "operations.union", json!({}));
    assert_eq!(shapes_of(&w, 1), page_one, "page 1 is as it was");
    assert_eq!(shapes_of(&w, 2).len(), 1, "page 2's two shapes were united");
    // And a format on page 2 reaches page 2's shapes and not page 1's.
    let united = shapes_of(&w, 2)[0];
    run_command(&mut w, "edit.select", json!({"ids": [united]}));
    let before = w.state().session().document().unwrap().pages.get(0).unwrap().clone();
    run_command(&mut w, "format.shape", json!({"fill": "#00FF00"}));
    assert_eq!(w.state().session().document().unwrap().pages.get(0).unwrap(), &before, "page 1 was not formatted");
}

/// The `ID`s of the shapes of the page with this `ID`.
fn shapes_of(w: &Window, page: u32) -> Vec<u32> {
    w.state().session().document().unwrap().page(page).unwrap().shapes.iter().map(|s| s.id).collect()
}

// ---- the panes -----------------------------------------------------------------------------------------------------------
//
// Size & Position and Format Shape took the page on view for the shapes selected, as the commands did, and a value typed in a field was sent with that page and
// those ids (`pane_field` records them when the box takes the focus, and `flush_edits` sends them without asking `selection_target`). A script that changes the page
// before the frame that drops the selection, in the one frame a pane draws, gets a field that shows page 2's shape 1 and takes what is typed for it. The panes now
// take the selection only when it is of the page on view (`selection_on_view`): in that frame they show nothing editable.

/// The window with the pane open and shape 1 of page 1 selected, drawn; then page 2 is shown with no frame between (the control channel serves several lines in
/// one frame), so the selection is of page 1 and the page on view is 2.
fn pane_window(dir: &TempDir, pane: &str) -> Window {
    let mut w = window(dir);
    run_command(&mut w, "window.pane", json!({"pane": pane, "shown": true}));
    run_command(&mut w, "edit.select", json!({"ids": [1]}));
    w
}

fn page_two_on_view(w: &mut Window) {
    w.state_mut().execute("page.select", &json!({"id": 2})).unwrap();
}

/// The centre of the field `name`, from the last frame.
fn field(w: &Window, name: &str) -> egui::Pos2 {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center()
}

/// What typing `text` in the field `name` at `at`, in the frame page 2 comes on view, comes to; the keys, the click and Enter all follow. **The document is as it was.**
fn typed_in_the_frame_the_page_changes(w: &mut Window, name: &str, text: &str, press_in_that_frame: bool) {
    let at = field(w, name);
    let before = w.state().session().document().unwrap().clone();
    let (revision, steps) = (w.state().session().revision(), undo_steps(w));
    if press_in_that_frame {
        // The pointer rests on the field, and its click, the typed text and the page change all come in one frame.
        for _ in 0..3 {
            move_to(w, at);
        }
    } else {
        // The field has the focus (from an earlier frame), with the selection of page 1.
        click(w, at);
        w.rest();
    }
    page_two_on_view(w);
    if press_in_that_frame {
        for pressed in [true, false] {
            w.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
    }
    w.input_mut().events.push(Event::Text(text.to_owned()));
    w.step();
    // **A box that holds the keyboard keeps the selection of page 1 until the canvas drops it** (`pane_keys_held` stops the keys that would drop it early), so the
    // pane is drawn in this frame with page 2 on view and the selection of page 1: it shows nothing of page 2 (neither a value in the box nor a name in the title)
    // and the box has taken nothing typed.
    assert_eq!(w.state().pane_field_text(name).unwrap_or_default(), "", "`{name}` shows or took something in the frame the page changed");
    assert!(w.query_by_label("Size & Position - Second page box").is_none(), "Size & Position's title names page 2's shape 1");
    // Enter, and frames to let an edit finish.
    key(w, Key::Enter, Modifiers::NONE);
    w.rest();
    for _ in 0..4 {
        w.step();
    }
    let now = w.state().session().document().unwrap();
    let changed: Vec<u32> = [1, 2].into_iter().filter(|page| now.page(*page) != before.page(*page)).collect();
    assert!(changed.is_empty(), "`{name}`: a value typed in the frame the page changed reached page(s) {changed:?}");
    assert_eq!(now, &before, "`{name}`: the document changed");
    assert_eq!((w.state().session().revision(), undo_steps(w)), (revision, steps), "`{name}`: a step was added");
    assert_eq!(w.state().ui_panics(), 0);
}

/// In the frame in which another page is on view, with the selection still of page 1, Size & Position shows nothing editable and nothing of page 2: its boxes
/// are quiet and blank, and the title has no shape's name; and Format Shape's boxes are quiet and blank, and its other controls send nothing (`format.shape` is refused).
#[test]
fn the_panes_show_nothing_of_another_page_in_the_frame_it_comes_on_view() {
    let dir = TempDir::new("selection-page-panes-shown");
    let mut w = pane_window(&dir, "size_position");
    w.get_by_label("Size & Position - First page box");
    assert!(w.state().element("pane.size.x").is_some_and(|e| e.enabled), "page 1's shape 1 is shown and can be moved");
    page_two_on_view(&mut w);
    w.step();
    assert!(w.state().element("pane.size.x").is_some_and(|e| !e.enabled), "Size & Position's X is quiet in that frame");
    assert_eq!(w.state().pane_field_text("pane.size.x").unwrap_or_default(), "", "and blank: none of page 2's shape 1");
    assert!(w.query_by_label("Size & Position - Second page box").is_none(), "and no shape of page 2 in its title");
    assert!(w.query_by_label("Size & Position - First page box").is_none(), "nor of page 1");
    w.rest();
    assert!(w.state().selection().is_empty(), "the next frame drops the selection");

    let dir = TempDir::new("selection-page-panes-shown-format");
    let mut w = pane_window(&dir, "format_shape");
    assert!(w.state().element("pane.format.line.weight").is_some_and(|e| e.enabled), "page 1's shape is formatted from here");
    page_two_on_view(&mut w);
    w.step();
    let weight = w.state().element("pane.format.line.weight").expect("the box is drawn");
    assert!(!weight.enabled, "the weight box is quiet in that frame");
    assert_eq!(w.state().pane_field_text("pane.format.line.weight").unwrap_or_default(), "", "and blank: none of page 2's weight");
    assert!(w.state().element("pane.format.line.rounding").is_some_and(|e| !e.enabled), "so is the rounding box");
    assert!(w.state().element("pane.format.fill.none").is_some_and(|e| !e.enabled), "and the fill's choices");
    w.rest();
}

/// A value typed in Size & Position's X in the frame page 2 comes on view reaches neither page: the box had the focus with the selection of page 1, and the
/// typed text finds no box to take it.
#[test]
fn a_value_typed_in_a_focused_size_and_position_box_in_the_frame_the_page_changes_changes_neither_page() {
    let dir = TempDir::new("selection-page-size-typed");
    let mut w = pane_window(&dir, "size_position");
    typed_in_the_frame_the_page_changes(&mut w, "pane.size.x", "9", false);
}

/// The same when the box is pressed in that frame too: it takes the focus for page 2's shape 1 and the text with it (the box is not drawn to be pressed).
#[test]
fn a_box_of_size_and_position_pressed_and_typed_in_the_frame_the_page_changes_changes_neither_page() {
    let dir = TempDir::new("selection-page-size-pressed");
    let mut w = pane_window(&dir, "size_position");
    typed_in_the_frame_the_page_changes(&mut w, "pane.size.x", "9", true);
}

/// The same for Format Shape's line weight, focused before and pressed in that frame.
#[test]
fn a_value_typed_in_format_shape_in_the_frame_the_page_changes_changes_neither_page() {
    for press_in_that_frame in [false, true] {
        let dir = TempDir::new("selection-page-format-typed");
        let mut w = pane_window(&dir, "format_shape");
        typed_in_the_frame_the_page_changes(&mut w, "pane.format.line.weight", "7", press_in_that_frame);
    }
}

/// With a frame between, nothing is stale: a value typed in Size & Position for the shape selected on the page shown is sent to that shape (and the other
/// page's shape of the same `ID` stays).
#[test]
fn a_value_typed_for_the_selection_on_the_page_shown_still_reaches_it_and_only_it() {
    let dir = TempDir::new("selection-page-size-ok");
    let mut w = pane_window(&dir, "size_position");
    let two_before = w.state().session().document().unwrap().page(2).unwrap().clone();
    let at = field(&w, "pane.size.x");
    click(&mut w, at);
    w.rest();
    key(&mut w, Key::A, Modifiers::COMMAND);
    type_text(&mut w, "9");
    key(&mut w, Key::Enter, Modifiers::NONE);
    let doc = w.state().session().document().unwrap();
    assert_eq!(doc.page(2).unwrap(), &two_before, "page 2 is as it was");
    let frame = diacraft_engine::draw::shape_frame(doc, 1, 1).unwrap();
    assert!((frame.x - 9.0).abs() < 1e-6, "shape 1 of page 1 was moved to X = 9: {frame:?}");
}
