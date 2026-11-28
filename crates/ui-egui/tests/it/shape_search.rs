//! Search for shapes and Quick Shapes in the Shapes window.
//!
//! The box at the top of the window narrows the stencil on view as words are typed, and Enter or the magnifier searches every stencil: the results are a stencil of their own, each
//! stencil's matches under its name. The Quick Shapes of each stencil stand above a thin divider, the Quick Shapes stencil gathers them, a master is added or taken away with its
//! right-click menu or by dragging it across the divider, and the AutoConnect flyout offers them.

use diacraft_ui_egui::edit::autoconnect::{ARROWS_DELAY, FLYOUT_DELAY};
use diacraft_ui_egui::theme::Tokens;
use egui::epaint::{ClippedPrimitive, Primitive, Vertex};
use egui::{Event, Key, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, Rect, Shape, TouchPhase, vec2};
use serde_json::{Value, json};

use crate::editing::{drag_tile_to, edit_window, key, type_text};
use crate::support::{Rest, Window, click, drag, elements_named, input, move_to, run_command};

fn window() -> Window {
    edit_window([1280.0, 900.0])
}

fn tile_names(w: &Window) -> Vec<String> {
    let mut tiles = elements_named(w, "shapes.tile.");
    tiles.sort_by(|a, b| a.1.min.y.total_cmp(&b.1.min.y).then(a.1.min.x.total_cmp(&b.1.min.x)));
    tiles.into_iter().map(|(n, ..)| n.trim_start_matches("shapes.tile.").to_owned()).collect()
}

fn rect_of(w: &Window, name: &str) -> Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

fn headings(w: &Window) -> Vec<String> {
    let mut found = elements_named(w, "shapes.group.");
    found.sort_by(|a, b| a.1.min.y.total_cmp(&b.1.min.y));
    found.into_iter().map(|(n, ..)| n.trim_start_matches("shapes.group.").to_owned()).collect()
}

fn state(w: &Window) -> Value {
    w.state().window_state()
}

/// A click in the box, the words typed, and Enter.
fn search_for(w: &mut Window, words: &str) {
    let at = rect_of(w, "shapes.filter").center();
    click(w, at);
    w.rest();
    type_text(w, words);
    key(w, Key::Enter, Modifiers::NONE);
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

fn quick_names(w: &mut Window, stencil: &str) -> Vec<String> {
    let v = w.state_mut().execute("stencil.quick", &json!({ "stencil": stencil })).unwrap();
    v["stencils"][0]["masters"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap().to_owned()).collect()
}

// ------------------------------------------------------------------------------------------------------------------------------- search

/// Typing narrows the stencil on view as before; Enter searches every stencil, and the results are a stencil of their own, each stencil's matches under its name.
#[test]
fn enter_in_the_box_searches_every_stencil_and_shows_the_results_under_the_stencils_names() {
    let mut w = window();
    assert!(w.state().element("shapes.search.button").is_some(), "the box has its magnifier");
    let at = rect_of(&w, "shapes.filter").center();
    click(&mut w, at);
    w.rest();
    type_text(&mut w, "arrow");
    // Typing alone narrows the stencil on view by name: Basic Shapes has no shape with "arrow" in its name.
    assert_eq!(state(&w)["stencil"], "basic");
    assert_eq!(state(&w)["shapeFilterCount"], 0);
    assert!(state(&w)["shapeSearch"].is_null());
    key(&mut w, Key::Enter, Modifiers::NONE);
    let s = state(&w);
    assert_eq!(s["stencil"], "search", "{s}");
    assert_eq!((s["shapeSearch"]["text"].as_str(), s["shapeSearch"]["mode"].as_str()), (Some("arrow"), Some("all")));
    assert_eq!(s["shapeFilter"], "arrow", "the box keeps the words");
    let tiles = tile_names(&w);
    for name in ["Right Arrow", "Bent Arrow", "Four-Way Arrow", "Data Flow"] {
        assert!(tiles.contains(&name.to_owned()), "{name} in {tiles:?}");
    }
    assert!(!tiles.contains(&"Rectangle".to_owned()));
    // The matches stand under the name of the stencil that holds them, in the order of the stencils.
    // (A stencil of the second batch that ships holds matches too: its words come after, in the order of the stencils.)
    assert!(headings(&w).starts_with(&["Arrow Shapes".to_owned(), "Gane-Sarson".to_owned()]), "{:?} {tiles:?}", headings(&w));
    assert!(rect_of(&w, "shapes.group.Arrow Shapes").min.y < rect_of(&w, "shapes.tile.Right Arrow").min.y);
    assert!(rect_of(&w, "shapes.tile.Right Arrow").min.y < rect_of(&w, "shapes.group.Gane-Sarson").min.y);
    // A master of the results drops as any does.
    drag_tile_to(&mut w, "Bent Arrow", 4.0, 4.0);
    let doc = w.state().session().document().unwrap();
    let page = doc.pages.get(0).unwrap();
    assert_eq!(page.shapes.len(), 1);
    assert_eq!(doc.master(page.shapes.get(0).unwrap().master.unwrap()).unwrap().name.as_deref(), Some("Bent Arrow"));
}

#[test]
fn the_magnifier_searches_too_and_the_words_are_whole_words() {
    let mut w = window();
    let at = rect_of(&w, "shapes.filter").center();
    click(&mut w, at);
    type_text(&mut w, "rect");
    let go = rect_of(&w, "shapes.search.button").center();
    click(&mut w, go);
    w.rest();
    assert_eq!(state(&w)["stencil"], "search");
    assert!(tile_names(&w).is_empty(), "a part of a word is not a word");
    assert!(state(&w)["shapeSearch"]["matches"] == 0);
    assert!(w.state().element("shapes.empty").is_some(), "the window says so");
    // The whole word finds it, with the box left as it is and searched again.
    run_command(&mut w, "shapes.search", json!({"text": "rectangle"}));
    assert_eq!(tile_names(&w), ["Rectangle"]);
}

#[test]
fn the_other_names_of_a_shape_find_it_through_its_search_words() {
    let mut w = window();
    run_command(&mut w, "shapes.search", json!({"text": "rhombus"}));
    assert_eq!(tile_names(&w), ["Diamond"]);
    assert_eq!(headings(&w), ["Basic Shapes"]);
}

#[test]
fn all_of_the_words_is_the_default_and_any_is_a_mode() {
    let mut w = window();
    run_command(&mut w, "shapes.search", json!({"text": "arrow decision"}));
    assert!(tile_names(&w).is_empty(), "no master is both");
    run_command(&mut w, "shapes.search", json!({"mode": "any"}));
    let tiles = tile_names(&w);
    assert!(tiles.contains(&"Decision".to_owned()) && tiles.contains(&"Right Arrow".to_owned()), "{tiles:?}");
    assert_eq!(state(&w)["shapeSearch"]["mode"], "any");
    run_command(&mut w, "shapes.search", json!({"mode": "all"}));
    assert!(tile_names(&w).is_empty());
    let e = w.state_mut().execute("shapes.search", &json!({"mode": "some"})).unwrap_err().to_string();
    assert!(e.contains("`mode` must be `all` or `any`"), "{e}");
}

#[test]
fn clearing_the_words_clears_the_search_and_the_stencil_before_it_is_shown_again() {
    let mut w = window();
    run_command(&mut w, "window.stencil", json!({"stencil": "flowchart"}));
    search_for(&mut w, "arrow");
    assert_eq!(state(&w)["stencil"], "search");
    // The cross in the box takes the words out, which resets the search.
    let clear = rect_of(&w, "shapes.filter.clear").center();
    click(&mut w, clear);
    w.rest();
    let s = state(&w);
    assert!(s["shapeSearch"].is_null(), "{s}");
    assert_eq!(s["stencil"], "flowchart", "the stencil that was shown before the search");
    assert_eq!(s["shapeFilter"], "");
    // Blank text through the command does the same, and a search that is not there is no error.
    run_command(&mut w, "shapes.search", json!({"text": "arrow"}));
    run_command(&mut w, "shapes.search", json!({"text": "  "}));
    assert!(state(&w)["shapeSearch"].is_null());
    assert_eq!(state(&w)["stencil"], "flowchart");
    run_command(&mut w, "shapes.search", json!({"text": ""}));
}

#[test]
fn the_results_are_a_row_of_the_stencil_menu_that_a_cross_clears() {
    let mut w = window();
    run_command(&mut w, "shapes.search", json!({"text": "arrow"}));
    run_command(&mut w, "window.stencil", json!({"stencil": "basic"}));
    assert!(!state(&w)["shapeSearch"].is_null(), "going to another stencil keeps the results");
    let more = rect_of(&w, "shapes.more").center();
    click(&mut w, more);
    w.rest();
    let row = rect_of(&w, "shapes.virtual.Search Results");
    assert!(row.min.y < rect_of(&w, "shapes.virtual.Quick Shapes").min.y, "the results lead");
    click(&mut w, row.center());
    w.rest();
    assert_eq!(state(&w)["stencil"], "search");
    click(&mut w, more);
    w.rest();
    let cross = rect_of(&w, "shapes.search.clear").center();
    click(&mut w, cross);
    w.rest();
    assert!(state(&w)["shapeSearch"].is_null());
    assert_eq!(state(&w)["stencil"], "basic");
}

#[test]
fn a_query_that_is_refused_says_so_in_the_status_bar_and_the_results_stay() {
    let mut w = window();
    run_command(&mut w, "shapes.search", json!({"text": "arrow"}));
    let many: Vec<String> = (0..20).map(|i| format!("w{i}")).collect();
    search_for(&mut w, &many.join(" "));
    // The box holds the 20 words (the Enter searched them), the engine refused, and the results are the ones of "arrow".
    let notice = w.state().notice().unwrap_or_default().to_owned();
    assert!(notice.contains("at most 16 different words"), "{notice}");
    assert_eq!(state(&w)["shapeSearch"]["text"], "arrow");
    let e = w.state_mut().execute("shapes.search", &json!({"text": "x".repeat(70)})).unwrap_err().to_string();
    assert!(e.contains("at most 64 characters"), "{e}");
    let e = w.state_mut().execute("shapes.search", &json!({"text": 5})).unwrap_err().to_string();
    assert!(e.contains("`text` must be text"), "{e}");
}

#[test]
fn the_results_follow_the_stencils_the_session_has_open() {
    let mut w = window();
    run_command(&mut w, "shapes.search", json!({"text": "process"}));
    // (A stencil of the second batch that ships holds matches too: its words come after, in the order of the stencils.)
    let first_two = ["Basic Flowchart".to_owned(), "Gane-Sarson".to_owned()];
    assert!(headings(&w).starts_with(&first_two), "{:?} {:?}", headings(&w), tile_names(&w));
    // A template that closes the flowchart still finds it: a stencil DiaCraft ships is searched whether or not it is open.
    run_command(&mut w, "file.new", json!({"template": "basic_diagram"}));
    run_command(&mut w, "shapes.search", json!({"text": "process"}));
    assert!(headings(&w).starts_with(&first_two), "{:?}", headings(&w));
}

// ------------------------------------------------------------------------------------------------------------------------- Quick Shapes

/// A stencil's Quick Shapes stand first, above a thin divider, and the rest below it.
#[test]
fn a_stencil_shows_its_quick_shapes_above_a_divider() {
    let mut w = window();
    run_command(&mut w, "stencil.add_quick", json!({"stencil": "basic", "master": "hexagon", "at": 0}));
    let tiles = tile_names(&w);
    assert_eq!(&tiles[..5], ["Hexagon", "Rectangle", "Square", "Ellipse", "Circle"], "{tiles:?}");
    assert_eq!(tiles.len(), 12, "every master is there once");
    let divider = rect_of(&w, "shapes.quickDivider");
    assert!(rect_of(&w, "shapes.tile.Circle").max.y <= divider.min.y + 1.0, "the group is above the divider");
    assert!(divider.max.y <= rect_of(&w, "shapes.tile.Triangle").min.y + 1.0, "the rest is below it");
    // A stencil with none customised shows the first four above it by default.
    run_command(&mut w, "stencil.reset", json!({"stencil": "basic"}));
    let tiles = tile_names(&w);
    assert_eq!(&tiles[..4], ["Rectangle", "Square", "Ellipse", "Circle"]);
    assert!(w.state().element("shapes.quickDivider").is_some());
}

#[test]
fn the_right_click_menu_of_a_tile_adds_and_takes_away_a_quick_shape() {
    let mut w = window();
    let hexagon = rect_of(&w, "shapes.tile.Hexagon").center();
    right_click(&mut w, hexagon);
    let add = rect_of(&w, "shapes.tileMenu.add");
    assert!(w.state().element("shapes.tileMenu.remove").is_none(), "a master below the divider is added, not removed");
    click(&mut w, add.center());
    w.rest();
    assert_eq!(quick_names(&mut w, "basic"), ["Rectangle", "Square", "Ellipse", "Circle", "Hexagon"]);
    assert_eq!(tile_names(&w)[4], "Hexagon", "the tile moved above the divider");
    // Now it is above it, and the menu takes it away.
    let hexagon = rect_of(&w, "shapes.tile.Hexagon").center();
    right_click(&mut w, hexagon);
    assert!(w.state().element("shapes.tileMenu.add").is_none());
    let remove = rect_of(&w, "shapes.tileMenu.remove");
    click(&mut w, remove.center());
    w.rest();
    assert_eq!(quick_names(&mut w, "basic"), ["Rectangle", "Square", "Ellipse", "Circle"]);
    assert_eq!(tile_names(&w)[4], "Triangle");
    // The choice is the drawing's and no undo step.
    assert!(w.state().session().is_dirty());
    assert!(w.state().command_enabled("edit.undo").is_err(), "nothing to undo");
}

#[test]
fn dragging_a_tile_across_the_divider_changes_its_group() {
    let mut w = window();
    // Down onto the group: the tile becomes a Quick Shape, at the tile it was let go on.
    let from = rect_of(&w, "shapes.tile.Hexagon").center();
    let onto = rect_of(&w, "shapes.tile.Square").center();
    drag(&mut w, from, onto - from);
    w.rest();
    assert_eq!(
        quick_names(&mut w, "basic"),
        ["Rectangle", "Hexagon", "Square", "Ellipse", "Circle"],
        "let go on the second tile, it takes its place"
    );
    assert!(w.state().session().document().unwrap().pages.get(0).unwrap().shapes.is_empty(), "nothing was dropped on the page");
    // Out of the group, onto the part below the divider: one no more.
    let from = rect_of(&w, "shapes.tile.Hexagon").center();
    let below = rect_of(&w, "shapes.tile.Triangle").center();
    drag(&mut w, from, below - from);
    w.rest();
    assert_eq!(quick_names(&mut w, "basic"), ["Rectangle", "Square", "Ellipse", "Circle"]);
    assert!(w.state().session().document().unwrap().pages.get(0).unwrap().shapes.is_empty());
}

#[test]
fn an_empty_group_shows_its_hint_and_reset_stencil_gives_the_stencil_its_own_back() {
    let mut w = window();
    for m in ["Rectangle", "Square", "Ellipse", "Circle"] {
        run_command(&mut w, "stencil.remove_quick", json!({"stencil": "basic", "master": m}));
    }
    assert!(w.state().element("shapes.quickHint").is_some(), "Drop Quick Shapes here");
    assert_eq!(tile_names(&w).len(), 12);
    // Reset Stencil, in the stencil menu.
    let more = rect_of(&w, "shapes.more").center();
    click(&mut w, more);
    w.rest();
    let reset = w.state().element("shapes.resetStencil").unwrap().clone();
    assert!(reset.enabled, "the stencil's Quick Shapes were set");
    click(&mut w, reset.rect.center());
    w.rest();
    assert_eq!(quick_names(&mut w, "basic"), ["Rectangle", "Square", "Ellipse", "Circle"]);
    assert!(w.state().element("shapes.quickHint").is_none());
    click(&mut w, more);
    w.rest();
    assert!(!w.state().element("shapes.resetStencil").unwrap().enabled, "and now they are the stencil's own");
    key(&mut w, Key::Escape, Modifiers::NONE);
}

#[test]
fn the_quick_shapes_stencil_gathers_every_open_stencils_group_under_its_name() {
    let mut w = window();
    run_command(&mut w, "window.stencil", json!({"stencil": "quick"}));
    assert_eq!(state(&w)["stencil"], "quick");
    assert_eq!(
        headings(&w),
        [
            "Basic Shapes",
            "Basic Flowchart",
            "Decorative Shapes",
            "Arrow Shapes",
            "Gane-Sarson",
            "Cross-Functional Flowchart Shapes",
            "Organization Chart Shapes"
        ]
    );
    let tiles = tile_names(&w);
    // (Two masters are called Process, so their tiles are named by id.)
    let all = elements_named(&w, "shapes.tile.").len() + elements_named(&w, "shapes.tileId.").len();
    assert_eq!(
        all,
        4 * 5 + 3 + 2,
        "four in each (the Organization Chart Shapes' too), three in Gane-Sarson (its connector is not one), and the two of the Cross-Functional Flowchart Shapes: {tiles:?}"
    );
    assert_eq!(&tiles[..4], ["Rectangle", "Square", "Ellipse", "Circle"]);
    // A template with fewer stencils gathers fewer, and then one group is not named.
    run_command(&mut w, "file.new", json!({"template": "basic_flowchart"}));
    run_command(&mut w, "window.stencil", json!({"stencil": "quick"}));
    assert_eq!(headings(&w), ["Basic Flowchart", "Basic Shapes"]);
    // The menu of a master here takes it out of its stencil's group.
    let process = rect_of(&w, "shapes.tile.Process").center();
    right_click(&mut w, process);
    let remove = rect_of(&w, "shapes.tileMenu.remove");
    click(&mut w, remove.center());
    w.rest();
    assert_eq!(quick_names(&mut w, "flowchart"), ["Decision", "Terminator", "Data"]);
    assert!(!tile_names(&w).contains(&"Process".to_owned()));
    // The row is in the stencil menu.
    let more = rect_of(&w, "shapes.more").center();
    click(&mut w, more);
    w.rest();
    assert!(w.state().element("shapes.virtual.Quick Shapes").is_some());
}

#[test]
fn the_filter_narrows_the_quick_shapes_stencil_by_name() {
    let mut w = window();
    run_command(&mut w, "window.stencil", json!({"stencil": "quick"}));
    run_command(&mut w, "window.shapeFilter", json!({"query": "circle"}));
    assert_eq!(tile_names(&w), ["Circle"]);
    assert_eq!(headings(&w), ["Basic Shapes"], "the groups that have a match, still named: there are several stencils");
}

// --------------------------------------------------------------------------------------------------------------------------- the flyout

fn shown_flyout(w: &Window) -> Vec<String> {
    let mut f: Vec<(String, Rect)> =
        w.state().elements().iter().filter(|e| e.name.starts_with("autoconnect.flyout.")).map(|e| (e.name.clone(), e.rect)).collect();
    f.sort_by(|a, b| a.0.cmp(&b.0));
    f.into_iter().map(|(n, _)| n).collect()
}

/// What the flyout offers, by master name.
fn offered(w: &Window) -> Vec<String> {
    w.state().autoconnect_flyout().map(|(_, names)| names).unwrap_or_default()
}

fn open_flyout(w: &mut Window) {
    run_command(w, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 5.0, "y": 4.0}));
    w.rest();
    let shape = w.state().screen_point_at((5.0, 4.0)).unwrap();
    for _ in 0..3 {
        move_to(w, shape);
    }
    w.advance(ARROWS_DELAY);
    w.step();
    let arrow = w.state().element("autoconnect.arrow.right").expect("the arrows are drawn").rect.center();
    for _ in 0..3 {
        move_to(w, arrow);
    }
    w.advance(FLYOUT_DELAY);
    w.step();
    w.step();
}

#[test]
fn the_flyout_offers_the_first_four_quick_shapes_of_the_stencil_on_view() {
    let mut w = window();
    run_command(&mut w, "stencil.add_quick", json!({"stencil": "basic", "master": "hexagon", "at": 0}));
    run_command(&mut w, "stencil.remove_quick", json!({"stencil": "basic", "master": "square"}));
    open_flyout(&mut w);
    assert_eq!(shown_flyout(&w).len(), 4, "{:?}", shown_flyout(&w));
    assert_eq!(offered(&w), ["Hexagon", "Rectangle", "Ellipse", "Circle"], "the Quick Shapes, as set");
    // The strip follows the stencil on view: another stencil, and the Quick Shapes stencil.
    run_command(&mut w, "window.stencil", json!({"stencil": "flowchart"}));
    run_command(&mut w, "window.stencil", json!({"stencil": "basic"}));
    key(&mut w, Key::Escape, Modifiers::NONE);
}

/// The window's strip is what `stencil.quick` says it is: the same four, for the stencil on view, whatever has been set.
#[test]
fn the_flyout_agrees_with_what_the_engine_offers() {
    let mut w = window();
    let sets: Vec<Vec<Value>> = vec![
        vec![],
        vec![json!({"c": "add", "stencil": "basic", "master": "hexagon", "at": 0})],
        vec![
            json!({"c": "remove", "stencil": "basic", "master": "rectangle"}),
            json!({"c": "remove", "stencil": "basic", "master": "square"}),
            json!({"c": "remove", "stencil": "basic", "master": "ellipse"}),
            json!({"c": "remove", "stencil": "basic", "master": "circle"}),
        ],
        vec![
            json!({"c": "add", "stencil": "basic", "master": "triangle", "at": 1}),
            json!({"c": "add", "stencil": "gane-sarson", "master": "data-flow", "at": 0}),
        ],
    ];
    for set in sets {
        for step in set {
            let command = if step["c"] == "add" { "stencil.add_quick" } else { "stencil.remove_quick" };
            let mut params = step.clone();
            params.as_object_mut().unwrap().remove("c");
            run_command(&mut w, command, params);
        }
        for stencil in ["basic", "gane-sarson", "flowchart"] {
            run_command(&mut w, "window.stencil", json!({ "stencil": stencil }));
            let engine = w.state_mut().execute("stencil.quick", &json!({ "stencil": stencil })).unwrap();
            let names: Vec<String> = engine["offer"]["masters"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap().to_owned()).collect();
            let list = w.state_mut().stencil_list();
            let window_offer: Vec<String> = list.iter().find(|s| s.id == stencil).unwrap().offer().into_iter().map(|m| m.name.clone()).collect();
            assert_eq!(window_offer, names, "{stencil}");
        }
    }
}

/// A Swimlane and a Separator are put in a list when dropped and are no shape to join, and Arrow Shapes does not feed the strip
/// With either stencil on view the flyout offers what the first open stencil that feeds it offers, as the engine says.
#[test]
fn the_flyout_offers_no_swimlane_or_separator_and_nothing_of_arrow_shapes_and_agrees_with_the_engine() {
    for stencil in ["cross-functional", "arrows"] {
        let mut w = window();
        run_command(&mut w, "window.stencil", json!({ "stencil": stencil }));
        open_flyout(&mut w);
        let got = offered(&w);
        let engine = w.state_mut().execute("stencil.quick", &json!({ "stencil": stencil })).unwrap();
        let want: Vec<String> = engine["offer"]["masters"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap().to_owned()).collect();
        assert_eq!(got, want, "{stencil}: {engine}");
        assert_eq!(got.len(), 4, "{stencil}: the next stencil's four");
        assert!(got.iter().all(|n| n != "Swimlane" && n != "Separator" && !n.contains("Arrow")), "{stencil}: {got:?}");
        assert_eq!(w.state_mut().autoconnect_offer().into_iter().map(|m| m.name).collect::<Vec<_>>(), want, "{stencil}: the window's own reading");
        key(&mut w, Key::Escape, Modifiers::NONE);
    }
}

// ----------------------------------------------------------------------------------------------------------------------- the box's own look

/// What the window's last frame drew at `pos`, in software: the flat-coloured triangles of the frame, in the order they were painted, each only inside its own clip rectangle (text and
/// pictures take their colours from a texture, and are left out). Colours are premultiplied, as the frame's are.
fn colour_at(w: &Window, pos: Pos2) -> [f32; 4] {
    let output = w.output();
    let mut seen = [0.0_f32; 4];
    for ClippedPrimitive { clip_rect, primitive } in w.ctx.tessellate(output.shapes.clone(), output.pixels_per_point) {
        let Primitive::Mesh(mesh) = primitive else { continue };
        if !clip_rect.contains(pos) {
            continue;
        }
        for corners in mesh.indices.as_chunks::<3>().0 {
            let v: Vec<Vertex> = corners.iter().filter_map(|i| mesh.vertices.get(*i as usize).copied()).collect();
            if v.len() != 3 || v[0].uv != v[1].uv || v[1].uv != v[2].uv {
                continue;
            }
            let (a, b, c) = (v[0].pos, v[1].pos, v[2].pos);
            let det = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
            if det.abs() < 1.0e-9 {
                continue;
            }
            let wa = ((b.x - pos.x) * (c.y - pos.y) - (b.y - pos.y) * (c.x - pos.x)) / det;
            let wb = ((c.x - pos.x) * (a.y - pos.y) - (c.y - pos.y) * (a.x - pos.x)) / det;
            let wc = 1.0 - wa - wb;
            if wa < -1.0e-4 || wb < -1.0e-4 || wc < -1.0e-4 {
                continue;
            }
            let colour = |k: usize| v[k].color.to_array().map(|n| f32::from(n) / 255.0);
            let (ca, cb, cc) = (colour(0), colour(1), colour(2));
            let src: Vec<f32> = (0..4).map(|k| wa * ca[k] + wb * cb[k] + wc * cc[k]).collect();
            for k in 0..4 {
                seen[k] = src[k] + seen[k] * (1.0 - src[3]);
            }
        }
    }
    seen
}

/// The lines of text the last frame laid out: the words, and the rectangle of the line.
fn texts(w: &Window) -> Vec<(String, Rect)> {
    fn walk(shape: &Shape, out: &mut Vec<(String, Rect)>) {
        match shape {
            Shape::Text(t) => out.push((t.galley.job.text.clone(), t.galley.rect.translate(t.pos.to_vec2()))),
            Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &w.output().shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// The wheel turned over `at`, down the tiles by `by` points (up, for a number below 0).
fn wheel(w: &mut Window, at: Pos2, by: f32) {
    move_to(w, at);
    w.input_mut().events.push(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, -by),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
        source: MouseWheelSource::Unknown,
    });
    // The wheel is smoothed over a few frames.
    for _ in 0..30 {
        w.step();
    }
    w.rest();
}

/// A window of `size` points at `ppp` pixels to the point, in the `theme` (`light` or `dark`), on a stencil that has more tiles than the window can show at once.
fn crowded_window(size: [f32; 2], ppp: f32, theme: &str) -> Window {
    // The harness's size is in the points of the screen before the scale: the window is that much smaller in its own points at a larger scale.
    let mut w = edit_window([size[0] * ppp, size[1] * ppp]);
    w.set_pixels_per_point(ppp);
    run_command(&mut w, "window.theme", json!({ "theme": theme }));
    let list = w.state_mut().stencil_list();
    let most = list.iter().filter(|s| !s.is_gathered()).max_by_key(|s| s.masters.len()).expect("a stencil is open").id.clone();
    run_command(&mut w, "window.stencil", json!({ "stencil": most }));
    w
}

/// Scrolls the tiles so that the first of them stands across the bottom edge of the box, and returns where to look: a point of the edge, inside a tile that stands across it.
/// The box's edge is a point wide and the tile is under it, so whatever of the tile is painted at that point shows in the frame.
fn scroll_a_tile_under_the_box(w: &mut Window) -> Pos2 {
    let (search, gallery) = (rect_of(w, "shapes.search"), rect_of(w, "shapes.gallery"));
    let tiles = elements_named(w, "shapes.tile");
    let first =
        tiles.iter().map(|t| t.1).min_by(|a, b| a.min.y.total_cmp(&b.min.y).then(a.min.x.total_cmp(&b.min.x))).expect("the stencil has tiles");
    wheel(w, gallery.center(), first.min.y - (search.bottom() - 20.0));
    let edge = search.bottom() - 0.5;
    let under = elements_named(w, "shapes.tile").into_iter().map(|t| t.1).find(|t| t.min.y < edge && t.max.y > edge);
    let under = under
        .unwrap_or_else(|| panic!("no tile stands across the box's bottom edge ({edge}) after the scroll: {:?}", elements_named(w, "shapes.tile")));
    Pos2::new(under.center().x, edge)
}

/// A narrow window and a wide one, at each scale, in each theme.
const LOOKS: [([f32; 2], f32, &str); 5] = [
    ([800.0, 520.0], 1.0, "light"),
    ([800.0, 520.0], 1.5, "dark"),
    ([1600.0, 640.0], 2.0, "light"),
    ([1280.0, 560.0], 1.0, "dark"),
    ([1600.0, 900.0], 1.5, "light"),
];

/// The area the tiles are drawn and clipped in begins where the box ends: nothing of a tile can reach the box's edge.
#[test]
fn the_tiles_area_begins_where_the_search_box_ends() {
    for (size, ppp, theme) in LOOKS {
        let w = crowded_window(size, ppp, theme);
        let (search, gallery) = (rect_of(&w, "shapes.search"), rect_of(&w, "shapes.gallery"));
        assert!(
            gallery.min.y >= search.max.y - 0.01,
            "{size:?} {ppp} {theme}: the tiles' area ({gallery:?}) begins above the box's bottom edge ({search:?})"
        );
    }
}

/// The tiles scroll under the box, and what is above the gallery is not theirs to paint: the box's bottom edge is as it was before the scroll, in each theme, at each scale, in a
/// narrow window and in a wide one.
#[test]
fn scrolled_tiles_do_not_paint_over_the_search_box() {
    for (size, ppp, theme) in LOOKS {
        let mut w = crowded_window(size, ppp, theme);
        let t = Tokens::get(&w.ctx);
        let (search, gallery) = (rect_of(&w, "shapes.search"), rect_of(&w, "shapes.gallery"));
        let first_before = elements_named(&w, "shapes.tile").into_iter().map(|t| t.1.min.y).fold(f32::MAX, f32::min);
        // The box's bottom edge, over the tile that will come to stand across it.
        let probe_x = elements_named(&w, "shapes.tile").into_iter().map(|t| t.1.center().x).fold(f32::MAX, f32::min);
        let before = colour_at(&w, Pos2::new(probe_x, search.bottom() - 0.5));
        let want = t.input_border.to_array().map(|n| f32::from(n) / 255.0);
        for k in 0..3 {
            assert!(
                (before[k] - want[k]).abs() < 0.02,
                "{size:?} {ppp} {theme}: the probe does not see the box's edge: {before:?} is not the border {want:?}"
            );
        }
        let at = scroll_a_tile_under_the_box(&mut w);
        let first_after = elements_named(&w, "shapes.tile").into_iter().map(|t| t.1.min.y).fold(f32::MAX, f32::min);
        assert!(first_after < first_before - 1.0, "{size:?} {ppp} {theme}: the tiles did not scroll ({first_before} to {first_after})");
        assert!(first_after < search.bottom(), "{size:?} {ppp} {theme}: the tiles are not under the box");
        let after = colour_at(&w, at);
        for k in 0..4 {
            assert!(
                (after[k] - before[k]).abs() < 0.02,
                "{size:?} {ppp} {theme}: a tile painted over the box's bottom edge: {before:?} became {after:?} at {at:?}"
            );
        }
        let (search_after, gallery_after) = (rect_of(&w, "shapes.search"), rect_of(&w, "shapes.gallery"));
        assert_eq!(search_after, search, "scrolling the tiles moves nothing above them");
        assert_eq!(gallery_after, gallery, "the tiles' area stays where it was");
    }
}

/// The words in the box, the prompt and the words typed, stand in the middle of the box from top to bottom, as the text of any field does.
#[test]
fn the_words_in_the_search_box_are_centred_in_it_from_top_to_bottom() {
    for (ppp, theme) in [(1.0, "light"), (1.5, "dark"), (2.0, "light")] {
        let mut w = crowded_window([1280.0, 700.0], ppp, theme);
        let search = rect_of(&w, "shapes.search");
        let line = |w: &Window, words: &str| {
            texts(w).into_iter().find(|(text, _)| text == words).map(|(_, r)| r).unwrap_or_else(|| panic!("{words:?} is not drawn: {:?}", texts(w)))
        };
        let prompt = line(&w, "Search shapes");
        assert!(
            (prompt.center().y - search.center().y).abs() <= 1.0,
            "{ppp} {theme}: the prompt ({prompt:?}) is not in the middle of the box ({search:?})"
        );
        let at = rect_of(&w, "shapes.filter").center();
        click(&mut w, at);
        w.rest();
        type_text(&mut w, "Ag");
        let typed = line(&w, "Ag");
        assert!(
            (typed.center().y - search.center().y).abs() <= 1.0,
            "{ppp} {theme}: the words typed ({typed:?}) are not in the middle of the box ({search:?})"
        );
        // The words and the prompt stand at the same height.
        assert!((typed.center().y - prompt.center().y).abs() <= 1.0, "{ppp} {theme}: {typed:?} and {prompt:?} differ in height");
    }
}
