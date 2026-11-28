//! The Layout and Routing tab of Page Setup, its Spacing dialog, its preview and the connector menu's three style rows.
//!
//! The tab reads `page.routing` and works on the dialog's draft: OK sends one `command.batch` labelled Page setup with a `page.set_routing` and a
//! `connector.line_jumps` that hold only what the person changed, and Cancel and Escape send nothing. What a test checks is the document the dialog
//! changed through those commands, and what `ui.inspect` and `ui.elements` report of the tab.

use diacraft_engine::draw::routing::page_routing;
use egui::{Key, Modifiers, Pos2, Rect, vec2};
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};

use crate::editing::{at, edit_window, key, shape_ids, type_text};
use crate::support::{Rest, Window, click, controlled_with, elements_named, move_to, press, run_command};

const TAB: &str = "Layout and Routing";

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

/// A new drawing on the Design tab.
fn design_window() -> Window {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "window.tab", json!({"tab": "Design"}));
    w
}

fn open_tab(w: &mut Window) {
    run_command(w, "window.pageSetup", json!({"tab": TAB}));
    assert_eq!(w.state().window_state()["pageSetup"], TAB);
}

/// What the tab holds, as the window reports it.
fn tab(w: &Window) -> Value {
    w.state().window_state()["pageSetupRouting"].clone()
}

/// `page.routing` of the first page (a query).
fn routing(w: &mut Window) -> Value {
    w.state_mut().execute("page.routing", &json!({})).unwrap()
}

fn rect_of(w: &Window, name: &str) -> Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

fn is_enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

fn click_on(w: &mut Window, name: &str) {
    let at = rect_of(w, name).center();
    click(w, at);
    w.rest();
}

/// Opens the list `combo` and takes the row `item` of it.
fn choose(w: &mut Window, combo: &str, item: &str) {
    press(w, combo);
    w.rest();
    assert!(w.state().element(item).is_some(), "{item} is not in the open list: {:?}", elements_named(w, &format!("{combo}.")));
    press(w, item);
    w.rest();
}

/// Types `text` into the box `name`, over what it held.
fn type_into(w: &mut Window, name: &str, text: &str) {
    click_on(w, name);
    key(w, Key::A, crate::saving::ctrl());
    type_text(w, text);
}

fn ok(w: &mut Window) {
    press(w, "pagesetup.ok");
    w.rest();
}

fn undo_steps(w: &Window) -> usize {
    w.state().session().history().map_or(0, |h| h.undo_len())
}

fn undo_label(w: &Window) -> Option<String> {
    w.state().session().history().and_then(|h| h.next_undo_label().map(str::to_owned))
}

fn escape(w: &mut Window) {
    key(w, Key::Escape, Modifiers::NONE);
}

/// The page's routing cells, in drawing units.
fn cells(w: &Window) -> diacraft_engine::draw::routing::PageRouting {
    page_routing(w.state().session().document().unwrap(), page_id(w))
}

// ---- the tab shows what the page says --------------------------------------------------------------------------------------------------------

#[test]
fn the_layout_launcher_opens_the_tab_and_the_command_names_it() {
    let mut w = design_window();
    press(&mut w, "ribbon.Design.PageSetupLayoutAndRoutingDialog");
    w.rest();
    assert_eq!(w.state().window_state()["pageSetup"], TAB, "the launcher of the Layout group opens Layout and Routing");
    assert!(w.state().is_modal_open());
    assert!(w.state().element("pagesetup.tab.Layout and Routing").is_some_and(|e| e.enabled), "the tab works now");
    run_command(&mut w, "window.pageSetup", json!({"close": true}));
    // The command takes the tab's name, in any case; Alt Text has its boxes.
    run_command(&mut w, "window.pageSetup", json!({"tab": "layout and routing"}));
    assert_eq!(w.state().window_state()["pageSetup"], TAB);
    run_command(&mut w, "window.pageSetup", json!({"close": true}));
    run_command(&mut w, "window.pageSetup", json!({"tab": "alt text"}));
    assert_eq!(w.state().window_state()["pageSetup"], "Alt Text");
    run_command(&mut w, "window.pageSetup", json!({"close": true}));
    let unknown = w.state_mut().execute("window.pageSetup", &json!({"tab": "Nonsense"})).unwrap_err().to_string();
    assert!(unknown.contains("Layout and Routing") && unknown.contains("Alt Text"), "the tabs are listed, these among them: {unknown}");
}

#[test]
fn a_new_page_shows_the_roots_values_and_a_greyed_direction() {
    let mut w = design_window();
    open_tab(&mut w);
    let now = tab(&w);
    let page = routing(&mut w);
    assert_eq!(now["style"], page["style"]);
    assert_eq!(now["style"], "right_angle");
    assert!(now["direction"].is_null());
    assert_eq!(now["directionEnabled"], false);
    assert!(!is_enabled(&w, "pagesetup.routing.direction"), "Direction is greyed for Right Angle");
    assert_eq!((&now["separate"], &now["overlap"], &now["appearance"]), (&page["separate"], &page["overlap"], &page["appearance"]));
    assert_eq!(now["jumps"]["to"], page["line_jumps"]["code"], "Add line jumps to shows the page's code");
    assert_eq!(now["jumps"]["style"], "arc");
    assert_eq!(now["jumps"]["vertical"], "0.6667", "the root's two thirds");
    assert_eq!(now["jumps"]["horizontal"], "0.6667");
    assert_eq!(now["unit"], "in", "a US drawing is shown in inches");
    assert_eq!(
        now["spacing"],
        json!({
            "between_shapes": ["0.375", "0.375"],
            "shape_size": ["0.25", "0.25"],
            "connector_to_connector": ["0.125", "0.125"],
            "connector_to_shape": ["0.125", "0.125"],
        }),
        "the root's 3/8, 1/4, 1/8 and 1/8 inch"
    );
    assert_eq!(now["other"], json!({"plow": page["plow"], "split": page["split"]}));
    assert!(now["spacingDialog"].is_null());
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn the_tab_shows_what_page_routing_gives_for_a_page_set_up_by_commands() {
    let mut w = design_window();
    run_command(
        &mut w,
        "page.set_routing",
        json!({
            "style": "flowchart", "direction": "left_to_right", "separate": "all", "overlap": "none", "appearance": "curved",
            "plow": true, "split": false,
            "between_shapes": {"x": 0.5, "y": 0.625}, "shape_size": {"x": 1.5, "y": 1.0},
            "connector_to_connector": {"x": 0.25, "y": 0.375}, "connector_to_shape": {"x": 0.3, "y": 0.2},
        }),
    );
    run_command(&mut w, "connector.line_jumps", json!({"code": 3, "style": 3, "factor_x": 2.0, "factor_y": 3.5}));
    open_tab(&mut w);
    let now = tab(&w);
    assert_eq!((now["style"].as_str(), now["direction"].as_str()), (Some("flowchart"), Some("left_to_right")));
    assert_eq!(now["directionEnabled"], true, "A flowchart takes a direction, and Direction is enabled");
    assert!(is_enabled(&w, "pagesetup.routing.direction"));
    assert_eq!((&now["separate"], &now["overlap"], &now["appearance"]), (&json!("all"), &json!("none"), &json!("curved")));
    assert_eq!(now["other"], json!({"plow": true, "split": false}));
    assert_eq!(now["jumps"], json!({"to": 3, "style": "square", "vertical": "3.5", "horizontal": "2"}), "Vertical is FactorY and Horizontal FactorX");
    assert_eq!(
        now["spacing"],
        json!({
            "between_shapes": ["0.5", "0.625"], "shape_size": ["1.5", "1"],
            "connector_to_connector": ["0.25", "0.375"], "connector_to_shape": ["0.3", "0.2"],
        })
    );
}

#[test]
fn direction_is_greyed_for_right_angle_straight_network_and_center_to_center() {
    let mut w = design_window();
    open_tab(&mut w);
    for (word, takes) in [
        ("org_chart", true),
        ("flowchart", true),
        ("tree", true),
        ("simple", true),
        ("right_angle", false),
        ("straight", false),
        ("network", false),
        ("centre_to_centre", false),
    ] {
        choose(&mut w, "pagesetup.routing.style", &format!("pagesetup.routing.style.{word}"));
        assert_eq!(tab(&w)["style"], word);
        assert_eq!(is_enabled(&w, "pagesetup.routing.direction"), takes, "{word}");
        assert_eq!(tab(&w)["directionEnabled"], takes, "{word}");
        assert_eq!(tab(&w)["direction"].is_null(), !takes, "{word}: a style with no direction has none to send");
    }
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn the_directions_a_style_takes_are_listed_and_the_simple_style_has_two_more() {
    let mut w = design_window();
    open_tab(&mut w);
    choose(&mut w, "pagesetup.routing.style", "pagesetup.routing.style.flowchart");
    assert_eq!(tab(&w)["direction"], "top_to_bottom", "a style that takes a direction starts from the top");
    press(&mut w, "pagesetup.routing.direction");
    w.rest();
    for word in ["top_to_bottom", "left_to_right", "bottom_to_top", "right_to_left"] {
        assert!(w.state().element(&format!("pagesetup.routing.direction.{word}")).is_some(), "{word} is listed for a flowchart");
    }
    assert!(w.state().element("pagesetup.routing.direction.level_first").is_none(), "level first is the simple style's alone");
    escape(&mut w);
    assert_eq!(w.state().window_state()["pageSetup"], TAB, "Escape closed the list and not the dialog");
    choose(&mut w, "pagesetup.routing.direction", "pagesetup.routing.direction.bottom_to_top");
    assert_eq!(tab(&w)["direction"], "bottom_to_top");
    // Another style that takes a direction keeps it; the simple style lists two more, and a direction it does not take starts over.
    choose(&mut w, "pagesetup.routing.style", "pagesetup.routing.style.simple");
    assert_eq!(tab(&w)["direction"], "bottom_to_top");
    press(&mut w, "pagesetup.routing.direction");
    w.rest();
    for word in ["level_first", "upright_first"] {
        assert!(w.state().element(&format!("pagesetup.routing.direction.{word}")).is_some(), "{word} is listed for the simple style");
    }
    press(&mut w, "pagesetup.routing.direction.level_first");
    w.rest();
    assert_eq!(tab(&w)["direction"], "level_first");
    choose(&mut w, "pagesetup.routing.style", "pagesetup.routing.style.tree");
    assert_eq!(tab(&w)["direction"], "top_to_bottom", "a tree does not take level first");
    cancel(&mut w);
}

fn cancel(w: &mut Window) {
    press(w, "pagesetup.cancel");
    w.rest();
}

// ---- OK, Cancel and Escape -------------------------------------------------------------------------------------------------------------------

/// Separate to All Lines and OK is one `command.batch` labelled Page setup with one `page.set_routing` and no `connector.line_jumps`.
#[test]
fn ok_sends_one_batch_labelled_page_setup_with_only_what_changed() {
    let mut w = design_window();
    let before = routing(&mut w);
    open_tab(&mut w);
    choose(&mut w, "pagesetup.routing.separate", "pagesetup.routing.separate.all");
    let (revision, steps) = (w.state().session().revision(), undo_steps(&w));
    assert_eq!(w.state().session().revision(), revision, "nothing is sent before OK");
    ok(&mut w);
    assert!(w.state().window_state()["pageSetup"].is_null(), "OK closes the dialog");
    assert_eq!(w.state().session().revision(), revision + 1, "one command");
    assert_eq!(undo_steps(&w), steps + 1, "one undo step");
    assert_eq!(undo_label(&w).as_deref(), Some("Page setup"), "a batch, with the dialog's label, though it holds one command");
    let now = routing(&mut w);
    assert_eq!(now["separate"], "all");
    for key in ["style", "overlap", "appearance", "plow", "split", "between_shapes", "shape_size", "connector_to_connector", "connector_to_shape"] {
        assert_eq!(now[key], before[key], "{key} was not touched");
    }
    let held = &now["held"];
    assert_eq!(held["separate"], true);
    for untouched in
        ["style", "overlap", "appearance", "plow", "split", "between_shapes", "shape_size", "connector_to_connector", "connector_to_shape"]
    {
        assert_eq!(held[untouched], before["held"][untouched], "{untouched}: no cell nobody touched is written");
    }
    assert_eq!(
        held["line_jumps"], false,
        "no `connector.line_jumps` went with it: that command writes the page's cells even for the values they hold"
    );
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(routing(&mut w), before, "one undo step puts it all back");
}

#[test]
fn a_change_of_routing_and_of_jumps_goes_in_the_same_one_undo_step() {
    let mut w = design_window();
    let before = routing(&mut w);
    open_tab(&mut w);
    choose(&mut w, "pagesetup.routing.overlap", "pagesetup.routing.overlap.all");
    choose(&mut w, "pagesetup.routing.appearance", "pagesetup.routing.appearance.curved");
    choose(&mut w, "pagesetup.routing.jump_to", "pagesetup.routing.jump_to.2");
    choose(&mut w, "pagesetup.routing.jump_style", "pagesetup.routing.jump_style.gap");
    type_into(&mut w, "pagesetup.routing.vertical_size.box", "4");
    let steps = undo_steps(&w);
    ok(&mut w);
    assert_eq!(undo_steps(&w), steps + 1, "both commands are one undo step");
    assert_eq!(undo_label(&w).as_deref(), Some("Page setup"));
    let now = routing(&mut w);
    assert_eq!((&now["overlap"], &now["appearance"]), (&json!("all"), &json!("curved")));
    assert_eq!(now["separate"], before["separate"]);
    assert_eq!(now["line_jumps"]["code"], 2);
    assert_eq!(now["line_jumps"]["style"], 2, "Gap is the code 2");
    assert!(near(number(&now["line_jumps"]["factor_y"]), 4.0), "Vertical size is FactorY: {now}");
    assert!(near(number(&now["line_jumps"]["factor_x"]), number(&before["line_jumps"]["factor_x"])), "Horizontal size was not changed: {now}");
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(routing(&mut w)["overlap"], before["overlap"]);
    assert_eq!(routing(&mut w)["line_jumps"], before["line_jumps"]);
}

#[test]
fn ok_with_nothing_changed_sends_nothing() {
    let mut w = design_window();
    open_tab(&mut w);
    // A list set to the value it has is no change either.
    choose(&mut w, "pagesetup.routing.separate", "pagesetup.routing.separate.unrelated");
    let (revision, steps) = (w.state().session().revision(), undo_steps(&w));
    ok(&mut w);
    assert!(w.state().window_state()["pageSetup"].is_null());
    assert_eq!(w.state().session().revision(), revision, "no command was sent");
    assert_eq!(undo_steps(&w), steps);
    assert!(!w.state().is_dirty());
}

#[test]
fn cancel_and_escape_send_nothing() {
    let mut w = design_window();
    let revision = w.state().session().revision();
    open_tab(&mut w);
    choose(&mut w, "pagesetup.routing.style", "pagesetup.routing.style.network");
    choose(&mut w, "pagesetup.routing.jump_to", "pagesetup.routing.jump_to.0");
    click_on(&mut w, "pagesetup.routing.plow");
    cancel(&mut w);
    assert!(w.state().window_state()["pageSetup"].is_null());
    open_tab(&mut w);
    assert_eq!(tab(&w)["style"], "right_angle", "a dialog opened again starts from the page and not from the draft that was thrown away");
    choose(&mut w, "pagesetup.routing.style", "pagesetup.routing.style.network");
    escape(&mut w);
    assert!(w.state().window_state()["pageSetup"].is_null(), "Escape is Cancel");
    assert_eq!(w.state().session().revision(), revision);
    assert!(!w.state().is_dirty());
}

#[test]
fn the_draft_is_kept_while_another_tab_is_shown() {
    let mut w = design_window();
    open_tab(&mut w);
    choose(&mut w, "pagesetup.routing.style", "pagesetup.routing.style.tree");
    press(&mut w, "pagesetup.tab.Page Size");
    w.rest();
    assert_eq!(w.state().window_state()["pageSetup"], "Page Size");
    press(&mut w, "pagesetup.tab.Layout and Routing");
    w.rest();
    assert_eq!(tab(&w)["style"], "tree", "what was chosen is still chosen");
    ok(&mut w);
    assert_eq!(routing(&mut w)["style"], "tree");
}

/// A new Style routes again the connectors that follow the page and are set to reroute freely; Appearance, Separate and the spacing route nothing.
#[test]
fn a_new_style_routes_the_connectors_that_follow_the_page_and_the_other_cells_route_nothing() {
    let mut w = design_window();
    let page = page_id(&w);
    let drop = |w: &mut Window, x: f64, y: f64| -> u32 {
        let answer = w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": page, "x": x, "y": y})).unwrap();
        u32::try_from(answer["id"].as_u64().unwrap()).unwrap()
    };
    let (a, b) = (drop(&mut w, 2.0, 3.0), drop(&mut w, 6.0, 5.0));
    let added = w.state_mut().execute("connector.add", &json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}, "page": page})).unwrap();
    let c = added["id"].as_u64().unwrap();
    let points = |w: &mut Window| -> usize {
        let path = w.state_mut().execute("connector.path", &json!({"id": c, "page": page})).unwrap();
        path["points"].as_array().map_or(0, Vec::len)
    };
    w.rest();
    assert_eq!(points(&mut w), 4, "a Z of three legs");
    open_tab(&mut w);
    choose(&mut w, "pagesetup.routing.appearance", "pagesetup.routing.appearance.curved");
    choose(&mut w, "pagesetup.routing.separate", "pagesetup.routing.separate.none");
    ok(&mut w);
    assert_eq!(points(&mut w), 4, "the appearance and Separate route nothing");
    open_tab(&mut w);
    choose(&mut w, "pagesetup.routing.style", "pagesetup.routing.style.straight");
    ok(&mut w);
    assert_eq!(points(&mut w), 2, "the connector follows the page, so it is one leg now");
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(points(&mut w), 4, "the new route and the page's style were one undo step");
}

// ---- the jump boxes --------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_jump_size_is_a_number_from_0_to_10_and_any_other_is_told_in_words() {
    let mut w = design_window();
    open_tab(&mut w);
    let (revision, steps) = (w.state().session().revision(), undo_steps(&w));
    for bad in ["abc", "11", "-1", "", "1e400"] {
        type_into(&mut w, "pagesetup.routing.horizontal_size.box", bad);
        ok(&mut w);
        assert_eq!(w.state().window_state()["pageSetup"], TAB, "`{bad}`: the dialog stays");
        w.get_by_label_contains("from 0 to 10");
        assert_eq!(w.state().session().revision(), revision, "`{bad}`: nothing was sent");
    }
    type_into(&mut w, "pagesetup.routing.horizontal_size.box", "2.5");
    ok(&mut w);
    assert!(w.state().window_state()["pageSetup"].is_null());
    assert_eq!(undo_steps(&w), steps + 1);
    let now = routing(&mut w);
    assert!(near(number(&now["line_jumps"]["factor_x"]), 2.5), "{now}");
    assert!(near(number(&now["line_jumps"]["factor_y"]), 2.0 / 3.0), "the other size was not sent: {now}");
}

#[test]
fn the_slider_and_its_box_show_one_number() {
    let mut w = design_window();
    open_tab(&mut w);
    let slider = rect_of(&w, "pagesetup.routing.vertical_size");
    // A press at the right end of the rail is the most the slider gives.
    click(&mut w, Pos2::new(slider.right() - 1.0, slider.center().y));
    w.rest();
    assert_eq!(tab(&w)["jumps"]["vertical"], "10", "the box follows the slider: {}", tab(&w)["jumps"]);
    click(&mut w, Pos2::new(slider.left() + 1.0, slider.center().y));
    w.rest();
    assert_eq!(tab(&w)["jumps"]["vertical"], "0");
    type_into(&mut w, "pagesetup.routing.vertical_size.box", "5");
    assert_eq!(tab(&w)["jumps"]["vertical"], "5", "a number typed in the box is the slider's too");
    ok(&mut w);
    assert!(near(number(&routing(&mut w)["line_jumps"]["factor_y"]), 5.0));
}

// ---- the Other group -------------------------------------------------------------------------------------------------------------------------

#[test]
fn the_two_other_boxes_write_their_cells_and_say_in_their_tip_that_they_are_not_acted_on() {
    let mut w = design_window();
    let page = page_id(&w);
    let drop = |w: &mut Window, x: f64, y: f64| -> u32 {
        let answer = w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": page, "x": x, "y": y})).unwrap();
        u32::try_from(answer["id"].as_u64().unwrap()).unwrap()
    };
    drop(&mut w, 2.0, 3.0);
    drop(&mut w, 3.0, 3.0);
    w.rest();
    let frames =
        |w: &mut Window| -> Vec<(u32, (f64, f64, f64, f64))> { shape_ids(w).into_iter().map(|id| (id, crate::editing::frame_of(w, id))).collect() };
    let shapes = frames(&mut w);
    let before = routing(&mut w);
    open_tab(&mut w);
    assert_eq!(tab(&w)["other"], json!({"plow": before["plow"], "split": before["split"]}));
    // The words are on the page of the tab: one line under the group, and each box's tooltip.
    w.get_by_label_contains("does not act on these two");
    click_on(&mut w, "pagesetup.routing.plow");
    click_on(&mut w, "pagesetup.routing.split");
    let flipped = json!({"plow": !before["plow"].as_bool().unwrap(), "split": !before["split"].as_bool().unwrap()});
    assert_eq!(tab(&w)["other"], flipped);
    ok(&mut w);
    let now = routing(&mut w);
    assert_eq!((&now["plow"], &now["split"]), (&flipped["plow"], &flipped["split"]), "written as the page's cells");
    assert_eq!((&now["held"]["plow"], &now["held"]["split"]), (&json!(true), &json!(true)));
    assert_eq!(frames(&mut w), shapes, "No shape moved: DiaCraft does not move shapes aside on a drop");
    // The tooltip of the box says the same.
    open_tab(&mut w);
    hover_for_a_tip(&mut w, "pagesetup.routing.plow");
    w.get_by_label_contains("does not move shapes aside");
    hover_for_a_tip(&mut w, "pagesetup.routing.split");
    w.get_by_label_contains("does not split a connector");
}

/// The greyed Direction says which styles take one ("available for the Flowchart, Tree, Organization Chart and Simple styles").
#[test]
fn the_greyed_direction_says_which_styles_take_one() {
    let mut w = design_window();
    open_tab(&mut w);
    assert!(!is_enabled(&w, "pagesetup.routing.direction"));
    hover_for_a_tip(&mut w, "pagesetup.routing.direction");
    w.get_by_label_contains("A direction is for the Organization Chart, Flowchart, Tree and Simple styles");
}

/// Holds the pointer over the control for longer than a tooltip waits.
fn hover_for_a_tip(w: &mut Window, name: &str) {
    let at = rect_of(w, name).center();
    for _ in 0..4 {
        move_to(w, at);
    }
    for _ in 0..60 {
        w.step();
    }
}

// ---- the Spacing dialog ----------------------------------------------------------------------------------------------------------------------

fn open_spacing(w: &mut Window) {
    press(w, "pagesetup.routing.spacing");
    w.rest();
    assert!(!tab(w)["spacingDialog"].is_null(), "Spacing... opens its dialog");
}

fn spacing_ok(w: &mut Window) {
    press(w, "routingspacing.ok");
    w.rest();
}

const GROUPS: [&str; 4] = ["between_shapes", "shape_size", "connector_to_connector", "connector_to_shape"];

fn boxes(v: &Value) -> Vec<(String, String)> {
    GROUPS.iter().map(|g| (v[*g][0].as_str().unwrap().to_owned(), v[*g][1].as_str().unwrap().to_owned())).collect()
}

fn words(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(x, y)| ((*x).to_owned(), (*y).to_owned())).collect()
}

#[test]
fn the_spacing_dialog_has_four_groups_of_two_lengths_and_cancel_keeps_the_tab_as_it_was() {
    let mut w = design_window();
    open_tab(&mut w);
    open_spacing(&mut w);
    for group in GROUPS {
        for axis in ["x", "y"] {
            assert!(is_enabled(&w, &format!("routingspacing.{group}.{axis}")), "{group} {axis}");
        }
    }
    for name in ["routingspacing.defaults", "routingspacing.ok", "routingspacing.cancel"] {
        assert!(is_enabled(&w, name), "{name}");
    }
    assert_eq!(boxes(&tab(&w)["spacingDialog"]), boxes(&tab(&w)["spacing"]), "the dialog opens on the tab's values");
    type_into(&mut w, "routingspacing.connector_to_shape.x", "0.9");
    press(&mut w, "routingspacing.cancel");
    w.rest();
    assert!(tab(&w)["spacingDialog"].is_null());
    assert_eq!(tab(&w)["spacing"]["connector_to_shape"], json!(["0.125", "0.125"]), "Cancel keeps what the tab had");
    assert_eq!(w.state().window_state()["pageSetup"], TAB, "the Page Setup dialog is still open under it");
    // Escape closes the Spacing dialog first and the Page Setup dialog on its next press.
    open_spacing(&mut w);
    escape(&mut w);
    assert!(tab(&w)["spacingDialog"].is_null());
    assert_eq!(w.state().window_state()["pageSetup"], TAB);
    escape(&mut w);
    assert!(w.state().window_state()["pageSetup"].is_null());
}

/// V8, US: Defaults gives 3/8, 1/4, 1/8 and 1/8 inch, and a page that held 0.5 now holds 0.125.
#[test]
fn defaults_give_the_roots_values_in_inches_for_a_us_drawing() {
    let mut w = design_window();
    run_command(&mut w, "page.set_routing", json!({"connector_to_shape": {"x": 0.5, "y": 0.5}, "between_shapes": {"x": 2.0, "y": 3.0}}));
    open_tab(&mut w);
    assert_eq!(tab(&w)["spacing"]["connector_to_shape"], json!(["0.5", "0.5"]));
    open_spacing(&mut w);
    press(&mut w, "routingspacing.defaults");
    w.rest();
    let defaults = words(&[("0.375", "0.375"), ("0.25", "0.25"), ("0.125", "0.125"), ("0.125", "0.125")]);
    assert_eq!(boxes(&tab(&w)["spacingDialog"]), defaults, "Defaults fills the boxes of the dialog: nothing is sent yet");
    assert_eq!(cells(&w).connector_to_shape.0, 0.5, "the page still holds what it held");
    spacing_ok(&mut w);
    assert!(tab(&w)["spacingDialog"].is_null());
    assert_eq!(boxes(&tab(&w)["spacing"]), defaults, "OK puts them in the tab's draft");
    assert_eq!(cells(&w).connector_to_shape.0, 0.5, "and sends nothing yet");
    ok(&mut w);
    let now = cells(&w);
    assert!(near(now.connector_to_shape.0, 0.125) && near(now.connector_to_shape.1, 0.125), "{now:?}");
    assert!(near(now.connector_to_connector.0, 0.125));
    assert!(near(now.between_shapes.0, 0.375) && near(now.between_shapes.1, 0.375));
    assert!(near(now.shape_size.0, 0.25) && near(now.shape_size.1, 0.25));
}

/// V8, metric: Defaults gives 7.5, 5, 2.5 and 2.5 mm, which the command takes as paper inches.
#[test]
fn defaults_give_the_roots_values_in_millimetres_for_a_metric_drawing() {
    let mut w = design_window();
    run_command(&mut w, "page.scale", json!({"paper": 1, "paper_unit": "mm", "drawing": 1, "drawing_unit": "mm"}));
    open_tab(&mut w);
    assert_eq!(tab(&w)["unit"], "mm", "a drawing that is scaled in millimetres is shown in them");
    open_spacing(&mut w);
    press(&mut w, "routingspacing.defaults");
    w.rest();
    assert_eq!(
        boxes(&tab(&w)["spacingDialog"]),
        words(&[("7.5", "7.5"), ("5", "5"), ("2.5", "2.5"), ("2.5", "2.5")]),
        "the metric root's 7.5, 5, 2.5 and 2.5 mm"
    );
    spacing_ok(&mut w);
    ok(&mut w);
    let now = cells(&w);
    for (got, mm) in [
        (now.between_shapes.0, 7.5),
        (now.between_shapes.1, 7.5),
        (now.shape_size.0, 5.0),
        (now.connector_to_connector.1, 2.5),
        (now.connector_to_shape.0, 2.5),
    ] {
        assert!(near(got, mm / 25.4), "{got} is not {mm} mm");
    }
    // The tab reads them back in millimetres.
    open_tab(&mut w);
    assert_eq!(boxes(&tab(&w)["spacing"]), words(&[("7.5", "7.5"), ("5", "5"), ("2.5", "2.5"), ("2.5", "2.5")]));
}

/// V8, scaled: Defaults are lengths on paper, so on a page of 1 inch to 10 inches `LineToLineX` is written 1.25, and the window shows what it showed.
#[test]
fn on_a_scaled_page_defaults_are_paper_lengths_written_as_drawing_lengths() {
    let mut w = design_window();
    run_command(&mut w, "page.scale", json!({"paper": 1, "paper_unit": "in", "drawing": 10, "drawing_unit": "in"}));
    open_tab(&mut w);
    // What the window shows of a page that holds none of the cells: the engine's default for a missing cell is 0.125 drawing inch, which is 0.0125 inch on paper.
    assert_eq!(
        tab(&w)["spacing"]["connector_to_shape"],
        json!(["0.0125", "0.0125"]),
        "an absent cell reads 0.125 drawing inch (1.4 of the design), shown as the paper length it is on this page"
    );
    assert_eq!(tab(&w)["unit"], "in");
    open_spacing(&mut w);
    press(&mut w, "routingspacing.defaults");
    w.rest();
    assert_eq!(
        boxes(&tab(&w)["spacingDialog"]),
        words(&[("0.375", "0.375"), ("0.25", "0.25"), ("0.125", "0.125"), ("0.125", "0.125")]),
        "Defaults are the root's paper values, not the drawing-unit default of an absent cell"
    );
    spacing_ok(&mut w);
    ok(&mut w);
    let now = cells(&w);
    assert!(near(now.connector_to_connector.0, 1.25), "LineToLineX is 0.125 paper inch times the scale of 10: {now:?}");
    assert!(near(now.connector_to_shape.0, 1.25) && near(now.between_shapes.0, 3.75) && near(now.shape_size.1, 2.5), "{now:?}");
    assert_eq!(routing(&mut w)["connector_to_connector"], json!({"x": 0.125, "y": 0.125}), "and it reads back as the paper length");
}

#[test]
fn a_spacing_box_that_is_no_length_is_told_in_words_and_the_dialog_stays() {
    let mut w = design_window();
    open_tab(&mut w);
    open_spacing(&mut w);
    for (bad, words) in [("abc", "length"), ("0", "more than 0"), ("-1", "more than 0"), ("", "length")] {
        type_into(&mut w, "routingspacing.between_shapes.y", bad);
        spacing_ok(&mut w);
        assert!(!tab(&w)["spacingDialog"].is_null(), "`{bad}`: the dialog stays");
        w.get_by_label_contains(words);
        assert_eq!(tab(&w)["spacing"]["between_shapes"], json!(["0.375", "0.375"]), "`{bad}`: the tab's draft is as it was");
    }
    // A length with a unit is a length: 1 cm is 0.3937 inch.
    type_into(&mut w, "routingspacing.between_shapes.y", "1 cm");
    spacing_ok(&mut w);
    assert!(tab(&w)["spacingDialog"].is_null());
    assert_eq!(tab(&w)["spacing"]["between_shapes"], json!(["0.375", "0.3937"]));
    ok(&mut w);
    assert!(near(number(&routing(&mut w)["between_shapes"]["y"]), 10.0 / 25.4));
    assert_eq!(routing(&mut w)["between_shapes"]["x"], json!(0.375), "the axis nobody changed was not sent");
}

#[test]
fn a_box_that_was_not_touched_is_not_refused_whatever_it_holds() {
    // A file may hold a spacing of 0 (LineToNodeX 0): the dialog opens, and OK, without touching it.
    let mut w = design_window();
    let page = page_id(&w);
    run_command(&mut w, "sheet.set_cell", json!({"page": page, "cell": "LineToNodeX", "formula": "0"}));
    open_tab(&mut w);
    open_spacing(&mut w);
    spacing_ok(&mut w);
    assert!(tab(&w)["spacingDialog"].is_null(), "an untouched box is not looked at again");
    let revision = w.state().session().revision();
    ok(&mut w);
    assert_eq!(w.state().session().revision(), revision, "and sends nothing");
}

// ---- the preview -----------------------------------------------------------------------------------------------------------------------------

fn preview(w: &Window) -> Value {
    tab(w)["preview"].clone()
}

fn point(v: &Value) -> (f64, f64) {
    (number(&v[0]), number(&v[1]))
}

fn route_of(v: &Value) -> Vec<(f64, f64)> {
    v["route"].as_array().unwrap().iter().map(point).collect()
}

fn boxes_of(v: &Value) -> Vec<(f64, f64, f64, f64)> {
    v["boxes"].as_array().unwrap().iter().map(|b| (number(&b[0]), number(&b[1]), number(&b[2]), number(&b[3]))).collect()
}

/// Whether a leg of the route goes through the inside of the box.
fn enters(route: &[(f64, f64)], (x0, y0, x1, y1): (f64, f64, f64, f64)) -> bool {
    route.windows(2).any(|leg| {
        let (a, b) = (leg[0], leg[1]);
        let (lo_x, hi_x, lo_y, hi_y) = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
        lo_x < x1 - 1e-9 && hi_x > x0 + 1e-9 && lo_y < y1 - 1e-9 && hi_y > y0 + 1e-9
    })
}

fn level_and_upright(route: &[(f64, f64)]) -> bool {
    route.windows(2).all(|leg| (leg[0].0 - leg[1].0).abs() < 1e-9 || (leg[0].1 - leg[1].1).abs() < 1e-9)
}

#[test]
fn the_preview_routes_a_connector_round_the_third_box_through_the_router() {
    let mut w = design_window();
    open_tab(&mut w);
    assert!(w.state().element("pagesetup.routing.preview").is_some(), "the tab shows a preview");
    let p = preview(&w);
    assert_eq!(p["outcome"], "routed", "{p}");
    let (route, boxes) = (route_of(&p), boxes_of(&p));
    assert_eq!(boxes.len(), 3, "two boxes with a third between them");
    assert!(route.len() >= 4 && level_and_upright(&route), "a right angle style: level and upright legs: {route:?}");
    // The third box is the one that lies between the other two along the flow: it is in the way of the straight line and the route goes round it.
    let middle = boxes[1];
    assert!(!enters(&route, middle), "the route goes round the third box: {route:?} against {middle:?}");
    assert!(enters(&[route[0], *route.last().unwrap()], middle), "the straight line from end to end would have gone through it");
    for b in [boxes[0], boxes[2]] {
        assert!(!enters(&route, b), "{route:?} through {b:?}");
    }
    assert_eq!(p["curved"], false);
    assert!(p["computed"].as_u64().unwrap() >= 1);
}

#[test]
fn the_preview_follows_the_style_the_direction_and_the_appearance() {
    let mut w = design_window();
    open_tab(&mut w);
    // Straight: one leg through the third box (it goes round nothing: item 1).
    choose(&mut w, "pagesetup.routing.style", "pagesetup.routing.style.straight");
    let p = preview(&w);
    assert_eq!(p["outcome"], "straight", "{p}");
    let (route, boxes) = (route_of(&p), boxes_of(&p));
    assert_eq!(route.len(), 2, "a straight connector is one leg: {route:?}");
    assert!(enters(&route, boxes[1]), "and it goes through the third box");
    choose(&mut w, "pagesetup.routing.style", "pagesetup.routing.style.centre_to_centre");
    assert_eq!(route_of(&preview(&w)).len(), 2, "centre to centre is one leg too");
    // A flowchart that flows from the top to the bottom puts the first box above the last, and the route leaves downward.
    choose(&mut w, "pagesetup.routing.style", "pagesetup.routing.style.flowchart");
    let down = preview(&w);
    let (route, boxes) = (route_of(&down), boxes_of(&down));
    assert!(boxes[0].1 > boxes[2].3, "the first box is above the last: {boxes:?}");
    assert!(route[1].1 < route[0].1 - 1e-9 && (route[1].0 - route[0].0).abs() < 1e-9, "the first leg goes down: {route:?}");
    assert!(!enters(&route, boxes[1]) && level_and_upright(&route));
    choose(&mut w, "pagesetup.routing.direction", "pagesetup.routing.direction.right_to_left");
    let back = preview(&w);
    let (route, boxes) = (route_of(&back), boxes_of(&back));
    assert!(boxes[0].0 > boxes[2].2, "the first box is at the right of the last: {boxes:?}");
    assert!(route[1].0 < route[0].0 - 1e-9, "the first leg goes left: {route:?}");
    // Curved is drawn through the same route.
    choose(&mut w, "pagesetup.routing.appearance", "pagesetup.routing.appearance.curved");
    assert_eq!(preview(&w)["curved"], true);
    assert_eq!(route_of(&preview(&w)), route, "a curved connector follows the right angle route's corners");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn the_preview_draws_the_jump_the_page_would_make_in_the_style_chosen() {
    let mut w = design_window();
    open_tab(&mut w);
    // The first box's connector crosses a second line once; which of the two hops depends on Add line jumps to.
    assert!(!preview(&w)["crossing"].is_null(), "a second line crosses the connector once");
    for (code, hops) in [(1, Some("level")), (2, Some("upright")), (3, Some("upright")), (4, Some("upright")), (5, Some("level")), (0, None)] {
        choose(&mut w, "pagesetup.routing.jump_to", &format!("pagesetup.routing.jump_to.{code}"));
        let jump = preview(&w)["jump"].clone();
        assert_eq!(jump["on"].as_str(), hops, "code {code}: {jump}");
    }
    // The look of the jump, and its size: the chord is the spacing times the factor of its leg's direction.
    choose(&mut w, "pagesetup.routing.jump_to", "pagesetup.routing.jump_to.1");
    for style in ["arc", "gap", "square", "sides2", "sides3", "sides4", "sides5", "sides6", "sides7"] {
        choose(&mut w, "pagesetup.routing.jump_style", &format!("pagesetup.routing.jump_style.{style}"));
        let jump = preview(&w)["jump"].clone();
        assert_eq!(jump["style"], style);
        let shape = jump["shape"].as_array().unwrap();
        let want = match style {
            "arc" => 13,
            "gap" => 0,
            "square" => 4,
            other => other.trim_start_matches("sides").parse::<usize>().unwrap() + 1,
        };
        assert_eq!(shape.len(), want, "{style}: {jump}");
    }
    type_into(&mut w, "pagesetup.routing.horizontal_size.box", "0");
    assert!(preview(&w)["jump"].is_null(), "a jump of no size is made no more");
    type_into(&mut w, "pagesetup.routing.horizontal_size.box", "1");
    let big = number(&preview(&w)["jump"]["chord"]);
    type_into(&mut w, "pagesetup.routing.horizontal_size.box", "0.5");
    let small = number(&preview(&w)["jump"]["chord"]);
    assert!((big - 2.0 * small).abs() < 1e-9 && small > 0.0, "the chord is in proportion to the size: {small} {big}");
    // A curved connector carries no jump.
    choose(&mut w, "pagesetup.routing.appearance", "pagesetup.routing.appearance.curved");
    assert!(preview(&w)["jump"].is_null());
}

#[test]
fn the_preview_keeps_the_clearance_of_the_spacing_dialog() {
    let mut w = design_window();
    open_tab(&mut w);
    let top = |w: &Window| route_of(&preview(w)).iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let third = |w: &Window| boxes_of(&preview(w))[1];
    let gap_before = top(&w) - third(&w).3;
    assert!((gap_before - 0.125).abs() < 1e-6, "the leg over the third box keeps the page's 1/8 inch: {gap_before}");
    open_spacing(&mut w);
    type_into(&mut w, "routingspacing.connector_to_shape.y", "0.4");
    type_into(&mut w, "routingspacing.connector_to_shape.x", "0.4");
    spacing_ok(&mut w);
    let gap = top(&w) - third(&w).3;
    assert!((gap - 0.4).abs() < 1e-6, "and now 0.4: {gap}");
}

#[test]
fn the_preview_is_drawn_again_only_when_something_it_shows_changes() {
    let mut w = design_window();
    open_tab(&mut w);
    let first = preview(&w)["computed"].as_u64().unwrap();
    for _ in 0..6 {
        w.step();
    }
    assert_eq!(preview(&w)["computed"].as_u64().unwrap(), first, "a window at rest routes nothing");
    choose(&mut w, "pagesetup.routing.separate", "pagesetup.routing.separate.all");
    assert_eq!(preview(&w)["computed"].as_u64().unwrap(), first, "Separate is not in the picture");
    choose(&mut w, "pagesetup.routing.style", "pagesetup.routing.style.straight");
    assert!(preview(&w)["computed"].as_u64().unwrap() > first, "a new style is");
}

// ---- ui.inspect and ui.elements --------------------------------------------------------------------------------------------------------------

#[test]
fn ui_inspect_reports_the_tab_and_ui_elements_names_its_controls() {
    use crate::support::call;
    let (mut w, tx) = controlled_with(|mut app| {
        app.execute("file.new", &json!({})).unwrap();
        app
    });
    w.rest();
    call(&mut w, &tx, "engine.execute", json!({"command": "window.pageSetup", "params": {"tab": TAB}})).unwrap();
    w.rest();
    let ui = call(&mut w, &tx, "ui.inspect", json!({})).unwrap();
    assert_eq!(ui["ui"]["pageSetup"], TAB);
    assert_eq!(ui["ui"]["pageSetupRouting"]["style"], "right_angle", "the tab's values are in the report: {}", ui["ui"]["pageSetupRouting"]);
    assert_eq!(ui["ui"]["pageSetupRouting"]["preview"]["outcome"], "routed");
    let listed = call(&mut w, &tx, "ui.elements", json!({"prefix": "pagesetup.routing."})).unwrap();
    let names: Vec<&str> = listed.as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
    for want in [
        "style",
        "direction",
        "separate",
        "overlap",
        "appearance",
        "jump_to",
        "jump_style",
        "vertical_size",
        "vertical_size.box",
        "horizontal_size",
        "horizontal_size.box",
        "plow",
        "split",
        "spacing",
        "preview",
    ] {
        let name = format!("pagesetup.routing.{want}");
        assert!(names.contains(&name.as_str()), "{name} is not named: {names:?}");
    }
    let direction = listed.as_array().unwrap().iter().find(|e| e["name"] == "pagesetup.routing.direction").unwrap();
    assert_eq!(direction["enabled"], false, "Direction is reported greyed for Right Angle");
    // A window with no dialog reports none.
    call(&mut w, &tx, "engine.execute", json!({"command": "window.pageSetup", "params": {"close": true}})).unwrap();
    w.rest();
    let ui = call(&mut w, &tx, "ui.inspect", json!({})).unwrap();
    assert!(ui["ui"]["pageSetupRouting"].is_null());
}

// ---- hostile input and a page that goes away -------------------------------------------------------------------------------------------------

#[test]
fn a_page_taken_away_under_the_dialog_is_told_when_ok_is_pressed_and_nothing_breaks() {
    let mut w = design_window();
    run_command(&mut w, "page.add", json!({"name": "Second"}));
    run_command(&mut w, "window.pageSetup", json!({"tab": TAB, "page": 2}));
    choose(&mut w, "pagesetup.routing.style", "pagesetup.routing.style.tree");
    w.state_mut().execute("page.delete", &json!({"id": 2})).unwrap();
    w.rest();
    ok(&mut w);
    assert_eq!(w.state().window_state()["pageSetup"], TAB, "the dialog stays, with the refusal in it");
    assert_eq!(w.state().ui_panics(), 0);
    cancel(&mut w);
}

// ---- narrow and wide windows -----------------------------------------------------------------------------------------------------------------

/// The names of the tab's controls (the rows of an open list are named beside them and are not looked at here).
fn tab_controls(w: &Window) -> Vec<(String, Rect, bool)> {
    elements_named(w, "pagesetup.routing.").into_iter().filter(|(name, ..)| name.matches('.').count() <= 2 || name.ends_with(".box")).collect()
}

#[test]
fn the_tab_and_the_spacing_dialog_fit_narrow_and_wide_windows() {
    for size in [[480.0, 360.0], [480.0, 720.0], [900.0, 600.0], [1920.0, 1080.0]] {
        let mut w = crate::support::drawing_window(size);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]));
        run_command(&mut w, "window.pageSetup", json!({"tab": TAB}));
        w.rest();
        let (ok_rect, cancel_rect) = (rect_of(&w, "pagesetup.ok"), rect_of(&w, "pagesetup.cancel"));
        assert!(
            screen.contains_rect(ok_rect) && screen.contains_rect(cancel_rect),
            "the buttons are in a {size:?} window: {ok_rect:?} {cancel_rect:?}"
        );
        let content = rect_of(&w, "pagesetup.content");
        assert!(screen.contains_rect(content), "the content is in a {size:?} window: {content:?}");
        for (name, rect, _) in elements_named(&w, "pagesetup.") {
            assert!(rect.min.x >= -0.5 && rect.max.x <= size[0] + 0.5, "{name} leaves a {size:?} window: {rect:?}");
        }
        // What shows of the tab does not overlap: two controls that are inside the part that shows keep apart.
        let shown: Vec<(String, Rect, bool)> = tab_controls(&w).into_iter().filter(|(_, r, _)| content.contains_rect(*r)).collect();
        assert!(shown.len() >= 3, "{size:?}: some of the tab shows: {:?}", shown.iter().map(|s| &s.0).collect::<Vec<_>>());
        for (i, (a, ra, _)) in shown.iter().enumerate() {
            for (b, rb, _) in &shown[i + 1..] {
                assert!(!ra.intersects(*rb), "{a} {ra:?} overlaps {b} {rb:?} in a {size:?} window");
            }
        }
        // The whole tab can be reached by rolling the content: the preview is the one to look for.
        let preview = rect_of(&w, "pagesetup.routing.preview");
        assert!(preview.width() >= 100.0 && preview.height() >= 60.0, "the preview is big enough to read at {size:?}: {preview:?}");
        assert!(preview.max.x <= content.max.x + 0.5, "the preview is inside the content's width: {preview:?} {content:?}");
        // The Spacing dialog over it.
        press_when_visible(&mut w, "pagesetup.routing.spacing");
        w.rest();
        assert!(!tab(&w)["spacingDialog"].is_null(), "{size:?}: Spacing... opens");
        for name in ["routingspacing.ok", "routingspacing.cancel", "routingspacing.defaults"] {
            let r = rect_of(&w, name);
            assert!(screen.contains_rect(r), "{name} is in a {size:?} window: {r:?}");
        }
        let (ok_rect, cancel_rect) = (rect_of(&w, "routingspacing.ok"), rect_of(&w, "routingspacing.cancel"));
        assert!(!ok_rect.intersects(cancel_rect));
        for (name, rect, _) in elements_named(&w, "routingspacing.") {
            assert!(rect.min.x >= -0.5 && rect.max.x <= size[0] + 0.5, "{name} leaves a {size:?} window: {rect:?}");
        }
        assert_eq!(w.state().ui_panics(), 0);
        run_command(&mut w, "window.pageSetup", json!({"close": true}));
    }
}

/// Rolls the content until the control is inside it, and presses it.
fn press_when_visible(w: &mut Window, name: &str) {
    for _ in 0..40 {
        let (row, area) = (rect_of(w, name), rect_of(w, "pagesetup.content"));
        if row.top() >= area.top() && row.bottom() <= area.bottom() {
            break;
        }
        let pos = area.center();
        for _ in 0..3 {
            move_to(w, pos);
        }
        let dy = if row.top() < area.top() { 1.0 } else { -1.0 };
        w.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, dy),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
            source: egui::MouseWheelSource::Unknown,
        });
        for _ in 0..6 {
            w.step();
        }
    }
    press(w, name);
}

// ---- the connector menu's three style rows ---------------------------------------------------------------------------------------------------

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

/// A window with two rectangles joined by a connector (a Z), and another pair joined by a second connector (one level leg); the first connector is selected.
fn two_connectors() -> (Window, u32, u32) {
    let mut w = edit_window([1280.0, 900.0]);
    let page = page_id(&w);
    let drop = |w: &mut Window, x: f64, y: f64| -> u32 {
        let answer = w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": page, "x": x, "y": y})).unwrap();
        u32::try_from(answer["id"].as_u64().unwrap()).unwrap()
    };
    let (a, b, c, d) = (drop(&mut w, 2.0, 3.0), drop(&mut w, 6.0, 5.0), drop(&mut w, 2.0, 6.5), drop(&mut w, 6.0, 6.5));
    let join = |w: &mut Window, from: u32, to: u32| -> u32 {
        let answer =
            w.state_mut().execute("connector.add", &json!({"from": {"id": from, "point": 3}, "to": {"id": to, "point": 1}, "page": page})).unwrap();
        u32::try_from(answer["id"].as_u64().unwrap()).unwrap()
    };
    let (first, second) = (join(&mut w, a, b), join(&mut w, c, d));
    w.state_mut().select(&[first]);
    w.rest();
    (w, first, second)
}

fn path(w: &mut Window, id: u32) -> Value {
    let page = page_id(w);
    w.state_mut().execute("connector.path", &json!({"id": id, "page": page})).unwrap()
}

#[test]
fn the_connector_menu_has_the_three_style_rows_above_reset_connector_and_each_styles_the_selection() {
    let (mut w, first, second) = two_connectors();
    right_click_at(&mut w, 3.0, 3.0);
    let row = |w: &Window, name: &str| w.state().element(&format!("menu.Connector{name}")).map(|e| (e.rect, e.enabled));
    let names = ["RightAngleConnector", "StraightConnector", "CurvedConnector", "ResetConnector"];
    let rows: Vec<(Rect, bool)> = names.iter().map(|n| row(&w, n).unwrap_or_else(|| panic!("{n} is not in the connector's menu"))).collect();
    for pair in rows.windows(2) {
        assert!(pair[0].0.min.y < pair[1].0.min.y, "the three style rows come in this order, above Reset Connector: {rows:?}");
    }
    assert!(rows.iter().all(|(_, enabled)| *enabled));
    w.get_by_label("Right-Angle Connector");
    w.get_by_label("Straight Connector");
    w.get_by_label("Curved Connector");
    assert_eq!(path(&mut w, first)["style"], "right_angle");
    let steps = undo_steps(&w);
    press(&mut w, "menu.ConnectorStraightConnector");
    w.rest();
    let straight = path(&mut w, first);
    assert_eq!(straight["style"], "straight");
    assert_eq!(straight["points"].as_array().unwrap().len(), 2, "routed again as one leg: {straight}");
    assert_eq!(undo_steps(&w), steps + 1, "one undo step");
    assert_eq!(path(&mut w, second)["style"], "right_angle", "the connector that was not selected is left alone");
    // The same through the menu again: Curved, then Right-Angle.
    right_click_at(&mut w, 4.0, 4.0);
    press(&mut w, "menu.ConnectorCurvedConnector");
    w.rest();
    assert_eq!(path(&mut w, first)["style"], "curved");
    right_click_at(&mut w, 4.0, 4.0);
    press(&mut w, "menu.ConnectorRightAngleConnector");
    w.rest();
    let again = path(&mut w, first);
    assert_eq!(again["style"], "right_angle");
    assert_eq!(again["points"].as_array().unwrap().len(), 4, "back to the Z");
    assert_eq!(path(&mut w, second)["style"], "right_angle");
    // The page's own default for new connectors was not touched: the menu is about the connectors selected.
    assert_eq!(routing(&mut w)["style"], "right_angle");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn the_style_rows_style_every_connector_selected_and_only_those() {
    let (mut w, first, second) = two_connectors();
    w.state_mut().select(&[first, second]);
    w.rest();
    right_click_at(&mut w, 3.0, 3.0);
    let steps = undo_steps(&w);
    press(&mut w, "menu.ConnectorCurvedConnector");
    w.rest();
    assert_eq!(
        (path(&mut w, first)["style"].as_str().map(str::to_owned), path(&mut w, second)["style"].as_str().map(str::to_owned)),
        (Some("curved".into()), Some("curved".into()))
    );
    assert_eq!(undo_steps(&w), steps + 1, "both in one undo step");
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(path(&mut w, first)["style"], "right_angle");
    assert_eq!(path(&mut w, second)["style"], "right_angle");
}
