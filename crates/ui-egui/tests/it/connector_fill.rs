//! In the window: the fill controls work on a connector, which is a 1-D shape. The inside of a hollow line end takes the shape's fill (the owner's ruling of
//! 2026-11-26), so a person must be able to colour it from Home > Fill, from the Fill section of the Format Shape pane and from the transparency, pattern and gradient items, exactly
//! as for a box. Each control is driven by the pointer as a person would, and what is checked is the document the commands changed and the picture the renderer makes of it.

use diacraft_render::{Renderer, View};
use serde_json::{Value, json};

use crate::editing::edit_window;
use crate::support::{Rest, Window, click, elements_named, run_command};

const WIDE: [f32; 2] = [1280.0, 900.0];

pub fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

fn is_enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

/// A click on the control called `name` in the last frame's list, then the window comes to rest.
pub fn hit(w: &mut Window, name: &str) {
    let at = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    click(w, at);
    w.rest();
}

/// What `shape.formatting` says of shape `id` now.
pub fn look(w: &mut Window, id: u32) -> Value {
    let page = page_id(w);
    w.state_mut().execute("shape.formatting", &json!({"ids": [id], "page": page})).unwrap()["shapes"][0].clone()
}

/// Two boxes joined by a connector (the first shape of the pair is dropped at (2, 4), the second at (7, 4)); the connector is selected, has a hollow square at its end, and the Format
/// Shape pane and the Home tab are on view. → the window and the connector's `ID`.
fn connector_window() -> (Window, u32) {
    let mut w = edit_window(WIDE);
    let page = page_id(&w);
    let mut ids = Vec::new();
    for x in [2.0, 7.0] {
        let made = w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": page, "x": x, "y": 4.0})).unwrap();
        ids.push(u32::try_from(made["id"].as_u64().unwrap()).unwrap());
    }
    let made = w.state_mut().execute("connector.add", &json!({"from": {"id": ids[0]}, "to": {"id": ids[1]}, "page": page})).unwrap();
    let connector = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    w.state_mut().execute("shape.format", &json!({"ids": [connector], "end_arrow": 21, "end_arrow_size": 6, "weight": 1.5, "page": page})).unwrap();
    w.state_mut().select(&[connector]);
    run_command(&mut w, "window.tab", json!({"tab": "Home"}));
    run_command(&mut w, "window.pane", json!({"pane": "format_shape", "shown": true}));
    w.rest();
    (w, connector)
}

/// The `n`th standard swatch of the fill palette (in reading order), chosen: its colour as `#RRGGBB`.
pub fn pick_swatch(w: &mut Window, n: usize) -> String {
    let mut swatches = elements_named(w, "palette.fill.standard.#");
    swatches.sort_by(|a, b| a.1.min.y.total_cmp(&b.1.min.y).then(a.1.min.x.total_cmp(&b.1.min.x)));
    let (name, rect, enabled) = swatches.get(n).cloned().unwrap_or_else(|| panic!("the fill palette has no swatch {n}: {} drawn", swatches.len()));
    assert!(enabled, "{name}");
    click(w, rect.center());
    w.rest();
    name.rsplit('.').next().unwrap().to_owned()
}

/// The pixel (red, green, blue) at paper inch `(x, y)`, y up, of page 1 as the renderer draws it at 100 pixels to the inch.
pub fn pixel(w: &Window, x: f64, y: f64) -> [u8; 3] {
    let doc = w.state().session().document().unwrap();
    let view = View::fit_page(doc, 1, 100.0).unwrap();
    let r = Renderer::new().render(doc, 1, &view);
    let page_height = f64::from(view.height) / 100.0;
    let (col, row) = ((x * 100.0) as usize, ((page_height - y) * 100.0) as usize);
    let at = (row * r.width as usize + col) * 4;
    [r.rgba_premul[at], r.rgba_premul[at + 1], r.rgba_premul[at + 2]]
}

/// Where the connector's last vertex is, in paper inches: the centre of its square end.
fn end_of(w: &Window, id: u32) -> (f64, f64) {
    let doc = w.state().session().document().unwrap();
    let last = diacraft_engine::draw::connector_path(doc, page_id(w), id).unwrap().points.last().copied().unwrap();
    (last.0, last.1)
}

#[test]
fn the_fill_controls_are_enabled_for_a_connector_and_for_a_box() {
    let (mut w, _) = connector_window();
    assert!(is_enabled(&w, "ribbon.Home.FillColorGallery"), "Home > Fill acts on a connector");
    // A connector DiaCraft makes has No Fill, so the pane has no transparency control yet (it belongs to a solid fill, as it does for a box); Solid fill gives the connector a fill and the control.
    for name in ["pane.format.fill.none", "pane.format.fill.solid", "pane.format.fill.gradient", "pane.format.fill.pattern"] {
        assert!(is_enabled(&w, name), "{name} acts on a connector");
    }
    assert!(w.state().element("pane.format.fill.transparency").is_none(), "a connector with No Fill has no transparency to set");
    hit(&mut w, "pane.format.fill.solid");
    assert!(is_enabled(&w, "pane.format.fill.transparency"), "a solid fill has a transparency, on a connector too");
    // The Fill menu's own items are there too, and enabled.
    hit(&mut w, "ribbon.Home.FillColorGallery");
    for name in ["palette.fill.none", "menu.FillTransparencyMenu", "menu.FillGradientMenu", "menu.FillPatternMenu"] {
        assert!(is_enabled(&w, name), "{name} acts on a connector");
    }
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn a_swatch_of_the_home_fill_menu_colours_the_inside_of_a_connectors_hollow_end_and_no_fill_takes_it_away() {
    let (mut w, id) = connector_window();
    hit(&mut w, "ribbon.Home.FillColorGallery");
    let hex = pick_swatch(&mut w, 5);
    assert_eq!(
        look(&mut w, id)["fill"].as_str().map(str::to_lowercase),
        Some(hex.to_lowercase()),
        "the connector's fill is the swatch: {}",
        look(&mut w, id)
    );
    // The square at the end of the connector is that colour inside (the box the connector ends on is white; the swatch is not).
    let (x, y) = end_of(&w, id);
    let want =
        [u8::from_str_radix(&hex[1..3], 16).unwrap(), u8::from_str_radix(&hex[3..5], 16).unwrap(), u8::from_str_radix(&hex[5..7], 16).unwrap()];
    let got = pixel(&w, x, y);
    assert!(got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 12), "inside the hollow end: {got:?}, the swatch {want:?}");
    // No Fill: the inside shows what is behind it again.
    hit(&mut w, "ribbon.Home.FillColorGallery");
    hit(&mut w, "palette.fill.none");
    let l = look(&mut w, id);
    assert_eq!(l["fill"], json!("none"), "no fill: {l}");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn the_fill_menus_transparency_pattern_and_gradient_items_work_on_a_connector() {
    let (mut w, id) = connector_window();
    // A colour first: the transparency is the fill's, and a connector DiaCraft makes has No Fill.
    hit(&mut w, "ribbon.Home.FillColorGallery");
    pick_swatch(&mut w, 5);
    hit(&mut w, "ribbon.Home.FillColorGallery");
    hit(&mut w, "menu.FillTransparencyMenu");
    hit(&mut w, "menu.FillTransparency50");
    assert_eq!(look(&mut w, id)["fill_transparency"], json!(50.0));
    hit(&mut w, "ribbon.Home.FillColorGallery");
    hit(&mut w, "menu.FillPatternMenu");
    hit(&mut w, "menu.FillPattern3");
    assert_eq!(look(&mut w, id)["fill_pattern"], json!(3));
    hit(&mut w, "ribbon.Home.FillColorGallery");
    hit(&mut w, "menu.FillGradientMenu");
    hit(&mut w, "menu.FillGradientRight");
    let g = look(&mut w, id)["gradient"].clone();
    assert_eq!((g["kind"].as_str(), g["angle"].as_f64()), (Some("linear"), Some(0.0)), "a gradient on a connector: {g}");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn the_format_shape_panes_fill_section_works_on_a_connector() {
    let (mut w, id) = connector_window();
    hit(&mut w, "pane.format.fill.none");
    assert_eq!(look(&mut w, id)["fill"], json!("none"), "No fill: {}", look(&mut w, id));
    hit(&mut w, "pane.format.fill.solid");
    let l = look(&mut w, id);
    assert_eq!(l["fill_pattern"], json!(1), "Solid fill: {l}");
    hit(&mut w, "pane.format.fill.gradient");
    assert!(look(&mut w, id)["gradient"].is_object(), "Gradient fill: {}", look(&mut w, id));
    // The connector's own fill is white, and a pattern's marks are the fill's colour over a white ground: red first, so that they show.
    let page = page_id(&w);
    w.state_mut().execute("shape.format", &json!({"ids": [id], "fill": "#FF0000", "page": page})).unwrap();
    w.rest();
    hit(&mut w, "pane.format.fill.pattern");
    let l = look(&mut w, id);
    assert!(l["gradient"].is_null() && l["fill_pattern"] == json!(2), "Pattern fill: {l}");
    // The inside of the hollow end follows: the pattern's marks (diagonal lines, a tenth of an inch apart) show inside the square, which a hollow end with no fill leaves paper.
    let (x, y) = end_of(&w, id);
    let marks = (0..30)
        .flat_map(|k| (0..30).map(move |j| (k, j)))
        .filter(|(k, j)| pixel(&w, x - 0.19 + f64::from(*k) * 0.01, y - 0.15 + f64::from(*j) * 0.01).iter().any(|c| *c < 200))
        .count();
    assert!(marks >= 10, "the marks of the pattern show inside the end: {marks} pixels of 900");
    assert_eq!(w.state().ui_panics(), 0);
}
