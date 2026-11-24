//! The colour palette, the Colors dialog and the live theme preview: the Fill, Line and Font Color menus with their Theme Colors, Variant Colors, Standard Colors and
//! Recent Colors, More Colors and the dialog it opens, and the page showing a theme while its tile is under the pointer. The pointer is given to egui as a person's would be, and what
//! is checked is the document the engine's commands changed. The rows are.

use std::sync::{Arc, Mutex};

use diacraft_engine::Session;
use diacraft_engine::formatting::{Rgb, ShapeLook, look_of};
use diacraft_render::{Rendered, Renderer};
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::render_worker::{Drawer, Job, MakeDrawer, bundled_drawer};
use egui::{Key, Modifiers};
use serde_json::{Value, json};

use crate::editing::{click_at, edit_window, key, three_shapes, type_text};
use crate::support::{Rest, Window, click, elements_named, fast_window, move_to, run_command};

const WIDE: [f32; 2] = [1280.0, 900.0];

fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

fn look(w: &Window, id: u32) -> ShapeLook {
    look_of(w.state().session().document().unwrap(), page_id(w), &[id]).unwrap().remove(0)
}

fn hit(w: &mut Window, name: &str) {
    let at = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    click(w, at);
    w.rest();
}

fn colors(w: &mut Window) -> Value {
    let page = page_id(w);
    w.state_mut().execute("theme.colors", &json!({"page": page})).unwrap()
}

/// The swatch of the page's palette whose `pick` word is `pick`: its colour.
fn palette_colour(w: &mut Window, pick: &str) -> Rgb {
    let p = colors(w);
    let all = p["theme_colors"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|c| c["swatches"].as_array().unwrap().iter())
        .chain(p["variant_colors"].as_array().unwrap().iter());
    let hex = all.clone().find(|s| s["pick"] == pick).unwrap_or_else(|| panic!("no swatch {pick}"))["hex"].as_str().unwrap().to_owned();
    Rgb::parse(&hex).unwrap()
}

/// A window on a new drawing with three shapes on it, the last selected, and `theme` applied to the page.
fn themed(theme: &str) -> (Window, Vec<u32>) {
    let mut w = edit_window(WIDE);
    let ids = three_shapes(&mut w);
    run_command(&mut w, "theme.apply", json!({"theme": theme}));
    w.rest();
    (w, ids)
}

fn open_fill_menu(w: &mut Window) {
    hit(w, "ribbon.Home.FillColorGallery");
}

// ---- the sections of the palette --------------------------------------------------------------------------------------------------------------

#[test]
fn the_fill_menu_has_use_default_theme_colors_variant_colors_standard_colors_recent_colors_no_fill_and_more_colors_in_that_order() {
    let (mut w, _) = themed("tidepool");
    open_fill_menu(&mut w);
    let y_of = |w: &Window, name: &str| w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center().y;
    let theme_swatches = elements_named(&w, "palette.fill.theme:");
    // Ten columns of six: the colour and its five shades (the third and fourth repeat the first two); and seven variant colours (named by the variant).
    let (variants, themes): (Vec<_>, Vec<_>) = theme_swatches.iter().partition(|(name, ..)| name.contains("variant"));
    assert_eq!(themes.len(), 10 * 6, "Theme Colors");
    assert_eq!(variants.len(), 7, "Variant Colors");
    assert_eq!(elements_named(&w, "palette.fill.standard.#").len(), 10, "Standard Colors");
    assert!(elements_named(&w, "palette.fill.recent.#").is_empty(), "no Recent Colors yet");
    let order = [
        y_of(&w, "palette.fill.default"),
        y_of(&w, "palette.fill.theme:accent2"),
        y_of(&w, "palette.fill.theme:variant1"),
        y_of(&w, "palette.fill.standard.#e53935"),
        y_of(&w, "palette.fill.none"),
        y_of(&w, "palette.fill.more"),
    ];
    assert!(order.windows(2).all(|p| p[0] < p[1]), "top to bottom: {order:?}");
    // The shades of a column are under its colour, in rows.
    let base = w.state().element("palette.fill.theme:accent2").unwrap().rect;
    let lighter = w.state().element("palette.fill.theme:accent2:80").unwrap().rect;
    let darker = w.state().element("palette.fill.theme:accent2:-50").unwrap().rect;
    assert!(base.min.y < lighter.min.y && lighter.min.y < darker.min.y && (base.center().x - darker.center().x).abs() < 1.0);
    // The Line menu has the same sections with its own words; the Font Color menu has no No-colour row.
    key(&mut w, Key::Escape, Modifiers::NONE);
    hit(&mut w, "ribbon.Home.LineColorGallery");
    assert!(w.state().element("palette.line.none").is_some() && w.state().element("palette.line.theme:accent1").is_some());
    key(&mut w, Key::Escape, Modifiers::NONE);
    hit(&mut w, "ribbon.Home.FontColorGallery");
    assert!(w.state().element("palette.color.none").is_none(), "a text has a colour");
    assert!(w.state().element("palette.color.theme:dark").is_some() && w.state().element("palette.color.more").is_some());
}

#[test]
fn the_theme_swatches_show_the_colours_of_the_pages_theme_and_no_theme_shows_the_table_of_themeval() {
    let (mut w, _) = themed("tidepool");
    open_fill_menu(&mut w);
    assert!(w.state().element("palette.fill.theme:accent1").is_some());
    key(&mut w, Key::Escape, Modifiers::NONE);
    // The palette is what the engine answers, and a drawing with no theme answers the no-theme table.
    run_command(&mut w, "theme.apply", json!({"theme": "none"}));
    assert_eq!(palette_colour(&mut w, "theme:accent2"), Rgb(0x9d, 0xbb, 0x61));
    open_fill_menu(&mut w);
    assert_eq!(
        elements_named(&w, "palette.fill.theme:").len(),
        10 * 6 + 7,
        "the sections are there with no theme: a pick follows the theme applied later"
    );
}

#[test]
fn the_theme_colors_are_ten_columns_across_the_menu_the_first_four_white_black_white_and_black_in_every_theme() {
    for theme in ["tidepool", "cinderglow"] {
        let (mut w, _) = themed(theme);
        open_fill_menu(&mut w);
        // The top row of each column, left to right: Light, Dark, Light again, Dark again, then the six accents.
        let top: Vec<(String, egui::Rect)> = ["theme:light", "theme:dark", "theme:light@3", "theme:dark@4"]
            .into_iter()
            .chain(["theme:accent1", "theme:accent2", "theme:accent3", "theme:accent4", "theme:accent5", "theme:accent6"])
            .map(|pick| (pick.to_owned(), w.state().element(&format!("palette.fill.{pick}")).unwrap_or_else(|| panic!("{theme}: no {pick}")).rect))
            .collect();
        let xs: Vec<f32> = top.iter().map(|(_, r)| r.center().x).collect();
        assert!(xs.windows(2).all(|p| p[0] < p[1]), "{theme}: ten columns left to right: {xs:?}");
        assert!(top.windows(2).all(|p| p[0].1.max.x <= p[1].1.min.x + 0.01), "{theme}: no two swatches overlap: {top:?}");
        assert!(top.iter().all(|(_, r)| (r.center().y - top[0].1.center().y).abs() < 0.5), "{theme}: one top row");
        // The first four are white, black, white and black whatever the theme's accents are.
        let p = colors(&mut w);
        let first_four: Vec<&str> = p["theme_colors"].as_array().unwrap()[..4].iter().map(|c| c["swatches"][0]["hex"].as_str().unwrap()).collect();
        assert_eq!(first_four[0], first_four[2], "{theme}");
        assert_eq!(first_four[1], first_four[3], "{theme}");
        assert_ne!(first_four[0], first_four[1], "{theme}");
        // The palette is as wide as the rows of fixed colours under it, ten swatches, so the ten columns fit the menu.
        let standard = elements_named(&w, "palette.fill.standard.#");
        let (left, right) =
            (standard.iter().map(|(_, r, _)| r.min.x).fold(f32::MAX, f32::min), standard.iter().map(|(_, r, _)| r.max.x).fold(f32::MIN, f32::max));
        assert!(top[0].1.min.x >= left - 4.0 && top[9].1.max.x <= right + 4.0, "{theme}: the columns lie within the menu: {left} to {right}");
        key(&mut w, Key::Escape, Modifiers::NONE);
    }
}

#[test]
fn a_swatch_of_the_third_or_fourth_column_writes_what_the_first_or_second_does() {
    let (mut w, ids) = themed("tidepool");
    open_fill_menu(&mut w);
    hit(&mut w, "palette.fill.theme:light:-25@3");
    let formula = |w: &Window, cell: &str| {
        let d = w.state().session().document().unwrap();
        let shape = d.shape(&d.find_shape(diacraft_doc::Owner::Page(page_id(w)), ids[2]).unwrap()).unwrap();
        match &shape.sheet.cell(cell).unwrap().formula {
            diacraft_doc::FormulaSlot::Local(f) => Some(f.source.to_string()),
            _ => None,
        }
    };
    assert_eq!(formula(&w, "FillForegnd").as_deref(), Some(r#"THEMEGUARD(MSOTINT(THEMEVAL("Light"),-25))"#));
    open_fill_menu(&mut w);
    hit(&mut w, "palette.fill.theme:dark:35@4");
    assert_eq!(formula(&w, "FillForegnd").as_deref(), Some(r#"THEMEGUARD(MSOTINT(THEMEVAL("Dark"),35))"#));
    // The same swatch of the first column writes the same cell.
    open_fill_menu(&mut w);
    hit(&mut w, "palette.fill.theme:dark:35");
    assert_eq!(formula(&w, "FillForegnd").as_deref(), Some(r#"THEMEGUARD(MSOTINT(THEMEVAL("Dark"),35))"#));
}

// ---- a pick -----------------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_theme_swatch_fills_the_shape_in_one_step_and_the_fill_follows_the_theme_when_it_changes() {
    let (mut w, ids) = themed("tidepool");
    let revision = w.state().session().revision();
    open_fill_menu(&mut w);
    hit(&mut w, "palette.fill.theme:accent3:60");
    assert_eq!(w.state().session().revision(), revision + 1, "one command");
    assert!(w.state().element("palette.fill.none").is_none(), "the menu closed with the choice");
    let want = palette_colour(&mut w, "theme:accent3:60");
    assert_eq!(look(&w, ids[2]).fill, Some(want));
    // Another theme: the fill follows. The other shapes are untouched by the pick, and a theme applied changes them as it always did.
    run_command(&mut w, "theme.apply", json!({"theme": "cinderglow"}));
    let want_now = palette_colour(&mut w, "theme:accent3:60");
    assert_ne!(want_now, want);
    assert_eq!(look(&w, ids[2]).fill, Some(want_now), "the pick follows the theme");
    // The control shows the colour the shape has.
    assert_eq!(w.state().fill_mix(), diacraft_ui_egui::format::Mix::One(Some(want_now)));
}

#[test]
fn a_standard_swatch_is_a_fixed_colour_that_no_theme_changes() {
    let (mut w, ids) = themed("tidepool");
    open_fill_menu(&mut w);
    hit(&mut w, "palette.fill.standard.#e53935");
    assert_eq!(look(&w, ids[2]).fill, Some(Rgb(0xe5, 0x39, 0x35)));
    run_command(&mut w, "theme.apply", json!({"theme": "plumline"}));
    assert_eq!(look(&w, ids[2]).fill, Some(Rgb(0xe5, 0x39, 0x35)), "");
    assert!(w.state().recent_colours().is_empty(), "the Standard Colors are always on the menu: they are no Recent Colors");
}

#[test]
fn variant_colors_follow_the_pages_colour_variant() {
    let (mut w, ids) = themed("tidepool");
    open_fill_menu(&mut w);
    hit(&mut w, "palette.fill.theme:variant3");
    let first = palette_colour(&mut w, "theme:variant3");
    assert_eq!(look(&w, ids[2]).fill, Some(first));
    run_command(&mut w, "theme.variant", json!({"colors": 2}));
    let second = palette_colour(&mut w, "theme:variant3");
    assert_ne!(first, second);
    assert_eq!(look(&w, ids[2]).fill, Some(second));
}

#[test]
fn use_default_hands_the_fill_the_line_and_the_text_to_the_theme() {
    let (mut w, ids) = themed("tidepool");
    let themed_fill = look(&w, ids[2]).fill;
    open_fill_menu(&mut w);
    hit(&mut w, "palette.fill.standard.#2e9e4f");
    assert_eq!(look(&w, ids[2]).fill, Some(Rgb(0x2e, 0x9e, 0x4f)));
    open_fill_menu(&mut w);
    hit(&mut w, "palette.fill.default");
    assert_eq!(look(&w, ids[2]).fill, themed_fill);
    // The line and the text colour have the row too.
    hit(&mut w, "ribbon.Home.LineColorGallery");
    hit(&mut w, "palette.line.standard.#e53935");
    assert_eq!(look(&w, ids[2]).line, Some(Rgb(0xe5, 0x39, 0x35)));
    hit(&mut w, "ribbon.Home.LineColorGallery");
    hit(&mut w, "palette.line.default");
    assert_ne!(look(&w, ids[2]).line, Some(Rgb(0xe5, 0x39, 0x35)));
}

#[test]
fn a_theme_swatch_of_the_font_color_menu_colours_the_text_and_follows_the_theme() {
    let (mut w, ids) = themed("tidepool");
    let page = page_id(&w);
    run_command(&mut w, "shape.set_text", json!({"id": ids[2], "text": "Hello", "page": page}));
    hit(&mut w, "ribbon.Home.FontColorGallery");
    hit(&mut w, "palette.color.theme:accent4");
    let want = palette_colour(&mut w, "theme:accent4");
    assert_eq!(look(&w, ids[2]).color, want);
    run_command(&mut w, "theme.apply", json!({"theme": "plumline"}));
    let now = palette_colour(&mut w, "theme:accent4");
    assert_ne!(now, want);
    assert_eq!(look(&w, ids[2]).color, now);
}

#[test]
fn a_menu_for_a_colour_that_no_theme_answers_has_only_the_fixed_colours() {
    // The gradient stop, the ground of a pattern and the shadow take a fixed colour: their palettes have Standard Colors, Recent Colors and More Colors.
    let (mut w, ids) = themed("tidepool");
    let _ = ids;
    run_command(&mut w, "format.gradient", json!({"kind": "linear", "angle": 90}));
    run_command(&mut w, "window.pane", json!({"pane": "format_shape", "shown": true}));
    w.rest();
    let swatch = w.state().element("pane.format.fill.gradient.stop.colour");
    if let Some(swatch) = swatch {
        let at = swatch.rect.center();
        click(&mut w, at);
        w.rest();
        assert!(elements_named(&w, "palette.stop_color.theme:").is_empty(), "no theme colours for a gradient stop");
        assert_eq!(elements_named(&w, "palette.stop_color.standard.#").len(), 10);
        assert!(w.state().element("palette.stop_color.more").is_some());
    }
}

// ---- More Colors ------------------------------------------------------------------------------------------------------------------------------

#[test]
fn more_colors_opens_the_colors_dialog_on_the_current_colour_and_ok_applies_a_fixed_colour_and_remembers_it() {
    let (mut w, ids) = themed("tidepool");
    let current = look(&w, ids[2]).fill.unwrap();
    open_fill_menu(&mut w);
    hit(&mut w, "palette.fill.more");
    let state = w.state().window_state()["colorsDialog"].clone();
    assert_eq!((state["target"].as_str(), state["tab"].as_str()), (Some("fill"), Some("standard")));
    assert_eq!(state["current"], current.hex());
    assert_eq!(state["new"], current.hex(), "the new colour is the current one until something is picked");
    assert!(w.state().element("colors.new").is_some() && w.state().element("colors.current").is_some());
    // A cell of the Standard tab is picked: the new colour changes and the shape does not, until OK.
    let revision = w.state().session().revision();
    hit(&mut w, "colors.standard.#ffffff");
    assert_eq!(w.state().window_state()["colorsDialog"]["new"], "#ffffff");
    assert_eq!(w.state().session().revision(), revision, "nothing is sent until OK");
    hit(&mut w, "colors.ok");
    assert!(w.state().window_state()["colorsDialog"].is_null(), "OK closes the dialog");
    assert_eq!(look(&w, ids[2]).fill, Some(Rgb(255, 255, 255)));
    assert_eq!(w.state().session().revision(), revision + 1);
    // It is a fixed colour (no theme changes it), and the newest of the Recent Colors.
    run_command(&mut w, "theme.apply", json!({"theme": "cinderglow"}));
    assert_eq!(look(&w, ids[2]).fill, Some(Rgb(255, 255, 255)));
    assert_eq!(w.state().recent_colours(), &[Rgb(255, 255, 255)]);
    open_fill_menu(&mut w);
    assert_eq!(elements_named(&w, "palette.fill.recent.#").len(), 1);
    // A recent colour is picked from the menu and moves to the front again.
    key(&mut w, Key::Escape, Modifiers::NONE);
    hit(&mut w, "ribbon.Home.LineColorGallery");
    hit(&mut w, "palette.line.recent.#ffffff");
    assert_eq!(look(&w, ids[2]).line, Some(Rgb(255, 255, 255)));
}

#[test]
fn cancel_and_escape_change_nothing_and_a_colour_that_is_no_colour_is_refused_in_words() {
    let (mut w, ids) = themed("tidepool");
    let before = look(&w, ids[2]).fill;
    let revision = w.state().session().revision();
    open_fill_menu(&mut w);
    hit(&mut w, "palette.fill.more");
    hit(&mut w, "colors.standard.#000000");
    hit(&mut w, "colors.cancel");
    assert!(w.state().window_state()["colorsDialog"].is_null());
    open_fill_menu(&mut w);
    hit(&mut w, "palette.fill.more");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(w.state().window_state()["colorsDialog"].is_null(), "Escape cancels");
    assert_eq!((look(&w, ids[2]).fill, w.state().session().revision()), (before, revision));
    assert!(w.state().recent_colours().is_empty());
    // The Hex box takes #RRGGBB, three or six digits are not guessed at, and what is not a colour stays in the dialog with its words.
    open_fill_menu(&mut w);
    hit(&mut w, "palette.fill.more");
    hit(&mut w, "colors.tab.custom");
    hit(&mut w, "colors.hex");
    key(&mut w, Key::A, Modifiers::COMMAND);
    type_text(&mut w, "blue");
    key(&mut w, Key::Enter, Modifiers::NONE);
    let said = w.state().window_state()["colorsDialog"]["error"].as_str().unwrap_or_default().to_owned();
    assert!(said.contains("#RRGGBB"), "{said:?}");
    assert!(!w.state().window_state()["colorsDialog"].is_null(), "the dialog stays");
    hit(&mut w, "colors.hex");
    key(&mut w, Key::A, Modifiers::COMMAND);
    type_text(&mut w, "#3C78D8");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert_eq!(w.state().window_state()["colorsDialog"]["new"], "#3c78d8");
    assert!(w.state().window_state()["colorsDialog"]["error"].is_null(), "a colour clears the words");
    hit(&mut w, "colors.ok");
    assert_eq!(look(&w, ids[2]).fill, Some(Rgb(0x3c, 0x78, 0xd8)));
}

#[test]
fn the_custom_tab_picks_by_the_numbers_of_either_model_and_by_the_colour_field_and_the_lightness_bar() {
    let (mut w, _) = themed("tidepool");
    open_fill_menu(&mut w);
    hit(&mut w, "palette.fill.more");
    hit(&mut w, "colors.tab.custom");
    let new = |w: &Window| w.state().window_state()["colorsDialog"]["new"].as_str().unwrap().to_owned();
    // The field: a press at the left and the top is red at full saturation; the bar's top is lighter than its bottom.
    let field = w.state().element("colors.field").unwrap().rect;
    click(&mut w, egui::pos2(field.left() + 1.0, field.top() + 1.0));
    w.rest();
    let red = Rgb::parse(&new(&w)).unwrap();
    assert!(red.0 > red.1 && red.0 > red.2, "red: {red:?}");
    let bar = w.state().element("colors.bar").unwrap().rect;
    click(&mut w, egui::pos2(bar.center().x, bar.top() + 2.0));
    w.rest();
    let light = Rgb::parse(&new(&w)).unwrap();
    click(&mut w, egui::pos2(bar.center().x, bar.bottom() - 2.0));
    w.rest();
    let dark = Rgb::parse(&new(&w)).unwrap();
    assert!(
        u32::from(light.0) + u32::from(light.1) + u32::from(light.2) > u32::from(dark.0) + u32::from(dark.1) + u32::from(dark.2),
        "{light:?} against {dark:?}"
    );
    // The model: RGB has Red, Green and Blue, HSL has Hue, Saturation and Lightness.
    assert!(w.state().element("colors.red").is_some() && w.state().element("colors.hue").is_none());
    hit(&mut w, "colors.model.hsl");
    assert!(
        w.state().element("colors.hue").is_some()
            && w.state().element("colors.saturation").is_some()
            && w.state().element("colors.lightness").is_some()
    );
    assert!(w.state().element("colors.red").is_none());
}

#[test]
fn the_font_color_dialog_colours_the_characters_that_were_selected_in_the_open_text_box() {
    let (mut w, ids) = themed("tidepool");
    let page = page_id(&w);
    run_command(&mut w, "shape.set_text", json!({"id": ids[2], "text": "Hello world", "page": page}));
    // A text box over the shape (F2), with the first five characters selected.
    click_at(&mut w, 5.0, 3.0);
    key(&mut w, Key::F2, Modifiers::NONE);
    key(&mut w, Key::Home, Modifiers::NONE);
    for _ in 0..5 {
        key(&mut w, Key::ArrowRight, Modifiers::SHIFT);
    }
    assert_eq!(w.state().edited_text_selection(), Some((0, 5)));
    hit(&mut w, "ribbon.Home.FontColorGallery");
    hit(&mut w, "palette.color.more");
    // A cell of the Standard tab, found by its place in the grid.
    let mut cells = elements_named(&w, "colors.standard.#");
    cells.sort_by(|a, b| a.1.min.y.total_cmp(&b.1.min.y).then(a.1.min.x.total_cmp(&b.1.min.x)));
    let (name, rect, _) = cells[30].clone();
    let picked = name.rsplit('.').next().unwrap().to_owned();
    click(&mut w, rect.center());
    w.rest();
    hit(&mut w, "colors.ok");
    let range = w.state_mut().execute("shape.formatting", &json!({"ids": [ids[2]], "page": page, "start": 0, "end": 5})).unwrap();
    assert_eq!(range["shapes"][0]["color"], picked, "{range}");
    let rest = w.state_mut().execute("shape.formatting", &json!({"ids": [ids[2]], "page": page, "start": 6, "end": 11})).unwrap();
    assert_ne!(rest["shapes"][0]["color"], picked, "only the characters that were selected: {rest}");
}

// ---- the live preview -------------------------------------------------------------------------------------------------------------------------

/// The last picture a drawer made, to read its pixels.
type Shown = Arc<Mutex<Option<(u32, [f64; 6], Rendered)>>>;

struct Recording {
    renderer: Renderer,
    last: Shown,
    drawn: Arc<Mutex<u32>>,
}

impl Drawer for Recording {
    fn draw(&mut self, job: &Job) -> Rendered {
        self.renderer.set_limits(job.limits);
        self.renderer.set_paper(None);
        let rendered = self.renderer.render(&job.doc, job.page, &job.view);
        *self.last.lock().unwrap() = Some((job.page, job.view.to_px.as_coeffs(), rendered.clone()));
        *self.drawn.lock().unwrap() += 1;
        rendered
    }
}

fn recording() -> (MakeDrawer, Shown, Arc<Mutex<u32>>) {
    let (last, drawn) = (Arc::new(Mutex::new(None)), Arc::new(Mutex::new(0)));
    let (kept, count) = (Arc::clone(&last), Arc::clone(&drawn));
    (Box::new(move || Box::new(Recording { renderer: Renderer::new(), last: kept, drawn: count })), last, drawn)
}

fn colour_at(last: &Shown, x: f64, y: f64) -> [u8; 3] {
    let guard = last.lock().unwrap();
    let (_, c, r) = guard.as_ref().expect("a picture was drawn");
    let (px, py) = (c[0] * x + c[2] * y + c[4], c[1] * x + c[3] * y + c[5]);
    let (col, row) = ((px.max(0.0) as usize).min(r.width as usize - 1), (py.max(0.0) as usize).min(r.height as usize - 1));
    let i = (row * r.width as usize + col) * 4;
    [r.rgba_premul[i], r.rgba_premul[i + 1], r.rgba_premul[i + 2]]
}

/// A window on the Design tab, a new drawing with one rectangle at (3, 4) on it, and the recorders of the page's picture and of the preview's.
fn design_window() -> (Window, Shown, Shown, u32) {
    let (page_make, page_last, _) = recording();
    let (preview_make, preview_last, _) = recording();
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let app = DiacraftApp::new(session, None).with_inline_pictures(page_make).with_inline_theme_preview(preview_make);
    let mut w = fast_window(app, [1600.0, 900.0]);
    w.rest();
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    let page = page_id(&w);
    let id = w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": page, "x": 3.0, "y": 4.0})).unwrap()["id"].as_u64().unwrap()
        as u32;
    run_command(&mut w, "window.tab", json!({"tab": "Design"}));
    w.rest();
    (w, page_last, preview_last, id)
}

fn tile_centre(w: &Window, name: &str) -> egui::Pos2 {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center()
}

#[test]
fn a_theme_tile_under_the_pointer_shows_the_theme_on_the_page_and_leaving_shows_the_page_as_it_was() {
    let (mut w, page_last, preview_last, _) = design_window();
    let themes = w.state_mut().execute("theme.list", &json!({})).unwrap();
    let id = themes["themes"][0]["id"].as_str().unwrap().to_owned();
    let (revision, dirty) = (w.state().session().revision(), w.state().session().is_dirty());
    let before = colour_at(&page_last, 3.0, 4.0);
    assert_eq!(before, [255, 255, 255], "white with no theme");
    // The pointer over the tile.
    let at = tile_centre(&w, &format!("ribbon.Design.theme.{id}"));
    for _ in 0..4 {
        move_to(&mut w, at);
    }
    w.rest();
    assert!(w.state().canvas_report().preview, "the page shows the preview");
    assert_ne!(colour_at(&preview_last, 3.0, 4.0), before, "the theme's fill");
    // Not an edit: no revision, no unsaved mark, and the page's own picture is as it was.
    assert_eq!((w.state().session().revision(), w.state().session().is_dirty()), (revision, dirty));
    assert_eq!(colour_at(&page_last, 3.0, 4.0), before);
    let theme = w.state_mut().execute("theme.list", &json!({})).unwrap();
    assert_eq!(theme["page"]["theme"], "none", "the drawing has no theme");
    // The pointer leaves: the page is its own again.
    move_to(&mut w, egui::pos2(5.0, 5.0));
    move_to(&mut w, egui::pos2(6.0, 6.0));
    w.rest();
    assert!(!w.state().canvas_report().preview, "the preview is gone");
    assert!(w.state().window_state()["themePreview"]["showing"].is_null());
    // A press on the tile applies it for real.
    let at = tile_centre(&w, &format!("ribbon.Design.theme.{id}"));
    click(&mut w, at);
    w.rest();
    assert_eq!(w.state().session().revision(), revision + 1);
    assert!(!w.state().canvas_report().preview);
}

#[test]
fn a_preview_leaves_the_undo_and_redo_stacks_and_the_unsaved_mark_alone() {
    let (mut w, _, preview_last, id) = design_window();
    // Something to redo: a fill, undone.
    w.state_mut().execute("shape.format", &json!({"ids": [id], "fill": "#00ff00"})).unwrap();
    w.state_mut().execute("edit.undo", &json!({})).unwrap();
    w.rest();
    let history = |w: &Window| {
        let h = w.state().session().history().unwrap();
        (h.undo_len(), h.redo_len())
    };
    let (stacks, revision, dirty) = (history(&w), w.state().session().revision(), w.state().session().is_dirty());
    assert_eq!(stacks.1, 1);
    let themes = w.state_mut().execute("theme.list", &json!({})).unwrap();
    for t in themes["themes"].as_array().unwrap().iter().take(3) {
        let at = tile_centre(&w, &format!("ribbon.Design.theme.{}", t["id"].as_str().unwrap()));
        for _ in 0..3 {
            move_to(&mut w, at);
        }
        w.rest();
        assert!(w.state().canvas_report().preview, "{}", t["id"]);
        assert_eq!((history(&w), w.state().session().revision(), w.state().session().is_dirty()), (stacks, revision, dirty), "{}", t["id"]);
    }
    let _ = preview_last;
    // The redo is still there.
    move_to(&mut w, egui::pos2(5.0, 5.0));
    w.rest();
    w.state_mut().execute("edit.redo", &json!({})).unwrap();
    assert_eq!(look(&w, id).fill, Some(Rgb(0, 255, 0)));
}

#[test]
fn moving_to_another_tile_shows_the_next_theme_and_each_tile_is_one_copy() {
    let (mut w, _, preview_last, _) = design_window();
    let themes = w.state_mut().execute("theme.list", &json!({})).unwrap();
    let ids: Vec<String> = themes["themes"].as_array().unwrap().iter().take(2).map(|t| t["id"].as_str().unwrap().to_owned()).collect();
    let mut seen = Vec::new();
    for id in &ids {
        let at = tile_centre(&w, &format!("ribbon.Design.theme.{id}"));
        for _ in 0..3 {
            move_to(&mut w, at);
        }
        w.rest();
        seen.push(colour_at(&preview_last, 3.0, 4.0));
    }
    assert_ne!(seen[0], seen[1], "each tile shows its own theme");
    // Staying on a tile makes no more copies.
    let made = w.state().window_state()["themePreview"]["made"].as_u64().unwrap();
    for _ in 0..5 {
        w.step();
    }
    assert_eq!(w.state().window_state()["themePreview"]["made"].as_u64().unwrap(), made);
}

#[test]
fn a_variant_tile_and_a_colors_row_preview_the_variant_of_the_pages_theme() {
    let (mut w, _, preview_last, id) = design_window();
    // With no theme the variants are quiet and show nothing.
    let at = tile_centre(&w, "ribbon.Design.variant.1");
    for _ in 0..3 {
        move_to(&mut w, at);
    }
    w.rest();
    assert!(!w.state().canvas_report().preview, "no theme, no variants to preview");
    w.state_mut().execute("theme.apply", &json!({"theme": "tidepool"})).unwrap();
    w.rest();
    let before = look(&w, id).fill;
    let revision = w.state().session().revision();
    let at = tile_centre(&w, "ribbon.Design.variant.1");
    for _ in 0..3 {
        move_to(&mut w, at);
    }
    w.rest();
    assert!(w.state().canvas_report().preview);
    let shown = colour_at(&preview_last, 3.0, 4.0);
    assert_ne!(Some(Rgb(shown[0], shown[1], shown[2])), before, "variant 1 colours the shape another way");
    assert_eq!(w.state().session().revision(), revision);
    assert_eq!(look(&w, id).fill, before, "the drawing is as it was");
}

#[test]
fn a_page_past_the_budget_has_no_preview_and_says_why_once() {
    // A page of more shapes than a preview is made for, put in the drawing as an edit (a command for each would take long).
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 4.0})).unwrap();
    s.edit("Test setup", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let mut shapes: Vec<Arc<diacraft_engine::doc::Shape>> = page.shapes.iter().cloned().collect();
        let model = Arc::clone(&shapes[0]);
        for n in 0..diacraft_engine::theme::MAX_PREVIEW_SHAPES as u32 {
            let mut shape = (*model).clone();
            shape.id = 1000 + n;
            shapes.push(Arc::new(shape));
        }
        page.shapes = diacraft_engine::doc::ShapeList::from_vec(shapes);
        Ok(())
    })
    .unwrap();
    let app = DiacraftApp::new(s, None).with_inline_pictures(bundled_drawer());
    let mut w = fast_window(app, [1600.0, 900.0]);
    w.rest();
    run_command(&mut w, "window.tab", json!({"tab": "Design"}));
    w.rest();
    let themes = w.state_mut().execute("theme.list", &json!({})).unwrap();
    let id = themes["themes"][0]["id"].as_str().unwrap().to_owned();
    let at = tile_centre(&w, &format!("ribbon.Design.theme.{id}"));
    for _ in 0..4 {
        move_to(&mut w, at);
    }
    w.rest();
    assert!(!w.state().canvas_report().preview);
    let words = w.state().window_state()["themePreview"]["refused"].as_str().unwrap_or_default().to_owned();
    assert!(words.contains("more than") && words.contains("apply the theme to see it"), "{words:?}");
    assert_eq!(w.state().window_state()["themePreview"]["made"], 0);
}
