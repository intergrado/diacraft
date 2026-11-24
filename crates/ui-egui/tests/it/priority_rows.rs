//! What the window gained to close the P0 rows still scored partial: the zoom keys of the inventory, distributed alignment, a group sized from the
//! Size & Position pane, More Shapes with every stencil, the sizes of an arrowhead, and the keys that move a connector's handle. Each test is a press or a key on what
//! the window drew, and what is checked is the window's own state and the document the engine holds.

use egui::accesskit::{Role, Toggled};
use egui::os::OperatingSystem;
use egui::{Key, Modifiers, Rect};
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

use crate::editing::{drag_tile_to, edit_window, frame_of, key, shape_ids, type_text};
use crate::saving::ctrl;
use crate::support::{Rest, Window, click, drawing_window, elements_named, press, run_command};

fn zoom_percent(w: &Window) -> f64 {
    w.state().window_state()["zoomPercent"].as_f64().expect("a zoom")
}

/// Alt+F6 zooms in, Alt+Shift+F6 zooms out and Ctrl+Shift+W fits the page to the window
/// The chords the window had before (Ctrl+= and Ctrl+-) still work. The keys are the
/// Windows desktop's, so the window is told it runs there (on a Mac, Fit to Window has a key of its own: the next test).
#[test]
fn the_zoom_keys_of_the_inventory_zoom_in_out_and_fit() {
    let mut w = drawing_window([1280.0, 720.0]);
    w.ctx.set_os(OperatingSystem::Windows);
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    key(&mut w, Key::F6, Modifiers::ALT);
    assert_eq!(zoom_percent(&w), 125.0, "Alt+F6 zooms in");
    key(&mut w, Key::F6, Modifiers::ALT | Modifiers::SHIFT);
    assert_eq!(zoom_percent(&w), 100.0, "Alt+Shift+F6 zooms out");
    // The page is fitted to the window: not the zoom it was, and the same as the command's.
    run_command(&mut w, "view.zoom", json!({"percent": 400}));
    key(&mut w, Key::W, Modifiers::COMMAND | Modifiers::SHIFT);
    let by_key = zoom_percent(&w);
    run_command(&mut w, "view.zoom", json!({"percent": 400}));
    run_command(&mut w, "view.fitPage", json!({}));
    assert_ne!(by_key, 400.0, "Ctrl+Shift+W changed the zoom");
    assert_eq!(by_key, zoom_percent(&w), "and fitted the page, as Fit to Window does");
    // The keys the window had.
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    key(&mut w, Key::Equals, Modifiers::COMMAND);
    assert_eq!(zoom_percent(&w), 125.0);
    key(&mut w, Key::Minus, Modifiers::COMMAND);
    assert_eq!(zoom_percent(&w), 100.0);
}

/// On a Mac, Command+Shift+W is one of the system's standard keys, "Close a file and its associated windows" (Apple's Human Interface Guidelines,
/// "Keyboards", read 2026-11-22), so Fit to Window is Command+0 there, and Command+Shift+W leaves the zoom alone. Elsewhere Ctrl+Shift+W fits, and Ctrl+0 is
/// not Fit to Window.
#[test]
fn on_a_mac_fit_to_window_is_command_0_and_command_shift_w_is_not_taken() {
    let mut w = drawing_window([1280.0, 720.0]);
    let fitted = {
        run_command(&mut w, "view.zoom", json!({"percent": 400}));
        run_command(&mut w, "view.fitPage", json!({}));
        zoom_percent(&w)
    };
    assert_ne!(fitted, 400.0);
    // egui-winit reports the Command key of a Mac as `mac_cmd` and `command`.
    let command = Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE };
    w.ctx.set_os(OperatingSystem::Mac);
    run_command(&mut w, "view.zoom", json!({"percent": 400}));
    key(&mut w, Key::W, command | Modifiers::SHIFT);
    assert_eq!(zoom_percent(&w), 400.0, "Command+Shift+W is the system's on a Mac");
    key(&mut w, Key::Num0, command);
    assert_eq!(zoom_percent(&w), fitted, "Command+0 fits the page to the window");

    let ctrl = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
    for os in [OperatingSystem::Windows, OperatingSystem::Nix] {
        w.ctx.set_os(os);
        run_command(&mut w, "view.zoom", json!({"percent": 400}));
        key(&mut w, Key::Num0, ctrl);
        assert_eq!(zoom_percent(&w), 400.0, "{os:?}: Ctrl+0 is not Fit to Window");
        key(&mut w, Key::W, ctrl | Modifiers::SHIFT);
        assert_eq!(zoom_percent(&w), fitted, "{os:?}: Ctrl+Shift+W fits");
    }
}

/// On a Mac, Command+Plus zooms the drawing in, beside Command+=. A German, Portuguese or Nordic Mac layout types `=` with Shift and the window
/// takes it as `0` there (the layout's Command table), while `+` is a key of its own; before, Command+Plus was egui's own key and scaled the whole window
/// instead. Elsewhere Ctrl+Plus is not a key of DiaCraft's (the shortcut list gives Ctrl+Plus to the web app only).
#[test]
fn on_a_mac_command_plus_zooms_the_drawing_in() {
    let mut w = drawing_window([1280.0, 720.0]);
    let command = Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE };
    w.ctx.set_os(OperatingSystem::Mac);
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    key(&mut w, Key::Plus, command);
    assert_eq!((zoom_percent(&w), w.ctx.zoom_factor()), (125.0, 1.0), "Command+Plus zooms the drawing, not the window");
    key(&mut w, Key::Equals, command);
    let twice = zoom_percent(&w);
    assert!(twice > 125.0, "Command+= still zooms in: {twice}");
    key(&mut w, Key::Minus, command);
    assert!(zoom_percent(&w) < twice);
    w.ctx.set_os(OperatingSystem::Windows);
    let before = zoom_percent(&w);
    key(&mut w, Key::Plus, Modifiers { ctrl: true, command: true, ..Modifiers::NONE });
    assert_eq!(zoom_percent(&w), before, "Ctrl+Plus is not DiaCraft's on Windows");
}

// ---- distributed alignment ---------------------------------------------------------------------------------------------------------------

/// The state of the radio button with this label (a ribbon button can carry the same words).
fn toggled(w: &Window, label: &str) -> Option<Toggled> {
    w.get_by_role_and_label(Role::RadioButton, label).accesskit_node().toggled()
}

fn rect_of(w: &Window, name: &str) -> Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

fn click_on(w: &mut Window, name: &str) {
    let at = rect_of(w, name).center();
    click(w, at);
    w.rest();
}

fn page_id(w: &Window) -> u32 {
    w.state().session().document().expect("a drawing").pages.get(0).expect("a page").id
}

/// What `shape.formatting` says of shape `id`.
fn look(w: &mut Window, id: u32) -> serde_json::Value {
    let page = page_id(w);
    w.state_mut().execute("shape.formatting", &json!({"ids": [id], "page": page})).expect("formatting")["shapes"][0].clone()
}

/// Left, centre, right, justify **and distributed**. Distributed has no ribbon button (`ParagraphDistributed` is off the ribbon);
/// the Text dialog's Alignment choices have it: picking it sends `text.format {align: distributed}`, which writes `HorzAlign` 4, and the dialog
/// shows it chosen when it is opened again.
#[test]
fn the_text_dialog_offers_distributed_alignment_and_the_engine_writes_horzalign_4() {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 4.0, 5.0);
    let id = shape_ids(&w)[0];
    press(&mut w, "ribbon.Home.ParagraphDialog");
    assert_eq!(w.state().window_state()["textDialog"], json!("Paragraph"));
    for word in ["left", "center", "right", "justify", "distributed"] {
        assert!(w.state().element(&format!("textdialog.align.{word}")).is_some_and(|e| e.enabled), "{word} is offered");
    }
    let revision = w.state().session().revision();
    click_on(&mut w, "textdialog.align.distributed");
    click_on(&mut w, "textdialog.ok");
    assert_eq!(w.state().session().revision(), revision + 1, "one command");
    assert_eq!(look(&mut w, id)["align"], json!("distributed"));
    // The dialog opens on what the shape has.
    press(&mut w, "ribbon.Home.ParagraphDialog");
    assert_eq!(toggled(&w, "Distributed"), Some(Toggled::True), "Distributed is the choice the dialog opens on");
    assert_eq!(toggled(&w, "Justify"), Some(Toggled::False), "and Justify is not");
    click_on(&mut w, "textdialog.cancel");
    // Justify then Distributed are two different alignments: the one press and the other state differ.
    run_command_text(&mut w, id, "justify");
    assert_eq!(look(&mut w, id)["align"], json!("justify"));
    run_command_text(&mut w, id, "distributed");
    assert_eq!(look(&mut w, id)["align"], json!("distributed"));
    assert_eq!(w.state().ui_panics(), 0);
}

fn run_command_text(w: &mut Window, id: u32, align: &str) {
    let page = page_id(w);
    crate::support::run_command(w, "text.format", json!({"ids": [id], "page": page, "align": align}));
}

// ---- a group sized from the Size & Position pane -----------------------------------------------------------------------------------------

/// Types `text` in the pane's field `name` and ends it with Enter.
fn type_in(w: &mut Window, name: &str, text: &str) {
    click_on(w, name);
    key(w, Key::A, ctrl());
    type_text(w, text);
    key(w, Key::Enter, Modifiers::NONE);
}

/// The pin of shape `id` on the page, along x, in paper inches (`shape.placement`: a member's is carried through its groups).
fn pin_x(w: &mut Window, id: u32) -> f64 {
    let page = page_id(w);
    let answer = w.state_mut().execute("shape.placement", &json!({"ids": [id], "page": page})).expect("placement");
    answer["shapes"][0]["x"].as_f64().expect("an x")
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

/// "a group acts as one shape for resize, rotate and align". The Size & Position pane sized no group (its Width and Height were
/// quiet for one, and `shape.size` refused it): now a size typed for a group is one `shape.size`, the group keeps its pin and its members follow it, in one undo step.
#[test]
fn the_size_and_position_pane_sizes_a_group_and_its_members_follow() {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 3.0, 6.0);
    drag_tile_to(&mut w, "Diamond", 7.0, 6.0);
    let ids = shape_ids(&w);
    run_command(&mut w, "edit.select", json!({"ids": ids}));
    run_command(&mut w, "arrange.group", json!({}));
    let selected = w.state().selection().to_vec();
    assert_eq!(selected.len(), 1, "the group is the selection: {selected:?}");
    let group = selected[0];
    run_command(&mut w, "window.pane", json!({"pane": "size_position", "shown": true}));
    w.rest();
    assert!(w.state().element("pane.size.width").is_some_and(|e| e.enabled), "Width is not quiet for a group");
    assert!(w.state().element("pane.size.height").is_some_and(|e| e.enabled), "nor is Height");
    let (rect_before, diamond_before, group_before) = (frame_of(&w, ids[0]), frame_of(&w, ids[1]), frame_of(&w, group));
    let (group_pin_before, rect_pin_before, diamond_pin_before) = (pin_x(&mut w, group), pin_x(&mut w, ids[0]), pin_x(&mut w, ids[1]));
    let revision = w.state().session().revision();
    let doubled = group_before.2 * 2.0;
    type_in(&mut w, "pane.size.width", &format!("{doubled}"));
    assert_eq!(w.state().session().revision(), revision + 1, "one command, one undo step");
    let group_after = frame_of(&w, group);
    assert!(near(group_after.2, doubled), "the group is {doubled} wide: {group_after:?}");
    assert!(near(group_after.3, group_before.3), "its height is as it was");
    // The members follow: the pair is twice as wide, so each is twice as far from the group's left edge, and as wide again.
    let (rect_after, diamond_after) = (frame_of(&w, ids[0]), frame_of(&w, ids[1]));
    assert!(
        near(rect_after.2, rect_before.2 * 2.0) && near(diamond_after.2, diamond_before.2 * 2.0),
        "the members are stretched: {rect_after:?} {diamond_after:?}"
    );
    // Where each pin is on the page: the group's pin stays, and each member's offset from it is stretched with the group.
    let (pin_group, pin_rect, pin_diamond) = (pin_x(&mut w, group), pin_x(&mut w, ids[0]), pin_x(&mut w, ids[1]));
    assert!(near(pin_group, group_pin_before), "the group keeps its pin");
    assert!(
        near(pin_rect - pin_group, (rect_pin_before - group_pin_before) * 2.0)
            && near(pin_diamond - pin_group, (diamond_pin_before - group_pin_before) * 2.0),
        "each member stays at the same place of the group's box"
    );
    run_command(&mut w, "edit.undo", json!({}));
    assert!(near(frame_of(&w, group).2, group_before.2) && near(frame_of(&w, ids[0]).2, rect_before.2), "undo gives back the group and its members");
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- More Shapes: every stencil, by category, with a check on the ones that are open ------------------------------------------------------------

/// The names of the elements the window drew whose name starts with `prefix`, with the prefix off, left to right and top to bottom.
fn drawn_names(w: &Window, prefix: &str) -> Vec<String> {
    let mut found = elements_named(w, prefix);
    found.sort_by(|a, b| a.1.min.y.total_cmp(&b.1.min.y).then(a.1.min.x.total_cmp(&b.1.min.x)));
    found.into_iter().map(|(name, ..)| name.strip_prefix(prefix).unwrap_or(&name).to_owned()).collect()
}

fn open_more_shapes(w: &mut Window) {
    click_on(w, "shapes.more");
    w.rest();
}

/// A category menu opens any stencil in any drawing, with check marks on those that
/// are open. A drawing made from a template has the stencils its kind needs open and the rest of the built-in ones available: before, the menu named only the open ones, so
/// nothing in the window could open the others. Now the menu files all five under their categories, marks the open ones, and a choice of one that is not open opens it and
/// shows its masters.
#[test]
fn more_shapes_files_every_built_in_stencil_by_category_checks_the_open_ones_and_opens_the_rest() {
    let mut w = edit_window([1280.0, 900.0]);
    // A new drawing from Basic Flowchart opens two of the five.
    run_command(&mut w, "file.new", json!({"template": "basic_flowchart"}));
    w.rest();
    open_more_shapes(&mut w);
    assert_eq!(
        drawn_names(&w, "shapes.category."),
        ["Flowchart", "General", "Software and Database", "Business"],
        "the categories, the open ones' first"
    );
    let all = [
        "Arrow Shapes",
        "Basic Flowchart",
        "Basic Shapes",
        "Cross-Functional Flowchart Shapes",
        "Decorative Shapes",
        "Gane-Sarson",
        "Organization Chart Shapes",
    ];
    let mut rows: Vec<String> = drawn_names(&w, "shapes.more.").into_iter().filter(|n| n != "open").collect();
    rows.sort();
    assert_eq!(rows, all, "every built-in stencil has a row, open or not");
    let mut checked = drawn_names(&w, "shapes.open.");
    checked.sort();
    assert_eq!(checked, ["Basic Flowchart", "Basic Shapes"], "the check marks are on the two the template opened");
    // The rows stand under their category: Flowchart's first, then General's, then Software and Database's.
    let y = |name: &str| rect_of(&w, name).center().y;
    assert!(y("shapes.category.Flowchart") < y("shapes.more.Basic Flowchart") && y("shapes.more.Basic Flowchart") < y("shapes.category.General"));
    assert!(
        y("shapes.category.General") < y("shapes.more.Arrow Shapes") && y("shapes.more.Arrow Shapes") < y("shapes.category.Software and Database")
    );
    assert!(y("shapes.category.Software and Database") < y("shapes.more.Gane-Sarson"));
    // The popup is inside the window.
    let screen = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 900.0));
    assert!(screen.contains_rect(rect_of(&w, "shapes.catalogue")), "the menu is inside the window");
    // A stencil that is not open: a click opens it, the window shows its masters, and it carries a check mark from then on.
    let engine_open = |w: &mut Window| -> Vec<String> {
        let list = w.state_mut().execute("stencil.list", &json!({})).expect("stencil.list");
        list["stencils"].as_array().expect("stencils").iter().filter_map(|s| s["id"].as_str().map(str::to_owned)).collect()
    };
    assert_eq!(engine_open(&mut w), ["flowchart", "basic"]);
    click_on(&mut w, "shapes.more.Arrow Shapes");
    assert_eq!(engine_open(&mut w), ["flowchart", "basic", "arrows"], "the engine opened it, after the others");
    assert_eq!(w.state().window_state()["stencil"], "arrows", "and the window shows it");
    assert!(w.state().element("shapes.tile.Right Arrow").is_some(), "its masters are in the window");
    open_more_shapes(&mut w);
    let mut checked = drawn_names(&w, "shapes.open.");
    checked.sort();
    assert_eq!(checked, ["Arrow Shapes", "Basic Flowchart", "Basic Shapes"]);
    // A stencil that is open is shown by a click and nothing is opened again.
    click_on(&mut w, "shapes.more.Basic Flowchart");
    assert_eq!(w.state().window_state()["stencil"], "flowchart");
    assert_eq!(engine_open(&mut w), ["flowchart", "basic", "arrows"]);
    assert_eq!(w.state().ui_panics(), 0);
}

/// A narrow window: the menu keeps inside it and every row has room for its name.
#[test]
fn the_more_shapes_menu_fits_a_narrow_window() {
    let mut w = edit_window([700.0, 600.0]);
    run_command(&mut w, "file.new", json!({"template": "basic_flowchart"}));
    w.rest();
    open_more_shapes(&mut w);
    let screen = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 600.0));
    let catalogue = rect_of(&w, "shapes.catalogue");
    assert!(screen.contains_rect(catalogue), "{catalogue:?} is inside {screen:?}");
    for name in drawn_names(&w, "shapes.more.").into_iter().filter(|n| n != "open") {
        let row = rect_of(&w, &format!("shapes.more.{name}"));
        assert!(row.width() >= 100.0 && row.height() >= 20.0, "{name} has room: {row:?}");
    }
}
