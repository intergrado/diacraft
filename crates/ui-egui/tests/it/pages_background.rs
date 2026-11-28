//! In the window: the Insert tab's Pages group (Blank Page, Background Page, Duplicate This Page), the page tab's menu with its six rows working, Page Setup's Page
//! Properties (Type, Background), the tabs of background pages, and a page drawn over its background. The pointer is given to egui as a person's would be, and what is checked
//! is the document the clicks changed through commands.

use std::sync::{Arc, Mutex};

use diacraft_render::{Rendered, Renderer};
use diacraft_ui_egui::render_worker::{Drawer, Job, MakeDrawer};
use egui::{Event, Modifiers, PointerButton, Pos2, Rect};
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

use crate::support::{Rest, Window, click, drawing_window, elements_named, input, move_to, run_command};

fn pages(w: &Window) -> Vec<(u32, String, bool, Option<u32>)> {
    w.state().session().document().unwrap().pages.iter().map(|p| (p.id, p.name.to_string(), p.background, p.back_page)).collect()
}

fn names(w: &Window) -> Vec<String> {
    pages(w).into_iter().map(|(_, name, ..)| name).collect()
}

fn shown_page(w: &Window) -> u64 {
    w.state().window_state()["page"].as_u64().unwrap()
}

fn rect_of(w: &Window, name: &str) -> Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

fn click_on(w: &mut Window, name: &str) {
    let at = rect_of(w, name).center();
    click(w, at);
    w.rest();
}

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

fn right_click_on(w: &mut Window, name: &str) {
    let at = rect_of(w, name).center();
    right_click(w, at);
}

fn is_enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

/// The window on the Insert tab with the New Page menu open.
fn insert_menu() -> Window {
    let mut w = drawing_window([1280.0, 800.0]);
    run_command(&mut w, "window.tab", json!({"tab": "Insert"}));
    w.get_by_label("New Page").click();
    w.step();
    w.step();
    w
}

fn two_shapes(w: &mut Window) {
    run_command(w, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0}));
    run_command(w, "shape.drop", json!({"master": "ellipse", "page": 1, "x": 6.0, "y": 4.0}));
}

// ---------------------------------------------------------------------------------------------------------------------- Insert > Pages

#[test]
fn duplicate_this_page_in_the_new_page_menu_copies_the_page_in_view_and_shows_the_copy() {
    let mut w = insert_menu();
    two_shapes(&mut w);
    let before = w.state().session().document().unwrap().page(1).unwrap().shapes.len();
    assert_eq!(before, 2);
    let node = w.get_by_label("Duplicate This Page");
    assert!(!node.accesskit_node().is_disabled(), "the row works now");
    node.click();
    w.rest();
    assert_eq!(names(&w), ["Page-1", "Page-1-2"]);
    assert_eq!(shown_page(&w), 2, "the copy is the page on view");
    assert_eq!(w.state().session().document().unwrap().page(2).unwrap().shapes.len(), before);
    // The status bar counts it, and one Undo takes the copy away and goes back to the page it was made from.
    w.get_by_label("Page 2 of 2");
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(names(&w), ["Page-1"]);
    assert_eq!(shown_page(&w), 1);
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn background_page_in_the_new_page_menu_adds_a_background_page_and_shows_it_after_the_other_tabs() {
    let mut w = insert_menu();
    let node = w.get_by_label("Background Page");
    assert!(!node.accesskit_node().is_disabled(), "the row works now");
    node.click();
    w.rest();
    assert_eq!(
        pages(&w),
        [(2, "VBackground-1".to_owned(), true, None), (1, "Page-1".to_owned(), false, None)],
        "the model lists backgrounds first, as a file does"
    );
    assert_eq!(shown_page(&w), 2, "the new background page is on view, where what is drawn on it is");
    // The tabs: the page that is not a background first, then the background, set apart from it.
    let mut tabs = elements_named(&w, "page-tab.");
    tabs.sort_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x));
    assert_eq!(tabs.iter().map(|(n, ..)| n.as_str()).collect::<Vec<_>>(), ["page-tab.1", "page-tab.2"]);
    let gap = tabs[1].1.min.x - tabs[0].1.max.x;
    assert!(gap > 4.0, "a gap sets the background's tab apart: {gap}");
    // It is the setup of the page it was made beside.
    assert_eq!(w.state_mut().execute("page.setup", &json!({"page": 2})).unwrap()["background"], true);
    // A second Background Page goes after the first of them, with the next number.
    w.get_by_label("New Page").click();
    w.step();
    w.step();
    w.get_by_label("Background Page").click();
    w.rest();
    assert_eq!(names(&w), ["VBackground-1", "VBackground-2", "Page-1"]);
}

#[test]
fn blank_page_in_the_new_page_menu_is_like_the_page_in_view() {
    let mut w = insert_menu();
    w.state_mut().execute("page.size", &json!({"preset": "a4"})).unwrap();
    w.get_by_label("Blank Page").click();
    w.rest();
    assert_eq!(w.state_mut().execute("page.setup", &json!({"page": 2})).unwrap()["size"]["id"], "a4");
    assert_eq!(shown_page(&w), 1, "the page in view stays, as it does for the plus of the tab row");
}

// ---------------------------------------------------------------------------------------------------------------------- the page tab's menu

#[test]
fn the_page_tab_menu_has_six_rows_and_each_one_works_or_says_why_not() {
    let mut w = drawing_window([1280.0, 800.0]);
    right_click_on(&mut w, "page-tab.1");
    let mut rows = elements_named(&w, "page-menu.");
    rows.sort_by(|a, b| a.1.min.y.total_cmp(&b.1.min.y));
    let listed: Vec<&str> = rows.iter().map(|(n, ..)| n.strip_prefix("page-menu.").unwrap()).collect();
    assert_eq!(listed, ["Insert", "Delete", "Rename", "Duplicate", "Page Setup", "Reorder Pages"]);
    let enabled: Vec<bool> = rows.iter().map(|(.., e)| *e).collect();
    assert_eq!(enabled, [true, false, true, true, true, false], "one page: nothing to delete or to put in order, and Duplicate works");
    // Duplicate copies the page and shows the copy.
    crate::editing::key(&mut w, egui::Key::Escape, Modifiers::NONE);
    two_shapes(&mut w);
    right_click_on(&mut w, "page-tab.1");
    click_on(&mut w, "page-menu.Duplicate");
    assert_eq!(names(&w), ["Page-1", "Page-1-2"]);
    assert_eq!(shown_page(&w), 2);
    assert_eq!(w.state().session().document().unwrap().page(2).unwrap().shapes.len(), 2);
    // With two pages the other two rows work.
    right_click_on(&mut w, "page-tab.2");
    assert!(is_enabled(&w, "page-menu.Delete") && is_enabled(&w, "page-menu.Reorder Pages"));
    click_on(&mut w, "page-menu.Delete");
    assert_eq!(names(&w), ["Page-1"]);
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn the_tab_menu_of_a_background_page_and_of_the_only_page_that_is_not_one() {
    let mut w = drawing_window([1280.0, 800.0]);
    run_command(&mut w, "page.add", json!({"background": true}));
    // A background page: its tab is at the right, it has the whole menu, and it can be deleted.
    right_click_on(&mut w, "page-tab.2");
    assert!(is_enabled(&w, "page-menu.Delete") && is_enabled(&w, "page-menu.Duplicate") && is_enabled(&w, "page-menu.Rename"));
    // Reorder Pages puts the pages that are not backgrounds in order: with one there is nothing to put in order.
    assert!(!is_enabled(&w, "page-menu.Reorder Pages"));
    crate::editing::key(&mut w, egui::Key::Escape, Modifiers::NONE);
    right_click_on(&mut w, "page-tab.1");
    assert!(!is_enabled(&w, "page-menu.Delete"), "a drawing keeps one page that is not a background, and the row says so before it is pressed");
    assert_eq!(w.state().session().document().unwrap().pages.len(), 2);
    // Duplicate of page 1 copies it and shows the copy (page 3: the background page is page 2).
    click_on(&mut w, "page-menu.Duplicate");
    assert_eq!(shown_page(&w), 3);
    assert_eq!(names(&w), ["VBackground-1", "Page-1", "Page-1-2"], "the copy is right after its source, the background first in the model");
}

#[test]
fn deleting_a_background_page_that_a_page_names_says_so_in_the_status_bar_and_undo_puts_it_back() {
    let mut w = drawing_window([1280.0, 800.0]);
    run_command(&mut w, "page.add", json!({"background": true}));
    run_command(&mut w, "page.background", json!({"background": 2, "page": 1}));
    right_click_on(&mut w, "page-tab.2");
    click_on(&mut w, "page-menu.Delete");
    assert_eq!(pages(&w), [(1, "Page-1".to_owned(), false, None)]);
    w.get_by_label_contains("1 page had it as its background");
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(pages(&w).iter().find(|p| p.0 == 1).unwrap().3, Some(2));
}

// ---------------------------------------------------------------------------------------------------------------------- Page Setup, Page Properties

#[test]
fn page_properties_makes_a_page_a_background_and_gives_another_page_that_background() {
    let mut w = drawing_window([1280.0, 900.0]);
    run_command(&mut w, "page.add", json!({"name": "Frame"}));
    // Page 2 becomes a background page through the dialog.
    run_command(&mut w, "window.pageSetup", json!({"tab": "Page Properties", "page": 2}));
    assert!(is_enabled(&w, "pagesetup.type.background") && is_enabled(&w, "pagesetup.type.foreground"));
    click_on(&mut w, "pagesetup.type.background");
    click_on(&mut w, "pagesetup.ok");
    assert_eq!(pages(&w), [(2, "Frame".to_owned(), true, None), (1, "Page-1".to_owned(), false, None)]);
    // Page 1 names it: the Background box lists None and the background page.
    run_command(&mut w, "window.pageSetup", json!({"tab": "Page Properties", "page": 1}));
    click_on(&mut w, "pagesetup.background");
    let items = w.get_all_by_label("Frame");
    items.last().unwrap().click();
    w.rest();
    click_on(&mut w, "pagesetup.ok");
    assert_eq!(pages(&w).iter().find(|p| p.0 == 1).unwrap().3, Some(2), "page 1 is shown over Frame");
    // Each OK is one undo step.
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(pages(&w).iter().find(|p| p.0 == 1).unwrap().3, None);
    assert!(pages(&w).iter().find(|p| p.0 == 2).unwrap().2, "the first OK is a step of its own");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn page_properties_does_not_let_the_only_page_that_is_not_a_background_become_one_and_lists_none_with_no_background_page() {
    let mut w = drawing_window([1280.0, 900.0]);
    run_command(&mut w, "window.pageSetup", json!({"tab": "Page Properties"}));
    assert!(!is_enabled(&w, "pagesetup.type.background"), "a drawing keeps one page that is not a background");
    assert!(is_enabled(&w, "pagesetup.type.foreground"));
    // With no background page in the drawing the Background box holds only None.
    click_on(&mut w, "pagesetup.background");
    assert!(w.query_by_label("None").is_some());
    assert_eq!(w.state().session().document().unwrap().pages.len(), 1);
}

/// A background page is sized apart from the pages that name it, by showing it and setting it up: the Design tab's Orientation, Size and Auto Size
/// work on the page in view, and not on the first page that is not a background.
#[test]
fn the_design_tab_sets_up_the_page_in_view_and_a_background_page_is_set_up_by_showing_it() {
    let mut w = drawing_window([1280.0, 900.0]);
    run_command(&mut w, "page.add", json!({"background": true}));
    run_command(&mut w, "page.select", json!({"id": 2}));
    run_command(&mut w, "window.tab", json!({"tab": "Design"}));
    let setup = |w: &mut Window, page: u32| w.state_mut().execute("page.setup", &json!({"page": page})).unwrap();
    assert_eq!((setup(&mut w, 1)["orientation"].as_str(), setup(&mut w, 2)["orientation"].as_str()), (Some("landscape"), Some("landscape")));
    crate::support::press(&mut w, "ribbon.Design.PageOrientation");
    click_on(&mut w, "menu.PageOrientationPortrait");
    assert_eq!(setup(&mut w, 2)["orientation"], "portrait", "the page in view, the background page, is turned");
    assert_eq!(setup(&mut w, 1)["orientation"], "landscape", "and the page that names it is not");
    crate::support::press(&mut w, "ribbon.Design.PageSize");
    click_on(&mut w, "size.item.a4");
    assert_eq!(setup(&mut w, 2)["size"]["id"], "a4");
    assert_eq!(setup(&mut w, 1)["size"]["id"], "letter");
    click_on(&mut w, "ribbon.Design.AutoSizePage");
    assert_eq!((setup(&mut w, 2)["auto_size"].as_bool(), setup(&mut w, 1)["auto_size"].as_bool()), (Some(true), Some(false)));
    // The first page, shown, is the one the group works on then.
    run_command(&mut w, "page.select", json!({"id": 1}));
    crate::support::press(&mut w, "ribbon.Design.PageOrientation");
    click_on(&mut w, "menu.PageOrientationPortrait");
    assert_eq!(setup(&mut w, 1)["orientation"], "portrait");
}

// ---------------------------------------------------------------------------------------------------------------------- the picture

/// The last picture a drawer made: the page, the coefficients of the view's transform (paper inches to pixels) and the pixels.
type Shown = Arc<Mutex<Option<(u32, [f64; 6], Rendered)>>>;

/// A drawer over the real renderer that keeps the last picture it made, so that a test reads the pixels the window was given.
struct Recording {
    renderer: Renderer,
    last: Shown,
}

impl Drawer for Recording {
    fn draw(&mut self, job: &Job) -> Rendered {
        self.renderer.set_limits(job.limits);
        self.renderer.set_paper(None);
        let rendered = self.renderer.render(&job.doc, job.page, &job.view);
        *self.last.lock().unwrap() = Some((job.page, job.view.to_px.as_coeffs(), rendered.clone()));
        rendered
    }
}

fn recording() -> (MakeDrawer, Shown) {
    let last = Arc::new(Mutex::new(None));
    let kept = Arc::clone(&last);
    (Box::new(move || Box::new(Recording { renderer: Renderer::new(), last: kept })), last)
}

/// The colour the last picture has at (`x`, `y`) inches of its page.
fn colour_at(last: &Shown, x: f64, y: f64) -> (u32, [u8; 3]) {
    let guard = last.lock().unwrap();
    let (page, c, r) = guard.as_ref().expect("a picture was drawn");
    let (px, py) = (c[0] * x + c[2] * y + c[4], c[1] * x + c[3] * y + c[5]);
    let (col, row) = ((px.max(0.0) as usize).min(r.width as usize - 1), (py.max(0.0) as usize).min(r.height as usize - 1));
    let i = (row * r.width as usize + col) * 4;
    (*page, [r.rgba_premul[i], r.rgba_premul[i + 1], r.rgba_premul[i + 2]])
}

#[test]
fn a_page_is_drawn_over_the_background_page_it_names_and_the_background_is_not_selectable_from_it() {
    let (make, last) = recording();
    let mut session = diacraft_engine::Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let app = diacraft_ui_egui::DiacraftApp::new(session, None).with_inline_pictures(make);
    let mut w = crate::support::window(app, [1280.0, 900.0]);
    w.rest();
    // A background page with a red rectangle across the middle, and a page with a blue square on it.
    run_command(&mut w, "page.add", json!({"background": true}));
    let red = w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": 2, "x": 4.0, "y": 4.0})).unwrap()["id"].as_u64().unwrap();
    for (command, params) in [
        ("shape.size", json!({"ids": [red], "width": 4.0, "height": 3.0, "page": 2})),
        ("shape.format", json!({"ids": [red], "fill": "#FF0000", "line": "none", "page": 2})),
    ] {
        w.state_mut().execute(command, &params).unwrap();
    }
    let blue = w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": 5.0, "y": 4.0})).unwrap()["id"].as_u64().unwrap();
    for (command, params) in [
        ("shape.size", json!({"ids": [blue], "width": 2.0, "height": 2.0, "page": 1})),
        ("shape.format", json!({"ids": [blue], "fill": "#0000FF", "line": "none", "page": 1})),
    ] {
        w.state_mut().execute(command, &params).unwrap();
    }
    run_command(&mut w, "page.select", json!({"id": 1}));
    // Not named: the page shows its own square only.
    assert_eq!(colour_at(&last, 3.0, 4.0), (1, [0, 0, 0]), "nothing of the background shows through a page that does not name it (clear pixels)");
    assert_eq!(colour_at(&last, 5.0, 4.0), (1, [0, 0, 255]));
    // Named: the red rectangle is under the blue square.
    run_command(&mut w, "page.background", json!({"background": 2, "page": 1}));
    assert_eq!(colour_at(&last, 3.0, 4.0), (1, [255, 0, 0]), "the background's shape shows where the page has none");
    assert_eq!(colour_at(&last, 5.0, 4.0), (1, [0, 0, 255]), "and the page's own shape is over it");
    // The pointer meets only the shapes of the page on view: a click on the red rectangle selects nothing.
    let at = crate::editing::at(&w, 3.0, 4.0);
    click(&mut w, at);
    w.rest();
    assert!(w.state().window_state()["selection"].as_array().unwrap().is_empty(), "{}", w.state().window_state()["selection"]);
    // The background page shown by itself is the page its own shapes are on: they are selectable there.
    run_command(&mut w, "page.select", json!({"id": 2}));
    let at = crate::editing::at(&w, 3.0, 4.0);
    click(&mut w, at);
    w.rest();
    assert_eq!(w.state().window_state()["selection"], json!([red]));
    assert_eq!(w.state().ui_panics(), 0);
}

// ---------------------------------------------------------------------------------------------------------------------- the page a drawing opens on

/// A window on the drawing the file at `path` holds, opened as a person opens one.
fn window_on(path: &std::path::Path) -> Window {
    let mut session = diacraft_engine::Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let app = diacraft_ui_egui::DiacraftApp::new(session, None).with_inline_pictures(diacraft_ui_egui::render_worker::bundled_drawer());
    let mut w = crate::support::fast_window(app, [1280.0, 800.0]);
    w.rest();
    w
}

/// A drawing with a background page, saved and opened again, opens on the first page that is not a background, whatever place the file lists the pages in, and the tabs put the background
/// pages after the others, set apart (a file lists its background pages first, and the window of the file names a page that is not one).
#[test]
fn a_drawing_with_a_background_page_opens_on_a_foreground_page_and_its_tabs_put_the_background_last() {
    for extension in ["vsdx", "diacraft"] {
        let dir = crate::support::TempDir::new("start-page");
        let path = dir.file(&format!("start.{extension}"));
        let mut made = diacraft_engine::Session::new();
        made.execute("file.new", &json!({})).unwrap();
        made.execute("page.add", &json!({"background": true})).unwrap();
        made.execute("page.add", &json!({"name": "Second"})).unwrap();
        made.execute("file.save_as", &json!({"path": path.to_str().unwrap()})).unwrap();
        let w = window_on(&path);
        assert_eq!(names(&w), ["VBackground-1", "Page-1", "Second"], "{extension}: the file lists the background page first");
        assert_eq!(shown_page(&w), 1, "{extension}: the drawing opens on `Page-1`, a page that is not a background");
        let mut tabs = elements_named(&w, "page-tab.");
        tabs.sort_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x));
        let order: Vec<&str> = tabs.iter().map(|(n, ..)| n.as_str()).collect();
        assert_eq!(order, ["page-tab.1", "page-tab.3", "page-tab.2"], "{extension}: the background's tab is the last");
        let gap = tabs[2].1.min.x - tabs[1].1.max.x;
        assert!(gap > 4.0, "{extension}: a gap sets the background's tab apart: {gap}");
    }
}

/// A file an earlier build wrote names a background page in its window and in `TopPage` (it took the model's first page, which lists the backgrounds first): the window opens on the first
/// page that is not a background all the same, as it does whatever the window says, and the background's tab is the last.
#[test]
fn a_file_whose_window_names_its_background_page_still_opens_on_a_foreground_page() {
    let dir = crate::support::TempDir::new("start-window");
    // The demo drawing's `Frame` (id 4) is its background page; its window and its document settings name it.
    let windows = crate::fixture::WINDOWS_PART.replace(r#"WindowType="Drawing""#, r#"WindowType="Drawing" ContainerType="Page" Page="4""#);
    let bytes = crate::fixture::demo_builder(Some(&windows)).document_extra(r#"<DocumentSettings TopPage="4"/>"#).build();
    let path = crate::fixture::write(dir.path(), "earlier.vsdx", &bytes);
    let w = window_on(&path);
    assert_eq!(shown_page(&w), 1, "`Overview`, the first page that is not a background, and not `Frame`");
    let mut tabs = elements_named(&w, "page-tab.");
    tabs.sort_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x));
    let order: Vec<&str> = tabs.iter().map(|(n, ..)| n.as_str()).collect();
    assert_eq!(order, ["page-tab.1", "page-tab.2", "page-tab.3", "page-tab.4"], "the background's tab is the last");
}
