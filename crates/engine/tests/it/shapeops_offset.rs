//! Offset: `shape.offset` through `Session::execute` as the control channel calls it. Each test is named for the vector or the rule of
//! it holds to (UNITS). Numbers are inches on a page of the default scale, where they are the cells' own.
//!
//! **How a result is read.** As in `shapeops.rs`: not through the engine's own reader of rows, but as the renderer builds each section's path from the rows, carried
//! through the shape's pose.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Row, Section, Shape, Value};
use diacraft_engine::Session;
use kurbo::{BezPath, ParamCurve, ParamCurveNearest, PathEl, Point};
use serde_json::{Value as Json, json};

use crate::shapeops::{
    Build, assert_near, assert_points, assert_unchanged, at, built, doc, error_of, flag_of, ids, line, local, near, num, page_points, pose_map, put,
    rect, row_types, section_count, session_with, top,
};
use crate::{blank_session, run, snapshot};

// ---- building and reading ---------------------------------------------------------------------------------------------------

/// A row of constants.
fn row(ix: u32, kind: &str, cells: &[(&str, f64)]) -> Arc<Row> {
    let mut row = Row { ix: Some(ix), row_type: Some(Arc::from(kind)), ..Row::default() };
    for (name, n) in cells {
        row.cells.insert(CellName::new(name), Cell::constant(Value::from_text(&format!("{n}"))));
    }
    Arc::new(row)
}

/// A shown `Geometry` section `ix` of rows, the five flags 0 unless `no_fill`.
fn section(ix: u32, rows: Vec<Arc<Row>>, no_fill: bool) -> Arc<Section> {
    let mut section = Section::new("Geometry", Some(ix));
    for flag in ["NoFill", "NoLine", "NoShow", "NoSnap", "NoQuickDrag"] {
        section.cells.insert(CellName::new(flag), Cell::constant(Value::from_text(if flag == "NoFill" && no_fill { "1" } else { "0" })));
    }
    section.rows = rows.into();
    Arc::new(section)
}

/// A shape `width` by `height` with its pin at (`x`, `y`) in the middle, and one section of these rows.
fn shape_of(id: u32, (width, height): (f64, f64), (x, y): (f64, f64), rows: Vec<Arc<Row>>, no_fill: bool) -> Shape {
    let mut shape = Build::new(id, width, height).pin(x, y).build();
    Arc::make_mut(&mut shape.sheet).sections.push(section(0, rows, no_fill));
    shape
}

/// An oval in the box (`x0`, `y0`) to (`x1`, `y1`): one `Ellipse` row.
fn oval(id: u32, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> Shape {
    let (w, h) = (x1 - x0, y1 - y0);
    let ellipse = row(1, "Ellipse", &[("X", w / 2.0), ("Y", h / 2.0), ("A", w), ("B", h / 2.0), ("C", w / 2.0), ("D", h)]);
    shape_of(id, (w, h), ((x0 + x1) / 2.0, (y0 + y1) / 2.0), vec![ellipse], false)
}

/// A circle of radius 1 centred at (`cx`, `cy`).
fn circle(id: u32, cx: f64, cy: f64) -> Shape {
    oval(id, (cx - 1.0, cy - 1.0, cx + 1.0, cy + 1.0))
}

/// The shape's vertices on the page, section by section, in order: the points the rows pass through, a closed section's start once at its end too.
fn path_points(d: &Document, id: u32, section: usize) -> Vec<(f64, f64)> {
    let map = pose_map(d, id);
    let (_, stroke, _) = built(d, id).swap_remove(section);
    stroke
        .elements()
        .iter()
        .filter_map(|el| match el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => Some(map * *p),
            PathEl::ClosePath => None,
        })
        .map(|p| (p.x, p.y))
        .collect()
}

/// A path in this order or the other way round, within a billionth.
fn assert_path(what: &str, got: &[(f64, f64)], want: &[(f64, f64)]) {
    let same = |a: &[(f64, f64)], b: &[(f64, f64)]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| near(x.0, y.0) && near(x.1, y.1));
    let mut back = want.to_vec();
    back.reverse();
    assert!(same(got, want) || same(got, &back), "{what}: got {got:?}, want {want:?} or the other way round");
}

/// The page points a shape's section draws, sampled: lines at their vertices, curves at 40 points each.
fn sampled(d: &Document, id: u32, section: usize) -> Vec<Point> {
    let map = pose_map(d, id);
    let (_, mut stroke, _) = built(d, id).swap_remove(section);
    stroke.apply_affine(map);
    let mut out = Vec::new();
    for seg in stroke.segments() {
        for k in 0..=40 {
            out.push(seg.eval(f64::from(k) / 40.0));
        }
    }
    out
}

fn page_path(d: &Document, id: u32, section: usize) -> BezPath {
    let map = pose_map(d, id);
    let (_, mut stroke, _) = built(d, id).swap_remove(section);
    stroke.apply_affine(map);
    stroke
}

fn text_of(d: &Document, id: u32) -> Option<String> {
    d.shape(&at(d, id))
        .unwrap()
        .text
        .as_ref()
        .map(|t| t.parts.iter().filter_map(|p| if let diacraft_doc::TextPart::Chars(c) = p { Some(c.to_string()) } else { None }).collect::<String>())
}

fn formatting(s: &mut Session, id: u32) -> Json {
    run(s, "shape.formatting", json!({"ids": [id]}))["shapes"][0].clone()
}

/// What the formatting of a shape says: pattern, line colour, fill.
fn look_of(s: &mut Session, id: u32) -> (Json, Json, Json) {
    let f = formatting(s, id);
    (f["pattern"].clone(), f["line"].clone(), f["fill"].clone())
}

#[test]
fn v21_learns_offset_example_gives_two_dashed_lines_a_distance_from_the_line_and_the_original_stays() {
    let mut s = session_with(vec![line(1, (3.0, 3.0), (6.0, 6.0)).text("A").build()]);
    run(&mut s, "shape.format", json!({"ids": [1], "pattern": 2, "line": "#0000FF", "weight": 2.0}));
    let result = run(&mut s, "shape.offset", json!({"ids": [1], "distance": 2.0}));
    let made = ids(&result, "ids");
    assert_eq!(made.len(), 2, "one on each side, not a single copy");
    assert!(ids(&result, "deleted").is_empty(), "the original is not deleted");
    let d = doc(&s);
    assert_eq!(top(d), [1, made[0], made[1]], "the original stays, the copies in front of it, the left one first");
    assert_path("left", &path_points(d, made[0], 0), &[(1.585_786_437_627, 4.414_213_562_373), (4.585_786_437_627, 7.414_213_562_373)]);
    assert_path("right", &path_points(d, made[1], 0), &[(4.414_213_562_373, 1.585_786_437_627), (7.414_213_562_373, 4.585_786_437_627)]);
    // The same direction as the original: from the end nearer its start.
    assert!(path_points(d, made[0], 0)[0].0 < path_points(d, made[0], 0)[1].0);
    for id in &made {
        assert!(d.shape(&at(d, *id)).unwrap().text.is_none(), "no text");
        assert_near("FillPattern", num(d, *id, "FillPattern"), 0.0);
        assert!(local(d, *id, "FillPattern").is_some(), "written here, so that a style cannot bring a fill back");
        assert_near("NoFill", flag_of(d, *id, 0, "NoFill"), 1.0);
    }
    for id in &made {
        let f = formatting(&mut s, *id);
        assert_eq!((f["pattern"].clone(), f["line"].clone()), (json!(2), json!("#0000ff")), "dashed and blue, as the original");
    }
    assert_eq!(text_of(doc(&s), 1).as_deref(), Some("A"));
}

#[test]
fn v22_offset_of_a_square_gives_a_square_outside_and_one_inside_and_only_the_outer_once_the_inner_vanishes() {
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).text("S").build()]);
    run(&mut s, "shape.format", json!({"ids": [1], "fill": "#FF0000"}));
    let made = ids(&run(&mut s, "shape.offset", json!({"ids": [1], "distance": 0.5})), "ids");
    assert_eq!(made.len(), 2);
    let d = doc(&s);
    assert_points("outer", &page_points(d, made[0]), &[(-0.5, -0.5), (2.5, -0.5), (2.5, 2.5), (-0.5, 2.5)]);
    assert_points("inner", &page_points(d, made[1]), &[(0.5, 0.5), (1.5, 0.5), (1.5, 1.5), (0.5, 1.5)]);
    for id in &made {
        assert_near("FillPattern", num(d, *id, "FillPattern"), 0.0);
        assert!(d.shape(&at(d, *id)).unwrap().text.is_none());
        assert_near("a closed section takes the fill, which the pattern then says is none", flag_of(d, *id, 0, "NoFill"), 0.0);
    }
    assert_eq!(top(d), [1, made[0], made[1]]);
    // Offset 1 gives only the outer square.
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build()]);
    let made = ids(&run(&mut s, "shape.offset", json!({"ids": [1], "distance": 1.0})), "ids");
    assert_eq!(made.len(), 1);
    assert_points("outer only", &page_points(doc(&s), made[0]), &[(-1.0, -1.0), (3.0, -1.0), (3.0, 3.0), (-1.0, 3.0)]);
    // Offset 0 and -1 are refused, and change nothing.
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build()]);
    let before = snapshot(&s);
    for bad in [0.0, -1.0] {
        let error = error_of(&mut s, "shape.offset", &json!({"ids": [1], "distance": bad}));
        assert!(error.to_string().contains("distance"), "{error}");
        assert_unchanged(&s, &before, "a distance of 0 or less");
    }
}

#[test]
fn r40_the_distance_is_needed_a_number_and_not_nearer_than_the_accuracy_of_the_result_or_farther_than_the_paths_can_be_worked() {
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build()]);
    let before = snapshot(&s);
    let missing = error_of(&mut s, "shape.offset", &json!({"ids": [1]}));
    assert!(missing.to_string().contains("distance"), "{missing}");
    for bad in [json!("two"), json!(null), json!([1])] {
        let error = error_of(&mut s, "shape.offset", &json!({"ids": [1], "distance": bad}));
        assert!(error.to_string().contains("distance"), "{error}");
    }
    let near = error_of(&mut s, "shape.offset", &json!({"ids": [1], "distance": 0.000_09}));
    assert!(near.to_string().contains("at least 0.0001"), "{near}");
    let far = error_of(&mut s, "shape.offset", &json!({"ids": [1], "distance": 6.0e13}));
    assert!(far.to_string().contains("at most"), "{far}");
    assert_unchanged(&s, &before, "refused distances");
    // The least distance is made.
    let made = ids(&run(&mut s, "shape.offset", json!({"ids": [1], "distance": 0.0001})), "ids");
    assert_eq!(made.len(), 2);
}

#[test]
fn r38_a_corner_is_mitred_so_a_rectangle_gives_rectangles_and_bevelled_past_four_times_the_distance() {
    // A thin triangle: the corner at (4, 0) is 20 degrees, so its mitre would stand 0.2 / sin 10 = 1.15 from it, past 4 times 0.2: bevelled. Its other corners are mitred.
    let a = 20f64.to_radians();
    let top = (4.0 - 4.0 * a.cos(), 4.0 * a.sin());
    let triangle = Build::new(1, 4.0, top.1).pin(2.0, top.1 / 2.0).section(&[(0.0, 0.0), (4.0, 0.0), (top.0, top.1)], true, false).build();
    let mut s = session_with(vec![triangle]);
    let made = ids(&run(&mut s, "shape.offset", json!({"ids": [1], "distance": 0.2})), "ids");
    let d = doc(&s);
    let outer = page_points(d, made[0]);
    assert_eq!(outer.len(), 4, "a triangle with one corner bevelled has four corners: {outer:?}");
    // The sharp corner's bevel has two points, each 0.2 from the corner (4, 0); the mitre tip, which would be 1.15 away, is not there.
    let near_the_corner: Vec<f64> = outer.iter().map(|p| (p.0 - 4.0).hypot(p.1)).filter(|d| *d < 2.0).collect();
    assert_eq!(near_the_corner.len(), 2, "the two ends of the bevel and no tip: {outer:?}");
    assert!(near_the_corner.iter().all(|d| (d - 0.2).abs() < 1e-6), "{near_the_corner:?}");
}

#[test]
fn r38_the_sides_of_a_curve_are_curves_a_distance_from_it_within_a_ten_thousandth() {
    let mut s = session_with(vec![circle(1, 0.0, 0.0)]);
    let made = ids(&run(&mut s, "shape.offset", json!({"ids": [1], "distance": 0.25})), "ids");
    assert_eq!(made.len(), 2);
    let d = doc(&s);
    for (id, radius) in [(made[0], 1.25), (made[1], 0.75)] {
        let kinds = row_types(d, id, 0);
        assert!(kinds[1..].iter().all(|k| k == "NURBSTo"), "a curve stays a curve: {kinds:?}");
        for p in sampled(d, id, 0) {
            assert!((p.distance(Point::ORIGIN) - radius).abs() < 1.0e-4, "{p:?} is {} from the centre, want {radius}", p.distance(Point::ORIGIN));
        }
    }
    // An open curve: its sides are 0.25 from it all along, and end on the lines through its ends square to it.
    let curve =
        shape_of(2, (4.0, 1.5), (12.0, 0.75), vec![row(1, "MoveTo", &[("X", 0.0), ("Y", 0.0)]), row(2, "NURBSTo", &[("X", 4.0), ("Y", 0.0)])], true);
    let _ = curve;
    let cubic = {
        let mut shape = Build::new(2, 4.0, 1.5).pin(12.0, 0.75).build();
        let rows = vec![
            row(1, "MoveTo", &[("X", 0.0), ("Y", 0.0)]),
            Arc::new(Row {
                ix: Some(2),
                row_type: Some(Arc::from("NURBSTo")),
                cells: [
                    ("X", Cell::constant(Value::from_text("4"))),
                    ("Y", Cell::constant(Value::from_text("0"))),
                    ("A", Cell::constant(Value::from_text("0"))),
                    ("B", Cell::constant(Value::from_text("1"))),
                    ("C", Cell::constant(Value::from_text("0"))),
                    ("D", Cell::constant(Value::from_text("1"))),
                    ("E", Cell::constant(Value::from_text("NURBS(1,3,0,0,0.25,0.8,0,1,0.75,0.8,0,1)")).with_unit("NURBS")),
                ]
                .into_iter()
                .map(|(n, c)| (CellName::new(n), c))
                .collect(),
                ..Row::default()
            }),
        ];
        Arc::make_mut(&mut shape.sheet).sections.push(section(0, rows, true));
        shape
    };
    let mut s = session_with(vec![cubic]);
    let spine = page_path(doc(&s), 2, 0);
    let made = ids(&run(&mut s, "shape.offset", json!({"ids": [2], "distance": 0.25})), "ids");
    assert_eq!(made.len(), 2);
    for id in &made {
        let side = sampled(doc(&s), *id, 0);
        let off = side.iter().map(|p| spine.segments().map(|sg| sg.nearest(*p, 1e-9).distance_sq.sqrt()).fold(f64::MAX, f64::min));
        for o in off {
            assert!((o - 0.25).abs() < 1.0e-4, "{o}");
        }
    }
}

#[test]
fn r39_each_copy_takes_the_line_format_of_its_own_original_and_the_copies_go_to_the_front_in_selection_order() {
    let mut s = session_with(vec![
        line(1, (0.0, 0.0), (4.0, 0.0)).build(),
        rect(9, (20.0, 20.0, 21.0, 21.0)).build(),
        line(2, (0.0, 5.0), (4.0, 5.0)).build(),
    ]);
    run(&mut s, "shape.format", json!({"ids": [1], "line": "#FF0000", "pattern": 2}));
    run(&mut s, "shape.format", json!({"ids": [2], "line": "#0000FF", "pattern": 1}));
    let made = ids(&run(&mut s, "shape.offset", json!({"ids": [2, 1], "distance": 0.5})), "ids");
    assert_eq!(made.len(), 4);
    assert_eq!(top(doc(&s)), [1, 9, 2, made[0], made[1], made[2], made[3]], "the originals stay; the copies of the first named come first");
    assert_eq!(look_of(&mut s, made[0]).0, json!(1));
    assert_eq!(look_of(&mut s, made[1]).1, json!("#0000ff"));
    assert_eq!(look_of(&mut s, made[2]), (json!(2), json!("#ff0000"), look_of(&mut s, made[2]).2));
    assert_eq!(look_of(&mut s, made[3]).1, json!("#ff0000"));
    // Left of 2 (running right) is above it, and the right is below.
    assert_near("above", path_points(doc(&s), made[0], 0)[0].1, 5.5);
    assert_near("below", path_points(doc(&s), made[1], 0)[0].1, 4.5);
}

#[test]
fn r38_a_path_that_crosses_itself_or_bends_so_hard_that_its_sides_meet_is_refused_in_words_and_changes_nothing() {
    let eight = Build::new(1, 2.0, 2.0).pin(1.0, 1.0).section(&[(0.0, 0.0), (2.0, 2.0), (2.0, 0.0), (0.0, 2.0)], false, true).build();
    let hairpin = Build::new(2, 4.0, 0.3).pin(12.0, 0.15).section(&[(0.0, 0.0), (4.0, 0.0), (4.0, 0.3), (0.0, 0.3)], false, true).build();
    let mut s = session_with(vec![eight, hairpin]);
    let before = snapshot(&s);
    let e = error_of(&mut s, "shape.offset", &json!({"ids": [1], "distance": 0.1}));
    assert!(e.to_string().contains("cannot make the two sides"), "{e}");
    let e = error_of(&mut s, "shape.offset", &json!({"ids": [2], "distance": 0.5}));
    assert!(e.to_string().contains("cannot make the two sides"), "{e}");
    assert_unchanged(&s, &before, "refused offsets");
}

#[test]
fn r38_a_ring_with_a_hole_keeps_the_hole_whichever_way_it_was_drawn() {
    // One shape, two sections of one fill: a big square and a hole, both drawn the same way round (the odd-crossings rule makes the hole).
    let mut s = session_with(vec![
        Build::new(1, 6.0, 6.0)
            .pin(3.0, 3.0)
            .section(&[(0.0, 0.0), (6.0, 0.0), (6.0, 6.0), (0.0, 6.0)], true, false)
            .section(&[(2.0, 2.0), (4.0, 2.0), (4.0, 4.0), (2.0, 4.0)], true, false)
            .build(),
    ]);
    let made = ids(&run(&mut s, "shape.offset", json!({"ids": [1], "distance": 0.25})), "ids");
    // Each section is a path of its own: two paths, two shapes each.
    assert_eq!(made.len(), 4);
    // The hole of a single section's region: one section that is a ring and its hole drawn as two runs.
    let mut hole_shape = Build::new(2, 6.0, 6.0).pin(13.0, 3.0).build();
    let rows = vec![
        row(1, "MoveTo", &[("X", 0.0), ("Y", 0.0)]),
        row(2, "LineTo", &[("X", 6.0), ("Y", 0.0)]),
        row(3, "LineTo", &[("X", 6.0), ("Y", 6.0)]),
        row(4, "LineTo", &[("X", 0.0), ("Y", 6.0)]),
        row(5, "LineTo", &[("X", 0.0), ("Y", 0.0)]),
        row(6, "MoveTo", &[("X", 2.0), ("Y", 2.0)]),
        row(7, "LineTo", &[("X", 4.0), ("Y", 2.0)]),
        row(8, "LineTo", &[("X", 4.0), ("Y", 4.0)]),
        row(9, "LineTo", &[("X", 2.0), ("Y", 4.0)]),
        row(10, "LineTo", &[("X", 2.0), ("Y", 2.0)]),
    ];
    Arc::make_mut(&mut hole_shape.sheet).sections.push(section(0, rows, false));
    let mut s = session_with(vec![hole_shape]);
    let made = ids(&run(&mut s, "shape.offset", json!({"ids": [2], "distance": 0.25})), "ids");
    assert_eq!(made.len(), 2);
    let d = doc(&s);
    assert_eq!(section_count(d, made[0]), 2, "the outer ring grown and the hole shrunk: one fill's boundary");
    let hole = built(d, made[0]).iter().map(|(_, stroke, _)| kurbo::Shape::bounding_box(stroke)).map(|b| b.width()).fold(f64::MAX, f64::min);
    assert_near("the hole is 2 across less 0.25 each side", hole, 1.5);
}

#[test]
fn e5_a_turned_operand_is_offset_as_it_is_drawn_and_the_copies_are_unturned() {
    let mut s = session_with(vec![Build::new(1, 2.0, 1.0).pin(5.0, 5.0).angle_degrees(90.0).section(&[(0.0, 0.0), (2.0, 0.0)], false, true).build()]);
    // The line is vertical on the page from (5.5, 4) to (5.5, 6) (the box is turned about its pin and the line is the box's lower edge): its sides are 0.5 left and
    // right of it.
    let made = ids(&run(&mut s, "shape.offset", json!({"ids": [1], "distance": 0.5})), "ids");
    let d = doc(&s);
    assert_path("left of the way it runs (up): the west side", &path_points(d, made[0], 0), &[(5.0, 4.0), (5.0, 6.0)]);
    assert_path("right: the east side", &path_points(d, made[1], 0), &[(6.0, 4.0), (6.0, 6.0)]);
    for id in &made {
        assert_near("Angle", num(d, *id, "Angle"), 0.0);
    }
}

// ---- Fit Curve -----------------------------------------------------------------------------------------------------

#[test]
fn r36_offset_makes_the_two_sides_of_each_path_of_a_shape_that_has_several_and_each_section_is_its_own_pair() {
    let mut s = session_with(vec![
        Build::new(1, 6.0, 3.0).pin(3.0, 1.5).section(&[(0.0, 0.0), (2.0, 0.0)], false, true).section(&[(4.0, 0.0), (6.0, 3.0)], false, true).build(),
    ]);
    let made = ids(&run(&mut s, "shape.offset", json!({"ids": [1], "distance": 0.5})), "ids");
    assert_eq!(made.len(), 4, "a pair for each section");
    let d = doc(&s);
    assert_path("first section, left", &path_points(d, made[0], 0), &[(0.0, 0.5), (2.0, 0.5)]);
    assert_path("first section, right", &path_points(d, made[1], 0), &[(0.0, -0.5), (2.0, -0.5)]);
}

#[test]
fn r38_the_sides_of_an_open_path_with_a_bend_are_mitred_at_it_and_each_side_is_one_path() {
    // An L: (0,0) to (4,0) to (4,3). Left of the way it runs is inside the bend and right is outside: the outer side has the mitre.
    let mut s = session_with(vec![Build::new(1, 4.0, 3.0).pin(2.0, 1.5).section(&[(0.0, 0.0), (4.0, 0.0), (4.0, 3.0)], false, true).build()]);
    let made = ids(&run(&mut s, "shape.offset", json!({"ids": [1], "distance": 0.5})), "ids");
    assert_eq!(made.len(), 2);
    let d = doc(&s);
    assert_path("inside the bend", &path_points(d, made[0], 0), &[(0.0, 0.5), (3.5, 0.5), (3.5, 3.0)]);
    assert_path("outside it, mitred", &path_points(d, made[1], 0), &[(0.0, -0.5), (4.5, -0.5), (4.5, 3.0)]);
}

#[test]
fn unit_1_the_distance_is_the_cells_own_units_and_is_not_converted_by_the_drawing_scale() {
    // A quarter-inch scale: 48 drawing units to a paper inch. The argument is in the cells' own units, which are drawing units on a scaled page, so a distance of 12
    // grows a rectangle 96 by 48 by 12 each side, and a distance of 0.25 by a quarter of a drawing unit (not by a quarter of a paper inch, 12 units).
    let mut s = blank_session();
    run(&mut s, "page.scale", json!({"preset": "arch_1_4"}));
    put(&mut s, vec![rect(1, (0.0, 0.0, 96.0, 48.0)).build()]);
    let made = ids(&run(&mut s, "shape.offset", json!({"ids": [1], "distance": 12.0})), "ids");
    assert_points("the outer side", &page_points(doc(&s), made[0]), &[(-12.0, -12.0), (108.0, -12.0), (108.0, 60.0), (-12.0, 60.0)]);
    assert_points("the inner side", &page_points(doc(&s), made[1]), &[(12.0, 12.0), (84.0, 12.0), (84.0, 36.0), (12.0, 36.0)]);
    let made = ids(&run(&mut s, "shape.offset", json!({"ids": [1], "distance": 0.25})), "ids");
    assert_points("a quarter of a unit", &page_points(doc(&s), made[0]), &[(-0.25, -0.25), (96.25, -0.25), (96.25, 48.25), (-0.25, 48.25)]);
}

#[test]
fn e7_a_copy_has_no_text_whatever_the_original_holds_and_the_original_keeps_it() {
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).text("A").build()]);
    for id in ids(&run(&mut s, "shape.offset", json!({"ids": [1], "distance": 0.25})), "ids") {
        assert!(doc(&s).shape(&at(doc(&s), id)).unwrap().text.is_none(), "a copy has no text");
    }
    assert_eq!(text_of(doc(&s), 1).as_deref(), Some("A"), "the original keeps it");
}
