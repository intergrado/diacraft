//! The compact chooser lists the engine's stencils and the gallery filters actual masters by name.
//! Imported catalogues, placement identity, keyboard ownership and preview work stay bounded.

use serde_json::{Value, json};

use crate::editing::{drag_tile_to, edit_window_with};
use crate::support::{Rest, Window, click, elements_named, run_command};

/// A window on a new drawing, optionally with a list of stencils of the test's own.
fn window_with(list: Option<Value>) -> Window {
    edit_window_with([1280.0, 900.0], |app| match &list {
        Some(list) => app.with_stencil_list(list),
        None => app,
    })
}

fn tile_names(w: &Window) -> Vec<String> {
    let mut tiles = elements_named(w, "shapes.tile.");
    tiles.sort_by(|a, b| a.1.min.y.total_cmp(&b.1.min.y).then(a.1.min.x.total_cmp(&b.1.min.x)));
    tiles.into_iter().map(|(n, ..)| n.trim_start_matches("shapes.tile.").to_owned()).collect()
}

fn rows(w: &mut Window) -> Vec<String> {
    crate::stencil_files::rows(w)
}

fn pick_row(w: &mut Window, name: &str) {
    let more = w.state().element("shapes.more").unwrap().rect.center();
    click(w, more);
    w.rest();
    let at = w.state().element(&format!("shapes.more.{name}")).unwrap_or_else(|| panic!("no row for {name}")).rect.center();
    click(w, at);
    w.rest();
}

fn shape_masters(w: &Window) -> Vec<String> {
    let doc = w.state().session().document().unwrap();
    let page = doc.pages.get(0).unwrap();
    let name_of = |id: u32| doc.master(id).and_then(|m| m.name.as_deref().map(str::to_owned)).unwrap_or_else(|| "?".to_owned());
    page.shapes.iter().map(|s| s.master.map_or_else(|| "none".to_owned(), name_of)).collect()
}

/// The engine has six stencils now, and the window shows a row for each, the first chosen; a click on the second row shows its masters
/// (all enabled, none quiet), and a tile of it drops its shape.
#[test]
fn each_stencil_the_engine_lists_has_a_row_and_a_click_shows_its_masters() {
    let mut w = window_with(None);
    assert_eq!(
        rows(&mut w),
        [
            "Basic Shapes",
            "Decorative Shapes",
            "Arrow Shapes",
            "Basic Flowchart",
            "Cross-Functional Flowchart Shapes",
            "Gane-Sarson",
            "Organization Chart Shapes"
        ],
        "one row per stencil, by category (General, Flowchart, Software and Database, Business), each in the engine's order"
    );
    assert_eq!(w.state().window_state()["stencil"], "basic");
    let basic = tile_names(&w);
    assert_eq!(basic.len(), 12, "Basic Shapes contains only its actual masters");
    assert!(basic.contains(&"Rectangle".to_owned()) && !basic.contains(&"Process".to_owned()));
    pick_row(&mut w, "Basic Flowchart");
    assert_eq!(w.state().window_state()["stencil"], "flowchart");
    let flow = tile_names(&w);
    assert_eq!(flow.len(), 18, "Basic Flowchart: eighteen masters: {flow:?}");
    for name in ["Process", "Decision", "Terminator", "Data", "Document"] {
        assert!(flow.contains(&name.to_owned()), "{name} in {flow:?}");
    }
    assert!(!flow.contains(&"Rectangle".to_owned()), "the other stencil's masters are not shown");
    assert!(elements_named(&w, "shapes.tile.").iter().all(|(_, _, enabled)| *enabled), "every master of the flowchart works");
    // A tile of the second stencil drops its shape.
    drag_tile_to(&mut w, "Decision", 4.0, 4.0);
    assert_eq!(shape_masters(&w), ["Decision"]);
    // And back to the first.
    pick_row(&mut w, "Basic Shapes");
    assert!(tile_names(&w).contains(&"Rectangle".to_owned()));
}

/// Every stencil the engine ships, the ones a new session leaves closed too (the second batch's: STENCIL2): opened, it has a row, and its tiles are its masters, each enabled. A stencil that
/// ships later is covered by this test with no change to it.
#[test]
fn every_shipped_stencil_once_opened_shows_a_tile_for_each_of_its_masters() {
    let mut w = window_with(None);
    for stencil in diacraft_engine::stencil::stencils() {
        if !stencil.default_open {
            run_command(&mut w, "stencil.open", json!({ "id": stencil.id }));
            w.rest();
        }
        run_command(&mut w, "window.stencil", json!({ "stencil": stencil.id }));
        w.rest();
        assert_eq!(w.state().window_state()["stencil"], stencil.id, "{}: the window shows it", stencil.id);
        let mut shown = tile_names(&w);
        shown.sort();
        let mut wanted: Vec<String> = stencil.masters().iter().filter(|m| !m.is_hidden()).map(|m| m.name.to_owned()).collect();
        wanted.sort();
        assert_eq!(shown, wanted, "{}: a tile for each master and no other", stencil.id);
        assert!(elements_named(&w, "shapes.tile.").iter().all(|(_, _, enabled)| *enabled), "{}: every master works", stencil.id);
    }
    assert_eq!(w.state().ui_panics(), 0);
}

/// More Shapes is a flyout that names every stencil; choosing one shows its masters.
#[test]
fn more_shapes_lists_the_stencils_and_a_choice_shows_its_masters() {
    let mut w = window_with(None);
    let more = w.state().element("shapes.more").expect("More Shapes is drawn").clone();
    assert!(more.enabled, "it has stencils to list");
    click(&mut w, more.rect.center());
    w.rest();
    let names: Vec<String> = elements_named(&w, "shapes.more.").into_iter().map(|(n, ..)| n).filter(|n| n != "shapes.more.open").collect();
    assert_eq!(names.len(), diacraft_engine::stencil::stencils().len(), "a row for every shipped built-in stencil: {names:?}");
    // Open Stencil is in the flyout, and quiet when the app has no file dialog to ask (the web; this window has none).
    assert!(!w.state().element("shapes.more.open").expect("Open Stencil is drawn").enabled);
    assert!(names.contains(&"shapes.more.Basic Shapes".to_owned()) && names.contains(&"shapes.more.Basic Flowchart".to_owned()));
    let flowchart = w.state().element("shapes.more.Basic Flowchart").unwrap().rect.center();
    click(&mut w, flowchart);
    w.rest();
    assert_eq!(w.state().window_state()["stencil"], "flowchart");
    assert!(tile_names(&w).contains(&"Terminator".to_owned()));
}

/// The review's case for a stencil the window was not written for: a second stencil in the list, with masters of its own, appears with a
/// row, tiles and a More Shapes entry, and nothing in the window names it.
#[test]
fn a_fake_second_stencil_appears_with_no_change_in_the_window() {
    let list = json!({"stencils": [
        {"id": "basic", "name": "Basic Shapes", "masters": [
            {"id": "rectangle", "name": "Rectangle", "width": 1.5, "height": 1.0},
            {"id": "star", "name": "Star", "width": 1.0, "height": 1.0}
        ]},
        {"id": "electrical", "name": "Electrical Parts", "masters": [
            {"id": "resistor", "name": "Resistor", "width": 1.0, "height": 0.3},
            {"id": "capacitor", "name": "Capacitor", "width": 0.4, "height": 1.0},
            {"id": "ground", "name": "Ground", "width": 0.6, "height": 0.6}
        ]},
        {"id": "empty", "name": "Nothing Yet", "masters": []}
    ]});
    let mut w = window_with(Some(list));
    assert_eq!(rows(&mut w), ["Basic Shapes", "Electrical Parts", "Nothing Yet"]);
    // Only the actual masters appear, including a master that used to be a placeholder.
    let basic = tile_names(&w);
    assert_eq!(basic.iter().filter(|n| *n == "Star").count(), 1, "{basic:?}");
    assert!(w.state().element("shapes.tile.Star").unwrap().enabled, "the list has a Star master, so its tile works");
    for quiet in ["Cross", "Arrow", "Cylinder"] {
        assert!(w.state().element(&format!("shapes.tile.{quiet}")).is_none(), "{quiet} has no master in this list");
    }
    pick_row(&mut w, "Electrical Parts");
    assert_eq!(tile_names(&w), ["Resistor", "Capacitor", "Ground"], "the masters of the stencil, in its order, and nothing else");
    assert!(elements_named(&w, "shapes.tile.").iter().all(|(_, _, enabled)| *enabled));
    // A stencil with no masters shows no tile.
    pick_row(&mut w, "Nothing Yet");
    assert!(tile_names(&w).is_empty());
    // More Shapes names all three.
    let more = w.state().element("shapes.more").unwrap().rect.center();
    click(&mut w, more);
    w.rest();
    assert_eq!(elements_named(&w, "shapes.more.").into_iter().filter(|(n, ..)| n != "shapes.more.open").count(), 3);
    // A master the engine does not have is dropped as the engine answers, in words, and the window goes on.
    let electrical = w.state().element("shapes.more.Electrical Parts").unwrap().rect.center();
    click(&mut w, electrical);
    w.rest();
    assert_eq!(tile_names(&w), ["Resistor", "Capacitor", "Ground"]);
    let revision = w.state().session().revision();
    drag_tile_to(&mut w, "Resistor", 4.0, 4.0);
    assert_eq!(w.state().session().revision(), revision, "the engine has no such master and nothing was dropped");
    assert!(w.state().notice().is_some_and(|n| n.contains("resistor")), "{:?}", w.state().notice());
    assert_eq!(w.state().ui_panics(), 0);
}

/// The window command, by id or name, and its refusal in words.
#[test]
fn the_window_command_shows_a_stencil_by_id_or_name() {
    let mut w = window_with(None);
    run_command(&mut w, "window.stencil", json!({"stencil": "flowchart"}));
    assert!(tile_names(&w).contains(&"Process".to_owned()));
    run_command(&mut w, "window.stencil", json!({"stencil": "basic shapes"}));
    assert!(tile_names(&w).contains(&"Rectangle".to_owned()));
    let refused = w.state_mut().execute("window.stencil", &json!({"stencil": "nope"})).unwrap_err().to_string();
    assert!(refused.contains("no stencil `nope`") && refused.contains("Basic Flowchart"), "{refused}");
    assert!(diacraft_engine::find_command("window.stencil").is_none(), "it is the window's, not the engine's");
}

/// Every approved new library is reachable through its row and drops by its exact master id.
#[test]
fn new_libraries_drop_their_nodes_and_data_flow_is_a_real_connector() {
    let mut w = window_with(None);
    for (library, tile, master_count) in [("Decorative Shapes", "5-Point Star", 8), ("Arrow Shapes", "Right Arrow", 8), ("Gane-Sarson", "Process", 4)]
    {
        pick_row(&mut w, library);
        assert_eq!(tile_names(&w).len(), master_count);
        drag_tile_to(&mut w, tile, 4.0, 4.0);
    }
    drag_tile_to(&mut w, "Data Flow", 6.0, 5.0);
    let doc = w.state().session().document().unwrap();
    let placed = doc.page(1).unwrap().shapes.iter().next_back().unwrap();
    assert!(placed.sheet.cells.keys().any(|name| name.as_str() == "BeginX"));
    assert!(placed.sheet.cells.keys().any(|name| name.as_str() == "EndX"));
    assert_eq!(doc.master(placed.master.unwrap()).unwrap().name.as_deref(), Some("Data Flow"));
    let process = doc.masters.iter().find(|master| master.name.as_deref() == Some("Process")).unwrap();
    assert!(process.unique_id.as_deref().unwrap().starts_with("{D1AC0006-"), "the exact Gane-Sarson tile selects the rounded node");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn filtering_uses_actual_names_and_ui_state_only_and_clears_when_switching() {
    use crate::editing::{key, type_text};
    use egui::{Key, Modifiers};
    let mut w = window_with(None);
    let revision = w.state().session().revision();
    run_command(&mut w, "window.shapeFilter", json!({"query": "TRIANGLE"}));
    assert_eq!(tile_names(&w), ["Triangle", "Right Triangle"]);
    assert_eq!(w.state().window_state()["shapeFilterCount"], 2);
    assert_eq!(w.state().session().revision(), revision);
    assert!(!w.state().is_dirty());
    assert!(w.state().command_enabled("edit.undo").is_err());
    run_command(&mut w, "window.shapeFilter", json!({"query": "Decision"}));
    assert!(tile_names(&w).is_empty(), "the filter never searches another stencil");
    assert!(w.state().element("shapes.empty").is_some());
    assert_eq!(w.state().window_state()["shapeFilterCount"], 0);
    pick_row(&mut w, "Basic Flowchart");
    assert_eq!(w.state().window_state()["shapeFilter"], "");
    assert_eq!(tile_names(&w).len(), 18);
    let at = w.state().element("shapes.filter").unwrap().rect.center();
    click(&mut w, at);
    type_text(&mut w, "dECis");
    assert_eq!(tile_names(&w), ["Decision"]);
    let clear = w.state().element("shapes.filter.clear").unwrap().rect.center();
    click(&mut w, clear);
    w.rest();
    assert_eq!(tile_names(&w).len(), 18);
    type_text(&mut w, &"é".repeat(160));
    assert_eq!(w.state().window_state()["shapeFilter"].as_str().unwrap().chars().count(), 128);
    key(&mut w, Key::Escape, Modifiers::NONE);
    run_command(&mut w, "window.shapeFilter", json!({"query": "é".repeat(160)}));
    assert_eq!(w.state().window_state()["shapeFilter"], "é".repeat(128));
    assert!(w.state_mut().execute("window.shapeFilter", &json!({"query": 12})).is_err());
    assert_eq!(w.state().session().revision(), revision);
}

#[test]
fn filter_owns_typing_edit_shortcuts_and_escape_without_changing_the_canvas() {
    use crate::editing::{frame_of, key, shape_ids, type_text};
    use crate::saving::ctrl;
    use egui::{Event, Key, Modifiers};
    let mut w = window_with(None);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    let id = shape_ids(&w)[0];
    let frame = frame_of(&w, id);
    let revision = w.state().session().revision();
    let selection = w.state().window_state()["selection"].clone();
    let at = w.state().element("shapes.filter").unwrap().rect.center();
    click(&mut w, at);
    type_text(&mut w, "rect");
    for (key_code, modifiers) in [
        (Key::Delete, Modifiers::NONE),
        (Key::ArrowRight, Modifiers::NONE),
        (Key::F2, Modifiers::NONE),
        (Key::C, ctrl()),
        (Key::X, ctrl()),
        (Key::Z, ctrl()),
    ] {
        key(&mut w, key_code, modifiers);
    }
    for event in [Event::Copy, Event::Cut, Event::Paste("circle".to_owned())] {
        w.input_mut().events.push(event);
        w.step();
    }
    w.rest();
    assert_eq!(w.state().session().revision(), revision);
    assert_eq!(frame_of(&w, id), frame);
    assert_eq!(w.state().window_state()["selection"], selection);
    assert_eq!(w.state().window_state()["editingText"], false);
    assert!(w.state().session().clipboard().is_none());
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().window_state()["selection"], selection, "Escape belongs to the filter on the frame egui drops its focus");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().window_state()["selection"], json!([]), "the following Escape belongs to the canvas again");
}

#[test]
fn filter_is_unicode_case_insensitive_and_empty_libraries_stay_empty() {
    let list = json!({"stencils": [
        {"id": "names", "name": "Names", "masters": [{"id": "one", "name": "Éclair"}, {"id": "two", "name": "Eclipse"}]},
        {"id": "empty", "name": "Empty", "masters": []}
    ]});
    let mut w = window_with(Some(list));
    run_command(&mut w, "window.shapeFilter", json!({"query": "éC"}));
    assert_eq!(tile_names(&w), ["Éclair"]);
    run_command(&mut w, "window.stencil", json!({"stencil": "empty"}));
    assert!(tile_names(&w).is_empty());
    assert!(w.state().element("shapes.empty").is_some());
    assert_eq!(w.state().window_state()["shapeFilterCount"], 0);
}

#[test]
fn same_named_tiles_keep_distinct_master_identity() {
    let list = json!({"stencils": [{"id": "duplicates", "name": "Duplicates", "masters": [
        {"id": "rectangle", "name": "Process"}, {"id": "circle", "name": "Process"}
    ]}]});
    let mut w = window_with(Some(list));
    assert!(w.state().element("shapes.tile.Process").is_none(), "ambiguous name aliases are omitted");
    for id in ["rectangle", "circle"] {
        let tile = w.state().element(&format!("shapes.tileId.{id}")).unwrap().rect.center();
        click(&mut w, tile);
        w.rest();
    }
    assert_eq!(shape_masters(&w), ["Rectangle", "Circle"]);
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn changing_filter_resets_gallery_scroll_and_keeps_the_header_fixed() {
    use crate::support::move_to;
    use egui::{Event, Modifiers, MouseWheelSource, MouseWheelUnit, TouchPhase, vec2};
    let list = json!({"stencils": [{"id": "many", "name": "Many", "masters": (0..300).map(|i| json!({"id": format!("m{i}"), "name": format!("Master {i}")})).collect::<Vec<_>>()}]});
    let mut w = window_with(Some(list));
    let start = w.state().element("shapes.tile.Master 0").unwrap().rect.top();
    let chooser = w.state().element("shapes.more").unwrap().rect;
    let filter = w.state().element("shapes.filter").unwrap().rect;
    let over = w.state().element("shapes.tile.Master 2").unwrap().rect.center();
    move_to(&mut w, over);
    w.input_mut().events.push(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, -800.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
        source: MouseWheelSource::Unknown,
    });
    for _ in 0..30 {
        w.step();
    }
    w.rest();
    assert!(w.state().element("shapes.tile.Master 0").unwrap().rect.top() < start - 100.0);
    assert_eq!(w.state().element("shapes.more").unwrap().rect, chooser);
    assert_eq!(w.state().element("shapes.filter").unwrap().rect, filter);
    run_command(&mut w, "window.shapeFilter", json!({"query": "Master 29"}));
    assert!((w.state().element("shapes.tile.Master 29").unwrap().rect.top() - start).abs() < 1.0);
    assert_eq!(w.state().window_state()["shapeFilterCount"], 11);
    run_command(&mut w, "window.shapeFilter", json!({"query": ""}));
    assert!((w.state().element("shapes.tile.Master 0").unwrap().rect.top() - start).abs() < 1.0);
    assert!(w.state().tiles_read() < 80, "filtering still reads only visible preview art");
}

#[test]
fn changing_query_or_stencil_during_a_tile_drag_cancels_the_hidden_source() {
    use crate::editing::{at, shape_ids};
    use crate::support::{button, move_to};
    for switch_stencil in [false, true] {
        let mut w = window_with(None);
        let from = w.state().element("shapes.tile.Rectangle").unwrap().rect.center();
        let to = at(&w, 4.0, 4.0);
        move_to(&mut w, from);
        button(&mut w, from, true);
        move_to(&mut w, from + egui::vec2(20.0, 20.0));
        if switch_stencil {
            w.state_mut().execute("window.stencil", &json!({"stencil": "flowchart"})).unwrap();
        } else {
            w.state_mut().execute("window.shapeFilter", &json!({"query": "Circle"})).unwrap();
        }
        move_to(&mut w, to);
        button(&mut w, to, false);
        w.rest();
        assert!(shape_ids(&w).is_empty(), "a removed source cannot drop a stale master");
        run_command(&mut w, "window.stencil", json!({"stencil": "basic"}));
        run_command(&mut w, "window.shapeFilter", json!({"query": ""}));
        drag_tile_to(&mut w, "Circle", 4.0, 4.0);
        assert_eq!(shape_ids(&w).len(), 1, "the next drag remains usable");
    }
}

#[test]
fn no_drawing_keeps_actual_masters_disabled_and_explains_how_to_place_them() {
    use egui_kittest::kittest::Queryable;
    let app = diacraft_ui_egui::DiacraftApp::new(diacraft_engine::Session::new(), None);
    let mut w = crate::support::window(app, [1000.0, 700.0]);
    w.rest();
    assert_eq!(tile_names(&w).len(), 12);
    assert!(elements_named(&w, "shapes.tile.").iter().all(|(_, _, on)| !on));
    w.get_by_label("Create or open a drawing to place shapes.");
    run_command(&mut w, "window.shapeFilter", json!({"query": "rect"}));
    assert_eq!(tile_names(&w), ["Rectangle"]);
    assert!(w.state().session().document().is_none());
}

#[test]
fn an_empty_catalogue_can_still_open_the_native_stencil_picker() {
    let app = diacraft_ui_egui::DiacraftApp::new(diacraft_engine::Session::new(), None)
        .with_stencil_list(&json!({"stencils": []}))
        .with_stencil_picker(std::sync::Arc::new(|| None));
    let mut w = crate::support::window(app, [1000.0, 700.0]);
    w.rest();
    let chooser = w.state().element("shapes.more").unwrap();
    assert!(chooser.enabled);
    let at = chooser.rect.center();
    click(&mut w, at);
    w.rest();
    let open = w.state().element("shapes.more.open").unwrap();
    assert!(open.enabled);
    let at = open.rect.center();
    click(&mut w, at);
    w.rest();
    assert!(!w.state().stencil_dialog_open());
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn maximum_catalogue_keeps_canvas_controls_reported_and_previews_lazy() {
    let list = json!({"stencils": [{"id": "many", "name": "Many", "masters": (0..3000).map(|i| json!({"id": format!("m{i}"), "name": format!("Master {i}")})).collect::<Vec<_>>()}]});
    let mut w = window_with(Some(list));
    assert_eq!(tile_names(&w).len(), 2048);
    assert!(w.state().element("canvas").is_some());
    assert!(w.state().tiles_read() < 40);
    run_command(&mut w, "window.shapeFilter", json!({"query": "Master 2047"}));
    assert_eq!(tile_names(&w), ["Master 2047"]);
    run_command(&mut w, "window.shapeFilter", json!({"query": "Master 2048"}));
    assert!(tile_names(&w).is_empty(), "filtering never expands the parsed catalogue budget");
}

#[test]
fn catalogue_scroll_keeps_open_stencil_reachable_and_selects_a_late_entry() {
    use crate::support::move_to;
    use egui::{Event, Modifiers, MouseWheelSource, MouseWheelUnit, TouchPhase, vec2};
    let list =
        json!({"stencils": (0..64).map(|i| json!({"id": format!("s{i}"), "name": format!("Stencil {i}"), "masters": []})).collect::<Vec<_>>()});
    let mut w = window_with(Some(list));
    let at = w.state().element("shapes.more").unwrap().rect.center();
    click(&mut w, at);
    w.rest();
    let catalogue = w.state().element("shapes.catalogue").unwrap().rect;
    let open = w.state().element("shapes.more.open").unwrap().rect;
    assert!(catalogue.bottom() <= open.top());
    assert!(open.bottom() <= 900.0);
    move_to(&mut w, catalogue.center());
    w.input_mut().events.push(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, -4000.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
        source: MouseWheelSource::Unknown,
    });
    for _ in 0..30 {
        w.step();
    }
    w.rest();
    let last = w.state().element("shapes.more.Stencil 63").unwrap().rect;
    assert!(catalogue.contains_rect(last));
    click(&mut w, last.center());
    w.rest();
    assert_eq!(w.state().window_state()["stencil"], "s63");
}

#[test]
fn pasted_and_api_queries_share_control_normalization_before_the_unicode_cap() {
    use crate::editing::key;
    use egui::{Event, Key, Modifiers};
    let mut w = window_with(None);
    let revision = w.state().session().revision();
    for (input, expected) in [
        ("Rect\nangle".to_owned(), "Rect angle".to_owned()),
        ("R\0e\tct\r\nangle\u{85}".to_owned(), "Rect  angle".to_owned()),
        (format!("{}\0\tΩ€Ж", "é".repeat(126)), format!("{}Ω€", "é".repeat(126))),
        (format!("{}\r\n\0Ω€Ж", "é".repeat(125)), format!("{}  Ω", "é".repeat(125))),
        (format!("{}Rectangle", "\0".repeat(4_096)), String::new()),
        (format!("{}ébc", "\0".repeat(4_095)), "é".to_owned()),
    ] {
        run_command(&mut w, "window.shapeFilter", json!({"query": input}));
        let api_query = w.state().window_state()["shapeFilter"].clone();
        assert_eq!(api_query, expected);
        run_command(&mut w, "window.shapeFilter", json!({"query": ""}));
        let filter = w.state().element("shapes.filter").unwrap().rect.center();
        click(&mut w, filter);
        w.input_mut().events.push(Event::Paste(input));
        w.step();
        w.rest();
        assert_eq!(w.state().window_state()["shapeFilter"], api_query, "paste and API apply the same policy before the cap");
        key(&mut w, Key::Escape, Modifiers::NONE);
    }
    assert_eq!(w.state().session().revision(), revision);
    assert!(w.state().command_enabled("edit.undo").is_err());
}

#[test]
fn a_template_refreshes_the_collapsed_gallery_state_before_another_frame() {
    let mut w = window_with(None);
    run_command(&mut w, "window.shapeFilter", json!({"query": "rect"}));
    run_command(&mut w, "window.shapes", json!({"shown": false}));
    assert!(w.state().element("shapes.filter").is_none());
    w.state_mut().execute("file.new", &json!({"template": "basic_flowchart"})).unwrap();
    let state = w.state().window_state();
    assert_eq!(state["shapesCollapsed"], true);
    assert_eq!(state["stencil"], "flowchart");
    assert_eq!(state["shapeFilter"], "");
    assert_eq!(state["shapeFilterCount"], 18);
    let revision = w.state().session().revision();
    let filtered = w.state_mut().execute("window.shapeFilter", &json!({"query": "process"})).unwrap();
    assert_eq!(filtered["stencil"], "flowchart");
    assert_eq!(filtered["shapeFilterCount"], 2);
    assert_eq!(w.state().session().revision(), revision);
    w.state_mut().execute("file.new", &json!({"template": "basic_diagram"})).unwrap();
    let state = w.state().window_state();
    assert_eq!(state["stencil"], "basic");
    assert_eq!(state["shapeFilter"], "");
    assert_eq!(state["shapeFilterCount"], 12);
    w.rest();
    let rendered = w.state().window_state();
    for field in ["stencil", "shapeFilter", "shapeFilterCount"] {
        assert_eq!(rendered[field], state[field], "rendering the collapsed strip is not needed to synchronize {field}");
    }
}

#[test]
fn uniform_square_cards_keep_size_order_and_bounds_across_resizing_filters_and_stencils() {
    use crate::support::drag;
    use diacraft_ui_egui::canvas::grid::RULER_THICKNESS;
    let names = ["A", "Long wrapped caption", "B", "Rectangle", "Tiny", "Another long shape name", "C"];
    let other_names = ["Other", "Another very long caption"];
    let list = json!({"stencils": [
        {"id": "packing", "name": "Packing", "masters": names.iter().enumerate().map(|(i, name)| json!({"id": format!("m{i}"), "name": name})).collect::<Vec<_>>()},
        {"id": "other", "name": "Other", "masters": other_names.iter().enumerate().map(|(i, name)| json!({"id": format!("other{i}"), "name": name})).collect::<Vec<_>>()}
    ]});
    let mut w = window_with(Some(list));
    const TILE: f32 = 72.0;
    let check = |w: &Window, expected: &[&str]| {
        let gallery = w.state().element("shapes.gallery").unwrap().rect;
        let cards: Vec<_> = expected.iter().map(|name| w.state().element(&format!("shapes.tile.{name}")).unwrap().rect).collect();
        assert_eq!(tile_names(w), expected, "the grid preserves catalogue order");
        for (i, card) in cards.iter().enumerate() {
            assert!(card.is_finite());
            assert_eq!(card.size(), egui::vec2(TILE, TILE), "every name uses the same compact square");
            assert!(gallery.contains_rect(*card), "{} escapes the gallery: {card:?} in {gallery:?}", expected[i]);
            for other in cards.iter().skip(i + 1) {
                assert!(!card.intersects(*other), "cards overlap: {card:?} and {other:?}");
            }
        }
        cards
    };
    // 200 more than the default is past the widest the panel is made (360), where four 72-point tiles fit and three of the 80 it was did.
    for (width, resize_by, columns) in [(1280.0, 0.0, 2), (1280.0, 108.0, 3), (1280.0, 200.0, 4), (480.0, 0.0, 1), (320.0, 0.0, 1)] {
        w.set_size(egui::vec2(width, 1200.0));
        w.rest();
        if resize_by > 0.0 {
            let canvas = w.state().element("canvas").unwrap().rect;
            let divider = egui::pos2(canvas.left() - RULER_THICKNESS, canvas.center().y);
            drag(&mut w, divider, egui::vec2(resize_by, 0.0));
            w.rest();
        }
        let cards = check(&w, &names);
        let gallery = w.state().element("shapes.gallery").unwrap().rect;
        if width == 1280.0 && resize_by == 0.0 {
            assert_eq!(gallery.width(), 174.0, "the default panel snugly fits two square cards");
            assert_eq!(gallery.right() - cards[1].right(), cards[0].left() - gallery.left(), "the trailing gutter matches the leading inset");
        }
        let first_row = cards.iter().take_while(|card| card.top() == cards[0].top()).count();
        assert_eq!(first_row, columns, "available panel width changes column count, never card size");
        run_command(&mut w, "window.shapeFilter", json!({"query": "long"}));
        check(&w, &["Long wrapped caption", "Another long shape name"]);
        run_command(&mut w, "window.stencil", json!({"stencil": "other"}));
        check(&w, &other_names);
        run_command(&mut w, "window.stencil", json!({"stencil": "packing"}));
        assert_eq!(
            w.state().element("shapes.gallery").unwrap().rect.width(),
            gallery.width(),
            "filtering and stencil choices preserve the resized panel width"
        );
    }
    assert_eq!(w.state().ui_panics(), 0);
}

/// Owner, 2026-11-23: a tile is 72 points square at the default scale, a tenth less than the 80 it was, in every stencil of the engine and in the
/// Quick Shapes group above the divider as well as below it, and the default panel still fits two of them with the same margin at each side.
#[test]
fn a_tile_is_72_points_square_at_the_default_scale() {
    let mut w = window_with(None);
    assert_eq!(w.ctx.pixels_per_point(), 1.0, "the harness draws at the default scale");
    for stencil in ["basic", "flowchart"] {
        run_command(&mut w, "window.stencil", json!({"stencil": stencil}));
        let tiles = elements_named(&w, "shapes.tile.");
        assert!(tiles.len() >= 12, "{stencil}: {} tiles", tiles.len());
        for (name, rect, _) in &tiles {
            assert_eq!(rect.size(), egui::vec2(72.0, 72.0), "{stencil}: {name}");
        }
        let gallery = w.state().element("shapes.gallery").unwrap().rect;
        assert_eq!(gallery.width(), 174.0, "{stencil}: the margin of 12 each side, two tiles and the 6-point gap");
        let mut lefts: Vec<f32> = tiles.iter().map(|(_, r, _)| r.min.x - gallery.min.x).collect();
        lefts.sort_by(f32::total_cmp);
        lefts.dedup();
        assert_eq!(lefts, [12.0, 90.0], "{stencil}: two columns, 72 and a 6-point gap apart");
    }
}

/// The Quick Shapes of a stencil and the Quick Shapes stencil (which gathers them) and the results of a search draw the same tiles as the stencil itself: one side.
#[test]
fn the_quick_shapes_and_the_search_results_have_the_same_tile_side() {
    let mut w = window_with(None);
    run_command(&mut w, "window.stencil", json!({"stencil": "basic"}));
    let divider = w.state().element("shapes.quickDivider").expect("the first masters of a stencil are its Quick Shapes, above a divider").rect;
    let above: Vec<_> = elements_named(&w, "shapes.tile.").into_iter().filter(|(_, r, _)| r.max.y < divider.min.y).collect();
    assert!(!above.is_empty(), "some tiles stand above the divider");
    assert!(above.iter().all(|(_, r, _)| r.size() == egui::vec2(72.0, 72.0)));
    run_command(&mut w, "window.stencil", json!({"stencil": "quick"}));
    let gathered = elements_named(&w, "shapes.tile.");
    assert!(!gathered.is_empty());
    assert!(gathered.iter().all(|(_, r, _)| r.size() == egui::vec2(72.0, 72.0)));
    run_command(&mut w, "shapes.search", json!({"text": "rectangle"}));
    let found = elements_named(&w, "shapes.tile.");
    assert!(!found.is_empty());
    assert!(found.iter().all(|(_, r, _)| r.size() == egui::vec2(72.0, 72.0)));
}

#[test]
fn shape_names_remain_accessible_and_appear_on_hover() {
    use crate::support::move_to;
    use egui_kittest::kittest::{By, NodeT, Queryable};
    let name = "A long descriptive master name";
    let list = json!({"stencils": [{"id": "names", "name": "Names", "masters": [{"id": "rectangle", "name": name}]}]});
    let mut w = window_with(Some(list));
    w.get_by_label(name);
    let revision = w.state().session().revision();
    let at = w.state().element(&format!("shapes.tile.{name}")).unwrap().rect.center();
    move_to(&mut w, at);
    for _ in 0..60 {
        w.step();
    }
    assert!(
        w.query_all(By::new().role(egui::accesskit::Role::Label)).any(|n| n.accesskit_node().value().as_deref() == Some(name)),
        "hover shows the full master name even though the card has no visible caption"
    );
    assert_eq!(w.state().session().revision(), revision);
    run_command(&mut w, "window.shapeFilter", json!({"query": "descriptive"}));
    assert_eq!(tile_names(&w), [name]);
}
