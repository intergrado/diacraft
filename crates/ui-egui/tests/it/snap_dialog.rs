//! The Snap & Glue dialog, the View tab's Visual Aids group, and the Glue switch in the window's own pick of a connector's end. The pointer and
//! the keys are given to egui as a person's would be, every window has the manual clock, and what is checked is the document the commands changed:
//! `snap.settings`, the journal, the `Connect` rows, and where a drag landed.

use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use diacraft_ui_egui::snap_dialog::boxed_options;
use diacraft_ui_egui::{ControlRequest, work};
use egui::accesskit::Toggled;
use egui::{Key, Modifiers, Pos2, Rect, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::{Value, json};

use crate::editing::{at, drag_tile_to, drag_with, edit_window, hold, key, shape_ids, three_shapes};
use crate::fixture;
use crate::guides::{add_guide, bounds, near, press_and_drag, rectangle_and_guide};
use crate::support::{Rest, TempDir, Window, button, controlled_with, elements_named, press, run_command, send};

/// The document's settings, as `snap.settings` answers (a query: it changes and journals nothing).
fn settings(w: &mut Window) -> Value {
    w.state_mut().execute("snap.settings", &json!({})).unwrap()
}

fn toggled(w: &Window, label: &str) -> Option<Toggled> {
    w.get_by_label(label).accesskit_node().toggled()
}

fn open(w: &mut Window, tab: &str) {
    run_command(w, "window.snapGlue", json!({"tab": tab}));
    assert_eq!(w.state().window_state()["snapGlue"], tab);
}

/// A press on the dialog's box for `option`.
fn tick(w: &mut Window, option: &str) {
    press(w, &format!("snapglue.option.{option}"));
}

fn ok(w: &mut Window) {
    press(w, "snapglue.ok");
    w.rest();
}

/// The entries of the session's journal.
fn journal(w: &Window) -> Vec<(String, Value)> {
    w.state().session().journal().to_vec()
}

fn connects_from(w: &Window, connector: u32) -> usize {
    let page = w.state().session().document().unwrap().pages.get(0).unwrap();
    page.connects.iter().filter(|c| c.from_sheet == connector).count()
}

fn rect_of(w: &Window, name: &str) -> Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

/// The launcher of the Visual Aids group and Alt+F9 open the dialog on General, and every box shows the document's own value, on both tabs.
#[test]
fn the_launcher_and_alt_open_the_dialog_with_the_documents_values() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(
        &mut w,
        "snap.set",
        json!({"snap": false, "grid": false, "snap_handles": true, "glue_geometry": true, "glue_guides": false, "ext_isometric": true, "ext_linear": false}),
    );
    let wanted = settings(&mut w);
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    let launcher = w.state().element("ribbon.View.SnapAndGlueDialog").expect("the Visual Aids group has its launcher").clone();
    assert!(launcher.enabled);
    press(&mut w, "ribbon.View.SnapAndGlueDialog");
    w.rest();
    assert_eq!(w.state().window_state()["snapGlue"], "General", "the launcher opens General");
    for name in boxed_options() {
        assert_eq!(w.state().snap_dialog_shows(name), wanted[name].as_bool(), "{name}");
    }
    // What is drawn says it too, to a screen reader as well.
    // (The View tab behind the dialog has a Snap box of its own: the dialog's is the one inside the dialog.)
    let in_dialog = w.state().element("snapglue.option.snap").unwrap().rect;
    let snap_in_dialog = w.get_all_by_label("Snap").find(|n| in_dialog.contains(n.rect().center())).expect("the dialog's Snap box");
    assert_eq!(snap_in_dialog.accesskit_node().toggled(), Some(Toggled::False));
    assert_eq!(toggled(&w, "Glue"), Some(Toggled::True));
    assert_eq!(toggled(&w, "Snap to grid"), Some(Toggled::False));
    assert_eq!(toggled(&w, "Snap to selection handles"), Some(Toggled::True));
    assert_eq!(toggled(&w, "Snap to guides"), Some(Toggled::True));
    assert_eq!(toggled(&w, "Glue to guides"), Some(Toggled::False));
    assert_eq!(toggled(&w, "Glue to shape geometry"), Some(Toggled::True));
    for option in ["snap", "glue", "ruler", "glue_connection_points"] {
        assert!(w.state().element(&format!("snapglue.option.{option}")).is_some(), "{option} is on General");
    }
    press(&mut w, "snapglue.tab.Advanced");
    assert_eq!(w.state().window_state()["snapGlue"], "Advanced");
    assert_eq!(toggled(&w, "Isometric angle lines"), Some(Toggled::True));
    assert_eq!(toggled(&w, "Linear extension"), Some(Toggled::False));
    assert_eq!(toggled(&w, "Center axes"), Some(Toggled::True));
    assert!(w.state().element("snapglue.option.grid").is_none(), "the targets are on General only");
    press(&mut w, "snapglue.cancel");
    w.rest();
    assert!(w.state().window_state()["snapGlue"].is_null());
    // Alt+F9.
    key(&mut w, Key::F9, Modifiers::ALT);
    assert_eq!(w.state().window_state()["snapGlue"], "General", "Alt+F9 opens the dialog");
    assert!(w.state_mut().execute("window.snapGlue", &json!({"tab": "Nothing"})).is_err());
    run_command(&mut w, "window.snapGlue", json!({"close": true}));
    assert!(w.state().window_state()["snapGlue"].is_null());
}

/// OK sends one `snap.set` with what was changed, which is no undo step; nothing goes before it. Cancel, Escape, and OK with nothing changed send nothing. Enter
/// is OK while no control has the keyboard.
#[test]
fn ok_sends_one_snap_set_and_cancel_sends_nothing() {
    let mut w = edit_window([1280.0, 900.0]);
    let before = settings(&mut w);
    let entries = journal(&w).len();
    open(&mut w, "General");
    tick(&mut w, "grid");
    tick(&mut w, "glue_vertices");
    press(&mut w, "snapglue.tab.Advanced");
    tick(&mut w, "ext_curved");
    // A box ticked twice is as it was, and is not sent.
    tick(&mut w, "ext_tangents");
    tick(&mut w, "ext_tangents");
    assert_eq!(journal(&w).len(), entries, "the dialog has sent nothing yet");
    assert_eq!(settings(&mut w), before);
    ok(&mut w);
    assert!(w.state().window_state()["snapGlue"].is_null(), "OK closes the dialog");
    let sent = journal(&w);
    assert_eq!(sent.len(), entries + 1, "one command: {sent:?}");
    let (id, params) = sent.last().unwrap();
    assert_eq!(id, "snap.set");
    assert_eq!(params, &json!({"grid": false, "glue_vertices": true, "ext_curved": true}));
    let after = settings(&mut w);
    assert_eq!(
        (&after["grid"], &after["glue_vertices"], &after["ext_curved"], &after["ext_tangents"]),
        (&json!(false), &json!(true), &json!(true), &json!(false))
    );
    assert_eq!(w.state().session().history().unwrap().undo_len(), 0, "no undo step");
    assert!(w.state().is_dirty(), "the drawing is unsaved");
    assert!(w.state_mut().execute("edit.undo", &json!({})).is_err(), "nothing to undo");
    assert_eq!(settings(&mut w), after, "Ctrl+Z leaves every option as OK set it");
    let before = after;

    let revision = w.state().session().revision();
    for close in ["cancel", "escape", "nothing"] {
        open(&mut w, "General");
        if close != "nothing" {
            tick(&mut w, "snap");
            assert_eq!(w.state().snap_dialog_shows("snap"), Some(false));
        }
        match close {
            "cancel" => press(&mut w, "snapglue.cancel"),
            "escape" => key(&mut w, Key::Escape, Modifiers::NONE),
            _ => ok(&mut w),
        }
        w.rest();
        assert!(w.state().window_state()["snapGlue"].is_null(), "{close} closes the dialog");
        assert_eq!(w.state().session().revision(), revision, "{close} sends nothing");
    }
    assert_eq!(settings(&mut w), before);
    // Enter, with no control holding the keyboard, is OK.
    open(&mut w, "General");
    tick(&mut w, "guides");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert!(w.state().window_state()["snapGlue"].is_null(), "Enter is OK");
    assert_eq!(settings(&mut w)["guides"], false);
}

/// A target turned off in the dialog stops a drag snapping to it, and turned on again the drag snaps to it again: a guide at x = 4.1, off the grid,
/// pulls the rectangle's left edge while Snap to guides is on; with it off the grid takes the edge instead.
#[test]
fn a_target_turned_off_stops_a_drag_snapping_to_it_and_on_again_resumes() {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 7.0, 4.0);
    let rectangle = shape_ids(&w)[0];
    assert!(near(bounds(&w, rectangle).0, 6.25, 1e-9), "{:?}", bounds(&w, rectangle));
    add_guide(&mut w, "vertical", 4.1);
    // The rectangle's left edge is at 6.25; the pointer takes it to 4.13, 0.03 from the guide and 0.12 from the grid's nearest line.
    let drag_left = |w: &mut Window| -> f64 {
        let b = bounds(w, rectangle);
        let middle = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
        let left = b.0;
        let (from, to) = (at(w, middle.0, middle.1), at(w, middle.0 - (left - 4.13), middle.1));
        press_and_drag(w, from, to);
        button(w, to, false);
        w.rest();
        bounds(w, rectangle).0
    };
    let landed = drag_left(&mut w);
    assert!(near(landed, 4.1, 1e-9), "Snap to guides on: the edge goes to the guide: {landed}");
    // Back to where it was, then Snap to guides turned off in the dialog.
    run_command(&mut w, "edit.undo", json!({}));
    assert!(near(bounds(&w, rectangle).0, 6.25, 1e-9));
    open(&mut w, "General");
    tick(&mut w, "guides");
    ok(&mut w);
    assert_eq!(settings(&mut w)["guides"], false);
    let landed = drag_left(&mut w);
    assert!(!near(landed, 4.1, 1e-6) && landed < 5.0, "Snap to guides off: the rectangle moved and the guide did not pull it: {landed}");
    run_command(&mut w, "edit.undo", json!({}));
    open(&mut w, "General");
    tick(&mut w, "guides");
    ok(&mut w);
    let landed = drag_left(&mut w);
    assert!(near(landed, 4.1, 1e-9), "on again, it pulls again: {landed}");
}

/// The Snap and Glue switches toggle from the dialog, and the Snap switch off leaves a drag where the pointer puts it.
#[test]
fn the_snap_and_glue_switches_toggle() {
    let mut w = edit_window([1280.0, 900.0]);
    let (rectangle, _) = rectangle_and_guide(&mut w);
    for (option, on) in [("snap", false), ("glue", false), ("snap", true), ("glue", true)] {
        open(&mut w, "General");
        assert_eq!(w.state().snap_dialog_shows(option), Some(!on), "the box shows the switch as the document has it");
        tick(&mut w, option);
        ok(&mut w);
        assert_eq!(settings(&mut w)[option], on, "{option} is {on}");
    }
    // Snap off: a drag lands where the pointer puts it, off the grid.
    open(&mut w, "General");
    tick(&mut w, "snap");
    ok(&mut w);
    let before = bounds(&w, rectangle);
    let (from, to) = (at(&w, 7.0, 4.0), at(&w, 7.0 - 1.13, 4.0 + 0.07));
    press_and_drag(&mut w, from, to);
    let delta = w.state().drag_preview().unwrap().delta.unwrap();
    button(&mut w, to, false);
    w.rest();
    let after = bounds(&w, rectangle);
    assert!(near(after.0 - before.0, delta.0, 1e-9) && near(after.1 - before.1, delta.1, 1e-9), "the release is the preview");
    assert!(near(after.0 - before.0, -1.13, 0.02) && near(after.1 - before.1, 0.07, 0.02), "nothing pulled it: {before:?} {after:?}");
}

/// The Visual Aids group: Dynamic Grid shows the document's `dynamic_grid` and each press is one `snap.set`; AutoConnect is the drawing's box
/// Connection Points marks the shapes' points whatever the tool, a window setting.
#[test]
fn the_visual_aids_boxes() {
    let mut w = edit_window([1280.0, 900.0]);
    // Shapes a program put on the page, none selected and the pointer never on the page: nothing has asked the window for its scene of the page yet.
    let page = w.state().session().document().unwrap().pages.get(0).unwrap().id;
    for (x, y) in [(3.0, 6.0), (7.0, 6.0), (5.0, 3.0)] {
        w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": page, "x": x, "y": y})).unwrap();
    }
    assert!(w.state().selection().is_empty());
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    assert_eq!(toggled(&w, "Dynamic Grid"), Some(Toggled::True), "a new drawing has the dynamic grid on");
    let entries = journal(&w).len();
    press(&mut w, "ribbon.View.DynamicGrid");
    w.rest();
    assert_eq!(settings(&mut w)["dynamic_grid"], false);
    assert_eq!(toggled(&w, "Dynamic Grid"), Some(Toggled::False));
    assert_eq!(journal(&w).len(), entries + 1);
    assert_eq!(journal(&w).last().unwrap(), &("snap.set".to_owned(), json!({"dynamic_grid": false})));
    press(&mut w, "ribbon.View.DynamicGrid");
    w.rest();
    assert_eq!(settings(&mut w)["dynamic_grid"], true);
    assert_eq!(toggled(&w, "Dynamic Grid"), Some(Toggled::True));
    press(&mut w, "ribbon.View.DynamicGrid");
    w.rest();
    assert_eq!(settings(&mut w)["dynamic_grid"], false);
    assert_eq!(toggled(&w, "Dynamic Grid"), Some(Toggled::False));
    // A press is no undo step: the history holds the three drops and nothing of the box, and the drawing is unsaved.
    assert_eq!(w.state().session().history().unwrap().undo_len(), 3, "the three drops only");
    assert!(w.state().is_dirty());
    // A setting changed by a program shows too.
    run_command(&mut w, "snap.set", json!({"dynamic_grid": true}));
    assert_eq!(toggled(&w, "Dynamic Grid"), Some(Toggled::True));

    // AutoConnect is the drawing's box (`autoconnect.rs` of the tests has its own checks): on in a drawing that holds nothing.
    assert!(w.state().element("ribbon.View.AutoConnect").unwrap().enabled);
    assert_eq!(toggled(&w, "AutoConnect"), Some(Toggled::True));

    // Connection Points: off, the Pointer tool marks none; on, the points of the shapes in view are marked.
    assert_eq!(w.state().window_state()["connectionPoints"], false);
    assert_eq!(toggled(&w, "Connection Points"), Some(Toggled::False));
    work::reset();
    w.step();
    assert_eq!(work::work().points_marked, 0);
    press(&mut w, "ribbon.View.ConnectionPoints");
    w.rest();
    assert_eq!(w.state().window_state()["connectionPoints"], true);
    assert_eq!(toggled(&w, "Connection Points"), Some(Toggled::True));
    work::reset();
    w.step();
    assert!(work::work().points_marked >= 3, "the three shapes' points are marked: {:?}", work::work());
    let revision = w.state().session().revision();
    press(&mut w, "ribbon.View.ConnectionPoints");
    w.rest();
    assert_eq!(w.state().window_state()["connectionPoints"], false);
    assert_eq!(w.state().session().revision(), revision, "the drawing's setting: a change a save keeps, and no new revision");
    work::reset();
    w.step();
    assert_eq!(work::work().points_marked, 0);
    // The Connector tool marks them with the box off, as it did before.
    run_command(&mut w, "tool.connector", json!({}));
    work::reset();
    w.step();
    assert!(work::work().points_marked >= 3);
}

/// The first connection point of shape `id`, in paper inches on the page.
fn first_point(w: &Window, id: u32) -> (f64, f64) {
    let doc = w.state().session().document().unwrap();
    let page = doc.pages.get(0).unwrap().id;
    let per_inch = diacraft_engine::draw::units_per_inch(doc, page);
    let point = diacraft_engine::draw::connection_points(doc, page, id).into_iter().next().expect("the shape has a connection point");
    (point.x / per_inch, point.y / per_inch)
}

/// The window's own pick of a connection point or a whole shape under a connector's end follows the Glue switch and Glue to connection points: on,
/// a drag from the middle of one shape (the whole shape) to a connection point of another glues both ends, and the preview shows the point glued; with
/// either off it glues neither end, and the preview shows none glued.
#[test]
fn a_connectors_ends_glue_only_with_the_glue_switch_and_glue_to_connection_points_on() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    let point = first_point(&w, ids[1]);
    run_command(&mut w, "tool.connector", json!({}));
    // From the rectangle's middle (the whole shape) to the diamond's first connection point, the button held to read the preview.
    let join = |w: &mut Window| -> (Option<(u32, u32)>, u32) {
        let (from, to) = (at(w, 3.0, 6.0), at(w, point.0, point.1));
        press_and_drag(w, from, to);
        let shown = w.state().drag_preview().unwrap().end_glued;
        button(w, to, false);
        w.rest();
        (shown, *shape_ids(w).last().unwrap())
    };
    let (shown, glued) = join(&mut w);
    assert_eq!(shown.map(|(id, _)| id), Some(ids[1]), "Glue on: the point under the pointer is shown glued");
    assert_eq!(connects_from(&w, glued), 2, "Glue on: both ends glued");
    for off in [json!({"glue": false}), json!({"glue": true, "glue_connection_points": false})] {
        run_command(&mut w, "snap.set", off.clone());
        let (shown, free) = join(&mut w);
        assert_eq!(shown, None, "{off}: the end is not shown glued");
        assert_ne!(free, glued);
        assert_eq!(connects_from(&w, free), 0, "{off}: neither end glued");
    }
    // And on again.
    run_command(&mut w, "snap.set", json!({"glue": true, "glue_connection_points": true}));
    let (_, again) = join(&mut w);
    assert_eq!(connects_from(&w, again), 2);
    // An end of a glued connector dragged to a shape with Glue off is let go there free.
    run_command(&mut w, "snap.set", json!({"glue": false}));
    run_command(&mut w, "tool.pointer", json!({}));
    run_command(&mut w, "edit.select", json!({"ids": [again]}));
    let end = w.state().connector_end_point(again, true).expect("an end handle");
    let end_on_page = w.state().page_point_at(end).unwrap();
    drag_with(&mut w, end_on_page, (5.0, 3.0), Modifiers::NONE);
    assert_eq!(connects_from(&w, again), 1, "the end let go on the ellipse is free; the other stays glued");
}

/// Runs `file.open` through the control channel, the window's own way of opening (a new session, revisions from 1 again).
fn open_file(window: &mut Window, tx: &Sender<ControlRequest>, path: &Path) {
    let answer = send(tx, "engine.execute", json!({"command": "file.open", "params": {"path": path.to_str().unwrap()}}));
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        window.step();
        if let Ok(reply) = answer.try_recv() {
            reply.unwrap();
            window.rest();
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the file was not opened in 30 s");
}

/// A file saved with settings that are not the defaults shows them when it is opened: in the dialog and in the Dynamic Grid box, after a drawing whose
/// settings were the defaults (the window's copy of the settings is kept for a drawing, not for a revision alone). The Connection Points box starts from
/// the file's `ShowConnectionPoints`, and off where it has none.
#[test]
fn a_file_opened_with_settings_that_are_not_the_defaults_shows_them() {
    let dir = TempDir::new("snap-settings");
    let path = dir.file("settings.vsdx");
    let mut s = diacraft_engine::Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("snap.set", &json!({"snap": false, "grid": false, "glue_vertices": true, "ext_curved": true, "dynamic_grid": false})).unwrap();
    s.execute("file.save_as", &json!({"path": path.to_str().unwrap()})).unwrap();
    let (mut w, tx) = controlled_with(|app| app);
    w.rest();
    let defaults = fixture::write(dir.path(), "defaults.vsdx", &fixture::demo_vsdx());
    open_file(&mut w, &tx, &defaults);
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    assert_eq!(toggled(&w, "Dynamic Grid"), Some(Toggled::True), "a file with the default words");
    open_file(&mut w, &tx, &path);
    assert_eq!(toggled(&w, "Dynamic Grid"), Some(Toggled::False), "the file's own DynamicGridEnabled");
    open(&mut w, "General");
    for (name, on) in [("snap", false), ("grid", false), ("glue_vertices", true), ("ext_curved", true), ("glue", true), ("guides", true)] {
        assert_eq!(w.state().snap_dialog_shows(name), Some(on), "{name}");
    }
    run_command(&mut w, "window.snapGlue", json!({"close": true}));
    // The window's Connection Points box from the file's drawing window.
    for (name, flag, shown) in [("points-on.vsdx", Some("1"), true), ("points-off.vsdx", Some("0"), false), ("points-none.vsdx", None, false)] {
        let xml = flag
            .map(|f| format!(r#"<Windows><Window ID="0" WindowType="Drawing"><ShowConnectionPoints>{f}</ShowConnectionPoints></Window></Windows>"#));
        let file = fixture::write(dir.path(), name, &fixture::demo_builder(xml.as_deref()).build());
        // The box was turned the other way before: the file decides.
        w.state_mut().execute("view.connectionPoints", &json!({"on": !shown})).unwrap();
        open_file(&mut w, &tx, &file);
        assert_eq!(w.state().window_state()["connectionPoints"], shown, "{name}");
    }
}

/// The dialog fits narrow and wide windows on both tabs: its buttons and every box are inside the window, no two boxes overlap, and the window comes to
/// rest (no frame asks for another for ever).
#[test]
fn the_dialog_fits_a_narrow_window_and_a_wide_one() {
    for size in [[480.0, 360.0], [480.0, 720.0], [900.0, 600.0], [1920.0, 1080.0]] {
        let mut w = crate::support::drawing_window(size);
        for tab in ["General", "Advanced"] {
            run_command(&mut w, "window.snapGlue", json!({"tab": tab}));
            w.rest();
            let screen = Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]));
            let (ok, cancel) = (rect_of(&w, "snapglue.ok"), rect_of(&w, "snapglue.cancel"));
            assert!(screen.contains_rect(ok) && screen.contains_rect(cancel), "the buttons are in a {size:?} window on {tab}: {ok:?} {cancel:?}");
            assert!(!ok.intersects(cancel));
            let content = rect_of(&w, "snapglue.content");
            assert!(screen.contains_rect(content), "the content of {tab} is in a {size:?} window: {content:?}");
            let boxes = elements_named(&w, "snapglue.option.");
            assert!(!boxes.is_empty());
            for (i, (name, rect, _)) in boxes.iter().enumerate() {
                assert!(
                    rect.min.x >= content.min.x - 0.5 && rect.max.x <= content.max.x + 0.5,
                    "{name} leaves the dialog in a {size:?} window: {rect:?} outside {content:?}"
                );
                for (other, r, _) in boxes.iter().skip(i + 1) {
                    assert!(!rect.shrink(0.5).intersects(r.shrink(0.5)), "{name} and {other} overlap in a {size:?} window: {rect:?} {r:?}");
                }
            }
            assert_eq!(w.state().ui_panics(), 0);
        }
        run_command(&mut w, "window.snapGlue", json!({"close": true}));
    }
}

/// An open dialog is a modal: a key meant for the drawing reaches nothing, and the shortcuts wait.
#[test]
fn an_open_dialog_is_a_modal() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    run_command(&mut w, "edit.select", json!({"ids": [ids[0]]}));
    open(&mut w, "General");
    key(&mut w, Key::Delete, Modifiers::NONE);
    assert_eq!(shape_ids(&w).len(), 3, "Delete reaches nothing under the dialog");
    hold(&mut w, Modifiers::NONE);
    assert_eq!(w.state().window_state()["snapGlue"], "General");
}

/// The View tab's controls never overlap, at narrow widths (where the groups are compact or folded) and wide ones, and **a check box is as wide as its
/// label at every size**: it is drawn with its label in a compact group too, and a column that counted only an icon's width drew the labels of the Show
/// and Visual Aids groups over the next column at 640 points.
#[test]
fn the_view_tabs_controls_do_not_overlap_at_any_width() {
    const CHECKS: [(&str, &str); 8] = [
        ("ShowRuler", "Ruler"),
        ("ShowGrid", "Grid"),
        ("ShowGuides", "Guides"),
        ("DynamicGrid", "Dynamic Grid"),
        ("AutoConnect", "AutoConnect"),
        ("ConnectionPoints", "Connection Points"),
        ("SnapToggle", "Snap"),
        ("SnapToGrid", "Snap to Grid"),
    ];
    for width in [560.0, 640.0, 720.0, 820.0, 960.0, 1280.0, 1920.0] {
        let mut w = crate::support::drawing_window([width, 700.0]);
        run_command(&mut w, "window.tab", json!({"tab": "View"}));
        let controls: Vec<(String, Rect, bool)> =
            elements_named(&w, "ribbon.View.").into_iter().filter(|(name, ..)| !name.starts_with("ribbon.View.group.")).collect();
        assert!(!controls.is_empty());
        for (i, (name, rect, _)) in controls.iter().enumerate() {
            for (other, r, _) in controls.iter().skip(i + 1) {
                assert!(!rect.shrink(0.5).intersects(r.shrink(0.5)), "{name} and {other} overlap at {width} points: {rect:?} {r:?}");
            }
        }
        // A check box drawn in its group (not folded away) holds its box and its label: 26 points to the label, then the label.
        for (id, label) in CHECKS {
            if let Some(drawn) = w.state().element(&format!("ribbon.View.{id}")) {
                let font = diacraft_ui_egui::theme::font(diacraft_ui_egui::theme::size::LABEL);
                let text = w.ctx.fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font, Default::default()).size().x);
                assert!(drawn.rect.width() >= 26.0 + text, "{id} is {} wide at {width} points, and its label {text}", drawn.rect.width());
            }
        }
    }
}

/// Opens `path` the window's way (`file.open` through `execute`, which reads the file's drawing window too) and lets the window rest.
fn open_path(w: &mut Window, path: &Path) {
    w.state_mut().execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    run_command(w, "view.fitPage", json!({}));
}

const POINTS_ON: &str = r#"<Windows><Window ID="0" WindowType="Drawing"><ShowConnectionPoints>1</ShowConnectionPoints></Window></Windows>"#;

/// Of the review: a file can turn View > Connection Points on, and the reader allows 320,000 rows a section. **Counted, not timed:** 300 instances
/// of a master of 10,000 connection rows have no section read (the rows are counted before they are merged, as the snap counts them, and a section of
/// more than 256 is skipped), with the Pointer tool and with the Connector tool over a shape; 300 instances of 200 rows (60,000 points) mark
/// `MAX_POINTS_MARKED` a frame.
#[test]
fn connection_points_from_a_file_cost_a_bounded_read_and_a_bounded_frame() {
    let dir = TempDir::new("points-cost");
    let mut w = edit_window([1280.0, 900.0]);
    let many = fixture::write(dir.path(), "many-rows.vsdx", &fixture::pointed_instances_vsdx(10_000, 300, Some(POINTS_ON)));
    diacraft_doc::work::reset();
    work::reset();
    open_path(&mut w, &many);
    assert_eq!(w.state().window_state()["connectionPoints"], true, "the file turned the box on");
    let merged = diacraft_doc::work::rows_merged();
    // Reading every section would merge 300 x 10,000 rows.
    assert!(merged < 30_000, "{merged} rows merged opening and drawing the page");
    assert_eq!(work::work().points_marked, 0, "no section of 10,000 rows is read");
    // The Connector tool over an instance reads the points the same bounded way.
    run_command(&mut w, "tool.connector", json!({}));
    diacraft_doc::work::reset();
    let over = at(&w, 0.45, 0.45);
    for _ in 0..3 {
        crate::support::move_to(&mut w, over);
    }
    assert!(diacraft_doc::work::rows_merged() < 1_000, "{} rows merged with the Connector tool over a shape", diacraft_doc::work::rows_merged());
    run_command(&mut w, "tool.pointer", json!({}));

    // 200 rows: read (under 256), and 60,000 points in view, of which a frame marks MAX_POINTS_MARKED.
    let some = fixture::write(dir.path(), "some-rows.vsdx", &fixture::pointed_instances_vsdx(200, 300, Some(POINTS_ON)));
    open_path(&mut w, &some);
    work::reset();
    w.step();
    assert_eq!(work::work().points_marked, diacraft_ui_egui::edit::MAX_POINTS_MARKED, "a frame marks the budget and no more");
    // The Connector tool with the pointer on a point: the budget, and the point lit besides it.
    run_command(&mut w, "tool.connector", json!({}));
    let point = at(&w, 0.3, 0.3);
    for _ in 0..3 {
        crate::support::move_to(&mut w, point);
    }
    work::reset();
    w.step();
    assert_eq!(work::work().points_marked, diacraft_ui_egui::edit::MAX_POINTS_MARKED + 1);
}

/// A: OK refused (the file's `SnapSettings` is text, so the grid's flag cannot be written) leaves the dialog open, saying why, and changes
/// nothing; Cancel then closes it.
#[test]
fn a_refused_ok_keeps_the_dialog_open_and_says_why() {
    let dir = TempDir::new("snap-refused");
    let bytes = fixture::demo_builder(None).document_extra("<DocumentSettings><SnapSettings>abc</SnapSettings></DocumentSettings>").build();
    let path = fixture::write(dir.path(), "text-word.vsdx", &bytes);
    let mut w = edit_window([1280.0, 900.0]);
    open_path(&mut w, &path);
    let revision = w.state().session().revision();
    open(&mut w, "General");
    tick(&mut w, "grid");
    ok(&mut w);
    assert_eq!(w.state().window_state()["snapGlue"], "General", "the dialog stays open");
    let said = w.state().snap_dialog_error().expect("the dialog says why").to_owned();
    assert!(said.contains("DocumentSettings") && said.contains("cannot be changed"), "the engine's words, why it refused: {said}");
    assert!(w.state().element("snapglue.error").is_some(), "and shows it: {said}");
    assert_eq!(w.state().session().revision(), revision, "nothing changed");
    assert_eq!(w.state().snap_dialog_shows("grid"), Some(false), "the box keeps what was chosen");
    press(&mut w, "snapglue.cancel");
    w.rest();
    assert!(w.state().window_state()["snapGlue"].is_null());
    assert_eq!(w.state().session().revision(), revision);
}

/// B: with a box reached by Tab, Enter ticks the box and does not press OK.
#[test]
fn enter_on_a_box_reached_by_tab_ticks_it_and_is_not_ok() {
    let mut w = edit_window([1280.0, 900.0]);
    let revision = w.state().session().revision();
    open(&mut w, "General");
    for _ in 0..12 {
        if w.get_by_label("Snap").accesskit_node().is_focused() {
            break;
        }
        key(&mut w, Key::Tab, Modifiers::NONE);
    }
    assert!(w.get_by_label("Snap").accesskit_node().is_focused(), "Tab reaches the Snap box");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert_eq!(w.state().window_state()["snapGlue"], "General", "Enter on a box is not OK");
    assert_eq!(w.state().snap_dialog_shows("snap"), Some(false), "Enter ticked the box");
    assert_eq!(w.state().session().revision(), revision, "nothing sent");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(w.state().window_state()["snapGlue"].is_null());
    assert_eq!(w.state().session().revision(), revision);
}

/// C: another drawing (`file.new`, `file.open`) closes the dialog, and nothing of its draft is written to the new drawing afterwards.
#[test]
fn another_drawing_closes_the_dialog_and_nothing_is_written_after() {
    let dir = TempDir::new("snap-replaced");
    let path = fixture::write(dir.path(), "other.vsdx", &fixture::demo_vsdx());
    let mut w = edit_window([1280.0, 900.0]);
    for replace in ["file.new", "file.open"] {
        open(&mut w, "General");
        tick(&mut w, "grid");
        tick(&mut w, "glue");
        match replace {
            "file.new" => run_command(&mut w, "file.new", json!({})),
            _ => open_path(&mut w, &path),
        }
        assert!(w.state().window_state()["snapGlue"].is_null(), "{replace} closes the dialog");
        let revision = w.state().session().revision();
        let before = settings(&mut w);
        // The keys the dialog took reach nothing of it now.
        key(&mut w, Key::Enter, Modifiers::NONE);
        w.rest();
        assert_eq!(w.state().session().revision(), revision, "{replace}: nothing written after");
        assert_eq!(settings(&mut w), before);
        assert_eq!((&before["grid"], &before["glue"]), (&json!(true), &json!(true)), "{replace}: the new drawing's own settings");
    }
}

/// A new drawing starts with the View tab's toggles at a new drawing's defaults, not where the last file left them: Connection
/// Points off, the grid, the rulers and the guides on.
#[test]
fn a_new_drawing_starts_with_the_view_toggles_of_a_new_drawing() {
    let dir = TempDir::new("view-reset");
    let windows = r#"<Windows><Window ID="0" WindowType="Drawing"><ShowRulers>0</ShowRulers><ShowGrid>0</ShowGrid><ShowGuides>0</ShowGuides><ShowConnectionPoints>1</ShowConnectionPoints></Window></Windows>"#;
    let path = fixture::write(dir.path(), "toggled.vsdx", &fixture::demo_builder(Some(windows)).build());
    let mut w = edit_window([1280.0, 900.0]);
    open_path(&mut w, &path);
    let state = w.state().window_state();
    assert_eq!(
        (&state["connectionPoints"], &state["grid"], &state["rulers"], &state["guides"]),
        (&json!(true), &json!(false), &json!(false), &json!(false))
    );
    // File > New: the Blank drawing.
    run_command(&mut w, "window.backstage", json!({"page": "New"}));
    press(&mut w, "backstage.tile.blank");
    w.rest();
    let state = w.state().window_state();
    assert!(state["backstage"].is_null(), "a new drawing is shown");
    assert_eq!(
        (&state["connectionPoints"], &state["grid"], &state["rulers"], &state["guides"]),
        (&json!(false), &json!(true), &json!(true), &json!(true))
    );
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    assert_eq!(toggled(&w, "Connection Points"), Some(Toggled::False));
}

/// General says, under Glue to, which boxes are saved and do not act yet.
#[test]
fn general_says_which_boxes_are_saved_and_do_not_act_yet() {
    let mut w = edit_window([1280.0, 900.0]);
    open(&mut w, "General");
    let note = rect_of(&w, "snapglue.note");
    let content = rect_of(&w, "snapglue.content");
    assert!(content.contains_rect(note), "the note shows without scrolling: {note:?} in {content:?}");
    let geometry = rect_of(&w, "snapglue.option.glue_geometry");
    assert!(note.min.y >= geometry.max.y && note.min.x >= geometry.min.x - 0.5, "under Glue to: {note:?} {geometry:?}");
    w.get_by_label(diacraft_ui_egui::snap_dialog::GENERAL_NOTE);
}

// ----: the six snap strength sliders ---------------------------------------------------------------------------------------------------

/// The six rows of the Advanced tab, in order: the key of the row's elements, the label, the engine's name, and the default in pixels.
const ROWS: [(&str, &str, &str, u64); 6] = [
    ("ruler", "Rulers", "strength_ruler", 4),
    ("grid", "Grid", "strength_grid", 5),
    ("guides", "Guides", "strength_guides", 10),
    ("points", "Points", "strength_points", 10),
    ("geometry", "Geometry", "strength_geometry", 8),
    ("extensions", "Extensions", "strength_extensions", 13),
];

/// What the number box of the row `key` says: the text of the text field inside the element the dialog named.
fn box_text(w: &Window, key: &str) -> String {
    let rect = rect_of(w, &format!("snapglue.strength.{key}.box"));
    w.get_all_by_role(egui::accesskit::Role::TextInput)
        .find(|n| rect.contains(n.rect().center()))
        .and_then(|n| n.value())
        .unwrap_or_else(|| panic!("the box of {key} says nothing"))
}

/// Types `text` into the number box of the row `key`, over what it held.
fn type_into_box(w: &mut Window, key: &str, text: &str) {
    press(w, &format!("snapglue.strength.{key}.box"));
    key_press(w, Key::A, Modifiers::COMMAND);
    crate::editing::type_text(w, text);
}

fn key_press(w: &mut Window, k: Key, modifiers: Modifiers) {
    key(w, k, modifiers);
}

/// The Advanced tab has the six rows in the spec's order, each a label, a slider and a number box, at the engine's numbers, under Weak, Strong and Pixels.
#[test]
fn the_advanced_tab_shows_the_six_strengths_at_their_defaults() {
    let mut w = edit_window([1280.0, 900.0]);
    open(&mut w, "Advanced");
    let wanted = settings(&mut w);
    let mut tops = Vec::new();
    for (key, label, name, default) in ROWS {
        assert_eq!(wanted[name], json!(default), "{name}: the engine has the default");
        assert_eq!(w.state().snap_dialog_strength(name), Some(u16::try_from(default).unwrap()), "{name}: the dialog shows the engine's number");
        assert_eq!(box_text(&w, key), default.to_string(), "{label}: the box says it");
        assert!(w.get_all_by_label_contains(&format!("{label}:")).count() >= 1, "{label}: the row has its label");
        let accessible = format!("{label} snap strength");
        let slider = w.get_by_label(&accessible);
        assert_eq!(slider.accesskit_node().numeric_value(), Some(default as f64), "{label}: the slider is at it");
        let (s, b) = (rect_of(&w, &format!("snapglue.strength.{key}.slider")), rect_of(&w, &format!("snapglue.strength.{key}.box")));
        assert!(s.max.x <= b.min.x + 0.5 && (s.center().y - b.center().y).abs() < 4.0, "{label}: the slider then the box, on one line: {s:?} {b:?}");
        tops.push(s.min.y);
    }
    assert!(tops.windows(2).all(|p| p[0] < p[1]), "the rows are in the spec's order, top to bottom: {tops:?}");
    for heading in ["Weak", "Strong", "Pixels", "Snap strength"] {
        w.get_by_label(heading);
    }
    assert!(w.state().snap_dialog_error().is_none());
    // Nothing is sent by looking.
    assert_eq!(journal(&w).iter().filter(|(id, _)| id == "snap.set").count(), 0);
}

/// Changing a slider changes the draft and its number box together, and OK sends one `snap.set` with that strength alone; the engine then has it, and a drag by it.
#[test]
fn a_slider_changes_its_box_and_ok_sets_the_engine_strength() {
    let mut w = edit_window([1280.0, 900.0]);
    let entries = journal(&w).len();
    open(&mut w, "Advanced");
    w.get_by_label("Grid snap strength").set_value_accesskit(9.0);
    w.step();
    w.step();
    assert_eq!(w.state().snap_dialog_strength("strength_grid"), Some(9), "the slider moved the draft");
    assert_eq!(box_text(&w, "grid"), "9", "and the box");
    assert_eq!(settings(&mut w)["strength_grid"], json!(5), "the engine has not heard of it yet");
    assert_eq!(journal(&w).len(), entries, "nothing is sent before OK");
    // A screen reader's steps move a slider by one each (egui's own stepping, which the arrow keys share).
    w.get_by_label("Extensions snap strength").increment_accesskit();
    w.step();
    w.step();
    assert_eq!(w.state().snap_dialog_strength("strength_extensions"), Some(14), "one step up");
    w.get_by_label("Extensions snap strength").decrement_accesskit();
    w.step();
    w.step();
    w.get_by_label("Extensions snap strength").decrement_accesskit();
    w.step();
    w.step();
    assert_eq!(w.state().snap_dialog_strength("strength_extensions"), Some(12));
    // The slider's own end is 30 or so; a number past it is the box's.
    ok(&mut w);
    let sent = journal(&w);
    assert_eq!(sent.len(), entries + 1, "one command: {sent:?}");
    assert_eq!(sent.last().unwrap(), &("snap.set".to_owned(), json!({"strength_grid": 9, "strength_extensions": 12})));
    let after = settings(&mut w);
    assert_eq!((&after["strength_grid"], &after["strength_extensions"], &after["strength_ruler"]), (&json!(9), &json!(12), &json!(4)));
    // A new dialog starts from what the engine has now, and Cancel sends nothing.
    open(&mut w, "Advanced");
    assert_eq!(w.state().snap_dialog_strength("strength_grid"), Some(9));
    assert_eq!(box_text(&w, "grid"), "9");
    w.get_by_label("Grid snap strength").set_value_accesskit(20.0);
    w.step();
    press(&mut w, "snapglue.cancel");
    w.rest();
    assert_eq!(settings(&mut w)["strength_grid"], json!(9), "Cancel changed nothing");
    // No undo step: Ctrl+Z has nothing to take back, and the strengths stay.
    assert!(w.state_mut().execute("edit.undo", &json!({})).is_err(), "nothing to undo");
    assert_eq!(settings(&mut w)["strength_grid"], json!(9));
    assert!(w.state().is_dirty(), "the drawing is unsaved");
}

/// A number typed in the box is the strength: the slider follows it (to its end for a number past its travel), OK sends it, and 0 and 999 are good.
#[test]
fn a_number_typed_in_the_box_moves_the_slider_and_sets_the_engine_strength() {
    let mut w = edit_window([1280.0, 900.0]);
    open(&mut w, "Advanced");
    type_into_box(&mut w, "geometry", "20");
    assert_eq!(box_text(&w, "geometry"), "20");
    assert_eq!(w.state().snap_dialog_strength("strength_geometry"), Some(20));
    assert_eq!(w.get_by_label("Geometry snap strength").accesskit_node().numeric_value(), Some(20.0), "the slider follows the box");
    type_into_box(&mut w, "guides", "999");
    type_into_box(&mut w, "ruler", "0");
    assert_eq!(w.state().snap_dialog_strength("strength_guides"), Some(999));
    assert_eq!(w.state().snap_dialog_strength("strength_ruler"), Some(0));
    ok(&mut w);
    assert!(w.state().window_state()["snapGlue"].is_null(), "OK closes the dialog");
    let after = settings(&mut w);
    assert_eq!(
        (&after["strength_geometry"], &after["strength_guides"], &after["strength_ruler"], &after["strength_points"]),
        (&json!(20), &json!(999), &json!(0), &json!(10))
    );
    assert_eq!(journal(&w).last().unwrap(), &("snap.set".to_owned(), json!({"strength_ruler": 0, "strength_guides": 999, "strength_geometry": 20})));
}

/// A box that says no whole number of pixels from 0 to 999 is marked and OK is refused in words that name the row; the dialog stays open, sends nothing and keeps what was
/// typed. A good number then lets OK through.
#[test]
fn a_number_that_is_not_a_whole_number_of_pixels_from_0_to_999_is_refused_in_words() {
    let mut w = edit_window([1280.0, 900.0]);
    let revision = w.state().session().revision();
    open(&mut w, "Advanced");
    for bad in ["1000", "", "ab"] {
        type_into_box(&mut w, "grid", bad);
        ok(&mut w);
        assert_eq!(w.state().window_state()["snapGlue"], "Advanced", "{bad:?}: the dialog stays open");
        let said = w.state().snap_dialog_error().unwrap_or_else(|| panic!("{bad:?}: the dialog says why")).to_owned();
        assert!(said.contains("Grid strength") && said.contains("whole number of screen pixels from 0 to 999"), "{bad:?}: {said}");
        assert!(w.state().element("snapglue.error").is_some());
        assert_eq!(w.state().session().revision(), revision, "{bad:?}: nothing sent");
        assert_eq!(w.state().snap_dialog_strength("strength_grid"), Some(5), "{bad:?}: the draft keeps its last good number");
    }
    // A good number takes the refusal away, and lets OK through.
    type_into_box(&mut w, "grid", "12");
    assert!(w.state().snap_dialog_error().is_none() && w.state().element("snapglue.error").is_none(), "the refusal goes when the box is right again");
    ok(&mut w);
    assert!(w.state().window_state()["snapGlue"].is_null(), "OK goes through with a good number");
    assert_eq!(settings(&mut w)["strength_grid"], json!(12));
}

/// The strengths are the engine's, so a drag by the window follows them: a rectangle dropped 0.04 inch (3.84 pixels at 100%) off the grid lands on the lines, with the grid's strength
/// at 0 it lands where the pointer put it, and at 20 pixels one dropped 0.1 inch (9.6) off lands on them.
#[test]
fn a_drop_follows_the_grid_strength_set_in_the_dialog() {
    let mut w = edit_window([1280.0, 900.0]);
    // Only the grid pulls: no ruler, no other shape to line up with.
    run_command(&mut w, "snap.set", json!({"ruler": false, "dynamic_grid": false}));
    drag_tile_to(&mut w, "Rectangle", 4.04, 5.04);
    let first = shape_ids(&w)[0];
    let (x, y, ..) = crate::editing::frame_of(&w, first);
    assert!(near(x, 3.25, 1e-6) && near(y, 4.5, 1e-6), "3.84 pixels off the lines: on them: {x} {y}");
    // The grid's strength to 0 in the dialog.
    open(&mut w, "Advanced");
    type_into_box(&mut w, "grid", "0");
    ok(&mut w);
    assert_eq!(settings(&mut w)["strength_grid"], json!(0));
    drag_tile_to(&mut w, "Rectangle", 9.04, 2.04);
    let second = *shape_ids(&w).last().unwrap();
    let (x, y, ..) = crate::editing::frame_of(&w, second);
    assert!(near(x, 8.29, 0.02) && near(y, 1.54, 0.02), "strength 0: where the pointer put it: {x} {y}");
    // And at 20 pixels a drop 9.6 pixels off the lines is pulled onto them.
    open(&mut w, "Advanced");
    type_into_box(&mut w, "grid", "20");
    ok(&mut w);
    drag_tile_to(&mut w, "Rectangle", 6.1, 2.05);
    assert_eq!(shape_ids(&w).len(), 3, "a third rectangle was dropped");
    let third = *shape_ids(&w).last().unwrap();
    let (x, y, ..) = crate::editing::frame_of(&w, third);
    assert!(near(x, 5.25, 1e-6) && near(y, 1.5, 1e-6), "20 pixels reach 9.6: {x} {y}");
}
