//! Page setup in the window: the Design tab's Page Setup group, the Page Setup dialog, the page tabs' add, menu, drag and rename, the Reorder
//! Pages dialog, and Auto Size growing the page under a view that follows the drawing. The pointer and the keyboard are given to egui as a person's
//! would be, and what is checked is the document they changed through commands.

use diacraft_engine::draw::paper::SIZES;
use egui::accesskit::Toggled;
use egui::{Event, Key, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, Rect, TouchPhase, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::{Value, json};

use crate::editing::{at, click_at, drag_tile_to, edit_window, key, shape_ids, type_text};
use crate::support::{Rest, Window, click, drag, elements_named, input, move_to, press, run_command};

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

fn undo(w: &mut Window) {
    run_command(w, "edit.undo", json!({}));
}

/// `page.setup` of the first page (a query: it changes and journals nothing).
fn setup(w: &mut Window) -> Value {
    w.state_mut().execute("page.setup", &json!({})).unwrap()
}

fn setup_of(w: &mut Window, page: u32) -> Value {
    w.state_mut().execute("page.setup", &json!({"page": page})).unwrap()
}

/// A new drawing at 100% on the Design tab.
fn design_window() -> Window {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "window.tab", json!({"tab": "Design"}));
    w
}

fn toggled(w: &Window, label: &str) -> Option<Toggled> {
    w.get_by_label(label).accesskit_node().toggled()
}

/// A click with the secondary button.
fn right_click(w: &mut Window, pos: Pos2) {
    for _ in 0..3 {
        move_to(w, pos);
    }
    for pressed in [true, false] {
        input(w, Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
    }
    w.step();
    w.step();
}

fn rect_of(w: &Window, name: &str) -> Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

/// A right click in the middle of the control named `name`.
fn right_click_on(w: &mut Window, name: &str) {
    let at = rect_of(w, name).center();
    right_click(w, at);
}

/// A click in the middle of the control named `name`.
fn click_on(w: &mut Window, name: &str) {
    let at = rect_of(w, name).center();
    click(w, at);
}

fn is_enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

/// Rolls the list `list` until the row `name` is inside it, where a click lands on it.
fn bring_into_list(w: &mut Window, name: &str, list: &str) {
    for _ in 0..40 {
        let (row, area) = (rect_of(w, name), rect_of(w, list));
        if row.top() >= area.top() && row.bottom() <= area.bottom() {
            return;
        }
        let pos = area.center();
        for _ in 0..3 {
            move_to(w, pos);
        }
        let dy = if row.top() < area.top() { 1.0 } else { -1.0 };
        w.input_mut().events.push(Event::MouseWheel {
            unit: MouseWheelUnit::Line,
            delta: vec2(0.0, dy),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
            source: MouseWheelSource::Unknown,
        });
        for _ in 0..6 {
            w.step();
        }
    }
    panic!("{name} did not come into view");
}

fn open_dialog(w: &mut Window, tab: &str) {
    run_command(w, "window.pageSetup", json!({"tab": tab}));
    assert_eq!(w.state().window_state()["pageSetup"], tab);
}

fn ok(w: &mut Window) {
    press(w, "pagesetup.ok");
    w.rest();
}

// ---- the Design tab's Page Setup group -------------------------------------------------------------------------------------------------------

/// Orientation turns the page and the printer paper; the way the page is held is marked in its menu; the whole page is put in view; one undo step.
#[test]
fn orientation_turns_the_page_and_marks_the_way_it_is_held() {
    let mut w = design_window();
    let first = setup(&mut w);
    assert_eq!(first["orientation"], "landscape", "a new drawing is Letter on its side");
    press(&mut w, "ribbon.Design.PageOrientation");
    assert_eq!(toggled(&w, "Landscape"), Some(Toggled::True), "the page is held on its side: that row is marked");
    assert_ne!(toggled(&w, "Portrait"), Some(Toggled::True));
    let revision = w.state().session().revision();
    click_on(&mut w, "menu.PageOrientationPortrait");
    w.rest();
    assert_eq!(w.state().session().revision(), revision + 1, "one command");
    let turned = setup(&mut w);
    assert_eq!(turned["orientation"], "portrait");
    assert!(near(number(&turned["width"]), 8.5) && near(number(&turned["height"]), 11.0), "{turned}");
    assert_eq!(turned["paper"]["orientation"], "portrait", "and the paper with it");
    let area = rect_of(&w, "canvas");
    assert!(area.contains_rect(w.state().canvas_report().page.unwrap()), "the whole page is in view");
    press(&mut w, "ribbon.Design.PageOrientation");
    assert_eq!(toggled(&w, "Portrait"), Some(Toggled::True), "and the mark follows");
    w.input_mut().events.push(Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    w.step();
    w.step();
    undo(&mut w);
    assert_eq!(setup(&mut w)["orientation"], "landscape");
}

/// Size lists the engine's named sizes, in their groups, marks the one the page has, and sets one: the page keeps its way round and the paper follows.
#[test]
fn size_lists_the_named_sizes_and_sets_one() {
    let mut w = design_window();
    press(&mut w, "ribbon.Design.PageSize");
    for preset in SIZES {
        assert!(w.state().element(&format!("size.item.{}", preset.id)).is_some(), "{} is listed", preset.id);
    }
    assert!(!is_enabled(&w, "menu.PageFitToDrawing"), "Fit to Drawing has no command yet");
    assert!(is_enabled(&w, "menu.MorePageSizes"));
    let revision = w.state().session().revision();
    click_on(&mut w, "size.item.legal");
    w.rest();
    assert_eq!(w.state().session().revision(), revision + 1, "one command");
    let legal = setup(&mut w);
    assert_eq!(legal["size"]["id"], "legal");
    assert_eq!(legal["orientation"], "landscape", "the page keeps its way round");
    assert!(near(number(&legal["width"]), 14.0) && near(number(&legal["height"]), 8.5), "{legal}");
    assert_eq!(legal["paper"]["kind"], 5, "and the printer paper is Legal too");
    press(&mut w, "ribbon.Design.PageSize");
    assert_eq!(toggled(&w, "Legal"), Some(Toggled::True), "the size the page has is marked");
    assert_ne!(toggled(&w, "Letter"), Some(Toggled::True));
    // More Page Sizes is the dialog's Page Size tab.
    click_on(&mut w, "menu.MorePageSizes");
    w.rest();
    assert_eq!(w.state().window_state()["pageSetup"], "Page Size");
    assert!(w.state().is_modal_open(), "the dialog is a modal: nothing under it takes the keys or the pointer");
}

/// Auto Size is a toggle, pressed while the page grows by itself.
#[test]
fn auto_size_is_a_toggle_that_shows_whether_it_is_on() {
    let mut w = design_window();
    assert_ne!(toggled(&w, "Auto Size"), Some(Toggled::True), "a new page does not grow by itself");
    assert_eq!(setup(&mut w)["auto_size"], false);
    press(&mut w, "ribbon.Design.AutoSizePage");
    assert_eq!(setup(&mut w)["auto_size"], true);
    assert_eq!(toggled(&w, "Auto Size"), Some(Toggled::True));
    press(&mut w, "ribbon.Design.AutoSizePage");
    assert_eq!(setup(&mut w)["auto_size"], false);
    assert_ne!(toggled(&w, "Auto Size"), Some(Toggled::True));
    undo(&mut w);
    assert_eq!(setup(&mut w)["auto_size"], true, "each press is one undo step");
}

/// With Auto Size on, a drop past the right edge grows the page, and a drop past the left edge grows it on the left and carries the drawing: the
/// window reads the drawing again, and the view follows it, so that the shape dropped first stays where it was on the screen.
#[test]
fn a_drop_past_the_edge_grows_the_page_and_the_view_follows_a_drawing_that_was_carried() {
    let mut w = design_window();
    run_command(&mut w, "view.zoom", json!({"percent": 60}));
    press(&mut w, "ribbon.Design.AutoSizePage");
    drag_tile_to(&mut w, "Rectangle", 10.9, 4.0);
    let grown = setup(&mut w);
    assert!(near(number(&grown["width"]), 22.0), "the page took another sheet on the right: {}", grown["width"]);
    assert_eq!(w.state().page_view().map(|s| s.width), Some(22.0), "the window read it");
    let first = shape_ids(&w)[0];
    let doc = w.state().session().document().unwrap();
    let frame = diacraft_engine::draw::shape_frame(doc, doc.pages.get(0).unwrap().id, first).unwrap();
    let (cx, cy) = (frame.x + frame.width / 2.0, frame.y + frame.height / 2.0);
    let before = at(&w, cx, cy);
    drag_tile_to(&mut w, "Ellipse", 0.1, 4.0);
    let grown = setup(&mut w);
    assert!(near(number(&grown["width"]), 33.0), "and another on the left: {}", grown["width"]);
    let doc = w.state().session().document().unwrap();
    let frame = diacraft_engine::draw::shape_frame(doc, doc.pages.get(0).unwrap().id, first).unwrap();
    assert!(near(frame.x + frame.width / 2.0, cx + 11.0), "the engine carried the first shape: {} against {cx}", frame.x + frame.width / 2.0);
    let after = at(&w, frame.x + frame.width / 2.0, cy);
    assert!((after - before).length() < 1.5, "and the view followed: {before:?} then {after:?}");
    assert_eq!(w.state().ui_panics(), 0);
    undo(&mut w);
    assert!(near(number(&setup(&mut w)["width"]), 22.0), "the left growth was part of the drop's one undo step");
}

// ---- the Page Setup dialog -------------------------------------------------------------------------------------------------------------------

/// The launcher opens the dialog with the six tabs in the order of the spec; the last two have no commands and are drawn quiet.
#[test]
fn the_dialog_has_the_six_tabs_of_the_spec_and_the_last_two_are_quiet() {
    let mut w = design_window();
    press(&mut w, "ribbon.Design.PageSetupDialog");
    assert_eq!(w.state().window_state()["pageSetup"], "Page Size", "the launcher opens Page Size");
    let mut tabs = elements_named(&w, "pagesetup.tab.");
    tabs.sort_by(|a, b| (a.1.min.y, a.1.min.x).partial_cmp(&(b.1.min.y, b.1.min.x)).unwrap());
    let names: Vec<&str> = tabs.iter().map(|(n, ..)| n.strip_prefix("pagesetup.tab.").unwrap()).collect();
    assert_eq!(names, ["Print Setup", "Page Size", "Drawing Scale", "Page Properties", "Layout and Routing", "Alt Text"]);
    let enabled: Vec<bool> = tabs.iter().map(|(.., e)| *e).collect();
    assert_eq!(enabled, [true, true, true, true, false, false]);
    assert!(w.state_mut().execute("window.pageSetup", &json!({"tab": "Alt Text"})).is_err(), "a tab with nothing behind it cannot be opened");
    assert!(w.state_mut().execute("window.pageSetup", &json!({"tab": "Nonsense"})).is_err());
    // The launcher of the Backgrounds group opens Page Properties.
    run_command(&mut w, "window.pageSetup", json!({"close": true}));
    assert!(w.state().window_state()["pageSetup"].is_null());
    press(&mut w, "ribbon.Design.PageSetupPageDialog");
    assert_eq!(w.state().window_state()["pageSetup"], "Page Properties");
}

/// A size and an orientation chosen in the dialog go on OK as one undo step; nothing goes before.
#[test]
fn ok_sends_the_size_and_the_orientation_as_one_undo_step_and_nothing_goes_before() {
    let mut w = design_window();
    let before = setup(&mut w);
    open_dialog(&mut w, "Page Size");
    let revision = w.state().session().revision();
    bring_into_list(&mut w, "pagesetup.size.a4", "pagesetup.size.list");
    click_on(&mut w, "pagesetup.size.a4");
    w.rest();
    press(&mut w, "pagesetup.orientation.portrait");
    assert_eq!(w.state().session().revision(), revision, "the dialog has sent nothing yet");
    ok(&mut w);
    assert!(w.state().window_state()["pageSetup"].is_null(), "OK closes the dialog");
    let now = setup(&mut w);
    assert_eq!(now["size"]["id"], "a4");
    assert_eq!(now["orientation"], "portrait");
    assert!(near(number(&now["width"]), 210.0 / 25.4) && near(number(&now["height"]), 297.0 / 25.4), "{now}");
    assert_eq!(now["paper"]["kind"], 9, "A4 paper follows the size");
    undo(&mut w);
    assert_eq!(setup(&mut w), before, "one undo step puts the page back exactly");
}

/// Cancel and Escape close the dialog and send nothing.
#[test]
fn cancel_and_escape_send_nothing() {
    let mut w = design_window();
    let revision = w.state().session().revision();
    open_dialog(&mut w, "Page Size");
    press(&mut w, "pagesetup.size.legal");
    press(&mut w, "pagesetup.cancel");
    w.rest();
    assert!(w.state().window_state()["pageSetup"].is_null());
    open_dialog(&mut w, "Page Size");
    press(&mut w, "pagesetup.size.legal");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(w.state().window_state()["pageSetup"].is_null(), "Escape is Cancel");
    assert_eq!(w.state().session().revision(), revision);
    assert!(!w.state().is_dirty());
}

/// A custom size is typed in inches; a text that is not a number is told in words and the dialog stays open.
#[test]
fn a_custom_size_is_typed_and_a_bad_number_is_refused_in_words() {
    let mut w = design_window();
    open_dialog(&mut w, "Page Size");
    press(&mut w, "pagesetup.size.custom");
    let width = rect_of(&w, "pagesetup.size.width").center();
    click(&mut w, width);
    key(&mut w, Key::A, crate::saving::ctrl());
    type_text(&mut w, "abc");
    let revision = w.state().session().revision();
    ok(&mut w);
    assert_eq!(w.state().window_state()["pageSetup"], "Page Size", "the dialog stays");
    assert_eq!(w.state().session().revision(), revision, "and nothing was sent");
    w.get_by_label_contains("numbers of inches");
    // (The words moved the dialog: the fields are found again.)
    let width = rect_of(&w, "pagesetup.size.width").center();
    click(&mut w, width);
    key(&mut w, Key::A, crate::saving::ctrl());
    type_text(&mut w, "5");
    let height = rect_of(&w, "pagesetup.size.height").center();
    click(&mut w, height);
    key(&mut w, Key::A, crate::saving::ctrl());
    type_text(&mut w, "7");
    ok(&mut w);
    let now = setup(&mut w);
    assert!(near(number(&now["width"]), 5.0) && near(number(&now["height"]), 7.0), "{now}");
    assert!(now["size"].is_null(), "a custom size is no named one");
    assert_eq!(now["orientation"], "portrait");
}

/// Turning the orientation in the dialog swaps the typed sizes, and a custom size with the orientation sends both.
#[test]
fn the_orientation_swaps_the_typed_size() {
    let mut w = design_window();
    open_dialog(&mut w, "Page Size");
    press(&mut w, "pagesetup.orientation.portrait");
    press(&mut w, "pagesetup.size.custom");
    ok(&mut w);
    let now = setup(&mut w);
    assert_eq!(now["orientation"], "portrait");
    assert!(near(number(&now["width"]), 8.5) && near(number(&now["height"]), 11.0), "the typed 11 x 8.5 swapped: {now}");
}

/// The Drawing Scale tab: a listed scale, and a custom one with units, both keep the page's size on paper.
#[test]
fn the_drawing_scale_tab_sets_a_listed_scale_and_a_custom_one() {
    let mut w = design_window();
    open_dialog(&mut w, "Drawing Scale");
    bring_into_list(&mut w, "pagesetup.scale.arch_1_4", "pagesetup.scale.list");
    click_on(&mut w, "pagesetup.scale.arch_1_4");
    w.rest();
    ok(&mut w);
    let now = setup(&mut w);
    assert_eq!(now["scale"]["preset"], "arch_1_4");
    assert!(near(number(&now["width"]), 11.0) && near(number(&now["height"]), 8.5), "the page keeps its size on paper: {now}");
    assert!(near(number(&now["drawing"]["width"]), 528.0), "and holds 528 drawing units across: {now}");
    // A custom scale: 1 inch on paper for 10 feet.
    open_dialog(&mut w, "Drawing Scale");
    press(&mut w, "pagesetup.scale.custom");
    for (name, text) in [("pagesetup.scale.paper", "1"), ("pagesetup.scale.drawing", "10")] {
        let field = rect_of(&w, name).center();
        click(&mut w, field);
        key(&mut w, Key::A, crate::saving::ctrl());
        type_text(&mut w, text);
    }
    // The paper's unit is inches already; the drawing's is feet.
    click_on(&mut w, "pagesetup.scale.drawing_unit");
    w.rest();
    w.get_by_label("ft").click();
    w.step();
    w.step();
    ok(&mut w);
    let now = setup(&mut w);
    assert_eq!(now["scale"]["preset"], "civil_10", "1 in = 10 ft is the listed scale of that ratio: {now}");
    assert_eq!(now["scale"]["drawing_unit"].as_str().map(str::to_lowercase), Some("ft".to_owned()));
}

/// The Print Setup tab: the printer paper and the way it is held go with `page.paper`; the page is not touched; what the engine cannot change yet
/// is drawn quiet.
#[test]
fn the_print_setup_tab_sets_the_paper_and_leaves_the_rest_quiet() {
    let mut w = design_window();
    open_dialog(&mut w, "Print Setup");
    for name in ["zoom", "fit", "margins", "centre_x", "centre_y", "grid"] {
        assert!(!is_enabled(&w, &format!("pagesetup.print.{name}")), "{name} cannot be changed yet");
    }
    let before = setup(&mut w);
    assert_eq!(before["paper"]["assumed"], true, "a new page names no paper");
    bring_into_list(&mut w, "pagesetup.paper.9", "pagesetup.paper.list");
    click_on(&mut w, "pagesetup.paper.9");
    w.rest();
    press(&mut w, "pagesetup.paper_orientation.portrait");
    ok(&mut w);
    let now = setup(&mut w);
    assert_eq!(now["paper"]["kind"], 9);
    assert_eq!(now["paper"]["orientation"], "portrait");
    assert_eq!(now["width"], before["width"], "the page's own size is not touched");
    assert_eq!(now["height"], before["height"]);
    assert_eq!(w.state().ui_panics(), 0);
}

/// Page Properties: the page's name.
#[test]
fn the_page_properties_tab_renames_the_page() {
    let mut w = design_window();
    open_dialog(&mut w, "Page Properties");
    let name = rect_of(&w, "pagesetup.name").center();
    click(&mut w, name);
    key(&mut w, Key::A, crate::saving::ctrl());
    type_text(&mut w, "Overview");
    ok(&mut w);
    assert_eq!(setup(&mut w)["name"], "Overview");
    w.get_by_label("Overview");
    // An empty name is refused in words.
    open_dialog(&mut w, "Page Properties");
    let name = rect_of(&w, "pagesetup.name").center();
    click(&mut w, name);
    key(&mut w, Key::A, crate::saving::ctrl());
    key(&mut w, Key::Backspace, Modifiers::NONE);
    ok(&mut w);
    assert_eq!(w.state().window_state()["pageSetup"], "Page Properties", "the dialog stays");
    assert_eq!(setup(&mut w)["name"], "Overview");
}

/// A dialog that is open takes the keys and the pointer: Delete does not delete the shape under it.
#[test]
fn an_open_dialog_is_a_modal() {
    let mut w = design_window();
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    assert_eq!(shape_ids(&w).len(), 1);
    open_dialog(&mut w, "Page Size");
    key(&mut w, Key::Delete, Modifiers::NONE);
    assert_eq!(shape_ids(&w).len(), 1, "Delete reaches nothing under the dialog");
    click_at(&mut w, 8.0, 2.0);
    assert_eq!(w.state().window_state()["pageSetup"], "Page Size", "a click on the page does not close it");
}

/// The dialog fits narrow and wide windows: every control is inside the window, and nothing overlaps its buttons.
#[test]
fn the_dialog_fits_a_narrow_window_and_a_wide_one() {
    for size in [[480.0, 360.0], [480.0, 720.0], [900.0, 600.0], [1920.0, 1080.0]] {
        let mut w = crate::support::drawing_window(size);
        for tab in ["Print Setup", "Page Size", "Drawing Scale", "Page Properties"] {
            run_command(&mut w, "window.pageSetup", json!({"tab": tab}));
            w.rest();
            let screen = Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]));
            let ok = rect_of(&w, "pagesetup.ok");
            let cancel = rect_of(&w, "pagesetup.cancel");
            assert!(screen.contains_rect(ok) && screen.contains_rect(cancel), "the buttons are in a {size:?} window on {tab}: {ok:?} {cancel:?}");
            assert!(!ok.intersects(cancel));
            for (name, rect, _) in elements_named(&w, "pagesetup.tab.") {
                assert!(screen.contains_rect(rect), "{name} leaves a {size:?} window: {rect:?}");
            }
            // The part of the tab that shows is inside the window, and every control of it is inside the window across. What is below the part
            // that shows has been rolled out of sight: the part scrolls.
            let content = rect_of(&w, "pagesetup.content");
            assert!(screen.contains_rect(content), "the content of {tab} is in a {size:?} window: {content:?}");
            for (name, rect, _) in elements_named(&w, "pagesetup.") {
                assert!(rect.min.x >= -0.5 && rect.max.x <= size[0] + 0.5, "{name} leaves a {size:?} window on {tab}: {rect:?}");
            }
            assert_eq!(w.state().ui_panics(), 0);
        }
        run_command(&mut w, "window.pageSetup", json!({"close": true}));
    }
}

// ---- the page tabs ----------------------------------------------------------------------------------------------------------------------------

/// The plus control adds a page that has the setup of the page on view.
#[test]
fn the_plus_adds_a_page_that_copies_the_setup_of_the_page_on_view() {
    let mut w = design_window();
    run_command(&mut w, "page.size", json!({"preset": "a4"}));
    run_command(&mut w, "page.orientation", json!({"orientation": "portrait"}));
    press(&mut w, "page-tabs.add");
    w.rest();
    let two = setup_of(&mut w, 2);
    assert_eq!(two["size"]["id"], "a4");
    assert_eq!(two["orientation"], "portrait");
    assert_eq!(two["paper"]["kind"], 9);
    assert_eq!(w.state().session().document().unwrap().pages.len(), 2);
}

/// A right click on a tab opens the menu of the spec in its order; Delete and Reorder Pages need two pages, and Duplicate works (`page.duplicate`).
#[test]
fn a_tab_menu_has_the_rows_of_the_spec_and_only_what_works_is_enabled() {
    let mut w = design_window();
    right_click_on(&mut w, "page-tab.1");
    let mut rows = elements_named(&w, "page-menu.");
    rows.sort_by(|a, b| a.1.min.y.total_cmp(&b.1.min.y));
    let names: Vec<&str> = rows.iter().map(|(n, ..)| n.strip_prefix("page-menu.").unwrap()).collect();
    assert_eq!(names, ["Insert", "Delete", "Rename", "Duplicate", "Page Setup", "Reorder Pages"]);
    let enabled: Vec<bool> = rows.iter().map(|(.., e)| *e).collect();
    assert_eq!(enabled, [true, false, true, true, true, false], "one page: nothing to delete or to put in order");
}

/// Delete takes the page out; Undo brings it back.
#[test]
fn delete_in_the_tab_menu_takes_the_page_out() {
    let mut w = design_window();
    run_command(&mut w, "page.add", json!({"name": "Second"}));
    right_click_on(&mut w, "page-tab.2");
    assert!(is_enabled(&w, "page-menu.Delete"));
    click_on(&mut w, "page-menu.Delete");
    w.rest();
    assert_eq!(w.state().session().document().unwrap().pages.len(), 1);
    assert!(w.state().element("page-tab.2").is_none());
    assert_eq!(w.state().ui_panics(), 0);
    undo(&mut w);
    assert_eq!(w.state().session().document().unwrap().pages.len(), 2);
}

/// A tab dragged to another place moves the page (`page.move`), and the marker shows where it would land.
#[test]
fn dragging_a_tab_moves_the_page() {
    let mut w = design_window();
    run_command(&mut w, "page.add", json!({"name": "Two"}));
    run_command(&mut w, "page.add", json!({"name": "Three"}));
    let order = |w: &Window| -> Vec<String> { w.state().session().document().unwrap().pages.iter().map(|p| p.name.to_string()).collect() };
    assert_eq!(order(&w), ["Page-1", "Two", "Three"]);
    let (from, to) = (rect_of(&w, "page-tab.3").center(), rect_of(&w, "page-tab.1").center() - vec2(10.0, 0.0));
    let revision = w.state().session().revision();
    drag(&mut w, from, to - from);
    w.rest();
    assert_eq!(order(&w), ["Three", "Page-1", "Two"], "the third tab went first");
    assert_eq!(w.state().session().revision(), revision + 1, "one command");
    // A drag that ends where it began moves nothing.
    let revision = w.state().session().revision();
    let tab = rect_of(&w, "page-tab.2").center();
    drag(&mut w, tab, vec2(6.0, 0.0));
    w.rest();
    assert_eq!(w.state().session().revision(), revision, "a tab let go in its own place sends nothing");
    undo(&mut w);
    assert_eq!(order(&w), ["Page-1", "Two", "Three"]);
}

/// Rename puts the name into edit in place; Enter keeps it, and Escape leaves the name as it was.
#[test]
fn rename_in_the_tab_menu_edits_the_name_in_place() {
    let mut w = design_window();
    right_click_on(&mut w, "page-tab.1");
    click_on(&mut w, "page-menu.Rename");
    w.rest();
    assert!(w.state().element("page-tab.rename").is_some(), "the name is in a box over the tab");
    type_text(&mut w, "Overview");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert!(w.state().element("page-tab.rename").is_none());
    assert_eq!(setup(&mut w)["name"], "Overview");
    right_click_on(&mut w, "page-tab.1");
    click_on(&mut w, "page-menu.Rename");
    w.rest();
    type_text(&mut w, "Nope");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(setup(&mut w)["name"], "Overview", "Escape keeps the name it had");
    assert!(w.state().element("page-tab.rename").is_none());
}

/// Insert makes a page like the one on view and opens Page Setup on its Page Properties.
#[test]
fn insert_makes_a_page_like_this_one_and_opens_its_properties() {
    let mut w = design_window();
    run_command(&mut w, "page.size", json!({"preset": "legal"}));
    right_click_on(&mut w, "page-tab.1");
    click_on(&mut w, "page-menu.Insert");
    w.rest();
    assert_eq!(w.state().session().document().unwrap().pages.len(), 2);
    assert_eq!(w.state().window_state()["pageSetup"], "Page Properties");
    assert_eq!(w.state().window_state()["page"], 2, "the new page is shown");
    assert_eq!(setup_of(&mut w, 2)["size"]["id"], "legal");
    // The name typed there is the new page's.
    let name = rect_of(&w, "pagesetup.name").center();
    click(&mut w, name);
    key(&mut w, Key::A, crate::saving::ctrl());
    type_text(&mut w, "Detail");
    ok(&mut w);
    assert_eq!(setup_of(&mut w, 2)["name"], "Detail");
}

/// The Reorder Pages dialog (Ctrl+Alt+P): the arrows move the page chosen, and the box renames the default names for the new places.
#[test]
fn the_reorder_pages_dialog_moves_pages_and_renumbers_their_names() {
    let mut w = design_window();
    run_command(&mut w, "page.add", json!({}));
    run_command(&mut w, "page.add", json!({}));
    let order =
        |w: &Window| -> Vec<(u32, String)> { w.state().session().document().unwrap().pages.iter().map(|p| (p.id, p.name.to_string())).collect() };
    assert_eq!(order(&w).iter().map(|p| p.0).collect::<Vec<_>>(), [1, 2, 3]);
    key(&mut w, Key::P, Modifiers { ctrl: true, command: true, alt: true, ..Modifiers::NONE });
    assert!(w.state().window_state()["reorderPages"].as_bool().unwrap(), "Ctrl+Alt+P opens it");
    press(&mut w, "reorder.page.3");
    press(&mut w, "reorder.up");
    press(&mut w, "reorder.up");
    assert_eq!(order(&w).iter().map(|p| p.0).collect::<Vec<_>>(), [3, 1, 2], "page 3 went to the front");
    assert!(!is_enabled(&w, "reorder.up"), "it cannot go further");
    assert_eq!(order(&w)[0].1, "Page-3", "the names stay while the box is off");
    press(&mut w, "reorder.renumber");
    press(&mut w, "reorder.down");
    assert_eq!(order(&w).iter().map(|p| p.0).collect::<Vec<_>>(), [1, 3, 2]);
    assert_eq!(order(&w).iter().map(|p| p.1.clone()).collect::<Vec<_>>(), ["Page-1", "Page-2", "Page-3"], "renumbered for the places they have");
    press(&mut w, "reorder.close");
    assert!(!w.state().window_state()["reorderPages"].as_bool().unwrap());
}

/// The File view's Print page opens the dialog on Print Setup, over the File view.
#[test]
fn the_print_view_opens_page_setup_on_print_setup() {
    let mut w = design_window();
    run_command(&mut w, "window.backstage", json!({"page": "Print"}));
    press(&mut w, "print.page_setup");
    w.rest();
    assert_eq!(w.state().window_state()["pageSetup"], "Print Setup");
    assert_eq!(w.state().window_state()["backstage"], "Print", "over the File view");
    press(&mut w, "pagesetup.cancel");
    w.rest();
}

/// A page set up in the dialog is saved with the drawing and read again.
#[test]
fn a_page_set_up_in_the_dialog_is_saved_and_reopened() {
    let dir = crate::support::TempDir::new("page-setup-save");
    let path = dir.file("setup.vsdx");
    let mut w = design_window();
    run_command(&mut w, "page.size", json!({"preset": "a4"}));
    run_command(&mut w, "page.scale", json!({"preset": "metric_1_100"}));
    run_command(&mut w, "page.auto_size", json!({"on": true}));
    let want = setup(&mut w);
    w.state_mut().execute("file.save_as", &json!({"path": path.to_str().unwrap()})).unwrap();
    let mut again = diacraft_engine::Session::new();
    again.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let got = again.execute("page.setup", &json!({})).unwrap();
    for key in ["width", "height", "orientation", "size", "scale", "auto_size"] {
        assert_eq!(got[key], want[key], "{key}");
    }
}

// ---- narrow windows --------------------------------------------------------------------------------------------------------------------------

/// The Page Setup group's controls and the Tools group's two new ones can be reached at every width, in the group or behind its folded button, and
/// they work from there.
#[test]
fn the_page_setup_group_and_the_text_tools_can_be_reached_in_narrow_windows() {
    for width in [480.0, 640.0, 800.0, 1280.0] {
        let mut w = crate::support::drawing_window([width, 720.0]);
        // The Design tab: Orientation, Size and Auto Size, in the group or behind its fold.
        run_command(&mut w, "window.tab", json!({"tab": "Design"}));
        if w.state().element("ribbon.Design.AutoSizePage").is_none() {
            let fold =
                w.state().element("ribbon.Design.fold.GroupPrintPreviewPageSetup").unwrap_or_else(|| panic!("no Page Setup group at {width}")).rect;
            click(&mut w, fold.center());
            w.step();
            w.step();
        }
        for id in ["PageOrientation", "PageSize", "AutoSizePage"] {
            let name = format!("ribbon.Design.{id}");
            assert!(w.state().element(&name).is_some_and(|e| e.enabled), "{name} works at {width}");
        }
        click_on(&mut w, "ribbon.Design.AutoSizePage");
        w.rest();
        assert_eq!(setup(&mut w)["auto_size"], true, "Auto Size works from where it is at {width}");
        // The Home tab: the Text tool and the Text Block tool.
        run_command(&mut w, "window.tab", json!({"tab": "Home"}));
        if w.state().element("ribbon.Home.TextBlockTool").is_none() {
            let fold = w.state().element("ribbon.Home.fold.GroupTools").unwrap_or_else(|| panic!("no Tools group at {width}")).rect;
            click(&mut w, fold.center());
            w.step();
            w.step();
        }
        for id in ["TextTool", "TextBlockTool"] {
            let name = format!("ribbon.Home.{id}");
            assert!(w.state().element(&name).is_some_and(|e| e.enabled), "{name} works at {width}");
        }
        click_on(&mut w, "ribbon.Home.TextBlockTool");
        w.rest();
        assert_eq!(w.state().window_state()["tool"], "text_block", "the Text Block tool turns on from where it is at {width}");
        assert_eq!(w.state().ui_panics(), 0);
    }
}

// ---- hostile input ---------------------------------------------------------------------------------------------------------------------------

/// The window commands of this task refuse what they cannot take, in words, and change nothing; a page taken out under an open dialog or a name being
/// typed leaves the window as it was.
#[test]
fn the_page_setup_commands_refuse_hostile_input_and_a_page_taken_away_under_a_dialog_is_survived() {
    let mut w = design_window();
    let revision = w.state().session().revision();
    for params in [
        json!({}),
        json!({"tab": 5}),
        json!({"tab": "Page Size", "page": 99}),
        json!({"tab": "Page Size", "page": -1}),
        json!({"tab": "Page Size", "page": 1.5}),
        json!({"tab": "Page Size", "extra": true}),
        json!({"tab": "x".repeat(10_000)}),
        json!({"close": "yes"}),
    ] {
        assert!(w.state_mut().execute("window.pageSetup", &params).is_err(), "{params} is refused");
    }
    assert!(w.state().window_state()["pageSetup"].is_null());
    assert_eq!(w.state().session().revision(), revision);
    // One page: nothing to put in order.
    assert!(w.state_mut().execute("window.reorderPages", &json!({})).is_err());
    assert!(w.state_mut().execute("window.reorderPages", &json!({"close": 7})).is_err());
    // A dialog over a page that is taken away: OK says why and nothing breaks.
    run_command(&mut w, "page.add", json!({"name": "Second"}));
    run_command(&mut w, "window.pageSetup", json!({"tab": "Page Size", "page": 2}));
    press(&mut w, "pagesetup.size.legal");
    w.state_mut().execute("page.delete", &json!({"id": 2})).unwrap();
    w.rest();
    ok(&mut w);
    assert_eq!(w.state().window_state()["pageSetup"], "Page Size", "the dialog stays, with the refusal in it");
    assert_eq!(w.state().ui_panics(), 0);
    press(&mut w, "pagesetup.cancel");
    w.rest();
    // A name being typed on a tab that goes away.
    run_command(&mut w, "page.add", json!({"name": "Third"}));
    right_click_on(&mut w, "page-tab.2");
    click_on(&mut w, "page-menu.Rename");
    w.rest();
    w.state_mut().execute("edit.undo", &json!({})).unwrap();
    w.rest();
    assert!(w.state().element("page-tab.rename").is_none(), "no box is left over a tab that is gone");
    assert_eq!(w.state().ui_panics(), 0);
}
