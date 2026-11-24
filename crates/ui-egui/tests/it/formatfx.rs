//! In the window: the Format Shape pane's transparency, gradient, pattern, line and shadow controls, the Home tab's Fill, Line and Effects menus, the Quick
//! Styles gallery, and the Format Painter carrying them. The pointer and the keyboard are given to egui as a person's would be, and what is checked is the document the
//! commands changed (`shape.formatting`'s answer for the shape), and the elements the window drew.

use egui::{Key, Modifiers};
use serde_json::{Value, json};

use crate::editing::{drag_tile_to, edit_window, key, shape_ids, type_text};
use crate::saving::ctrl;
use crate::support::{Rest, Window, click, drag, run_command};

const WIDE: [f32; 2] = [1280.0, 900.0];

fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

fn rect_of(w: &Window, name: &str) -> egui::Rect {
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

/// Types `text` in the field `name` and ends it with Enter: one value, as a person types it.
fn type_in(w: &mut Window, name: &str, text: &str) {
    click_on(w, name);
    key(w, Key::A, ctrl());
    type_text(w, text);
    key(w, Key::Enter, Modifiers::NONE);
}

fn shown_in(w: &Window, name: &str) -> String {
    w.state().pane_field_text(name).unwrap_or_else(|| panic!("{name} was not drawn")).to_owned()
}

fn open(w: &mut Window, pane: &str) {
    run_command(w, "window.pane", json!({"pane": pane, "shown": true}));
}

fn steps(w: &Window) -> usize {
    w.state().session().history().map_or(0, |h| h.undo_len())
}

/// What `shape.formatting` says of shape `id` now.
fn look(w: &mut Window, id: u32) -> Value {
    let page = page_id(w);
    w.state_mut().execute("shape.formatting", &json!({"ids": [id], "page": page})).unwrap()["shapes"][0].clone()
}

/// A rectangle dropped at (4, 5) and selected, with the Format Shape pane open.
fn window_with_a_shape() -> (Window, u32) {
    let mut w = edit_window(WIDE);
    drag_tile_to(&mut w, "Rectangle", 4.0, 5.0);
    let id = shape_ids(&w)[0];
    open(&mut w, "format_shape");
    w.rest();
    (w, id)
}

// ---- the pane: fill ------------------------------------------------------------------------------------------------------------------

#[test]
fn the_fill_has_four_types_and_each_shows_its_own_controls() {
    let (mut w, id) = window_with_a_shape();
    for name in ["pane.format.fill.none", "pane.format.fill.solid", "pane.format.fill.gradient", "pane.format.fill.pattern"] {
        assert!(is_enabled(&w, name), "{name} works");
    }
    assert!(w.state().element("pane.format.fill.transparency").is_some(), "a solid fill has a transparency");
    assert!(w.state().element("pane.format.fill.gradient.bar").is_none() && w.state().element("pane.format.fill.pattern.2").is_none());
    click_on(&mut w, "pane.format.fill.gradient");
    assert!(look(&mut w, id)["gradient"].is_object(), "Gradient fill gives the shape a gradient: {}", look(&mut w, id));
    for name in [
        "pane.format.fill.gradient.type",
        "pane.format.fill.gradient.angle",
        "pane.format.fill.gradient.bar",
        "pane.format.fill.gradient.stop.0",
        "pane.format.fill.gradient.stop.1",
        "pane.format.fill.gradient.add",
        "pane.format.fill.gradient.remove",
        "pane.format.fill.gradient.stop.colour",
        "pane.format.fill.gradient.stop.position",
        "pane.format.fill.gradient.stop.transparency",
        "pane.format.fill.gradient.rotate",
    ] {
        assert!(w.state().element(name).is_some(), "{name} is drawn for a gradient");
    }
    assert!(w.state().element("pane.format.fill.transparency").is_none(), "transparency belongs to the stops of a gradient");
    click_on(&mut w, "pane.format.fill.pattern");
    let l = look(&mut w, id);
    assert!(l["gradient"].is_null() && l["fill_pattern"] == json!(2), "the pattern took the place of the gradient: {l}");
    assert!(w.state().element("pane.format.fill.pattern.2").is_some() && w.state().element("pane.format.fill.background").is_some());
    click_on(&mut w, "pane.format.fill.solid");
    let l = look(&mut w, id);
    assert_eq!(l["fill_pattern"], json!(1), "Solid fill puts the pattern away too: {l}");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn a_typed_transparency_is_one_command_and_a_slider_dragged_is_one_undo_step_that_shows_as_it_goes() {
    let (mut w, id) = window_with_a_shape();
    let before = steps(&w);
    type_in(&mut w, "pane.format.fill.transparency", "40");
    assert_eq!(look(&mut w, id)["fill_transparency"], json!(40.0));
    assert_eq!(steps(&w), before + 1);
    assert_eq!(shown_in(&w, "pane.format.fill.transparency"), "40%");
    // A drag along the slider: the value is wherever it ends, and the whole drag is one step more.
    let slider = rect_of(&w, "pane.format.fill.transparency.slider");
    let from = slider.left_center() + egui::vec2(4.0, 0.0);
    drag(&mut w, from, egui::vec2(slider.width() * 0.7, 0.0));
    w.rest();
    let after = look(&mut w, id)["fill_transparency"].as_f64().unwrap();
    assert!(after > 40.0 && after < 100.0, "the slider moved the value: {after}");
    assert_eq!(steps(&w), before + 2, "one step for the typed value and one for the whole drag");
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(look(&mut w, id)["fill_transparency"], json!(40.0), "one undo takes the whole drag back");
    // Refused in words.
    type_in(&mut w, "pane.format.fill.transparency", "120");
    assert!(w.state().pane_field_refusal("pane.format.fill.transparency").is_some_and(|m| m.contains("0 to 100")));
    assert_eq!(look(&mut w, id)["fill_transparency"], json!(40.0));
}

#[test]
fn the_gradient_editor_adds_selects_edits_and_removes_stops_and_changes_the_layout() {
    let (mut w, id) = window_with_a_shape();
    click_on(&mut w, "pane.format.fill.gradient");
    let stops = |w: &mut Window| look(w, id)["gradient"]["stops"].as_array().unwrap().clone();
    assert_eq!(stops(&mut w).len(), 2);
    click_on(&mut w, "pane.format.fill.gradient.add");
    let list = stops(&mut w);
    assert_eq!(list.len(), 3, "a stop was added: {list:?}");
    assert_eq!(list[1]["position"], json!(0.5), "in the middle of the only gap");
    // The new stop is the one selected: its position is typed.
    type_in(&mut w, "pane.format.fill.gradient.stop.position", "30");
    assert_eq!(stops(&mut w)[1]["position"], json!(0.3));
    type_in(&mut w, "pane.format.fill.gradient.stop.transparency", "55");
    assert_eq!(stops(&mut w)[1]["transparency"], json!(55.0), "A stop has its own transparency");
    assert_eq!(stops(&mut w)[0]["transparency"], json!(0.0));
    // Another stop is picked by its handle.
    click_on(&mut w, "pane.format.fill.gradient.stop.2");
    assert_eq!(shown_in(&w, "pane.format.fill.gradient.stop.position"), "100%");
    click_on(&mut w, "pane.format.fill.gradient.remove");
    assert_eq!(stops(&mut w).len(), 2);
    assert!(!is_enabled(&w, "pane.format.fill.gradient.remove"), "a gradient keeps two stops");
    // The angle of a linear gradient, typed.
    type_in(&mut w, "pane.format.fill.gradient.angle", "45");
    assert_eq!(look(&mut w, id)["gradient"]["angle"], json!(45.0));
    // The layout, picked from the list.
    click_on(&mut w, "pane.format.fill.gradient.type");
    click_on(&mut w, "pane.format.fill.gradient.type.1");
    let g = look(&mut w, id)["gradient"].clone();
    assert_eq!((g["kind"].as_str(), g["origin"].as_str()), (Some("radial"), Some("center")), "{g}");
    assert!(w.state().element("pane.format.fill.gradient.direction").is_some() && w.state().element("pane.format.fill.gradient.angle").is_none());
    click_on(&mut w, "pane.format.fill.gradient.rotate");
    assert_eq!(look(&mut w, id)["gradient"]["rotate_with_shape"], json!(false));
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn the_pattern_grid_picks_a_pattern_and_the_colours_are_the_marks_and_the_ground() {
    let (mut w, id) = window_with_a_shape();
    click_on(&mut w, "pane.format.fill.pattern");
    for n in [2, 7, 13, 24] {
        assert!(w.state().element(&format!("pane.format.fill.pattern.{n}")).is_some(), "pattern {n} is on the grid");
    }
    assert!(w.state().element("pane.format.fill.pattern.25").is_none() && w.state().element("pane.format.fill.pattern.1").is_none(), "2 to 24 only");
    click_on(&mut w, "pane.format.fill.pattern.9");
    assert_eq!(look(&mut w, id)["fill_pattern"], json!(9));
    run_command(&mut w, "format.shape", json!({"fill_background": "#FFFF00"}));
    assert_eq!(look(&mut w, id)["fill_background"], json!("#ffff00"));
    type_in(&mut w, "pane.format.fill.transparency", "20");
    assert_eq!(look(&mut w, id)["fill_transparency"], json!(20.0));
    // The window remembers the pattern for the next time the radio is pressed.
    click_on(&mut w, "pane.format.fill.solid");
    click_on(&mut w, "pane.format.fill.pattern");
    assert_eq!(look(&mut w, id)["fill_pattern"], json!(9), "the pattern the person picked last");
}

// ---- the pane: line ------------------------------------------------------------------------------------------------------------------

#[test]
fn the_line_has_a_transparency_a_gradient_a_cap_and_a_compound_type() {
    let (mut w, id) = window_with_a_shape();
    for name in [
        "pane.format.line.none",
        "pane.format.line.solid",
        "pane.format.line.gradient",
        "pane.format.line.transparency",
        "pane.format.line.cap",
        "pane.format.line.compound",
    ] {
        assert!(is_enabled(&w, name), "{name} works");
    }
    type_in(&mut w, "pane.format.line.transparency", "30");
    assert_eq!(look(&mut w, id)["line_transparency"], json!(30.0));
    click_on(&mut w, "pane.format.line.cap");
    click_on(&mut w, "pane.format.line.cap.2");
    assert_eq!(look(&mut w, id)["line_cap"], json!("square"));
    click_on(&mut w, "pane.format.line.compound");
    click_on(&mut w, "pane.format.line.compound.1");
    assert_eq!(look(&mut w, id)["compound"], json!("double"));
    click_on(&mut w, "pane.format.line.gradient");
    assert!(look(&mut w, id)["line_gradient"].is_object());
    assert!(w.state().element("pane.format.line.gradient.bar").is_some() && w.state().element("pane.format.line.gradient.stop.0").is_some());
    click_on(&mut w, "pane.format.line.solid");
    assert!(look(&mut w, id)["line_gradient"].is_null(), "a solid line puts the gradient away");
}

// ---- the pane: shadow ----------------------------------------------------------------------------------------------------------------

#[test]
fn the_shadow_group_has_presets_and_fields_that_wake_with_a_shadow() {
    let (mut w, id) = window_with_a_shape();
    click_on(&mut w, "pane.format.tab.effects");
    assert!(w.state().element("pane.format.effects.0").is_some(), "the Shadow group heads the Effects tab");
    for n in 1..=5 {
        assert!(!is_enabled(&w, &format!("pane.format.effects.{n}")), "effect {n} has no command yet");
    }
    for name in ["transparency", "size", "blur", "angle", "distance"] {
        assert!(!is_enabled(&w, &format!("pane.format.shadow.{name}")), "{name} is quiet while the shape has no shadow");
    }
    assert!(is_enabled(&w, "pane.format.shadow.presets"));
    click_on(&mut w, "pane.format.shadow.presets");
    click_on(&mut w, "pane.format.shadow.presets.1");
    let s = look(&mut w, id)["shadow"].clone();
    assert_eq!((s["angle"].as_f64(), s["distance"].as_f64()), (Some(45.0), Some(4.0)), "Offset Bottom Right: {s}");
    for name in ["transparency", "size", "blur", "angle", "distance"] {
        assert!(is_enabled(&w, &format!("pane.format.shadow.{name}")), "{name} works with a shadow");
    }
    assert_eq!(shown_in(&w, "pane.format.shadow.distance"), "4 pt");
    // Angle 70 and Distance 9 pt.
    type_in(&mut w, "pane.format.shadow.angle", "70");
    type_in(&mut w, "pane.format.shadow.distance", "9 pt");
    type_in(&mut w, "pane.format.shadow.blur", "6");
    type_in(&mut w, "pane.format.shadow.size", "134");
    type_in(&mut w, "pane.format.shadow.transparency", "38");
    let s = look(&mut w, id)["shadow"].clone();
    assert_eq!(
        (s["angle"].as_f64(), s["distance"].as_f64(), s["blur"].as_f64(), s["size"].as_f64(), s["transparency"].as_f64()),
        (Some(70.0), Some(9.0), Some(6.0), Some(134.0), Some(38.0)),
        "{s}"
    );
    assert_eq!(shown_in(&w, "pane.format.shadow.angle"), "70 deg");
    // The slider moves the transparency live, in one step.
    let before = steps(&w);
    let slider = rect_of(&w, "pane.format.shadow.transparency.slider");
    drag(&mut w, slider.left_center() + egui::vec2(4.0, 0.0), egui::vec2(slider.width() * 0.5, 0.0));
    w.rest();
    assert_eq!(steps(&w), before + 1, "the drag is one step");
    // No Shadow takes it away, and the fields go quiet.
    click_on(&mut w, "pane.format.shadow.presets");
    click_on(&mut w, "pane.format.shadow.presets.0");
    assert!(look(&mut w, id)["shadow"].is_null());
    assert!(!is_enabled(&w, "pane.format.shadow.distance"));
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- the Home tab ----------------------------------------------------------------------------------------------------------------------

fn hit(w: &mut Window, name: &str) {
    click_on(w, name);
}

#[test]
fn the_effects_menu_gives_a_shadow_and_takes_it_away() {
    let (mut w, id) = window_with_a_shape();
    run_command(&mut w, "window.tab", json!({"tab": "Home"}));
    hit(&mut w, "ribbon.Home.ShapeEffectsMenu");
    hit(&mut w, "menu.ObjectEffectShadowGallery");
    hit(&mut w, "menu.ShadowBottomLeft");
    let s = look(&mut w, id)["shadow"].clone();
    assert_eq!((s["angle"].as_f64(), s["distance"].as_f64()), (Some(135.0), Some(4.0)), "{s}");
    hit(&mut w, "ribbon.Home.ShapeEffectsMenu");
    hit(&mut w, "menu.ObjectEffectShadowGallery");
    hit(&mut w, "menu.ShadowNone");
    assert!(look(&mut w, id)["shadow"].is_null());
    // Shadow Options... opens the Effects tab.
    hit(&mut w, "ribbon.Home.ShapeEffectsMenu");
    hit(&mut w, "menu.ObjectEffectShadowGallery");
    hit(&mut w, "menu.ShadowOptionsDialog");
    assert!(w.state().element("pane.format.effects.0").is_some(), "the Effects tab is on view");
}

#[test]
fn the_fill_and_line_menus_set_transparency_gradients_and_patterns() {
    let (mut w, id) = window_with_a_shape();
    run_command(&mut w, "window.tab", json!({"tab": "Home"}));
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
    assert_eq!((g["kind"].as_str(), g["angle"].as_f64()), (Some("linear"), Some(0.0)), "{g}");
    hit(&mut w, "ribbon.Home.LineColorGallery");
    hit(&mut w, "menu.LineTransparencyMenu");
    hit(&mut w, "menu.LineTransparency75");
    assert_eq!(look(&mut w, id)["line_transparency"], json!(75.0));
}

// ---- Quick Styles ----------------------------------------------------------------------------------------------------------------------

#[test]
fn quick_styles_wait_for_a_theme_and_then_apply_a_style_from_a_gallery_of_forty_six_tiles() {
    let (mut w, id) = window_with_a_shape();
    run_command(&mut w, "window.tab", json!({"tab": "Home"}));
    assert!(!is_enabled(&w, "ribbon.Home.ShapeStylesGallery"), "a page with no theme has no styles");
    let why = w.state().command_enabled("format.quickStyle").unwrap_err();
    assert!(why.contains("no theme") && why.contains("Design"), "{why}");
    run_command(&mut w, "theme.apply", json!({"theme": "tidepool"}));
    // The shape was dropped before the theme: it follows it now.
    w.state_mut().select(&[id]);
    w.rest();
    assert!(is_enabled(&w, "ribbon.Home.ShapeStylesGallery"));
    hit(&mut w, "ribbon.Home.ShapeStylesGallery");
    for n in 0..4 {
        assert!(w.state().element(&format!("menu.quickstyle.variant.{n}")).is_some(), "variant style {n}");
    }
    let tiles = (1..=6).flat_map(|r| (1..=7).map(move |c| format!("menu.quickstyle.{r}.{c}"))).filter(|n| w.state().element(n).is_some()).count();
    assert_eq!(tiles, 42, "six rows of seven");
    let before = look(&mut w, id)["fill"].clone();
    hit(&mut w, "menu.quickstyle.3.5");
    let after = look(&mut w, id);
    assert_ne!(after["fill"], before, "the style changed the fill: {before} then {after}");
    let page = page_id(&w);
    let tiles = w.state_mut().execute("shape.quick_styles", &json!({"ids": [id], "page": page})).unwrap();
    assert_eq!(
        (tiles["current"]["matrix"].as_u64(), tiles["current"]["colour"].as_u64()),
        (Some(3), Some(104)),
        "the tile pressed is the one marked now"
    );
    // A Variant Style.
    hit(&mut w, "ribbon.Home.ShapeStylesGallery");
    hit(&mut w, "menu.quickstyle.variant.1");
    let tiles = w.state_mut().execute("shape.quick_styles", &json!({"ids": [id], "page": page})).unwrap();
    assert_eq!((tiles["current"]["matrix"].as_u64(), tiles["current"]["colour"].as_u64()), (Some(101), Some(100)));
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- the Format Painter ----------------------------------------------------------------------------------------------------------------

#[test]
fn the_format_painter_carries_the_transparency_the_gradient_the_pattern_and_the_shadow() {
    let mut w = edit_window(WIDE);
    drag_tile_to(&mut w, "Rectangle", 3.0, 6.0);
    drag_tile_to(&mut w, "Ellipse", 7.0, 6.0);
    let ids = shape_ids(&w);
    let page = page_id(&w);
    let (a, b) = (ids[0], ids[1]);
    let send = |w: &mut Window, command: &str, params: Value| {
        let mut p = params;
        p["ids"] = json!([a]);
        p["page"] = json!(page);
        w.state_mut().execute(command, &p).unwrap();
    };
    send(
        &mut w,
        "shape.format",
        json!({"fill": "#336699", "fill_transparency": 30, "line_transparency": 10, "line_cap": "square", "compound": "double"}),
    );
    send(
        &mut w,
        "shape.gradient",
        json!({"angle": 45, "stops": [{"position": 0, "color": "#FF0000"}, {"position": 1, "color": "#0000FF", "transparency": 25}]}),
    );
    send(&mut w, "shape.shadow", json!({"preset": "bottom", "blur": 9}));
    w.rest();
    w.state_mut().select(&[a]);
    w.rest();
    run_command(&mut w, "format.painter", json!({"on": true}));
    let before = steps(&w);
    run_command(&mut w, "format.painter", json!({"to": [b]}));
    assert_eq!(steps(&w), before + 1, "a paint is one undo step");
    let (source, painted) = (look(&mut w, a), look(&mut w, b));
    for key in ["fill_transparency", "line_transparency", "line_cap", "compound", "gradient", "shadow"] {
        assert_eq!(painted[key], source[key], "{key}");
    }
    assert!(painted["gradient"].is_object() && painted["shadow"].is_object());
    // A source with no shadow takes the shadow away from the shape painted.
    w.state_mut().select(&[b]);
    run_command(&mut w, "shape.shadow", json!({"ids": [b], "page": page, "on": false}));
    w.state_mut().select(&[b]);
    w.rest();
    run_command(&mut w, "format.painter", json!({"on": true}));
    run_command(&mut w, "format.painter", json!({"to": [a]}));
    assert!(look(&mut w, a)["shadow"].is_null(), "the shadow was painted away");
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- the pane: the size of an arrowhead -----------------------------------------------------------------------------------

/// The Line section has **Begin Arrow size** and **End Arrow size**, a list each with Medium by default. A choice is one `format.shape`
/// and one undo step; the seven size classes are the list.
#[test]
fn the_line_section_has_begin_and_end_arrow_size_lists_that_send_one_command_each() {
    let (mut w, id) = window_with_a_shape();
    for name in ["pane.format.line.begin_size", "pane.format.line.end_size"] {
        assert!(is_enabled(&w, name), "{name} works for a shape whose line is drawn");
    }
    let l = look(&mut w, id);
    assert_eq!((l["begin_arrow_size"].as_u64(), l["end_arrow_size"].as_u64()), (Some(2), Some(2)), "Medium until one is chosen");
    click_on(&mut w, "pane.format.line.end_size");
    for n in 0..=6 {
        assert!(w.state().element(&format!("pane.format.line.end_size.{n}")).is_some(), "size {n} is in the list");
    }
    assert!(w.state().element("pane.format.line.end_size.7").is_none(), "and there are seven");
    let before = steps(&w);
    click_on(&mut w, "pane.format.line.end_size.5");
    let l = look(&mut w, id);
    assert_eq!((l["begin_arrow_size"].as_u64(), l["end_arrow_size"].as_u64()), (Some(2), Some(5)), "the end only: {l}");
    assert_eq!(steps(&w), before + 1, "one undo step");
    click_on(&mut w, "pane.format.line.begin_size");
    click_on(&mut w, "pane.format.line.begin_size.0");
    let l = look(&mut w, id);
    assert_eq!((l["begin_arrow_size"].as_u64(), l["end_arrow_size"].as_u64()), (Some(0), Some(5)), "{l}");
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(look(&mut w, id)["begin_arrow_size"].as_u64(), Some(2));
    assert_eq!(w.state().ui_panics(), 0);
}

/// With nothing selected the lists are quiet, as the pane's other line controls are.
#[test]
fn the_arrow_size_lists_are_quiet_with_nothing_selected() {
    let mut w = edit_window(WIDE);
    open(&mut w, "format_shape");
    w.rest();
    for name in ["pane.format.line.begin_size", "pane.format.line.end_size"] {
        assert!(!is_enabled(&w, name), "{name} is quiet with nothing selected");
    }
}
