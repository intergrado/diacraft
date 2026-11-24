//! Guides and snapping in the window: guides drawn and hidden by View > Guides, made by a drag out of a ruler, moved and deleted on the
//! drawing area; the window's drags sent with the engine's `snap`, glued to guides, previewed where they land. The pointer and the keys are given
//! to egui as a person's would be, every window has the manual clock, and what is checked is the document the commands changed.

use diacraft_engine::snap::Axis;
use diacraft_engine::snap::box_of;
use diacraft_engine::snap::guides::{GuideInfo, list_guides};
use egui::{Key, Modifiers, Pos2, vec2};
use serde_json::json;

use crate::editing::{at, click_at, drag_tile_to, drag_with, edit_window, key, shape_ids};
use crate::support::{Rest, TempDir, Window, button, drag, move_to, run_command};

pub fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

/// The page's guides, as the engine lists them (paper inches).
pub fn guides(w: &Window) -> Vec<GuideInfo> {
    list_guides(w.state().session().document().unwrap(), page_id(w)).unwrap().0
}

pub fn add_guide(w: &mut Window, orientation: &str, at: f64) -> u32 {
    let page = page_id(w);
    let answer = w.state_mut().execute("guide.add", &json!({"page": page, "orientation": orientation, "at": at})).unwrap();
    w.rest();
    u32::try_from(answer["id"].as_u64().unwrap()).unwrap()
}

/// A shape's box on the page in paper inches: left, bottom, right, top.
pub fn bounds(w: &Window, id: u32) -> (f64, f64, f64, f64) {
    let r = box_of(w.state().session().document().unwrap(), page_id(w), &[id]).unwrap();
    (r.x0, r.y0, r.x1, r.y1)
}

/// The shapes the page's `Connect` rows hold to guide `guide` (from an alignment cell, `FromPart` 1 to 6).
pub fn held_to(w: &Window, guide: u32) -> Vec<u32> {
    let page = w.state().session().document().unwrap().pages.get(0).unwrap();
    page.connects.iter().filter(|c| c.to_sheet == guide && c.from_part.is_some_and(|p| (1..=6).contains(&p))).map(|c| c.from_sheet).collect()
}

pub fn near(a: f64, b: f64, by: f64) -> bool {
    (a - b).abs() <= by
}

pub fn element_center(w: &Window, name: &str) -> Pos2 {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center()
}

/// Presses at `from`, drags to `to` in five frames, and stops with the button still down: the drag in progress.
pub fn press_and_drag(w: &mut Window, from: Pos2, to: Pos2) {
    for _ in 0..3 {
        move_to(w, from);
    }
    button(w, from, true);
    for step in 1..=5 {
        move_to(w, from + (to - from) * (step as f32 / 5.0));
    }
}

pub fn undo(w: &mut Window) {
    key(w, Key::Z, Modifiers::COMMAND);
}

/// A rectangle (1.5 by 1 inch) dropped at (7, 4), and a vertical guide at x = 4 with nothing on it.
pub fn rectangle_and_guide(w: &mut Window) -> (u32, u32) {
    drag_tile_to(w, "Rectangle", 7.0, 4.0);
    let rectangle = shape_ids(w)[0];
    assert!(near(bounds(w, rectangle).0, 6.25, 1e-9), "the drop landed on the grid: {:?}", bounds(w, rectangle));
    let guide = add_guide(w, "vertical", 4.0);
    (rectangle, guide)
}

/// A guide is drawn across the drawing area while View > Guides is on, and neither drawn nor taken hold of while it is off; the box's state is the
/// window's and a new drawing has it on.
#[test]
fn a_guide_is_drawn_and_view_guides_hides_it() {
    let mut w = edit_window([1280.0, 900.0]);
    assert_eq!(w.state().window_state()["guides"], json!(true), "a new drawing shows its guides");
    let id = add_guide(&mut w, "vertical", 4.0);
    let band = w.state().element(&format!("canvas.guide.{id}")).expect("the guide is drawn").rect;
    let canvas = w.state().element("canvas").unwrap().rect;
    assert!(near(f64::from(band.center().x), f64::from(at(&w, 4.0, 0.0).x), 0.5), "at x = 4: {band:?}");
    assert!(band.height() >= canvas.height() - 1.0, "across the drawing area: {band:?} in {canvas:?}");
    // View > Guides off: not drawn, and a click on its line selects nothing.
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    let check = w.state().element("ribbon.View.ShowGuides").expect("the View tab has Guides").clone();
    assert!(check.enabled, "Guides is wired to view.guides");
    crate::support::click(&mut w, check.rect.center());
    w.rest();
    assert_eq!(w.state().window_state()["guides"], json!(false));
    assert!(w.state().element(&format!("canvas.guide.{id}")).is_none(), "hidden");
    click_at(&mut w, 4.0, 6.5);
    assert_eq!(w.state().selected_guide_id(), None, "a hidden guide is not selected");
    // And on again: drawn, and a click on it selects it.
    crate::support::click(&mut w, check.rect.center());
    w.rest();
    assert!(w.state().element(&format!("canvas.guide.{id}")).is_some());
    click_at(&mut w, 4.0, 6.5);
    assert_eq!(w.state().selected_guide_id(), Some(id));
    assert!(w.state().selection().is_empty(), "a guide is selected alone");
    assert_eq!(w.state().selection_summary().as_deref(), Some("Vertical guide at 4 in"), "the status bar says where it is");
    // Hiding the guides drops the selection of one.
    run_command(&mut w, "view.guides", json!({"on": false}));
    assert_eq!(w.state().selected_guide_id(), None);
    assert_eq!(w.state().ui_panics(), 0);
}

/// A drag out of the top ruler makes a horizontal guide where it is let go, and one out of the left ruler a vertical guide: one `guide.add` each, one
/// undo step each, the new guide selected. A drag let go back over the ruler makes nothing.
#[test]
fn a_drag_out_of_a_ruler_makes_a_guide_where_it_is_let_go() {
    let mut w = edit_window([1280.0, 900.0]);
    let before = w.state().session().revision();
    let top = element_center(&w, "canvas.ruler.top");
    let to = at(&w, 6.0, 5.0);
    press_and_drag(&mut w, top, to);
    let preview = w.state().drag_preview().expect("the drag shows where the guide goes");
    assert!(near(preview.guide_at.unwrap(), 5.0, 1e-4), "{preview:?}");
    assert_eq!(w.state().session().revision(), before, "nothing is sent while the button is down");
    button(&mut w, to, false);
    w.rest();
    let made = guides(&w);
    assert_eq!(made.len(), 1, "{made:?}");
    assert_eq!(made[0].axis, Axis::Y, "the top ruler gives a horizontal guide");
    assert!(near(made[0].at, 5.0, 1e-4), "{made:?}");
    assert_eq!(w.state().selected_guide_id(), Some(made[0].id), "the new guide is selected");
    assert_eq!(w.state().session().revision(), before + 1, "one command");
    // The left ruler gives a vertical one.
    let left = element_center(&w, "canvas.ruler.left");
    let to = at(&w, 2.0, 3.0);
    drag(&mut w, left, to - left);
    w.rest();
    let made = guides(&w);
    assert_eq!(made.len(), 2);
    assert_eq!(made[1].axis, Axis::X);
    assert!(near(made[1].at, 2.0, 1e-4), "{made:?}");
    // A drag that comes back over the ruler it left makes nothing.
    let revision = w.state().session().revision();
    let inside = at(&w, 6.0, 6.0);
    press_and_drag(&mut w, top, inside);
    for step in 1..=3 {
        move_to(&mut w, inside + (top - inside) * (step as f32 / 3.0));
    }
    button(&mut w, top, false);
    w.rest();
    assert_eq!(guides(&w).len(), 2, "released over the ruler");
    assert_eq!(w.state().session().revision(), revision);
    // Each guide made is one undo step.
    undo(&mut w);
    assert_eq!(guides(&w).len(), 1);
    undo(&mut w);
    assert!(guides(&w).is_empty());
    assert_eq!(w.state().ui_panics(), 0);
}

/// A shape dragged near a guide snaps to it and is glued: the release sends `snap`, and the engine's answer names the shape in `held`. The preview
/// shows where it lands, which is where the release puts it.
#[test]
fn a_shape_dragged_near_a_guide_snaps_and_is_glued_where_the_preview_showed() {
    let mut w = edit_window([1280.0, 900.0]);
    let (rectangle, guide) = rectangle_and_guide(&mut w);
    // The left edge goes to 4.05 by the pointer: 0.05 inch (5 points at 100%) from the guide, which pulls from 8.
    let (from, to) = (at(&w, 7.0, 4.0), at(&w, 4.8, 4.0));
    let before = bounds(&w, rectangle);
    press_and_drag(&mut w, from, to);
    let preview = w.state().drag_preview().expect("a move in progress has a landing");
    let delta = preview.delta.unwrap();
    assert!(preview.snap, "the release sends snap");
    assert!(
        preview.marks.iter().any(
            |m| matches!(m, diacraft_ui_egui::edit::snapping::Mark::Line { shape: Some(g), targets, .. } if *g == guide && targets.contains(&diacraft_engine::snap::Target::Guide))
        ),
        "the guide is drawn as what it snapped to: {:?}",
        preview.marks
    );
    button(&mut w, to, false);
    w.rest();
    let after = bounds(&w, rectangle);
    assert!(near(after.0, 4.0, 1e-9), "the left edge is on the guide: {after:?}");
    assert!(
        near(after.0 - before.0, delta.0, 1e-9) && near(after.1 - before.1, delta.1, 1e-9),
        "the preview {delta:?} is the release {before:?} -> {after:?}"
    );
    let snap = w.state().last_snap().cloned().expect("the move was sent with snap");
    assert_eq!(snap["held"], json!([rectangle]), "{snap}");
    assert_eq!(held_to(&w, guide), vec![rectangle], "glued by a Connect row to the guide");
    // One undo step takes the move and the glue back.
    undo(&mut w);
    assert!(near(bounds(&w, rectangle).0, 6.25, 1e-9));
    assert!(held_to(&w, guide).is_empty());
    assert_eq!(w.state().ui_panics(), 0);
}

/// With the document's Snap switch off (`snap.set {snap: false}`) the same drag lands where the pointer put it and glues nothing; Alt held does the
/// same for one drag.
#[test]
fn with_snapping_off_or_alt_held_the_same_drag_does_not_snap() {
    let mut w = edit_window([1280.0, 900.0]);
    let (rectangle, guide) = rectangle_and_guide(&mut w);
    run_command(&mut w, "snap.set", json!({"snap": false}));
    drag_with(&mut w, (7.0, 4.0), (4.8, 4.0), Modifiers::NONE);
    let after = bounds(&w, rectangle);
    assert!(near(after.0, 4.05, 1e-3), "not pulled to the guide: {after:?}");
    assert!(held_to(&w, guide).is_empty());
    undo(&mut w);
    // Snap on again, and Alt held: the drag is not snapped either, and goes without `snap`.
    run_command(&mut w, "snap.set", json!({"snap": true}));
    drag_with(&mut w, (7.0, 4.0), (4.8, 4.0), Modifiers::ALT);
    assert!(near(bounds(&w, rectangle).0, 4.05, 1e-3), "{:?}", bounds(&w, rectangle));
    assert!(held_to(&w, guide).is_empty());
    assert_eq!(w.state().last_snap(), None, "no snap was sent");
}

/// A guide dragged on the drawing area moves with one `guide.move`, previewed while it is dragged, and the shape glued to it follows; one undo takes
/// both back.
#[test]
fn a_guide_drag_moves_it_and_the_shape_glued_to_it_follows() {
    let mut w = edit_window([1280.0, 900.0]);
    let (rectangle, guide) = rectangle_and_guide(&mut w);
    drag_with(&mut w, (7.0, 4.0), (4.8, 4.0), Modifiers::NONE);
    assert_eq!(held_to(&w, guide), vec![rectangle]);
    let revision = w.state().session().revision();
    // Taken where no shape is: high on the guide.
    let (from, to) = (at(&w, 4.0, 6.5), at(&w, 5.5, 6.5));
    press_and_drag(&mut w, from, to);
    assert_eq!(w.state().selected_guide_id(), Some(guide), "the press selected the guide");
    let preview = w.state().drag_preview().expect("the guide's drag has a landing");
    assert!(near(preview.guide_at.unwrap(), 5.5, 1e-4), "{preview:?}");
    assert!(
        w.state().element(&format!("canvas.guide.{guide}")).is_some_and(|e| near(f64::from(e.rect.center().x), f64::from(to.x), 0.5)),
        "drawn where it goes"
    );
    assert_eq!(w.state().session().revision(), revision, "nothing is sent while the button is down");
    button(&mut w, to, false);
    w.rest();
    let moved = guides(&w);
    assert!(near(moved[0].at, 5.5, 1e-4), "{moved:?}");
    assert!(near(bounds(&w, rectangle).0, moved[0].at, 1e-9), "the glued shape follows its guide: {:?}", bounds(&w, rectangle));
    assert_eq!(w.state().session().revision(), revision + 1, "one command");
    undo(&mut w);
    assert!(near(guides(&w)[0].at, 4.0, 1e-9));
    assert!(near(bounds(&w, rectangle).0, 4.0, 1e-9), "one undo takes the guide and its shape back");
}

/// Delete on a selected guide sends `guide.delete`: a free guide goes, in one undo step; a guide that a shape is glued to is refused, the status bar
/// says why, and the guide stays.
#[test]
fn delete_removes_a_free_guide_and_refuses_a_glued_one_with_a_notice() {
    let mut w = edit_window([1280.0, 900.0]);
    let (rectangle, glued) = rectangle_and_guide(&mut w);
    drag_with(&mut w, (7.0, 4.0), (4.8, 4.0), Modifiers::NONE);
    assert_eq!(held_to(&w, glued), vec![rectangle]);
    let free = add_guide(&mut w, "horizontal", 6.0);
    // The free one.
    click_at(&mut w, 8.5, 6.0);
    assert_eq!(w.state().selected_guide_id(), Some(free));
    let revision = w.state().session().revision();
    key(&mut w, Key::Delete, Modifiers::NONE);
    assert_eq!(guides(&w).len(), 1, "the free guide is deleted");
    assert_eq!(w.state().session().revision(), revision + 1);
    assert_eq!(w.state().selected_guide_id(), None);
    assert_eq!(shape_ids(&w).len(), 2, "and nothing else: the rectangle and the other guide");
    // The glued one.
    click_at(&mut w, 4.0, 6.5);
    assert_eq!(w.state().selected_guide_id(), Some(glued));
    key(&mut w, Key::Delete, Modifiers::NONE);
    assert_eq!(guides(&w).len(), 1, "the glued guide stays");
    // The window's own words, told apart from the engine's (which name a command and also say "glued").
    assert_eq!(
        w.state().notice(),
        Some("The guide was not deleted: shapes are glued to it. Move them off the guide first, then delete it."),
        "the window says why"
    );
    assert_eq!(w.state().selected_guide_id(), Some(glued), "and it stays selected");
    assert_eq!(held_to(&w, glued), vec![rectangle]);
    // Undo brings the free one back.
    undo(&mut w);
    assert_eq!(guides(&w).len(), 2);
    assert_eq!(w.state().ui_panics(), 0);
}

/// A resize's preview is where the release puts the box: its moved edge pulled to a guide, the other edges where they were.
#[test]
fn a_resize_previews_where_it_lands_and_its_edge_snaps_to_a_guide() {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 3.0, 4.0);
    let rectangle = shape_ids(&w)[0];
    let guide = add_guide(&mut w, "vertical", 5.0);
    click_at(&mut w, 3.0, 4.0);
    // The east handle, at (3.75, 4), to 4.96: 0.04 inch from the guide.
    let (from, to) = (at(&w, 3.75, 4.0), at(&w, 4.96, 4.0));
    press_and_drag(&mut w, from, to);
    let sized = w.state().drag_preview().and_then(|p| p.sized).expect("a resize in progress has a landing");
    assert!(near(sized.0 + sized.2, 5.0, 1e-9), "the moved edge is on the guide: {sized:?}");
    button(&mut w, to, false);
    w.rest();
    let after = bounds(&w, rectangle);
    assert!(near(after.0, sized.0, 1e-9) && near(after.2, sized.0 + sized.2, 1e-9), "the preview {sized:?} is the release {after:?}");
    assert!(near(after.0, 2.25, 1e-9), "the west edge stayed");
    assert_eq!(held_to(&w, guide), vec![rectangle], "the edge pulled to the guide is glued to it");
}

/// A metric drawing at 1:100: a guide out of the ruler goes at the drawing's place for the paper's (a hundred times the paper's length, in
/// millimetres on the ruler), the grid drawn is the snap's at that scale, and a shape dragged near the guide snaps to it.
#[test]
fn a_metric_drawing_at_one_to_a_hundred_places_guides_and_grid_at_its_scale() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "page.size", json!({"preset": "a4"}));
    run_command(&mut w, "page.scale", json!({"preset": "metric_1_100"}));
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    let page = page_id(&w);
    let settings = diacraft_ui_egui::canvas::page_drawing::page_settings(w.state().session().document().unwrap(), page).unwrap();
    assert_eq!(settings.ruler_unit, diacraft_units::Unit::Millimetre);
    assert!(near(settings.drawing_scale, 0.01, 1e-9), "{settings:?}");
    // In drawing inches at 96 points to a paper inch: 32 drawing inches (0.32 paper) is the smallest eighth times a power of two 16 points
    // apart; across A4's 11.69 inches, 37 lines (the window's own grid drew 0.25 paper inch, 47 lines).
    assert_eq!(w.state().canvas_report().grid_lines.0, 37, "{:?}", w.state().canvas_report());
    // A vertical guide out of the left ruler, let go 2 inches of paper from the page's left edge: 200 inches of the drawing, 5080 mm on the ruler.
    let left = element_center(&w, "canvas.ruler.left");
    let to = at(&w, 2.0, 4.0);
    drag(&mut w, left, to - left);
    w.rest();
    let made = guides(&w);
    assert_eq!(made.len(), 1);
    assert!(near(made[0].at, 2.0, 1e-4), "{made:?}");
    let doc = w.state().session().document().unwrap();
    let pin = diacraft_engine::draw::shape_frame(doc, page, made[0].id).unwrap().pin_x;
    assert!(near(pin, 200.0, 1e-2), "the guide's PinX is in drawing inches: {pin}");
    let summary = w.state().selection_summary().unwrap();
    assert!(summary.starts_with("Vertical guide at 50") && summary.ends_with(" mm"), "the ruler's reading: {summary}");
    // A shape made an inch of paper square, then dragged near the guide: its left edge lands on it and is glued.
    drag_tile_to(&mut w, "Square", 5.0, 4.0);
    let square = *shape_ids(&w).last().unwrap();
    let (l, b, _, _) = bounds(&w, square);
    run_command(&mut w, "shape.resize", json!({"id": square, "x": l, "y": b, "width": 1.0, "height": 1.0, "page": page}));
    let (l, b, r, t) = bounds(&w, square);
    let middle = ((l + r) / 2.0, (b + t) / 2.0);
    // The left edge to 2.04 paper inches: 0.04 from the guide (under 4 points at 100%).
    drag_with(&mut w, middle, (middle.0 - (l - 2.04), middle.1), Modifiers::NONE);
    assert!(near(bounds(&w, square).0, made[0].at, 1e-9), "{:?}", bounds(&w, square));
    assert_eq!(held_to(&w, made[0].id), vec![square]);
    assert_eq!(w.state().ui_panics(), 0);
}

/// Guides are never printed or exported: a PNG, an SVG and a PDF of the page, and the print of it, are the same with a guide on the page as without.
#[test]
fn guides_are_never_exported_or_printed() {
    let dir = TempDir::new("guides-export");
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    let write = |w: &mut Window, tag: &str| -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for ext in ["png", "svg", "pdf"] {
            let path = dir.file(&format!("{tag}.{ext}"));
            run_command(w, "file.export", json!({"path": path.to_str().unwrap()}));
            out.push(std::fs::read(&path).unwrap());
        }
        let path = dir.file(&format!("{tag}-print.pdf"));
        run_command(w, "file.print", json!({"path": path.to_str().unwrap()}));
        out.push(std::fs::read(&path).unwrap());
        out
    };
    let without = write(&mut w, "without");
    add_guide(&mut w, "vertical", 4.0);
    add_guide(&mut w, "horizontal", 4.0);
    assert_eq!(guides(&w).len(), 2);
    let with = write(&mut w, "with");
    assert_eq!(with[0], without[0], "the PNG shows the guides");
    assert_eq!(with[1], without[1], "the SVG shows the guides");
    for (i, kind) in [(2, "PDF"), (3, "printed PDF")] {
        assert_eq!(strip_dates(&with[i]), strip_dates(&without[i]), "the {kind} shows the guides");
    }
}

/// A file's bytes with any PDF date (`/CreationDate (D:...)`, `/ModDate (D:...)`) taken out, so that two writes a second apart compare.
fn strip_dates(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_ref();
    while let Some(at) = rest.find("(D:") {
        out.push_str(&rest[..at]);
        rest = rest[at..].find(')').map_or("", |end| &rest[at + end + 1..]);
    }
    out.push_str(rest);
    out.into_bytes()
}

/// Each guide action and each drag is one undo step, and the window's state follows an undo: a guide undone is no longer selected.
#[test]
fn a_guide_undone_is_no_longer_selected_and_the_window_survives_hostile_moves() {
    let mut w = edit_window([1280.0, 900.0]);
    let top = element_center(&w, "canvas.ruler.top");
    let middle = at(&w, 5.0, 5.0);
    drag(&mut w, top, middle - top);
    w.rest();
    let id = guides(&w)[0].id;
    assert_eq!(w.state().selected_guide_id(), Some(id));
    undo(&mut w);
    assert_eq!(w.state().selected_guide_id(), None, "the guide undone is gone from the selection");
    // A drag out of a ruler far off the window, and back, makes nothing and costs nothing.
    press_and_drag(&mut w, top, top + vec2(0.0, 5000.0));
    button(&mut w, top + vec2(0.0, 5000.0), false);
    w.rest();
    assert!(guides(&w).is_empty());
    // Escape drops a drag out of a ruler.
    press_and_drag(&mut w, top, middle);
    key(&mut w, Key::Escape, Modifiers::NONE);
    button(&mut w, middle, false);
    w.rest();
    assert!(guides(&w).is_empty(), "Escape dropped it");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn shapes_divider_drag_resizes_without_creating_a_guide_and_ruler_middle_still_places_one() {
    use diacraft_ui_egui::canvas::grid::RULER_THICKNESS;
    for offset in [-2.0, 0.0, 2.0] {
        let mut w = edit_window([1280.0, 900.0]);
        let revision = w.state().session().revision();
        let canvas = w.state().element("canvas").unwrap().rect;
        let edge = canvas.left() - RULER_THICKNESS;
        let from = egui::pos2(edge + offset, canvas.center().y);
        drag(&mut w, from, vec2(80.0, 0.0));
        w.rest();
        let resized = w.state().element("canvas").unwrap().rect.left() - RULER_THICKNESS;
        assert!(resized > edge + 50.0, "offset {offset}: the divider resizes Shapes ({edge} -> {resized})");
        assert_eq!(w.state().session().revision(), revision, "resizing the panel never edits the drawing");
        assert!(guides(&w).is_empty(), "the resize band cannot start a ruler drag");
        let middle = element_center(&w, "canvas.ruler.left");
        let to = at(&w, 4.0, 4.0);
        drag(&mut w, middle, to - middle);
        w.rest();
        let made = guides(&w);
        assert_eq!(made.len(), 1, "the ruler's interior still creates a guide");
        assert_eq!(made[0].axis, Axis::X);
        assert!(near(made[0].at, 4.0, 1e-4));
        assert_eq!(w.state().session().revision(), revision + 1);
    }
}

#[test]
fn collapsed_shapes_preserves_the_full_left_ruler_interaction() {
    use diacraft_ui_egui::canvas::grid::RULER_THICKNESS;
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "window.shapes", json!({"shown": false}));
    let ruler = w.state().element("canvas.ruler.left").unwrap().rect;
    assert_eq!(ruler.width(), RULER_THICKNESS);
    let from = egui::pos2(ruler.left() + 1.0, ruler.center().y);
    let to = at(&w, 4.0, 4.0);
    drag(&mut w, from, to - from);
    w.rest();
    assert_eq!(guides(&w).len(), 1, "the folded strip has no resize band to reserve");
}
