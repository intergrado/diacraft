//! The page tabs along the bottom of the drawing area, the status bar's page count and zoom, and the Shapes window.

use egui::{Rect, vec2};
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::support::{Rest, click, drag, drawing_window, elements_named, run_command};

fn tab_names(window: &crate::support::Window) -> Vec<String> {
    let mut tabs = elements_named(window, "page-tab.");
    tabs.sort_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x));
    tabs.into_iter().map(|(n, ..)| n).collect()
}

/// With two pages the row holds two tabs, then All, then the add button, left to right.
#[test]
fn the_page_row_holds_a_tab_per_page_then_all_then_add() {
    let mut window = drawing_window([1280.0, 720.0]);
    assert_eq!(tab_names(&window), ["page-tab.1"]);
    window.get_by_label("Add Page").click();
    window.step();
    window.step();
    assert_eq!(tab_names(&window), ["page-tab.1", "page-tab.2"]);
    let (all, add) = (window.state().element("page-tabs.all").unwrap().rect, window.state().element("page-tabs.add").unwrap().rect);
    let last = window.state().element("page-tab.2").unwrap().rect;
    assert!(last.right() <= all.left() && all.right() <= add.left(), "tabs, All, add: {last:?} {all:?} {add:?}");
    // The tabs show the pages' names as the document holds them.
    window.get_by_label("Page-1");
    window.get_by_label("Page-2");
}

/// A click on a tab shows that page; the status bar counts; undoing the page's addition falls back to the first page.
#[test]
fn clicking_a_tab_switches_the_page_and_the_status_bar_counts() {
    let mut window = drawing_window([1280.0, 720.0]);
    run_command(&mut window, "page.add", json!({"name": "Overview"}));
    run_command(&mut window, "page.add", json!({}));
    window.get_by_label("Page 1 of 3");
    assert_eq!(window.state().window_state()["page"], 1);
    window.get_by_label("Overview").click();
    window.step();
    window.step();
    assert_eq!(window.state().window_state()["page"], 2);
    window.get_by_label("Page 2 of 3");
    // A page that was switched to is drawn, with its own view.
    assert!(window.state().canvas_report().page.is_some());
    run_command(&mut window, "page.select", json!({"id": 3}));
    window.get_by_label("Page 3 of 3");
    // The page is the document's: undo takes the added page away and the window falls back to a page that is there.
    run_command(&mut window, "edit.undo", json!({}));
    window.get_by_label("Page 1 of 2");
    assert_eq!(window.state().window_state()["page"], 1);
    assert!(window.state_mut().execute("page.select", &json!({"id": 99})).is_err(), "no such page");
}

/// The All menu lists every page and takes you to the one chosen.
#[test]
fn the_all_menu_lists_the_pages_and_goes_to_one() {
    let mut window = drawing_window([1280.0, 720.0]);
    run_command(&mut window, "page.add", json!({"name": "Second"}));
    window.get_by_label("All").click();
    window.step();
    window.step();
    // The menu lists both pages (the tab of the second also has its name, so the row is found by its place).
    assert!(window.state().element("all-pages.1").is_some() && window.state().element("all-pages.2").is_some());
    let row = window.state().element("all-pages.2").unwrap().rect;
    click(&mut window, row.center());
    window.step();
    window.step();
    assert_eq!(window.state().window_state()["page"], 2);
}

/// A page with a very long name has a tab of a bounded width, and the row holds.
#[test]
fn a_hostile_page_name_is_shown_safely() {
    let mut window = drawing_window([800.0, 500.0]);
    run_command(&mut window, "page.add", json!({"name": "w".repeat(250)}));
    window.rest();
    let tab = window.state().element("page-tab.2").unwrap().rect;
    assert!(tab.width() <= 181.0, "{tab:?}");
    assert_eq!(window.state().ui_panics(), 0);
    let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 500.0));
    assert!(screen.contains_rect(window.state().element("page-tabs.add").unwrap().rect));
}

/// The zoom slider, the two zoom buttons and the Fit button work, and the percentage follows.
#[test]
fn the_status_bar_zoom_controls_work() {
    let mut window = drawing_window([1280.0, 720.0]);
    run_command(&mut window, "view.zoom", json!({"percent": 100}));
    window.get_by_label("100%");
    window.get_by_label("Zoom In").click();
    window.step();
    window.step();
    window.get_by_label("125%");
    window.get_by_label("Zoom Out").click();
    window.step();
    window.step();
    window.get_by_label("100%");
    // Dragging the slider to the end of its track gives the largest zoom, and to its start the smallest.
    let slider = window.state().element("status.zoom_slider").unwrap().rect;
    drag(&mut window, slider.center(), vec2(slider.width(), 0.0));
    window.rest();
    assert_eq!(window.state().window_state()["zoomPercent"], 400.0);
    drag(&mut window, slider.center() + vec2(slider.width() / 2.0 - 8.0, 0.0), vec2(-slider.width(), 0.0));
    window.rest();
    assert_eq!(window.state().window_state()["zoomPercent"], 10.0);
    // Fit brings the page back inside the drawing area.
    window.get_by_label("Fit Page to Window").click();
    window.step();
    window.step();
    let area = window.state().element("canvas").unwrap().rect;
    assert!(area.contains_rect(window.state().canvas_report().page.unwrap()));
    // No selection, so no Width, Height or Angle.
    for label in ["Width", "Height", "Angle"] {
        assert!(window.query_by_label_contains(label).is_none(), "{label} shows with nothing selected");
    }
}

/// Window controls that do nothing yet are drawn disabled in the status bar.
#[test]
fn the_status_bar_buttons_that_do_nothing_are_disabled() {
    let window = drawing_window([1280.0, 720.0]);
    for name in ["status.presentation", "status.view1", "status.view2", "status.language", "status.macro"] {
        let e = window.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn"));
        assert!(!e.enabled, "{name} does nothing yet");
    }
    for name in ["status.zoom_in", "status.zoom_out", "status.fit", "status.zoom_slider"] {
        assert!(window.state().element(name).unwrap().enabled, "{name} works");
    }
}

/// The Shapes window keeps its chooser and filter above the actual-master gallery.
/// Folding to a strip and unfolding leave more room for the canvas.
#[test]
fn the_shapes_window_has_a_row_for_each_stencil_and_the_tiles_of_the_current_one_and_folds() {
    let mut window = drawing_window([1280.0, 720.0]);
    let names: Vec<String> = elements_named(&window, "shapes.").into_iter().map(|(n, ..)| n).collect();
    for expected in ["shapes.collapse", "shapes.search", "shapes.more", "shapes.tile.Rectangle", "shapes.tile.Hexagon"] {
        assert!(names.iter().any(|n| n == expected), "{expected} is not drawn: {names:?}");
    }
    assert!(elements_named(&window, "shapes.tile.").iter().all(|(_, _, enabled)| *enabled));
    // The compact chooser and current-stencil filter leave the gallery below them.
    let y = |n: &str| window.state().element(n).unwrap().rect.min.y;
    assert!(y("shapes.more") < y("shapes.search") && y("shapes.search") < y("shapes.tile.Rectangle"));
    assert!(window.state().element("shapes.quick").is_none());
    assert!(elements_named(&window, "shapes.stencil.").is_empty(), "the catalogue is in the popup");
    let canvas_before = window.state().element("canvas").unwrap().rect;
    window.get_by_label("Minimize the Shapes window").click();
    window.step();
    window.step();
    assert!(window.state().element("shapes.tile.Rectangle").is_none(), "the window is folded to a strip");
    assert!(window.state().element("canvas").unwrap().rect.width() > canvas_before.width() + 100.0, "the drawing area takes the room");
    window.get_by_label("Expand the Shapes window").click();
    window.step();
    window.step();
    assert!(window.state().element("shapes.tile.Rectangle").is_some());
    // The View tab's Task Panes menu has the same switch.
    run_command(&mut window, "window.shapes", json!({"shown": false}));
    assert!(window.state().element("shapes.tile.Rectangle").is_none());
    run_command(&mut window, "window.shapes", json!({}));
    assert!(window.state().element("shapes.tile.Rectangle").is_some());
}

/// The File tab opens the File view over the window; its rail holds Home, New, Open, Account and Options, and Back leaves it.
#[test]
fn the_file_view_has_a_rail_a_blank_drawing_tile_that_works_and_a_back_button() {
    let mut window = drawing_window([1280.0, 720.0]);
    window.get_by_label("File").click();
    window.step();
    window.step();
    assert_eq!(window.state().window_state()["backstage"], "Home");
    assert!(
        window.state().element("canvas").is_none() && window.state().element("ribbon.collapse").is_none(),
        "the File view replaces the window's body"
    );
    // Home, New, Open at the top (then Save and Save As, which a file view has) and Account, Options at the bottom. Open and Save As
    // need a dialog the test window does not have; Save works (it asks for a file, once there is a dialog); Account does nothing, and Options opens
    // the Options dialog (where the Developer tab is turned on).
    let y = |n: &str| window.state().element(n).unwrap().rect.min.y;
    assert!(y("backstage.back") < y("backstage.Home") && y("backstage.Home") < y("backstage.New") && y("backstage.New") < y("backstage.Open"));
    assert!(y("backstage.Open") < y("backstage.Save") && y("backstage.Save") < y("backstage.Save As"));
    assert!(y("backstage.Save As") < y("backstage.Account") && y("backstage.Account") < y("backstage.Options"));
    for (name, enabled) in [("Home", true), ("New", true), ("Open", false), ("Save", true), ("Save As", false), ("Account", false), ("Options", true)]
    {
        assert_eq!(window.state().element(&format!("backstage.{name}")).unwrap().enabled, enabled, "{name}");
    }
    window.get_by_label("New").click();
    window.step();
    window.step();
    assert_eq!(window.state().window_state()["backstage"], "New");
    // The Blank drawing tile makes a new drawing, named for the window's second.
    // A drawing with a change that is not saved is asked about first.
    run_command(&mut window, "page.add", json!({}));
    window.get_by_label("Blank drawing").click();
    window.step();
    window.step();
    assert!(window.state().prompt_open(), "the drawing has an edit that is not saved: the window asks");
    assert_eq!(window.state().window_state()["backstage"], "New", "and nothing has happened yet");
    window.get_by_label("Don't Save").click();
    window.step();
    window.step();
    assert_eq!(window.state().window_state()["backstage"], serde_json::Value::Null);
    window.get_by_label("Drawing2 - DiaCraft");
    window.get_by_label("Page 1 of 1");
    // Back leaves without doing anything; so does Escape.
    run_command(&mut window, "window.backstage", json!({"page": "Home"}));
    window.get_by_label("Back").click();
    window.step();
    window.step();
    assert!(window.state().window_state()["backstage"].is_null());
    run_command(&mut window, "window.backstage", json!({"page": "New"}));
    window.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    window.step();
    window.step();
    assert!(window.state().window_state()["backstage"].is_null());
}

/// The Shapes window takes a third of the window at most, so a small window keeps room for the drawing: a width set in a large
/// window does not carry over.
#[test]
fn the_shapes_window_gives_way_in_a_small_window() {
    let mut window = drawing_window([1280.0, 720.0]);
    let wide = window.state().element("shapes.search").unwrap().rect.width();
    window.set_size(vec2(480.0, 600.0));
    window.rest();
    let narrow = window.state().element("shapes.search").unwrap().rect.width();
    assert!(narrow < wide, "{narrow} < {wide}");
    let canvas = window.state().element("canvas").unwrap().rect;
    assert!(canvas.width() > 480.0 * 0.5, "the drawing area keeps over half of a 480 point window: {canvas:?}");
}

/// Home keeps its full cover and usable new-drawing action inside narrow and short windows.
#[test]
fn home_cover_preserves_aspect_and_the_blank_action_on_small_windows() {
    for [width, height] in [[1280.0, 800.0], [480.0, 480.0], [320.0, 480.0]] {
        let mut window = drawing_window([width, height]);
        run_command(&mut window, "window.backstage", json!({"page": "Home"}));
        let cover = window.state().element("backstage.cover").unwrap().rect;
        let tile = window.state().element("backstage.tile.blank").unwrap().rect;
        assert!((cover.width() / cover.height() - 1672.0 / 941.0).abs() < 0.001);
        assert!(cover.right() <= width && tile.right() <= width);
        assert!(cover.bottom() < tile.top() && tile.bottom() <= height, "Home action outside {width} by {height}: {tile:?}");
        window.get_by_label("Blank drawing").click();
        window.step();
        window.step();
        assert!(window.state().window_state()["backstage"].is_null());
        window.get_by_label("Drawing2 - DiaCraft");
    }
}
