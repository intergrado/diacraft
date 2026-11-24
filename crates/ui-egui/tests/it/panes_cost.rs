//! The work the panes do on big drawings, **counted** and never timed (`diacraft_ui_egui::work`): a page of detailed shapes selected as a whole, and a
//! document with thousands of masters. What each frame does is bounded by what the pane shows, and what depends on the drawing is done once for each revision.

use diacraft_engine::Session;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::render_worker::bundled_drawer;
use diacraft_ui_egui::work;
use serde_json::json;

use crate::fixture;
use crate::support::{Rest, TempDir, Window, fast_window, run_command};

const THUMB_POINT_BUDGET: usize = work::THUMB_POINT_BUDGET;
const EXPLORER_ROWS_MAX: usize = work::EXPLORER_ROWS_MAX;

fn window_of(dir: &TempDir, bytes: &[u8]) -> Window {
    let path = fixture::write(dir.path(), "big.vsdx", bytes);
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let mut w = fast_window(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()), [1280.0, 900.0]);
    w.rest();
    w
}

fn open(w: &mut Window, pane: &str) {
    run_command(w, "window.pane", json!({"pane": pane, "shown": true}));
}

fn frames(w: &mut Window, n: usize) {
    for _ in 0..n {
        w.step();
    }
}

/// 3,000 shapes of 100 points each (the scene keeps about 16 of them: 48,000 points, over the budget) selected as a whole, with Size & Position and Pan & Zoom
/// open: each frame does work bounded by the shapes (never the points), the lines of the picture are made once for a revision of the drawing within a budget of
/// points with every shape in it (some simplified, none left out, and the picture says so), and the engine is asked once.
#[test]
fn size_and_position_and_pan_and_zoom_do_bounded_work_on_a_page_of_detailed_shapes() {
    let dir = TempDir::new("detailed");
    let mut w = window_of(&dir, &fixture::detailed_vsdx(3_000, 100));
    let all: Vec<u32> = (1..=3_000).collect();
    work::reset();
    w.state_mut().select(&all);
    open(&mut w, "size_position");
    open(&mut w, "pan_zoom");
    w.rest();
    let first = work::work();
    assert_eq!(first.thumb_builds, 1, "the picture's lines were made once: {first:?}");
    assert_eq!(first.placement_queries, 1, "and the engine asked once for the selection: {first:?}");
    assert!(first.thumb_points <= THUMB_POINT_BUDGET && first.thumb_points > THUMB_POINT_BUDGET - 3_000, "within the budget and near it: {first:?}");
    assert_eq!(first.thumb_left_out, 0, "every shape has its box: none is left out: {first:?}");
    assert!(first.thumb_simplified > 0, "and the detail is shared, so some are drawn simplified: {first:?}");
    // The picture says so under itself.
    {
        use egui_kittest::kittest::Queryable;
        w.get_by_label_contains("shapes are drawn simplified");
    }
    assert!(w.state().element("pane.pan_zoom.note").is_some());
    // Many frames of a still drawing: nothing is made again, and what each frame draws and looks up is bounded.
    let before = work::work();
    frames(&mut w, 20);
    let after = work::work();
    assert_eq!(after.thumb_builds, before.thumb_builds, "no new picture for a still drawing");
    assert_eq!(after.placement_queries, before.placement_queries, "no new question to the engine");
    assert!(after.thumb_painted <= THUMB_POINT_BUDGET, "a frame draws at most the budget: {after:?}");
    assert!(
        after.caps_built - before.caps_built <= 20 * 3_000,
        "each frame looks at the shapes selected and never at their points ({} lookups in 20 frames)",
        after.caps_built - before.caps_built
    );
    // A new revision: one new picture and one new question.
    run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 0.1, "dy": 0.0}));
    w.rest();
    frames(&mut w, 10);
    let moved = work::work();
    assert_eq!(moved.thumb_builds, after.thumb_builds + 1, "{moved:?}");
    assert_eq!(moved.placement_queries, after.placement_queries + 1, "{moved:?}");
    // A new selection with the drawing as it is: a new question, and the same picture.
    w.state_mut().select(&[1, 2, 3]);
    w.rest();
    let picked = work::work();
    assert_eq!(picked.thumb_builds, moved.thumb_builds, "the selection does not make the picture again");
    assert_eq!(picked.placement_queries, moved.placement_queries + 1);
    assert_eq!(w.state().ui_panics(), 0);
}

/// A document with 6,000 masters: the tree is counted once for each revision in one pass that builds no name, only the branches that are unfolded build names
/// and only the first rows of them, and the rest is said in one line.
#[test]
fn drawing_explorer_builds_only_the_rows_it_shows_and_once_for_each_revision() {
    let dir = TempDir::new("masters");
    let mut w = window_of(&dir, &fixture::many_masters_vsdx(6_000));
    work::reset();
    open(&mut w, "drawing_explorer");
    w.rest();
    frames(&mut w, 20);
    let folded = work::work();
    assert_eq!(folded.explorer_scans, 1, "{folded:?}");
    assert_eq!(folded.explorer_names, 1, "only the page's name was built: the masters are folded");
    assert!(w.state().element("explorer.branch.masters").is_some());
    assert!(w.state().element("explorer.more.masters").is_none());
    run_command(&mut w, "window.explorerBranch", json!({"branch": "masters", "open": true}));
    w.rest();
    frames(&mut w, 20);
    let open_now = work::work();
    assert_eq!(open_now.explorer_scans, 1, "unfolding a branch does not count the tree again: {open_now:?}");
    assert_eq!(open_now.explorer_names, 1 + EXPLORER_ROWS_MAX, "the first rows of the masters and no more: {open_now:?}");
    assert!(w.state().element("explorer.more.masters").is_some(), "the rest is said in a line");
    // A new revision: counted again once, and the rows of the branches that are unfolded built again.
    run_command(&mut w, "page.add", json!({"name": "Second"}));
    w.rest();
    frames(&mut w, 10);
    let next = work::work();
    assert_eq!(next.explorer_scans, 2, "{next:?}");
    assert_eq!(next.explorer_names, open_now.explorer_names + 2 + EXPLORER_ROWS_MAX, "{next:?}");
    assert_eq!(w.state().ui_panics(), 0);
}

/// A branch of more than the cap lists the cap and says how many it leaves out; a branch of fewer says nothing.
#[test]
fn a_branch_over_the_row_cap_lists_the_cap_and_says_how_many_it_leaves_out() {
    use egui_kittest::kittest::Queryable;
    let dir = TempDir::new("cap");
    let mut w = window_of(&dir, &fixture::many_masters_vsdx(EXPLORER_ROWS_MAX as u32 + 25));
    open(&mut w, "drawing_explorer");
    run_command(&mut w, "window.explorerBranch", json!({"branch": "masters", "open": true}));
    w.rest();
    w.get_by_label("25 more not listed");
    let few = TempDir::new("under");
    let mut small = window_of(&few, &fixture::many_masters_vsdx(40));
    open(&mut small, "drawing_explorer");
    run_command(&mut small, "window.explorerBranch", json!({"branch": "masters", "open": true}));
    small.rest();
    assert!(small.state().element("explorer.more.masters").is_none());
}

/// A page of 5,000 shapes (the most the scene reads): every one has a box in the picture, whatever their detail, and a drawing of simple shapes says nothing
/// under the picture.
#[test]
fn every_shape_has_a_box_in_the_picture_and_a_simple_drawing_is_not_called_simplified() {
    let dir = TempDir::new("boxes");
    let mut w = window_of(&dir, &fixture::detailed_vsdx(5_000, 2));
    work::reset();
    open(&mut w, "pan_zoom");
    w.rest();
    let drawn = work::work();
    assert_eq!(drawn.thumb_left_out, 0, "{drawn:?}");
    assert!(drawn.thumb_points >= 5_000 * 2, "each of the 5,000 shapes has its line: {drawn:?}");
    assert_eq!(drawn.thumb_simplified, 0, "two-point shapes are drawn whole: {drawn:?}");
    assert!(w.state().element("pane.pan_zoom.note").is_none(), "so nothing is said");
}

/// A shape of nine paths is drawn with eight (the cut of a shape's paths), which is less than its whole outline: the picture counts it as simplified and
/// says so in the singular.
#[test]
fn a_shape_cut_to_eight_paths_is_counted_as_simplified_and_the_note_is_singular() {
    use egui_kittest::kittest::Queryable;
    let dir = TempDir::new("nine-paths");
    let mut w = window_of(&dir, &fixture::nine_paths_vsdx());
    work::reset();
    open(&mut w, "pan_zoom");
    w.rest();
    let drawn = work::work();
    assert_eq!(drawn.thumb_simplified, 1, "{drawn:?}");
    assert_eq!(drawn.thumb_left_out, 0);
    w.get_by_label("1 shape is drawn simplified.");
}
