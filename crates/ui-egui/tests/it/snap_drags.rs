//! The window's drags and the engine's snap: what a drag costs on a crowded page, **counted** (never timed); the keys that change
//! a drag (Shift, Alt, Ctrl); the zoom it is sent with; connectors' free ends; a selection past what `snap` takes; and a guide and shapes never selected
//! together. Every window has the manual clock; what is checked is the document the commands changed, and that the preview is the release.

use diacraft_engine::Session;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::render_worker::bundled_drawer;
use diacraft_ui_egui::work;
use egui::{Key, Modifiers, Pos2, vec2};
use serde_json::json;

use crate::editing::{at, click_at, edit_window, hold, key, shape_ids};
use crate::fixture;
use crate::guides::{add_guide, bounds, element_center, guides, held_to, near, page_id, press_and_drag, rectangle_and_guide};
use crate::support::{Rest, TempDir, Window, button, fast_window, move_to, run_command};

/// A window on the drawing `bytes` (read from a file in `dir`), at 100%.
fn file_window(dir: &TempDir, bytes: &[u8]) -> Window {
    let path = fixture::write(dir.path(), "drawing.vsdx", bytes);
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let mut w = fast_window(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()), [1280.0, 900.0]);
    w.rest();
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    w
}

/// A rectangle (1.5 by 1) dropped by the command, centred at `(x, y)` exactly (no snap). → its `ID`.
fn rectangle_at(w: &mut Window, x: f64, y: f64) -> u32 {
    let page = page_id(w);
    let answer = w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": page, "x": x, "y": y})).unwrap();
    w.rest();
    u32::try_from(answer["id"].as_u64().unwrap()).unwrap()
}

/// The page place of the begin or the end of connector `id`, as the last frame drew it.
fn end_of(w: &Window, id: u32, end: bool) -> (f64, f64) {
    let screen = w.state().connector_end_point(id, end).expect("the connector has ends");
    w.state().page_point_at(screen).unwrap()
}

/// Lets go of the button at `at` and lets the window rest.
fn release(w: &mut Window, at: Pos2) {
    button(w, at, false);
    w.rest();
}

/// Of the review: a move of one shape on a page of 5,000 read the page and looked up the shapes at every move of the pointer (36 ms a move). Now
/// the page is read and the shapes looked up once for the drag: a move after the first reads no page, looks up no shape, and asks the snap once.
#[test]
fn a_move_on_a_crowded_page_reads_the_page_and_looks_up_its_shapes_once_for_the_drag() {
    let dir = TempDir::new("crowded-move");
    let mut w = file_window(&dir, &fixture::crowded_vsdx(5_000));
    // Shape 4054 is the 0.08 inch square with its lower left at (5.5, 4.2). It is not selected: a press on it selects it and moves it (a press on so small
    // a shape once selected takes a handle).
    let id = 4054;
    let from = at(&w, 5.54, 4.24);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    work::reset();
    button(&mut w, from, true);
    move_to(&mut w, from + vec2(30.0, 0.0));
    assert_eq!(w.state().selection(), [id]);
    let first = work::work();
    assert_eq!(first.move_subjects, 1, "the shapes that move are looked up once, for the drag: {first:?}");
    assert!(first.snap_page_reads <= 1, "the page is read once at the most: {first:?}");
    work::reset();
    diacraft_doc::work::reset();
    for step in 1..=3 {
        move_to(&mut w, from + vec2(30.0 + 7.0 * step as f32, 3.0 * step as f32));
    }
    let moves = work::work();
    assert_eq!((moves.snap_page_reads, moves.move_subjects, moves.snaps), (0, 0, 3), "three moves: three snaps and nothing read again: {moves:?}");
    assert_eq!(diacraft_doc::work::finds_visited(), 0, "and no shape is looked for by a walk of the page");
    let preview = w.state().drag_preview().unwrap().delta.unwrap();
    let before = bounds(&w, id);
    release(&mut w, from + vec2(51.0, 9.0));
    let after = bounds(&w, id);
    assert!(near(after.0 - before.0, preview.0, 1e-9) && near(after.1 - before.1, preview.1, 1e-9), "{preview:?} {before:?} {after:?}");
    // Alt held over the whole page selected: nothing is looked up or snapped at all.
    let all: Vec<u32> = (1..=5_000).collect();
    w.state_mut().select(&all);
    w.rest();
    let from = at(&w, after.0 + 0.04, after.1 + 0.04);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    hold(&mut w, Modifiers::ALT);
    work::reset();
    diacraft_doc::work::reset();
    button(&mut w, from, true);
    for step in 1..=3 {
        move_to(&mut w, from + vec2(10.0 * step as f32, 0.0));
    }
    let alt = work::work();
    assert_eq!((alt.move_subjects, alt.snaps), (0, 0), "{alt:?}");
    assert_eq!(diacraft_doc::work::finds_visited(), 0);
    hold(&mut w, Modifiers::NONE);
    key(&mut w, Key::Escape, Modifiers::NONE);
    release(&mut w, from + vec2(30.0, 0.0));
}

/// A move of more shapes than `snap` takes (512) goes without it, as the preview shows, and is made (`shape.move` refuses `snap` with more); their box is
/// not worked out.
#[test]
fn a_move_of_six_hundred_shapes_goes_without_snap_and_is_made() {
    let dir = TempDir::new("six-hundred");
    let mut w = file_window(&dir, &fixture::crowded_vsdx(600));
    let all: Vec<u32> = (1..=600).collect();
    w.state_mut().select(&all);
    w.rest();
    // Shape 1 is the square at (0.2, 0.2): the view is moved to show it.
    run_command(&mut w, "view.fitPage", json!({}));
    let before = bounds(&w, 1);
    let from = at(&w, 0.24, 0.24);
    work::reset();
    diacraft_doc::work::reset();
    press_and_drag(&mut w, from, from + vec2(40.0, -10.0));
    let preview = w.state().drag_preview().unwrap();
    assert!(!preview.snap, "past 512 shapes no snap is sent");
    assert_eq!(work::work().snaps, 0, "and none is asked");
    assert_eq!(diacraft_doc::work::finds_visited(), 0, "and their box is not worked out: not one of the 600 shapes is looked up");
    release(&mut w, from + vec2(40.0, -10.0));
    let after = bounds(&w, 1);
    let delta = preview.delta.unwrap();
    assert!(after.0 - before.0 > 0.1, "the shapes moved: {before:?} {after:?}");
    assert!(near(after.0 - before.0, delta.0, 1e-9) && near(after.1 - before.1, delta.1, 1e-9), "by the distance the preview showed");
    assert_eq!(w.state().last_snap(), None);
    assert_eq!(w.state().notice(), None, "nothing was refused");
}

/// Shift is the engine's `lock`: the move keeps to the page axis the whole drag has gone farther along, the axis it keeps off does not move at all, to
/// the bit, and the free axis snaps, as a move without Shift does: the shape pulled to a guide on the free axis is held to it, and the release is the preview
/// (it was sent without `snap`, and not glued, where the snap would have moved the axis Shift holds, before the engine took the lock).
#[test]
fn a_shift_move_is_the_engines_lock_so_the_free_axis_snaps_and_is_held_to_a_guide() {
    let mut w = edit_window([1280.0, 900.0]);
    // Off the grid on y: its box is 3.6 to 4.6, a tenth of an inch from a line, which the lock leaves alone.
    let rectangle = rectangle_at(&mut w, 7.0, 4.1);
    let guide = add_guide(&mut w, "vertical", 4.0);
    let (from, to) = (at(&w, 7.0, 4.1), at(&w, 4.8, 4.13));
    hold(&mut w, Modifiers::SHIFT);
    press_and_drag(&mut w, from, to);
    let preview = w.state().drag_preview().unwrap();
    assert!(preview.snap, "goes with snap, and the lock: {preview:?}");
    let delta = preview.delta.unwrap();
    assert_eq!(delta.1, 0.0, "the axis Shift keeps off does not move, to the bit: {delta:?}");
    release(&mut w, to);
    hold(&mut w, Modifiers::NONE);
    let after = bounds(&w, rectangle);
    assert!(near(after.0, 4.0, 1e-9), "the free axis is pulled to the guide: {after:?}");
    assert!(near(after.1, 3.6, 1e-9), "the axis kept off did not move: {after:?}");
    assert!(near(after.0 - 6.25, delta.0, 1e-9), "the release is the preview");
    assert_eq!(held_to(&w, guide), vec![rectangle], "pulled to a guide on the free axis, it is glued to it");
    let said = w.state().last_snap().cloned().expect("the move was sent with snap");
    assert_eq!(said["dy"], json!(0.0), "{said}");
}

/// Shift's axis is picked from the whole drag at every move: x when the pointer has gone farther along x since the press, y when along y, a tie to x; the axis may
/// change during one drag, and the release takes the axis of the last move.
#[test]
fn the_axis_a_shift_move_keeps_to_is_picked_from_the_whole_drag_at_every_move_and_a_tie_goes_to_x() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "snap.set", json!({"grid": false, "ruler": false}));
    let rectangle = rectangle_at(&mut w, 6.0, 4.0);
    let from = at(&w, 6.0, 4.0);
    hold(&mut w, Modifiers::SHIFT);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    // 1.0 along x and 0.5 along y: the move keeps to x.
    let p = at(&w, 7.0, 4.5);
    move_to(&mut w, p);
    let first = w.state().drag_preview().unwrap();
    assert!(first.snap && first.delta.unwrap().1 == 0.0 && near(first.delta.unwrap().0, 1.0, 1e-5), "{first:?}");
    // 0.5 along x and 1.0 along y: now to y, though the drag began along x.
    let p = at(&w, 6.5, 5.0);
    move_to(&mut w, p);
    let second = w.state().drag_preview().unwrap();
    assert!(second.delta.unwrap().0 == 0.0 && near(second.delta.unwrap().1, 1.0, 1e-5), "the axis changes with the drag: {second:?}");
    // The same distance along both: a tie, to x.
    let p = at(&w, 6.75, 4.75);
    move_to(&mut w, p);
    let tie = w.state().drag_preview().unwrap();
    assert!(tie.delta.unwrap().1 == 0.0 && near(tie.delta.unwrap().0, 0.75, 1e-5), "a tie goes to x: {tie:?}");
    // Let go with the pointer along y: the release takes y.
    let before = bounds(&w, rectangle);
    let to = at(&w, 6.25, 5.5);
    move_to(&mut w, to);
    release(&mut w, to);
    hold(&mut w, Modifiers::NONE);
    let after = bounds(&w, rectangle);
    assert!(near(after.0, before.0, 1e-9) && near(after.1 - before.1, 1.5, 1e-5), "the last axis is the release's: {before:?} {after:?}");
}

/// Shift keeps a resize's proportions: a corner does with no key at all, and the engine does it (`keep: "proportions"`): the box is the engine's, snapped inside the
/// proportions, and the release is the preview.
#[test]
fn a_shift_resize_keeps_its_proportions_by_the_engine_and_is_the_preview() {
    let mut w = edit_window([1280.0, 900.0]);
    let rectangle = rectangle_at(&mut w, 3.0, 4.1);
    click_at(&mut w, 3.0, 4.1);
    assert_eq!(w.state().selection(), [rectangle]);
    // The north-east corner, at (3.75, 4.6).
    let (from, to) = (at(&w, 3.75, 4.6), at(&w, 4.9, 5.37));
    hold(&mut w, Modifiers::SHIFT);
    press_and_drag(&mut w, from, to);
    let preview = w.state().drag_preview().unwrap();
    assert!(preview.snap, "the engine keeps the proportions: {preview:?}");
    let sized = preview.sized.unwrap();
    release(&mut w, to);
    hold(&mut w, Modifiers::NONE);
    let after = bounds(&w, rectangle);
    let (width, height) = (after.2 - after.0, after.3 - after.1);
    assert!(near(width / height, 1.5, 1e-6), "the proportions are kept: {width} x {height}");
    assert!(
        near(after.0, sized.0, 1e-9) && near(after.1, sized.1, 1e-9) && near(width, sized.2, 1e-9) && near(height, sized.3, 1e-9),
        "{sized:?} {after:?}"
    );
    assert!(w.state().last_snap().is_some(), "it was sent with snap");
}

/// The snap a drag is sent with carries the view's zoom: at 63% the grid's lines are half an inch apart (a quarter inch at 100%) and its pull of 5 pixels is a wider reach in
/// inches, and the release lands where the preview, worked out at 63%, showed.
#[test]
fn a_drag_at_another_zoom_lands_where_its_preview_showed() {
    let mut w = edit_window([1280.0, 900.0]);
    // Its box from y = 3.56 to 4.56: 0.06 above a line of the half-inch grid, which is 3.6 pixels at 63% and so within the grid's 5, and 5.8 pixels at 100%, which is not.
    let rectangle = rectangle_at(&mut w, 7.0, 4.06);
    run_command(&mut w, "view.zoom", json!({"percent": 63}));
    let before = bounds(&w, rectangle);
    let (from, to) = (at(&w, 7.0, 4.22), at(&w, 6.0, 4.22));
    press_and_drag(&mut w, from, to);
    let delta = w.state().drag_preview().unwrap().delta.unwrap();
    assert!(near(delta.1, -0.06, 1e-4), "pulled to the half-inch grid at 63%: {delta:?}");
    release(&mut w, to);
    let after = bounds(&w, rectangle);
    assert!(near(after.0 - before.0, delta.0, 1e-9) && near(after.1 - before.1, delta.1, 1e-9), "{delta:?} {before:?} {after:?}");
}

/// A connector's free end is sent with `snap` (by `connector.add` and `connector.reconnect`) and lands where the preview showed it, on the grid; with
/// Alt held it is neither snapped nor previewed snapped.
#[test]
fn a_connectors_free_end_snaps_where_its_preview_showed_and_alt_leaves_it() {
    let mut w = edit_window([1280.0, 900.0]);
    rectangle_at(&mut w, 3.0, 4.0);
    run_command(&mut w, "tool.connector", json!({}));
    // From the rectangle to bare page, off the grid.
    let (from, to) = (at(&w, 3.0, 4.0), at(&w, 6.04, 2.21));
    press_and_drag(&mut w, from, to);
    let end = w.state().drag_preview().unwrap().end.expect("the free end's landing is shown");
    assert!(near(end.0, 6.0, 1e-6) && near(end.1, 2.25, 1e-6), "the end is pulled to the grid: {end:?}");
    release(&mut w, to);
    let connector = *shape_ids(&w).last().unwrap();
    let landed = end_of(&w, connector, true);
    assert!(near(landed.0, end.0, 1e-3) && near(landed.1, end.1, 1e-3), "connector.add snapped it as the preview did: {landed:?}");
    // The same with Alt held: the end goes where the pointer is.
    let (from, to) = (at(&w, 3.0, 4.0), at(&w, 6.1, 6.13));
    hold(&mut w, Modifiers::ALT);
    press_and_drag(&mut w, from, to);
    let end = w.state().drag_preview().unwrap().end.unwrap();
    assert!(near(end.0, 6.1, 1e-4) && near(end.1, 6.13, 1e-4), "not snapped under Alt: {end:?}");
    release(&mut w, to);
    hold(&mut w, Modifiers::NONE);
    let free = *shape_ids(&w).last().unwrap();
    let landed = end_of(&w, free, true);
    assert!(near(landed.0, 6.1, 1e-3) && near(landed.1, 6.13, 1e-3), "{landed:?}");
    // The first connector's end dragged elsewhere (`connector.reconnect`): snapped as the preview showed.
    run_command(&mut w, "tool.pointer", json!({}));
    run_command(&mut w, "edit.select", json!({"ids": [connector]}));
    let (from, to) = (at(&w, 6.0, 2.25), at(&w, 8.04, 2.71));
    press_and_drag(&mut w, from, to);
    let end = w.state().drag_preview().unwrap().end.unwrap();
    assert!(near(end.0, 8.0, 1e-6) && near(end.1, 2.75, 1e-6), "{end:?}");
    release(&mut w, to);
    let landed = end_of(&w, connector, true);
    assert!(near(landed.0, 8.0, 1e-3) && near(landed.1, 2.75, 1e-3), "connector.reconnect snapped it as the preview did: {landed:?}");
}

/// A Ctrl copy-drag lines up with its originals: they stay where they are and are targets of the copy's snap, so a copy let go just past the
/// original's edge abuts it (the grid alone would put it elsewhere).
#[test]
fn a_ctrl_copy_snaps_to_its_original() {
    let mut w = edit_window([1280.0, 900.0]);
    // Its box from x = 6.35 to 7.85, off the grid.
    let original = rectangle_at(&mut w, 7.1, 4.0);
    click_at(&mut w, 7.1, 4.0);
    // The copy's left edge goes to 7.88 by the pointer: 0.03 past the original's right edge (the grid alone would take it to 8.0).
    let (from, to) = (at(&w, 7.1, 4.0), at(&w, 8.63, 4.0));
    hold(&mut w, Modifiers::COMMAND);
    press_and_drag(&mut w, from, to);
    let delta = w.state().drag_preview().unwrap().delta.unwrap();
    release(&mut w, to);
    hold(&mut w, Modifiers::NONE);
    let copy = *shape_ids(&w).last().unwrap();
    assert_ne!(copy, original);
    let placed = bounds(&w, copy);
    assert!(near(placed.0, 7.85, 1e-9), "the copy abuts the original: {placed:?}");
    assert!(near(placed.0 - 6.35, delta.0, 1e-9), "the release is the preview: {delta:?}");
    assert!(near(bounds(&w, original).0, 6.35, 1e-12), "the original stays");
}

/// A drag out of a ruler while the guides are hidden shows them again, so that the guide made is seen; Escape drops a guide's selection.
#[test]
fn a_ruler_drag_shows_hidden_guides_and_escape_drops_a_guide_selected() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "view.guides", json!({"on": false}));
    let top = element_center(&w, "canvas.ruler.top");
    let to = at(&w, 6.0, 5.0);
    press_and_drag(&mut w, top, to);
    release(&mut w, to);
    assert_eq!(guides(&w).len(), 1);
    assert_eq!(w.state().window_state()["guides"], json!(true), "the guides show again");
    let id = guides(&w)[0].id;
    assert_eq!(w.state().selected_guide_id(), Some(id));
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().selected_guide_id(), None, "Escape drops the guide's selection");
}

/// What a guide's drag carries are the shapes held to it (`Connect` rows from an alignment cell, parts 1 to 6), not a line whose end names it.
#[test]
fn a_guides_drag_carries_the_shapes_held_to_it_and_no_other_row() {
    let dir = TempDir::new("guide-rows");
    let mut w = file_window(&dir, &fixture::guide_rows_vsdx());
    assert_eq!(guides(&w).len(), 1);
    let (from, to) = (at(&w, 9.5, 5.0), at(&w, 9.5, 5.5));
    press_and_drag(&mut w, from, to);
    assert_eq!(w.state().selected_guide_id(), Some(10));
    let preview = w.state().drag_preview().unwrap();
    assert_eq!(preview.held, vec![2], "the box held by AlignBottom goes with it, and not the one whose BeginX names it: {preview:?}");
    key(&mut w, Key::Escape, Modifiers::NONE);
    release(&mut w, to);
}

/// Of the review: a guide selected, the Text Block tool, a click on a shape: the shape is selected and the guide no longer, and Delete deletes
/// the shape and leaves the guide.
#[test]
fn a_guide_and_a_shape_are_never_selected_together() {
    let mut w = edit_window([1280.0, 900.0]);
    let (rectangle, guide) = rectangle_and_guide(&mut w);
    click_at(&mut w, 4.0, 6.5);
    assert_eq!(w.state().selected_guide_id(), Some(guide));
    run_command(&mut w, "tool.text_block", json!({}));
    click_at(&mut w, 7.0, 4.0);
    assert_eq!(w.state().selection(), [rectangle]);
    assert_eq!(w.state().selected_guide_id(), None, "selecting the shape dropped the guide");
    assert!(w.state().selection_summary().is_none_or(|s| !s.contains("guide")), "{:?}", w.state().selection_summary());
    key(&mut w, Key::Delete, Modifiers::NONE);
    assert!(!shape_ids(&w).contains(&rectangle), "Delete deleted the shape");
    assert_eq!(guides(&w).len(), 1, "and left the guide");
    // And the Text tool's new box drops a guide selected too.
    run_command(&mut w, "tool.pointer", json!({}));
    click_at(&mut w, 4.0, 6.5);
    assert_eq!(w.state().selected_guide_id(), Some(guide));
    run_command(&mut w, "tool.text", json!({}));
    click_at(&mut w, 8.0, 2.0);
    assert_eq!(w.state().selected_guide_id(), None);
    key(&mut w, Key::Escape, Modifiers::NONE);
}

/// Of the review: a tile dragged over the page asks the snap once for each place of the pointer, and nothing in a frame with the pointer still.
#[test]
fn a_tile_held_still_over_the_page_asks_the_snap_nothing() {
    let mut w = edit_window([1280.0, 900.0]);
    let tile = w.state().element("shapes.tile.Rectangle").unwrap().rect.center();
    let to = at(&w, 6.1, 4.13);
    for _ in 0..3 {
        move_to(&mut w, tile);
    }
    button(&mut w, tile, true);
    for step in 1..=5 {
        move_to(&mut w, tile + (to - tile) * (step as f32 / 5.0));
    }
    work::reset();
    for _ in 0..5 {
        w.step();
    }
    assert_eq!(work::work().snaps, 0, "five frames with the pointer still: {:?}", work::work());
    move_to(&mut w, to + vec2(9.0, 0.0));
    assert_eq!(work::work().snaps, 1, "a move: one snap");
    release(&mut w, to + vec2(9.0, 0.0));
    assert_eq!(shape_ids(&w).len(), 1, "and the release drops it");
}

/// Of the review: a zoom about the pointer in the middle of a drag (the page point under it stays) lands the drag again at the new zoom, where the
/// release, sent with that zoom, puts it.
#[test]
fn a_zoom_in_the_middle_of_a_drag_lands_it_again() {
    let mut w = edit_window([1280.0, 900.0]);
    // Its box from y = 3.56 to 4.56: 0.06 (5.8 pixels at 100%) off a line, which the grid's 5 pixels do not reach, and 3.6 pixels at 63%, which they do.
    let rectangle = rectangle_at(&mut w, 7.0, 4.06);
    let before = bounds(&w, rectangle);
    let (from, to) = (at(&w, 7.0, 4.06), at(&w, 6.0, 4.06));
    press_and_drag(&mut w, from, to);
    let at_100 = w.state().drag_preview().unwrap().delta.unwrap();
    // 63% about the pointer: the grid goes from a quarter inch to half an inch, and its pull of 5 pixels is a wider reach in inches.
    w.state_mut().execute("view.zoom", &json!({"percent": 63, "x": to.x, "y": to.y})).unwrap();
    move_to(&mut w, to);
    let at_63 = w.state().drag_preview().unwrap().delta.unwrap();
    assert!(!near(at_63.1, at_100.1, 1e-3), "the landing follows the zoom: {at_100:?} {at_63:?}");
    release(&mut w, to);
    let after = bounds(&w, rectangle);
    assert!(near(after.1 - before.1, at_63.1, 1e-6) && near(after.0 - before.0, at_63.0, 1e-6), "{at_63:?} {before:?} {after:?}");
}

/// A move pulls the shapes' own connection points with their box (`own_points`, as `shape.move` does): a point 0.3 along a box's bottom edge, let go a
/// few points from another box's point, joins it, and the preview showed it there (without its own points it would show the box's middle pulled to the
/// other's corner instead).
#[test]
fn a_moves_own_connection_points_are_pulled_as_the_command_pulls_them() {
    let dir = TempDir::new("own-points");
    let mut w = file_window(&dir, &fixture::off_middle_points_vsdx());
    // Box 1's point is at (2.3, 2.0); box 2's at (5.7, 6.0). By the pointer, box 1 goes 3.42 right and 4.02 up: its point to (5.72, 6.02), under 3
    // points from box 2's, and its bottom middle to (5.92, 6.02), 8 points from box 2's corner.
    let (from, to) = (at(&w, 2.5, 2.5), at(&w, 5.92, 6.52));
    press_and_drag(&mut w, from, to);
    let delta = w.state().drag_preview().unwrap().delta.unwrap();
    release(&mut w, to);
    let after = bounds(&w, 1);
    assert!(near(after.0, 5.4, 1e-9) && near(after.1, 6.0, 1e-9), "box 1's point is on box 2's: {after:?}");
    assert!(near(after.0 - 2.0, delta.0, 1e-9) && near(after.1 - 2.0, delta.1, 1e-9), "the preview showed it there: {delta:?}");
}
