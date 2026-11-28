//! Reverse Ends in the window: a row of the connector's right-click menu, of the Connector button's menu on Home, Tools, and of
//! Rotate Shapes under Position on Home, Arrange; **Ctrl+Alt+R**; all of them sending `connector.reverse_ends` for the connectors and lines among the shapes selected, greyed when there are none.
//! What a test checks is the document the command changed, one undo step, and what the window says of the rows and the key.

use diacraft_doc::{Document, Owner, Resolver, SheetRef};
use egui::os::OperatingSystem;
use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::editing::{at, click_at, edit_window, key};
use crate::support::{Rest, Window, click, move_to, press, run_command};

type P = (f64, f64);

/// What a press of Ctrl+Alt holds on a desktop that is not a Mac's.
const CTRL_ALT: Modifiers = Modifiers { alt: true, ctrl: true, shift: false, mac_cmd: false, command: true };

fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

fn undo_steps(w: &Window) -> usize {
    w.state().session().history().map_or(0, |h| h.undo_len())
}

fn number(d: &Document, page: u32, id: u32, name: &str) -> f64 {
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(page), id).unwrap());
    Resolver::new(d).resolve(&sheet, &diacraft_doc::CellAddr::direct(name)).and_then(|f| f.value.cell?.value.as_ref()?.number()).unwrap_or(0.0)
}

fn ends(w: &Window, id: u32) -> (P, P) {
    let (d, page) = (w.state().session().document().unwrap(), page_id(w));
    ((number(d, page, id, "BeginX"), number(d, page, id, "BeginY")), (number(d, page, id, "EndX"), number(d, page, id, "EndY")))
}

fn angle(w: &Window, id: u32) -> f64 {
    number(w.state().session().document().unwrap(), page_id(w), id, "Angle")
}

fn near(a: P, b: P) -> bool {
    (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
}

/// The glue of the connector's begin and end as the page's `Connect` rows have it: the shape each is on.
fn glue(w: &Window, id: u32) -> (Option<u32>, Option<u32>) {
    let page = w.state().session().document().unwrap().pages.get(0).unwrap().clone();
    let on = |cell: &str| page.connects.iter().find(|c| c.from_sheet == id && c.from_cell.as_deref() == Some(cell)).map(|c| c.to_sheet);
    (on("BeginX"), on("EndX"))
}

/// Two rectangles A (2, 3) and B (6, 5) joined by a connector from A's right point to B's left point (a Z), and a third rectangle on its own, at (4, 8). → the window,
/// A, B, the connector and the lone rectangle. Nothing is selected.
fn window_with_a_connector() -> (Window, u32, u32, u32, u32) {
    let mut w = edit_window([1280.0, 900.0]);
    let page = page_id(&w);
    let drop = |w: &mut Window, x: f64, y: f64| -> u32 {
        let answer = w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": page, "x": x, "y": y})).unwrap();
        u32::try_from(answer["id"].as_u64().unwrap()).unwrap()
    };
    let (a, b, lone) = (drop(&mut w, 2.0, 3.0), drop(&mut w, 6.0, 5.0), drop(&mut w, 4.0, 8.0));
    let answer = w.state_mut().execute("connector.add", &json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}, "page": page})).unwrap();
    let c = u32::try_from(answer["id"].as_u64().unwrap()).unwrap();
    w.rest();
    (w, a, b, c, lone)
}

fn select(w: &mut Window, ids: &[u32]) {
    w.state_mut().select(ids);
    w.rest();
}

fn right_click_at(w: &mut Window, x: f64, y: f64) {
    let pos = at(w, x, y);
    for _ in 0..3 {
        move_to(w, pos);
    }
    for pressed in [true, false] {
        w.input_mut().events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        w.step();
    }
    w.rest();
}

fn is_drawn(w: &Window, name: &str) -> bool {
    w.state().element(name).is_some()
}

fn is_enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

fn rect_of(w: &Window, name: &str) -> egui::Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

/// A click on the row `name` of the menu that is open.
fn press_row(w: &mut Window, name: &str) {
    let pos = rect_of(w, name).center();
    for _ in 0..3 {
        move_to(w, pos);
    }
    click(w, pos);
    w.rest();
}

#[test]
fn the_connectors_right_click_menu_has_reverse_ends_beside_the_style_rows_and_it_reverses_the_connector_in_one_undo_step() {
    let (mut w, a, b, c, _) = window_with_a_connector();
    select(&mut w, &[c]);
    let before = ends(&w, c);
    right_click_at(&mut w, 3.0, 3.0);
    let rows: Vec<f32> =
        ["CurvedConnector", "ReverseEnds", "ResetConnector"].iter().map(|n| rect_of(&w, &format!("menu.Connector{n}")).min.y).collect();
    assert!(rows[0] < rows[1] && rows[1] < rows[2], "Reverse Ends comes after the style rows and above Reset Connector: {rows:?}");
    assert!(is_enabled(&w, "menu.ConnectorReverseEnds"));
    w.get_by_label("Reverse Ends");
    let steps = undo_steps(&w);
    press_row(&mut w, "menu.ConnectorReverseEnds");
    let now = ends(&w, c);
    assert!(near(now.0, before.1) && near(now.1, before.0), "the ends swapped: {now:?} {before:?}");
    assert_eq!(glue(&w, c), (Some(b), Some(a)), "each end is still on its shape");
    assert_eq!(undo_steps(&w), steps + 1, "one undo step");
    assert_eq!(w.state().selection(), [c], "the connector is still selected");
    assert_eq!(w.state().ui_panics(), 0);
    run_command(&mut w, "edit.undo", json!({}));
    assert!(near(ends(&w, c).0, before.0));
}

#[test]
fn the_rotate_shapes_menu_of_position_has_reverse_ends_and_it_is_quiet_for_shapes_with_no_ends() {
    let (mut w, _, _, c, lone) = window_with_a_connector();
    let open = |w: &mut Window| {
        press(w, "ribbon.Home.PositionMenu");
        press_row(w, "menu.ShapesRotateGallery");
        assert!(is_drawn(w, "menu.ShapesReverseEnds"), "the Rotate Shapes menu has Reverse Ends");
    };
    // Nothing selected: the whole Rotate Shapes row is quiet, and so is what is in it.
    press(&mut w, "ribbon.Home.PositionMenu");
    assert!(!is_enabled(&w, "menu.ShapesRotateGallery"), "nothing is selected");
    key(&mut w, Key::Escape, Modifiers::NONE);
    // A rectangle: Rotate Shapes opens, and Reverse Ends in it is quiet, as a rectangle has no begin and end.
    select(&mut w, &[lone]);
    open(&mut w);
    assert!(!is_enabled(&w, "menu.ShapesReverseEnds"), "a rectangle has no begin and end");
    assert!(is_enabled(&w, "menu.ShapesRotateRight"), "the rectangle can be turned");
    key(&mut w, Key::Escape, Modifiers::NONE);
    // The rectangle and the connector: the row works on the connector and leaves the rectangle.
    select(&mut w, &[lone, c]);
    let before = (ends(&w, c), w.state().session().document().unwrap().clone());
    open(&mut w);
    assert!(is_enabled(&w, "menu.ShapesReverseEnds"));
    let steps = undo_steps(&w);
    press_row(&mut w, "menu.ShapesReverseEnds");
    let now = ends(&w, c);
    assert!(near(now.0, before.0.1) && near(now.1, before.0.0), "the connector is reversed");
    assert_eq!(undo_steps(&w), steps + 1);
    // The rectangle is as it was: its cells did not change.
    let d = w.state().session().document().unwrap();
    let page = page_id(&w);
    for name in ["PinX", "PinY", "Width", "Height", "Angle"] {
        assert_eq!(number(d, page, lone, name), number(&before.1, page, lone, name), "{name}");
    }
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn the_connector_button_of_tools_picks_the_tool_and_its_menu_has_the_styles_and_reverse_ends() {
    let (mut w, _, _, c, _) = window_with_a_connector();
    select(&mut w, &[c]);
    // The button itself still picks the Connector tool.
    press(&mut w, "ribbon.Home.ConnectorTool.main");
    assert!(w.state().window_state()["tool"].as_str().is_some_and(|t| t == "connector"), "{}", w.state().window_state()["tool"]);
    run_command(&mut w, "tool.pointer", json!({}));
    select(&mut w, &[c]);
    // Its menu holds the three styles and Reverse Ends, beside them.
    press(&mut w, "ribbon.Home.ConnectorTool.menu");
    let names = ["ConnectorToolRightAngle", "ConnectorToolStraight", "ConnectorToolCurved", "ConnectorToolReverseEnds"];
    let ys: Vec<f32> = names.iter().map(|n| rect_of(&w, &format!("menu.{n}")).min.y).collect();
    assert!(ys.windows(2).all(|p| p[0] < p[1]), "the rows come in this order: {ys:?}");
    assert!(names.iter().all(|n| is_enabled(&w, &format!("menu.{n}"))));
    let before = ends(&w, c);
    let steps = undo_steps(&w);
    press_row(&mut w, "menu.ConnectorToolReverseEnds");
    let now = ends(&w, c);
    assert!(near(now.0, before.1) && near(now.1, before.0));
    assert_eq!(undo_steps(&w), steps + 1);
    // A style row of the same menu styles the connector selected (`connector.style`), as the right-click menu's does.
    press(&mut w, "ribbon.Home.ConnectorTool.menu");
    press_row(&mut w, "menu.ConnectorToolStraight");
    let page = page_id(&w);
    let path = w.state_mut().execute("connector.path", &json!({"id": c, "page": page})).unwrap();
    assert_eq!(path["style"], "straight");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn ctrl_alt_r_reverses_the_connectors_selected_and_leaves_ctrl_r_and_other_shapes_alone() {
    let (mut w, _, _, c, lone) = window_with_a_connector();
    select(&mut w, &[c]);
    let before = ends(&w, c);
    let steps = undo_steps(&w);
    key(&mut w, Key::R, CTRL_ALT);
    let now = ends(&w, c);
    assert!(near(now.0, before.1) && near(now.1, before.0), "Ctrl+Alt+R swapped the ends: {now:?}");
    assert_eq!(undo_steps(&w), steps + 1, "one undo step");
    key(&mut w, Key::R, CTRL_ALT);
    assert!(near(ends(&w, c).0, before.0), "and again, back");
    // A rectangle selected: the key does nothing, and above all it does not turn the rectangle (Ctrl+R does).
    select(&mut w, &[lone]);
    let (steps, was) = (undo_steps(&w), angle(&w, lone));
    key(&mut w, Key::R, CTRL_ALT);
    assert_eq!((undo_steps(&w), angle(&w, lone)), (steps, was), "the key is no Ctrl+R for a rectangle");
    key(&mut w, Key::R, Modifiers { ctrl: true, command: true, ..Modifiers::NONE });
    assert_eq!(undo_steps(&w), steps + 1, "Ctrl+R still turns the rectangle to the right");
    assert!((angle(&w, lone) - was).abs() > 1.0, "{} {}", angle(&w, lone), was);
    // Nothing selected: quiet.
    select(&mut w, &[]);
    let steps = undo_steps(&w);
    key(&mut w, Key::R, CTRL_ALT);
    assert_eq!(undo_steps(&w), steps);
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn ctrl_alt_r_stands_aside_while_a_text_box_is_open() {
    let (mut w, _, _, c, _) = window_with_a_connector();
    click_at(&mut w, 3.0, 3.0);
    assert_eq!(w.state().selection(), [c], "the connector is selected by a click on it");
    key(&mut w, Key::F2, Modifiers::NONE);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "the text box is open");
    let (before, steps) = (ends(&w, c), undo_steps(&w));
    key(&mut w, Key::R, CTRL_ALT);
    assert_eq!((ends(&w, c), undo_steps(&w)), (before, steps), "the text box has the key");
    key(&mut w, Key::Escape, Modifiers::NONE);
}

#[test]
fn the_window_command_is_reachable_by_name_and_is_greyed_with_nothing_to_reverse() {
    let (mut w, _, _, c, lone) = window_with_a_connector();
    // `ui.commands` lists it, with the key.
    let spec = diacraft_ui_egui::commands::find_ui_command("arrange.reverseEnds").expect("a window command");
    assert_eq!(spec.params, "none");
    // Nothing selected, and a rectangle: refused in words and nothing changes.
    for selected in [vec![], vec![lone]] {
        select(&mut w, &selected);
        let was = undo_steps(&w);
        let e = w.state_mut().execute("arrange.reverseEnds", &json!({})).unwrap_err();
        assert!(e.to_string().contains("select a connector or a line") || e.to_string().contains("nothing is selected"), "{e}");
        assert_eq!(undo_steps(&w), was);
    }
    // A connector: it runs, by the name the control channel gives it.
    select(&mut w, &[c]);
    let before = ends(&w, c);
    run_command(&mut w, "arrange.reverseEnds", json!({}));
    assert!(near(ends(&w, c).0, before.1));
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn the_key_is_written_as_each_system_writes_it() {
    let ctx = egui::Context::default();
    // The context has fonts once it has run a frame.
    let mut frame = ctx.run_ui(egui::RawInput::default(), |_ui| {});
    frame.textures_delta.clear();
    for (os, want) in [(OperatingSystem::Nix, "Ctrl+Alt+R"), (OperatingSystem::Windows, "Ctrl+Alt+R"), (OperatingSystem::Mac, "Option+Cmd+R")] {
        ctx.set_os(os);
        assert_eq!(diacraft_ui_egui::shortcuts::hint_for(&ctx, "arrange.reverseEnds").as_deref(), Some(want), "{os:?}");
    }
    // The commands whose keys the reference publishes show none.
    assert_eq!(diacraft_ui_egui::shortcuts::hint_for(&ctx, "arrange.rotate"), None);
}
