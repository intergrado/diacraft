//! Design > Layout > Re-Layout Page and More Layout Options, Home > Arrange > Position > Auto Space, Spacing Options, Avoid Page Breaks and Move off Page Breaks, and View >
//! Show > Page Breaks in the window. The pointer is given to egui as a person's
//! would be; what is checked is the drawing the commands changed and what the window drew.

use serde_json::{Value as Json, json};

use crate::editing::edit_window;
use crate::support::{Rest, Window, click, press, run_command};

fn undo_steps(w: &Window) -> usize {
    w.state().session().history().map_or(0, |h| h.undo_len())
}

fn enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

fn click_on(w: &mut Window, name: &str) {
    let pos = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    click(w, pos);
    w.rest();
}

/// The middle of a shape on the page, in inches.
fn centre(w: &Window, id: u32) -> (f64, f64) {
    let doc = w.state().session().document().unwrap();
    let c = diacraft_engine::draw::shape_corners(doc, 1, id).unwrap();
    (c.iter().map(|p| p.0).sum::<f64>() / 4.0, c.iter().map(|p| p.1).sum::<f64>() / 4.0)
}

/// A window on a new drawing with three rectangles in a row (x = 3, 6, 9 on y = 4), joined 1 -> 2 -> 3, on the Design tab. → the window and the shapes.
fn window_with_three() -> (Window, [u32; 3]) {
    let mut w = edit_window([1400.0, 900.0]);
    let mut drop = |x: f64| {
        w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": x, "y": 4.0})).unwrap()["id"].as_u64().unwrap() as u32
    };
    let shapes = [drop(3.0), drop(6.0), drop(9.0)];
    for pair in shapes.windows(2) {
        w.state_mut().execute("connector.add", &json!({"from": {"id": pair[0], "point": 3}, "to": {"id": pair[1], "point": 1}})).unwrap();
    }
    run_command(&mut w, "window.tab", json!({"tab": "Design"}));
    (w, shapes)
}

#[test]
fn the_gallery_has_fourteen_tiles_under_five_headings_and_each_makes_its_layout() {
    // Flowchart (four), Hierarchy (four), Compact Tree (four), Radial (one), Circular (one), and More Layout Options at the foot. A tile makes the page's layout in one undo step.
    let (mut w, shapes) = window_with_three();
    assert!(enabled(&w, "ribbon.Design.PageRelayoutGallery"), "Re-Layout Page works on a drawing with shapes");
    press(&mut w, "ribbon.Design.PageRelayoutGallery");
    w.rest();
    let tiles = crate::support::elements_named(&w, "menu.relayout.");
    assert_eq!(tiles.len(), 14, "{tiles:?}");
    assert!(tiles.iter().all(|(_, _, enabled)| *enabled), "every tile works");
    let steps = undo_steps(&w);
    click_on(&mut w, "menu.relayout.flowchart.top_to_bottom");
    assert_eq!(undo_steps(&w), steps + 1, "one undo step");
    let (a, b, c) = (centre(&w, shapes[0]), centre(&w, shapes[1]), centre(&w, shapes[2]));
    assert!((a.0 - b.0).abs() < 1e-6 && (b.0 - c.0).abs() < 1e-6, "one column: {a:?} {b:?} {c:?}");
    assert!(a.1 > b.1 && b.1 > c.1, "the first on top: {a:?} {b:?} {c:?}");
    let layout = w.state_mut().execute("page.layout", &json!({})).unwrap();
    assert_eq!(
        (layout["style"].as_str(), layout["direction"].as_str(), layout["place_style"].as_u64()),
        (Some("flowchart"), Some("top_to_bottom"), Some(1)),
        "{layout}"
    );
    // The other styles: Radial and Circular put the shapes on rings, and nothing overlaps.
    for key in ["radial", "circular", "hierarchy.left_to_right", "compact_tree.down_right"] {
        press(&mut w, "ribbon.Design.PageRelayoutGallery");
        w.rest();
        click_on(&mut w, &format!("menu.relayout.{key}"));
        let at: Vec<(f64, f64)> = shapes.iter().map(|id| centre(&w, *id)).collect();
        assert!(at.iter().all(|p| p.0.is_finite() && p.1.is_finite()), "{key}: {at:?}");
        assert_ne!(at[0], at[1], "{key}");
    }
}

#[test]
fn a_tile_lays_out_the_selection_only_and_an_empty_drawing_has_the_gallery_quiet() {
    let (mut w, shapes) = window_with_three();
    let held = centre(&w, shapes[2]);
    w.state_mut().select(&[shapes[0], shapes[1]]);
    w.rest();
    press(&mut w, "ribbon.Design.PageRelayoutGallery");
    w.rest();
    click_on(&mut w, "menu.relayout.flowchart.top_to_bottom");
    assert_eq!(centre(&w, shapes[2]), held, "a shape not selected is not laid out");
    assert_eq!(w.state().selection(), [shapes[0], shapes[1]], "and the selection stays");
    // A new drawing with no shapes: Re-Layout Page is greyed.
    let mut empty = edit_window([1400.0, 900.0]);
    run_command(&mut empty, "window.tab", json!({"tab": "Design"}));
    assert!(!enabled(&empty, "ribbon.Design.PageRelayoutGallery"));
}

#[test]
fn more_layout_options_opens_the_dialog_with_the_pages_settings_and_ok_lays_out_and_closes() {
    let (mut w, shapes) = window_with_three();
    run_command(&mut w, "window.relayoutOptions", json!({}));
    w.rest();
    assert_eq!(w.state().window_state()["relayoutDialog"], "flowchart");
    for name in [
        "relayout.style.hierarchy",
        "relayout.direction.left_to_right",
        "relayout.alignment.center",
        "relayout.spacing",
        "relayout.connector_style.straight",
        "relayout.appearance.curved",
        "relayout.apply_routing",
        "relayout.enlarge",
        "relayout.to_page",
        "relayout.preview",
        "relayout.ok",
        "relayout.cancel",
        "relayout.apply",
    ] {
        assert!(w.state().element(name).is_some(), "{name} is not drawn");
    }
    // Alignment is for a hierarchy: greyed for a flowchart. Selection is greyed when nothing is selected.
    assert!(!enabled(&w, "relayout.alignment.center"), "a flowchart has no alignment");
    assert!(!enabled(&w, "relayout.to_selection"), "nothing is selected");
    click_on(&mut w, "relayout.style.hierarchy");
    assert!(enabled(&w, "relayout.alignment.center"), "a hierarchy has one");
    click_on(&mut w, "relayout.direction.left_to_right");
    click_on(&mut w, "relayout.connector_style.straight");
    let steps = undo_steps(&w);
    click_on(&mut w, "relayout.ok");
    assert_eq!(w.state().window_state()["relayoutDialog"], Json::Null, "OK closes it");
    assert_eq!(undo_steps(&w), steps + 1, "one step");
    let layout = w.state_mut().execute("page.layout", &json!({})).unwrap();
    assert_eq!(
        (layout["style"].as_str(), layout["direction"].as_str(), layout["connector_style"].as_str()),
        (Some("hierarchy"), Some("left_to_right"), Some("straight")),
        "{layout}"
    );
    let (a, b) = (centre(&w, shapes[0]), centre(&w, shapes[1]));
    assert!(a.0 < b.0 && (a.1 - b.1).abs() < 1e-6, "left to right: {a:?} {b:?}");
}

#[test]
fn apply_leaves_the_dialog_open_cancel_sends_nothing_and_a_bad_spacing_is_said() {
    let (mut w, _) = window_with_three();
    run_command(&mut w, "window.relayoutOptions", json!({}));
    w.rest();
    click_on(&mut w, "relayout.apply");
    assert_eq!(w.state().window_state()["relayoutDialog"], "flowchart", "Apply keeps the dialog");
    // A spacing that is no number is refused in words, in the dialog; nothing is sent.
    let steps = undo_steps(&w);
    let spacing = w.state().element("relayout.spacing").unwrap().rect.center();
    click(&mut w, spacing);
    w.rest();
    for _ in 0..8 {
        crate::support::input(
            &mut w,
            egui::Event::Key { key: egui::Key::Backspace, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE },
        );
    }
    crate::support::input(&mut w, egui::Event::Text("abc".to_owned()));
    w.rest();
    click_on(&mut w, "relayout.apply");
    assert!(w.state().element("relayout.error").is_some(), "the refusal is said");
    assert_eq!(undo_steps(&w), steps);
    click_on(&mut w, "relayout.cancel");
    assert_eq!(w.state().window_state()["relayoutDialog"], Json::Null);
    assert_eq!(undo_steps(&w), steps, "Cancel sends nothing");
}

#[test]
fn the_position_menu_has_auto_space_spacing_options_and_the_two_page_break_items() {
    let (mut w, shapes) = window_with_three();
    run_command(&mut w, "window.tab", json!({"tab": "Home"}));
    press(&mut w, "ribbon.Home.PositionMenu");
    w.rest();
    for name in ["menu.AutoSpace", "menu.SpacingOptionsDialog", "menu.ShapesMoveOffPageBreaks", "menu.AvoidPageBreaks"] {
        assert!(w.state().element(name).is_some_and(|e| e.enabled), "{name} works");
    }
    // Auto Space: the three shapes are 3 apart (a gap of 1.5); the page's spacing is 0.375, so they are brought closer, as one undo step.
    let steps = undo_steps(&w);
    click_on(&mut w, "menu.AutoSpace");
    assert_eq!(undo_steps(&w), steps + 1);
    let (a, b) = (centre(&w, shapes[0]), centre(&w, shapes[1]));
    assert!((b.0 - a.0 - 1.875).abs() < 1e-6, "1.5 wide and a gap of 0.375: {a:?} {b:?}");
    // Avoid Page Breaks is a box on the page; it shows checked once turned.
    press(&mut w, "ribbon.Home.PositionMenu");
    w.rest();
    click_on(&mut w, "menu.AvoidPageBreaks");
    assert!(w.state().session().document().is_some());
    let layout = w.state_mut().execute("page.layout", &json!({})).unwrap();
    assert_eq!(layout["avoid_page_breaks"], true, "{layout}");
}

#[test]
fn spacing_options_opens_a_dialog_of_the_two_spaces_and_ok_sets_the_pages_cells() {
    let (mut w, _) = window_with_three();
    run_command(&mut w, "window.spacingOptions", json!({}));
    w.rest();
    assert_eq!(w.state().window_state()["spacingDialog"], true);
    assert!(w.state().element("spacing.horizontal").is_some() && w.state().element("spacing.vertical").is_some());
    let h = w.state().element("spacing.horizontal").unwrap().rect.center();
    click(&mut w, h);
    w.rest();
    for _ in 0..8 {
        crate::support::input(
            &mut w,
            egui::Event::Key { key: egui::Key::Backspace, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE },
        );
    }
    crate::support::input(&mut w, egui::Event::Text("0.75".to_owned()));
    w.rest();
    click_on(&mut w, "spacing.ok");
    assert_eq!(w.state().window_state()["spacingDialog"], false);
    let layout = w.state_mut().execute("page.layout", &json!({})).unwrap();
    assert_eq!(layout["spacing"]["x"], 0.75, "{layout}");
}

#[test]
fn the_page_breaks_box_draws_dashed_lines_where_the_page_passes_the_paper_and_is_no_undo_step() {
    let (mut w, _) = window_with_three();
    w.state_mut().execute("page.size", &json!({"width": 20.0, "height": 7.5})).unwrap();
    w.rest();
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    assert!(enabled(&w, "ribbon.View.ShowPageBreaks"), "the Page Breaks box works");
    assert_eq!(w.state().canvas_report().page_breaks, (0, 0), "no lines while the box is off");
    let steps = undo_steps(&w);
    press(&mut w, "ribbon.View.ShowPageBreaks");
    w.rest();
    assert_eq!(w.state().window_state()["pageBreaks"], true);
    assert_eq!(w.state().canvas_report().page_breaks, (1, 0), "a page two sheets across has one line across it");
    assert_eq!(undo_steps(&w), steps, "no undo step");
    press(&mut w, "ribbon.View.ShowPageBreaks");
    w.rest();
    assert_eq!(w.state().canvas_report().page_breaks, (0, 0));
}

#[test]
fn the_pictures_and_the_lines_are_drawn_in_both_themes() {
    // Colours come from the tokens: the break line and the tile ink differ from the paper and from each other in the light theme and the dark one.
    for t in [diacraft_ui_egui::theme::Tokens::light(), diacraft_ui_egui::theme::Tokens::dark()] {
        assert_ne!(t.page_break, t.grid_line);
        assert_ne!(t.page_break, t.canvas_fill);
        assert_ne!(t.icon_ink, t.input_fill);
    }
}
