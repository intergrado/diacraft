//! Turned shapes: `shape.place` and `snap.at` use the box `shape.move` uses, and a turned shape far from the
//! origin is still turned (review L7).

use diacraft_doc::{Document, RowKey};
use diacraft_engine::draw::{Turn, connection_points, drop_shape, rotate_shapes};
use diacraft_engine::snap::{Axis, Rect, SnapPage, Target, bounds_of, box_of};
use diacraft_engine::stencil::find_master;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::json;

use super::snap_review_fixes::{ask, near, page_of, session_of, vector_settings};
use crate::run;

/// An ellipse (the stencil's) dropped with its pin at (5, 5) and turned `degrees`; → the document and the ellipse's `ID`, width and height.
fn turned_ellipse(degrees: f64) -> (Document, u32, f64, f64) {
    let mut doc = page_of(&[]);
    let info = drop_shape(&mut doc, 1, find_master("ellipse").unwrap(), 5.0, 5.0).unwrap();
    rotate_shapes(&mut doc, 1, &[info.id], Turn::To(degrees.to_radians())).unwrap();
    (doc, info.id, info.width, info.height)
}

/// The half height of the box round an ellipse of `w` by `h` turned `degrees`.
fn ellipse_half_height(w: f64, h: f64, degrees: f64) -> f64 {
    let (s, c) = degrees.to_radians().sin_cos();
    (w * w / 4.0 * s * s + h * h / 4.0 * c * c).sqrt()
}

// ---------------------------------------------------------------------------------------------------------------- shape.place and snap.at align a turned shape by its path box

#[test]
fn shape_place_snaps_a_turned_shape_by_the_box_round_its_paths_as_shape_move_does() {
    // The ellipse of 2 by 1 turned 30 degrees at (5, 5): its curve's top is 0.6614 above the pin, and the box round its corners 0.933. A rectangle's top is 0.0286 above the
    // curve's. Placed where it is, with `snap`, the ellipse's curve comes to the rectangle's top (`shape.move` does, in `r44_v23_a_box_lines_up...`); by its corner box nothing is
    // near enough and it stays.
    let (mut doc, ellipse, w, h) = turned_ellipse(30.0);
    let top = 5.0 + ellipse_half_height(w, h, 30.0);
    let rect = drop_shape(&mut doc, 1, find_master("rectangle").unwrap(), 12.0, top + 0.0286 - 0.5).unwrap();
    let rect_top = box_of(&doc, 1, &[rect.id]).unwrap().y1;
    let mut s = session_of(doc);
    let r = run(&mut s, "shape.place", json!({"ids": [ellipse], "y": 5.0, "snap": {"zoom": 96.0}}));
    let b = bounds_of(s.document().unwrap(), 1, &[ellipse]).unwrap();
    assert!((b.rect.y1 - rect_top).abs() < 0.004, "the curve's top comes to the rectangle's: {} against {rect_top}: {r}", b.rect.y1);
    assert_eq!(r["moved"], json!(1), "{r}");
    // The result says how many of the shapes kept the box round their corners where the box round their paths was meant: none.
    assert_eq!(r["snap"]["corner_boxes"], json!(0), "{r}");
}

#[test]
fn snap_at_shifts_the_own_points_of_a_turned_shape_by_the_move_of_its_path_box() {
    // `snap.at` with `ids` carries the shapes' connection points with the box the caller gives, by how far the box is from the box of the shapes now. For a turned shape that is
    // the box round its paths (the box `shape.move` and the window's preview move), and not the box round its corners, which is 0.21 and 0.27 inch out for the 2 by 1 ellipse at
    // 30 degrees: the points were carried that far off, and a point 0.02 inch from another shape's was no hit.
    let (mut doc, ellipse, ..) = turned_ellipse(30.0);
    let target = drop_shape(&mut doc, 1, find_master("rectangle").unwrap(), 11.0, 9.0).unwrap();
    let own = connection_points(&doc, 1, ellipse).into_iter().find(|p| p.index == 0).expect("the ellipse's first connection point");
    let t_left = connection_points(&doc, 1, target.id)
        .into_iter()
        .find(|p| (p.x - (11.0 - target.width / 2.0)).abs() < 1e-9 && (p.y - 9.0).abs() < 1e-9)
        .expect("the rectangle's left middle");
    // The box round the ellipse's paths, moved so that its first point is 0.02 inch right of and 0.01 inch above the rectangle's.
    let (by_x, by_y) = (t_left.x + 0.02 - own.x, t_left.y + 0.01 - own.y);
    let at = bounds_of(&doc, 1, &[ellipse]).unwrap().rect;
    let mut s = session_of(doc);
    run(
        &mut s,
        "snap.set",
        json!({"snap_connection_points": true, "dynamic_grid": false, "alignment_box": false, "snap_geometry": false, "snap_vertices": false, "snap_handles": false, "intersections": false, "guides": false}),
    );
    let r = run(
        &mut s,
        "snap.at",
        json!({"page": 1, "left": at.x0 + by_x, "bottom": at.y0 + by_y, "right": at.x1 + by_x, "top": at.y1 + by_y, "ids": [ellipse], "zoom": 96.0}),
    );
    let hit = r["hits"].as_array().unwrap().iter().find(|h| h["target"] == json!("connection_point")).unwrap_or_else(|| panic!("{r}")).clone();
    assert_eq!((hit["shape"].clone(), hit["point"].clone()), (json!(target.id), json!(t_left.index)), "{r}");
    assert!(near(r["dx"].as_f64().unwrap(), -0.02) && near(r["dy"].as_f64().unwrap(), -0.01), "{r}");
}

// ---------------------------------------------------------------------------------------------------------------- L7, a turned shape far from the origin is turned

/// The box of the polygon `points` (the shape's own frame, inches) of a shape with `loc` for its pin's place in its frame, its pin at `pin` on the page, turned
/// `degrees`: the level and upright box round the turned points, by plain arithmetic on offsets from the pin.
fn box_round(points: &[(f64, f64)], loc: (f64, f64), degrees: f64) -> (f64, f64, f64, f64) {
    let (s, c) = degrees.to_radians().sin_cos();
    let page: Vec<(f64, f64)> = points.iter().map(|(x, y)| ((x - loc.0) * c - (y - loc.1) * s, (x - loc.0) * s + (y - loc.1) * c)).collect();
    let (xs, ys): (Vec<f64>, Vec<f64>) = page.into_iter().unzip();
    let low = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
    let high = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (low(&xs), low(&ys), high(&xs), high(&ys))
}

const DIAMOND: [(f64, f64); 5] = [(1.0, 0.0), (2.0, 0.5), (1.0, 1.0), (0.0, 0.5), (1.0, 0.0)];

/// A diamond that fills a 2 by 1 inch frame (its pin at the middle of the frame) at `pin`, turned `degrees`.
fn diamond(id: u32, pin: (f64, f64), degrees: f64) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new(id)
        .cell("PinX", &pin.0.to_string())
        .cell("PinY", &pin.1.to_string())
        .cell("Width", "2")
        .cell("Height", "1")
        .cell("LocPinX", "1")
        .cell("LocPinY", "0.5")
        .cell("Angle", &degrees.to_radians().to_string());
    for (i, (x, y)) in DIAMOND.iter().enumerate() {
        let row = RowKey::Ix(u32::try_from(i).unwrap());
        let kind = if i == 0 { "MoveTo" } else { "LineTo" };
        shape = shape.row_type("Geometry", Some(0), row.clone(), kind).row_cell("Geometry", Some(0), row.clone(), "X", &x.to_string()).row_cell(
            "Geometry",
            Some(0),
            row,
            "Y",
            &y.to_string(),
        );
    }
    shape
}

fn page_of_shapes(shapes: Vec<ShapeBuilder>) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20");
    for shape in shapes {
        page = page.shape(shape);
    }
    DocBuilder::new().page(1, page).build()
}

/// The box round a shape's paths, as the four numbers of its sides.
type Sides = (f64, f64, f64, f64);

/// What the page's index made of shape 1: the box round its paths, and how many shapes the page counts by their paths and by their corners.
fn paths_of(doc: &Document) -> (Option<Sides>, usize, usize) {
    let page = SnapPage::read(doc, 1).unwrap();
    let index = page.shapes(doc);
    (index.find(1).unwrap().path_box.map(|b| (b.x0, b.y0, b.x1, b.y1)), index.bounds_by_paths, index.bounds_by_corners)
}

const FAR: [f64; 8] = [0.0, 1.0e4, 1.0e5, 1.0e6, 1.0e7, 1.0e8, 1.0e9, 1.0e10];

#[test]
fn a_turned_shape_far_from_the_origin_has_the_box_round_its_paths_as_it_has_at_the_origin() {
    // The 2 by 1 diamond turned 30, 135 and -60 degrees with its pin at (off + 5.3, off + 5.3): the box round its paths is the same size as at the origin, to the rounding of
    // the numbers at that distance (at 1e8 a step is 1.5e-8 inch). Before, the shape's sides were held to a billionth of their own length and a turned shape at 1e8 was no
    // longer square to itself: no box round its paths, and no count.
    for degrees in [30.0, 135.0, -60.0] {
        let want = box_round(&DIAMOND, (1.0, 0.5), degrees);
        for off in FAR {
            let doc = page_of_shapes(vec![diamond(1, (off + 5.3, off + 5.3), degrees)]);
            let (path_box, by_paths, by_corners) = paths_of(&doc);
            let b = path_box
                .unwrap_or_else(|| panic!("{degrees} degrees at {off:e}: no box round its paths ({by_paths} by paths, {by_corners} by corners)"));
            let slack = 1.0e-6 + 8.0 * f64::EPSILON * off;
            for (got, want) in [(b.0 - off - 5.3, want.0), (b.1 - off - 5.3, want.1), (b.2 - off - 5.3, want.2), (b.3 - off - 5.3, want.3)] {
                assert!((got - want).abs() < slack, "{degrees} degrees at {off:e}: {b:?} against {want:?}");
            }
            assert_eq!((by_paths, by_corners), (1, 0), "{degrees} degrees at {off:e}");
        }
    }
}

#[test]
fn a_shape_that_is_not_turned_is_not_turned_far_from_the_origin_either() {
    // Quarter turns, whatever the rounding at the distance, are not turned: no box round the paths and nothing counted, at every distance.
    for degrees in [0.0, 90.0, 180.0, 270.0, -90.0, 360.0] {
        for off in FAR {
            let doc = page_of_shapes(vec![diamond(1, (off + 5.3, off + 5.3), degrees)]);
            assert_eq!(paths_of(&doc), (None, 0, 0), "{degrees} degrees at {off:e}");
        }
    }
}

#[test]
fn a_box_lines_up_with_the_top_of_a_turned_shape_far_from_the_origin_as_it_does_near_it() {
    // A box whose top is 0.03 inch above the diamond's top (the box round its paths) comes down onto it, at every distance: the same pull as at the origin, to the rounding.
    for off in FAR {
        let doc = page_of_shapes(vec![diamond(1, (off + 5.3, off + 5.3), 30.0)]);
        let (_, _, _, hy) = box_round(&DIAMOND, (1.0, 0.5), 30.0);
        let r = Rect { x0: off + 8.0, y0: off + 5.3 + hy + 0.03 - 3.0, x1: off + 9.0, y1: off + 5.3 + hy + 0.03 };
        let out = ask(&doc, &vector_settings(), r, &[], 96.0);
        let slack = 1.0e-6 + 16.0 * f64::EPSILON * off;
        assert!((out.dy + 0.03).abs() < slack, "at {off:e}: dy {} (not {}): {out:?}", out.dy, -0.03);
        assert!(out.hits.iter().any(|h| h.target == Target::Alignment && h.axis == Some(Axis::Y)), "at {off:e}: {out:?}");
    }
}

#[test]
fn a_turned_shape_far_out_is_one_that_a_move_and_a_snap_see_alike() {
    // The same page far and near gives the same bounds of the moving shape (`bounds_of`), to the rounding at the distance.
    for off in [1.0e6, 1.0e8] {
        let doc = page_of_shapes(vec![diamond(1, (off + 5.3, off + 5.3), 30.0)]);
        let near_doc = page_of_shapes(vec![diamond(1, (5.3, 5.3), 30.0)]);
        let (far, close) = (bounds_of(&doc, 1, &[1]).unwrap(), bounds_of(&near_doc, 1, &[1]).unwrap());
        assert_eq!((far.by_corners, close.by_corners), (0, 0));
        let slack = 1.0e-6 + 8.0 * f64::EPSILON * off;
        assert!(((far.rect.x1 - far.rect.x0) - (close.rect.x1 - close.rect.x0)).abs() < slack, "{far:?} {close:?}");
        assert!(((far.rect.y1 - far.rect.y0) - (close.rect.y1 - close.rect.y0)).abs() < slack, "{far:?} {close:?}");
        // The box round the corners is bigger than the one round the paths: the shape was recognised as turned.
        let corners = box_of(&doc, 1, &[1]).unwrap();
        assert!(corners.x1 - corners.x0 > far.rect.x1 - far.rect.x0 + 0.1 - slack, "{corners:?} {far:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------- the page budget's counts, said and kept

/// A group 4 by 2 inches at (5, 5) turned 30 degrees, with `members` members that each draw a rectangle.
fn group_of(members: u32) -> ShapeBuilder {
    let mut g = ShapeBuilder::new(1)
        .group()
        .cell("PinX", "5")
        .cell("PinY", "5")
        .cell("Width", "4")
        .cell("Height", "2")
        .cell("LocPinX", "2")
        .cell("LocPinY", "1")
        .cell("Angle", &30.0_f64.to_radians().to_string());
    for i in 0..members {
        let mut m = ShapeBuilder::new(2 + i)
            .cell("PinX", "2")
            .cell("PinY", "1")
            .cell("Width", "1")
            .cell("Height", "0.5")
            .cell("LocPinX", "0.5")
            .cell("LocPinY", "0.25");
        for (k, (x, y)) in [(0.0, 0.0), (1.0, 0.0), (1.0, 0.5), (0.0, 0.5), (0.0, 0.0)].iter().enumerate() {
            let row = RowKey::Ix(u32::try_from(k).unwrap());
            let kind = if k == 0 { "MoveTo" } else { "LineTo" };
            m = m.row_type("Geometry", Some(0), row.clone(), kind).row_cell("Geometry", Some(0), row.clone(), "X", &x.to_string()).row_cell(
                "Geometry",
                Some(0),
                row,
                "Y",
                &y.to_string(),
            );
        }
        g = g.child(m);
    }
    g
}

#[test]
fn a_turned_group_of_more_shapes_than_the_cap_is_a_cut_of_the_page_and_the_result_says_so() {
    let cap = u32::try_from(diacraft_engine::snap::MAX_BOX_MEMBERS).unwrap();
    // The group and 255 members are 256 shapes: the cap, and paid for. One member more is a cut, counted by the page and told by the result.
    for (members, cut) in [(cap - 1, false), (cap, true)] {
        let doc = page_of_shapes(vec![group_of(members)]);
        let page = SnapPage::read(&doc, 1).unwrap();
        let index = page.shapes(&doc);
        assert_eq!(
            (index.bounds_cut, index.bounds_by_corners, index.bounds_by_paths),
            (usize::from(cut), usize::from(cut), usize::from(!cut)),
            "{members} members"
        );
        let out = ask(&doc, &vector_settings(), Rect { x0: 12.0, y0: 5.0, x1: 13.0, y1: 6.0 }, &[], 96.0);
        assert_eq!(out.truncated, cut, "{members} members: {out:?}");
    }
}

#[test]
fn a_target_the_budget_could_not_pay_for_is_told_by_every_query_of_a_drag_not_only_the_first() {
    // The targets are kept on the page for a drag: the second query is told from the kept set, with its count.
    use diacraft_engine::snap::{Query, Subject, Use, snap};
    let (doc, _) = super::snap_review_fixes::budget_spent_by(512);
    let page = SnapPage::read(&doc, 1).unwrap();
    let query = Query {
        subject: Subject::Box(Rect { x0: 202.0, y0: 199.0, x1: 203.0, y1: 200.0 }),
        using: Use::Place,
        moving: &[],
        zoom: 96.0,
        own_points: &[],
    };
    for drag in 0..3 {
        assert!(snap(&doc, &page, &vector_settings(), &query).truncated, "the {drag}th query of one drag");
    }
}

#[test]
fn shape_place_tells_how_many_shapes_it_aligned_by_their_corners_where_their_paths_were_meant() {
    // A turned shape that draws no path keeps the box round its corners, which `shape.move` counts in `snap.corner_boxes`: so does `shape.place`.
    let doc = page_of_shapes(vec![
        ShapeBuilder::new(1)
            .cell("PinX", "5")
            .cell("PinY", "5")
            .cell("Width", "2")
            .cell("Height", "1")
            .cell("LocPinX", "1")
            .cell("LocPinY", "0.5")
            .cell("Angle", "0.5"),
    ]);
    let mut s = session_of(doc);
    let r = run(&mut s, "shape.place", json!({"ids": [1], "x": 6.0, "snap": {"zoom": 96.0}}));
    assert_eq!(r["snap"]["corner_boxes"], json!(1), "{r}");
    let r = run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.5, "dy": 0.0, "snap": {"zoom": 96.0}}));
    assert_eq!(r["snap"]["corner_boxes"], json!(1), "{r}");
}
