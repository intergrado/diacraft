//! The drawing tools in the window: Home, Tools, Drawing Tools. The pointer and the keyboard are given to egui as a person's would be, and what is checked is the
//! document they changed through `shape.draw`, which is one undo step each.

use diacraft_doc::{CellAddr, Owner, Resolver, SectionKey, SheetRef};
use egui::{Key, Modifiers};
use serde_json::{Value, json};

use crate::editing::{at, drag_with, edit_window, frame_of, hold, key, shape_ids};
use crate::saving::ctrl;
use crate::support::{Rest, Window, button, move_to, press, run_command};

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-3
}

fn undo(w: &mut Window) {
    run_command(w, "edit.undo", json!({}));
}

fn tool(w: &Window) -> String {
    w.state().tool().name().to_owned()
}

/// The kinds of the rows of the first `Geometry` section of shape `id`.
fn kinds(w: &Window, id: u32) -> Vec<String> {
    let doc = w.state().session().document().unwrap();
    let at = doc.find_shape(Owner::Page(doc.pages.get(0).unwrap().id), id).unwrap();
    let rows = Resolver::new(doc).effective_rows(&SheetRef::Shape(at), &SectionKey::new("Geometry", Some(0))).unwrap();
    rows.iter().map(|r| r.row_type.as_deref().unwrap_or("").to_owned()).collect()
}

/// The first `Geometry` section's `NoFill`: 0 when the path takes the fill.
fn no_fill(w: &Window, id: u32) -> f64 {
    let doc = w.state().session().document().unwrap();
    let at = doc.find_shape(Owner::Page(doc.pages.get(0).unwrap().id), id).unwrap();
    Resolver::new(doc)
        .resolve(&SheetRef::Shape(at), &CellAddr::in_section("Geometry", Some(0), "NoFill"))
        .and_then(|found| found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()))
        .unwrap_or(0.0)
}

/// A press at the first point of the page, the pointer taken through the others, and a release at the last, with `modifiers` held from the press.
fn stroke(w: &mut Window, points: &[(f64, f64)], modifiers: Modifiers) {
    let spots: Vec<_> = points.iter().map(|p| at(w, p.0, p.1)).collect();
    for _ in 0..3 {
        move_to(w, spots[0]);
    }
    hold(w, modifiers);
    button(w, spots[0], true);
    for spot in &spots[1..] {
        move_to(w, *spot);
    }
    if let Some(last) = spots.last() {
        button(w, *last, false);
    }
    hold(w, Modifiers::NONE);
    w.rest();
}

/// A straight run of `n` points on the page from `a` to `b`.
fn along(a: (f64, f64), b: (f64, f64), n: usize) -> Vec<(f64, f64)> {
    (0..n).map(|i| (a.0 + (b.0 - a.0) * i as f64 / (n - 1) as f64, a.1 + (b.1 - a.1) * i as f64 / (n - 1) as f64)).collect()
}

// ---- choosing a tool ----------------------------------------------------------------------------------------------------------------------

/// Each tool has its key (Ctrl+8 Rectangle, Ctrl+9 Ellipse, Ctrl+6 Line, Ctrl+5 Freeform, Ctrl+7 Arc, Ctrl+4 Pencil), is a mode that stays on, and Escape and Ctrl+1 end it.
#[test]
fn each_drawing_tool_has_its_key_and_stays_on_until_the_pointer_tool_or_escape() {
    let mut w = edit_window([1280.0, 900.0]);
    for (code, name) in
        [(Key::Num8, "rectangle"), (Key::Num9, "ellipse"), (Key::Num6, "line"), (Key::Num5, "freeform"), (Key::Num7, "arc"), (Key::Num4, "pencil")]
    {
        key(&mut w, code, ctrl());
        assert_eq!(tool(&w), name, "{name}");
        assert_eq!(w.state().window_state()["tool"], json!(name));
    }
    // The tool stays on after a drawing, and Ctrl+1 is the way back.
    key(&mut w, Key::Num8, ctrl());
    stroke(&mut w, &[(2.0, 2.0), (4.0, 3.0)], Modifiers::NONE);
    assert_eq!(tool(&w), "rectangle");
    key(&mut w, Key::Num1, ctrl());
    assert_eq!(tool(&w), "pointer");
    // Escape is the way back too, with the drag first: a drag dropped sends nothing.
    run_command(&mut w, "tool.pencil", json!({}));
    let before = w.state().session().revision();
    let (from, to) = (at(&w, 6.0, 6.0), at(&w, 8.0, 7.0));
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    move_to(&mut w, to);
    assert_eq!(w.state().window_state()["drawing"]["tool"], json!("pencil"), "a stroke is under way");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().window_state()["drawing"], Value::Null, "Escape drops the stroke");
    button(&mut w, to, false);
    w.rest();
    assert_eq!(w.state().session().revision(), before, "and nothing was sent");
    assert_eq!(tool(&w), "pencil", "the tool is still on after the drag Escape dropped");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(tool(&w), "pointer", "a second Escape leaves the tool");
}

/// The Drawing Tools menu lists the six, each a row that runs its tool's command, and its button shows the tool used last.
#[test]
fn the_drawing_tools_menu_lists_the_six_tools_and_its_button_shows_the_last_one_used() {
    let mut w = edit_window([1600.0, 900.0]);
    assert_eq!(w.state().last_drawing_tool(), None);
    assert!(w.state().element("ribbon.Home.DrawingToolsMenu").is_some_and(|e| e.enabled), "the menu works");
    press(&mut w, "ribbon.Home.DrawingToolsMenu");
    w.rest();
    for name in ["RectangleTool", "EllipseTool", "LineTool", "FreeformTool", "ArcTool", "PencilTool"] {
        assert!(w.state().element(&format!("menu.{name}")).is_some_and(|e| e.enabled), "{name} is a row of the menu and works");
    }
    press(&mut w, "menu.EllipseTool");
    w.rest();
    assert_eq!(tool(&w), "ellipse");
    assert_eq!(w.state().last_drawing_tool(), Some(diacraft_engine::draw::pen::Tool::Ellipse));
    // The Pointer Tool is back, and the menu's button still shows the last drawing tool.
    run_command(&mut w, "tool.pointer", json!({}));
    assert_eq!(w.state().last_drawing_tool(), Some(diacraft_engine::draw::pen::Tool::Ellipse));
}

// ---- the tools that read the two ends of a drag --------------------------------------------------------------------------------------------

/// Rectangle: a drag draws one, on the grid, selects it and is one undo step; the tool is still on and the next drag draws another.
#[test]
fn a_drag_with_the_rectangle_tool_draws_a_rectangle_on_the_grid_and_selects_it() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "tool.rectangle", json!({}));
    let revision = w.state().session().revision();
    drag_with(&mut w, (2.04, 2.03), (5.02, 3.97), Modifiers::NONE);
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 1);
    assert_eq!(w.state().session().revision(), revision + 1, "one command");
    let (x, y, width, height) = frame_of(&w, ids[0]);
    assert!(near(x, 2.0) && near(y, 2.0) && near(width, 3.0) && near(height, 2.0), "on the grid: {x} {y} {width} {height}");
    assert_eq!(kinds(&w, ids[0]), ["MoveTo", "LineTo", "LineTo", "LineTo", "LineTo"]);
    assert_eq!(w.state().selection(), ids.as_slice(), "the new shape is selected");
    assert_eq!(w.state().window_state()["drawing"], Value::Null);
    assert_eq!(tool(&w), "rectangle");
    drag_with(&mut w, (7.0, 2.0), (9.0, 3.0), Modifiers::NONE);
    assert_eq!(shape_ids(&w).len(), 2, "the tool is still on");
    undo(&mut w);
    assert_eq!(shape_ids(&w).len(), 1, "one undo step takes the second back");
    undo(&mut w);
    assert!(shape_ids(&w).is_empty());
    assert_eq!(w.state().ui_panics(), 0);
}

/// The press and release of a click draw nothing, and the preview of a drag is the engine's outline of what the release will make.
#[test]
fn a_click_draws_nothing_and_a_drag_in_progress_is_shown_before_it_is_sent() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "tool.ellipse", json!({}));
    let revision = w.state().session().revision();
    let spot = at(&w, 4.0, 4.0);
    crate::support::click(&mut w, spot);
    w.rest();
    assert!(shape_ids(&w).is_empty() && w.state().session().revision() == revision, "a click draws nothing");
    let (from, to) = (at(&w, 3.0, 3.0), at(&w, 6.0, 5.0));
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    for step in 1..=5 {
        move_to(&mut w, from + (to - from) * (step as f32 / 5.0));
    }
    let drawing = w.state().window_state()["drawing"].clone();
    assert_eq!(
        (drawing["tool"].as_str(), drawing["continuing"].is_null(), drawing["closing"].as_bool()),
        (Some("ellipse"), true, Some(false)),
        "{drawing}"
    );
    let corners = w.state().window_state()["drag"]["corners"].clone();
    assert_eq!(corners, json!([[3.0, 3.0], [6.0, 5.0]]), "the box the drag has made so far, on the grid");
    assert!(shape_ids(&w).is_empty(), "nothing is sent while the button is down");
    button(&mut w, to, false);
    w.rest();
    assert_eq!(shape_ids(&w).len(), 1);
}

/// Shift makes the Rectangle a square and the Ellipse a circle, and holds the Line and the Arc to a step of 45 degrees (the end is the pointer's, not the grid's).
#[test]
fn shift_makes_a_square_a_circle_and_holds_a_line_to_a_step_of_45_degrees() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "tool.rectangle", json!({}));
    drag_with(&mut w, (2.0, 2.0), (6.0, 4.0), Modifiers::SHIFT);
    let (_, _, width, height) = frame_of(&w, shape_ids(&w)[0]);
    assert!(near(width, 4.0) && near(height, 4.0), "a square: {width} x {height}");
    run_command(&mut w, "tool.ellipse", json!({}));
    drag_with(&mut w, (7.0, 2.0), (9.0, 6.0), Modifiers::SHIFT);
    let (_, _, width, height) = frame_of(&w, shape_ids(&w)[1]);
    assert!(near(width, 4.0) && near(height, 4.0), "a circle: {width} x {height}");
    run_command(&mut w, "tool.line", json!({}));
    drag_with(&mut w, (2.0, 7.0), (6.0, 7.5), Modifiers::SHIFT);
    let (_, _, width, height) = frame_of(&w, shape_ids(&w)[2]);
    assert!(width > 3.9 && height < 1.0e-6, "a level line: {width} x {height}");
}

/// Line draws a segment; a press on the end vertex of the selected line goes on from it as one shape; Ctrl+Z takes the last segment away.
#[test]
fn a_press_on_the_end_vertex_of_the_selected_line_goes_on_from_it_as_one_shape() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "tool.line", json!({}));
    drag_with(&mut w, (2.0, 2.0), (5.0, 2.0), Modifiers::NONE);
    let id = shape_ids(&w)[0];
    assert_eq!(kinds(&w, id), ["MoveTo", "LineTo"]);
    // The next drag begins on the end vertex (a hair off it: within the reach of a handle) and goes up.
    drag_with(&mut w, (5.03, 2.03), (5.0, 5.0), Modifiers::NONE);
    assert_eq!(shape_ids(&w), vec![id], "still one shape");
    assert_eq!(kinds(&w, id), ["MoveTo", "LineTo", "LineTo"]);
    assert_eq!(w.state().selection(), [id].as_slice());
    let (x, y, width, height) = frame_of(&w, id);
    assert!(near(x, 2.0) && near(y, 2.0) && near(width, 3.0) && near(height, 3.0), "the box grew to take it in: {x} {y} {width} {height}");
    // A drag that begins elsewhere is a new line.
    drag_with(&mut w, (8.0, 2.0), (9.0, 4.0), Modifiers::NONE);
    assert_eq!(shape_ids(&w).len(), 2);
    undo(&mut w);
    assert_eq!(shape_ids(&w), vec![id]);
    undo(&mut w);
    assert_eq!(kinds(&w, id), ["MoveTo", "LineTo"], "Ctrl+Z takes the segments away in reverse order");
}

/// A stroke let go over the first vertex of the path it goes on from closes it, and it takes the fill.
#[test]
fn a_segment_let_go_over_the_first_vertex_closes_the_path_and_it_takes_the_fill() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "tool.line", json!({}));
    drag_with(&mut w, (2.0, 2.0), (6.0, 2.0), Modifiers::NONE);
    let id = shape_ids(&w)[0];
    drag_with(&mut w, (6.0, 2.0), (4.0, 5.0), Modifiers::NONE);
    assert_eq!(no_fill(&w, id), 1.0, "open so far");
    // Back to the first vertex, a hair off it.
    let (from, to) = (at(&w, 4.0, 5.0), at(&w, 2.03, 2.03));
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    for step in 1..=5 {
        move_to(&mut w, from + (to - from) * (step as f32 / 5.0));
    }
    assert_eq!(w.state().window_state()["drawing"]["closing"], json!(true), "the preview says it will close");
    button(&mut w, to, false);
    w.rest();
    assert_eq!(kinds(&w, id), ["MoveTo", "LineTo", "LineTo", "LineTo"]);
    assert_eq!(no_fill(&w, id), 0.0, "a closed shape is filled");
}

/// Arc draws a curved segment: a move and an ArcTo.
#[test]
fn a_drag_with_the_arc_tool_draws_an_arc() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "tool.arc", json!({}));
    drag_with(&mut w, (2.0, 3.0), (6.0, 3.0), Modifiers::NONE);
    let id = shape_ids(&w)[0];
    assert_eq!(kinds(&w, id), ["MoveTo", "ArcTo"]);
    let (_, _, width, height) = frame_of(&w, id);
    assert!(near(width, 4.0) && height > 0.5 && height < 1.0, "{width} x {height}");
}

// ---- the tools that read the stroke -------------------------------------------------------------------------------------------------------

/// Pencil: a stroke of a straight run and then a corner is lines, and a stroke round part of a circle is an arc.
#[test]
fn the_pencil_makes_lines_and_arcs_from_the_stroke() {
    let mut w = edit_window([1600.0, 900.0]);
    run_command(&mut w, "tool.pencil", json!({}));
    let mut corner = along((2.0, 2.0), (5.0, 2.0), 20);
    corner.extend(along((5.0, 2.0), (5.0, 4.0), 12).into_iter().skip(1));
    stroke(&mut w, &corner, Modifiers::NONE);
    let id = shape_ids(&w)[0];
    assert_eq!(kinds(&w, id), ["MoveTo", "LineTo", "LineTo"], "two lines");
    // A quarter of a circle of radius 2.5.
    let arc: Vec<(f64, f64)> =
        (0..=24).map(|i| (7.0 + 2.5 * (f64::from(i) * 3.75).to_radians().cos(), 3.0 + 2.5 * (f64::from(i) * 3.75).to_radians().sin())).collect();
    stroke(&mut w, &arc, Modifiers::NONE);
    let second = shape_ids(&w)[1];
    assert_eq!(kinds(&w, second), ["MoveTo", "ArcTo"], "an arc");
    assert!(w.state().window_state()["drawing"].is_null());
}

/// Freeform: a wavy stroke is one smooth curve, a NURBS row; its smoothing is a window setting, refused in words when it is out of range.
#[test]
fn the_freeform_tool_makes_a_smooth_curve_and_its_smoothing_is_a_window_setting() {
    let mut w = edit_window([1600.0, 900.0]);
    run_command(&mut w, "tool.freeform", json!({}));
    let wave: Vec<(f64, f64)> = (0..50).map(|i| (2.0 + f64::from(i) * 0.12, 4.0 + (f64::from(i) * 0.25).sin())).collect();
    stroke(&mut w, &wave, Modifiers::NONE);
    let id = shape_ids(&w)[0];
    assert_eq!(kinds(&w, id), ["MoveTo", "NURBSTo"]);
    assert_eq!(w.state().window_state()["freeformSmoothing"], json!(40.0), "40 percent at first");
    run_command(&mut w, "window.freeformSmoothing", json!({"percent": 80}));
    assert_eq!(w.state().window_state()["freeformSmoothing"], json!(80.0));
    assert!(near(w.state().freeform_smoothing(), 0.8));
    let e = w.state_mut().execute("window.freeformSmoothing", &json!({"percent": 150})).unwrap_err();
    assert!(e.to_string().contains("from 0 to 100"), "{e}");
    assert!(near(w.state().freeform_smoothing(), 0.8), "a refusal changes nothing");
}

/// A stroke of the Pencil that comes back to where it began closes: the shape is filled.
#[test]
fn a_pencil_stroke_that_comes_back_to_its_start_is_closed_and_takes_the_fill() {
    let mut w = edit_window([1600.0, 900.0]);
    run_command(&mut w, "tool.pencil", json!({}));
    let mut ring: Vec<(f64, f64)> =
        (0..=36).map(|i| (5.0 + 2.0 * (f64::from(i) * 10.0).to_radians().cos(), 4.0 + 1.5 * (f64::from(i) * 10.0).to_radians().sin())).collect();
    // Let go a hair from the first point.
    if let Some(last) = ring.last_mut() {
        *last = (7.02, 4.02);
    }
    stroke(&mut w, &ring, Modifiers::NONE);
    let id = shape_ids(&w)[0];
    assert_eq!(no_fill(&w, id), 0.0, "closed and filled");
}

// ---- the Draw tab ---------------------------------------------------------------------------------------------------------------------------

/// Close Shape (Draw, Create) closes the open path of the selected shape, and is greyed out with nothing selected.
#[test]
fn close_shape_on_the_draw_tab_closes_the_selected_open_path() {
    let mut w = edit_window([2400.0, 900.0]);
    run_command(&mut w, "window.tab", json!({"tab": "Draw"}));
    assert!(w.state().element("ribbon.Draw.InkShapeClose").is_some_and(|e| !e.enabled), "nothing is selected");
    run_command(&mut w, "tool.line", json!({}));
    drag_with(&mut w, (2.0, 2.0), (6.0, 2.0), Modifiers::NONE);
    drag_with(&mut w, (6.0, 2.0), (4.0, 5.0), Modifiers::NONE);
    let id = shape_ids(&w)[0];
    run_command(&mut w, "tool.pointer", json!({}));
    w.rest();
    assert!(w.state().element("ribbon.Draw.InkShapeClose").is_some_and(|e| e.enabled), "an open shape is selected");
    press(&mut w, "ribbon.Draw.InkShapeClose");
    w.rest();

    assert_eq!(kinds(&w, id), ["MoveTo", "LineTo", "LineTo", "LineTo"]);
    assert_eq!(no_fill(&w, id), 0.0);
    undo(&mut w);
    assert_eq!(no_fill(&w, id), 1.0, "one undo step");
}

/// The Snap switch decides "whether shapes pull into alignment while being drawn or moved". The drawing tools are
/// among the shapes being drawn: with Snap on a Rectangle starts and ends on the grid, and with the switch off (`snap.set {snap: false}`, the Snap & Glue dialog's box, View >
/// Visual Aids > Snap) it lands where the pointer put it. The tools' evidence said "if it must include the Draw tools, keep P": it does, and this is the test.
#[test]
fn the_snap_switch_decides_whether_a_drawing_tool_lands_on_the_grid() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "tool.rectangle", json!({}));
    drag_with(&mut w, (2.04, 2.03), (5.02, 3.97), Modifiers::NONE);
    let first = shape_ids(&w)[0];
    let (x, y, width, height) = frame_of(&w, first);
    assert!(near(x, 2.0) && near(y, 2.0) && near(width, 3.0) && near(height, 2.0), "Snap on: on the grid, {x} {y} {width} {height}");
    run_command(&mut w, "snap.set", json!({"snap": false}));
    drag_with(&mut w, (6.04, 2.03), (9.02, 3.97), Modifiers::NONE);
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 2);
    let (x, y, width, height) = frame_of(&w, ids[1]);
    assert!(
        near(x, 6.04) && near(y, 2.03) && near(x + width, 9.02) && near(y + height, 3.97),
        "Snap off: where the pointer put it, {x} {y} {width} {height}"
    );
    // The Ellipse and the Line read the same switch.
    run_command(&mut w, "tool.ellipse", json!({}));
    drag_with(&mut w, (2.04, 6.03), (4.02, 7.97), Modifiers::NONE);
    let ellipse = shape_ids(&w)[2];
    let (x, y, width, _) = frame_of(&w, ellipse);
    assert!(near(x, 2.04) && near(y, 6.03) && near(x + width, 4.02), "an Ellipse with Snap off is not on the grid: {x} {y} {width}");
    run_command(&mut w, "snap.set", json!({"snap": true}));
    drag_with(&mut w, (6.04, 6.03), (8.02, 7.97), Modifiers::NONE);
    let again = shape_ids(&w)[3];
    let (x, y, width, height) = frame_of(&w, again);
    assert!(near(x, 6.0) && near(y, 6.0) && near(width, 2.0) && near(height, 2.0), "Snap on again: on the grid, {x} {y} {width} {height}");
    assert_eq!(w.state().ui_panics(), 0);
}
