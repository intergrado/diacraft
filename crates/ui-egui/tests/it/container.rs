//! In the window: the Insert tab's Container gallery puts a container round the shapes selected (or drops an empty one), the Container Format tab shows while a
//! container is selected and goes when it is not, its controls send the engine's commands, the right-click row adds a shape to the container beneath it, and a container lights
//! while one of its members is selected. The pointer is given to egui as a person's would be; what is checked is the drawing the commands changed and what the window showed.

use serde_json::{Value as Json, json};

use crate::editing::edit_window;
use crate::support::{Rest, Window, press, run_command};

fn drop_at(w: &mut Window, master: &str, x: f64, y: f64) -> u32 {
    w.state_mut().execute("shape.drop", &json!({"master": master, "page": 1, "x": x, "y": y})).unwrap()["id"].as_u64().unwrap() as u32
}

fn info(w: &mut Window, ids: &[u32]) -> Json {
    w.state_mut().execute("container.info", &json!({"ids": ids})).unwrap()
}

fn tabs(w: &Window) -> Vec<String> {
    diacraft_ui_egui::ribbon::spec::tab_names(w.state().tab_context()).iter().map(|s| (*s).to_owned()).collect()
}

fn enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

fn select(w: &mut Window, ids: &[u32]) {
    run_command(w, "edit.select", json!({"ids": ids}));
}

/// A window with two rectangles selected on the Insert tab. → the window and the shapes.
fn two_selected() -> (Window, u32, u32) {
    let mut w = edit_window([1400.0, 900.0]);
    let a = drop_at(&mut w, "rectangle", 3.0, 4.0);
    let b = drop_at(&mut w, "rectangle", 6.0, 4.0);
    select(&mut w, &[a, b]);
    run_command(&mut w, "window.tab", json!({"tab": "Insert"}));
    (w, a, b)
}

#[test]
fn the_container_gallery_puts_a_container_round_the_shapes_selected_and_selects_it() {
    let (mut w, a, b) = two_selected();
    assert!(enabled(&w, "ribbon.Insert.ContainerGallery"), "Container is live");
    assert!(!tabs(&w).iter().any(|t| t == "Container Format"), "no container is selected: no Container Format tab");
    press(&mut w, "ribbon.Insert.ContainerGallery");
    w.rest();
    press(&mut w, "menu.container.tint-strip");
    w.rest();
    let state = w.state().window_state();
    let selected: Vec<u32> = state["selection"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32).collect();
    assert_eq!(selected.len(), 1, "the new container is selected: {state}");
    let c = selected[0];
    let all = info(&mut w, &[c]);
    assert_eq!(all["containers"][0]["style"], "tint-strip");
    assert_eq!(all["containers"][0]["members"], json!([a, b]));
    assert!(tabs(&w).last().is_some_and(|t| t == "Container Format"), "the tab shows at the right end of the row: {:?}", tabs(&w));
    // One undo step took it back.
    run_command(&mut w, "edit.undo", json!({}));
    assert!(info(&mut w, &[c])["containers"].as_array().unwrap().is_empty());
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn with_nothing_selected_an_empty_container_is_dropped_in_the_middle_of_the_view() {
    let mut w = edit_window([1400.0, 900.0]);
    let a = drop_at(&mut w, "rectangle", 4.0, 5.0);
    select(&mut w, &[]);
    run_command(&mut w, "window.tab", json!({"tab": "Insert"}));
    press(&mut w, "ribbon.Insert.ContainerGallery");
    w.rest();
    press(&mut w, "menu.container.frame");
    w.rest();
    let state = w.state().window_state();
    let c = state["selection"][0].as_u64().unwrap() as u32;
    assert_eq!(info(&mut w, &[c])["containers"][0]["members"], json!([]), "it holds nothing, even over the shape at its middle ({a})");
}

#[test]
fn the_container_format_tab_sends_the_engines_commands_for_the_container_selected() {
    let (mut w, a, _b) = two_selected();
    press(&mut w, "ribbon.Insert.ContainerGallery");
    w.rest();
    press(&mut w, "menu.container.strip");
    w.rest();
    let c = w.state().window_state()["selection"][0].as_u64().unwrap() as u32;
    run_command(&mut w, "window.tab", json!({"tab": "Container Format"}));
    for control in [
        "ContainerMargins",
        "FitToContents",
        "AutomaticResize",
        "ContainerStyles",
        "ContainerHeadingStyles",
        "ContainerLock",
        "SelectContents",
        "ContainerDisband",
    ] {
        assert!(enabled(&w, &format!("ribbon.Container Format.{control}")), "{control} is live on a selected container");
    }
    // Lock Container: a check box that is pressed when the container is locked.
    press(&mut w, "ribbon.Container Format.ContainerLock");
    w.rest();
    assert_eq!(info(&mut w, &[c])["containers"][0]["locked"], true);
    press(&mut w, "ribbon.Container Format.ContainerLock");
    w.rest();
    assert_eq!(info(&mut w, &[c])["containers"][0]["locked"], false);
    // Heading Style: Rotate Heading goes one side on.
    press(&mut w, "ribbon.Container Format.ContainerHeadingStyles");
    w.rest();
    press(&mut w, "menu.ContainerRotateHeading");
    w.rest();
    assert_eq!(info(&mut w, &[c])["containers"][0]["heading"], "right");
    // Select Contents selects the members.
    press(&mut w, "ribbon.Container Format.SelectContents");
    w.rest();
    let selected: Vec<u64> = w.state().window_state()["selection"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!(selected.len(), 2, "the two shapes: {selected:?} ({a})");
    assert!(!tabs(&w).iter().any(|t| t == "Container Format"), "the tab goes when the container is not selected");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn disband_container_leaves_the_shapes_selected_and_no_frame() {
    let (mut w, a, b) = two_selected();
    press(&mut w, "ribbon.Insert.ContainerGallery");
    w.rest();
    press(&mut w, "menu.container.strip");
    w.rest();
    run_command(&mut w, "window.tab", json!({"tab": "Container Format"}));
    press(&mut w, "ribbon.Container Format.ContainerDisband");
    w.rest();
    let selected: Vec<u64> = w.state().window_state()["selection"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!(selected, [u64::from(a), u64::from(b)]);
    assert_eq!(w.state().window_state()["tab"], "Home", "the tab that went hands the ribbon back to the first tab");
}

#[test]
fn add_to_underlying_container_is_a_row_of_the_right_click_menu_and_works_on_the_shapes_selected() {
    let mut w = edit_window([1400.0, 900.0]);
    let a = drop_at(&mut w, "rectangle", 4.25, 5.5);
    run_command(&mut w, "container.add", json!({"x": 4.25, "y": 5.5, "width": 4.0, "height": 3.0}));
    select(&mut w, &[a]);
    assert_eq!(w.state().command_enabled("window.containerDo"), Ok(()));
    run_command(&mut w, "window.containerDo", json!({"op": "add_member"}));
    let all = info(&mut w, &[1, 2]);
    let held: Vec<Json> = all["containers"].as_array().unwrap().iter().map(|c| c["members"].clone()).collect();
    assert!(held.contains(&json!([a])), "{all}");
    // A container command with nothing selected is refused in words.
    select(&mut w, &[]);
    assert!(w.state().command_enabled("window.containerDo").is_err());
    let e = w.state_mut().execute("window.containerDo", &json!({"op": "fit"})).unwrap_err();
    assert!(e.to_string().contains("nothing is selected") || e.to_string().contains("disabled"), "{e}");
}

#[test]
fn a_container_is_lit_while_one_of_its_members_is_selected() {
    let (mut w, a, _b) = two_selected();
    press(&mut w, "ribbon.Insert.ContainerGallery");
    w.rest();
    press(&mut w, "menu.container.strip");
    w.rest();
    select(&mut w, &[a]);
    w.rest();
    let held = w.state().window_state()["container"]["heldBy"].clone();
    assert_eq!(held.as_array().unwrap().len(), 1, "the member selected is told which container holds it: {held}");
    assert_eq!(held[0]["id"], a);
}

#[test]
fn an_unknown_operation_and_a_missing_value_are_refused_in_words() {
    let (mut w, _a, _b) = two_selected();
    press(&mut w, "ribbon.Insert.ContainerGallery");
    w.rest();
    press(&mut w, "menu.container.strip");
    w.rest();
    let e = w.state_mut().execute("window.containerDo", &json!({"op": "wiggle"})).unwrap_err();
    assert!(e.to_string().contains("no operation `wiggle`"), "{e}");
    let e = w.state_mut().execute("window.containerDo", &json!({"op": "style"})).unwrap_err();
    assert!(e.to_string().contains("needs a `value`"), "{e}");
    let e = w.state_mut().execute("window.containerDo", &json!({})).unwrap_err();
    assert!(e.to_string().contains("missing `op`"), "{e}");
}

/// A container moved by an arrow key takes its free member and leaves the one whose end a GUARD holds, and the status bar says which and
/// why, so a person who moved it sees why a shape stayed behind.
#[test]
fn a_container_moved_by_a_key_leaves_a_member_a_guard_holds_and_the_status_bar_says_so() {
    use diacraft_engine::Session;
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let id = |v: Json| u32::try_from(v["id"].as_u64().unwrap()).unwrap();
    let a = id(session.execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 4.0})).unwrap());
    let line = id(session.execute("connector.add", &json!({"from": {"x": 4.5, "y": 3.5}, "to": {"x": 6.0, "y": 4.5}})).unwrap());
    let c = id(session.execute("container.add", &json!({"ids": [a, line]})).unwrap());
    session
        .edit("guard", |d| {
            let at = d.find_shape(diacraft_doc::Owner::Page(1), line).unwrap();
            d.write_cell(
                &diacraft_doc::SheetRef::Shape(at),
                &diacraft_doc::CellAddr::direct("BeginX"),
                diacraft_doc::CellEdit::Cell(diacraft_doc::Cell::with_formula("GUARD(4.5)", Some(diacraft_doc::Value::from_text("4.5")))),
                diacraft_doc::WriteOrigin::Automation,
            )
            .map(|_| ())
            .map_err(|e| diacraft_engine::EngineError::Failed { cmd: "test".into(), msg: e.to_string() })
        })
        .unwrap();
    let app = diacraft_ui_egui::DiacraftApp::new(session, None).with_inline_pictures(diacraft_ui_egui::render_worker::bundled_drawer());
    let mut w = crate::support::fast_window(app, [1400.0, 900.0]);
    w.rest();
    let pin = |w: &Window, id: u32, cell: &str| {
        let d = w.state().session().document().unwrap();
        let at = d.find_shape(diacraft_doc::Owner::Page(1), id).unwrap();
        d.shape(&at).unwrap().sheet.cell(cell).and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap()
    };
    let (rect_x, begin_x) = (pin(&w, a, "PinX"), pin(&w, line, "BeginX"));
    select(&mut w, &[c]);
    crate::editing::key(&mut w, egui::Key::ArrowRight, egui::Modifiers::SHIFT);
    assert!(pin(&w, a, "PinX") > rect_x, "the free member went with its container");
    assert_eq!(pin(&w, line, "BeginX"), begin_x, "the guarded one stayed");
    let notice = w.state().notice().unwrap_or_default().to_owned();
    assert!(notice.contains(&format!("shape {line} ")) && notice.contains("GUARD") && notice.contains("left where it was"), "{notice}");
}
