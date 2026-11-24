//! The Design tab's Layout > Connectors menu, Themes gallery and Variants gallery. The pointer is given to egui as a person's would be, and
//! what is checked is the document the engine's commands changed and what the controls show of it.

use diacraft_engine::draw::{PathStyle, StyleChoice, connector_path, line_jump_settings, page_style};
use egui::accesskit::Toggled;
use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::{Value, json};

use crate::editing::edit_window;
use crate::support::{Rest, Window, click, input, move_to, press, run_command};

/// A new drawing on the Design tab, wide enough to show every group in full.
fn design_window() -> Window {
    let mut w = edit_window([1600.0, 900.0]);
    run_command(&mut w, "window.tab", json!({"tab": "Design"}));
    w
}

fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

fn toggled(w: &Window, label: &str) -> Option<Toggled> {
    w.get_by_label(label).accesskit_node().toggled()
}

fn is_enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

fn rect_of(w: &Window, name: &str) -> egui::Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

fn click_on(w: &mut Window, name: &str) {
    let at = rect_of(w, name).center();
    click(w, at);
    w.rest();
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

fn revision(w: &Window) -> u64 {
    w.state().session().revision()
}

fn undo(w: &mut Window) {
    run_command(w, "edit.undo", json!({}));
}

fn drop_shape(w: &mut Window, master: &str, x: f64, y: f64) -> u32 {
    let page = page_id(w);
    let made = w.state_mut().execute("shape.drop", &json!({"master": master, "page": page, "x": x, "y": y})).unwrap();
    w.rest();
    made["id"].as_u64().unwrap() as u32
}

fn connect(w: &mut Window, from: u32, to: u32) -> u32 {
    let page = page_id(w);
    let made = w.state_mut().execute("connector.add", &json!({"from": {"id": from}, "to": {"id": to}, "page": page})).unwrap();
    w.rest();
    made["id"].as_u64().unwrap() as u32
}

/// The style a connector draws (`None` for a connector that is not made of lines, a curved one).
fn drawn_style(w: &Window, id: u32) -> Option<PathStyle> {
    connector_path(w.state().session().document().unwrap(), page_id(w), id).map(|p| p.style)
}

fn default_style(w: &Window) -> StyleChoice {
    page_style(w.state().session().document().unwrap(), page_id(w))
}

/// `theme.list`, with the theme the first page has under `page.theme`.
fn theme_list(w: &mut Window) -> Value {
    w.state_mut().execute("theme.list", &json!({})).unwrap()
}

fn page_theme(w: &mut Window) -> String {
    theme_list(w)["page"]["theme"].as_str().unwrap().to_owned()
}

fn theme_ids(w: &mut Window) -> Vec<String> {
    theme_list(w)["themes"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_owned()).collect()
}

fn theme_name(w: &mut Window, id: &str) -> String {
    theme_list(w)["themes"].as_array().unwrap().iter().find(|t| t["id"] == id).unwrap()["name"].as_str().unwrap().to_owned()
}

/// Opens the arrow at the end of the gallery `id`.
fn open_gallery(w: &mut Window, id: &str) {
    click_on(w, &format!("ribbon.Design.{id}.more"));
}

// ---- Layout > Connectors ---------------------------------------------------------------------------------------------------------------------

const STYLE_ROWS: [&str; 3] = ["ConnectorRightAngle", "ConnectorStraightLines", "ConnectorCurvedLines"];

/// With no connector selected the menu sets the page's default for new connectors, marks the one the page has, and each change is one undo step.
#[test]
fn the_connectors_menu_sets_the_pages_default_style_and_marks_it() {
    let mut w = design_window();
    assert_eq!(default_style(&w), StyleChoice::RightAngle, "a new page makes right angles");
    press(&mut w, "ribbon.Design.ConnectorsGallery");
    for row in STYLE_ROWS.into_iter().chain(["ConnectorShowLineJumps"]) {
        assert!(is_enabled(&w, &format!("menu.{row}")), "{row} works on a page with nothing selected");
    }
    assert_eq!(toggled(&w, "Right-Angle"), Some(Toggled::True), "the style the page has is marked");
    assert_ne!(toggled(&w, "Curved Lines"), Some(Toggled::True));
    let before = revision(&w);
    click_on(&mut w, "menu.ConnectorCurvedLines");
    assert_eq!(default_style(&w), StyleChoice::Curved);
    assert_eq!(revision(&w), before + 1, "one command, one step");
    press(&mut w, "ribbon.Design.ConnectorsGallery");
    assert_eq!(toggled(&w, "Curved Lines"), Some(Toggled::True), "and the mark follows");
    assert_ne!(toggled(&w, "Right-Angle"), Some(Toggled::True));
    click_on(&mut w, "menu.ConnectorStraightLines");
    assert_eq!(default_style(&w), StyleChoice::Straight);
    undo(&mut w);
    assert_eq!(default_style(&w), StyleChoice::Curved, "one undo takes one choice back");
}

/// With connectors selected the menu styles those connectors and not the page's default or the other connectors; one undo takes it back.
#[test]
fn the_connectors_menu_styles_the_selected_connectors() {
    let mut w = design_window();
    let (a, b, c) = (drop_shape(&mut w, "rectangle", 2.0, 5.0), drop_shape(&mut w, "rectangle", 6.0, 3.0), drop_shape(&mut w, "rectangle", 9.0, 6.0));
    let (first, second) = (connect(&mut w, a, b), connect(&mut w, b, c));
    assert_eq!(drawn_style(&w, first), Some(PathStyle::RightAngle));
    w.state_mut().select(&[first]);
    w.rest();
    press(&mut w, "ribbon.Design.ConnectorsGallery");
    assert_ne!(toggled(&w, "Right-Angle"), Some(Toggled::True), "the marks are the page's default, which a connector selected is not about");
    let before = revision(&w);
    click_on(&mut w, "menu.ConnectorStraightLines");
    assert_eq!(drawn_style(&w, first), Some(PathStyle::Straight), "the selected connector is straight");
    assert_eq!(drawn_style(&w, second), Some(PathStyle::RightAngle), "the other connector is as it was");
    assert_eq!(default_style(&w), StyleChoice::RightAngle, "and so is the page's default");
    assert_eq!(revision(&w), before + 1);
    // Curved lines make a curve, which `connector_path` does not read as a path of lines.
    press(&mut w, "ribbon.Design.ConnectorsGallery");
    click_on(&mut w, "menu.ConnectorCurvedLines");
    assert_eq!(drawn_style(&w, first), None, "a curve is no path of lines");
    undo(&mut w);
    assert_eq!(drawn_style(&w, first), Some(PathStyle::Straight), "one undo takes the curve back");
    // Both connectors selected, with a shape: only the connectors are styled.
    w.state_mut().select(&[first, second, a]);
    w.rest();
    press(&mut w, "ribbon.Design.ConnectorsGallery");
    click_on(&mut w, "menu.ConnectorRightAngle");
    assert_eq!(drawn_style(&w, first), Some(PathStyle::RightAngle));
    assert_eq!(drawn_style(&w, second), Some(PathStyle::RightAngle));
    assert_eq!(default_style(&w), StyleChoice::RightAngle);
}

/// A selection of shapes and no connector is not a selection the menu can style: the page's default is what it sets.
#[test]
fn shapes_selected_and_no_connector_leave_the_menu_on_the_page() {
    let mut w = design_window();
    let a = drop_shape(&mut w, "rectangle", 2.0, 5.0);
    w.state_mut().select(&[a]);
    w.rest();
    press(&mut w, "ribbon.Design.ConnectorsGallery");
    assert_eq!(toggled(&w, "Right-Angle"), Some(Toggled::True), "no connector is selected: the page's default is shown");
    click_on(&mut w, "menu.ConnectorStraightLines");
    assert_eq!(default_style(&w), StyleChoice::Straight);
}

/// Show Line Jumps is a check box of the page: on in a new drawing, and each press turns it the other way.
#[test]
fn show_line_jumps_is_a_check_of_the_pages_jumps() {
    let mut w = design_window();
    let jumps = |w: &Window| line_jump_settings(w.state().session().document().unwrap(), page_id(w)).code;
    assert_eq!(jumps(&w), 1, "a new page jumps horizontal lines");
    press(&mut w, "ribbon.Design.ConnectorsGallery");
    assert_eq!(toggled(&w, "Show Line Jumps"), Some(Toggled::True));
    let before = revision(&w);
    click_on(&mut w, "menu.ConnectorShowLineJumps");
    assert_eq!(jumps(&w), 0, "off");
    assert_eq!(revision(&w), before + 1, "one command, one step");
    press(&mut w, "ribbon.Design.ConnectorsGallery");
    assert_eq!(toggled(&w, "Show Line Jumps"), Some(Toggled::False), "the box shows it is off");
    click_on(&mut w, "menu.ConnectorShowLineJumps");
    assert_eq!(jumps(&w), 1, "and on again");
    undo(&mut w);
    assert_eq!(jumps(&w), 0, "one undo takes one press back");
}

/// Show Line Jumps with a connector selected is still the page's: the connector's own jump cells are not touched.
#[test]
fn show_line_jumps_with_a_connector_selected_is_the_pages() {
    let mut w = design_window();
    let (a, b) = (drop_shape(&mut w, "rectangle", 2.0, 5.0), drop_shape(&mut w, "rectangle", 6.0, 3.0));
    let line = connect(&mut w, a, b);
    w.state_mut().select(&[line]);
    w.rest();
    press(&mut w, "ribbon.Design.ConnectorsGallery");
    click_on(&mut w, "menu.ConnectorShowLineJumps");
    let doc = w.state().session().document().unwrap();
    assert_eq!(line_jump_settings(doc, page_id(&w)).code, 0, "the page's code is off");
}

// ---- Themes ----------------------------------------------------------------------------------------------------------------------------------

/// The strip shows the first themes of `theme.list`, each named for the theme; a press applies it to the page in one undo step, and the tile of
/// the theme the page has is pressed.
#[test]
fn the_themes_strip_is_filled_from_theme_list_and_applies_a_theme() {
    let mut w = design_window();
    let ids = theme_ids(&mut w);
    assert!(ids.len() >= 4, "{ids:?}");
    assert_eq!(page_theme(&mut w), "none", "a new drawing has no theme");
    for id in &ids[..3] {
        assert!(is_enabled(&w, &format!("ribbon.Design.theme.{id}")), "{id} is a tile of the strip");
        let name = theme_name(&mut w, id);
        assert_ne!(toggled(&w, &name), Some(Toggled::True), "{name} is not the page's theme");
    }
    assert!(w.state().element(&format!("ribbon.Design.theme.{}", ids[3])).is_none(), "the strip holds three: the rest are in the gallery");
    let before = revision(&w);
    press(&mut w, &format!("ribbon.Design.theme.{}", ids[1]));
    w.rest();
    assert_eq!(page_theme(&mut w), ids[1]);
    assert_eq!(revision(&w), before + 1, "one command, one step");
    let name = theme_name(&mut w, &ids[1]);
    assert_eq!(toggled(&w, &name), Some(Toggled::True), "the tile of the page's theme is pressed");
    undo(&mut w);
    assert_eq!(page_theme(&mut w), "none");
}

/// The gallery's arrow opens every theme of the list, and No Theme; each applies, and No Theme takes the theme away.
#[test]
fn the_themes_gallery_lists_every_theme_and_no_theme_takes_it_away() {
    let mut w = design_window();
    let ids = theme_ids(&mut w);
    open_gallery(&mut w, "ThemesGallery");
    for id in &ids {
        assert!(is_enabled(&w, &format!("menu.theme.{id}")), "{id} is in the gallery");
    }
    assert!(is_enabled(&w, "menu.ThemeNone"), "No Theme is a row of the gallery");
    assert!(!is_enabled(&w, "menu.ThemeApplyToNewShapes"), "the engine has no setting for new shapes yet");
    click_on(&mut w, &format!("menu.theme.{}", ids[4]));
    assert_eq!(page_theme(&mut w), ids[4]);
    open_gallery(&mut w, "ThemesGallery");
    let name = theme_name(&mut w, &ids[4]);
    assert_eq!(toggled(&w, &name), Some(Toggled::True), "the page's theme is marked in the gallery");
    click_on(&mut w, "menu.ThemeNone");
    assert_eq!(page_theme(&mut w), "none", "No Theme");
    open_gallery(&mut w, "ThemesGallery");
    assert_eq!(toggled(&w, "No Theme"), Some(Toggled::True), "and it is the row marked");
}

/// A theme applies to the page on view, and not to the first page.
#[test]
fn a_theme_applies_to_the_page_on_view() {
    let mut w = design_window();
    run_command(&mut w, "page.add", json!({}));
    let pages: Vec<u32> = w.state().session().document().unwrap().pages.iter().map(|p| p.id).collect();
    assert_eq!(pages.len(), 2);
    run_command(&mut w, "page.select", json!({"id": pages[1]}));
    let ids = theme_ids(&mut w);
    press(&mut w, &format!("ribbon.Design.theme.{}", ids[0]));
    w.rest();
    let first = w.state_mut().execute("theme.list", &json!({"page": pages[0]})).unwrap();
    let second = w.state_mut().execute("theme.list", &json!({"page": pages[1]})).unwrap();
    assert_eq!(first["page"]["theme"], "none", "the first page is as it was");
    assert_eq!(second["page"]["theme"], ids[0].as_str(), "the page on view has the theme");
}

/// A right click on a theme offers the current page and all pages.
#[test]
fn a_right_click_on_a_theme_applies_it_to_all_pages() {
    let mut w = design_window();
    run_command(&mut w, "page.add", json!({}));
    let pages: Vec<u32> = w.state().session().document().unwrap().pages.iter().map(|p| p.id).collect();
    let ids = theme_ids(&mut w);
    let tile = rect_of(&w, &format!("ribbon.Design.theme.{}", ids[2])).center();
    right_click(&mut w, tile);
    assert!(is_enabled(&w, "menu.theme.apply_page") && is_enabled(&w, "menu.theme.apply_all"));
    click_on(&mut w, "menu.theme.apply_all");
    for page in pages {
        let list = w.state_mut().execute("theme.list", &json!({"page": page})).unwrap();
        assert_eq!(list["page"]["theme"], ids[2].as_str(), "page {page}");
    }
}

/// A right click on a tile of the Themes menu offers the same two rows under the tiles (a context menu cannot open inside a menu), and they go when the
/// menu closes.
#[test]
fn a_right_click_in_the_themes_menu_offers_the_current_page_and_all_pages() {
    let mut w = design_window();
    run_command(&mut w, "page.add", json!({}));
    let pages: Vec<u32> = w.state().session().document().unwrap().pages.iter().map(|p| p.id).collect();
    let ids = theme_ids(&mut w);
    open_gallery(&mut w, "ThemesGallery");
    assert!(w.state().element("menu.theme.apply_all").is_none(), "no theme is picked yet");
    let tile = rect_of(&w, &format!("menu.theme.{}", ids[4])).center();
    right_click(&mut w, tile);
    assert!(is_enabled(&w, "menu.theme.apply_page") && is_enabled(&w, "menu.theme.apply_all"));
    click_on(&mut w, "menu.theme.apply_all");
    for page in &pages {
        let list = w.state_mut().execute("theme.list", &json!({"page": page})).unwrap();
        assert_eq!(list["page"]["theme"], ids[4].as_str(), "page {page}");
    }
    open_gallery(&mut w, "ThemesGallery");
    assert!(w.state().element("menu.theme.apply_all").is_none(), "the pick was forgotten with the menu");
}

// ---- Variants --------------------------------------------------------------------------------------------------------------------------------

/// With no theme the four variant tiles are quiet; with one they apply a variant to the page, and the tile of the page's variant is pressed.
#[test]
fn the_variants_strip_needs_a_theme_and_picks_a_variant() {
    let mut w = design_window();
    for n in 0..4 {
        assert!(!is_enabled(&w, &format!("ribbon.Design.variant.{n}")), "variant {n} needs a theme");
    }
    let ids = theme_ids(&mut w);
    press(&mut w, &format!("ribbon.Design.theme.{}", ids[0]));
    w.rest();
    for n in 0..4 {
        assert!(is_enabled(&w, &format!("ribbon.Design.variant.{n}")), "variant {n}");
    }
    let names: Vec<String> = theme_list(&mut w)["variants"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_owned()).collect();
    let before = revision(&w);
    press(&mut w, "ribbon.Design.variant.2");
    w.rest();
    let page = theme_list(&mut w)["page"].clone();
    assert_eq!((page["colors"].as_u64(), page["styles"].as_u64()), (Some(2), Some(2)));
    assert_eq!(revision(&w), before + 1, "one command, one step");
    assert_eq!(toggled(&w, &names[2]), Some(Toggled::True), "the page's variant is pressed");
    assert_ne!(toggled(&w, &names[0]), Some(Toggled::True));
    undo(&mut w);
    assert_eq!(theme_list(&mut w)["page"]["colors"].as_u64(), Some(0));
}

/// The Colors menu of the Variants gallery sets the colours' variant alone; the menus of the other three have no engine behind them yet.
#[test]
fn the_colors_menu_sets_the_colours_variant_alone() {
    let mut w = design_window();
    assert!(!is_enabled(&w, "ribbon.Design.ThemeVariantsGallery"), "Variants needs a theme");
    let ids = theme_ids(&mut w);
    press(&mut w, &format!("ribbon.Design.theme.{}", ids[0]));
    w.rest();
    press(&mut w, "ribbon.Design.variant.3");
    w.rest();
    open_gallery(&mut w, "ThemeVariantsGallery");
    assert!(is_enabled(&w, "menu.ThemeColorsGallery"));
    for row in ["ThemeEffectsGallery", "ThemeConnectorsGallery", "EmbellishmentGallery"] {
        assert!(!is_enabled(&w, &format!("menu.{row}")), "{row} has no engine behind it yet");
    }
    click_on(&mut w, "menu.ThemeColorsGallery");
    for n in 0..4 {
        assert!(is_enabled(&w, &format!("menu.variant.colors.{n}")), "colour variant {n}");
    }
    assert!(!is_enabled(&w, "menu.ThemeColorsCreateNew"), "custom colours have no engine behind them yet");
    let before = revision(&w);
    click_on(&mut w, "menu.variant.colors.1");
    let page = theme_list(&mut w)["page"].clone();
    assert_eq!((page["colors"].as_u64(), page["styles"].as_u64()), (Some(1), Some(3)), "the colours moved and the styles did not");
    assert_eq!(revision(&w), before + 1);
}
