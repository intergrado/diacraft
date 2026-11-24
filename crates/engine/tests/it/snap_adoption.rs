//! B: the changes of the design's table ("What users see change at adoption" in the craft-snap design), each on the vectors of its spec rows (the
//! snapping-adoption spec: R and V numbers are in the test names). The settings are the spec's "Settings for vectors": the Snap and Glue switches, guides and the
//! dynamic grid on, the grid and the ruler off, the options of a new drawing's Snap to list otherwise; every shape a plain rectangle with a visible line; paper
//! inches, y up, 96 screen points to an inch, on a 20 by 20 inch page.

use diacraft_doc::{Cell, CellAddr, Document, ForeignData, RowKey, ShapeType, Value};
use diacraft_engine::Session;
use diacraft_engine::draw::{Turn, connection_points, drop_shape, rotate_shapes};
use diacraft_engine::snap::guides::add_guide;
use diacraft_engine::snap::{
    Axis, DEFAULT_ZOOM, Extras, Glue, Nudge, Query, Rect, SnapMark, SnapPage, SnapSettings, Snapped, Subject, Target, Turn as TurnOf, Use, bounds_of,
    box_of, rotation_step, snap, snap_ext, snap_resize, snap_resize_ext, snap_turn,
};
use diacraft_engine::stencil::find_master;
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use serde_json::json;

use crate::{blank_session, run};

/// A page of rectangles, each `(id, x0, y0, x1, y1)` in paper inches.
fn page_of(shapes: &[(u32, f64, f64, f64, f64)]) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20");
    for (id, x0, y0, x1, y1) in shapes {
        let (w, h) = (x1 - x0, y1 - y0);
        page = page.shape(
            ShapeBuilder::new(*id)
                .cell("PinX", &((x0 + x1) / 2.0).to_string())
                .cell("PinY", &((y0 + y1) / 2.0).to_string())
                .cell("Width", &w.to_string())
                .cell("Height", &h.to_string())
                .cell("LocPinX", &(w / 2.0).to_string())
                .cell("LocPinY", &(h / 2.0).to_string())
                .cell("Angle", "0"),
        );
    }
    DocBuilder::new().page(1, page).build()
}

/// As [`page_of`], with a `LinePattern` for the shapes named: `(id, pattern)`.
fn page_with_patterns(shapes: &[(u32, f64, f64, f64, f64)], patterns: &[(u32, &str)]) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20");
    for (id, x0, y0, x1, y1) in shapes {
        let (w, h) = (x1 - x0, y1 - y0);
        let mut shape = ShapeBuilder::new(*id)
            .cell("PinX", &((x0 + x1) / 2.0).to_string())
            .cell("PinY", &((y0 + y1) / 2.0).to_string())
            .cell("Width", &w.to_string())
            .cell("Height", &h.to_string())
            .cell("LocPinX", &(w / 2.0).to_string())
            .cell("LocPinY", &(h / 2.0).to_string())
            .cell("Angle", "0");
        if let Some((_, pattern)) = patterns.iter().find(|(p, _)| p == id) {
            shape = shape.cell("LinePattern", pattern);
        }
        page = page.shape(shape);
    }
    DocBuilder::new().page(1, page).build()
}

fn vector_settings() -> SnapSettings {
    let mut s = SnapSettings::none();
    s.snap = true;
    s.glue = true;
    s.guides = true;
    s.glue_guides = true;
    s.dynamic_grid = true;
    s.snap_connection_points = true;
    s.snap_vertices = true;
    s.snap_geometry = true;
    s.intersections = true;
    s
}

fn drag(doc: &Document, settings: &SnapSettings, r: Rect, moving: &[u32]) -> Snapped {
    let page = SnapPage::read(doc, 1).unwrap();
    snap(doc, &page, settings, &Query { subject: Subject::Box(r), using: Use::Place, moving, zoom: 96.0, own_points: &[] })
}

fn landed(r: Rect, s: &Snapped) -> (f64, f64, f64, f64) {
    (r.x0 + s.dx, r.y0 + s.dy, r.x1 + s.dx, r.y1 + s.dy)
}

fn near4(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    [(a.0, b.0), (a.1, b.1), (a.2, b.2), (a.3, b.3)].iter().all(|(x, y)| (x - y).abs() < 1e-9)
}

// ---------------------------------------------------------------------------------------------------------------- row 13, shape to shape like with like

#[test]
fn a_top_meets_another_shapes_top_and_never_its_bottom_or_a_middle_a_top() {
    let doc = page_of(&[(1, 2.0, 2.0, 3.0, 3.0)]);
    let settings = vector_settings();
    // Its top is 0.03 inch from A's bottom line: no snap (any side to any line gave (5, 1.5) to (6, 2)).
    let r = Rect { x0: 5.0, y0: 1.47, x1: 6.0, y1: 1.97 };
    let out = drag(&doc, &settings, r, &[]);
    assert!(out.dx == 0.0 && out.dy == 0.0 && out.hits.is_empty(), "{out:?}");
    // Its top is 0.03 inch from A's top: it lands on it.
    let r = Rect { x0: 5.0, y0: 2.47, x1: 6.0, y1: 2.97 };
    let out = drag(&doc, &settings, r, &[]);
    assert!(near4(landed(r, &out), (5.0, 2.5, 6.0, 3.0)), "{out:?}");
    assert!(out.hits.iter().any(|h| h.target == Target::Alignment && h.axis == Some(Axis::Y) && h.shape == Some(1)), "{out:?}");
    // Its middle is 0.03 inch from A's top: no snap (any side to any line gave (5, 2.75) to (6, 3.25)).
    let r = Rect { x0: 5.0, y0: 2.72, x1: 6.0, y1: 3.22 };
    let out = drag(&doc, &settings, r, &[]);
    assert!(out.dx == 0.0 && out.dy == 0.0 && out.hits.is_empty(), "{out:?}");
}

#[test]
fn a_guide_takes_any_side_of_the_box() {
    let mut doc = page_of(&[(1, 2.0, 2.0, 3.0, 3.0)]);
    add_guide(&mut doc, 1, Axis::Y, 4.03, 0).unwrap();
    let r = Rect { x0: 5.0, y0: 3.5, x1: 6.0, y1: 4.0 };
    let out = drag(&doc, &vector_settings(), r, &[]);
    assert!(near4(landed(r, &out), (5.0, 3.53, 6.0, 4.03)), "{out:?}");
    assert_eq!(out.hits.iter().find(|h| h.axis == Some(Axis::Y)).map(|h| h.target), Some(Target::Guide));
}

#[test]
fn the_page_margin_takes_any_side_of_the_box() {
    // A margin has no side, so it meets any side of the box. The left margin is at 0.25 inch: a box whose left edge is 0.03 from it lands on it.
    let doc = page_of(&[]);
    let r = Rect { x0: 0.28, y0: 8.0, x1: 1.28, y1: 9.0 };
    let out = drag(&doc, &vector_settings(), r, &[]);
    assert!(near4(landed(r, &out), (0.25, 8.0, 1.25, 9.0)), "{out:?}");
    assert_eq!(out.hits.first().map(|h| h.target), Some(Target::PageMargin));
}

// ---------------------------------------------------------------------------------------------------------------- row 12, alignment with shapes anywhere on the page

#[test]
fn a_box_lines_up_with_a_shape_ten_inches_away_and_the_mark_spans_both() {
    let doc = page_of(&[(1, 2.0, 2.0, 3.0, 3.0)]);
    let r = Rect { x0: 13.0, y0: 2.03, x1: 14.0, y1: 3.03 };
    let out = drag(&doc, &vector_settings(), r, &[]);
    assert!(near4(landed(r, &out), (13.0, 2.0, 14.0, 3.0)), "{out:?}");
    let hit = out.hits.iter().find(|h| h.axis == Some(Axis::Y)).unwrap();
    assert_eq!(
        (hit.target, hit.shape, hit.span),
        (Target::Alignment, Some(1), Some((2.0, 14.0))),
        "the mark runs from the shape to the box: {hit:?}"
    );
}

#[test]
fn a_query_that_only_aligns_gathers_nothing_from_the_index() {
    // The alignment box is the only shape option on: no dynamic grid, connection point, vertex, outline or intersection.
    let doc = page_of(&[(1, 2.0, 2.0, 3.0, 3.0), (2, 5.0, 9.0, 6.0, 10.0)]);
    let mut settings = SnapSettings::none();
    settings.snap = true;
    settings.alignment_box = true;
    let r = Rect { x0: 13.0, y0: 2.03, x1: 14.0, y1: 3.03 };
    let out = drag(&doc, &settings, r, &[]);
    assert!(near4(landed(r, &out), (13.0, 2.0, 14.0, 3.0)), "{out:?}");
    assert_eq!((out.visited, out.examined, out.candidates), (0, 0, 0), "a lookup in the sorted lines, no gather: {out:?}");
}

// ---------------------------------------------------------------------------------------------------------------- row 9, equal spacing offers any gap in the row

#[test]
fn a_box_may_repeat_any_gap_of_its_row() {
    // 1 by 1 inch shapes at x = 2, 4, 7 and 9 (gaps 1, 2, 1), all from y 5 to 6.
    let doc = page_of(&[(1, 2.0, 5.0, 3.0, 6.0), (2, 4.0, 5.0, 5.0, 6.0), (3, 7.0, 5.0, 8.0, 6.0), (4, 9.0, 5.0, 10.0, 6.0)]);
    let settings = vector_settings();
    // A fifth, 2 by 1 inches, dragged to x = 12.03: the 2-inch gap after the last, which only the neighbour's gap beyond (1 inch) offered before.
    let r = Rect { x0: 12.03, y0: 5.0, x1: 14.03, y1: 6.0 };
    let out = drag(&doc, &settings, r, &[]);
    assert!(near4(landed(r, &out), (12.0, 5.0, 14.0, 6.0)), "{out:?}");
    let hit = out.hits.iter().find(|h| h.axis == Some(Axis::X)).unwrap();
    assert_eq!((hit.target, hit.gap.map(|g| (g * 1e9).round() / 1e9)), (Target::Spacing, Some(2.0)), "{hit:?}");
    // Dragged to x = 11.02: the 1-inch gap, as before.
    let r = Rect { x0: 11.02, y0: 5.0, x1: 13.02, y1: 6.0 };
    let out = drag(&doc, &settings, r, &[]);
    assert!(near4(landed(r, &out), (11.0, 5.0, 13.0, 6.0)), "{out:?}");
}

// ---------------------------------------------------------------------------------------------------------------- row 11, far drawings snap as near ones do

#[test]
fn shapes_ten_million_inches_from_the_origin_snap_as_they_do_at_the_origin() {
    // The shapes (1 by 1 inch at x = 2, 4 and 6, from y 5 to 6) and the box dragged beside them, all moved by the same far amount.
    let at = |far: f64| {
        let shapes: Vec<(u32, f64, f64, f64, f64)> =
            [2.0, 4.0, 6.0].iter().zip(1u32..).map(|(x, id)| (id, x + far, 5.0 + far, x + 1.0 + far, 6.0 + far)).collect();
        let doc = page_of(&shapes);
        let r = Rect { x0: 8.03 + far, y0: 5.0 + far, x1: 9.53 + far, y1: 6.0 + far };
        let out = drag(&doc, &vector_settings(), r, &[]);
        (out.dx, out.dy, out.hits.iter().map(|h| (h.target, h.axis, h.anchor, h.shape)).collect::<Vec<_>>())
    };
    let near = at(0.0);
    for far in [1.0e3, 1.0e6, 1.0e7] {
        let moved = at(far);
        assert!((moved.0 - near.0).abs() < 1.0e-8 && (moved.1 - near.1).abs() < 1.0e-8, "{far}: {moved:?} against {near:?}");
        assert_eq!(moved.2, near.2, "{far}: the same hits");
    }
    assert!((near.0 + 0.03).abs() < 1.0e-9 && near.2.iter().any(|h| h.0 == Target::Spacing), "{near:?}");
}

// ---------------------------------------------------------------------------------------------------------------- row 15, a shape with no line is no line-up target

#[test]
fn a_shape_with_no_line_offers_no_alignment_lines_and_stays_in_equal_gap_rows() {
    let settings = vector_settings();
    // A (2, 2) to (3, 3) with `LinePattern` 0 and a 1 by 1 box dragged to (5, 2.03): no snap. With a line (pattern 1) it lands on it.
    let shapes = [(1, 2.0, 2.0, 3.0, 3.0)];
    let r = Rect { x0: 5.0, y0: 2.03, x1: 6.0, y1: 3.03 };
    let out = drag(&page_with_patterns(&shapes, &[(1, "0")]), &settings, r, &[]);
    assert!(out.dx == 0.0 && out.dy == 0.0 && out.hits.is_empty(), "{out:?}");
    let out = drag(&page_with_patterns(&shapes, &[(1, "1")]), &settings, r, &[]);
    assert!(near4(landed(r, &out), (5.0, 2.0, 6.0, 3.0)), "{out:?}");
    // The rule is for targets: the box that is dragged lines up whatever line the shape it comes from has.
    let doc = page_with_patterns(&[(1, 2.0, 2.0, 3.0, 3.0), (2, 5.0, 2.03, 6.0, 3.03)], &[(2, "0")]);
    let out = drag(&doc, &settings, r, &[2]);
    assert!(near4(landed(r, &out), (5.0, 2.0, 6.0, 3.0)), "{out:?}");
    // A (2, 5) to (3, 6) with no line, B (4, 5) to (5, 6) with one, and a 1.5 by 1 box dragged to (6.03, 5): A's gap to B (1 inch) is repeated after B.
    let doc = page_with_patterns(&[(1, 2.0, 5.0, 3.0, 6.0), (2, 4.0, 5.0, 5.0, 6.0)], &[(1, "0")]);
    let r = Rect { x0: 6.03, y0: 5.0, x1: 7.53, y1: 6.0 };
    let out = drag(&doc, &settings, r, &[]);
    assert!(near4(landed(r, &out), (6.0, 5.0, 7.5, 6.0)), "{out:?}");
    assert!(out.hits.iter().any(|h| h.target == Target::Spacing), "{out:?}");
}

#[test]
fn a_shape_with_no_line_pattern_set_anywhere_keeps_its_lines() {
    // A cell found nowhere reads 0, but a shape that sets no pattern at all is not a shape that draws none: only a pattern of 0 takes the lines.
    let doc = page_of(&[(1, 2.0, 2.0, 3.0, 3.0)]);
    let r = Rect { x0: 5.0, y0: 2.03, x1: 6.0, y1: 3.03 };
    let out = drag(&doc, &vector_settings(), r, &[]);
    assert!(near4(landed(r, &out), (5.0, 2.0, 6.0, 3.0)), "{out:?}");
}

// ---------------------------------------------------------------------------------------------------------------- row 1, Shift-move snaps along the free axis

/// A session holding `doc`, with the grid and the ruler off, as the spec's vectors have them.
fn session_of(doc: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    run(&mut s, "snap.set", json!({"grid": false, "ruler": false}));
    s
}

fn box_in(s: &Session, id: u32) -> Rect {
    box_of(s.document().unwrap(), 1, &[id]).unwrap()
}

#[test]
fn a_locked_move_snaps_along_the_free_axis_and_the_locked_delta_is_zero_to_the_bit() {
    // A 1 by 1 inch shape at (2, 2) to (3, 3) and an upright guide at x = 4.03: the move (1.0, 0.3) locked to x lands on the guide, and goes not at all in y.
    let mut doc = page_of(&[(1, 2.0, 2.0, 3.0, 3.0)]);
    add_guide(&mut doc, 1, Axis::X, 4.03, 0).unwrap();
    let mut s = session_of(doc);
    let r = run(&mut s, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.3, "snap": {"lock": "x"}}));
    let landed = box_in(&s, 1);
    assert!((landed.x0 - 3.03).abs() < 1e-9 && (landed.x1 - 4.03).abs() < 1e-9, "the right edge is on the guide: {landed:?}");
    assert!(landed.y0 == 2.0 && landed.y1 == 3.0, "the locked axis did not move, to the bit: {landed:?}");
    assert_eq!(r["snap"]["hits"][0]["target"], json!("guide"), "{r}");
    assert!(r["snap"]["hits"].as_array().unwrap().iter().all(|h| h["axis"] == json!("x")), "marks on the free axis only: {r}");
    assert_eq!(r["snap"]["held"].as_array().map(Vec::len), Some(1), "the guide holds it: {r}");
}

#[test]
fn a_pull_on_the_locked_axis_is_ignored() {
    // A level guide at y = 2.03 and the move (0.5, 0.01) locked to x: the shape does not go up, and the guide is not a hit.
    let mut doc = page_of(&[(1, 2.0, 2.0, 3.0, 3.0)]);
    add_guide(&mut doc, 1, Axis::Y, 2.03, 0).unwrap();
    let mut s = session_of(doc);
    let r = run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.5, "dy": 0.01, "snap": {"lock": "x"}}));
    let landed = box_in(&s, 1);
    assert!((landed.x0 - 2.5).abs() < 1e-9 && landed.y0 == 2.0 && landed.y1 == 3.0, "{landed:?}");
    assert_eq!(r["snap"]["hits"], json!([]), "{r}");
}

#[test]
fn a_lock_other_than_x_or_y_is_refused_in_words() {
    let mut s = session_of(page_of(&[(1, 2.0, 2.0, 3.0, 3.0)]));
    let err = s.execute("shape.move", &json!({"ids": [1], "dx": 1.0, "dy": 0.0, "snap": {"lock": "diagonal"}})).unwrap_err();
    assert!(err.to_string().contains("snap.lock"), "{err}");
}

// ---------------------------------------------------------------------------------------------------------------- the invariant, through the commands

/// A seeded generator: no clock, no entropy.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
    fn between(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.next()
    }
}

/// Where the anchor a hit names is on the landed box, on the hit's axis.
fn anchor_on(landed: Rect, axis: &str, anchor: &str) -> Option<f64> {
    let (lo, hi) = if axis == "x" { (landed.x0, landed.x1) } else { (landed.y0, landed.y1) };
    match anchor {
        "low" => Some(lo),
        "high" => Some(hi),
        "middle" => Some((lo + hi) / 2.0),
        _ => None,
    }
}

/// A page of eight boxes at random places with three guides near their edges, and the moving shape (id 1) at random.
fn random_page(rng: &mut Rng) -> (Document, Rect) {
    let mut shapes: Vec<(u32, f64, f64, f64, f64)> = Vec::new();
    let (w, h) = (rng.between(0.5, 2.0), rng.between(0.5, 2.0));
    let (x, y) = (rng.between(1.0, 15.0), rng.between(1.0, 15.0));
    let moving = Rect { x0: x, y0: y, x1: x + w, y1: y + h };
    shapes.push((1, x, y, x + w, y + h));
    for id in 2..10u32 {
        let (bw, bh) = (rng.between(0.4, 2.5), rng.between(0.4, 2.5));
        let (bx, by) = (rng.between(0.5, 16.0), rng.between(0.5, 16.0));
        shapes.push((id, bx, by, bx + bw, by + bh));
    }
    let mut doc = page_of(&shapes);
    for _ in 0..3 {
        let near = shapes.get(1 + (rng.next() * 8.0) as usize % 8).copied().unwrap_or((2, 3.0, 3.0, 4.0, 4.0));
        let at = [near.1, (near.1 + near.3) / 2.0, near.3][(rng.next() * 3.0) as usize % 3] + rng.between(-0.05, 0.05);
        add_guide(&mut doc, 1, if rng.next() < 0.5 { Axis::X } else { Axis::Y }, at, 0).unwrap();
    }
    (doc, moving)
}

#[test]
fn what_a_move_reports_is_where_the_shape_lands() {
    // Every line a `shape.move` with `snap` reports (guide, alignment, margin, grid, spacing) is at the place the landed box has the anchor it names: the guide
    // drawn is where the shape lands. 300 seeded pages, a drag of up to a pull in each direction from a start near a feature, free and locked.
    let mut rng = Rng(77);
    let (mut lines, mut spacing) = (0usize, 0usize);
    for case in 0..300 {
        let (doc, moving) = random_page(&mut rng);
        let mut s = session_of(doc);
        run(&mut s, "snap.set", json!({"alignment_box": true, "grid": case % 3 == 0}));
        let (dx, dy) = (rng.between(-0.4, 0.4), rng.between(-0.4, 0.4));
        let lock = ["x", "y"][case % 2];
        let snap = if case % 4 == 0 { json!({"zoom": 96.0, "lock": lock}) } else { json!({"zoom": 96.0}) };
        let r = run(&mut s, "shape.move", json!({"ids": [1], "dx": dx, "dy": dy, "snap": snap}));
        let landed = box_in(&s, 1);
        if case % 4 == 0 {
            // A locked move: the other axis did not move at all.
            let (still, was) = if lock == "x" { (landed.y0, moving.y0) } else { (landed.x0, moving.x0) };
            assert!((still - was).abs() < 1e-9, "case {case}: the locked axis moved: {still} against {was}");
        }
        for hit in r["snap"]["hits"].as_array().unwrap() {
            let (axis, anchor, line) = (hit["axis"].as_str(), hit["anchor"].as_str().unwrap(), hit["line"].as_f64());
            let (Some(axis), Some(line)) = (axis, line) else { continue };
            let Some(at) = anchor_on(landed, axis, anchor) else { continue };
            assert!(
                (at - line).abs() < 2.0e-6,
                "case {case}: a {} hit says {anchor} is at {line} on {axis}, and the box landed with it at {at}: {hit} {landed:?}",
                hit["target"]
            );
            lines += 1;
            spacing += usize::from(hit["target"] == json!("spacing"));
        }
    }
    assert!(lines > 150, "the cases exercise the invariant: {lines} lines");
    let _ = spacing;
}

#[test]
fn what_a_resize_reports_is_where_the_shape_lands() {
    let mut rng = Rng(78);
    let mut lines = 0usize;
    for case in 0..300 {
        let (doc, shape) = random_page(&mut rng);
        let mut s = session_of(doc);
        run(&mut s, "snap.set", json!({"alignment_box": true, "grid": case % 3 == 0}));
        // The right and top edges dragged by up to a pull; the left and bottom stay.
        let (w, h) = (shape.x1 - shape.x0 + rng.between(-0.3, 0.3), shape.y1 - shape.y0 + rng.between(-0.3, 0.3));
        let r = run(&mut s, "shape.resize", json!({"id": 1, "x": shape.x0, "y": shape.y0, "width": w, "height": h, "snap": {"zoom": 96.0}}));
        let landed = box_in(&s, 1);
        for hit in r["snap"]["hits"].as_array().unwrap() {
            let (axis, anchor, line) = (hit["axis"].as_str(), hit["anchor"].as_str().unwrap(), hit["line"].as_f64());
            let (Some(axis), Some(line)) = (axis, line) else { continue };
            let Some(at) = anchor_on(landed, axis, anchor) else { continue };
            assert!(
                (at - line).abs() < 2.0e-6,
                "case {case}: a {} hit says {anchor} is at {line} on {axis}, and the box landed with it at {at}: {hit} {landed:?}",
                hit["target"]
            );
            lines += 1;
        }
    }
    assert!(lines > 100, "the cases exercise the invariant: {lines} lines");
}

// ---------------------------------------------------------------------------------------------------------------- row 8, marks to the nearest aligned shape

/// The level or upright lines among the marks: `(axis, at, from, to, count, hit)`, the segment's ends in increasing order along the line.
fn line_marks(s: &Snapped, axis: Axis) -> Vec<(f64, f64, f64, u32, bool)> {
    let mut out: Vec<(f64, f64, f64, u32, bool)> = s
        .marks
        .iter()
        .filter_map(|m| match m {
            SnapMark::Line { axis: a, at, from, to, count, hit, .. } if *a == axis => {
                let (p, q) = if axis == Axis::Y { (from.0, to.0) } else { (from.1, to.1) };
                Some((*at, p.min(q), p.max(q), *count, *hit))
            }
            _ => None,
        })
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

#[test]
fn two_equal_boxes_side_by_side_mark_their_three_lines_each_from_one_to_the_other() {
    let doc = page_of(&[(1, 2.0, 2.0, 3.0, 3.0)]);
    let r = Rect { x0: 4.02, y0: 2.02, x1: 5.02, y1: 3.02 };
    let out = drag(&doc, &vector_settings(), r, &[]);
    assert!(near4(landed(r, &out), (4.02, 2.0, 5.02, 3.0)), "{out:?}");
    let marks = line_marks(&out, Axis::Y);
    assert_eq!(marks.len(), 3, "bottom, middle and top: {marks:?}");
    for ((at, from, to, count, _), want) in marks.iter().zip([2.0, 2.5, 3.0]) {
        assert!((at - want).abs() < 1e-9 && (from - 2.0).abs() < 1e-9 && (to - 5.02).abs() < 1e-9 && *count == 1, "{marks:?}");
    }
    // Only one of them is what the box was pulled to; the others hold where it lands. (Hits only gave one mark.)
    assert_eq!(marks.iter().filter(|m| m.4).count(), 1, "{marks:?}");
}

#[test]
fn in_a_row_of_three_the_line_runs_to_the_nearer_box_only_and_counts_both() {
    let doc = page_of(&[(1, 2.0, 2.0, 3.0, 3.0), (2, 4.0, 2.0, 5.0, 3.0)]);
    let r = Rect { x0: 6.5, y0: 2.03, x1: 7.5, y1: 3.03 };
    let out = drag(&doc, &vector_settings(), r, &[]);
    assert!(near4(landed(r, &out), (6.5, 2.0, 7.5, 3.0)), "{out:?}");
    let marks = line_marks(&out, Axis::Y);
    assert_eq!(marks.len(), 3, "{marks:?}");
    for (at, from, to, count, _) in &marks {
        assert!(
            (from - 4.0).abs() < 1e-9 && (to - 7.5).abs() < 1e-9 && *count == 2,
            "from the box to B at x = 4, and A beyond it is counted: {at} {marks:?}"
        );
    }
    // A shape's span in the hit is the mark's.
    let hit = out.hits.iter().find(|h| h.axis == Some(Axis::Y)).unwrap();
    assert_eq!(hit.span.map(|(a, b)| ((a * 1e6).round() / 1e6, (b * 1e6).round() / 1e6)), Some((4.0, 7.5)), "{hit:?}");
}

// ---------------------------------------------------------------------------------------------------------------- row 5, gap marks with values and chains

fn gaps(s: &Snapped) -> Vec<(u16, f64, String, bool)> {
    s.marks
        .iter()
        .filter_map(|m| match m {
            SnapMark::Gap { chain, value, label, own, .. } => Some((*chain, *value, label.clone(), *own)),
            _ => None,
        })
        .collect()
}

#[test]
fn three_equal_gaps_in_a_row_are_one_chain_each_labelled_with_its_length() {
    // 1 by 1 inch shapes from x = 2, 4 and 6 (y 5 to 6), and a 1.5 inch long box dragged to x = 8.03: it lands at x = 8 and the three gaps of the row are one chain.
    let doc = page_of(&[(1, 2.0, 5.0, 3.0, 6.0), (2, 4.0, 5.0, 5.0, 6.0), (3, 6.0, 5.0, 7.0, 6.0)]);
    let r = Rect { x0: 8.03, y0: 5.0, x1: 9.53, y1: 6.0 };
    let out = drag(&doc, &vector_settings(), r, &[]);
    assert!(near4(landed(r, &out), (8.0, 5.0, 9.5, 6.0)), "{out:?}");
    let g = gaps(&out);
    assert_eq!(g.len(), 3, "{g:?}");
    assert!(g.iter().all(|(chain, value, label, _)| *chain == g[0].0 && (value - 1.0).abs() < 1e-9 && label == "1 in"), "{g:?}");
    assert_eq!(g.iter().filter(|x| x.3).count(), 1, "one of them is the box's own gap: {g:?}");
    // Naive (today): two gaps marked, no labels.
}

#[test]
fn the_commands_tell_the_marks_with_their_labels() {
    let doc = page_of(&[(1, 2.0, 5.0, 3.0, 6.0), (2, 4.0, 5.0, 5.0, 6.0), (3, 6.0, 5.0, 7.0, 6.0), (4, 8.03, 5.0, 9.53, 6.0)]);
    let mut s = session_of(doc);
    let r = run(&mut s, "shape.move", json!({"ids": [4], "dx": 0.0, "dy": 0.0, "snap": {"zoom": 96.0}}));
    let marks = r["snap"]["marks"].as_array().unwrap();
    let gaps: Vec<_> = marks.iter().filter(|m| m["mark"] == json!("gap")).collect();
    assert_eq!(gaps.len(), 3, "{r}");
    assert!(gaps.iter().all(|m| m["label"] == json!("1 in") && m["chain"] == gaps[0]["chain"]), "{r}");
    assert!(marks.iter().any(|m| m["mark"] == json!("line") && m["targets"].as_array().is_some_and(|t| !t.is_empty())), "{r}");
}

// ---------------------------------------------------------------------------------------------------------------- row 6, size match with the shapes in view

/// B (id 2, from x 6 to 7.5) resized by its right side to `width` 1.97 (to x = 7.97), with `snap` as given; → where its right edge is, and the command's result.
fn resize_b(snap: serde_json::Value) -> (f64, serde_json::Value) {
    // A (id 1) is 2 inches wide, from x 3.4 to 5.4; B is 1 inch tall.
    let doc = page_of(&[(1, 3.4, 2.0, 5.4, 3.5), (2, 6.0, 2.0, 7.5, 3.0)]);
    let mut s = session_of(doc);
    let r = run(&mut s, "shape.resize", json!({"id": 2, "x": 6.0, "y": 2.0, "width": 1.97, "height": 1.0, "snap": snap}));
    (box_in(&s, 2).x1, r)
}

#[test]
fn a_resize_stops_at_the_width_of_a_shape_in_the_view_and_marks_it() {
    let (right, r) = resize_b(json!({"zoom": 96.0, "view": [0.0, 0.0, 10.0, 10.0]}));
    assert!((right - 8.0).abs() < 1e-9, "the width of A, 2 inches: {right} {r}");
    let marks = r["snap"]["marks"].as_array().unwrap();
    let size = marks.iter().find(|m| m["mark"] == json!("size") && m["hit"] == json!(true)).unwrap_or_else(|| panic!("{r}"));
    assert_eq!((size["label"].clone(), size["matched"]["shape"].clone()), (json!("2 in"), json!(1)), "{size}");
    let hit = r["snap"]["hits"].as_array().unwrap().iter().find(|h| h["target"] == json!("size")).unwrap_or_else(|| panic!("{r}"));
    assert_eq!((hit["axis"].clone(), hit["anchor"].clone(), hit["shape"].clone()), (json!("x"), json!("high"), json!(1)), "{hit}");
}

#[test]
fn a_shape_out_of_the_view_is_not_matched_and_with_no_view_every_shape_is() {
    // A wholly outside the view: no match, the edge stays where it was dragged. (Naive: the shapes gathered near B, not the view: A matched.)
    let (right, r) = resize_b(json!({"zoom": 96.0, "view": [5.5, 0.0, 10.0, 10.0]}));
    assert!((right - 7.97).abs() < 1e-9, "{right} {r}");
    assert!(r["snap"]["marks"].as_array().unwrap().iter().all(|m| m["mark"] != json!("size")), "{r}");
    // No view: every indexed shape counts.
    let (right, r) = resize_b(json!({"zoom": 96.0}));
    assert!((right - 8.0).abs() < 1e-9, "{right} {r}");
}

#[test]
fn a_view_that_is_not_four_numbers_is_refused_in_words() {
    let mut s = session_of(page_of(&[(1, 3.4, 2.0, 5.4, 3.5)]));
    for view in [json!([0.0, 0.0, 1.0]), json!("all"), json!([0.0, 0.0, 1.0, null])] {
        let err = s.execute("shape.resize", &json!({"id": 1, "x": 3.4, "y": 2.0, "width": 2.0, "height": 1.5, "snap": {"view": view}})).unwrap_err();
        assert!(err.to_string().contains("snap.view"), "{err}");
    }
}

// ---------------------------------------------------------------------------------------------------------------- row 2, a resize that keeps the proportions

/// A 2 by 1 inch shape (id 1, from (1, 1) to (3, 2)) and an upright guide at `guide`, resized to the box `(x, y, w, h)` with `snap`; → the box it lands in, and the result.
fn resize_with_a_guide(guide: f64, to: (f64, f64, f64, f64), snap: serde_json::Value) -> (Rect, serde_json::Value) {
    let mut doc = page_of(&[(1, 1.0, 1.0, 3.0, 2.0)]);
    add_guide(&mut doc, 1, Axis::X, guide, 0).unwrap();
    let mut s = session_of(doc);
    let r = run(&mut s, "shape.resize", json!({"id": 1, "x": to.0, "y": to.1, "width": to.2, "height": to.3, "snap": snap}));
    (box_in(&s, 1), r)
}

fn same_box(a: Rect, want: (f64, f64, f64, f64)) -> bool {
    near4((a.x0, a.y0, a.x1, a.y1), want)
}

#[test]
fn a_corner_keeps_the_proportions_and_lands_on_the_guide_inside_them() {
    // The top-right corner dragged to (4.97, 2.985) with (1, 1) fixed (on the diagonal, ratio 2) and a guide at x = 5: (1, 1) to (5, 3).
    let (b, r) = resize_with_a_guide(5.0, (1.0, 1.0, 3.97, 1.985), json!({"zoom": 96.0, "keep": "proportions"}));
    assert!(same_box(b, (1.0, 1.0, 5.0, 3.0)), "{b:?} {r}");
    assert!(((b.x1 - b.x0) / (b.y1 - b.y0) - 2.0).abs() < 1e-9, "the ratio holds: {b:?}");
    assert_eq!(r["snap"]["hits"][0]["target"], json!("guide"), "{r}");
    // Naive (the right side snapped alone): (1, 1) to (5, 2.985), ratio 2.015.
    // The guide at x = 5.2 is out of reach: the box goes where it was dragged.
    let (b, r) = resize_with_a_guide(5.2, (1.0, 1.0, 3.97, 1.985), json!({"zoom": 96.0, "keep": "proportions"}));
    assert!(same_box(b, (1.0, 1.0, 4.97, 2.985)), "{b:?} {r}");
}

#[test]
fn a_side_keeps_the_opposite_side_and_the_other_axis_grows_about_its_middle() {
    // The right side dragged to x = 3.97 with the guide at x = 4: the left side stays, the box is (1, 0.75) to (4, 2.25).
    let (b, r) = resize_with_a_guide(4.0, (1.0, 1.0, 2.97, 1.0), json!({"zoom": 96.0, "keep": "proportions"}));
    assert!(same_box(b, (1.0, 0.75, 4.0, 2.25)), "{b:?} {r}");
    assert!(((b.x1 - b.x0) / (b.y1 - b.y0) - 2.0).abs() < 1e-9, "{b:?}");
    // Naive (the other axis grown from the bottom): (1, 1) to (4, 2.5). Without keep, the side alone lands on the guide: (1, 1) to (4, 2).
    let (b, r) = resize_with_a_guide(4.0, (1.0, 1.0, 2.97, 1.0), json!({"zoom": 96.0}));
    assert!(same_box(b, (1.0, 1.0, 4.0, 2.0)), "{b:?} {r}");
}

#[test]
fn a_keep_other_than_proportions_is_refused_in_words() {
    let mut s = session_of(page_of(&[(1, 1.0, 1.0, 3.0, 2.0)]));
    let err = s.execute("shape.resize", &json!({"id": 1, "x": 1.0, "y": 1.0, "width": 3.0, "height": 1.5, "snap": {"keep": "centre"}})).unwrap_err();
    assert!(err.to_string().contains("snap.keep"), "{err}");
}

// ---------------------------------------------------------------------------------------------------------------- row 7, rotation match on the rotation handle

/// Shape 1 (the one that turns) at angle 0, and shape 2 turned `other` degrees, both 1 by 1 inch and far from each other.
fn turn_page(other: f64) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20");
    for (id, x, angle) in [(1u32, 3.0, 0.0), (2, 12.0, other.to_radians())] {
        page = page.shape(
            ShapeBuilder::new(id)
                .cell("PinX", &x.to_string())
                .cell("PinY", "5")
                .cell("Width", "1")
                .cell("Height", "1")
                .cell("LocPinX", "0.5")
                .cell("LocPinY", "0.5")
                .cell("Angle", &angle.to_string()),
        );
    }
    DocBuilder::new().page(1, page).build()
}

/// The turn of shape 1 to `raw` degrees with the pointer `points` screen points from the pin at 96 points to an inch: the degrees it comes to, and the snap.
fn turned_to(doc: &Document, raw: f64, points: f64) -> (f64, Snapped) {
    let page = SnapPage::read(doc, 1).unwrap();
    let turn = TurnOf { pivot: (3.0, 5.0), angle: raw.to_radians(), radius: points / 96.0, zoom: 96.0 };
    let out = snap_turn(doc, &page, &vector_settings(), &[1], turn);
    (out.turned.unwrap().to_degrees(), out)
}

#[test]
fn a_turn_stops_on_another_shapes_angle_within_reach_and_that_beats_the_step() {
    // The pointer 150 points from the pin: the pull is the angle whose tangent is 6 / 150, 2.29 degrees, and the step is 5 degrees.
    let doc = turn_page(33.0);
    let (a, out) = turned_to(&doc, 35.0, 150.0);
    assert!((a - 33.0).abs() < 1e-9, "a match within reach: {a}");
    assert!(out.hits.iter().any(|h| h.target == Target::Angle && h.shape == Some(2)), "{out:?}");
    let label = out.marks.iter().find_map(|m| match m {
        SnapMark::Angle { label, hit: true, matched: Some(2), .. } => Some(label.clone()),
        _ => None,
    });
    assert_eq!(label.as_deref(), Some("33 deg"), "{:?}", out.marks);
    // Raw 36 is 3 degrees from 33: outside reach, so the 5-degree step: 35. Raw 31 is 2 from it: 33.
    assert!((turned_to(&doc, 36.0, 150.0).0 - 35.0).abs() < 1e-9);
    assert!((turned_to(&doc, 31.0, 150.0).0 - 33.0).abs() < 1e-9);
    // Naive (the step only): 35 for raw 35.
    assert_ne!(turned_to(&doc, 35.0, 150.0).0, 35.0);
}

#[test]
fn angles_are_compared_over_a_full_turn_and_a_pointer_near_the_pin_matches_nothing() {
    // Another shape at 213 degrees is no match for 35 (a half-turn period would make 33 of it): the 5-degree step gives 35.
    let (a, _) = turned_to(&turn_page(213.0), 35.0, 150.0);
    assert!((a - 35.0).abs() < 1e-9, "{a}");
    // Within 6 points of the pin nothing matches: 4.8 points from it, the step is 15 degrees, and 35 goes to 30.
    let (a, out) = turned_to(&turn_page(33.0), 35.0, 4.8);
    assert!((a - 30.0).abs() < 1e-9, "{a} {out:?}");
}

#[test]
fn the_rotation_handles_steps_get_finer_with_the_distance_from_the_pin() {
    // 15 degrees under 110 points, 5 under 190, 1 under 300, a tenth beyond.
    assert_eq!([0.0, 109.9, 110.0, 189.9, 190.0, 299.9, 300.0, 1000.0].map(rotation_step), [15.0, 15.0, 5.0, 5.0, 1.0, 1.0, 0.1, 0.1]);
}

#[test]
fn shape_rotate_with_snap_equals_the_function_and_says_where_it_came_to() {
    let mut s = session_of(turn_page(33.0));
    let r = run(&mut s, "shape.rotate", json!({"ids": [1], "angle": 35.0, "snap": {"zoom": 96.0, "radius": 1.5625}}));
    assert_eq!(r["snap"]["angle"], json!(33.0), "{r}");
    assert_eq!(r["rotated"], json!(1), "{r}");
    assert_eq!(r["snap"]["hits"][0]["target"], json!("angle"), "{r}");
    let mark = r["snap"]["marks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["mark"] == json!("angle") && m["hit"] == json!(true))
        .unwrap_or_else(|| panic!("{r}"));
    assert_eq!((mark["label"].clone(), mark["matched"].clone()), (json!("33 deg"), json!(2)), "{mark}");
    // The shape is at 33 degrees now: a second turn to the same angle is no change.
    let again = run(&mut s, "shape.rotate", json!({"ids": [1], "angle": 33.0}));
    assert_eq!(again["rotated"], json!(0), "{again}");
    // Refused in words: no radius, and a turn by an amount.
    let err = s.execute("shape.rotate", &json!({"ids": [1], "angle": 10.0, "snap": true})).unwrap_err();
    assert!(err.to_string().contains("radius"), "{err}");
    let err = s.execute("shape.rotate", &json!({"ids": [1], "by": 10.0, "snap": {"radius": 1.0}})).unwrap_err();
    assert!(err.to_string().contains("angle"), "{err}");
}

// ---------------------------------------------------------------------------------------------------------------- row 4, an arrow key goes to the next snap place

/// A page of rectangles with a fixed grid of a quarter inch, each `(id, x0, y0, x1, y1)`.
fn page_with_a_quarter_grid(shapes: &[(u32, f64, f64, f64, f64)]) -> Document {
    let mut page = PageBuilder::new()
        .cell("PageWidth", "20")
        .cell("PageHeight", "20")
        .cell("XGridDensity", "0")
        .cell("YGridDensity", "0")
        .cell("XGridSpacing", "0.25")
        .cell("YGridSpacing", "0.25");
    for (id, x0, y0, x1, y1) in shapes {
        let (w, h) = (x1 - x0, y1 - y0);
        page = page.shape(
            ShapeBuilder::new(*id)
                .cell("PinX", &((x0 + x1) / 2.0).to_string())
                .cell("PinY", &((y0 + y1) / 2.0).to_string())
                .cell("Width", &w.to_string())
                .cell("Height", &h.to_string())
                .cell("LocPinX", &(w / 2.0).to_string())
                .cell("LocPinY", &(h / 2.0).to_string())
                .cell("Angle", "0"),
        );
    }
    DocBuilder::new().page(1, page).build()
}

/// One press of the right arrow (`dx` 1, `dy` 0) on shape 1, with `tick` inches when nothing is ahead; → the result.
fn press(s: &mut Session, dx: f64, dy: f64, tick: Option<f64>) -> serde_json::Value {
    let nudge = tick.map_or(json!({}), |t| json!({"tick": t}));
    run(s, "shape.move", json!({"ids": [1], "dx": dx, "dy": dy, "snap": {"zoom": 96.0, "nudge": nudge}}))
}

#[test]
fn each_press_goes_to_the_next_place_and_a_stop_on_a_guide_is_held() {
    // A fixed grid of 0.25 inch, the ruler off, a shape from (1, 1) to (2, 2) and an upright guide at x = 2.1: presses go 0.1 (onto the guide), 0.15 (the grid
    // lines at 1.25 and 2.25), then 0.25.
    let mut doc = page_with_a_quarter_grid(&[(1, 1.0, 1.0, 2.0, 2.0)]);
    add_guide(&mut doc, 1, Axis::X, 2.1, 0).unwrap();
    let mut s = session_of(doc);
    run(&mut s, "snap.set", json!({"grid": true, "ruler": false}));
    let mut lefts = Vec::new();
    let mut first = serde_json::Value::Null;
    for i in 0..3 {
        let r = press(&mut s, 1.0, 0.0, Some(0.125));
        if i == 0 {
            first = r;
        }
        lefts.push(box_in(&s, 1).x0);
    }
    for (got, want) in lefts.iter().zip([1.1, 1.25, 1.5]) {
        assert!((got - want).abs() < 1e-9, "{lefts:?}");
    }
    assert_eq!(first["snap"]["hits"][0]["target"], json!("guide"), "{first}");
    assert_eq!(first["snap"]["holds"].as_array().map(Vec::len), Some(1), "the stop on the guide is held: {first}");
    // Naive (one pitch each): 0.25, 0.25, 0.25. Naive (a place the box is already on counts as ahead): 0 at the first press.
}

#[test]
fn a_press_goes_as_far_as_the_next_like_line_of_another_shape_and_a_press_with_nothing_ahead_goes_a_tick() {
    // The grid and the ruler off, no guide, a shape (1, 1) to (2, 2) and another C (4, 5) to (5, 6): one right press moves 3, the box's right side, middle and left side
    // onto C's all at once. (Any line to any line: 2, the right side onto C's left.)
    let mut s = session_of(page_of(&[(1, 1.0, 1.0, 2.0, 2.0), (2, 4.0, 5.0, 5.0, 6.0)]));
    press(&mut s, 1.0, 0.0, Some(0.125));
    assert!((box_in(&s, 1).x0 - 4.0).abs() < 1e-9, "{:?}", box_in(&s, 1));
    // With the dynamic grid off, nothing is ahead: the press is the tick, 0.125.
    let mut s = session_of(page_of(&[(1, 1.0, 1.0, 2.0, 2.0), (2, 4.0, 5.0, 5.0, 6.0)]));
    run(&mut s, "snap.set", json!({"dynamic_grid": false, "alignment_box": false}));
    press(&mut s, 1.0, 0.0, Some(0.125));
    assert!((box_in(&s, 1).x0 - 1.125).abs() < 1e-9, "{:?}", box_in(&s, 1));
    // Without a tick the engine works out the ruler's at the zoom: an eighth of an inch at 96 points to the inch.
    press(&mut s, 1.0, 0.0, None);
    assert!((box_in(&s, 1).x0 - 1.25).abs() < 1e-9, "{:?}", box_in(&s, 1));
}

#[test]
fn the_ruler_option_makes_every_tick_a_stop_and_a_press_goes_back_as_it_goes_forward() {
    // The ruler on, ticks 0.125 apart at the zoom, the box at (1.03, 1) to (2.03, 2): each of its three lines onto the next tick, 0.095.
    let mut s = session_of(page_with_a_quarter_grid(&[(1, 1.03, 1.0, 2.03, 2.0)]));
    run(&mut s, "snap.set", json!({"grid": false, "ruler": true, "dynamic_grid": false}));
    press(&mut s, 1.0, 0.0, Some(0.125));
    assert!((box_in(&s, 1).x0 - 1.125).abs() < 1e-9, "{:?}", box_in(&s, 1));
    // Backwards: from a box on a grid line, the next one behind.
    let mut s = session_of(page_with_a_quarter_grid(&[(1, 1.0, 1.0, 2.0, 2.0)]));
    run(&mut s, "snap.set", json!({"grid": true, "ruler": false, "dynamic_grid": false}));
    press(&mut s, -1.0, 0.0, Some(0.125));
    assert!((box_in(&s, 1).x0 - 0.75).abs() < 1e-9, "{:?}", box_in(&s, 1));
    // Up, along y.
    let mut s = session_of(page_with_a_quarter_grid(&[(1, 1.0, 1.0, 2.0, 2.0)]));
    run(&mut s, "snap.set", json!({"grid": true, "ruler": false, "dynamic_grid": false}));
    press(&mut s, 0.0, 1.0, Some(0.125));
    assert!((box_in(&s, 1).y0 - 1.25).abs() < 1e-9, "{:?}", box_in(&s, 1));
}

#[test]
fn a_press_needs_one_direction_and_a_tick_above_zero() {
    let mut s = session_of(page_of(&[(1, 1.0, 1.0, 2.0, 2.0)]));
    for params in [json!({"dx": 1.0, "dy": 1.0}), json!({"dx": 0.0, "dy": 0.0})] {
        let mut p = json!({"ids": [1], "snap": {"nudge": {}}});
        p.as_object_mut().unwrap().extend(params.as_object().unwrap().clone());
        let err = s.execute("shape.move", &p).unwrap_err();
        assert!(err.to_string().contains("snap.nudge"), "{err}");
    }
    let err = s.execute("shape.move", &json!({"ids": [1], "dx": 1.0, "dy": 0.0, "snap": {"nudge": {"tick": 0.0}}})).unwrap_err();
    assert!(err.to_string().contains("tick"), "{err}");
    let err = s.execute("shape.move", &json!({"ids": [1], "dx": 1.0, "dy": 0.0, "snap": {"nudge": {"step": 1.0}}})).unwrap_err();
    assert!(err.to_string().contains("nudge"), "{err}");
}

// ---------------------------------------------------------------------------------------------------------------- row 14, a turned shape lines up by the box round its paths

/// An ellipse (the stencil's) dropped with its pin at (5, 5) and turned `degrees`; → the document and the ellipse's `ID`, width and height.
fn turned_ellipse(degrees: f64) -> (Document, u32, f64, f64) {
    let mut doc = page_of(&[]);
    let info = drop_shape(&mut doc, 1, find_master("ellipse").unwrap(), 5.0, 5.0).unwrap();
    if degrees != 0.0 {
        rotate_shapes(&mut doc, 1, &[info.id], Turn::To(degrees.to_radians())).unwrap();
    }
    (doc, info.id, info.width, info.height)
}

/// The half width and half height of the box round an ellipse of `w` by `h` turned `degrees`.
fn ellipse_half(w: f64, h: f64, degrees: f64) -> (f64, f64) {
    let (s, c) = degrees.to_radians().sin_cos();
    ((w * w / 4.0 * c * c + h * h / 4.0 * s * s).sqrt(), (w * w / 4.0 * s * s + h * h / 4.0 * c * c).sqrt())
}

#[test]
fn the_box_round_the_path_of_a_turned_ellipse_is_the_curves_extremes_not_its_corners() {
    let (doc, id, w, h) = turned_ellipse(30.0);
    let page = SnapPage::read(&doc, 1).unwrap();
    let index = page.shapes(&doc);
    let item = index.find(id).unwrap();
    let (hx, hy) = ellipse_half(w, h, 30.0);
    let b = item.path_box.expect("a turned shape has the box round its paths");
    // The curve is a Bezier ellipse: its extremes are the ellipse's to a few thousandths.
    assert!(((b.x1 - b.x0) - 2.0 * hx).abs() < 0.004 && ((b.y1 - b.y0) - 2.0 * hy).abs() < 0.004, "{b:?} against {hx} {hy}");
    assert!(((b.x0 + b.x1) / 2.0 - 5.0).abs() < 0.004 && ((b.y0 + b.y1) / 2.0 - 5.0).abs() < 0.004, "{b:?}");
    // The box round its corners is larger: 2.23 by 1.87 for the 2 by 1 ellipse of the spec's example.
    assert!(item.bbox.width() > b.x1 - b.x0 + 0.1 && item.bbox.height() > b.y1 - b.y0 + 0.1, "{:?} {b:?}", item.bbox);
    assert_eq!((index.bounds_by_paths, index.bounds_by_corners), (1, 0));
    // A shape that is not turned has no path box: its box is its bounds.
    let (doc, id, _, _) = turned_ellipse(0.0);
    let page = SnapPage::read(&doc, 1).unwrap();
    assert!(page.shapes(&doc).find(id).unwrap().path_box.is_none());
    let (doc, id, _, _) = turned_ellipse(90.0);
    let page = SnapPage::read(&doc, 1).unwrap();
    assert!(page.shapes(&doc).find(id).unwrap().path_box.is_none(), "a quarter turn is not turned");
}

#[test]
fn a_box_lines_up_with_the_extremes_of_a_turned_ellipse_and_not_with_the_corners_of_its_frame() {
    let (mut doc, _, w, h) = turned_ellipse(30.0);
    let (_, hy) = ellipse_half(w, h, 30.0);
    let top = 5.0 + hy;
    // A box whose top is 0.03 inch above the curve's top: it comes down onto it. (Naive, the box round the corners, top 5.93: no snap.)
    let r = Rect { x0: 8.0, y0: top + 0.03 - 1.0, x1: 9.0, y1: top + 0.03 };
    let out = drag(&doc, &vector_settings(), r, &[]);
    let landed_top = r.y1 + out.dy;
    assert!((landed_top - top).abs() < 0.004, "{landed_top} against {top}: {out:?}");
    assert!(out.hits.iter().any(|h| h.target == Target::Alignment && h.axis == Some(Axis::Y)), "{out:?}");
    // The command moves the ellipse itself by its path box: another shape A with its top at 5.69, and the ellipse's curve 0.0286 below it, lands its top on it.
    let a_top = top + 0.0286;
    doc = {
        let (d, ..) = turned_ellipse(30.0);
        let mut page_shapes = d;
        drop_shape(&mut page_shapes, 1, find_master("rectangle").unwrap(), 12.0, a_top - 0.5).unwrap();
        page_shapes
    };
    let mut s = session_of(doc);
    let rect_top = {
        let d = s.document().unwrap();
        let ids: Vec<u32> = (1..=2).collect();
        let b = box_of(d, 1, &ids[1..]).unwrap();
        b.y1
    };
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.0, "dy": 0.0, "snap": {"zoom": 96.0}}));
    let b = bounds_of(s.document().unwrap(), 1, &[1]).unwrap();
    assert_eq!(b.by_corners, 0);
    let b = b.rect;
    assert!((b.y1 - rect_top).abs() < 0.004, "the ellipse's curve comes to the top of the other shape: {} against {rect_top}", b.y1);
}

/// A master of one shape that draws a line of `rows` rows, and `n` instances of it turned 0.5 radians, 1 by 1 inch, on a grid of places.
fn turned_instances_of_a_big_master(n: u32, rows: u32) -> Document {
    let (master, page) = big_master_page(n, rows);
    DocBuilder::new().master(1, master).page(1, page).build()
}

/// The master and the page of [`turned_instances_of_a_big_master`], to add to.
fn big_master_page(n: u32, rows: u32) -> (MasterBuilder, PageBuilder) {
    let mut master_shape = ShapeBuilder::new(1).cell("Width", "1").cell("Height", "1").cell("LocPinX", "0.5").cell("LocPinY", "0.5");
    for i in 0..rows {
        let (row, t) = (RowKey::Ix(i), if i == 0 { "MoveTo" } else { "LineTo" });
        let v = (f64::from(i % 100) / 100.0).to_string();
        master_shape = master_shape.row_type("Geometry", Some(0), row.clone(), t).row_cell("Geometry", Some(0), row.clone(), "X", &v).row_cell(
            "Geometry",
            Some(0),
            row,
            "Y",
            &v,
        );
    }
    let mut page = PageBuilder::new().cell("PageWidth", "400").cell("PageHeight", "400");
    for i in 0..n {
        page = page.shape(
            ShapeBuilder::new(100 + i)
                .master(1)
                .cell("PinX", &(f64::from(i % 50) * 3.0 + 2.0).to_string())
                .cell("PinY", &(f64::from(i / 50) * 3.0 + 2.0).to_string())
                .cell("Width", "1")
                .cell("Height", "1")
                .cell("LocPinX", "0.5")
                .cell("LocPinY", "0.5")
                .cell("Angle", "0.5"),
        );
    }
    (MasterBuilder::new().shape(master_shape), page)
}

#[test]
fn a_page_of_turned_shapes_with_big_paths_spends_the_page_budget_and_the_rest_keep_their_corner_boxes_counted() {
    // 600 turned shapes of a master of 400 rows: the page pays for the first ones (400 points each against 131,072) and the rest keep the box round their corners.
    let doc = turned_instances_of_a_big_master(600, 400);
    let page = SnapPage::read(&doc, 1).unwrap();
    let index = page.shapes(&doc);
    let cost = index.box_cost;
    eprintln!("page budget: by paths {} by corners {} cost {cost:?}", index.bounds_by_paths, index.bounds_by_corners);
    assert_eq!(index.bounds_by_paths + index.bounds_by_corners, 600, "every turned shape is counted once");
    assert_eq!(index.bounds_by_paths, 327, "131,072 points at 400 rows each: {cost:?}");
    assert!(cost.points <= diacraft_engine::snap::PAGE_BUILD_POINTS && cost.elements <= diacraft_engine::snap::PAGE_BUILD_ELEMENTS, "{cost:?}");
    assert!(cost.curve_work <= diacraft_engine::snap::PAGE_BUILD_WORK && cost.left_out == 273, "{cost:?}");
    // The shapes after the budget are still targets, by their corners: the box is the box round them.
    let late = index.items().iter().rev().find(|i| i.path_box.is_none()).unwrap();
    assert!(late.bbox.width() > 1.0, "a turned 1 by 1 inch shape's corner box is wider than 1: {:?}", late.bbox);
    // A page of the same shapes not turned reads no path box at all (and spends nothing).
    let doc = turned_instances_of_a_big_master(0, 400);
    let page = SnapPage::read(&doc, 1).unwrap();
    assert_eq!((page.shapes(&doc).bounds_by_paths, page.shapes(&doc).box_cost.points), (0, 0));
}

#[test]
fn the_moving_selection_is_paid_for_by_one_budget_and_a_shape_it_cannot_pay_for_keeps_its_corner_box_counted() {
    // 20 turned shapes of 400 rows: the query's budget of 4,096 points pays for ten (400 each), the other ten keep the box round their corners.
    let doc = turned_instances_of_a_big_master(20, 400);
    let ids: Vec<u32> = (100..120).collect();
    let all = bounds_of(&doc, 1, &ids).unwrap();
    assert_eq!(all.by_corners, 10, "{all:?}");
    // One shape alone is paid for, and its box is smaller than the box round its corners.
    let one = bounds_of(&doc, 1, &[100]).unwrap();
    let (alone, none) = (one.rect, one.by_corners);
    let corners = box_of(&doc, 1, &[100]).unwrap();
    assert_eq!(none, 0);
    assert!(
        alone.width() < corners.width() + 1e-9 && alone.height() < corners.height() + 1e-9 && alone.width() < corners.width() - 0.01,
        "{alone:?} {corners:?}"
    );
    // A shape that is not there is no box.
    assert!(bounds_of(&doc, 1, &[999]).is_none());
}

/// The box of the polygon `points` (the shape's own frame, inches) of a shape with `loc` for its pin's place in its frame, its pin at `pin` on the page, turned
/// `degrees`: the level and upright box round the turned points, by plain arithmetic.
fn box_round(points: &[(f64, f64)], loc: (f64, f64), pin: (f64, f64), degrees: f64) -> (f64, f64, f64, f64) {
    let (s, c) = degrees.to_radians().sin_cos();
    let page: Vec<(f64, f64)> = points
        .iter()
        .map(|(x, y)| {
            let (dx, dy) = (x - loc.0, y - loc.1);
            (pin.0 + dx * c - dy * s, pin.1 + dx * s + dy * c)
        })
        .collect();
    let (xs, ys): (Vec<f64>, Vec<f64>) = page.into_iter().unzip();
    let low = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
    let high = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (low(&xs), low(&ys), high(&xs), high(&ys))
}

/// One `Geometry` section of `ix`, a polygon through `points` (the shape's own frame); `no_show` and `no_snap` set the section's cells.
fn with_polygon(mut shape: ShapeBuilder, ix: u32, points: &[(f64, f64)], no_show: bool, no_snap: bool) -> ShapeBuilder {
    for (i, (x, y)) in points.iter().enumerate() {
        let row = RowKey::Ix(u32::try_from(i).unwrap());
        let kind = if i == 0 { "MoveTo" } else { "LineTo" };
        shape = shape.row_type("Geometry", Some(ix), row.clone(), kind).row_cell("Geometry", Some(ix), row.clone(), "X", &x.to_string()).row_cell(
            "Geometry",
            Some(ix),
            row,
            "Y",
            &y.to_string(),
        );
    }
    for (name, on) in [("NoShow", no_show), ("NoSnap", no_snap)] {
        if on {
            shape = shape.set(CellAddr::in_section("Geometry", Some(ix), name), Cell::constant(Value::from_text("1")));
        }
    }
    shape
}

/// A shape 2 by 1 inches (its pin at the middle of its frame) at `pin`, turned `degrees`.
fn frame_2_by_1(id: u32, pin: (f64, f64), degrees: f64) -> ShapeBuilder {
    ShapeBuilder::new(id)
        .cell("PinX", &pin.0.to_string())
        .cell("PinY", &pin.1.to_string())
        .cell("Width", "2")
        .cell("Height", "1")
        .cell("LocPinX", "1")
        .cell("LocPinY", "0.5")
        .cell("Angle", &degrees.to_radians().to_string())
}

/// The ellipse of the 2 by 1 frame as a polygon of `n` vertices (its extremes among them when `n` is a multiple of 4).
fn ellipse_points(n: usize) -> Vec<(f64, f64)> {
    (0..=n)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            (1.0 + t.cos(), 0.5 + 0.5 * t.sin())
        })
        .collect()
}

const DIAMOND: [(f64, f64); 5] = [(1.0, 0.0), (2.0, 0.5), (1.0, 1.0), (0.0, 0.5), (1.0, 0.0)];
const FRAME: [(f64, f64); 5] = [(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (0.0, 1.0), (0.0, 0.0)];

fn page_of_builders(shapes: Vec<ShapeBuilder>) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20");
    for shape in shapes {
        page = page.shape(shape);
    }
    DocBuilder::new().page(1, page).build()
}

fn path_box_of(doc: &Document, id: u32) -> Option<(f64, f64, f64, f64)> {
    let page = SnapPage::read(doc, 1).unwrap();
    let index = page.shapes(doc);
    index.find(id).unwrap().path_box.map(|b| (b.x0, b.y0, b.x1, b.y1))
}

fn same4(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    [(a.0, b.0), (a.1, b.1), (a.2, b.2), (a.3, b.3)].iter().all(|(x, y)| (x - y).abs() < 1e-6)
}

#[test]
fn a_section_that_is_not_shown_is_no_part_of_the_box_and_one_that_cannot_snap_is() {
    let at = (5.0, 5.0);
    let corners = box_round(&FRAME, (1.0, 0.5), at, 30.0);
    assert!((corners.2 - corners.0 - 2.232051).abs() < 1e-6 && (corners.3 - corners.1 - 1.866025).abs() < 1e-6, "{corners:?}");
    let diamond = box_round(&DIAMOND, (1.0, 0.5), at, 30.0);
    assert!(diamond.2 - diamond.0 < corners.2 - corners.0 - 0.4, "the diamond is the smaller box: {diamond:?}");
    // The ellipse's section `NoShow` 1 and a second, shown, section filling the frame: the box round the corners.
    let doc = page_of_builders(vec![with_polygon(with_polygon(frame_2_by_1(1, at, 30.0), 0, &DIAMOND, true, false), 1, &FRAME, false, false)]);
    assert!(same4(path_box_of(&doc, 1).unwrap(), corners), "{:?}", path_box_of(&doc, 1));
    // The other way round, which tells a section left out from one counted: the shown diamond and a hidden frame give the diamond's box, not the corners'.
    let doc = page_of_builders(vec![with_polygon(with_polygon(frame_2_by_1(1, at, 30.0), 0, &DIAMOND, false, false), 1, &FRAME, true, false)]);
    assert!(same4(path_box_of(&doc, 1).unwrap(), diamond), "{:?} against {diamond:?}", path_box_of(&doc, 1));
    // `NoSnap` stops snapping to the path's own points, not the shape's alignment: a section with it counts.
    let doc = page_of_builders(vec![with_polygon(with_polygon(frame_2_by_1(1, at, 30.0), 0, &DIAMOND, false, true), 1, &FRAME, true, false)]);
    assert!(same4(path_box_of(&doc, 1).unwrap(), diamond), "{:?}", path_box_of(&doc, 1));
    // No shown path at all, and no path at all: the box round the corners, and the page counts the shape.
    for shape in [with_polygon(frame_2_by_1(1, at, 30.0), 0, &DIAMOND, true, false), frame_2_by_1(1, at, 30.0)] {
        let doc = page_of_builders(vec![shape]);
        let page = SnapPage::read(&doc, 1).unwrap();
        let index = page.shapes(&doc);
        assert!(index.find(1).unwrap().path_box.is_none());
        assert_eq!((index.bounds_by_paths, index.bounds_by_corners), (0, 1));
    }
}

/// A group 4 by 2 inches (its pin at its middle) at (5, 5) turned 30 degrees, with these members.
fn turned_group(members: Vec<ShapeBuilder>) -> ShapeBuilder {
    let mut g = ShapeBuilder::new(1)
        .group()
        .cell("PinX", "5")
        .cell("PinY", "5")
        .cell("Width", "4")
        .cell("Height", "2")
        .cell("LocPinX", "2")
        .cell("LocPinY", "1")
        .cell("Angle", &30.0_f64.to_radians().to_string());
    for m in members {
        g = g.child(m);
    }
    g
}

/// A member 1 by 0.5 inch with its pin at `pin` in the group's frame, drawing the rectangle of its own frame when `drawn`.
fn member(id: u32, pin: (f64, f64), drawn: bool) -> ShapeBuilder {
    let shape = ShapeBuilder::new(id)
        .cell("PinX", &pin.0.to_string())
        .cell("PinY", &pin.1.to_string())
        .cell("Width", "1")
        .cell("Height", "0.5")
        .cell("LocPinX", "0.5")
        .cell("LocPinY", "0.25");
    if drawn { with_polygon(shape, 0, &[(0.0, 0.0), (1.0, 0.0), (1.0, 0.5), (0.0, 0.5), (0.0, 0.0)], false, false) } else { shape }
}

#[test]
fn a_turned_groups_box_is_round_its_members_paths_and_a_picture_in_it_counts_by_its_corners() {
    // The members' corners in the group's frame: member 2 at (0.5, 0.25) to (1.5, 0.75), member 3 at (2.5, 1.25) to (3.5, 1.75).
    let both = [(0.5, 0.25), (1.5, 0.25), (1.5, 0.75), (0.5, 0.75), (2.5, 1.25), (3.5, 1.25), (3.5, 1.75), (2.5, 1.75)];
    let want = box_round(&both, (2.0, 1.0), (5.0, 5.0), 30.0);
    let doc = page_of_builders(vec![turned_group(vec![member(2, (1.0, 0.5), true), member(3, (3.0, 1.5), true)])]);
    let got = path_box_of(&doc, 1).expect("a turned group has the box round its members' paths");
    assert!(same4(got, want), "{got:?} against {want:?}");
    let whole = box_round(&[(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (0.0, 2.0)], (2.0, 1.0), (5.0, 5.0), 30.0);
    assert!(
        got.2 - got.0 < whole.2 - whole.0 - 0.5 && got.3 - got.1 < whole.3 - whole.1 - 0.5,
        "round the members, not the group's frame: {got:?} {whole:?}"
    );
    // A picture in the group counts by the box round its corners: member 3 a picture of the same frame.
    let picture = member(3, (3.0, 1.5), false).foreign(ForeignData::default());
    let doc = page_of_builders(vec![turned_group(vec![member(2, (1.0, 0.5), true), picture])]);
    assert!(same4(path_box_of(&doc, 1).unwrap(), want), "{:?}", path_box_of(&doc, 1));
    // A group that draws nothing keeps the box round its corners.
    let doc = page_of_builders(vec![turned_group(vec![member(2, (1.0, 0.5), false)])]);
    assert!(path_box_of(&doc, 1).is_none());
}

#[test]
fn a_groups_box_is_made_of_paths_for_at_most_the_cap_of_shapes_with_the_group() {
    let many = |n: u32| turned_group((0..n).map(|i| member(2 + i, (2.0, 1.0), true)).collect());
    let cap = u32::try_from(diacraft_engine::snap::MAX_BOX_MEMBERS).unwrap();
    // The group and 255 members are 256 shapes: the cap. One more member and the group keeps the box round its corners, and the page counts it.
    assert!(path_box_of(&page_of_builders(vec![many(cap - 1)]), 1).is_some());
    let doc = page_of_builders(vec![many(cap)]);
    let page = SnapPage::read(&doc, 1).unwrap();
    assert!(page.shapes(&doc).find(1).unwrap().path_box.is_none());
    assert_eq!((page.shapes(&doc).bounds_by_paths, page.shapes(&doc).bounds_by_corners), (0, 1));
}

#[test]
fn a_target_the_page_cannot_pay_for_lines_up_by_its_corners_counted_and_one_within_the_budget_by_its_paths() {
    // The target is the 2 by 1 ellipse (a polygon of 300 vertices) turned 30 degrees at (205, 205): the box round its path has its top at 205.661438 (the
    // 5.661438), the box round its corners at 205.933013. A is a 1 by 0.2 inch box dragged to put its top 0.02 inch above the corners' top.
    let ellipse = ellipse_points(396);
    let corners_top = box_round(&FRAME, (1.0, 0.5), (205.0, 205.0), 30.0).3;
    let paths_top = box_round(&ellipse, (1.0, 0.5), (205.0, 205.0), 30.0).3;
    assert!((corners_top - 205.933013).abs() < 1e-6 && (paths_top - 205.661438).abs() < 1e-5, "{corners_top} {paths_top}");
    let a = Rect { x0: 208.0, y0: corners_top + 0.02 - 0.2, x1: 209.0, y1: corners_top + 0.02 };
    let target = || with_polygon(frame_2_by_1(5000, (205.0, 205.0), 30.0), 0, &ellipse, false, false);
    // After 400 turned shapes of 400 rows each, which spend the page's budget of 131,072 points (327 of them, 272 points left): the target of 397 rows is left out and is
    // by its corners: A comes down onto them. The page counts it with the 73 big shapes it could not pay for.
    let (master, page) = big_master_page(400, 400);
    let spent = DocBuilder::new().master(1, master).page(1, page.shape(target())).build();
    let read = SnapPage::read(&spent, 1).unwrap();
    assert_eq!(read.shapes(&spent).bounds_by_corners, 73 + 1, "seventy-three of the 400 and the target: {:?}", read.shapes(&spent).box_cost);
    let moved = drag(&spent, &vector_settings(), a, &[]);
    assert!((a.y1 + moved.dy - corners_top).abs() < 1e-9, "A's top is on the top of the corner box: {moved:?}");
    // The same target on a page whose budget pays for it: its paths, and A stays where it was dragged, 0.29 inch above the top of the box round the path.
    let paid = DocBuilder::new().page(1, PageBuilder::new().cell("PageWidth", "400").cell("PageHeight", "400").shape(target())).build();
    let read = SnapPage::read(&paid, 1).unwrap();
    assert_eq!((read.shapes(&paid).bounds_by_paths, read.shapes(&paid).bounds_by_corners), (1, 0));
    let stays = drag(&paid, &vector_settings(), a, &[]);
    assert!(stays.dy == 0.0 && stays.hits.iter().all(|h| h.target != Target::Alignment), "{stays:?}");
}

/// A shape 2 by 1 turned 30 degrees at `pin` whose one path is a polyline of `points` points (from (0, 0), inside the frame).
fn turned_with_a_polyline(id: u32, pin: (f64, f64), points: usize) -> ShapeBuilder {
    let mut text = String::from("POLYLINE(1, 1");
    for i in 0..points {
        text.push_str(&format!(", {:.4}, {:.4}", 2.0 * i as f64 / points as f64, ((i % 7) as f64) / 14.0));
    }
    text.push(')');
    frame_2_by_1(id, pin, 30.0)
        .row_type("Geometry", Some(0), RowKey::Ix(0), "MoveTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(0), "X", "0")
        .row_cell("Geometry", Some(0), RowKey::Ix(0), "Y", "0")
        .row_type("Geometry", Some(0), RowKey::Ix(1), "PolylineTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "2")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "Y", "0")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "A", &text)
}

#[test]
fn a_dragged_shape_past_the_subjects_budget_keeps_its_corner_box_and_the_result_counts_it() {
    // 5,000 points are over the query's budget of 4,096 and well within the page's of 131,072: the same shape is a target by its paths, and a subject by its corners.
    let doc = page_of_builders(vec![turned_with_a_polyline(1, (5.0, 5.0), 5_000)]);
    let paths = path_box_of(&doc, 1).expect("the page pays for it");
    let subject = bounds_of(&doc, 1, &[1]).unwrap();
    let corners = box_of(&doc, 1, &[1]).unwrap();
    assert_eq!(subject.by_corners, 1);
    assert!(
        near4((subject.rect.x0, subject.rect.y0, subject.rect.x1, subject.rect.y1), (corners.x0, corners.y0, corners.x1, corners.y1)),
        "{subject:?}"
    );
    assert!(paths.3 - paths.1 < corners.height() - 0.1, "the box round the path is smaller: {paths:?} {corners:?}");
    // What it cost is counted: its two rows were charged, no text was read and nothing was built; the section was left out.
    assert_eq!((subject.cost.bytes, subject.cost.elements, subject.cost.built, subject.cost.left_out), (0, 0, 0, 1), "{:?}", subject.cost);
    // A shape within the budget is paid for, and counted in the same words: its constructor's bytes and the elements it built.
    let small = page_of_builders(vec![turned_with_a_polyline(1, (5.0, 5.0), 500)]);
    let paid = bounds_of(&small, 1, &[1]).unwrap();
    assert_eq!((paid.by_corners, paid.cost.built, paid.cost.left_out), (0, 1, 0), "{paid:?}");
    assert!(paid.cost.bytes > 5_000 && paid.cost.bytes < 12_000 && paid.cost.elements >= 500 && paid.cost.points >= 500, "{:?}", paid.cost);
    // Through the command: A's top is 0.03 inch above the top of the box round the path and 0.24 under the corner box's, so a subject by its paths would come up to
    // it (and the shape move by 0.03); one by its corners does not. The result says one shape kept its corners.
    let page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20").shape(turned_with_a_polyline(1, (5.0, 5.0), 5_000)).shape(
        ShapeBuilder::new(2)
            .cell("PinX", "8.5")
            .cell("PinY", &(paths.3 + 0.03 - 0.1).to_string())
            .cell("Width", "1")
            .cell("Height", "0.2")
            .cell("LocPinX", "0.5")
            .cell("LocPinY", "0.1"),
    );
    let mut s = session_of(DocBuilder::new().page(1, page).build());
    let r = run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.0, "dy": 0.0, "snap": {"zoom": 96.0}}));
    assert_eq!(r["snap"]["corner_boxes"], json!(1), "{r}");
    assert!(
        r["snap"]["hits"].as_array().is_some_and(|h| h.iter().all(|h| h["target"] != json!("alignment"))),
        "no snap by a box that is not the paths': {r}"
    );
    // One shape within the budget: none counted, and the box round its path is what comes up to A (0.03 inch, less the few thousandths the shorter polyline's end
    // is lower).
    let page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20").shape(turned_with_a_polyline(1, (5.0, 5.0), 500)).shape(
        ShapeBuilder::new(2)
            .cell("PinX", "8.5")
            .cell("PinY", &(paths.3 + 0.03 - 0.1).to_string())
            .cell("Width", "1")
            .cell("Height", "0.2")
            .cell("LocPinX", "0.5")
            .cell("LocPinY", "0.1"),
    );
    let mut s = session_of(DocBuilder::new().page(1, page).build());
    let r = run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.0, "dy": 0.0, "snap": {"zoom": 96.0}}));
    assert_eq!(r["snap"]["corner_boxes"], json!(0), "{r}");
    assert!(
        r["snap"]["hits"].as_array().is_some_and(|h| h.iter().any(|h| h["target"] == json!("alignment"))),
        "a subject by its paths lines up with A: {r}"
    );
}

#[test]
#[ignore = "a measurement, not a test: SNAP_MEASURE_N=<shapes> prints what a plain move and a snapped move cost on a page of that many shapes (the yardstick of the cap); run with --ignored --nocapture"]
fn measure_a_snapped_move_beside_a_plain_one_on_a_page_of_many_shapes() {
    use std::time::Instant;
    let n: u32 = std::env::var("SNAP_MEASURE_N").ok().and_then(|v| v.parse().ok()).unwrap_or(5_000);
    let mut page = PageBuilder::new().cell("PageWidth", "1000").cell("PageHeight", "1000");
    for i in 0..n {
        let (x, y) = (f64::from(i % 300) * 1.5 + 1.0, f64::from(i / 300) * 1.5 + 1.0);
        page = page.shape(
            ShapeBuilder::new(i + 1)
                .cell("PinX", &x.to_string())
                .cell("PinY", &y.to_string())
                .cell("Width", "1")
                .cell("Height", "1")
                .cell("LocPinX", "0.5")
                .cell("LocPinY", "0.5")
                .cell("Angle", "0"),
        );
    }
    let mut s = session_of(DocBuilder::new().page(1, page).build());
    let time = |s: &mut Session, params: serde_json::Value| {
        let started = Instant::now();
        let r = s.execute("shape.move", &params).unwrap();
        (started.elapsed(), r)
    };
    // Plain moves of one shape (the yardstick: what an edit costs on this page), then snapped ones; each call reads its own page.
    let plain: Vec<_> = (0..3).map(|i| time(&mut s, json!({"ids": [1], "dx": 0.01, "dy": 0.0 * f64::from(i)})).0).collect();
    let snapped: Vec<_> = (0..3).map(|_| time(&mut s, json!({"ids": [1], "dx": 0.01, "dy": 0.0, "snap": {"zoom": 96.0}}))).collect();
    let ms = |d: &std::time::Duration| format!("{:.2}", d.as_secs_f64() * 1e3);
    eprintln!(
        "MEASURE n={n} plain_move_ms={:?} snapped_move_ms={:?} truncated={}",
        plain.iter().map(ms).collect::<Vec<_>>(),
        snapped.iter().map(|(d, _)| ms(d)).collect::<Vec<_>>(),
        snapped[0].1["snap"]["truncated"]
    );
}

/// Where the `i`th shape of [`turned_stock_page`] is, and how it is turned (radians): on a grid 1.5 inches apart, 100 to a row, in one of seven turns.
fn turned_stock_place(i: usize) -> (f64, f64, f64) {
    ((i % 100) as f64 * 1.5 + 1.0, (i / 100) as f64 * 1.5 + 1.0, (10.0 + (i % 7) as f64 * 11.0).to_radians())
}

/// `n` stock shapes (the built-in masters in turn), each turned, on one page: what the page budget is measured against. **Built in one pass** (of the
/// re-review): dropping and then turning each shape in a document that holds the others copies the page's list and walks its shapes every time, which is quadratic, and took 72
/// to 94 seconds for 10,000 against the runner's 120. One of each master is dropped, so that the document holds the masters as a drop brings them, and the page is then made
/// of instances by a builder, linear in `n` (a second or two for 10,000). [`turned_stock_page_one_at_a_time`] is the slow way, which a test holds this to.
pub fn turned_stock_page(n: usize) -> Document {
    let masters: Vec<_> = diacraft_engine::stencil::all_masters().filter(|m| !m.is_connector()).collect();
    let mut doc = page_of(&[]);
    let held: Vec<u32> = masters.iter().map(|m| drop_shape(&mut doc, 1, m, 0.0, 0.0).unwrap().master).collect();
    let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20");
    for i in 0..n {
        let (x, y, angle) = turned_stock_place(i);
        page = page.shape(
            ShapeBuilder::new(u32::try_from(i + 1).unwrap())
                .master(held[i % held.len()])
                .cell("PinX", &x.to_string())
                .cell("PinY", &y.to_string())
                .cell("Angle", &angle.to_string()),
        );
    }
    doc.pages = DocBuilder::new().page(1, page).build().pages;
    doc
}

/// [`turned_stock_page`] as the commands make it: each shape dropped and then turned in the document, quadratic in `n`. Small pages only.
fn turned_stock_page_one_at_a_time(n: usize) -> Document {
    let masters: Vec<_> = diacraft_engine::stencil::all_masters().filter(|m| !m.is_connector()).collect();
    let mut doc = page_of(&[]);
    for i in 0..n {
        let (x, y, angle) = turned_stock_place(i);
        let info = drop_shape(&mut doc, 1, masters[i % masters.len()], x, y).unwrap();
        rotate_shapes(&mut doc, 1, &[info.id], Turn::To(angle)).unwrap();
    }
    doc
}

/// The page built in one pass is the page the commands build, as far as a snap reads it: every shape has the same box round its paths, and the page costs the same points,
/// elements and index (so the measurements the R45 tests hold are of the same page).
#[test]
fn the_turned_stock_page_built_in_one_pass_is_the_one_the_commands_make() {
    let n = 130;
    let (fast, slow) = (turned_stock_page(n), turned_stock_page_one_at_a_time(n));
    let (a, b) = (SnapPage::read(&fast, 1).unwrap(), SnapPage::read(&slow, 1).unwrap());
    let (ia, ib) = (a.shapes(&fast), b.shapes(&slow));
    assert_eq!((ia.bounds_by_paths, ia.bounds_by_corners, ia.bounds_cut), (n, 0, 0));
    assert_eq!((ia.bounds_by_paths, ia.bounds_by_corners, ia.bounds_cut), (ib.bounds_by_paths, ib.bounds_by_corners, ib.bounds_cut));
    assert_eq!(ia.box_cost, ib.box_cost);
    for id in 1..=u32::try_from(n).unwrap() {
        let (x, y) = (bounds_of(&fast, 1, &[id]).unwrap().rect, bounds_of(&slow, 1, &[id]).unwrap().rect);
        assert!(
            [(x.x0, y.x0), (x.y0, y.y0), (x.x1, y.x1), (x.y1, y.y1)].iter().all(|(p, q)| (p - q).abs() < 1e-9),
            "shape {id}: the box a snap aligns by is {x:?} built in one pass and {y:?} one at a time"
        );
    }
}

#[test]
fn a_page_of_five_thousand_stock_shapes_turned_is_paid_for_in_full_with_room_to_spare() {
    // The measurement the page budget was first set by (16 times a query's): 5,000 stock shapes, each turned, spend 43,054 points and 60,498 elements and no constructor
    // text and no spline work. 5,000 was the most the index read until a later change; it is no longer the most, and the budget is 32 times a query's (below): the room is
    // three times the points and four times the elements.
    let doc = turned_stock_page(5_000);
    let page = SnapPage::read(&doc, 1).unwrap();
    let index = page.shapes(&doc);
    let c = index.box_cost;
    assert_eq!((index.bounds_by_paths, index.bounds_by_corners), (5_000, 0), "{c:?}");
    assert!((39_000..=47_000).contains(&c.points) && (54_000..=67_000).contains(&c.elements), "{c:?}");
    assert_eq!((c.bytes, c.curve_work, c.left_out), (0, 0, 0), "{c:?}");
    assert!(c.points * 3 / 2 <= diacraft_engine::snap::PAGE_BUILD_POINTS && c.elements * 2 <= diacraft_engine::snap::PAGE_BUILD_ELEMENTS, "{c:?}");
}

#[test]
fn a_page_of_the_most_shapes_the_index_reads_all_turned_stock_shapes_is_paid_for_in_full() {
    // The page budget is 32 times a query's, taken with the index's 10,000 shapes: a page of 10,000 stock shapes, every one turned, spends about 86,100 points and
    // 121,000 elements of the 131,072 and 524,288, and no text or spline work, and every one has the box round its paths (16 times paid for 7,617 of them, and the 2,383 after
    // them kept their corners and said `truncated`).
    let all = diacraft_engine::snap::index::MAX_INDEXED;
    assert_eq!(all, 10_000);
    let doc = turned_stock_page(all);
    let page = SnapPage::read(&doc, 1).unwrap();
    let index = page.shapes(&doc);
    let c = index.box_cost;
    assert_eq!((index.bounds_by_paths, index.bounds_by_corners, index.bounds_cut), (all, 0, 0), "paid in full: {c:?}");
    assert!((80_000..=92_000).contains(&c.points) && (110_000..=135_000).contains(&c.elements), "{c:?}");
    assert_eq!((c.bytes, c.curve_work, c.left_out), (0, 0, 0), "{c:?}");
    assert!(
        c.points * 3 / 2 <= diacraft_engine::snap::PAGE_BUILD_POINTS && c.elements * 4 <= diacraft_engine::snap::PAGE_BUILD_ELEMENTS,
        "room to spare: {c:?}"
    );
    // A snap that lines up by these boxes is not cut: nothing says `truncated` of a page that was paid for.
    let b = bounds_of(&doc, 1, &[1]).unwrap().rect;
    let q = Query { subject: Subject::Box(b), using: Use::Place, moving: &[1], zoom: 96.0, own_points: &[] };
    assert!(!snap_ext(&doc, &page, &SnapSettings::of(&doc), &q, Extras::default()).truncated);
}

#[test]
fn a_page_of_the_most_shapes_the_index_reads_all_turned_with_more_points_than_the_budget_pays_for_keeps_the_last_ones_corners() {
    // 10,000 turned shapes of a master of 20 points spend 200,000, past the budget's 131,072, so the shapes the budget cannot pay for (the last of the page) keep the box round
    // their corners, are counted, and say `truncated`; it is never spent past.
    let doc = turned_instances_of_a_big_master(u32::try_from(diacraft_engine::snap::index::MAX_INDEXED).unwrap(), 20);
    let page = SnapPage::read(&doc, 1).unwrap();
    let index = page.shapes(&doc);
    let c = index.box_cost;
    let all = diacraft_engine::snap::index::MAX_INDEXED;
    assert_eq!(index.bounds_by_paths + index.bounds_by_corners, all, "{c:?}");
    assert!(index.bounds_by_corners > 0 && index.bounds_by_corners * 10 < all * 4, "{} by corners of {all}: {c:?}", index.bounds_by_corners);
    assert!(
        c.points <= diacraft_engine::snap::PAGE_BUILD_POINTS && c.elements <= diacraft_engine::snap::PAGE_BUILD_ELEMENTS,
        "never past the budget: {c:?}"
    );
    assert!(c.left_out >= index.bounds_by_corners, "{c:?}");
}

#[test]
fn the_windows_a_call_reads_are_the_librarys_bound_for_a_query_times_the_queries_a_call_makes() {
    // `MAX_GATHERS` bounds every call's work in the tests that count it (`visited <= MAX_GATHERS * MAX_VISITS`): pin the number, so that a bound set too wide, or too
    // narrow for the library's, is seen.
    assert_eq!(diacraft_engine::snap::MAX_LIBRARY_QUERIES, 3);
    assert_eq!(diacraft_engine::snap::MAX_GATHERS, craft_snap::MAX_GATHERS as usize * diacraft_engine::snap::MAX_LIBRARY_QUERIES);
}

#[test]
fn a_move_locked_to_y_snaps_along_x_and_goes_not_at_all_in_y_and_a_pull_across_it_is_ignored() {
    // A level guide at y = 4.03: the move (0.3, 1.0) locked to y lands the top on it and goes not at all in x. An upright guide at x = 2.03 and the move (0.01, 0.5) locked
    // to y: the shape does not go right, and the guide is not a hit.
    let mut doc = page_of(&[(1, 2.0, 2.0, 3.0, 3.0)]);
    add_guide(&mut doc, 1, Axis::Y, 4.03, 0).unwrap();
    let mut s = session_of(doc);
    let r = run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.3, "dy": 1.0, "snap": {"lock": "y"}}));
    let landed = box_in(&s, 1);
    assert!((landed.y0 - 3.03).abs() < 1e-9 && (landed.y1 - 4.03).abs() < 1e-9, "the top is on the guide: {landed:?}");
    assert!(landed.x0 == 2.0 && landed.x1 == 3.0, "the locked axis did not move, to the bit: {landed:?}");
    assert!(r["snap"]["hits"].as_array().unwrap().iter().all(|h| h["axis"] == json!("y")), "{r}");
    let mut doc = page_of(&[(1, 2.0, 2.0, 3.0, 3.0)]);
    add_guide(&mut doc, 1, Axis::X, 2.03, 0).unwrap();
    let mut s = session_of(doc);
    let r = run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.01, "dy": 0.5, "snap": {"lock": "y"}}));
    let landed = box_in(&s, 1);
    assert!((landed.y0 - 2.5).abs() < 1e-9 && landed.x0 == 2.0 && landed.x1 == 3.0, "{landed:?}");
    assert_eq!(r["snap"]["hits"], json!([]), "{r}");
}

#[test]
fn a_radius_below_0_and_a_zoom_that_is_not_above_0_are_refused_in_words() {
    let mut s = session_of(page_of(&[(1, 2.0, 2.0, 3.0, 3.0)]));
    for (snap, word) in [
        (json!({"radius": -1.0}), "snap.radius"),
        (json!({"radius": 1.0, "zoom": 0.0}), "snap.zoom"),
        (json!({"radius": 1.0, "zoom": -2.0}), "snap.zoom"),
    ] {
        let err = s.execute("shape.rotate", &json!({"ids": [1], "angle": 10.0, "snap": snap})).unwrap_err();
        assert!(err.to_string().contains(word), "{err}");
    }
    // A radius of 0 is a number: the pointer on the pin, which matches nothing and steps by the coarsest step.
    assert!(s.execute("shape.rotate", &json!({"ids": [1], "angle": 10.0, "snap": {"radius": 0.0}})).is_ok());
}

#[test]
fn a_press_without_a_tick_goes_by_the_ruler_of_its_own_axis() {
    // The ruler's density differs between the axes of this page (`XRulerDensity` 8, `YRulerDensity` 32), so its ticks do: a press along x and one along y, each with no
    // `tick` and nothing ahead, go by the tick of their own axis.
    let page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20").cell("XRulerDensity", "8").cell("YRulerDensity", "32").shape(
        ShapeBuilder::new(1)
            .cell("PinX", "5.5")
            .cell("PinY", "5.5")
            .cell("Width", "1")
            .cell("Height", "1")
            .cell("LocPinX", "0.5")
            .cell("LocPinY", "0.5")
            .cell("Angle", "0"),
    );
    let doc = DocBuilder::new().page(1, page).build();
    let grid = SnapPage::read(&doc, 1).unwrap().grid;
    let (tx, ty) = (grid.ruler(Axis::X, 1.0, 96.0).map(|m| m.pitch), grid.ruler(Axis::Y, 1.0, 96.0).map(|m| m.pitch));
    let (tx, ty) = (tx.expect("a ruler along x"), ty.expect("a ruler along y"));
    assert!((tx - ty).abs() > 1e-9, "the page's two rulers differ: {tx} {ty}");
    let mut s = session_of(doc);
    run(&mut s, "snap.set", json!({"dynamic_grid": false, "alignment_box": false, "ruler": false, "grid": false}));
    press(&mut s, 1.0, 0.0, None);
    let after_x = box_in(&s, 1);
    assert!((after_x.x0 - (5.0 + tx)).abs() < 1e-9 && (after_x.y0 - 5.0).abs() < 1e-9, "{after_x:?} against {tx}");
    press(&mut s, 0.0, 1.0, None);
    let after_y = box_in(&s, 1);
    assert!((after_y.y0 - (5.0 + ty)).abs() < 1e-9, "{after_y:?} against {ty}");
}

#[test]
fn a_shape_with_no_height_offers_its_corners_and_side_middles_as_handles_and_no_other_point() {
    // A shape 2 inches wide with no height at y = 3, from x = 2 to 4: its frame is no rectangle with a size, so its corners and side middles are given as points: (2, 3),
    // (3, 3) and (4, 3). A connector's end near one is glued to it as a handle; near where a wrong side middle would be, it is not.
    let doc = page_of(&[(1, 2.0, 3.0, 4.0, 3.0)]);
    let mut s = SnapSettings::none();
    s.snap = true;
    s.glue = true;
    s.glue_handles = true;
    let page = SnapPage::read(&doc, 1).unwrap();
    let at = |p: (f64, f64)| {
        let out = snap(&doc, &page, &s, &Query { subject: Subject::Point(p), using: Use::Connect, moving: &[], zoom: 96.0, own_points: &[] });
        (out.apply(p), out.glue)
    };
    for (p, want) in [((2.03, 3.02), (2.0, 3.0)), ((3.03, 2.98), (3.0, 3.0)), ((3.97, 3.03), (4.0, 3.0))] {
        let (got, glue) = at(p);
        assert!(
            (got.0 - want.0).abs() < 1e-9 && (got.1 - want.1).abs() < 1e-9 && matches!(glue, Some(Glue::Handle { shape: 1 })),
            "{p:?}: {got:?} {glue:?}"
        );
    }
    for p in [(-1.02, 3.0), (6.0, 3.0), (3.0, 0.02), (12.0, 3.0), (1.0, 3.0)] {
        let (got, glue) = at(p);
        assert!(glue.is_none() && got == p, "{p:?}: {got:?} {glue:?}");
    }
}

#[test]
fn a_page_with_no_size_has_no_margins_to_snap_to() {
    // PageWidth and PageHeight 0: the page has no size, so no margin lines. A box 0.02 inch from where the left margin would be (0.25 inch) goes where it was put.
    let page = PageBuilder::new().cell("PageWidth", "0").cell("PageHeight", "0");
    let doc = DocBuilder::new().page(1, page).build();
    let r = Rect { x0: 0.27, y0: 5.0, x1: 1.27, y1: 6.0 };
    let out = drag(&doc, &vector_settings(), r, &[]);
    assert!(out.dx == 0.0 && out.hits.is_empty(), "{out:?}");
    // A page that has a size has them: the same box lands on the margin.
    let doc = DocBuilder::new().page(1, PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20")).build();
    let out = drag(&doc, &vector_settings(), r, &[]);
    assert!((out.dx + 0.02).abs() < 1e-9 && out.hits.iter().any(|h| h.target == Target::PageMargin), "{out:?}");
}

#[test]
fn a_shape_with_no_line_is_left_out_of_size_match_and_one_with_a_line_is_not() {
    // B (id 2) resized by its right side to 1.97 wide, A (id 1) 2 inches wide: with A's line B stops at A's width (right edge 8), without it B goes where it was put.
    let shapes = [(1, 3.4, 2.0, 5.4, 3.5), (2, 6.0, 2.0, 7.5, 3.0)];
    for (patterns, want) in [(vec![], 8.0), (vec![(1u32, "0")], 7.97)] {
        let mut s = session_of(page_with_patterns(&shapes, &patterns));
        run(
            &mut s,
            "shape.resize",
            json!({"id": 2, "x": 6.0, "y": 2.0, "width": 1.97, "height": 1.0, "snap": {"zoom": 96.0, "view": [0.0, 0.0, 10.0, 10.0]}}),
        );
        assert!((box_in(&s, 2).x1 - want).abs() < 1e-9, "{patterns:?}: {:?}", box_in(&s, 2));
    }
}

/// A shape 1 by 1 inch at (5, 5) with `rows` connection rows: the first at `first` in the shape's frame, the others in a row along its middle.
fn shape_with_connection_rows(id: u32, rows: u32, first: (f64, f64)) -> ShapeBuilder {
    let mut s = ShapeBuilder::new(id)
        .cell("PinX", "5")
        .cell("PinY", "5")
        .cell("Width", "1")
        .cell("Height", "1")
        .cell("LocPinX", "0.5")
        .cell("LocPinY", "0.5")
        .cell("Angle", "0");
    for i in 0..rows {
        let (x, y) = if i == 0 { first } else { (0.25 + f64::from(i) * 1.0e-3, 0.5) };
        s = s.row_cell("Connection", None, RowKey::Ix(i), "X", &x.to_string()).row_cell("Connection", None, RowKey::Ix(i), "Y", &y.to_string());
    }
    s
}

/// Where a connector's end near `p` lands, and what it glued to, on `doc` (the Glue switch and glue to connection points on).
fn connector_end_near(doc: &Document, p: (f64, f64)) -> ((f64, f64), Option<Glue>, bool) {
    let mut s = SnapSettings::none();
    s.snap = true;
    s.glue = true;
    s.glue_connection_points = true;
    let page = SnapPage::read(doc, 1).unwrap();
    let out = snap(doc, &page, &s, &Query { subject: Subject::Point(p), using: Use::Connect, moving: &[], zoom: 96.0, own_points: &[] });
    (out.apply(p), out.glue, out.truncated)
}

#[test]
fn a_connection_point_on_a_scaled_page_is_a_target_in_paper_inches() {
    // DrawingScale 2 over PageScale 1: a shape 2 by 2 drawing units at pin (10, 10) with a point at its right middle, (2, 1) in its frame: (11, 10) in the drawing, (5.5, 5)
    // on paper.
    let page = PageBuilder::new().cell("PageWidth", "40").cell("PageHeight", "40").cell("PageScale", "1").cell("DrawingScale", "2").shape(
        ShapeBuilder::new(1)
            .cell("PinX", "10")
            .cell("PinY", "10")
            .cell("Width", "2")
            .cell("Height", "2")
            .cell("LocPinX", "1")
            .cell("LocPinY", "1")
            .cell("Angle", "0")
            .row_cell("Connection", None, RowKey::Ix(0), "X", "2")
            .row_cell("Connection", None, RowKey::Ix(0), "Y", "1"),
    );
    let doc = DocBuilder::new().page(1, page).build();
    let (got, glue, _) = connector_end_near(&doc, (5.53, 5.02));
    assert!(
        (got.0 - 5.5).abs() < 1e-9 && (got.1 - 5.0).abs() < 1e-9 && matches!(glue, Some(Glue::ConnectionPoint { shape: 1, point: 0 })),
        "{got:?} {glue:?}"
    );
}

#[test]
fn a_shape_is_read_for_its_connection_points_up_to_the_cap_of_rows_and_a_call_up_to_the_cap_of_its_own() {
    use diacraft_engine::snap::{MAX_CONNECTION_ROWS, MAX_CONNECTION_ROWS_PER_QUERY};
    // The cap of a shape: exactly the most rows is read (a point of its first row is pulled to), one more is not, and the query says so.
    let rows = u32::try_from(MAX_CONNECTION_ROWS).unwrap();
    for (n, read) in [(rows, true), (rows + 1, false)] {
        let doc = DocBuilder::new()
            .page(1, PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20").shape(shape_with_connection_rows(1, n, (0.9, 0.1))))
            .build();
        let (got, glue, truncated) = connector_end_near(&doc, (5.41, 4.62));
        assert_eq!(glue.is_some(), read, "{n} rows: {got:?} {glue:?}");
        assert_eq!(truncated, !read, "{n} rows");
    }
    // The cap of a call: five shapes of the most rows each, stacked, each with a point of its own in its first row; the call reads the rows of four (1,024 in all) and no more.
    let per_call = u32::try_from(MAX_CONNECTION_ROWS_PER_QUERY / MAX_CONNECTION_ROWS).unwrap();
    assert_eq!(per_call, 4);
    let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20");
    for id in 1..=5u32 {
        page = page.shape(shape_with_connection_rows(id, rows, (0.18 * f64::from(id), 0.1)));
    }
    let doc = DocBuilder::new().page(1, page).build();
    let mut pulled = 0;
    for id in 1..=5u32 {
        // The point of shape `id` is at (4.5 + 0.18 id, 4.6) on the page, farther from each other than a pull reaches.
        let (got, glue, truncated) = connector_end_near(&doc, (4.5 + 0.18 * f64::from(id) + 0.02, 4.6 + 0.01));
        if glue.is_some() {
            pulled += 1;
            assert!((got.0 - (4.5 + 0.18 * f64::from(id))).abs() < 1e-9 && (got.1 - 4.6).abs() < 1e-9, "{id}: {got:?}");
        }
        assert!(truncated || glue.is_some(), "a shape that is not read is said: {id}");
    }
    assert_eq!(pulled, 4, "four of the five shapes are read");
}

#[test]
fn a_grid_with_lines_on_one_axis_only_still_snaps_along_that_axis() {
    // A fixed grid of 1 inch along x and none along y (a fixed grid with no spacing has no lines).
    let page = PageBuilder::new()
        .cell("PageWidth", "20")
        .cell("PageHeight", "20")
        .cell("XGridDensity", "0")
        .cell("XGridSpacing", "1")
        .cell("YGridDensity", "0")
        .cell("YGridSpacing", "0");
    let doc = DocBuilder::new().page(1, page).build();
    let grid = SnapPage::read(&doc, 1).unwrap().grid;
    assert!(grid.grid(Axis::X, 1.0, 96.0).is_some() && grid.grid(Axis::Y, 1.0, 96.0).is_none());
    let mut s = SnapSettings::none();
    s.snap = true;
    s.grid = true;
    let r = Rect { x0: 3.03, y0: 4.07, x1: 4.03, y1: 5.07 };
    let out = drag(&doc, &s, r, &[]);
    assert!((out.dx + 0.03).abs() < 1e-9 && out.dy == 0.0, "{out:?}");
}

#[test]
fn a_subject_with_one_number_that_is_not_finite_snaps_to_nothing_and_so_does_one_whose_own_points_are_not_finite_to_what_it_would() {
    // An upright guide at x = 3.03 and the box 2 to 3 wide: its right edge lands on it. With any one number of the box not finite, nothing is asked and nothing is given.
    let mut doc = page_of(&[(1, 2.0, 2.0, 3.0, 3.0)]);
    add_guide(&mut doc, 1, Axis::X, 3.03, 0).unwrap();
    let page = SnapPage::read(&doc, 1).unwrap();
    let settings = vector_settings();
    let ask = |r: Rect, own: &[(f64, f64)]| {
        snap(&doc, &page, &settings, &Query { subject: Subject::Box(r), using: Use::Place, moving: &[], zoom: 96.0, own_points: own })
    };
    let ok = Rect { x0: 2.03, y0: 2.0, x1: 3.03, y1: 3.0 };
    let base = ask(ok, &[]);
    assert!(base.hits.iter().any(|h| h.target == Target::Guide), "{base:?}");
    for bad in [Rect { x0: f64::NAN, ..ok }, Rect { y0: f64::NAN, ..ok }, Rect { x1: f64::INFINITY, ..ok }, Rect { y1: f64::NEG_INFINITY, ..ok }] {
        assert_eq!(ask(bad, &[]), Snapped::default(), "{bad:?}");
    }
    // Own points that are not finite are left out of the query, which is then the one without them.
    assert_eq!(ask(ok, &[(f64::NAN, 1.0), (1.0, f64::INFINITY), (f64::NAN, f64::NAN)]), base);
}

#[test]
fn snap_resize_is_what_a_window_calls_and_gives_the_snap_and_the_box_it_comes_to() {
    // An upright guide at x = 4.03; the shape from (2, 2) to (4, 3) resized by its right side to x = 4.05: the right side lands on the guide.
    let mut doc = page_of(&[(1, 2.0, 2.0, 4.0, 3.0)]);
    add_guide(&mut doc, 1, Axis::X, 4.03, 0).unwrap();
    let (snapped, (x, y, w, h)) = snap_resize(&doc, 1, None, 1, (2.0, 2.0, 2.05, 1.0), 96.0).expect("a resize that moves an edge");
    assert!((x - 2.0).abs() < 1e-9 && (y - 2.0).abs() < 1e-9 && (x + w - 4.03).abs() < 1e-9 && (h - 1.0).abs() < 1e-9, "{x} {y} {w} {h}");
    assert!(snapped.hits.iter().any(|h| h.target == Target::Guide), "{snapped:?}");
    // No edge moves: nothing to snap.
    assert!(snap_resize(&doc, 1, None, 1, (2.0, 2.0, 2.0, 1.0), 96.0).is_none());
}

#[test]
fn a_kept_resize_keeps_the_shapes_own_proportions_whatever_they_are() {
    // 4 by 2 inches (proportions 2 to 1): the top right corner dragged to (6.5, 4.05) with `keep` comes back 2 to 1, and not as the product of the sides.
    let doc = page_of(&[(1, 2.0, 2.0, 6.0, 4.0)]);
    let extras = Extras { keep: true, ..Extras::default() };
    let (_, (x, y, w, h)) = snap_resize_ext(&doc, 1, None, 1, (2.0, 2.0, 4.5, 2.05), 96.0, extras).expect("a kept resize has a result");
    assert!((x - 2.0).abs() < 1e-9 && (y - 2.0).abs() < 1e-9 && w > 0.0 && (w / h - 2.0).abs() < 1e-9, "{x} {y} {w} {h}");
}

#[test]
fn the_box_of_the_moving_shapes_on_a_page_drawn_at_two_to_one_is_in_paper_inches_and_so_is_a_pictures_in_a_turned_group() {
    // DrawingScale 2 over PageScale 1. An unturned shape of 4 by 2 drawing units at (10, 10) is 2 by 1 paper inches at (5, 5): bounds_of and box_of say so.
    let scaled = |shapes: Vec<ShapeBuilder>| {
        let mut page = PageBuilder::new().cell("PageWidth", "40").cell("PageHeight", "40").cell("PageScale", "1").cell("DrawingScale", "2");
        for shape in shapes {
            page = page.shape(shape);
        }
        DocBuilder::new().page(1, page).build()
    };
    let flat = ShapeBuilder::new(1)
        .cell("PinX", "10")
        .cell("PinY", "10")
        .cell("Width", "4")
        .cell("Height", "2")
        .cell("LocPinX", "2")
        .cell("LocPinY", "1")
        .cell("Angle", "0");
    let doc = scaled(vec![flat]);
    let frame = box_of(&doc, 1, &[1]).unwrap();
    assert!(near4((frame.x0, frame.y0, frame.x1, frame.y1), (4.0, 4.5, 6.0, 5.5)), "{frame:?}");
    assert_eq!(bounds_of(&doc, 1, &[1]).unwrap().rect, frame);
    // A turned group of 8 by 4 drawing units at (10, 10) with a picture of 2 by 1 at (4, 2) in its frame, whose box is what the group's is made of: in paper inches.
    let picture = ShapeBuilder::new(2)
        .foreign(ForeignData::default())
        .cell("PinX", "4")
        .cell("PinY", "2")
        .cell("Width", "2")
        .cell("Height", "1")
        .cell("LocPinX", "1")
        .cell("LocPinY", "0.5");
    let group = ShapeBuilder::new(1)
        .group()
        .cell("PinX", "10")
        .cell("PinY", "10")
        .cell("Width", "8")
        .cell("Height", "4")
        .cell("LocPinX", "4")
        .cell("LocPinY", "2")
        .cell("Angle", &30.0_f64.to_radians().to_string())
        .child(picture);
    let doc = scaled(vec![group]);
    let (x0, y0, x1, y1) = box_round(&[(3.0, 1.5), (5.0, 1.5), (5.0, 2.5), (3.0, 2.5)], (4.0, 2.0), (10.0, 10.0), 30.0);
    let want = (x0 / 2.0, y0 / 2.0, x1 / 2.0, y1 / 2.0);
    let got = path_box_of(&doc, 1).expect("the picture is the group's box");
    assert!(same4(got, want), "{got:?} against {want:?}");
}

#[test]
fn a_turn_asked_at_a_zoom_that_is_no_zoom_is_asked_at_the_default_and_a_page_of_more_shapes_than_are_read_says_so() {
    let doc = turn_page(33.0);
    let page = SnapPage::read(&doc, 1).unwrap();
    let base = TurnOf { pivot: (3.0, 5.0), angle: 35.0_f64.to_radians(), radius: 150.0 / DEFAULT_ZOOM, zoom: DEFAULT_ZOOM };
    let want = snap_turn(&doc, &page, &vector_settings(), &[1], base);
    assert!(!want.truncated && want.turned.is_some(), "{want:?}");
    for zoom in [0.0, -5.0, f64::NAN, f64::INFINITY] {
        let got = snap_turn(&doc, &page, &vector_settings(), &[1], TurnOf { zoom, ..base });
        assert_eq!(got, want, "zoom {zoom}");
    }
    // More shapes than the index reads: matching another shape's angle needs the index, and the result says it left shapes out; the step alone does not and does not.
    let mut crowded = PageBuilder::new().cell("PageWidth", "1000").cell("PageHeight", "1000");
    // One more than the index reads (10,000; it was 5,000).
    for i in 0..(diacraft_engine::snap::index::MAX_INDEXED as u32 + 1) {
        crowded = crowded.shape(
            ShapeBuilder::new(i + 1)
                .cell("PinX", &(f64::from(i % 300) * 1.5 + 1.0).to_string())
                .cell("PinY", &(f64::from(i / 300) * 1.5 + 1.0).to_string())
                .cell("Width", "1")
                .cell("Height", "1")
                .cell("LocPinX", "0.5")
                .cell("LocPinY", "0.5")
                .cell("Angle", "0"),
        );
    }
    let big = DocBuilder::new().page(1, crowded).build();
    let page = SnapPage::read(&big, 1).unwrap();
    let on = snap_turn(&big, &page, &vector_settings(), &[1], base);
    assert!(on.truncated, "{on:?}");
    let mut off = vector_settings();
    off.snap = false;
    assert!(!snap_turn(&big, &page, &off, &[1], base).truncated);
}

#[test]
fn a_view_with_a_number_that_is_not_finite_is_no_view_and_every_shape_is_matched() {
    // B (id 2) resized by its right side to 1.97 wide, A (id 1) 2 inches wide: with no view (and with one that is no view) B stops at A's width, its right edge at 8.
    let doc = page_of(&[(1, 3.4, 2.0, 5.4, 3.5), (2, 6.0, 2.0, 7.5, 3.0)]);
    for view in [None, Some([f64::NAN, 0.0, 10.0, 10.0]), Some([0.0, f64::INFINITY, 10.0, 10.0])] {
        let extras = Extras { view, ..Extras::default() };
        let (_, (x, _, w, _)) = snap_resize_ext(&doc, 1, None, 2, (6.0, 2.0, 1.97, 1.0), 96.0, extras).expect("a resize that moves an edge");
        assert!((x + w - 8.0).abs() < 1e-9, "{view:?}: the right edge is at {}", x + w);
    }
}

#[test]
fn a_line_hit_on_each_axis_runs_over_its_own_shapes_and_not_the_others() {
    // A is a 1 by 1 box at (1, 1) and B a 1 by 1 box at (6, 5), top at y = 6. The box dragged from (1.03, 4.97) to (2.03, 5.97) lines its left side up with A's (x = 1) and
    // its top with B's (y = 6).
    let doc = page_of(&[(1, 1.0, 1.0, 2.0, 2.0), (2, 6.0, 5.0, 7.0, 6.0)]);
    let r = Rect { x0: 1.03, y0: 4.97, x1: 2.03, y1: 5.97 };
    let out = drag(&doc, &vector_settings(), r, &[]);
    let hit_on = |axis: Axis| out.hits.iter().find(|h| h.target == Target::Alignment && h.axis == Some(axis)).copied();
    let (x_hit, y_hit) = (hit_on(Axis::X).expect("the left sides line up"), hit_on(Axis::Y).expect("the tops line up"));
    assert_eq!((x_hit.shape, y_hit.shape), (Some(1), Some(2)), "{out:?}");
    let (xs, ys) = (x_hit.span.unwrap(), y_hit.span.unwrap());
    // The line on x = 1 runs along y from A (its bottom, y = 1) to the box (its top, 6 once landed); the line on y = 6 runs along x from the box's left (1) to B's right (7).
    assert!((xs.0 - 1.0).abs() < 1e-9 && (xs.1 - 6.0).abs() < 1e-9, "{xs:?}");
    assert!((ys.0 - 1.0).abs() < 1e-9 && (ys.1 - 7.0).abs() < 1e-9, "{ys:?}");
    let marks: Vec<_> = out
        .marks
        .iter()
        .filter_map(|m| match m {
            SnapMark::Line { axis, from, to, hit: true, .. } => Some((*axis, *from, *to)),
            _ => None,
        })
        .collect();
    assert!(marks.len() >= 2, "{marks:?}");
}

#[test]
fn a_size_match_says_which_side_of_which_axis_it_moved() {
    // A (id 1) is 2 inches wide and 1.5 high, B (id 2) 1.5 wide and 1 high at (6, 2) to (7.5, 3). Each resize moves one side of B a hair short of A's width or height.
    let doc = page_of(&[(1, 3.4, 0.5, 5.4, 2.0), (2, 6.0, 2.0, 7.5, 3.0)]);
    for (to, axis, anchor, edge) in [
        ((6.0, 2.0, 1.97, 1.0), "x", "high", (8.0, 1.0)), // the right side, to x = 8
        ((5.53, 2.0, 1.97, 1.0), "x", "low", (5.5, 0.0)), // the left side, to x = 5.5
        ((6.0, 2.0, 1.5, 1.47), "y", "high", (3.5, 2.0)), // the top, to y = 3.5
        ((6.0, 1.53, 1.5, 1.47), "y", "low", (1.5, 0.0)), // the bottom, to y = 1.5
    ] {
        let mut s = session_of(doc.clone());
        let r = run(
            &mut s,
            "shape.resize",
            json!({"id": 2, "x": to.0, "y": to.1, "width": to.2, "height": to.3, "snap": {"zoom": 96.0, "view": [0.0, 0.0, 10.0, 10.0]}}),
        );
        let b = box_in(&s, 2);
        let landed = match (axis, anchor) {
            ("x", "high") => b.x1,
            ("x", _) => b.x0,
            (_, "high") => b.y1,
            _ => b.y0,
        };
        assert!((landed - edge.0).abs() < 1e-9, "{axis} {anchor}: {b:?} {r}");
        let hit =
            r["snap"]["hits"].as_array().unwrap().iter().find(|h| h["target"] == json!("size")).unwrap_or_else(|| panic!("a size hit: {r}")).clone();
        assert_eq!((hit["axis"].clone(), hit["anchor"].clone()), (json!(axis), json!(anchor)), "{r}");
    }
}

#[test]
fn the_page_budget_is_thirty_two_times_a_querys_in_each_of_its_four_units() {
    use diacraft_engine::snap::{PAGE_BUILD_BYTES, PAGE_BUILD_ELEMENTS, PAGE_BUILD_POINTS, PAGE_BUILD_WORK};
    assert_eq!((PAGE_BUILD_BYTES, PAGE_BUILD_POINTS, PAGE_BUILD_ELEMENTS, PAGE_BUILD_WORK), (4 * 1024 * 1024, 131_072, 524_288, 160_000_000));
}

#[test]
fn a_resize_that_keeps_proportions_tells_the_box_it_landed_in_and_no_other_snap_does() {
    // `landed` is told for a resize with `keep` only: not for a move that is sent `keep`, and not for a resize that is not.
    let doc = page_of(&[(1, 2.0, 2.0, 6.0, 4.0)]);
    let mut s = session_of(doc.clone());
    let kept = run(
        &mut s,
        "shape.resize",
        json!({"id": 1, "x": 2.0, "y": 2.0, "width": 4.5, "height": 2.05, "snap": {"zoom": 96.0, "keep": "proportions"}}),
    );
    assert!(kept["snap"]["landed"].is_array(), "{kept}");
    let mut s = session_of(doc.clone());
    let free = run(&mut s, "shape.resize", json!({"id": 1, "x": 2.0, "y": 2.0, "width": 4.5, "height": 2.05, "snap": {"zoom": 96.0}}));
    assert!(free["snap"]["landed"].is_null(), "{free}");
    // (a move is not sent `keep`, which is a resize's word: it is refused in words, and a move tells no `landed`.)
    let mut s = session_of(doc);
    let err = s.execute("shape.move", &json!({"ids": [1], "dx": 0.5, "dy": 0.5, "snap": {"zoom": 96.0, "keep": "proportions"}})).unwrap_err();
    assert!(err.to_string().contains("snap.keep") && err.to_string().contains("shape.move"), "{err}");
    let moved = run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.5, "dy": 0.5, "snap": {"zoom": 96.0}}));
    assert!(moved["snap"]["landed"].is_null(), "{moved}");
}

#[test]
fn a_group_does_not_take_the_paths_of_a_deleted_member_or_of_a_guide_in_it() {
    let both = [(0.5, 0.25), (1.5, 0.25), (1.5, 0.75), (0.5, 0.75)];
    let want = box_round(&both, (2.0, 1.0), (5.0, 5.0), 30.0);
    // Member 3 is far from member 2 in the group's frame, drawn, and deleted: its paths are not part of the box.
    let doc = page_of_builders(vec![turned_group(vec![member(2, (1.0, 0.5), true), member(3, (3.0, 1.5), true).deleted()])]);
    assert!(same4(path_box_of(&doc, 1).unwrap(), want), "{:?} against {want:?}", path_box_of(&doc, 1));
    // The same member made a guide (a member that is a guide draws nothing that a shape lines up by).
    let doc = page_of_builders(vec![turned_group(vec![member(2, (1.0, 0.5), true), member(3, (3.0, 1.5), true).kind(ShapeType::Guide)])]);
    assert!(same4(path_box_of(&doc, 1).unwrap(), want), "{:?} against {want:?}", path_box_of(&doc, 1));
    // Both there and shown, the box takes them both: the two tests above are of what is left out.
    let doc = page_of_builders(vec![turned_group(vec![member(2, (1.0, 0.5), true), member(3, (3.0, 1.5), true)])]);
    assert!(path_box_of(&doc, 1).unwrap().2 - path_box_of(&doc, 1).unwrap().0 > want.2 - want.0 + 1.0);
}

#[test]
fn a_group_nested_deeper_than_the_document_allows_is_not_walked_to_the_bottom() {
    // 205 groups one in another, a drawn rectangle at the bottom: the document holds 200 levels (`MAX_SHAPE_DEPTH`), the walk stops there, and the shape keeps the box
    // round its corners.
    let levels = diacraft_doc::MAX_SHAPE_DEPTH + 5;
    let mut inner = member(1000, (2.0, 1.0), true);
    for d in 0..levels {
        let id = u32::try_from(2 + d).unwrap();
        inner = ShapeBuilder::new(id)
            .group()
            .cell("PinX", "2")
            .cell("PinY", "1")
            .cell("Width", "4")
            .cell("Height", "2")
            .cell("LocPinX", "2")
            .cell("LocPinY", "1")
            .child(inner);
    }
    let doc = page_of_builders(vec![turned_group(vec![inner])]);
    let page = SnapPage::read(&doc, 1).unwrap();
    assert!(page.shapes(&doc).find(1).unwrap().path_box.is_none());
    assert_eq!((page.shapes(&doc).bounds_by_paths, page.shapes(&doc).bounds_by_corners), (0, 1));
}

#[test]
fn a_shape_with_a_section_the_box_cannot_be_made_of_keeps_the_box_round_its_corners_and_not_a_part_of_its_paths() {
    // Section 0 is the diamond (paid for), section 1 has 401 rows, over the 400 a section may have: it is left out, and a box round the diamond alone would be wrong.
    let far: Vec<(f64, f64)> = (0..401).map(|i| (f64::from(i % 2) * 2.0, f64::from(i % 3) / 2.0)).collect();
    let doc = page_of_builders(vec![with_polygon(with_polygon(frame_2_by_1(1, (5.0, 5.0), 30.0), 0, &DIAMOND, false, false), 1, &far, false, false)]);
    let page = SnapPage::read(&doc, 1).unwrap();
    assert!(page.shapes(&doc).find(1).unwrap().path_box.is_none(), "{:?}", page.shapes(&doc).find(1).unwrap().path_box);
    assert_eq!((page.shapes(&doc).bounds_by_paths, page.shapes(&doc).bounds_by_corners), (0, 1));
    // With 400 rows it is read, and the box is made of both sections.
    let far: Vec<(f64, f64)> = (0..400).map(|i| (f64::from(i % 2) * 2.0, f64::from(i % 3) / 2.0)).collect();
    let doc = page_of_builders(vec![with_polygon(with_polygon(frame_2_by_1(1, (5.0, 5.0), 30.0), 0, &DIAMOND, false, false), 1, &far, false, false)]);
    assert!(path_box_of(&doc, 1).is_some());
}

#[test]
fn a_shape_is_turned_by_more_than_a_tolerance_of_a_quarter_turn_and_not_by_less() {
    let degrees_of = |radians: f64| radians.to_degrees();
    let quarter = std::f64::consts::FRAC_PI_2;
    for (what, angle, turned) in [
        ("nothing", 0.0, false),
        ("a quarter turn and a hair less", quarter - 1.0e-10, false),
        ("a hair over nothing", 1.0e-10, false),
        ("a half turn", 2.0 * quarter, false),
        ("three quarters", 3.0 * quarter, false),
        ("a whole turn", 4.0 * quarter, false),
        ("a hair over the tolerance", 1.0e-7, true),
        ("a quarter turn less a hair over the tolerance", quarter - 1.0e-7, true),
    ] {
        let doc = page_of_builders(vec![with_polygon(frame_2_by_1(1, (5.0, 5.0), degrees_of(angle)), 0, &DIAMOND, false, false)]);
        assert_eq!(path_box_of(&doc, 1).is_some(), turned, "{what}");
    }
}

#[test]
fn an_unturned_shape_with_paths_inside_its_frame_is_aligned_by_its_box_by_the_command_and_counted_as_none() {
    // The diamond in a 2 by 1 frame, not turned: the box the shapes align by is the frame's, and nothing is counted (it is not a turned shape).
    let doc = page_of_builders(vec![with_polygon(frame_2_by_1(1, (5.0, 5.0), 0.0), 0, &DIAMOND, false, false)]);
    let subject = bounds_of(&doc, 1, &[1]).unwrap();
    let frame = box_of(&doc, 1, &[1]).unwrap();
    assert_eq!(subject.by_corners, 0);
    assert!(
        near4((subject.rect.x0, subject.rect.y0, subject.rect.x1, subject.rect.y1), (frame.x0, frame.y0, frame.x1, frame.y1)),
        "{subject:?} {frame:?}"
    );
    assert_eq!(subject.cost.built, 0, "no section was built for a shape that is not turned: {:?}", subject.cost);
}

#[test]
fn a_path_with_a_coordinate_that_is_no_number_gives_no_box_with_one() {
    // A hostile coordinate: either the section is left out or the box is made of the finite paths, never a box that is not finite.
    let mut shape = frame_2_by_1(1, (5.0, 5.0), 30.0);
    for (i, (x, y)) in [("0", "0"), ("2", "0"), ("1e309", "1"), ("0", "1")].iter().enumerate() {
        let row = RowKey::Ix(u32::try_from(i).unwrap());
        shape = shape
            .row_type("Geometry", Some(0), row.clone(), if i == 0 { "MoveTo" } else { "LineTo" })
            .row_cell("Geometry", Some(0), row.clone(), "X", x)
            .row_cell("Geometry", Some(0), row, "Y", y);
    }
    let doc = page_of_builders(vec![shape]);
    let box_of_paths = path_box_of(&doc, 1);
    assert!(box_of_paths.is_none_or(|b| [b.0, b.1, b.2, b.3].iter().all(|v| v.is_finite())), "{box_of_paths:?}");
}

// ---------------------------------------------------------------------------------------------------------------- what the second pass of hand mutants found nothing watching

#[test]
fn the_library_gives_a_press_with_lines_on_the_page_and_none_ahead_the_tick_it_is_told() {
    // Through `snap_ext` (what a window's preview calls): the alignment box on and another shape on the page whose lines are all behind the key's way. The library's
    // fallback is the tick, and no hit is made.
    let mut settings = SnapSettings::none();
    settings.snap = true;
    settings.alignment_box = true;
    let r = Rect { x0: 5.0, y0: 5.0, x1: 6.0, y1: 6.0 };
    let query = Query { subject: Subject::Box(r), using: Use::Place, moving: &[1], zoom: 96.0, own_points: &[] };
    // Another shape below and to the left: ahead of nothing to the right and up.
    let behind = page_of(&[(1, 5.0, 5.0, 6.0, 6.0), (2, 1.0, 1.0, 2.0, 2.0)]);
    // Another shape above and to the right: ahead of nothing to the left.
    let ahead = page_of(&[(1, 5.0, 5.0, 6.0, 6.0), (2, 9.0, 9.0, 10.0, 10.0)]);
    for (doc, axis, forward, want) in [
        (&behind, Axis::X, true, (0.125, 0.0)),
        (&behind, Axis::Y, true, (0.0, 0.125)),
        (&ahead, Axis::X, false, (-0.125, 0.0)),
        (&ahead, Axis::Y, false, (0.0, -0.125)),
    ] {
        let page = SnapPage::read(doc, 1).unwrap();
        let extras = Extras { nudge: Some(Nudge { axis, forward, tick: 0.125 }), ..Extras::default() };
        let out = snap_ext(doc, &page, &settings, &query, extras);
        assert!(out.hits.is_empty() && (out.dx - want.0).abs() < 1e-12 && (out.dy - want.1).abs() < 1e-12, "{axis:?} {forward}: {out:?}");
    }
}

#[test]
fn a_kept_resize_dragged_to_no_width_or_height_lands_in_proportion_with_a_size() {
    // The right side dragged onto the left side, and the top onto the bottom, with `keep`: the library solves inside the shape's proportions (2 to 1), so what comes
    // back has a size and the ratio, never a box of none (the `w > 0 && h > 0` of `snap_resize_ext` is a guard on what the library does not give).
    let doc = page_of(&[(1, 2.0, 2.0, 4.0, 3.0)]);
    let extras = Extras { keep: true, ..Extras::default() };
    for sized in [(2.0, 2.0, 0.0, 1.0), (2.0, 2.0, 2.0, 0.0)] {
        let (_, (_, _, w, h)) = snap_resize_ext(&doc, 1, None, 1, sized, 96.0, extras).expect("a kept resize has a result");
        assert!(w > 0.0 && h > 0.0 && (w / h - 2.0).abs() < 1e-9, "{sized:?}: {w} by {h}");
    }
}

#[test]
fn a_label_is_in_the_drawing_units_of_the_page_and_two_chains_are_two_chains() {
    // A page drawn at 2 to 1 (`DrawingScale` 2 over `PageScale` 1): a gap of half a paper inch is 1 in the drawing. Three boxes of 2 by 2 units (x 4 to 6, 7 to 9 and 10
    // to 12, y 10 to 12) and a dragged box 1 paper inch wide.
    let mut page = PageBuilder::new().cell("PageWidth", "40").cell("PageHeight", "40").cell("PageScale", "1").cell("DrawingScale", "2");
    for (id, x) in [(1u32, 5.0), (2, 8.0), (3, 11.0)] {
        page = page.shape(
            ShapeBuilder::new(id)
                .cell("PinX", &x.to_string())
                .cell("PinY", "11")
                .cell("Width", "2")
                .cell("Height", "2")
                .cell("LocPinX", "1")
                .cell("LocPinY", "1")
                .cell("Angle", "0"),
        );
    }
    let doc = DocBuilder::new().page(1, page).build();
    let r = Rect { x0: 6.53, y0: 5.0, x1: 7.53, y1: 6.0 };
    let out = drag(&doc, &vector_settings(), r, &[]);
    assert!((r.x0 + out.dx - 6.5).abs() < 1e-9, "{out:?}");
    let g = gaps(&out);
    assert_eq!(g.len(), 3, "{g:?}");
    assert!(g.iter().all(|(_, value, label, _)| (value - 0.5).abs() < 1e-9 && label == "1 in"), "{g:?}");
    // Two chains: boxes 0.25 inch wide from x 1, 1.5 and 2 (gaps of 0.25), then a gap of 0.75, then boxes from x 3, 3.5 and 4; a box dragged to 4.53 repeats the gap.
    let boxes: Vec<(u32, f64, f64, f64, f64)> =
        [1.0, 1.5, 2.0, 3.0, 3.5, 4.0].iter().enumerate().map(|(i, x)| (u32::try_from(i).unwrap() + 1, *x, 5.0, x + 0.25, 6.0)).collect();
    let doc = page_of(&boxes);
    let r = Rect { x0: 4.53, y0: 5.0, x1: 4.78, y1: 6.0 };
    let out = drag(&doc, &vector_settings(), r, &[]);
    assert!((r.x0 + out.dx - 4.5).abs() < 1e-9, "{out:?}");
    let g = gaps(&out);
    let mut chains: Vec<u16> = g.iter().map(|x| x.0).collect();
    chains.sort_unstable();
    chains.dedup();
    assert_eq!(g.len(), 5, "{g:?}");
    assert_eq!(chains.len(), 2, "the gap of 0.75 divides the row into two chains: {g:?}");
    let with_own = g.iter().find(|x| x.3).unwrap().0;
    assert_eq!(g.iter().filter(|x| x.0 == with_own).count(), 3, "the box's own gap joins the chain it repeats: {g:?}");
}

#[test]
fn the_spline_work_and_the_constructor_bytes_of_the_page_budget_each_stop_a_page_that_spends_them() {
    use diacraft_engine::snap::{PAGE_BUILD_BYTES, PAGE_BUILD_WORK};
    // Spline work: 100 turned instances of a NURBS of 700 tuples and degree 25. The work runs out inside a section, which then gives no box, and the budget is gone: the
    // shapes after it keep the box round their corners, counted.
    let doc = turned_instances_of_constructors(100, 700, 0, 25, 0);
    let page = SnapPage::read(&doc, 1).unwrap();
    let (index, c) = (page.shapes(&doc), page.shapes(&doc).box_cost);
    assert_eq!(index.bounds_by_paths + index.bounds_by_corners, 100, "{c:?}");
    assert!(c.curve_work <= PAGE_BUILD_WORK && c.curve_work > PAGE_BUILD_WORK - PAGE_BUILD_WORK / 100, "the spline work is spent: {c:?}");
    assert!(
        index.bounds_by_corners >= 97 && c.built < 100 && c.points <= diacraft_engine::snap::PAGE_BUILD_POINTS,
        "{c:?} {}",
        index.bounds_by_corners
    );
    // Constructor bytes: 80 instances of a polyline of 20 points and 100,000 bytes of spaces: the bytes pay for as many as 4 MiB holds and no more.
    let doc = turned_instances_of_constructors(80, 0, 20, 3, 100_000);
    let page = SnapPage::read(&doc, 1).unwrap();
    let (index, c) = (page.shapes(&doc), page.shapes(&doc).box_cost);
    assert_eq!(index.bounds_by_paths + index.bounds_by_corners, 80, "{c:?}");
    assert!(c.bytes <= PAGE_BUILD_BYTES && c.bytes + 101_000 > PAGE_BUILD_BYTES, "the bytes are spent: {c:?}");
    assert!((41..=42).contains(&index.bounds_by_paths), "{} paid: {c:?}", index.bounds_by_paths);
}

/// A master of one shape: a `MoveTo`, a `NURBSTo` row of `tuples` tuples of the `degree` and a `PolylineTo` row of `points` points (the sample files' largest constructors
/// are 773 tuples and 490 pairs), and `n` instances of it turned 0.5 radians on a grid of places.
fn turned_instances_of_constructors(n: u32, tuples: usize, points: usize, degree: usize, padding: usize) -> Document {
    let mut nurbs = format!("NURBS({}, {degree}, 1, 1", tuples + degree);
    for i in 0..tuples {
        nurbs.push_str(&format!(", {:.4}, {:.4}, {}, 1", i as f64 / tuples as f64, ((i % 9) as f64) / 20.0, i + 1));
    }
    nurbs.push(')');
    let mut poly = String::from("POLYLINE(1, 1");
    for i in 0..points {
        poly.push_str(&format!(", {:.4}, {:.4}", i as f64 / points as f64, ((i % 7) as f64) / 20.0));
    }
    poly.push_str(&" ".repeat(padding));
    poly.push(')');
    let ix = |i: u32| RowKey::Ix(i);
    let mut master_shape = ShapeBuilder::new(1).cell("Width", "1").cell("Height", "1").cell("LocPinX", "0.5").cell("LocPinY", "0.5");
    master_shape = master_shape.row_type("Geometry", Some(0), ix(0), "MoveTo").row_cell("Geometry", Some(0), ix(0), "X", "0").row_cell(
        "Geometry",
        Some(0),
        ix(0),
        "Y",
        "0",
    );
    if tuples > 0 {
        master_shape = master_shape
            .row_type("Geometry", Some(0), ix(1), "NURBSTo")
            .row_cell("Geometry", Some(0), ix(1), "X", "1")
            .row_cell("Geometry", Some(0), ix(1), "Y", "0")
            .row_cell("Geometry", Some(0), ix(1), "A", &(tuples + 2).to_string())
            .row_cell("Geometry", Some(0), ix(1), "B", "1")
            .row_cell("Geometry", Some(0), ix(1), "C", "0")
            .row_cell("Geometry", Some(0), ix(1), "D", "1")
            .row_cell("Geometry", Some(0), ix(1), "E", &nurbs);
    }
    if points > 0 {
        master_shape = master_shape
            .row_type("Geometry", Some(0), ix(2), "PolylineTo")
            .row_cell("Geometry", Some(0), ix(2), "X", "0")
            .row_cell("Geometry", Some(0), ix(2), "Y", "1")
            .row_cell("Geometry", Some(0), ix(2), "A", &poly);
    }
    let mut page = PageBuilder::new().cell("PageWidth", "400").cell("PageHeight", "400");
    for i in 0..n {
        page = page.shape(
            ShapeBuilder::new(100 + i)
                .master(1)
                .cell("PinX", &(f64::from(i % 50) * 3.0 + 2.0).to_string())
                .cell("PinY", &(f64::from(i / 50) * 3.0 + 2.0).to_string())
                .cell("Width", "1")
                .cell("Height", "1")
                .cell("LocPinX", "0.5")
                .cell("LocPinY", "0.5")
                .cell("Angle", "0.5"),
        );
    }
    DocBuilder::new().master(1, MasterBuilder::new().shape(master_shape)).page(1, page).build()
}

#[test]
#[ignore = "a measurement, not a test: prints what the boxes round the paths of turned shapes cost, for the built-in masters one by one, for a crowded page of them and for hostile pages, with the page budget spent; run with --ignored --nocapture"]
fn measure_what_the_page_budget_pays_for() {
    use std::time::Instant;
    // The built-in masters, each turned 30 degrees alone on a page: what the box round its paths costs.
    let (mut worst, mut sum) = (diacraft_engine::snap::OutlineWork::default(), diacraft_engine::snap::OutlineWork::default());
    let mut masters = 0usize;
    let mut by_cost = Vec::new();
    for spec in diacraft_engine::stencil::all_masters() {
        let mut doc = page_of(&[]);
        let Ok(info) = drop_shape(&mut doc, 1, spec, 5.0, 5.0) else { continue };
        rotate_shapes(&mut doc, 1, &[info.id], Turn::To(30.0_f64.to_radians())).unwrap();
        let page = SnapPage::read(&doc, 1).unwrap();
        let index = page.shapes(&doc);
        if index.bounds_by_paths + index.bounds_by_corners == 0 {
            continue;
        }
        let c = index.box_cost;
        masters += 1;
        by_cost.push((c.points, c.elements, c.curve_work, c.bytes, spec.id, index.bounds_by_paths, index.bounds_by_corners));
        worst = diacraft_engine::snap::OutlineWork {
            points: worst.points.max(c.points),
            elements: worst.elements.max(c.elements),
            curve_work: worst.curve_work.max(c.curve_work),
            bytes: worst.bytes.max(c.bytes),
            ..worst
        };
        sum = diacraft_engine::snap::OutlineWork {
            bytes: sum.bytes + c.bytes,
            points: sum.points + c.points,
            elements: sum.elements + c.elements,
            curve_work: sum.curve_work + c.curve_work,
            ..sum
        };
    }
    by_cost.sort();
    eprintln!("MASTERS {masters} turned alone: the five dearest by points: {:?}", &by_cost[by_cost.len().saturating_sub(5)..]);
    eprintln!(
        "MASTERS worst a shape: points {} elements {} curve_work {} bytes {}; mean a shape: points {:.1} elements {:.1} curve_work {:.0} bytes {:.1}",
        worst.points,
        worst.elements,
        worst.curve_work,
        worst.bytes,
        sum.points as f64 / masters as f64,
        sum.elements as f64 / masters as f64,
        sum.curve_work as f64 / masters as f64,
        sum.bytes as f64 / masters as f64
    );
    // A crowded page: 5,000 turned shapes, the masters in turn.
    let specs: Vec<_> = diacraft_engine::stencil::all_masters().filter(|m| !m.is_connector()).collect();
    let mut doc = page_of(&[]);
    for i in 0..5_000usize {
        let (x, y) = ((i % 100) as f64 * 1.5 + 1.0, (i / 100) as f64 * 1.5 + 1.0);
        let info = drop_shape(&mut doc, 1, specs[i % specs.len()], x, y).unwrap();
        rotate_shapes(&mut doc, 1, &[info.id], Turn::To((10.0 + (i % 7) as f64 * 11.0).to_radians())).unwrap();
    }
    let started = Instant::now();
    let page = SnapPage::read(&doc, 1).unwrap();
    let index = page.shapes(&doc);
    let took = started.elapsed();
    eprintln!(
        "CROWDED 5000 stock shapes turned: page read {:.1} ms; by paths {} by corners {} cost {:?}",
        took.as_secs_f64() * 1e3,
        index.bounds_by_paths,
        index.bounds_by_corners,
        index.box_cost
    );
    // The same page with no shape turned, as the yardstick: the frames alone.
    let flat = {
        let mut flat = doc.clone();
        let ids: Vec<u32> = (1..=5_000).collect();
        // `Turn::To(0)` on all of them, one at a time through the same function.
        for id in &ids {
            rotate_shapes(&mut flat, 1, &[*id], Turn::To(0.0)).unwrap();
        }
        flat
    };
    let started = Instant::now();
    let page = SnapPage::read(&flat, 1).unwrap();
    let index = page.shapes(&flat);
    eprintln!(
        "CROWDED same page, none turned: page read {:.1} ms; by paths {} by corners {}",
        started.elapsed().as_secs_f64() * 1e3,
        index.bounds_by_paths,
        index.bounds_by_corners
    );
    // Hostile pages: 5,000 turned instances of a master that is dear to build in one thing each.
    for (what, doc) in [
        ("400 rows of LineTo", turned_instances_of_a_big_master(5_000, 400)),
        ("a NURBS of 700 tuples (30 KB)", turned_instances_of_constructors(5_000, 700, 0, 3, 0)),
        ("a polyline of 3,000 points (50 KB)", turned_instances_of_constructors(5_000, 0, 3_000, 3, 0)),
        ("both constructors", turned_instances_of_constructors(5_000, 700, 3_000, 3, 0)),
        ("a NURBS of 700 tuples of degree 25 (30 KB)", turned_instances_of_constructors(5_000, 700, 0, 25, 0)),
    ] {
        let started = Instant::now();
        let page = SnapPage::read(&doc, 1).unwrap();
        let index = page.shapes(&doc);
        let took = started.elapsed();
        eprintln!(
            "HOSTILE 5000 turned instances of {what}: page read {:.1} ms; by paths {} by corners {} cost {:?}",
            took.as_secs_f64() * 1e3,
            index.bounds_by_paths,
            index.bounds_by_corners,
            index.box_cost
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------- row 10, a dropped master lands by its own connection points

#[test]
fn a_dropped_master_lands_with_its_own_connection_point_on_another_shapes_and_not_by_its_box() {
    // V16 with the stock masters: the Off-Page Reference (0.6 by 0.8 inch) has connection points on its sides 0.7 of its height up, 0.56 inch above its bottom: not the box's
    // side middles (0.4 up) and so not one of its nine places. T is a stock rectangle at (5, 5) with a point at the middle of its left side; the master is dropped to the left
    // of it with its right point 0.03 inch short of T's and 0.02 inch above it, and its box's own right middle 0.14 inch under T's point, out of the 0.0625 inch reach.
    let mut doc = page_of(&[]);
    let t = drop_shape(&mut doc, 1, find_master("rectangle").unwrap(), 5.0, 5.0).unwrap();
    let t_left = connection_points(&doc, 1, t.id)
        .into_iter()
        .find(|p| (p.x - (5.0 - t.width / 2.0)).abs() < 1e-9 && (p.y - 5.0).abs() < 1e-9)
        .expect("the rectangle's left middle");
    let mut s = session_of(doc);
    run(
        &mut s,
        "snap.set",
        json!({"snap_connection_points": true, "dynamic_grid": false, "alignment_box": false, "snap_geometry": false, "snap_vertices": false, "snap_handles": false, "intersections": false, "guides": false}),
    );
    let (cx, cy) = (t_left.x - 0.03 - 0.3, t_left.y + 0.02 - 0.16);
    let r = run(&mut s, "shape.drop", json!({"master": "off_page_reference", "page": 1, "x": cx, "y": cy, "snap": {"zoom": 96.0}}));
    let id = u32::try_from(r["id"].as_u64().unwrap()).unwrap();
    let right = connection_points(s.document().unwrap(), 1, id).into_iter().find(|p| p.index == 3).expect("the master's right point");
    assert!((right.x - t_left.x).abs() < 1e-9 && (right.y - t_left.y).abs() < 1e-9, "the point is on T's: {right:?} against {t_left:?} ({r})");
    // The hit is a connection point, and the same drop with the option off stays where it was put: nothing else is in reach.
    assert!(r["snap"]["hits"].as_array().is_some_and(|h| h.iter().any(|h| h["target"] == json!("connection_point"))), "{r}");
    // Its place mark says whose point it met and which: T's left middle, the number of its row.
    assert!(
        r["snap"]["marks"].as_array().is_some_and(|m| m.iter().any(|m| m["mark"] == json!("place")
            && m["target"] == json!("connection_point")
            && m["shape"] == json!(t.id)
            && m["point"] == json!(t_left.index)
            && m["hit"] == json!(true))),
        "{r}"
    );
    let mut plain = session_of({
        let mut doc = page_of(&[]);
        drop_shape(&mut doc, 1, find_master("rectangle").unwrap(), 5.0, 5.0).unwrap();
        doc
    });
    run(
        &mut plain,
        "snap.set",
        json!({"snap_connection_points": false, "dynamic_grid": false, "alignment_box": false, "snap_geometry": false, "snap_vertices": false, "snap_handles": false, "intersections": false, "guides": false}),
    );
    let r = run(&mut plain, "shape.drop", json!({"master": "off_page_reference", "page": 1, "x": cx, "y": cy, "snap": {"zoom": 96.0}}));
    let id = u32::try_from(r["id"].as_u64().unwrap()).unwrap();
    let stays = connection_points(plain.document().unwrap(), 1, id).into_iter().find(|p| p.index == 3).unwrap();
    assert!((stays.x - (cx + 0.3)).abs() < 1e-9 && (stays.y - (cy + 0.16)).abs() < 1e-9, "with the option off it stays: {stays:?}");
}

// ---------------------------------------------------------------------------------------------------------------- what hand mutants found nothing watching

#[test]
fn matching_another_shapes_angle_needs_the_snap_switch_and_the_handles_step_does_not() {
    // The Snap switch off: the angle of another shape (33) pulls nothing, and the handle's own step of 5 degrees at 150 points rules: raw 35 stays 35.
    let doc = turn_page(33.0);
    let page = SnapPage::read(&doc, 1).unwrap();
    let mut off = vector_settings();
    off.snap = false;
    let turn = TurnOf { pivot: (3.0, 5.0), angle: 35.0_f64.to_radians(), radius: 150.0 / 96.0, zoom: 96.0 };
    let out = snap_turn(&doc, &page, &off, &[1], turn);
    assert!((out.turned.unwrap().to_degrees() - 35.0).abs() < 1e-9, "{out:?}");
    assert!(!out.hits.is_empty() && out.hits.iter().all(|h| h.target == Target::AngleStep), "{out:?}");
    // A page that inhibits snapping is the same.
    let mut page_cells = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20").cell("InhibitSnap", "1");
    for (id, angle) in [(1u32, 0.0_f64), (2, 33.0_f64.to_radians())] {
        page_cells = page_cells.shape(
            ShapeBuilder::new(id)
                .cell("PinX", &(f64::from(id) * 3.0).to_string())
                .cell("PinY", "5")
                .cell("Width", "1")
                .cell("Height", "1")
                .cell("LocPinX", "0.5")
                .cell("LocPinY", "0.5")
                .cell("Angle", &angle.to_string()),
        );
    }
    let inhibited = DocBuilder::new().page(1, page_cells).build();
    let page = SnapPage::read(&inhibited, 1).unwrap();
    let out = snap_turn(&inhibited, &page, &vector_settings(), &[1], turn);
    assert!((out.turned.unwrap().to_degrees() - 35.0).abs() < 1e-9, "{out:?}");
}

#[test]
fn a_press_with_nothing_ahead_goes_a_tick_either_way_and_with_the_snap_switch_off() {
    // Nothing ahead (the dynamic grid and the alignment box off): the library's tick, forward and back.
    let mut s = session_of(page_of(&[(1, 5.0, 5.0, 6.0, 6.0)]));
    run(&mut s, "snap.set", json!({"dynamic_grid": false, "alignment_box": false}));
    press(&mut s, -1.0, 0.0, Some(0.125));
    assert!((box_in(&s, 1).x0 - 4.875).abs() < 1e-9, "{:?}", box_in(&s, 1));
    press(&mut s, 0.0, -1.0, Some(0.125));
    assert!((box_in(&s, 1).y0 - 4.875).abs() < 1e-9, "{:?}", box_in(&s, 1));
    // The Snap switch off says nothing about places: a press is one tick, back as well as forward (a nudge is a key, not a snap).
    let mut s = session_of(page_of(&[(1, 5.0, 5.0, 6.0, 6.0), (2, 8.0, 5.0, 9.0, 6.0)]));
    run(&mut s, "snap.set", json!({"snap": false}));
    press(&mut s, 1.0, 0.0, Some(0.125));
    assert!((box_in(&s, 1).x0 - 5.125).abs() < 1e-9, "{:?}", box_in(&s, 1));
    press(&mut s, -1.0, 0.0, Some(0.125));
    press(&mut s, -1.0, 0.0, Some(0.125));
    assert!((box_in(&s, 1).x0 - 4.875).abs() < 1e-9, "{:?}", box_in(&s, 1));
}
