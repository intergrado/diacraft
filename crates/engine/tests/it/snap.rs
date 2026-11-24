//! Snapping: the function itself, on pages built in code. The targets (grid, ruler, guides, alignment boxes, the dynamic grid, connection points,
//! vertices, handles, outlines, intersections), the strength between them, the switches, `InhibitSnap`, drawing scale, and what a query costs.
//!
//! Lengths are paper inches, as the commands and the window use them. A page built here has its cells in drawing units, as a file's does.

use diacraft_doc::{Document, RowKey, ShapeType};
use diacraft_engine::draw::{DropInfo, connection_points, connection_points_within, drop_shape};
use diacraft_engine::snap::guides::add_guide;
use diacraft_engine::snap::{
    Anchor, Axis, Glue, MAX_BUILD_ELEMENTS, MAX_BUILD_POINTS, MAX_BUILD_WORK, MAX_CANDIDATES, MAX_CONNECTION_ROWS_PER_QUERY, MAX_CONSTRUCTOR_BYTES,
    MAX_GATHERS, MAX_GUIDES, MAX_LIBRARY_QUERIES, MAX_TARGET_PIECES, MAX_TARGET_POINTS, Query, Rect, STRENGTH_MAX, SnapPage, SnapSettings, Snapped,
    Strength, Subject, Target, Use,
    index::{MAX_INDEXED, MAX_VISITS},
    snap,
};
use diacraft_engine::stencil::find_master;
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};

const LETTER: (f64, f64) = (11.0, 8.5);
const A4: (f64, f64) = (11.693_307_086_614_17, 8.267_716_535_433_07);

/// A scale: `PageScale` and `DrawingScale` as they are held (inches in `V`), and the drawing inches to a paper inch they make.
struct Scale {
    name: &'static str,
    paper: f64,
    drawing: f64,
}

const SCALES: [Scale; 3] = [
    Scale { name: "1:1", paper: 1.0, drawing: 1.0 },
    Scale { name: "1:100", paper: 1.0, drawing: 100.0 },
    // A quarter inch to the foot: 0.25 inch of paper is 12 inches of drawing.
    Scale { name: "1/4 in = 1 ft", paper: 0.25, drawing: 12.0 },
];

impl Scale {
    fn k(&self) -> f64 {
        self.drawing / self.paper
    }
}

/// A page of `paper` inches at `scale`, holding `shapes`, each `(id, centre x, centre y, width, height)` in **paper** inches (the cells are in drawing
/// units, so each is multiplied by `k`).
fn page_of(paper: (f64, f64), scale: &Scale, shapes: &[(u32, f64, f64, f64, f64)]) -> Document {
    let k = scale.k();
    let mut page = PageBuilder::new()
        .cell("PageWidth", &(paper.0 * k).to_string())
        .cell("PageHeight", &(paper.1 * k).to_string())
        .cell("PageScale", &scale.paper.to_string())
        .cell("DrawingScale", &scale.drawing.to_string());
    for (id, x, y, w, h) in shapes {
        page = page.shape(
            ShapeBuilder::new(*id)
                .cell("PinX", &(x * k).to_string())
                .cell("PinY", &(y * k).to_string())
                .cell("Width", &(w * k).to_string())
                .cell("Height", &(h * k).to_string())
                .cell("LocPinX", &(w * k / 2.0).to_string())
                .cell("LocPinY", &(h * k / 2.0).to_string())
                .cell("Angle", "0"),
        );
    }
    DocBuilder::new().page(1, page).build()
}

fn plain_page(shapes: &[(u32, f64, f64, f64, f64)]) -> Document {
    page_of(LETTER, &SCALES[0], shapes)
}

/// Settings with only the named options on. **The grid and the ruler reach as far as a strength goes** (999 pixels, the most), so that these tests, which are about where the lines
/// are and which kind outranks which, find the nearest line as they did when the two pulled from any distance; the reach itself is `snap_strength.rs`'s.
fn only(f: impl FnOnce(&mut SnapSettings)) -> SnapSettings {
    let strength = Strength { grid: STRENGTH_MAX, ruler: STRENGTH_MAX, ..Strength::default() };
    let mut s = SnapSettings { snap: true, glue: true, strength, ..SnapSettings::none() };
    f(&mut s);
    s
}

fn snap_point(doc: &Document, settings: &SnapSettings, p: (f64, f64), using: Use, moving: &[u32]) -> Snapped {
    let page = SnapPage::read(doc, 1).unwrap();
    snap(doc, &page, settings, &Query { subject: Subject::Point(p), using, moving, own_points: &[], zoom: 96.0 })
}

fn snap_box(doc: &Document, settings: &SnapSettings, r: Rect, moving: &[u32]) -> Snapped {
    let page = SnapPage::read(doc, 1).unwrap();
    snap(doc, &page, settings, &Query { subject: Subject::Box(r), using: Use::Place, moving, own_points: &[], zoom: 96.0 })
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// ------------------------------------------------------------------------------------------------------------------ the grid

#[test]
fn a_point_snaps_to_the_grid_at_letter_and_a4_and_three_scales() {
    // The grid's pitch is in drawing inches (16 points apart at the least, an eighth times a power of two): 0.25 of paper at 1:1, 32 drawing
    // inches (0.32 of paper) at 1:100, and 8 drawing inches (1/6 of paper) at a quarter inch to the foot.
    let pitches = [0.25, 0.32, 8.0 / 48.0];
    for paper in [LETTER, A4] {
        for (scale, pitch) in SCALES.iter().zip(pitches) {
            let doc = page_of(paper, scale, &[]);
            let settings = only(|s| s.grid = true);
            let p = (1.13, 2.60);
            let out = snap_point(&doc, &settings, p, Use::Place, &[]);
            let (x, y) = out.apply(p);
            let want = |v: f64| (v / pitch).round() * pitch;
            assert!(near(x, want(p.0)) && near(y, want(p.1)), "{} on {paper:?}: {x} {y}, wanted {} {}", scale.name, want(p.0), want(p.1));
            assert!((x - p.0).abs() <= pitch / 2.0 + 1e-9 && (y - p.1).abs() <= pitch / 2.0 + 1e-9);
            assert_eq!(out.hits.len(), 2, "one hit for each axis: {out:?}");
            assert!(out.hits.iter().all(|h| h.target == Target::Grid && h.anchor == Anchor::Point));
        }
    }
}

#[test]
fn the_numbers_of_the_three_scales() {
    let settings = only(|s| s.grid = true);
    // 1:1: 1.13 -> 1.25 (0.12 against 0.13), 2.60 -> 2.5.
    let doc = page_of(LETTER, &SCALES[0], &[]);
    let (x, y) = snap_point(&doc, &settings, (1.13, 2.60), Use::Place, &[]).apply((1.13, 2.60));
    assert!(near(x, 1.25) && near(y, 2.5), "{x} {y}");
    // 1:100: lines 0.32 apart: 1.13 -> 1.28 (4 lines), 2.60 -> 2.56 (8 lines).
    let doc = page_of(LETTER, &SCALES[1], &[]);
    let (x, y) = snap_point(&doc, &settings, (1.13, 2.60), Use::Place, &[]).apply((1.13, 2.60));
    assert!(near(x, 1.28) && near(y, 2.56), "{x} {y}");
    // A quarter inch to the foot: lines 1/6 apart: 1.13 -> 7/6, 2.60 -> 16/6.
    let doc = page_of(LETTER, &SCALES[2], &[]);
    let (x, y) = snap_point(&doc, &settings, (1.13, 2.60), Use::Place, &[]).apply((1.13, 2.60));
    assert!(near(x, 7.0 / 6.0) && near(y, 16.0 / 6.0), "{x} {y}");
}

#[test]
fn a_variable_grid_follows_the_zoom() {
    let doc = plain_page(&[]);
    let settings = only(|s| s.grid = true);
    let page = SnapPage::read(&doc, 1).unwrap();
    let at = |zoom: f64| {
        let q = Query { subject: Subject::Point((1.06, 1.06)), using: Use::Place, moving: &[], own_points: &[], zoom };
        snap(&doc, &page, &settings, &q).apply((1.06, 1.06)).0
    };
    // At 96 points to an inch the lines are 0.25 apart; zoomed in 8 times they are an eighth apart.
    assert!(near(at(96.0), 1.0), "{}", at(96.0));
    assert!(near(at(768.0), 1.0), "{}", at(768.0));
    let q = |zoom: f64| Query { subject: Subject::Point((1.07, 1.07)), using: Use::Place, moving: &[], own_points: &[], zoom };
    let coarse = snap(&doc, &page, &settings, &q(96.0)).apply((1.07, 1.07)).0;
    let fine = snap(&doc, &page, &settings, &q(768.0)).apply((1.07, 1.07)).0;
    assert!(near(coarse, 1.0), "{coarse}");
    assert!(near(fine, 1.125), "an eighth: {fine}");
}

#[test]
fn a_fixed_grid_sits_at_the_origin_plus_multiples_of_the_spacing() {
    // Origin 0.3, spacing 0.5, density 0: lines at 0.3, 0.8, 1.3 and -0.2, at any zoom.
    let page = PageBuilder::new()
        .cell("PageWidth", "11")
        .cell("PageHeight", "8.5")
        .cell("XGridDensity", "0")
        .cell("YGridDensity", "0")
        .cell("XGridSpacing", "0.5")
        .cell("YGridSpacing", "0.5")
        .cell("XGridOrigin", "0.3")
        .cell("YGridOrigin", "0.3");
    let doc = DocBuilder::new().page(1, page).build();
    let settings = only(|s| s.grid = true);
    for zoom in [20.0, 96.0, 2_000.0] {
        let p = SnapPage::read(&doc, 1).unwrap();
        for (from, to) in [(0.35, 0.3), (0.7, 0.8), (1.2, 1.3), (-0.1, -0.2)] {
            let q = Query { subject: Subject::Point((from, from)), using: Use::Place, moving: &[], own_points: &[], zoom };
            let at = snap(&doc, &p, &settings, &q).apply((from, from));
            assert!(near(at.0, to) && near(at.1, to), "zoom {zoom}: {from} -> {at:?}, wanted {to}");
        }
    }
}

#[test]
fn a_box_is_pulled_by_whichever_of_its_edges_and_middle_is_nearest_a_line() {
    let doc = plain_page(&[]);
    let settings = only(|s| s.grid = true);
    // Edges at 1.12 and 2.20, middle 1.66: the right edge is 0.05 from 2.25, the nearest anything is.
    let out = snap_box(&doc, &settings, Rect { x0: 1.12, y0: 1.0, x1: 2.20, y1: 1.5 }, &[]);
    assert!(near(out.dx, 0.05), "{out:?}");
    assert!(out.hits.iter().any(|h| h.axis == Some(Axis::X) && h.anchor == Anchor::High && h.target == Target::Grid));
    // Y edges at 1.0 and 1.5 are on lines already: no move.
    assert!(near(out.dy, 0.0));
}

#[test]
fn the_rulers_subdivisions_are_a_target_of_their_own() {
    let doc = plain_page(&[]);
    // Ruler only, in inches at 96 points to an inch: an eighth apart.
    let out = snap_point(&doc, &only(|s| s.ruler = true), (1.06, 1.07), Use::Place, &[]);
    let (x, y) = out.apply((1.06, 1.07));
    assert!(near(x, 1.0) && near(y, 1.125), "{x} {y}");
    assert!(out.hits.iter().all(|h| h.target == Target::Ruler));
    // With both on, the grid wins where it pulls: grid lines are the stronger of the two.
    let out = snap_point(&doc, &only(|s| (s.ruler, s.grid) = (true, true)), (1.06, 1.07), Use::Place, &[]);
    assert!(out.hits.iter().all(|h| h.target == Target::Grid), "{out:?}");
}

#[test]
fn the_grid_is_the_one_of_the_off_by_flags_of_the_two_bits() {
    // Settings off: nothing moves, nothing is said.
    let doc = plain_page(&[]);
    let none = only(|_| {});
    let out = snap_point(&doc, &none, (1.13, 2.6), Use::Place, &[]);
    assert_eq!(out.apply((1.13, 2.6)), (1.13, 2.6));
    assert!(out.hits.is_empty());
    // The Snap switch off turns every target off, with the grid on.
    let off = only(|s| {
        s.grid = true;
        s.snap = false;
    });
    let out = snap_point(&doc, &off, (1.13, 2.6), Use::Place, &[]);
    assert!(out.hits.is_empty() && out.dx == 0.0 && out.dy == 0.0);
}

#[test]
fn numbers_that_are_not_numbers_snap_to_nothing() {
    let doc = plain_page(&[]);
    let settings = SnapSettings::default();
    for p in [(f64::NAN, 1.0), (1.0, f64::INFINITY), (f64::NEG_INFINITY, f64::NAN)] {
        let out = snap_point(&doc, &settings, p, Use::Place, &[]);
        assert!(out.hits.is_empty() && out.dx == 0.0 && out.dy == 0.0, "{p:?}");
    }
    let r = Rect { x0: f64::NAN, y0: 0.0, x1: 1.0, y1: 1.0 };
    assert!(snap_box(&doc, &settings, r, &[]).hits.is_empty());
    // A zoom that is not a positive number reads as 100%.
    let page = SnapPage::read(&doc, 1).unwrap();
    for zoom in [0.0, -3.0, f64::NAN, f64::INFINITY] {
        let q = Query { subject: Subject::Point((1.13, 2.6)), using: Use::Place, moving: &[], own_points: &[], zoom };
        let out = snap(&doc, &page, &only(|s| s.grid = true), &q);
        assert!(near(out.apply((1.13, 2.6)).0, 1.25), "{zoom}");
    }
}

// ----------------------------------------------------------------------------------------------------------------- guides

#[test]
fn a_guide_pulls_from_its_strength_and_a_stronger_target_wins_over_a_nearer_one() {
    let mut doc = plain_page(&[]);
    add_guide(&mut doc, 1, Axis::X, 3.0, 0).unwrap();
    add_guide(&mut doc, 1, Axis::Y, 2.0, 0).unwrap();
    let guides = only(|s| s.guides = true);
    // 3.05 is 0.05 off a vertical guide; at 96 points to an inch the guide pulls from 8 points (0.083).
    let out = snap_point(&doc, &guides, (3.05, 4.4), Use::Place, &[]);
    assert!(near(out.apply((3.05, 4.4)).0, 3.0) && near(out.apply((3.05, 4.4)).1, 4.4), "{out:?}");
    assert_eq!(out.hits.len(), 1);
    assert_eq!(out.hits[0].target, Target::Guide);
    assert!(out.hits[0].shape.is_some());
    // Out of reach: 3.2 is 19 points away.
    assert!(snap_point(&doc, &guides, (3.2, 4.4), Use::Place, &[]).hits.is_empty());
    // A level guide pulls on y alone.
    let out = snap_point(&doc, &guides, (7.0, 2.04), Use::Place, &[]);
    assert!(near(out.apply((7.0, 2.04)).1, 2.0) && near(out.apply((7.0, 2.04)).0, 7.0));
    // The grid line at 3.0 is nearer to 3.01 than the guide at 3.03; the guide is the stronger and wins.
    let mut doc = plain_page(&[]);
    add_guide(&mut doc, 1, Axis::X, 3.03, 0).unwrap();
    let both = only(|s| (s.guides, s.grid) = (true, true));
    let out = snap_point(&doc, &both, (3.01, 1.0), Use::Place, &[]);
    assert!(near(out.apply((3.01, 1.0)).0, 3.03), "{out:?}");
    // Zoomed out, a point pull of 8 screen points reaches farther in inches.
    let page = SnapPage::read(&doc, 1).unwrap();
    let q = Query { subject: Subject::Point((3.5, 1.0)), using: Use::Place, moving: &[], own_points: &[], zoom: 12.0 };
    let out = snap(&doc, &page, &guides_only(), &q);
    assert!(near(out.apply((3.5, 1.0)).0, 3.03), "{out:?}");
}

fn guides_only() -> SnapSettings {
    only(|s| s.guides = true)
}

#[test]
fn a_guide_at_a_scale_is_a_place_in_the_drawings_units() {
    // A guide made at 2.0 paper inches on a page at 1:100 is at 200 drawing inches, and is snapped to at 2.0 paper inches.
    let mut doc = page_of(LETTER, &SCALES[1], &[]);
    let info = add_guide(&mut doc, 1, Axis::X, 2.0, 0).unwrap();
    assert!(near(info.at, 2.0));
    let shape = doc.page(1).unwrap().shapes.iter().find(|s| s.id == info.id).unwrap().clone();
    assert_eq!(shape.sheet.cell("PinX").unwrap().value.as_ref().unwrap().number(), Some(200.0));
    let out = snap_point(&doc, &guides_only(), (2.03, 3.0), Use::Place, &[]);
    assert!(near(out.apply((2.03, 3.0)).0, 2.0), "{out:?}");
}

#[test]
fn inhibit_snap_keeps_the_grid_and_the_ruler_and_nothing_else_of_the_page() {
    // 1 stops the snap to shapes, and the snap to the grid stays.
    let page = PageBuilder::new().cell("PageWidth", "11").cell("PageHeight", "8.5").cell("InhibitSnap", "1").shape(
        ShapeBuilder::new(1).cell("PinX", "3").cell("PinY", "3").cell("Width", "1").cell("Height", "1").cell("LocPinX", "0.5").cell("LocPinY", "0.5"),
    );
    let mut doc = DocBuilder::new().page(1, page).build();
    add_guide(&mut doc, 1, Axis::X, 5.0, 10).unwrap();
    let settings = only(|s| (s.grid, s.guides, s.alignment_box, s.dynamic_grid) = (true, true, true, true));
    let out = snap_point(&doc, &settings, (5.04, 3.51), Use::Place, &[]);
    assert!(out.hits.iter().all(|h| h.target == Target::Grid), "{out:?}");
    assert!(near(out.apply((5.04, 3.51)).0, 5.0), "grid line at 5.0 all the same");
    assert_eq!(out.candidates, 0, "no shape was looked at");
}

// ----------------------------------------------------------------------------------------------------- other shapes

#[test]
fn a_box_aligns_to_the_edges_and_middles_of_other_shapes() {
    // Shape 1: 2 to 4 across, 3 to 4 up. A box dropped with its left edge at 2.04 snaps to 2.0; its middle 1 inch wide near 3.0 snaps to 3.0.
    let doc = plain_page(&[(1, 3.0, 3.5, 2.0, 1.0)]);
    let settings = only(|s| s.alignment_box = true);
    let out = snap_box(&doc, &settings, Rect { x0: 2.04, y0: 5.0, x1: 3.04, y1: 5.5 }, &[]);
    assert!(near(out.dx, -0.04), "{out:?}");
    let hit = out.hits.iter().find(|h| h.axis == Some(Axis::X)).unwrap();
    assert_eq!((hit.target, hit.shape, hit.anchor), (Target::Alignment, Some(1), Anchor::Low));
    assert!(near(hit.line, 2.0));
    // The aid reaches from the box to the shape along the line (on y: 3 to 5.5).
    let span = hit.span.unwrap();
    assert!(near(span.0, 3.0) && near(span.1, 5.5), "{span:?}");
    // The middle of a box 0.5 up and the shape's middle (y 3.5): edges at 3.4..3.9?: use a box whose middle is 3.47.
    let out = snap_box(&doc, &settings, Rect { x0: 4.5, y0: 3.22, x1: 5.5, y1: 3.72 }, &[]);
    assert!(near(out.dy, 0.03), "{out:?}");
    assert_eq!(out.hits.iter().find(|h| h.axis == Some(Axis::Y)).map(|h| h.anchor), Some(Anchor::Middle));
}

#[test]
fn the_shapes_that_move_are_not_targets_and_edges_between_different_sizes_align() {
    let doc = plain_page(&[(1, 3.0, 3.5, 2.0, 1.0), (2, 6.0, 3.5, 0.5, 0.5)]);
    let settings = only(|s| s.alignment_box = true);
    // Moving shape 1 up out of its row, with its edges 0.02 off where they were: shape 1's own edges (2, 3, 4) are not targets while it moves.
    let r = Rect { x0: 2.02, y0: 6.0, x1: 4.02, y1: 7.0 };
    let out = snap_box(&doc, &settings, r, &[1]);
    assert!(out.hits.is_empty(), "{out:?}");
    let out = snap_box(&doc, &settings, r, &[]);
    assert!(near(out.dx, -0.02), "any other shape, the same box: {out:?}");
    // The large shape's top (4.0) and the small shape's top (3.75) are different heights; a box whose top is 3.78 aligns to the small one's.
    // (row 12, R35: a box lines up with shapes anywhere on the page, so the large shape, 3 inches off, is a target now: the box's bottom was on the
    // large shape's bottom line (3.0) and took it. Its bottom is at 3.3 now, 0.3 off every line.)
    let out = snap_box(&doc, &settings, Rect { x0: 7.0, y0: 3.3, x1: 8.0, y1: 3.78 }, &[]);
    assert!(near(out.dy, -0.03), "{out:?}");
    assert_eq!(out.hits.iter().find(|h| h.axis == Some(Axis::Y)).and_then(|h| h.shape), Some(2));
}

#[test]
fn the_dynamic_grid_adds_the_page_margins_and_the_spacing_between_shapes() {
    // Margins are 0.25 in a page with none of its own (the root's): a box whose left edge is 0.29 from the page's edge snaps to 0.25.
    let doc = plain_page(&[]);
    let off = snap_box(&doc, &only(|_| {}), Rect { x0: 0.29, y0: 2.0, x1: 1.29, y1: 3.0 }, &[]);
    assert!(off.hits.is_empty());
    let on = snap_box(&doc, &only(|s| s.dynamic_grid = true), Rect { x0: 0.29, y0: 2.0, x1: 1.29, y1: 3.0 }, &[]);
    assert!(near(on.dx, -0.04), "{on:?}");
    assert_eq!(on.hits.first().map(|h| h.target), Some(Target::PageMargin));
    // The right margin: the page is 11 wide, so a right edge at 10.72 snaps to 10.75.
    let on = snap_box(&doc, &only(|s| s.dynamic_grid = true), Rect { x0: 9.72, y0: 2.0, x1: 10.72, y1: 3.0 }, &[]);
    assert!(near(on.dx, 0.03), "{on:?}");

    // Spacing: shapes 1 and 3 are 1 wide at x 2..3 and 6..7, a box 1 wide between them has gaps (box.x0 - 3) and (6 - box.x1); at 4.02..5.02 they
    // are 1.02 and 0.98 and the box moves 0.02 to even them.
    let doc = plain_page(&[(1, 2.5, 5.0, 1.0, 1.0), (3, 6.5, 5.0, 1.0, 1.0)]);
    let settings = only(|s| s.dynamic_grid = true);
    let out = snap_box(&doc, &settings, Rect { x0: 4.02, y0: 4.5, x1: 5.02, y1: 5.5 }, &[]);
    assert!(near(out.dx, -0.02), "{out:?}");
    let hit = out.hits.iter().find(|h| h.target == Target::Spacing).unwrap();
    assert!(near(hit.gap.unwrap(), 1.0), "{hit:?}");
    // The same gap as between two other shapes: shapes at 2..3 and 4..5 are 1 apart, and a box beside the second, 1.03 from it, comes to 1.0.
    let doc = plain_page(&[(1, 2.5, 5.0, 1.0, 1.0), (2, 4.5, 5.0, 1.0, 1.0)]);
    let out = snap_box(&doc, &settings, Rect { x0: 6.03, y0: 4.5, x1: 7.03, y1: 5.5 }, &[]);
    assert!(near(out.dx, -0.03), "{out:?}");
    assert_eq!(out.hits.iter().find(|h| h.axis == Some(Axis::X)).map(|h| h.target), Some(Target::Spacing));
    // Shapes in another row (no overlap across) are not neighbours.
    let doc = plain_page(&[(1, 2.5, 1.0, 1.0, 1.0), (2, 4.5, 1.0, 1.0, 1.0)]);
    let out = snap_box(&doc, &settings, Rect { x0: 6.03, y0: 4.5, x1: 7.03, y1: 5.5 }, &[]);
    assert!(out.hits.iter().all(|h| h.target != Target::Spacing), "{out:?}");
}

#[test]
fn a_connector_end_lands_on_a_connection_point_and_says_which() {
    let mut doc = plain_page(&[]);
    let rectangle = find_master("rectangle").unwrap();
    let info = drop_shape(&mut doc, 1, rectangle, 4.0, 4.0).unwrap();
    // The rectangle's connection points are the middles of its sides: the right one is at (4 + width/2, 4).
    let right = 4.0 + info.width / 2.0;
    let settings = only(|s| s.glue_connection_points = true);
    let p = (right + 0.06, 4.05);
    let out = snap_point(&doc, &settings, p, Use::Connect, &[]);
    let (x, y) = out.apply(p);
    assert!(near(x, right) && near(y, 4.0), "{out:?}");
    assert!(matches!(out.glue, Some(Glue::ConnectionPoint { shape, .. }) if shape == info.id), "{out:?}");
    assert_eq!(out.hits[0].target, Target::ConnectionPoint);
    // A place that is only placed, not connected, is pulled to the same point by the option of SnapSettings and does not
    // glue; the option of GlueSettings alone does not pull it.
    assert!(snap_point(&doc, &settings, p, Use::Place, &[]).hits.is_empty(), "glue to connection points is a connector's");
    let settings = only(|s| (s.glue_connection_points, s.snap_connection_points) = (true, true));
    let out = snap_point(&doc, &settings, p, Use::Place, &[]);
    assert!(out.glue.is_none(), "{out:?}");
    assert_eq!((out.hits.len(), out.hits[0].target, out.hits[0].shape), (1, Target::ConnectionPoint, Some(info.id)), "{out:?}");
    assert!(near(out.apply(p).0, right) && near(out.apply(p).1, 4.0), "{out:?}");
    // The option of SnapSettings does not glue a connector's end either: that is GlueSettings'.
    let snap_only = only(|s| s.snap_connection_points = true);
    assert!(snap_point(&doc, &snap_only, p, Use::Connect, &[]).glue.is_none(), "snap to connection points is a shape's");
    // Placed with the Snap switch off, nothing: it is the Snap switch that a placing asks, and the Glue switch that a connecting asks.
    let snap_off = only(|s| {
        s.glue_connection_points = true;
        s.snap_connection_points = true;
        s.snap = false;
    });
    assert!(snap_point(&doc, &snap_off, p, Use::Place, &[]).hits.is_empty());
    assert!(snap_point(&doc, &snap_off, p, Use::Connect, &[]).glue.is_some());
    // Out of reach: 12 points is 0.125 of an inch.
    assert!(snap_point(&doc, &settings, (right + 0.3, 4.0), Use::Connect, &[]).glue.is_none());
    // The Glue switch off turns glue off.
    let off = only(|s| {
        s.glue_connection_points = true;
        s.glue = false;
    });
    assert!(snap_point(&doc, &off, p, Use::Connect, &[]).glue.is_none());
}

#[test]
fn a_connection_point_is_stronger_than_a_grid_line_and_a_guide() {
    let mut doc = plain_page(&[]);
    let rectangle = find_master("rectangle").unwrap();
    let info = drop_shape(&mut doc, 1, rectangle, 4.0, 4.0).unwrap();
    let right = 4.0 + info.width / 2.0;
    add_guide(&mut doc, 1, Axis::X, right + 0.04, 100).unwrap();
    let settings = only(|s| (s.glue_connection_points, s.guides, s.grid) = (true, true, true));
    let p = (right + 0.035, 4.0);
    let out = snap_point(&doc, &settings, p, Use::Connect, &[]);
    assert!(near(out.apply(p).0, right), "{out:?}");
    assert!(out.glue.is_some());
}

#[test]
fn handles_vertices_and_outlines_are_glue_targets_and_report_their_shape() {
    let mut doc = plain_page(&[]);
    let rectangle = find_master("rectangle").unwrap();
    let info = drop_shape(&mut doc, 1, rectangle, 4.0, 4.0).unwrap();
    let (hw, hh) = (info.width / 2.0, info.height / 2.0);
    // A corner is a handle and a vertex both.
    let corner = (4.0 + hw, 4.0 + hh);
    let p = (corner.0 + 0.05, corner.1 + 0.04);
    let handle = snap_point(&doc, &only(|s| s.glue_handles = true), p, Use::Connect, &[]);
    assert!(near(handle.apply(p).0, corner.0) && near(handle.apply(p).1, corner.1), "{handle:?}");
    assert_eq!(handle.glue, Some(Glue::Handle { shape: info.id }));
    let vertex = snap_point(&doc, &only(|s| s.glue_vertices = true), p, Use::Connect, &[]);
    assert!(near(vertex.apply(p).0, corner.0) && near(vertex.apply(p).1, corner.1), "{vertex:?}");
    assert_eq!(vertex.glue, Some(Glue::Vertex { shape: info.id }));
    // Handles also include the middle of a side; vertices do not, in a rectangle that has four corners.
    let top_mid = (4.0, 4.0 + hh);
    let q = (top_mid.0 + 0.03, top_mid.1 + 0.04);
    let handle = snap_point(&doc, &only(|s| s.glue_handles = true), q, Use::Connect, &[]);
    assert!(near(handle.apply(q).0, top_mid.0) && near(handle.apply(q).1, top_mid.1), "{handle:?}");
    assert!(snap_point(&doc, &only(|s| s.glue_vertices = true), q, Use::Connect, &[]).glue.is_none());
    // The outline is the nearest point on a side: 0.05 above the top side and 0.3 along it.
    let r = (4.0 + 0.3, 4.0 + hh + 0.05);
    let outline = snap_point(&doc, &only(|s| s.glue_geometry = true), r, Use::Connect, &[]);
    assert!(near(outline.apply(r).0, r.0) && near(outline.apply(r).1, 4.0 + hh), "{outline:?}");
    assert_eq!(outline.glue, Some(Glue::Outline { shape: info.id }));
    assert_eq!(outline.hits[0].target, Target::Outline);
}

#[test]
fn a_geometry_section_that_says_no_snap_is_not_a_target() {
    // 1 stops other shapes snapping to the path.
    let mut doc = plain_page(&[]);
    let rectangle = find_master("rectangle").unwrap();
    let info = drop_shape(&mut doc, 1, rectangle, 4.0, 4.0).unwrap();
    let corner = (4.0 + info.width / 2.0, 4.0 + info.height / 2.0);
    let p = (corner.0 + 0.05, corner.1 + 0.05);
    assert!(snap_point(&doc, &only(|s| s.glue_vertices = true), p, Use::Connect, &[]).glue.is_some());
    let at = doc.find_shape(diacraft_doc::Owner::Page(1), info.id).unwrap();
    let sheet = diacraft_doc::SheetRef::Shape(at);
    doc.write_cell(
        &sheet,
        &diacraft_doc::CellAddr::in_section("Geometry", Some(0), "NoSnap"),
        diacraft_doc::CellEdit::Value(diacraft_doc::Value::from_text("1")),
        diacraft_doc::WriteOrigin::Drawing,
    )
    .unwrap();
    assert!(snap_point(&doc, &only(|s| s.glue_vertices = true), p, Use::Connect, &[]).glue.is_none());
}

#[test]
fn the_intersections_of_two_outlines_are_a_target() {
    let mut doc = plain_page(&[]);
    let rectangle = find_master("rectangle").unwrap();
    let a = drop_shape(&mut doc, 1, rectangle, 4.0, 4.0).unwrap();
    let _b = drop_shape(&mut doc, 1, rectangle, 4.5, 4.4).unwrap();
    // The right side of a is at x = 4 + a.width/2, and the lower side of b at y = 4.4 - b.height/2: they cross there.
    let at = (4.0 + a.width / 2.0, 4.4 - a.height / 2.0);
    let p = (at.0 + 0.04, at.1 - 0.03);
    let out = snap_point(&doc, &only(|s| s.intersections = true), p, Use::Place, &[]);
    assert!(near(out.apply(p).0, at.0) && near(out.apply(p).1, at.1), "{out:?}");
    assert_eq!(out.hits[0].target, Target::Intersection);
    assert_eq!(out.hits[0].shape, None);
    // Off, nothing.
    assert!(snap_point(&doc, &only(|_| {}), p, Use::Place, &[]).hits.is_empty());
}

// ------------------------------------------------------------------------------------------------------------------ cost

/// A page of `n` boxes in rows, 1 inch wide and 0.5 inch high, 1.2 apart across and 0.7 apart down, 100 to a row.
fn big_page(n: u32) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "200").cell("PageHeight", "100");
    for i in 0..n {
        let (col, row) = (i % 100, i / 100);
        page = page.shape(
            ShapeBuilder::new(i + 1)
                .cell("PinX", &(0.7 + f64::from(col) * 1.2).to_string())
                .cell("PinY", &(0.5 + f64::from(row) * 0.7).to_string())
                .cell("Width", "1")
                .cell("Height", "0.5")
                .cell("LocPinX", "0.5")
                .cell("LocPinY", "0.25"),
        );
    }
    DocBuilder::new().page(1, page).build()
}

#[test]
fn a_query_costs_the_shapes_near_it_whatever_the_page_holds() {
    // Counted, not timed: the shapes a query looked at. 2,000 shapes and 12,000 shapes (past the index's bound) give the same few.
    let settings = only(|s| (s.alignment_box, s.dynamic_grid) = (true, true));
    let mut examined = Vec::new();
    for n in [2_000, 12_000] {
        let doc = big_page(n);
        let page = SnapPage::read(&doc, 1).unwrap();
        assert_eq!(page.scanned, n as usize, "the guides are found by one pass of the page, once");
        assert!(!page.shapes_read(), "nothing reads the shapes until a snap needs them");
        // Over the 30th box of the 5th row.
        let r = Rect { x0: 35.0, y0: 3.1, x1: 36.0, y1: 3.6 };
        let q = Query { subject: Subject::Box(r), using: Use::Place, moving: &[], own_points: &[], zoom: 96.0 };
        let out = snap(&doc, &page, &settings, &q);
        let index = page.shapes(&doc);
        assert!(index.read <= MAX_INDEXED, "the index read {} shapes", index.read);
        assert_eq!(index.total, n as usize);
        assert_eq!(index.truncated, n as usize > MAX_INDEXED);
        assert_eq!(out.truncated, n as usize > MAX_INDEXED);
        eprintln!(
            "a snap on a page of {n} shapes: the index read {} of {} shapes, the query looked at {} and took {} as candidates; {} top-level shapes scanned for guides",
            index.read, index.total, out.examined, out.candidates, page.scanned
        );
        // Measured 234 and 180 (the library counts the shapes of each window it reads, and alignment no longer reads one); was 4 * 64 on three windows.
        assert!(out.examined <= 4 * 64, "a query looked at {} shapes on a page of {n}", out.examined);
        // A second query reads no shape again: the index is the page's.
        let before = index.read;
        let _ = snap(&doc, &page, &settings, &q);
        assert_eq!(page.shapes(&doc).read, before);
        examined.push(out.examined);
    }
    assert!(examined[1] <= examined[0] + 64, "the cost does not grow with the page: {examined:?}");
}

#[test]
fn a_page_without_the_shape_targets_reads_no_shape_at_all() {
    let doc = big_page(3_000);
    let page = SnapPage::read(&doc, 1).unwrap();
    let q = Query { subject: Subject::Point((3.3, 3.3)), using: Use::Place, moving: &[], own_points: &[], zoom: 96.0 };
    let no_shapes = only(|s| (s.ruler, s.grid, s.guides) = (true, true, true));
    let out = snap(&doc, &page, &no_shapes, &q);
    assert!(!out.hits.is_empty());
    assert!(!page.shapes_read(), "the grid, the ruler and the guides need no shape");
    assert_eq!((out.examined, out.visited, out.points), (0, 0, 0));
    // The default settings glue to connection points, and a placing is pulled to them, so they read the shapes near the query,
    // and only those.
    let out = snap(&doc, &page, &SnapSettings::default(), &q);
    assert!(page.shapes_read());
    assert!(out.examined <= 64 && out.visited <= MAX_VISITS, "{out:?}");
}

/// What a snap says it did is what it did: finite numbers; no hit, no move; a hit on an axis puts the anchor of the subject on the line, and a hit on a place
/// puts a point on the place.
fn assert_consistent(subject: Subject, out: &Snapped) {
    assert!(out.dx.is_finite() && out.dy.is_finite(), "{subject:?} -> {out:?}");
    if out.hits.is_empty() {
        assert!(out.dx == 0.0 && out.dy == 0.0, "{subject:?} -> {out:?}");
    }
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * (1.0 + a.abs().max(b.abs()));
    for hit in &out.hits {
        assert!(hit.line.is_finite() && hit.at.is_none_or(|(x, y)| x.is_finite() && y.is_finite()), "{hit:?}");
        let Some(axis) = hit.axis else {
            // A hit on a place pulled both axes: a point is on it.
            let at = hit.at.expect("a hit with no axis names its place");
            if let Subject::Point(p) = subject {
                let (x, y) = out.apply(p);
                assert!(close(x, at.0) && close(y, at.1), "{subject:?} -> {out:?}");
            }
            continue;
        };
        let (low, high, shift, point) = match (subject, axis) {
            (Subject::Point(p), Axis::X) => (p.0, p.0, out.dx, true),
            (Subject::Point(p), Axis::Y) => (p.1, p.1, out.dy, true),
            (Subject::Box(r) | Subject::Edges(r, _), Axis::X) => (r.x0, r.x1, out.dx, false),
            (Subject::Box(r) | Subject::Edges(r, _), Axis::Y) => (r.y0, r.y1, out.dy, false),
        };
        let anchored = match hit.anchor {
            Anchor::Point if point => low,
            Anchor::Low => low,
            Anchor::Middle => (low + high) / 2.0,
            Anchor::High => high,
            Anchor::Point => panic!("a box has no point anchor: {hit:?}"),
        };
        assert!(close(anchored + shift, hit.line), "{subject:?} -> {out:?}: {hit:?} is at {} after the move", anchored + shift);
    }
}

#[test]
fn hostile_pages_and_shapes_snap_without_a_panic() {
    // Boxes at absurd places and sizes, NaN cells, a shape with no cells at all.
    let page = PageBuilder::new()
        .cell("PageWidth", "11")
        .cell("PageHeight", "8.5")
        .cell("XGridDensity", "NaN")
        .shape(ShapeBuilder::new(1).cell("PinX", "1e300").cell("PinY", "-1e300").cell("Width", "1e300").cell("Height", "1e300"))
        .shape(ShapeBuilder::new(2))
        .shape(ShapeBuilder::new(3).cell("PinX", "NaN").cell("PinY", "inf").cell("Width", "1").cell("Height", "1"))
        .shape(ShapeBuilder::new(4).cell("PinX", "5").cell("PinY", "5").cell("Width", "0").cell("Height", "0"));
    let doc = DocBuilder::new().page(1, page).build();
    let all = SnapSettings::all();
    for p in [(5.0, 5.0), (0.0, 0.0), (1.0e300, 1.0e300), (-1.0e9, 4.0), (f64::NAN, 1.0), (1.0, f64::INFINITY)] {
        for using in [Use::Connect, Use::Place] {
            let out = snap_point(&doc, &all, p, using, &[]);
            assert_consistent(Subject::Point(p), &out);
            // Whatever the page holds, the work is the cap's.
            assert!(out.visited <= MAX_GATHERS * MAX_VISITS && out.examined <= 4 * 64 && out.points <= MAX_TARGET_POINTS, "{p:?}: {out:?}");
        }
    }
    // A point on the zero-size shape of the page is where it is: it is on the grid, on the ruler, on that shape's place, on its alignment.
    let at = snap_point(&doc, &all, (5.0, 5.0), Use::Connect, &[]);
    assert!(near(at.apply((5.0, 5.0)).0, 5.0) && near(at.apply((5.0, 5.0)).1, 5.0), "{at:?}");
    // A point that is not a number snaps to nothing and is not moved.
    let nan = snap_point(&doc, &all, (f64::NAN, 1.0), Use::Place, &[]);
    assert_eq!((nan.dx, nan.dy, nan.hits.len()), (0.0, 0.0, 0), "{nan:?}");
    for r in [
        Rect { x0: 4.0, y0: 4.0, x1: 6.0, y1: 6.0 },
        Rect { x0: -1.0e300, y0: -1.0e300, x1: 1.0e300, y1: 1.0e300 },
        Rect { x0: 5.0, y0: 5.0, x1: 5.0, y1: 5.0 },
    ] {
        let out = snap_box(&doc, &all, r, &[]);
        assert_consistent(Subject::Box(r), &out);
        assert!(out.visited <= MAX_GATHERS * MAX_VISITS && out.examined <= 4 * 64, "{r:?}: {out:?}");
    }
    // The box of the zero-size shape's own place is moved by the grid and the shape under it at most: a box 2 inches wide at 4 to 6 snaps its middle to 5.
    let out = snap_box(&doc, &all, Rect { x0: 4.02, y0: 3.98, x1: 6.02, y1: 5.98 }, &[]);
    assert!(near(out.dx, -0.02) && near(out.dy, 0.02), "{out:?}");
}

// ------------------------------------------------------------------------------------------------------------------ cost, round 1 (H1, M3)

/// A master of one shape that holds `rows` rows in each of a `Geometry` section and a `Connection` section, and `n` instances of it stacked on one place:
/// the box 4.5 to 5.5 across and 3.5 to 4.5 up. Read with every row of every instance merged, 64 instances of 100,000 rows are 12.8 million rows.
pub fn instances_of_a_big_master(n: u32, rows: u32, kind: ShapeType) -> Document {
    let mut master_shape = ShapeBuilder::new(1).cell("Width", "1").cell("Height", "1").cell("LocPinX", "0.5").cell("LocPinY", "0.5");
    for i in 0..rows {
        let (row, t) = (RowKey::Ix(i), if i == 0 { "MoveTo" } else { "LineTo" });
        let v = (f64::from(i % 100) / 100.0).to_string();
        master_shape = master_shape
            .row_type("Geometry", Some(0), row.clone(), t)
            .row_cell("Geometry", Some(0), row.clone(), "X", &v)
            .row_cell("Geometry", Some(0), row.clone(), "Y", &v)
            .row_cell("Connection", None, row.clone(), "X", &v)
            .row_cell("Connection", None, row, "Y", &v);
    }
    stacked_instances_of(master_shape, n, 1.0, kind)
}

/// `n` instances of a master of the one shape `master_shape`, stacked on the place (5, 4), each `side` inches across and up.
fn stacked_instances_of(master_shape: ShapeBuilder, n: u32, side: f64, kind: ShapeType) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "11").cell("PageHeight", "8.5");
    for i in 0..n {
        page = page.shape(
            ShapeBuilder::new(100 + i)
                .master(1)
                .kind(kind.clone())
                .cell("PinX", "5")
                .cell("PinY", "4")
                .cell("Width", &side.to_string())
                .cell("Height", &side.to_string())
                .cell("LocPinX", &(side / 2.0).to_string())
                .cell("LocPinY", &(side / 2.0).to_string())
                .cell("Angle", "0"),
        );
    }
    DocBuilder::new().master(1, MasterBuilder::new().shape(master_shape)).page(1, page).build()
}

/// `NURBS(...)` of `tuples` control points along a gentle curve, degree 3.
fn nurbs_text(tuples: usize) -> String {
    let mut text = format!("NURBS({}, 3, 1, 1", tuples + 3);
    for i in 0..tuples {
        text.push_str(&format!(", {:.4}, {:.4}, {}, 1", i as f64 / tuples as f64, ((i % 9) as f64) / 20.0, i + 1));
    }
    text.push(')');
    text
}

/// `POLYLINE(...)` of `points` vertices.
fn polyline_text(points: usize) -> String {
    let mut text = String::from("POLYLINE(1, 1");
    for i in 0..points {
        text.push_str(&format!(", {:.4}, {:.4}", i as f64 / points as f64, ((i % 7) as f64) / 20.0));
    }
    text.push(')');
    text
}

/// A master shape of a `MoveTo`, a `NURBSTo` row with a constructor of `tuples` tuples and a `PolylineTo` row with one of `points` points.
fn master_of_constructors(tuples: usize, points: usize) -> ShapeBuilder {
    ShapeBuilder::new(1)
        .cell("Width", "1")
        .cell("Height", "1")
        .cell("LocPinX", "0.5")
        .cell("LocPinY", "0.5")
        .row_type("Geometry", Some(0), RowKey::Ix(0), "MoveTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(0), "X", "0")
        .row_cell("Geometry", Some(0), RowKey::Ix(0), "Y", "0")
        .row_type("Geometry", Some(0), RowKey::Ix(1), "NURBSTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "1")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "Y", "0")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "A", &(tuples + 2).to_string())
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "B", "1")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "C", "0")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "D", "1")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "E", &nurbs_text(tuples))
        .row_type("Geometry", Some(0), RowKey::Ix(2), "PolylineTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(2), "X", "0")
        .row_cell("Geometry", Some(0), RowKey::Ix(2), "Y", "1")
        .row_cell("Geometry", Some(0), RowKey::Ix(2), "A", &polyline_text(points))
}

/// Every option that reads a shape's rows on, and the ones that read its box.
fn every_shape_target() -> SnapSettings {
    only(|s| {
        s.alignment_box = true;
        s.intersections = true;
        s.glue_geometry = true;
        s.glue_handles = true;
        s.glue_vertices = true;
        s.glue_connection_points = true;
        s.snap_geometry = true;
        s.snap_handles = true;
        s.snap_vertices = true;
        s.snap_connection_points = true;
        s.dynamic_grid = true;
    })
}

#[test]
fn sixty_four_stacked_instances_of_a_master_of_100000_rows_merge_no_row_to_snap_to_them() {
    // A section's rows are counted over the shape, its master and the styles before any is merged, and a section past the cap is
    // not read. 64 instances of a master of 100,000 rows in each of two sections were 12.8 million rows for one query.
    let doc = instances_of_a_big_master(64, 100_000, ShapeType::Shape);
    diacraft_doc::work::reset();
    let page = SnapPage::read(&doc, 1).unwrap();
    let settings = every_shape_target();
    let corner = (5.5, 4.5);
    let p = (corner.0 + 0.04, corner.1 + 0.03);
    let out = snap_point(&doc, &settings, p, Use::Connect, &[]);
    // The boxes are still targets: the corner of the stack is a handle of one of them.
    assert!(near(out.apply(p).0, corner.0) && near(out.apply(p).1, corner.1), "{out:?}");
    assert!(matches!(out.glue, Some(Glue::Handle { shape }) if (100..164).contains(&shape)), "{out:?}");
    assert!(out.truncated, "a section that is not read is said: {out:?}");
    // A box placed over the stack, and a point placed: the same.
    let boxed = snap_box(&doc, &settings, Rect { x0: 5.53, y0: 3.03, x1: 6.53, y1: 4.03 }, &[]);
    let placed = snap_point(&doc, &settings, (5.5, 3.5), Use::Place, &[]);
    for (what, out) in [("connect", &out), ("box", &boxed), ("place", &placed)] {
        eprintln!(
            "{what}: examined {} visited {} points {}; rows merged so far {}",
            out.examined,
            out.visited,
            out.points,
            diacraft_doc::work::rows_merged()
        );
        // Measured 320 over the call's library queries (the engine's budgets are the call's, summed); was 4 * 64 on three windows.
        assert!(out.visited <= MAX_GATHERS * MAX_VISITS && out.examined <= 5 * 64 && out.points <= MAX_TARGET_POINTS, "{what}: {out:?}");
    }
    // Nothing was merged: the counts of the raw rows say the sections are too big, before any is.
    let merged = diacraft_doc::work::rows_merged();
    assert_eq!(merged, 0, "the snaps merged {merged} rows");
    let _ = page;
}

#[test]
fn guides_past_the_cap_that_are_instances_of_a_big_master_cost_nothing() {
    // H1: the cap comes before the read of a guide, and a guide's section of many rows is skipped before any row is merged.
    let doc = instances_of_a_big_master(MAX_GUIDES as u32 + 500, 100_000, ShapeType::Guide);
    diacraft_doc::work::reset();
    let page = SnapPage::read(&doc, 1).unwrap();
    assert_eq!(page.scanned, MAX_GUIDES + 500);
    assert_eq!(page.guides.len(), MAX_GUIDES, "the line through the pin along its angle, for a guide whose section is no guide's");
    assert_eq!(page.guides_left_out, 500, "the ones past the cap are counted and not read");
    assert_eq!(diacraft_doc::work::rows_merged(), 0);
}

#[test]
fn a_windows_read_of_connection_points_is_capped_before_any_row_is_merged() {
    // The window reads a shape's points (View > Connection Points, the Connector tool) as the snap does: a section of more rows than
    // the cap, counted over the shape and its master, is not read.
    let doc = instances_of_a_big_master(2, 300, ShapeType::Shape);
    diacraft_doc::work::reset();
    assert_eq!(connection_points_within(&doc, 1, 100, 299), None, "300 rows over a cap of 299: not read");
    assert_eq!(diacraft_doc::work::rows_merged(), 0, "and nothing merged to find that out");
    let read = connection_points_within(&doc, 1, 100, 300).expect("300 rows within a cap of 300");
    assert_eq!(read.len(), 300);
    assert_eq!(read, connection_points(&doc, 1, 100), "the same points as the read with no cap");
    assert_eq!(connection_points_within(&doc, 1, 999, 300), Some(Vec::new()), "a shape that is not there has none");
}

/// Shapes `(id, x, y, w, h)` as `page_of` takes them, then the query: a box over `(5, 4)`. → what the query did.
fn box_query_over(shapes: &[(u32, f64, f64, f64, f64)], settings: &SnapSettings) -> Snapped {
    let doc = plain_page(shapes);
    diacraft_doc::work::reset();
    let page = SnapPage::read(&doc, 1).unwrap();
    let q =
        Query { subject: Subject::Box(Rect { x0: 4.62, y0: 3.62, x1: 5.62, y1: 4.12 }), using: Use::Place, moving: &[], own_points: &[], zoom: 96.0 };
    let out = snap(&doc, &page, settings, &q);
    // A second query on the same page does not read the shapes again, and costs what the first did.
    let again = snap(&doc, &page, settings, &q);
    assert_eq!((again.dx, again.dy, again.visited, again.examined), (out.dx, out.dy, out.visited, out.examined));
    out
}

#[test]
fn the_cost_of_a_query_is_the_caps_on_a_page_of_many_shapes_over_one_spot() {
    // On the reviewer's layout: 2,500 tiny shapes and 2,500 squares 3.9 inches wide, all over one spot; and 5,000 stacked.
    let settings = every_shape_target();
    let mut tiny_and_big: Vec<(u32, f64, f64, f64, f64)> = (0..2_500).map(|i| (i + 1, 5.0, 4.0, 0.01, 0.01)).collect();
    tiny_and_big.extend((0..2_500).map(|i| (2_501 + i, 5.0, 4.0, 3.9, 3.9)));
    let stacked: Vec<(u32, f64, f64, f64, f64)> = (0..5_000).map(|i| (i + 1, 5.0, 4.0, 1.0, 1.0)).collect();
    for (what, shapes) in [("2,500 tiny and 2,500 squares", tiny_and_big), ("5,000 stacked", stacked)] {
        let out = box_query_over(&shapes, &settings);
        eprintln!(
            "{what}: visited {} examined {} candidates {} points {} truncated {}",
            out.visited, out.examined, out.candidates, out.points, out.truncated
        );
        assert!(out.visited <= MAX_GATHERS * MAX_VISITS, "{what}: visited {}", out.visited);
        // Measured 20,480 visits (five windows of 4,096) and 64 candidates, summed over the call's library queries. The alignment pulls from 13 points
        // now (it was 6), so this box, 0.12 inch past an edge, is pulled on one axis and the other is asked alone: measured 49,152 (twelve windows), at most the call's three library
        // queries of five windows each.
        assert!(out.visited <= MAX_LIBRARY_QUERIES * 5 * MAX_VISITS, "{what}: visited {}", out.visited);
        // The candidates are counted for each window gathered (at most MAX_CANDIDATES each) and summed over the call's queries: measured 64 once, and 256 (two queries of 128).
        assert!(out.examined <= out.visited && out.candidates <= MAX_GATHERS * MAX_CANDIDATES && out.points <= MAX_TARGET_POINTS, "{what}: {out:?}");
        assert!(out.truncated, "{what}: more shapes lie over the spot than a query looks into, and it says so: {out:?}");
        assert!(out.dx.is_finite() && out.dy.is_finite());
    }
}

#[test]
fn the_cap_sets_the_cut_and_a_few_shapes_over_a_spot_are_all_looked_at() {
    // Under the caps nothing is cut and the answer is complete: eight stacked boxes are all candidates, and the query says it left none out.
    let shapes: Vec<(u32, f64, f64, f64, f64)> = (0..8).map(|i| (i + 1, 5.0, 4.0, 1.0, 1.0)).collect();
    let out = box_query_over(&shapes, &every_shape_target());
    assert!(!out.truncated, "{out:?}");
    // Measured 192: the library reads a window again to check a place and for its marks, and a box is asked on each axis alone (up to three library
    // queries to a call), so eight stacked boxes are 24 entries each, not 16. The alignment pulls from 13 points now (it was 6): this box is 0.12 inch (11.5
    // points) past the edge of the boxes, so it is pulled on x and the other axis is asked alone: measured 336, 42 entries each.
    assert!(out.visited >= 8 && out.visited <= 8 * 48, "{out:?}");
}

// ------------------------------------------------------------------------------------------------------------------ a box and its points (M4)

/// A rectangle at (4, 4): its corner at the upper right, and its size.
fn rectangle_at_4_4() -> (Document, DropInfo, (f64, f64)) {
    let mut doc = plain_page(&[]);
    let rectangle = find_master("rectangle").unwrap();
    let info = drop_shape(&mut doc, 1, rectangle, 4.0, 4.0).unwrap();
    let corner = (4.0 + info.width / 2.0, 4.0 + info.height / 2.0);
    (doc, info, corner)
}

#[test]
fn a_box_placed_near_a_corner_is_pulled_to_it_by_whichever_of_its_nine_points_is_nearest() {
    // A shape being placed is pulled by the options that pull a connector's end.
    let (doc, a, corner) = rectangle_at_4_4();
    // The lower left corner of a box 1 by 0.5 is 0.04 right of and 0.03 above the corner of the rectangle.
    let r = Rect { x0: corner.0 + 0.04, y0: corner.1 + 0.03, x1: corner.0 + 1.04, y1: corner.1 + 0.53 };
    let out = snap_box(&doc, &only(|s| s.snap_vertices = true), r, &[]);
    assert!(near(out.dx, -0.04) && near(out.dy, -0.03), "{out:?}");
    assert_eq!((out.hits.len(), out.hits[0].target, out.hits[0].shape), (1, Target::Vertex, Some(a.id)), "{out:?}");
    assert!(out.hits[0].at.is_some_and(|at| near(at.0, corner.0) && near(at.1, corner.1)), "{out:?}");
    assert!(out.glue.is_none(), "a box placed is not glued");
    // The same corner from the other side: the box's upper right corner, which is the one nearest.
    let r = Rect { x0: corner.0 - 1.04, y0: corner.1 - 0.53, x1: corner.0 - 0.03, y1: corner.1 + 0.02 };
    let out = snap_box(&doc, &only(|s| s.snap_vertices = true), r, &[]);
    assert!(near(out.dx, 0.03) && near(out.dy, -0.02), "{out:?}");
    // Far from any corner is no pull.
    let r = Rect { x0: 8.0, y0: 6.0, x1: 9.0, y1: 6.5 };
    assert!(snap_box(&doc, &only(|s| s.snap_vertices = true), r, &[]).hits.is_empty());
}

#[test]
fn a_box_is_pulled_to_the_connection_points_the_handles_the_outline_and_the_crossings_of_other_shapes() {
    let (mut doc, a, corner) = rectangle_at_4_4();
    let (hw, hh) = (a.width / 2.0, a.height / 2.0);
    // The right-hand connection point: a box whose left middle is 0.05 off it.
    let r = Rect { x0: 4.0 + hw + 0.05, y0: 4.0 - 0.25 + 0.04, x1: 4.0 + hw + 1.05, y1: 4.0 + 0.25 + 0.04 };
    let out = snap_box(&doc, &only(|s| s.snap_connection_points = true), r, &[]);
    assert!(near(out.dx, -0.05) && near(out.dy, -0.04), "{out:?}");
    assert_eq!((out.hits[0].target, out.hits[0].shape, out.hits[0].point), (Target::ConnectionPoint, Some(a.id), Some(3)), "{out:?}");
    // A handle that is no vertex: the middle of the top side, which a box's bottom middle is 0.03 above.
    let r = Rect { x0: 3.5, y0: 4.0 + hh + 0.03, x1: 4.5, y1: 4.0 + hh + 0.53 };
    let out = snap_box(&doc, &only(|s| s.snap_handles = true), r, &[]);
    assert!(near(out.dx, 0.0) && near(out.dy, -0.03) && out.hits[0].target == Target::Handle, "{out:?}");
    // The outline: a box whose bottom is 0.05 above the top side, between its middle and its corner, is on the side where it was.
    let r = Rect { x0: 4.1, y0: 4.0 + hh + 0.05, x1: 4.3, y1: 4.0 + hh + 0.4 };
    let out = snap_box(&doc, &only(|s| s.snap_geometry = true), r, &[]);
    assert!(near(out.dx, 0.0) && near(out.dy, -0.05) && out.hits[0].target == Target::Outline && out.hits[0].shape == Some(a.id), "{out:?}");
    // The crossing of two outlines: the centre of a box 0.04 right of and 0.03 under it.
    let rectangle = find_master("rectangle").unwrap();
    let _b = drop_shape(&mut doc, 1, rectangle, 4.5, 4.4).unwrap();
    let at = (4.0 + hw, 4.4 - hh);
    let r = Rect { x0: at.0 + 0.04 - 0.2, y0: at.1 - 0.03 - 0.2, x1: at.0 + 0.04 + 0.2, y1: at.1 - 0.03 + 0.2 };
    let out = snap_box(&doc, &only(|s| s.intersections = true), r, &[]);
    assert!(near(out.dx, -0.04) && near(out.dy, 0.03) && out.hits[0].target == Target::Intersection, "{out:?}");
    // The corner is a vertex too, but with only handles on the box does not meet the crossing option.
    let off = snap_box(&doc, &only(|_| {}), r, &[]);
    assert!(off.hits.is_empty(), "{off:?}");
    let _ = corner;
}

#[test]
fn the_places_of_the_other_shapes_pull_a_box_only_when_the_switches_and_the_page_allow() {
    let (doc, a, corner) = rectangle_at_4_4();
    let r = Rect { x0: corner.0 + 0.04, y0: corner.1 + 0.03, x1: corner.0 + 1.04, y1: corner.1 + 0.53 };
    let vertices = only(|s| s.snap_vertices = true);
    assert!(!snap_box(&doc, &vertices, r, &[]).hits.is_empty());
    // The Snap switch off: a placing is not snapped (the Glue switch is not asked of a placing).
    assert!(snap_box(&doc, &SnapSettings { snap: false, ..vertices }, r, &[]).hits.is_empty());
    assert!(!snap_box(&doc, &SnapSettings { glue: false, ..vertices }, r, &[]).hits.is_empty());
    // The option off: nothing, whatever the others say.
    assert!(snap_box(&doc, &only(|s| s.snap_handles = true), Rect { x0: 8.0, y0: 6.0, x1: 9.0, y1: 6.5 }, &[]).hits.is_empty());
    assert!(snap_box(&doc, &only(|_| {}), r, &[]).hits.is_empty());
    // The shape that moves is no target of its own move.
    assert!(snap_box(&doc, &vertices, r, &[a.id]).hits.is_empty());
    // A resize pulls the edges it moves, and no place.
    let page = SnapPage::read(&doc, 1).unwrap();
    let edges = diacraft_engine::snap::Edges { left: true, bottom: true, right: false, top: false };
    let q = Query { subject: Subject::Edges(r, edges), using: Use::Place, moving: &[], own_points: &[], zoom: 96.0 };
    assert!(snap(&doc, &page, &vertices, &q).hits.is_empty());
    // `InhibitSnap` on the page keeps the shapes out of it, as for every other target of a shape.
    let mut inhibited = doc.clone();
    inhibited
        .write_cell(
            &diacraft_doc::SheetRef::Page(1),
            &diacraft_doc::CellAddr::direct("InhibitSnap"),
            diacraft_doc::CellEdit::Value(diacraft_doc::Value::from_text("1")),
            diacraft_doc::WriteOrigin::Drawing,
        )
        .unwrap();
    assert!(snap_box(&inhibited, &vertices, r, &[]).hits.is_empty());
}

#[test]
fn the_own_connection_points_of_a_moving_shape_are_pulled_with_its_box() {
    let (doc, a, _) = rectangle_at_4_4();
    let right = 4.0 + a.width / 2.0;
    let page = SnapPage::read(&doc, 1).unwrap();
    let settings = only(|s| s.snap_connection_points = true);
    // A box far from every shape, with a connection point of its own 0.05 right of and 0.04 above the right-hand point of the rectangle.
    let own = [(right + 0.05, 4.04)];
    let r = Rect { x0: 7.0, y0: 6.0, x1: 8.0, y1: 6.5 };
    let q = Query { subject: Subject::Box(r), using: Use::Place, moving: &[99], own_points: &own, zoom: 96.0 };
    let out = snap(&doc, &page, &settings, &q);
    assert!(near(out.dx, -0.05) && near(out.dy, -0.04), "{out:?}");
    assert_eq!((out.hits[0].target, out.hits[0].shape), (Target::ConnectionPoint, Some(a.id)), "{out:?}");
    // Without the point it is not pulled; with the option off it is not either.
    let none = Query { own_points: &[], ..q };
    assert!(snap(&doc, &page, &settings, &none).hits.is_empty());
    assert!(snap(&doc, &page, &only(|_| {}), &q).hits.is_empty());
    // More than the cap are not all weighed, and the work is the cap's.
    let many: Vec<(f64, f64)> = (0..10_000).map(|i| (right + 0.05 + f64::from(i) * 1.0e-6, 4.04)).collect();
    let out = snap(&doc, &page, &settings, &Query { own_points: &many, ..q });
    assert!(out.points <= MAX_TARGET_POINTS, "{out:?}");
}

#[test]
fn sixty_four_stacked_instances_of_a_master_with_huge_constructors_build_none_of_them() {
    // The rows are few (three) and the constructors are not: 40,000 tuples and 40,000 points, each about half a megabyte of text, read and built for each of
    // the 16 shapes a query looks into. The budget is charged before a section is parsed, from the length of its text.
    let doc = stacked_instances_of(master_of_constructors(40_000, 40_000), 64, 1.0, ShapeType::Shape);
    let settings = every_shape_target();
    diacraft_doc::work::reset();
    let p = (5.54, 4.53);
    for (what, out) in [
        ("connect", snap_point(&doc, &settings, p, Use::Connect, &[])),
        ("place", snap_point(&doc, &settings, p, Use::Place, &[])),
        ("box", snap_box(&doc, &settings, Rect { x0: 5.53, y0: 3.03, x1: 6.53, y1: 4.03 }, &[])),
    ] {
        eprintln!("{what}: {:?}, pieces {}", out.outline, out.pieces);
        assert_eq!(out.outline.bytes, 0, "{what}: no constructor was read");
        assert_eq!((out.outline.built, out.outline.elements, out.outline.curve_work), (0, 0, 0), "{what}");
        assert_eq!(out.outline.points, out.outline.rows, "{what}: only the rows were charged, no constructor");
        assert!(out.outline.left_out >= 1 && out.truncated, "{what}: the sections left out are said: {out:?}");
        assert_eq!((out.pieces, out.flattened), (0, 0), "{what}");
    }
    // The rows were few, and so were the rows merged.
    assert!(diacraft_doc::work::rows_merged() <= 3 * 16 * 3, "{}", diacraft_doc::work::rows_merged());
}

#[test]
fn the_constructors_of_the_shapes_near_are_built_to_one_budget_for_the_query() {
    // 400 tuples and 400 points: about 14 kilobytes of text and 1,200 points for each instance. 16 shapes are looked into, and the budget pays for some of
    // them: the rest are left out, whichever shapes they are, and the total is under every cap.
    let doc = stacked_instances_of(master_of_constructors(400, 400), 64, 1.0, ShapeType::Shape);
    let settings = every_shape_target();
    let out = snap_point(&doc, &settings, (5.54, 4.53), Use::Connect, &[]);
    let w = out.outline;
    eprintln!("{w:?}, pieces {}", out.pieces);
    assert!(w.built >= 1 && w.built < 16, "{w:?}");
    assert!(w.left_out >= 1 && out.truncated, "{out:?}");
    assert!(
        w.bytes <= MAX_CONSTRUCTOR_BYTES && w.points <= MAX_BUILD_POINTS && w.elements <= MAX_BUILD_ELEMENTS && w.curve_work <= MAX_BUILD_WORK,
        "{w:?}"
    );
    // And one shape is built whole when it alone fits.
    let alone = stacked_instances_of(master_of_constructors(400, 400), 1, 1.0, ShapeType::Shape);
    let w = snap_point(&alone, &settings, (5.54, 4.53), Use::Connect, &[]).outline;
    assert_eq!((w.built, w.left_out), (1, 0), "{w:?}");
    assert!(w.bytes > 10_000 && w.points > 1_000 && w.elements > 400, "{w:?}");
}

#[test]
fn a_shape_with_curves_a_trillion_inches_across_costs_no_flattening() {
    // RelCubBezTo rows are shares of Width and Height, so a shape a trillion inches across draws a curve whose control points are around 1e12: flattened
    // within a thousandth of an inch that is tens of millions of pieces. The curve is left out from the box of its control points, nothing is charged for
    // it, and the answer says so.
    let master = ShapeBuilder::new(1)
        .cell("Width", "1")
        .cell("Height", "1")
        .row_type("Geometry", Some(0), RowKey::Ix(0), "MoveTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(0), "X", "0")
        .row_cell("Geometry", Some(0), RowKey::Ix(0), "Y", "0")
        .row_type("Geometry", Some(0), RowKey::Ix(1), "RelCubBezTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "1")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "Y", "1")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "A", "1")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "B", "0")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "C", "0")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "D", "1");
    let doc = stacked_instances_of(master, 16, 1.0e12, ShapeType::Shape);
    let settings = every_shape_target();
    for p in [(5.0, 4.0), (5.04, 4.03), (1.0e11, 1.0e11)] {
        let out = snap_point(&doc, &settings, p, Use::Connect, &[]);
        eprintln!("{p:?}: pieces {} flattened {} built {} {:?}", out.pieces, out.flattened, out.outline.built, out.truncated);
        assert_eq!(out.flattened, 0, "{p:?}: no curve was estimated or flattened");
        assert_eq!(out.outline.built, 16, "{p:?}: the outlines were built, they are a few rows each");
        assert!(out.dx.is_finite() && out.dy.is_finite());
    }
    let out = snap_point(&doc, &settings, (5.04, 4.03), Use::Connect, &[]);
    assert!(out.truncated, "{out:?}");
    // The same curve at a size that is a drawing's is flattened and charged.
    let small = stacked_instances_of(
        ShapeBuilder::new(1)
            .cell("Width", "1")
            .cell("Height", "1")
            .row_type("Geometry", Some(0), RowKey::Ix(0), "MoveTo")
            .row_cell("Geometry", Some(0), RowKey::Ix(0), "X", "0")
            .row_cell("Geometry", Some(0), RowKey::Ix(0), "Y", "0")
            .row_type("Geometry", Some(0), RowKey::Ix(1), "RelCubBezTo")
            .row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "1")
            .row_cell("Geometry", Some(0), RowKey::Ix(1), "Y", "1")
            .row_cell("Geometry", Some(0), RowKey::Ix(1), "A", "1")
            .row_cell("Geometry", Some(0), RowKey::Ix(1), "B", "0")
            .row_cell("Geometry", Some(0), RowKey::Ix(1), "C", "0")
            .row_cell("Geometry", Some(0), RowKey::Ix(1), "D", "1"),
        2,
        1.0,
        ShapeType::Shape,
    );
    let out = snap_box(&small, &only(|s| s.intersections = true), Rect { x0: 5.2, y0: 3.7, x1: 6.2, y1: 4.2 }, &[]);
    assert!(out.flattened > 0 && out.pieces <= MAX_TARGET_PIECES, "{out:?}");
}

/// A master shape with `sections` `Geometry` sections of `rows` rows each (a zigzag across the box) and `connections` connection points.
fn master_at_the_caps(sections: u32, rows: u32, connections: u32) -> ShapeBuilder {
    let mut m = ShapeBuilder::new(1).cell("Width", "1").cell("Height", "1").cell("LocPinX", "0.5").cell("LocPinY", "0.5");
    for s in 0..sections {
        for i in 0..rows {
            let t = if i == 0 { "MoveTo" } else { "LineTo" };
            let x = (f64::from(i % 20) / 19.0).to_string();
            let y = (f64::from((i * 7 + s) % 23) / 22.0).to_string();
            m = m.row_type("Geometry", Some(s), RowKey::Ix(i), t).row_cell("Geometry", Some(s), RowKey::Ix(i), "X", &x).row_cell(
                "Geometry",
                Some(s),
                RowKey::Ix(i),
                "Y",
                &y,
            );
        }
    }
    for i in 0..connections {
        let v = (f64::from(i) / f64::from(connections.max(2) - 1)).to_string();
        m = m.row_cell("Connection", None, RowKey::Ix(i), "X", &v).row_cell("Connection", None, RowKey::Ix(i), "Y", &v);
    }
    m
}

#[test]
fn at_the_caps_a_query_merges_no_more_rows_than_its_budget_whatever_the_shapes_hold() {
    // Every cap was met by every shape (8 sections of 400 rows, 256 connection points), and the budget was charged after the rows
    // were merged: 64 instances merged 67,584 rows for one query. Now the rows are counted over the layers and charged before they are merged.
    let doc = stacked_instances_of(master_at_the_caps(8, 400, 256), 64, 1.0, ShapeType::Shape);
    let settings = every_shape_target();
    let budget = (MAX_BUILD_POINTS + MAX_CONNECTION_ROWS_PER_QUERY) as u64;
    for (what, subject, using) in [
        ("connect", Subject::Point((5.54, 4.53)), Use::Connect),
        ("place", Subject::Point((5.54, 4.53)), Use::Place),
        ("box", Subject::Box(Rect { x0: 5.53, y0: 3.03, x1: 6.53, y1: 4.03 }), Use::Place),
    ] {
        let page = SnapPage::read(&doc, 1).unwrap();
        diacraft_doc::work::reset();
        let q = Query { subject, using, moving: &[], own_points: &[], zoom: 96.0 };
        let out = snap(&doc, &page, &settings, &q);
        let merged = diacraft_doc::work::rows_merged();
        eprintln!("{what}: rows merged {merged} (budget {budget}), outline {:?}", out.outline);
        assert!(merged <= budget, "{what}: {merged} rows merged, the budget is {budget}");
        assert!(out.outline.rows <= MAX_BUILD_POINTS && out.outline.points <= MAX_BUILD_POINTS, "{what}: {:?}", out.outline);
        assert!(out.truncated && out.outline.left_out >= 1, "{what}: what the budget did not pay for is said: {out:?}");
        assert!(out.visited <= MAX_GATHERS * MAX_VISITS, "{what}");
    }
}

#[test]
fn a_master_of_8000_sections_is_not_listed_for_the_guides_or_the_shapes_that_inherit_it() {
    // `effective_sections` merged every `Geometry` section of every layer before the first four or eight were taken: 1,000 guides
    // of a master of 8,000 one-row sections were a second for each page read. The sections are counted over the layers first, and a shape with more than a few
    // dozen is not listed.
    let many_sections = || {
        let mut m = ShapeBuilder::new(1).cell("Width", "1").cell("Height", "1").cell("LocPinX", "0.5").cell("LocPinY", "0.5");
        for s in 0..8_000u32 {
            m = m.row_type("Geometry", Some(s), RowKey::Ix(1), "MoveTo").row_cell("Geometry", Some(s), RowKey::Ix(1), "X", "0").row_cell(
                "Geometry",
                Some(s),
                RowKey::Ix(1),
                "Y",
                "0",
            );
        }
        m
    };
    let guides = stacked_instances_of(many_sections(), MAX_GUIDES as u32, 1.0, ShapeType::Guide);
    diacraft_doc::work::reset();
    let page = SnapPage::read(&guides, 1).unwrap();
    assert_eq!(page.scanned, MAX_GUIDES);
    assert_eq!(diacraft_doc::work::sections_merged(), 0, "no section was merged");
    assert_eq!(diacraft_doc::work::rows_merged(), 0);
    assert_eq!(page.guides.len(), MAX_GUIDES, "a guide whose sections are no guide's is the line through its pin along its angle");
    // The shapes that inherit it: a query lists none of the sections.
    let shapes = stacked_instances_of(many_sections(), 16, 1.0, ShapeType::Shape);
    diacraft_doc::work::reset();
    let out = snap_point(&shapes, &every_shape_target(), (5.54, 4.53), Use::Connect, &[]);
    assert_eq!(diacraft_doc::work::sections_merged(), 0, "{out:?}");
    assert_eq!((out.outline.built, out.outline.rows), (0, 0), "{out:?}");
    assert!(out.outline.left_out >= 1 && out.truncated, "{out:?}");
    // A shape with a few sections is listed as before (the cap is of the sections a sheet holds, not a reading of them).
    let few = stacked_instances_of(master_at_the_caps(8, 3, 0), 2, 1.0, ShapeType::Shape);
    diacraft_doc::work::reset();
    let out = snap_point(&few, &every_shape_target(), (5.54, 4.53), Use::Connect, &[]);
    assert!(diacraft_doc::work::sections_merged() > 0 && out.outline.built == 2 * 8, "{:?}", out.outline);
}

#[test]
fn the_curves_near_the_pointer_are_charged_to_one_total_the_query_does_not_pass() {
    // L-a: the estimate of the pieces a call may flatten was a cap of its own, so a query overshot the query's by about a half:
    // 19,206 pieces against 16,384. The call is now given what is left of the query's.
    for (label, r) in [("r = 2900", 2900.0f64), ("r = 100", 100.0)] {
        let mut m = ShapeBuilder::new(1).cell("Width", "1").cell("Height", "1").cell("LocPinX", "0.5").cell("LocPinY", "0.5");
        for s in 0..8u32 {
            for i in 0..3u32 {
                let rr = r * (1.0 + f64::from(i) * 1.0e-4) + f64::from(s) * 0.01;
                let (cx, cy) = (1.0 - rr, 0.5 + f64::from(i) * 0.05 - f64::from(s) * 0.02);
                m = m
                    .row_type("Geometry", Some(s), RowKey::Ix(i), "Ellipse")
                    .row_cell("Geometry", Some(s), RowKey::Ix(i), "X", &cx.to_string())
                    .row_cell("Geometry", Some(s), RowKey::Ix(i), "Y", &cy.to_string())
                    .row_cell("Geometry", Some(s), RowKey::Ix(i), "A", &(cx + rr).to_string())
                    .row_cell("Geometry", Some(s), RowKey::Ix(i), "B", &cy.to_string())
                    .row_cell("Geometry", Some(s), RowKey::Ix(i), "C", &cx.to_string())
                    .row_cell("Geometry", Some(s), RowKey::Ix(i), "D", &(cy + rr * 0.7).to_string());
            }
        }
        let doc = stacked_instances_of(m, 16, 1.0, ShapeType::Shape);
        let settings = every_shape_target();
        for (what, out) in [
            ("on the curves", snap_point(&doc, &settings, (5.5, 4.0), Use::Connect, &[])),
            ("a box over them", snap_box(&doc, &settings, Rect { x0: 5.45, y0: 3.4, x1: 6.45, y1: 4.4 }, &[])),
        ] {
            eprintln!("{label} {what}: pieces {} flattened {} outline {:?}", out.pieces, out.flattened, out.outline);
            assert!(out.pieces <= MAX_TARGET_PIECES && out.flattened <= MAX_TARGET_PIECES, "{label} {what}: {out:?}");
        }
    }
}

/// Not a test of anything but a measurement for the report: the time of the one bounded read of the shapes, and of a query, on a page past the bound.
#[test]
#[ignore = "a measurement: run with --ignored --nocapture"]
fn measure_the_index_and_a_query() {
    let doc = big_page(12_000);
    let t = std::time::Instant::now();
    let page = SnapPage::read(&doc, 1).unwrap();
    let read = t.elapsed();
    let settings = only(|s| (s.alignment_box, s.dynamic_grid) = (true, true));
    let q =
        Query { subject: Subject::Box(Rect { x0: 35.0, y0: 3.1, x1: 36.0, y1: 3.6 }), using: Use::Place, moving: &[], own_points: &[], zoom: 96.0 };
    let t = std::time::Instant::now();
    let _ = snap(&doc, &page, &settings, &q);
    let first = t.elapsed();
    let t = std::time::Instant::now();
    for _ in 0..100 {
        let _ = snap(&doc, &page, &settings, &q);
    }
    let each = t.elapsed() / 100;
    eprintln!("page read {read:?}; first query (builds the index) {first:?}; each later query {each:?}");
}

/// A small generator (xorshift), so that the same run is the same run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number in `[lo, hi)`.
    fn between(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (self.next() >> 11) as f64 / (1u64 << 53) as f64 * (hi - lo)
    }

    fn flag(&mut self) -> bool {
        self.next() & 1 == 1
    }
}

#[test]
fn random_queries_on_a_page_of_real_shapes_give_finite_numbers_and_never_panic() {
    let mut doc = plain_page(&[(1, 3.0, 3.5, 2.0, 1.0)]);
    let rectangle = find_master("rectangle").unwrap();
    let ellipse = find_master("ellipse").unwrap();
    for i in 0..30 {
        let spec = if i % 3 == 0 { ellipse } else { rectangle };
        drop_shape(&mut doc, 1, spec, 1.0 + f64::from(i % 8) * 1.2, 1.0 + f64::from(i / 8) * 1.1).unwrap();
    }
    add_guide(&mut doc, 1, Axis::X, 4.2, 100).unwrap();
    add_guide(&mut doc, 1, Axis::Y, 3.3, 100).unwrap();
    let page = SnapPage::read(&doc, 1).unwrap();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..4_000 {
        let mut settings = SnapSettings::none();
        for i in 0..diacraft_engine::snap::OPTIONS.len() {
            settings.put(i, rng.flag());
        }
        // The switches are mostly on.
        settings.snap = settings.snap || rng.flag();
        settings.glue = settings.glue || rng.flag();
        let (x, y) = (rng.between(-2.0, 13.0), rng.between(-2.0, 10.0));
        let subject = match rng.next() % 3 {
            0 => Subject::Point((x, y)),
            1 => Subject::Box(Rect::of((x, y), (x + rng.between(0.0, 3.0), y + rng.between(0.0, 3.0)))),
            _ => Subject::Edges(
                Rect::of((x, y), (x + rng.between(0.0, 3.0), y + rng.between(0.0, 3.0))),
                diacraft_engine::snap::Edges { left: rng.flag(), bottom: rng.flag(), right: rng.flag(), top: rng.flag() },
            ),
        };
        let zoom = [0.001, 12.0, 96.0, 768.0, 1.0e6][(rng.next() % 5) as usize];
        let using = if rng.flag() { Use::Connect } else { Use::Place };
        let moving: Vec<u32> = (0..(rng.next() % 4)).map(|_| (rng.next() % 40) as u32).collect();
        let out = snap(&doc, &page, &settings, &Query { subject, using, moving: &moving, own_points: &[], zoom });
        // A snap moves a subject by what its hits say, and by nothing when there are none.
        assert_consistent(subject, &out);
        assert!(out.visited <= MAX_GATHERS * MAX_VISITS && out.points <= MAX_TARGET_POINTS, "{settings:?}: {out:?}");
        assert!(out.hits.len() <= 2);
        assert!(out.candidates <= diacraft_engine::snap::MAX_CANDIDATES);
    }
}
