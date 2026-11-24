//! In the window: the Cross-Functional Flowchart tab shows while the page on view has swimlanes (after Help, before Container Format) and goes when it has none, its controls send the
//! engine's commands (Swimlane, Separator, Orientation, Move Earlier and Later, Rotate Lane Label), a right click on a lane has the Swimlane row, and the window state tells what the tab
//! knows. The pointer is given to egui as a person's would be; what is checked is the drawing the commands changed and what the window showed.

use serde_json::{Value as Json, json};

use crate::editing::edit_window;
use crate::support::{Rest, Window, press, run_command};

const TAB: &str = "Cross-Functional Flowchart";

fn tabs(w: &Window) -> Vec<String> {
    diacraft_ui_egui::ribbon::spec::tab_names(w.state().tab_context()).iter().map(|s| (*s).to_owned()).collect()
}

fn enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

fn select(w: &mut Window, ids: &[u32]) {
    run_command(w, "edit.select", json!({"ids": ids}));
}

/// Makes the window draw a frame after the session was changed behind its back, so that it reads the page again.
fn settled(w: &mut Window) {
    select(w, &[]);
    w.rest();
}

fn info(w: &mut Window) -> Json {
    w.state_mut().execute("swimlane.info", &json!({})).unwrap()["structures"][0].clone()
}

fn lane_ids(w: &mut Window) -> Vec<u32> {
    info(w)["lanes"].as_array().unwrap().iter().map(|l| l["id"].as_u64().unwrap() as u32).collect()
}

/// A window with a structure of two lanes and two phases. → the window and the answer of `swimlane.create`.
fn with_lanes() -> (Window, Json) {
    let mut w = edit_window([1400.0, 900.0]);
    let made =
        w.state_mut().execute("swimlane.create", &json!({"x": 1.0, "y": 8.0, "length": 6.0, "thickness": 1.5, "lanes": 2, "phases": 2})).unwrap();
    // The window reads the structure when the ribbon is drawn.
    settled(&mut w);
    (w, made)
}

fn ids(v: &Json) -> Vec<u32> {
    v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()
}

#[test]
fn the_tab_shows_while_the_page_has_swimlanes_after_help_and_goes_when_it_has_none() {
    let mut w = edit_window([1400.0, 900.0]);
    assert!(!tabs(&w).iter().any(|t| t == TAB), "a page with no lanes has no tab: {:?}", tabs(&w));
    let made = w.state_mut().execute("swimlane.create", &json!({"x": 1.0, "y": 8.0, "lanes": 2, "phases": 1})).unwrap();
    settled(&mut w);
    let names = tabs(&w);
    assert_eq!(names.last().map(String::as_str), Some(TAB), "the template's tab is at the right end of the row: {names:?}");
    assert!(names.iter().position(|t| t == "Help") < names.iter().position(|t| t == TAB));
    // With a lane selected, Container Format comes after it (a template tab, then Container Format).
    let lanes = ids(&made["lanes"]);
    select(&mut w, &[lanes[0]]);
    w.rest();
    let names = tabs(&w);
    assert_eq!(names[names.len() - 2..], [TAB.to_owned(), "Container Format".to_owned()], "{names:?}");
    // One undo takes the structure back, and the tab goes.
    select(&mut w, &[]);
    run_command(&mut w, "edit.undo", json!({}));
    settled(&mut w);
    assert!(!tabs(&w).iter().any(|t| t == TAB));
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn the_swimlane_button_inserts_a_lane_after_the_lane_selected_and_selects_it() {
    let (mut w, made) = with_lanes();
    let lanes = ids(&made["lanes"]);
    select(&mut w, &[lanes[0]]);
    run_command(&mut w, "window.tab", json!({"tab": TAB}));
    w.rest();
    assert!(enabled(&w, &format!("ribbon.{TAB}.InsertSwimlane")), "Swimlane is live");
    press(&mut w, &format!("ribbon.{TAB}.InsertSwimlane"));
    w.rest();
    let now = lane_ids(&mut w);
    assert_eq!(now.len(), 3);
    assert_eq!(now[0], lanes[0]);
    assert_eq!(now[2], lanes[1], "the new lane went after the lane that was selected: {now:?}");
    let state = w.state().window_state();
    assert_eq!(state["selection"], json!([now[1]]), "the new lane is selected");
    assert_eq!(state["swimlane"]["present"], true);
    assert_eq!(state["swimlane"]["selected"][0]["role"], "lane");
    // The command of the window says it in words when there are no swimlanes.
    let mut plain = edit_window([1400.0, 900.0]);
    let e = plain.state_mut().execute("window.swimlaneDo", &json!({"op": "insert"})).unwrap_err();
    assert!(e.to_string().contains("no swimlanes"), "{e}");
}

#[test]
fn the_swimlane_menu_inserts_before_and_the_separator_button_splits_the_phase_selected() {
    let (mut w, made) = with_lanes();
    let (lanes, phases) = (ids(&made["lanes"]), ids(&made["phases"]));
    select(&mut w, &[lanes[1]]);
    run_command(&mut w, "window.tab", json!({"tab": TAB}));
    w.rest();
    run_command(&mut w, "window.swimlaneDo", json!({"op": "insert", "value": "before"}));
    let now = lane_ids(&mut w);
    assert_eq!(now[0], lanes[0]);
    assert_eq!(now[2], lanes[1], "before the lane selected: {now:?}");
    select(&mut w, &[phases[0]]);
    w.rest();
    press(&mut w, &format!("ribbon.{TAB}.InsertSeparator"));
    w.rest();
    assert_eq!(info(&mut w)["phases"].as_array().unwrap().len(), 3);
    // A value that is neither is refused in words.
    let e = w.state_mut().execute("window.swimlaneDo", &json!({"op": "insert", "value": "beside"})).unwrap_err();
    assert!(e.to_string().contains("before or after"), "{e}");
}

#[test]
fn rotate_lane_label_turns_the_labels_of_the_lanes_selected_or_of_all_when_none_is() {
    let (mut w, made) = with_lanes();
    let lanes = ids(&made["lanes"]);
    run_command(&mut w, "window.tab", json!({"tab": TAB}));
    select(&mut w, &[lanes[0]]);
    w.rest();
    press(&mut w, &format!("ribbon.{TAB}.RotateLaneLabel"));
    w.rest();
    let st = info(&mut w);
    assert_eq!((st["lanes"][0]["heading"].as_str(), st["lanes"][1]["heading"].as_str()), (Some("top"), Some("left")));
    // With nothing selected, every lane of the page's structure.
    select(&mut w, &[]);
    w.rest();
    run_command(&mut w, "window.swimlaneDo", json!({"op": "rotate_label"}));
    let st = info(&mut w);
    assert_eq!((st["lanes"][0]["heading"].as_str(), st["lanes"][1]["heading"].as_str()), (Some("left"), Some("top")));
}

#[test]
fn the_orientation_menu_turns_the_structure_and_marks_the_one_it_has_and_move_later_moves_the_lane() {
    let (mut w, made) = with_lanes();
    let lanes = ids(&made["lanes"]);
    run_command(&mut w, "window.tab", json!({"tab": TAB}));
    w.rest();
    run_command(&mut w, "window.swimlaneDo", json!({"op": "orientation", "value": "vertical"}));
    w.rest();
    assert_eq!(info(&mut w)["orientation"], "vertical");
    assert_eq!(w.state().window_state()["swimlane"]["orientation"], "vertical");
    select(&mut w, &[lanes[0]]);
    w.rest();
    press(&mut w, &format!("ribbon.{TAB}.SwimlaneLater"));
    w.rest();
    assert_eq!(lane_ids(&mut w), [lanes[1], lanes[0]], "the lane selected is one place on");
    run_command(&mut w, "window.swimlaneDo", json!({"op": "earlier"}));
    assert_eq!(lane_ids(&mut w), [lanes[0], lanes[1]]);
    let e = w.state_mut().execute("window.swimlaneDo", &json!({"op": "orientation"})).unwrap_err();
    assert!(e.to_string().contains("needs a `value`"), "{e}");
}

#[test]
fn a_right_click_on_a_lane_has_the_swimlane_row_and_on_other_shapes_it_does_not() {
    let (mut w, made) = with_lanes();
    let lanes = ids(&made["lanes"]);
    select(&mut w, &[lanes[0]]);
    w.rest();
    let rows: Vec<String> = diacraft_ui_egui::ribbon::spec::LANE_MENU.iter().map(|c| c.id.to_owned()).collect();
    assert!(rows.contains(&"CanvasSwimlane".to_owned()));
    assert!(!diacraft_ui_egui::ribbon::spec::CANVAS_MENU.iter().any(|c| c.id == "CanvasSwimlane"));
    assert_eq!(w.state().window_state()["swimlane"]["selected"][0]["id"], lanes[0]);
}

#[test]
fn every_control_of_the_tab_is_drawn_and_enabled_while_the_page_has_swimlanes_and_the_ribbon_folds_its_groups() {
    let tab = diacraft_ui_egui::ribbon::spec::tab_by_label(TAB).expect("the tab is in the tables");
    assert_eq!(tab.groups.iter().map(|g| g.label).collect::<Vec<_>>(), ["Insert", "Arrange", "Design"], "Names the first and the last");
    for width in [480.0_f32, 800.0, 1400.0] {
        let mut w = edit_window([width, 720.0]);
        w.state_mut().execute("swimlane.create", &json!({"x": 1.0, "y": 8.0, "lanes": 2, "phases": 1})).unwrap();
        settled(&mut w);
        run_command(&mut w, "window.tab", json!({"tab": TAB}));
        w.rest();
        let groups = crate::support::elements_named(&w, &format!("ribbon.{TAB}.group."));
        assert_eq!(groups.len(), tab.groups.len(), "at {width}: a group is missing");
        if width >= 1400.0 {
            for group in tab.groups {
                for ctl in group.top_level() {
                    assert!(enabled(&w, &format!("ribbon.{TAB}.{}", ctl.id)), "{} is live", ctl.id);
                }
            }
        }
    }
}
