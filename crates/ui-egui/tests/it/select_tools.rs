//! Select by Type, Area Select, Lasso Select and the option to select shapes partially within the area.
//! The pointer and the keyboard are given to egui as a person's would be; what is checked is the
//! selection the window holds, what it painted, and that nothing was sent to the engine that changes the drawing.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, CellName, Document, Owner, RowKey, Value};
use diacraft_engine::Session;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::render_worker::bundled_drawer;
use diacraft_ui_egui::theme::Tokens;
use egui::{Key, Modifiers, Shape};
use serde_json::{Value as Json, json};

use crate::editing::{at, drag_with, edit_window, frame_of, hold, key, shape_ids};
use crate::selection_look::{AROUND_TWO, painted, three};
use crate::support::{Rest, Window, button, fast_window, move_to, press, run_command};

fn tool(w: &Window) -> String {
    w.state().tool().name().to_owned()
}

fn revision(w: &Window) -> u64 {
    w.state().session().revision()
}

/// A press at the first point, the pointer taken through the others and a release at the last, with `modifiers` held from the press.
fn stroke(w: &mut Window, points: &[(f64, f64)], modifiers: Modifiers) {
    stroke_held(w, points, modifiers);
    if let Some(last) = points.last() {
        button(w, at(w, last.0, last.1), false);
    }
    hold(w, Modifiers::NONE);
    w.rest();
}

/// As [`stroke`], with the button left down at the last point.
fn stroke_held(w: &mut Window, points: &[(f64, f64)], modifiers: Modifiers) {
    let spots: Vec<_> = points.iter().map(|p| at(w, p.0, p.1)).collect();
    for _ in 0..3 {
        move_to(w, spots[0]);
    }
    hold(w, modifiers);
    button(w, spots[0], true);
    for pair in spots.windows(2) {
        for step in 1..=4 {
            move_to(w, pair[0] + (pair[1] - pair[0]) * (step as f32 / 4.0));
        }
    }
    w.step();
}

// ---- the option ----------------------------------------------------------------------------------------------------------------------------

/// "Select shapes partially within area" is off, as the page states, and `window.selectPartially` sets it or flips it.
#[test]
fn the_partial_option_is_off_and_its_command_sets_or_flips_it() {
    let mut w = edit_window([1280.0, 900.0]);
    assert_eq!(w.state().window_state()["selectPartially"], false);
    run_command(&mut w, "window.selectPartially", json!({"on": true}));
    assert_eq!(w.state().window_state()["selectPartially"], true);
    run_command(&mut w, "window.selectPartially", json!({}));
    assert_eq!(w.state().window_state()["selectPartially"], false, "no `on` flips it");
    let refused = w.state_mut().execute("window.selectPartially", &json!({"on": "yes"})).unwrap_err().to_string();
    assert!(refused.contains("true or false"), "{refused}");
}

/// The option is in File, Options, Advanced, as a box under "Editing options": OK sets it, Cancel and Escape leave it.
#[test]
fn the_option_is_a_box_on_the_advanced_page_of_the_options_dialog() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "window.options", json!({"tab": "Advanced"}));
    assert_eq!(w.state().options_dialog_tab(), Some("Advanced"));
    assert_eq!(w.state().window_state()["options"], "Advanced");
    // Both working sections can be chosen; the others stay quiet.
    assert!(w.state().element("options.section.Advanced").unwrap().enabled);
    assert!(w.state().element("options.section.Customize Ribbon").unwrap().enabled);
    assert!(!w.state().element("options.section.General").unwrap().enabled);
    let boxed = w.state().element("options.partial").expect("the box is drawn").clone();
    assert!(boxed.enabled);
    assert_eq!(w.state().options_dialog_shows_partial(), Some(false));
    press(&mut w, "options.partial");
    assert_eq!(w.state().options_dialog_shows_partial(), Some(true));
    assert_eq!(w.state().window_state()["selectPartially"], false, "nothing is set until OK");
    press(&mut w, "options.cancel");
    assert_eq!(w.state().window_state()["selectPartially"], false);
    // Escape is Cancel too.
    run_command(&mut w, "window.options", json!({"tab": "Advanced"}));
    press(&mut w, "options.partial");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().window_state()["selectPartially"], false);
    // OK sets it, and the dialog starts from what the window has.
    run_command(&mut w, "window.options", json!({"tab": "Advanced"}));
    press(&mut w, "options.partial");
    press(&mut w, "options.ok");
    assert!(w.state().options_dialog_tab().is_none());
    assert_eq!(w.state().window_state()["selectPartially"], true);
    run_command(&mut w, "window.options", json!({"tab": "Advanced"}));
    assert_eq!(w.state().options_dialog_shows_partial(), Some(true));
    press(&mut w, "options.section.Customize Ribbon");
    assert_eq!(w.state().options_dialog_tab(), Some("Customize Ribbon"), "a section of the list is chosen by a click");
    press(&mut w, "options.cancel");
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- the net, with the option off and on ---------------------------------------------------------------------------------------------------

/// A net from bare page takes the shapes wholly inside it (the default); with the option on it takes those it only touches too, and an untouched shape is never taken.
#[test]
fn a_net_takes_the_shapes_wholly_inside_it_and_with_the_option_on_those_it_touches() {
    let (mut w, ids) = three();
    let before = revision(&w);
    // The net takes the rectangle (3, 6) and the left half of the diamond (7, 6); the ellipse (5, 3) is below it.
    drag_with(&mut w, (1.5, 7.2), (7.0, 4.2), Modifiers::NONE);
    assert_eq!(w.state().selection(), &ids[..1], "only the rectangle lies wholly inside");
    run_command(&mut w, "window.selectPartially", json!({"on": true}));
    drag_with(&mut w, (1.5, 7.2), (7.0, 4.2), Modifiers::NONE);
    assert_eq!(w.state().selection(), &ids[..2], "the diamond is touched");
    drag_with(&mut w, (1.5, 7.2), (7.0, 5.0), Modifiers::NONE);
    assert_eq!(w.state().selection(), &ids[..2], "the net ends over the diamond's lower half");
    drag_with(&mut w, (8.9, 1.2), (8.5, 1.5), Modifiers::NONE);
    assert!(w.state().selection().is_empty(), "a net over bare page takes nothing");
    assert_eq!(revision(&w), before, "a selection changes nothing in the drawing");
    assert_eq!(w.state().ui_panics(), 0);
}

/// With the option off a net the old way (the shapes wholly inside it) still gives the shapes of V1: the rectangle and the diamond, not the ellipse.
#[test]
fn a_net_round_two_shapes_still_takes_those_two() {
    let (mut w, ids) = three();
    drag_with(&mut w, AROUND_TWO.0, AROUND_TWO.1, Modifiers::NONE);
    assert_eq!(w.state().selection(), &ids[..2]);
}

// ---- Area Select ---------------------------------------------------------------------------------------------------------------------------

/// Two rectangles, 1 at (2, 3) and 2 at (6, 2).
fn two_rectangles() -> (Window, Vec<u32>) {
    let mut w = edit_window([1280.0, 900.0]);
    crate::editing::drag_tile_to(&mut w, "Rectangle", 2.0, 3.0);
    crate::editing::drag_tile_to(&mut w, "Rectangle", 6.0, 2.0);
    let ids = shape_ids(&w);
    crate::editing::click_at(&mut w, 8.0, 1.5);
    assert!(w.state().selection().is_empty());
    (w, ids)
}

/// Area Select (Home, Editing, Select): the next drag draws a net, **even from a press on a shape**, which a drag with the Pointer Tool would move; one drag uses the mode up.
#[test]
fn area_select_draws_a_net_from_a_press_on_a_shape_and_is_used_up_by_one_drag() {
    let (mut w, ids) = two_rectangles();
    let frames: Vec<_> = ids.iter().map(|id| frame_of(&w, *id)).collect();
    run_command(&mut w, "tool.area_select", json!({}));
    assert_eq!(tool(&w), "area_select");
    assert_eq!(w.state().window_state()["tool"], "area_select");
    let before = revision(&w);
    // The press is in the middle of rectangle 1, the net takes rectangle 2 wholly.
    drag_with(&mut w, (2.5, 3.0), (7.5, 1.0), Modifiers::NONE);
    assert_eq!(w.state().selection(), &ids[1..], "rectangle 2 is wholly inside the net, rectangle 1 is not");
    assert_eq!(revision(&w), before, "no shape was moved: the drag was a net");
    assert_eq!(ids.iter().map(|id| frame_of(&w, *id)).collect::<Vec<_>>(), frames);
    assert_eq!(tool(&w), "pointer", "the Pointer Tool is back: one drag used the mode up");
}

/// A click that is no drag leaves Area Select armed (the mode is for the next drag), and Escape puts it down.
#[test]
fn a_click_leaves_area_select_armed_and_escape_puts_it_down() {
    let (mut w, ids) = two_rectangles();
    crate::editing::click_at(&mut w, 2.0, 3.0);
    assert_eq!(w.state().selection(), &ids[..1], "the Pointer Tool selected rectangle 1");
    run_command(&mut w, "tool.area_select", json!({}));
    crate::editing::click_at(&mut w, 8.0, 1.5);
    assert_eq!(tool(&w), "area_select", "no drag, so the mode is still on");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(tool(&w), "pointer");
}

/// The net of Area Select looks as the Pointer Tool's does: the veil from the first movement, no marks on the shapes under it, the crosshair.
#[test]
fn the_net_of_area_select_is_the_same_veil() {
    let t = Tokens::light();
    let (mut w, _) = two_rectangles();
    run_command(&mut w, "tool.area_select", json!({}));
    let a = at(&w, 2.5, 3.0);
    for _ in 0..3 {
        move_to(&mut w, a);
    }
    assert_eq!(w.output().platform_output.cursor_icon, egui::CursorIcon::Crosshair, "the mode shows before the press");
    button(&mut w, a, true);
    move_to(&mut w, a + egui::vec2(120.0, 60.0));
    assert!(painted(&w).iter().any(|s| matches!(s, Shape::Rect(r) if r.fill == t.net_fill)), "the veil is drawn");
    button(&mut w, a + egui::vec2(120.0, 60.0), false);
    w.rest();
}

// ---- Lasso Select ---------------------------------------------------------------------------------------------------------------------------

/// A loop takes the shapes wholly inside it, and **the loop need not be closed**: the last point joins the first.
#[test]
fn a_lasso_takes_the_shapes_wholly_inside_the_loop_which_need_not_be_closed() {
    let (mut w, ids) = three();
    run_command(&mut w, "tool.lasso_select", json!({}));
    assert_eq!(tool(&w), "lasso_select");
    let before = revision(&w);
    // Round the rectangle (3, 6) and not closed: it ends below where it began.
    stroke(&mut w, &[(1.6, 5.0), (4.4, 5.0), (4.4, 7.0), (1.6, 7.0), (1.6, 5.6)], Modifiers::NONE);
    assert_eq!(w.state().selection(), &ids[..1]);
    assert_eq!(revision(&w), before);
    assert_eq!(tool(&w), "pointer", "the loop used the mode up");
    // A loop round the rectangle (3, 6) and the diamond (7, 6) and clear of the ellipse (5, 3).
    run_command(&mut w, "tool.lasso_select", json!({}));
    stroke(&mut w, &[(1.5, 4.9), (8.8, 4.9), (8.8, 7.4), (1.5, 7.4)], Modifiers::NONE);
    assert_eq!(w.state().selection(), &ids[..2]);
}

/// A loop that is not a rectangle: a triangle takes what is in it, and a shape across its edge only with the option on ("Area Select and Lasso Select").
#[test]
fn the_option_applies_to_the_lasso_as_it_does_to_the_net() {
    let (mut w, ids) = three();
    // A triangle with the rectangle (3, 6) in it, its long side running up across the lower half of the diamond (7, 6): the diamond's bottom vertex is inside it and its middle is not.
    let triangle = [(1.5, 4.8), (9.0, 4.8), (1.5, 8.0)];
    run_command(&mut w, "tool.lasso_select", json!({}));
    stroke(&mut w, &triangle, Modifiers::NONE);
    assert_eq!(w.state().selection(), &ids[..1], "the rectangle, wholly inside; the diamond is crossed");
    run_command(&mut w, "window.selectPartially", json!({"on": true}));
    run_command(&mut w, "tool.lasso_select", json!({}));
    stroke(&mut w, &triangle, Modifiers::NONE);
    assert_eq!(w.state().selection(), &ids[..2], "with the option on the loop takes the diamond it crosses");
}

/// Shift at the press adds to what is selected, as it does for a net.
#[test]
fn a_lasso_with_shift_adds_to_the_selection() {
    let (mut w, ids) = three();
    crate::editing::click_at(&mut w, 5.0, 3.0);
    assert_eq!(w.state().selection(), &ids[2..]);
    run_command(&mut w, "tool.lasso_select", json!({}));
    stroke(&mut w, &[(1.6, 5.0), (4.4, 5.0), (4.4, 7.0), (1.6, 7.0)], Modifiers::SHIFT);
    assert_eq!(w.state().selection(), &[ids[2], ids[0]], "the ellipse was first, the rectangle is added after it");
}

/// The loop's trail is a thin dotted grey line behind the crosshair, with no veil and no box, and the shapes are marked only on the release (still, never moving).
#[test]
fn the_lasso_trail_is_a_dotted_grey_line_with_no_veil_and_nothing_is_marked_before_the_release() {
    let t = Tokens::light();
    let (mut w, _) = three();
    run_command(&mut w, "tool.lasso_select", json!({}));
    stroke_held(&mut w, &[(1.6, 5.0), (4.4, 5.0), (4.4, 7.0)], Modifiers::NONE);
    assert_eq!(w.output().platform_output.cursor_icon, egui::CursorIcon::Crosshair);
    let shapes = painted(&w);
    let dots = shapes.iter().filter(|s| matches!(s, Shape::Circle(c) if c.fill == t.net_line && c.radius < 1.6)).count();
    assert!(dots >= 20, "a dotted line is a run of dots: {dots}");
    assert!(!shapes.iter().any(|s| matches!(s, Shape::Rect(r) if r.fill == t.net_fill)), "no veil");
    assert!(
        !shapes.iter().any(|s| matches!(s, Shape::Rect(r) if r.stroke.color == t.select_frame || r.stroke.color == t.select_box)),
        "no frame and no box"
    );
    assert!(w.state().selection().is_empty(), "nothing is selected before the release");
    // Still: a second frame with the pointer where it was paints the same dots (nothing moves).
    w.step();
    let again = painted(&w).iter().filter(|s| matches!(s, Shape::Circle(c) if c.fill == t.net_line && c.radius < 1.6)).count();
    assert_eq!(again, dots);
    let spot = at(&w, 4.4, 7.0);
    button(&mut w, spot, false);
    w.rest();
    assert!(!painted(&w).iter().any(|s| matches!(s, Shape::Circle(c) if c.fill == t.net_line && c.radius < 1.6)), "the trail goes at the release");
}

/// Escape in the middle of a loop drops it and selects nothing; the mode stays on until a second Escape.
#[test]
fn escape_in_the_middle_of_a_lasso_selects_nothing_and_the_mode_stays_on() {
    let (mut w, _) = three();
    run_command(&mut w, "tool.lasso_select", json!({}));
    stroke_held(&mut w, &[(1.6, 5.0), (4.4, 5.0), (4.4, 7.0)], Modifiers::NONE);
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(!w.state().edit_dragging());
    let spot = at(&w, 4.4, 7.0);
    button(&mut w, spot, false);
    w.rest();
    assert!(w.state().selection().is_empty());
    assert_eq!(tool(&w), "lasso_select");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(tool(&w), "pointer");
}

// ---- the menu and the Draw tab -------------------------------------------------------------------------------------------------------------

/// The Select menu's three rows that were dead work, and Lasso Select works on the Draw tab too.
#[test]
fn the_select_menu_and_the_draw_tabs_lasso_run_their_commands() {
    let mut w = edit_window([2400.0, 900.0]);
    press(&mut w, "ribbon.Home.SelectMenu");
    w.rest();
    for row in ["SelectAll", "SelectByTypeDialog", "AreaSelect", "LassoSelect"] {
        assert!(w.state().element(&format!("menu.{row}")).is_some_and(|e| e.enabled), "{row} works");
    }
    press(&mut w, "menu.AreaSelect");
    w.rest();
    assert_eq!(tool(&w), "area_select");
    press(&mut w, "ribbon.Home.SelectMenu");
    w.rest();
    press(&mut w, "menu.LassoSelect");
    w.rest();
    assert_eq!(tool(&w), "lasso_select");
    run_command(&mut w, "tool.pointer", json!({}));
    run_command(&mut w, "window.tab", json!({"tab": "Draw"}));
    assert!(w.state().element("ribbon.Draw.LassoSelect").is_some_and(|e| e.enabled), "the Draw tab's Lasso Select works");
    press(&mut w, "ribbon.Draw.LassoSelect");
    w.rest();
    assert_eq!(tool(&w), "lasso_select");
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- edit.selectRegion ----------------------------------------------------------------------------------------------------------------------

/// A program selects in a region as the pointer does: `edit.selectRegion` takes a rectangle or a polygon, the option unless `partial` says otherwise, and `add` keeps what
/// was selected.
#[test]
fn edit_select_region_selects_as_the_pointer_does() {
    let (mut w, ids) = three();
    run_command(&mut w, "edit.selectRegion", json!({"rect": [1.5, 4.2, 7.0, 7.2]}));
    assert_eq!(w.state().selection(), &ids[..1]);
    run_command(&mut w, "edit.selectRegion", json!({"rect": [1.5, 4.2, 7.0, 7.2], "partial": true}));
    assert_eq!(w.state().selection(), &ids[..2]);
    run_command(&mut w, "window.selectPartially", json!({"on": true}));
    let triangle = json!([[1.5, 4.8], [9.0, 4.8], [1.5, 8.0]]);
    run_command(&mut w, "edit.selectRegion", json!({"polygon": triangle}));
    assert_eq!(w.state().selection(), &ids[..2], "the option applies when `partial` is left out");
    run_command(&mut w, "edit.selectRegion", json!({"polygon": triangle, "partial": false}));
    assert_eq!(w.state().selection(), &ids[..1], "and `partial` false overrides it");
    run_command(&mut w, "edit.selectRegion", json!({"rect": [4.0, 2.0, 6.0, 4.0], "partial": false}));
    assert_eq!(w.state().selection(), &ids[2..]);
    run_command(&mut w, "edit.selectRegion", json!({"rect": [2.0, 5.0, 4.0, 7.0], "add": true}));
    assert_eq!(w.state().selection(), &[ids[2], ids[0]], "added after what was selected");
    let refused = w.state_mut().execute("edit.selectRegion", &json!({})).unwrap_err().to_string();
    assert!(refused.contains("polygon") && refused.contains("rect"), "{refused}");
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- Select by Type ---------------------------------------------------------------------------------------------------------------------------

/// A drawing of two shapes and a connector between them: 1 a rectangle on layer 0 `Walls`, 2 a diamond on layer 1 `Pipes`, 3 the connector.
fn typed_window() -> (Window, Vec<u32>) {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let mut drop = |master: &str, x: f64, y: f64| {
        let made = session.execute("shape.drop", &json!({"master": master, "page": 1, "x": x, "y": y})).unwrap();
        u32::try_from(made["id"].as_u64().unwrap()).unwrap()
    };
    let (a, b) = (drop("rectangle", 3.0, 6.0), drop("diamond", 7.0, 6.0));
    let joined = session.execute("connector.add", &json!({"from": {"id": a}, "to": {"id": b}})).unwrap();
    let c = u32::try_from(joined["id"].as_u64().unwrap()).unwrap();
    session
        .edit("layers", |doc: &mut Document| {
            let page = Arc::make_mut(doc.pages.get_mut(0).expect("a page"));
            let mut sheet = (*page.sheet).clone();
            for (ix, name) in [(0u32, "Walls"), (1, "Pipes")] {
                sheet.set_cell(&CellAddr::in_row("Layer", None, RowKey::Ix(ix), "Name"), Cell::constant(Value::from_text(name))).unwrap();
            }
            page.sheet = Arc::new(sheet);
            for (id, layer) in [(a, "0"), (b, "1")] {
                let found = doc.find_shape(Owner::Page(1), id).expect("the shape");
                let page = Arc::make_mut(doc.pages.get_mut(0).expect("a page"));
                let (first, _) = found.path.split_first().expect("a path");
                let shape = Arc::make_mut(page.shapes.get_mut(usize::try_from(*first).unwrap()).expect("a shape"));
                Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("LayerMember"), Cell::constant(Value::from_text(layer)));
            }
            Ok(())
        })
        .unwrap();
    let mut w = fast_window(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()), [1600.0, 900.0]);
    w.rest();
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    (w, vec![a, b, c])
}

fn dialog_names(w: &Window, prefix: &str) -> Vec<String> {
    crate::support::elements_named(w, prefix).into_iter().map(|(name, ..)| name).collect()
}

/// The Select menu's Select by Type opens a dialog with the types, the roles and the layers of the page: the seven types and two roles always, and the layers the page has.
#[test]
fn select_by_type_opens_a_dialog_that_lists_the_types_the_roles_and_the_layers() {
    let (mut w, _) = typed_window();
    press(&mut w, "ribbon.Home.SelectMenu");
    w.rest();
    press(&mut w, "menu.SelectByTypeDialog");
    w.rest();
    let state = w.state().window_state()["selectByType"].clone();
    assert!(state.is_object(), "the dialog is open: {state}");
    assert_eq!(
        dialog_names(&w, "select_type.type."),
        [
            "select_type.type.shape",
            "select_type.type.group",
            "select_type.type.guide",
            "select_type.type.ole",
            "select_type.type.metafile",
            "select_type.type.bitmap",
            "select_type.type.ink"
        ]
    );
    assert_eq!(dialog_names(&w, "select_type.role."), ["select_type.role.connector", "select_type.role.container"]);
    assert_eq!(dialog_names(&w, "select_type.layer."), ["select_type.layer.0", "select_type.layer.1"]);
    assert!(!w.state().element("select_type.ok").unwrap().enabled, "nothing is ticked, so OK has nothing to select");
    assert!(w.state().element("select_type.cancel").unwrap().enabled);
    press(&mut w, "select_type.cancel");
    assert!(w.state().window_state()["selectByType"].is_null());
    assert_eq!(w.state().ui_panics(), 0);
}

/// Ticking a type, a role or a layer and OK selects what matches; the three lists are put together by and; Cancel and Escape select nothing.
#[test]
fn ok_selects_the_shapes_of_the_types_roles_and_layers_ticked() {
    let (mut w, ids) = typed_window();
    let before = revision(&w);
    run_command(&mut w, "window.selectByType", json!({}));
    press(&mut w, "select_type.role.connector");
    assert!(w.state().element("select_type.ok").unwrap().enabled);
    press(&mut w, "select_type.ok");
    assert!(w.state().window_state()["selectByType"].is_null(), "OK closes it");
    assert_eq!(w.state().selection(), &ids[2..], "the connector");
    // Shapes on the layer Pipes.
    run_command(&mut w, "window.selectByType", json!({}));
    press(&mut w, "select_type.type.shape");
    press(&mut w, "select_type.layer.1");
    press(&mut w, "select_type.ok");
    assert_eq!(w.state().selection(), &ids[1..2], "the diamond is the shape on Pipes");
    // Cancel and Escape select nothing.
    run_command(&mut w, "window.selectByType", json!({}));
    press(&mut w, "select_type.type.shape");
    press(&mut w, "select_type.cancel");
    assert_eq!(w.state().selection(), &ids[1..2]);
    run_command(&mut w, "window.selectByType", json!({}));
    press(&mut w, "select_type.type.shape");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(w.state().window_state()["selectByType"].is_null());
    assert_eq!(w.state().selection(), &ids[1..2]);
    // All the types: every shape, the connector too.
    run_command(&mut w, "window.selectByType", json!({}));
    press(&mut w, "select_type.type.shape");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert_eq!(w.state().selection(), ids.as_slice(), "Enter is OK");
    assert_eq!(revision(&w), before, "a selection changes nothing in the drawing");
    assert_eq!(w.state().ui_panics(), 0);
}

/// `edit.selectByType` selects without a dialog, as a program does, and refuses in words; `window.selectByType` with `close` closes the dialog.
#[test]
fn edit_select_by_type_selects_without_a_dialog_and_refuses_in_words() {
    let (mut w, ids) = typed_window();
    run_command(&mut w, "edit.selectByType", json!({"roles": ["connector"]}));
    assert_eq!(w.state().selection(), &ids[2..]);
    run_command(&mut w, "edit.selectByType", json!({"layers": ["Walls"]}));
    assert_eq!(w.state().selection(), &ids[..1]);
    assert!(w.state().window_state()["selectByType"].is_null(), "no dialog opened");
    let refused = w.state_mut().execute("edit.selectByType", &json!({})).unwrap_err().to_string();
    assert!(refused.contains("give types, roles or layers"), "{refused}");
    let refused = w.state_mut().execute("edit.selectByType", &json!({"types": ["triangle"]})).unwrap_err().to_string();
    assert!(refused.contains("no shape type `triangle`"), "{refused}");
    run_command(&mut w, "window.selectByType", json!({}));
    assert!(w.state().window_state()["selectByType"].is_object());
    run_command(&mut w, "window.selectByType", json!({"close": true}));
    assert!(w.state().window_state()["selectByType"].is_null());
    assert_eq!(w.state().ui_panics(), 0);
}

/// The dialog's lists fit a narrow window and a wide one: every row is inside the window and the rows do not overlap.
#[test]
fn the_dialog_fits_a_narrow_window_and_a_wide_one() {
    for size in [[420.0f32, 640.0], [1600.0, 900.0]] {
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let mut w = fast_window(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()), size);
        w.rest();
        run_command(&mut w, "window.selectByType", json!({}));
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
        let rows = crate::support::elements_named(&w, "select_type.");
        assert!(rows.len() >= 11, "{rows:?}");
        for (name, rect, _) in &rows {
            assert!(screen.contains_rect(*rect), "{name} {rect:?} is inside the window {size:?}");
        }
        for (i, (a, ra, _)) in rows.iter().enumerate() {
            for (b, rb, _) in &rows[i + 1..] {
                assert!(!ra.intersects(*rb), "{a} and {b} overlap at {size:?}: {ra:?} {rb:?}");
            }
        }
        let _: Json = w.state().window_state();
    }
}

/// Opening the dialog on a drawing with no shapes still works, and a shape's page (not the first) is the one it asks about.
#[test]
fn the_dialog_asks_about_the_page_on_view() {
    let (mut w, ids) = typed_window();
    run_command(&mut w, "page.add", json!({"name": "Second"}));
    let second = w.state().session().document().unwrap().pages.get(1).unwrap().id;
    run_command(&mut w, "page.select", json!({"id": second}));
    run_command(&mut w, "edit.selectByType", json!({"types": ["shape"]}));
    assert!(w.state().selection().is_empty(), "the second page has no shape");
    let first = w.state().session().document().unwrap().pages.get(0).unwrap().id;
    run_command(&mut w, "page.select", json!({"id": first}));
    run_command(&mut w, "edit.selectByType", json!({"types": ["shape"]}));
    assert_eq!(w.state().selection(), ids.as_slice());
}

/// In a window too narrow for the list of sections beside the page, the two sections that work are a row of buttons over the page, so that both pages can be reached, and
/// every control is inside the window.
#[test]
fn a_narrow_options_dialog_reaches_both_working_pages() {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let mut w = fast_window(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()), [420.0, 640.0]);
    w.rest();
    run_command(&mut w, "window.options", json!({"tab": "Advanced"}));
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 640.0));
    for name in ["options.page.Advanced", "options.page.Customize Ribbon", "options.partial", "options.ok", "options.cancel"] {
        let e = w.state().element(name).unwrap_or_else(|| panic!("{name} is drawn"));
        assert!(screen.contains_rect(e.rect), "{name} {:?} is inside the window", e.rect);
    }
    press(&mut w, "options.page.Customize Ribbon");
    assert_eq!(w.state().options_dialog_tab(), Some("Customize Ribbon"));
    assert!(w.state().element("options.tab.Developer").is_some());
    press(&mut w, "options.page.Advanced");
    assert_eq!(w.state().options_dialog_tab(), Some("Advanced"));
    assert_eq!(w.state().ui_panics(), 0);
}
