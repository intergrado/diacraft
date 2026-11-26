//! The edges of a shape's outline and the connection points on each, as `shape.connection_edges` tells them (design §1, §2, §3.8, §8.2, §9 of
//! the connection-point doubling design). A query: it changes nothing and is no undo step.
//!
//! Numbers are paper inches on pages of the default scale unless a test sets another. Shapes the tests make by hand sit at the page's origin with their local pin at
//! (0, 0), so a place in the shape's frame is the same place on the page.

use diacraft_doc::{Cell, CellAddr, Document, RowKey, ShapeType, Value as CellValue};
use diacraft_engine::Session;
use diacraft_engine::draw::conn_points::{DEFAULT_MIN_GAP, MAX_ARC_CALLS, MAX_EDGES_PER_SHAPE, MAX_OUTLINE_STEPS, MAX_PATH_POINTS, query_edges};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::{Value, json};

use crate::edit::{doc, drop_shape, id_of};
use crate::{blank_session, check_session, run, snapshot};

// ---- builders ----------------------------------------------------------------------------------------------------------------------

fn session_with(built: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = built;
        Ok(())
    })
    .unwrap();
    s
}

fn page_of(shapes: Vec<ShapeBuilder>) -> Document {
    let mut page = PageBuilder::new();
    for shape in shapes {
        page = page.shape(shape);
    }
    DocBuilder::new().page(1, page).build()
}

/// A shape `w` by `h` at the page's origin with its local pin at (0, 0), and nothing drawn.
fn base(id: u32, w: f64, h: f64) -> ShapeBuilder {
    ShapeBuilder::new(id)
        .cell("PinX", "0")
        .cell("PinY", "0")
        .cell("Width", &w.to_string())
        .cell("Height", &h.to_string())
        .cell("LocPinX", "0")
        .cell("LocPinY", "0")
}

/// A row of `Geometry` section `section`: its type and its cells.
fn row(s: ShapeBuilder, section: u32, ix: u32, kind: &str, cells: &[(&str, &str)]) -> ShapeBuilder {
    let key = RowKey::Ix(ix);
    let mut s = s.row_type("Geometry", Some(section), key.clone(), kind);
    for (name, text) in cells {
        s = s.row_cell("Geometry", Some(section), key.clone(), name, text);
    }
    s
}

fn xy(x: f64, y: f64) -> [(&'static str, String); 2] {
    [("X", x.to_string()), ("Y", y.to_string())]
}

fn at(s: ShapeBuilder, section: u32, ix: u32, kind: &str, x: f64, y: f64) -> ShapeBuilder {
    let cells = xy(x, y);
    row(s, section, ix, kind, &[("X", &cells[0].1), ("Y", &cells[1].1)])
}

/// A closed polygon: a move to the first vertex, a line to each other one, and a line back to the first. The rows are numbered from 1, as a master's are.
fn polygon(mut s: ShapeBuilder, section: u32, vertices: &[(f64, f64)]) -> ShapeBuilder {
    for (i, (x, y)) in vertices.iter().chain(vertices.first()).enumerate() {
        s = at(s, section, u32::try_from(i + 1).unwrap(), if i == 0 { "MoveTo" } else { "LineTo" }, *x, *y);
    }
    s
}

/// An arc of a circle from the pen to (`x`, `y`) through (`mx`, `my`): an `EllipticalArcTo` of ratio 1.
fn arc(s: ShapeBuilder, section: u32, ix: u32, (x, y): (f64, f64), (mx, my): (f64, f64)) -> ShapeBuilder {
    let (x, y, mx, my) = (x.to_string(), y.to_string(), mx.to_string(), my.to_string());
    row(s, section, ix, "EllipticalArcTo", &[("X", &x), ("Y", &y), ("A", &mx), ("B", &my), ("C", "0"), ("D", "1")])
}

/// The rectangle `w` by `h` as one section of a shape of that size.
fn rectangle(id: u32, w: f64, h: f64) -> ShapeBuilder {
    polygon(base(id, w, h), 0, &[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)])
}

/// An ordinary inward connection point at (`x`, `y`) in the shape's frame, as row `ix`.
fn point(s: ShapeBuilder, ix: u32, x: f64, y: f64) -> ShapeBuilder {
    let key = RowKey::Ix(ix);
    let (x, y) = (x.to_string(), y.to_string());
    let s = s.row_cell("Connection", None, key.clone(), "X", &x).row_cell("Connection", None, key.clone(), "Y", &y);
    s.row_cell("Connection", None, key.clone(), "DirX", "0").row_cell("Connection", None, key.clone(), "DirY", "0").row_cell(
        "Connection",
        None,
        key,
        "Type",
        "0",
    )
}

fn hidden(s: ShapeBuilder, section: u32) -> ShapeBuilder {
    s.set(CellAddr::in_section("Geometry", Some(section), "NoShow"), Cell::constant(CellValue::from_text("1")))
}

// ---- reading the answer ------------------------------------------------------------------------------------------------------------

fn query(s: &mut Session, params: Value) -> Value {
    run(s, "shape.connection_edges", params)
}

fn shape_answer(s: &mut Session, id: u32) -> Value {
    query(s, json!({"id": id}))["shapes"][0].clone()
}

fn edges_of(s: &mut Session, id: u32) -> Vec<Value> {
    shape_answer(s, id)["edges"].as_array().unwrap().clone()
}

fn kinds(edges: &[Value]) -> Vec<&str> {
    edges.iter().map(|e| e["kind"].as_str().unwrap()).collect()
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

fn assert_near(what: &str, got: f64, want: f64) {
    assert!(near(got, want), "{what}: got {got}, want {want}");
}

fn place(v: &Value) -> (f64, f64) {
    (f(&v["x"]), f(&v["y"]))
}

fn points_of(edge: &Value) -> Vec<u64> {
    edge["points"].as_array().unwrap().iter().map(|p| p.as_u64().unwrap()).collect()
}

fn rows_of(edge: &Value) -> (u64, u64) {
    (edge["rows"][0].as_u64().unwrap(), edge["rows"][1].as_u64().unwrap())
}

fn refusal(s: &mut Session, id: u32) -> String {
    let answer = shape_answer(s, id);
    assert!(answer["edges"].as_array().unwrap().is_empty(), "a refused shape lists no edge: {answer}");
    answer["refused"].as_str().unwrap_or_else(|| panic!("shape {id} was not refused: {answer}")).to_owned()
}

// ---- design 1.2: the eight vectors -------------------------------------------------------------------------------------------------

#[test]
fn a_rectangle_has_four_straight_edges() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 2.0);
    let edges = edges_of(&mut s, id);
    assert_eq!(kinds(&edges), ["straight"; 4]);
    // The master's rectangle is drawn counterclockwise from its lower-left corner: a move (row 1) and four lines (rows 2 to 5).
    assert_eq!(edges.iter().map(rows_of).collect::<Vec<_>>(), [(2, 2), (3, 3), (4, 4), (5, 5)]);
    assert_eq!(edges.iter().map(|e| e["edge"].as_u64().unwrap()).collect::<Vec<_>>(), [0, 1, 2, 3]);
    assert!(edges.iter().all(|e| e["section"] == "Geometry1"));
    for (edge, length) in edges.iter().zip([1.5, 1.0, 1.5, 1.0]) {
        assert_near("length", f(&edge["length"]), length);
    }
}

#[test]
fn the_ellipse_is_one_closed_edge() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "ellipse", 3.0, 2.0);
    let edges = edges_of(&mut s, id);
    assert_eq!(kinds(&edges), ["closed"]);
    assert_eq!(rows_of(&edges[0]), (1, 1), "the one `Ellipse` row");
    // The perimeter of a 1.5 by 1 ellipse (Ramanujan's second approximation, good to some 1E-8 at this eccentricity).
    let (a, b) = (0.75_f64, 0.5_f64);
    let h = ((a - b) / (a + b)).powi(2);
    let perimeter = std::f64::consts::PI * (a + b) * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt()));
    assert!((f(&edges[0]["length"]) - perimeter).abs() < 1.0e-6, "{} against {perimeter}", edges[0]["length"]);
    // A closed edge starts and ends at the curve's start: the ellipse starts at its right end.
    assert_eq!(edges[0]["from"], edges[0]["to"]);
    let (x, y) = place(&edges[0]["from"]);
    assert!(near(x, 3.75) && near(y, 2.0), "({x}, {y})");
}

#[test]
fn a_rounded_rectangle_has_eight_edges() {
    // 2 by 1 with quarter-circle corners of radius 0.25: four sides and four arcs, drawn counterclockwise from the bottom side's start.
    let r = 0.25_f64;
    let m = r - r * std::f64::consts::FRAC_1_SQRT_2;
    let mut s = at(base(1, 2.0, 1.0), 0, 1, "MoveTo", 0.25, 0.0);
    s = at(s, 0, 2, "LineTo", 1.75, 0.0);
    s = arc(s, 0, 3, (2.0, 0.25), (2.0 - m, m));
    s = at(s, 0, 4, "LineTo", 2.0, 0.75);
    s = arc(s, 0, 5, (1.75, 1.0), (2.0 - m, 1.0 - m));
    s = at(s, 0, 6, "LineTo", 0.25, 1.0);
    s = arc(s, 0, 7, (0.0, 0.75), (m, 1.0 - m));
    s = at(s, 0, 8, "LineTo", 0.0, 0.25);
    s = arc(s, 0, 9, (0.25, 0.0), (m, m));
    let mut session = session_with(page_of(vec![s]));
    let edges = edges_of(&mut session, 1);
    assert_eq!(kinds(&edges), ["straight", "curved", "straight", "curved", "straight", "curved", "straight", "curved"]);
    assert_near("a quarter circle's length", f(&edges[1]["length"]), std::f64::consts::FRAC_PI_2 * r);
    assert_eq!(rows_of(&edges[1]), (3, 3));
}

/// A circle of radius 1 about (1, 1) drawn as four quarter arcs from its right end.
fn circle_of_four_arcs(id: u32) -> ShapeBuilder {
    let k = std::f64::consts::FRAC_1_SQRT_2;
    let mut s = at(base(id, 2.0, 2.0), 0, 1, "MoveTo", 2.0, 1.0);
    s = arc(s, 0, 2, (1.0, 2.0), (1.0 + k, 1.0 + k));
    s = arc(s, 0, 3, (0.0, 1.0), (1.0 - k, 1.0 + k));
    s = arc(s, 0, 4, (1.0, 0.0), (1.0 - k, 1.0 - k));
    arc(s, 0, 5, (2.0, 1.0), (1.0 + k, 1.0 - k))
}

#[test]
fn four_smooth_arcs_make_one_closed_edge() {
    let mut s = session_with(page_of(vec![circle_of_four_arcs(1)]));
    let edges = edges_of(&mut s, 1);
    assert_eq!(kinds(&edges), ["closed"]);
    assert_eq!(rows_of(&edges[0]), (2, 5), "from the first arc's row to the last's");
    assert_near("the circle's length", f(&edges[0]["length"]), 2.0 * std::f64::consts::PI);
}

#[test]
fn a_teardrop_has_two_curved_edges() {
    // Two arcs from a tip at (1, 2) to the bottom (1, 0) and back, which meet at the tip in a corner.
    let mut s = at(base(1, 2.0, 2.0), 0, 1, "MoveTo", 1.0, 2.0);
    s = arc(s, 0, 2, (1.0, 0.0), (0.1, 0.8));
    s = arc(s, 0, 3, (1.0, 2.0), (1.9, 0.8));
    let mut session = session_with(page_of(vec![s]));
    let edges = edges_of(&mut session, 1);
    assert_eq!(kinds(&edges), ["curved", "curved"]);
    assert_eq!(edges.iter().map(rows_of).collect::<Vec<_>>(), [(2, 2), (3, 3)]);
}

#[test]
fn an_arc_of_bow_nought_is_a_straight_edge_and_one_with_a_bow_is_a_curved_one() {
    // `ArcTo` draws a straight line where its bow (`A`) is 0 (§1.2 rule 1), and a curve where it is not.
    let arc_to = |id: u32, bow: &str| row(at(base(id, 2.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0), 0, 2, "ArcTo", &[("X", "2"), ("Y", "0"), ("A", bow)]);
    let mut s = session_with(page_of(vec![arc_to(1, "0"), arc_to(2, "0.5")]));
    assert_eq!(kinds(&edges_of(&mut s, 1)), ["straight"]);
    let curved = edges_of(&mut s, 2);
    assert_eq!(kinds(&curved), ["curved"]);
    assert!(f(&curved[0]["length"]) > 2.0, "a bowed arc is longer than its chord: {}", curved[0]["length"]);
}

#[test]
fn each_piece_of_a_polyline_is_an_edge() {
    // A move and a `PolylineTo` whose constructor holds two vertices before the row's own end: three pieces, all of the row 2.
    let s = at(base(1, 3.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0);
    let s = row(s, 0, 2, "PolylineTo", &[("X", "3"), ("Y", "0"), ("A", "POLYLINE(1,1,1,1,2,0)")]);
    let mut session = session_with(page_of(vec![s]));
    let edges = edges_of(&mut session, 1);
    assert_eq!(kinds(&edges), ["straight"; 3]);
    assert_eq!(edges.iter().map(rows_of).collect::<Vec<_>>(), [(2, 2); 3]);
    assert_near("the first piece", f(&edges[0]["length"]), 2.0_f64.sqrt());
    assert_near("the last piece", f(&edges[2]["length"]), 1.0);
}

#[test]
fn two_sections_give_eight_edges() {
    let s = polygon(rectangle(1, 2.0, 1.0), 1, &[(5.0, 5.0), (6.0, 5.0), (6.0, 6.0), (5.0, 6.0)]);
    let mut session = session_with(page_of(vec![s]));
    let edges = edges_of(&mut session, 1);
    assert_eq!(edges.len(), 8);
    assert_eq!(edges.iter().map(|e| e["edge"].as_u64().unwrap()).collect::<Vec<_>>(), (0..8).collect::<Vec<_>>());
    assert_eq!(edges.iter().filter(|e| e["section"] == "Geometry1").count(), 4);
    assert_eq!(edges.iter().filter(|e| e["section"] == "Geometry2").count(), 4);
    assert!(edges[..4].iter().all(|e| e["section"] == "Geometry1"), "the sections in `IX` order, then in the order drawn");
}

#[test]
fn a_hidden_section_gives_none() {
    let mut session = session_with(page_of(vec![hidden(rectangle(1, 2.0, 1.0), 0)]));
    let words = refusal(&mut session, 1);
    assert_eq!(words, "shape 1 draws no outline: it has no edge to put connection points on");
    // A hidden section beside a shown one gives the shown one's edges, numbered from 0.
    let s = polygon(hidden(rectangle(1, 2.0, 1.0), 0), 1, &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]);
    let mut session = session_with(page_of(vec![s]));
    let edges = edges_of(&mut session, 1);
    assert_eq!(edges.len(), 3);
    assert!(edges.iter().all(|e| e["section"] == "Geometry2"));
    assert_eq!(edges[0]["edge"], 0);
}

#[test]
fn a_zero_length_step_is_no_edge_and_leaves_no_gap_in_the_numbers() {
    let s = at(base(1, 2.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0);
    let s = at(s, 0, 2, "LineTo", 0.0, 0.0);
    let s = at(s, 0, 3, "LineTo", 1.0, 0.0);
    let mut session = session_with(page_of(vec![s]));
    let edges = edges_of(&mut session, 1);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0]["edge"], 0);
    assert_eq!(rows_of(&edges[0]), (3, 3));
}

#[test]
fn an_inherited_section_is_read_through_the_master() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "diamond", 3.0, 2.0);
    // The instance holds no `Geometry` section of its own: its four edges are the master's.
    let at_shape = doc(&s).find_shape(diacraft_doc::Owner::Page(1), id).unwrap();
    assert!(doc(&s).shape(&at_shape).unwrap().sheet.sections.iter().all(|sec| sec.name.as_str() != "Geometry"));
    let edges = edges_of(&mut s, id);
    assert_eq!(kinds(&edges), ["straight"; 4]);
    assert_near("a side of the diamond", f(&edges[0]["length"]), (0.75_f64 * 0.75 + 0.5 * 0.5).sqrt());
}

// ---- design 2: which points belong to an edge ----------------------------------------------------------------------------

#[test]
fn a_point_on_a_corner_is_on_no_edge() {
    // The diamond's four points are its vertices (rows 0 to 3): corners, so on no edge, and kept.
    let mut s = blank_session();
    let id = drop_shape(&mut s, "diamond", 3.0, 2.0);
    let answer = shape_answer(&mut s, id);
    assert!(answer["edges"].as_array().unwrap().iter().all(|e| points_of(e).is_empty()), "{answer}");
    assert_eq!(answer["other"], json!([0, 1, 2, 3]));
    assert_eq!(answer["points"], 4);
}

#[test]
fn an_outward_point_and_an_extended_row_are_on_no_edge() {
    // A 2 by 1 rectangle. Row 0 is an ordinary inward point on the bottom side, row 1 an outward one (`Type` 1) there, row 2 an extended row (a formula in `D`) on the
    // left side, and row 3 a point of `Type` 2 (both) on the top side: rows 0 and 3 are points of edges, rows 1 and 2 are kept and on none.
    let s = point(rectangle(1, 2.0, 1.0), 0, 1.0, 0.0);
    let s = point(s, 1, 0.5, 0.0).row_cell("Connection", None, RowKey::Ix(1), "Type", "1");
    let s = point(s, 2, 0.0, 0.5).row_formula("Connection", None, RowKey::Ix(2), "D", "1+1", "2");
    let s = point(s, 3, 1.0, 1.0).row_cell("Connection", None, RowKey::Ix(3), "Type", "2");
    let mut session = session_with(page_of(vec![s]));
    let answer = shape_answer(&mut session, 1);
    let edges = answer["edges"].as_array().unwrap();
    assert_eq!(points_of(&edges[0]), [0], "the bottom side");
    assert_eq!(points_of(&edges[2]), [3], "the top side");
    assert!(points_of(&edges[3]).is_empty(), "the extended row is not on the left side");
    assert_eq!(answer["other"], json!([1, 2]));
}

#[test]
fn a_point_near_two_edges_is_on_the_nearer() {
    // Two sections draw a line each, 0.0005 inch apart: a point 0.0002 from the first is the first's, one 0.0004 from it (0.0001 from the second) the second's.
    let s = at(base(1, 2.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0);
    let s = at(s, 0, 2, "LineTo", 2.0, 0.0);
    let s = at(s, 1, 1, "MoveTo", 0.0, 0.0005);
    let s = at(s, 1, 2, "LineTo", 2.0, 0.0005);
    let s = point(point(s, 0, 1.0, 0.0002), 1, 0.5, 0.0004);
    let mut session = session_with(page_of(vec![s]));
    let edges = edges_of(&mut session, 1);
    assert_eq!(points_of(&edges[0]), [0]);
    assert_eq!(points_of(&edges[1]), [1]);
    // As near to both goes to the lower-numbered edge.
    let s = at(base(1, 2.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0);
    let s = at(s, 0, 2, "LineTo", 2.0, 0.0);
    let s = at(s, 1, 1, "MoveTo", 0.0, 0.0);
    let s = at(s, 1, 2, "LineTo", 2.0, 0.0);
    let s = point(s, 0, 1.0, 0.0);
    let mut session = session_with(page_of(vec![s]));
    let edges = edges_of(&mut session, 1);
    assert_eq!(points_of(&edges[0]), [0]);
    assert!(points_of(&edges[1]).is_empty());
}

#[test]
fn a_point_a_thousandth_of_an_inch_off_is_on_and_two_are_not() {
    // One line along the bottom. 0.001 inch off it, on either side, is on it (a glued end lies on its row within that); 0.002 off is not.
    let s = at(base(1, 2.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0);
    let s = at(s, 0, 2, "LineTo", 2.0, 0.0);
    let s = point(point(point(point(s, 0, 0.5, 0.001), 1, 1.0, -0.001), 2, 1.5, 0.002), 3, 1.2, -0.002);
    let mut session = session_with(page_of(vec![s]));
    let answer = shape_answer(&mut session, 1);
    assert_eq!(points_of(&answer["edges"][0]), [0, 1]);
    assert_eq!(answer["other"], json!([2, 3]));
}

#[test]
fn a_point_within_a_thousandth_of_an_end_of_an_open_edge_is_on_no_edge_and_on_a_closed_one_it_is() {
    // The open side's end is a corner: a point 0.0005 from it is on no edge. A closed curve has no end: a point at its start is on it.
    let s = at(base(1, 2.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0);
    let s = at(s, 0, 2, "LineTo", 2.0, 0.0);
    let s = point(point(s, 0, 0.0005, 0.0), 1, 1.0, 0.0);
    let mut session = session_with(page_of(vec![s]));
    let answer = shape_answer(&mut session, 1);
    assert_eq!(points_of(&answer["edges"][0]), [1]);
    assert_eq!(answer["other"], json!([0]));
    let mut session = session_with(page_of(vec![point(circle_of_four_arcs(1), 0, 2.0, 1.0)]));
    let edges = edges_of(&mut session, 1);
    assert_eq!(points_of(&edges[0]), [0], "the circle's start");
}

#[test]
fn the_points_of_an_edge_come_in_their_order_along_it() {
    // The rectangle's right-hand rows come first by `IX`, but along the bottom side (left to right) row 2 is before row 1.
    let s = point(point(point(rectangle(1, 3.0, 1.0), 0, 2.0, 0.0), 1, 1.0, 0.0), 2, 2.0, 0.0);
    let mut session = session_with(page_of(vec![s]));
    let edges = edges_of(&mut session, 1);
    assert_eq!(points_of(&edges[0]), [1, 0, 2], "by place along the edge, the lower row first at one place");
}

#[test]
fn the_ellipses_four_points_are_on_its_one_edge_from_its_start() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "ellipse", 3.0, 2.0);
    let answer = shape_answer(&mut s, id);
    // The ellipse starts at its right end and runs counterclockwise: right (row 3), top (0), left (1), bottom (2).
    assert_eq!(points_of(&answer["edges"][0]), [3, 0, 1, 2]);
    assert_eq!(answer["other"], json!([]));
}

// ---- design 1.3: shapes with no edge, refused in words ---------------------------------------------------------------------------

#[test]
fn a_group_is_refused_in_words() {
    let group = base(12, 2.0, 1.0).group().child(rectangle(13, 2.0, 1.0));
    let mut s = session_with(page_of(vec![group]));
    assert_eq!(refusal(&mut s, 12), "shape 12 is a group: select a shape inside it to change its connection points");
    // A member of the group is a shape like any other, in its own frame.
    assert_eq!(edges_of(&mut s, 13).len(), 4);
}

#[test]
fn a_connector_is_refused_in_words() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 4.0);
    let connector = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    assert_eq!(
        refusal(&mut s, connector),
        format!("shape {connector} is a line or a connector: connection points are put on two-dimensional shapes")
    );
}

#[test]
fn a_guide_is_refused_in_words() {
    let mut s = session_with(page_of(vec![ShapeBuilder::new(9).kind(ShapeType::Guide)]));
    assert_eq!(refusal(&mut s, 9), "shape 9 is a line or a connector: connection points are put on two-dimensional shapes");
}

#[test]
fn a_text_shape_with_no_geometry_is_refused_in_words() {
    // A text box a file holds: text and a frame, and no `Geometry` section.
    let mut s = session_with(page_of(vec![base(4, 1.0, 0.25).text("Hello")]));
    assert_eq!(refusal(&mut s, 4), "shape 4 draws no outline: it has no edge to put connection points on");
}

#[test]
fn the_text_tool_box_has_the_outline_it_is_made_with() {
    // DiaCraft's own text box holds a rectangle: it paints nothing until formatted, but it has four sides and four points.
    let mut s = blank_session();
    run(&mut s, "text.box", json!({"page": 1, "x": 3, "y": 4, "text": "Hello"}));
    let answer = shape_answer(&mut s, 1);
    assert_eq!(answer["refused"], Value::Null);
    assert_eq!(answer["edges"].as_array().unwrap().len(), 4);
    assert_eq!(answer["points"], 4);
}

#[test]
fn a_shape_on_a_locked_layer_is_refused_in_words() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [id], "layers": ["Frozen"]}));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    let words = refusal(&mut s, id);
    assert!(words.contains(&format!("shape {id} ")) && words.contains("\"Frozen\"") && words.contains("locked"), "{words}");
}

// ---- the query's answer ------------------------------------------------------------------------------------------------------------

#[test]
fn the_query_tells_the_rectangles_edges_and_points() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 2.0);
    let before = snapshot(&s);
    let answer = query(&mut s, json!({"id": id}));
    assert_eq!(snapshot(&s), before, "a query changes nothing and is no undo step");
    assert_eq!(answer["page"], 1);
    assert_near("min_gap", f(&answer["min_gap"]), DEFAULT_MIN_GAP);
    assert!((f(&answer["min_gap"]) - 0.11811).abs() < 1.0e-5, "the default is 3 mm: {}", answer["min_gap"]);
    assert_eq!(answer["units_per_inch"], 1.0);
    assert_eq!(answer["scale_note"], Value::Null);
    let shape = &answer["shapes"][0];
    assert_eq!((shape["id"].as_u64(), &shape["refused"]), (Some(u64::from(id)), &Value::Null));
    assert_eq!(
        (shape["other"].clone(), shape["points"].clone(), shape["can_double"].clone(), shape["truncated"].clone()),
        (json!([]), json!(4), json!(true), json!(false))
    );
    let edges = shape["edges"].as_array().unwrap();
    // The rectangle's box on the page: (2.25, 1.5) to (3.75, 2.5). The bottom side first, then the right, the top and the left.
    let corners = [(2.25, 1.5), (3.75, 1.5), (3.75, 2.5), (2.25, 2.5)];
    for (i, edge) in edges.iter().enumerate() {
        let (from, to) = (corners[i], corners[(i + 1) % 4]);
        let (got_from, got_to) = (place(&edge["from"]), place(&edge["to"]));
        assert!(near(got_from.0, from.0) && near(got_from.1, from.1), "edge {i} from {got_from:?}");
        assert!(near(got_to.0, to.0) && near(got_to.1, to.1), "edge {i} to {got_to:?}");
        let path = edge["path"].as_array().unwrap();
        assert!(!path.is_empty() && path.len() <= 32, "a path of at most 32 points: {}", path.len());
        let (first, last) = (place(&path[0]), place(path.last().unwrap()));
        assert!(near(first.0, got_from.0) && near(first.1, got_from.1), "the path starts at the edge's start");
        assert!(near(last.0, got_to.0) && near(last.1, got_to.1), "and ends at its end");
    }
    // One point each: the top (row 0) is the third edge, the left (1) the fourth, the bottom (2) the first and the right (3) the second.
    assert_eq!(edges.iter().map(points_of).collect::<Vec<_>>(), [vec![2], vec![3], vec![0], vec![1]]);
    // A doubling of a side with its middle point makes two more, the least gap half of a portion (0.375 and 0.25 inch).
    assert_eq!(edges.iter().map(|e| e["adds"].as_u64().unwrap()).collect::<Vec<_>>(), [2, 2, 2, 2]);
    for (edge, gap) in edges.iter().zip([0.375, 0.25, 0.375, 0.25]) {
        assert_near("gap", f(&edge["gap"]), gap);
    }
}

#[test]
fn a_member_of_a_turned_group_is_told_on_the_page() {
    // The group is at (5, 5) turned a quarter turn counterclockwise; its member's bottom side runs from (0, 0) to (2, 0) in the group's frame: on the page from (5, 5) to
    // (5, 7), and the side's one point (its middle) is at (5, 6).
    let member = point(rectangle(13, 2.0, 1.0), 0, 1.0, 0.0);
    let group = ShapeBuilder::new(12)
        .group()
        .cell("PinX", "5")
        .cell("PinY", "5")
        .cell("LocPinX", "0")
        .cell("LocPinY", "0")
        .cell("Width", "2")
        .cell("Height", "1")
        .cell("Angle", "1.5707963267948966")
        .child(member);
    let mut s = session_with(page_of(vec![group]));
    let edges = edges_of(&mut s, 13);
    let (from, to) = (place(&edges[0]["from"]), place(&edges[0]["to"]));
    assert!(near(from.0, 5.0) && near(from.1, 5.0), "{from:?}");
    assert!(near(to.0, 5.0) && near(to.1, 7.0), "{to:?}");
    assert_eq!(points_of(&edges[0]), [0]);
    assert_near("its length is the same on the page", f(&edges[0]["length"]), 2.0);
}

#[test]
fn a_curved_edges_path_has_at_most_32_points_on_the_curve() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "ellipse", 3.0, 2.0);
    let edge = edges_of(&mut s, id).remove(0);
    let path = edge["path"].as_array().unwrap();
    assert!(path.len() > 4 && path.len() <= 32, "{}", path.len());
    // Every sample is on the ellipse about (3, 2) with semi-axes 0.75 and 0.5.
    for p in path {
        let (x, y) = place(p);
        let r = ((x - 3.0) / 0.75).powi(2) + ((y - 2.0) / 0.5).powi(2);
        assert!((r - 1.0).abs() < 1.0e-6, "({x}, {y}) is off the curve: {r}");
    }
}

#[test]
fn a_mixed_list_tells_a_connectors_words_beside_a_rectangles_edges() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "ellipse", 6.0, 4.0);
    let connector = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    let answer = query(&mut s, json!({"ids": [a, connector, b, a]}));
    let shapes = answer["shapes"].as_array().unwrap();
    assert_eq!(
        shapes.iter().map(|sh| sh["id"].as_u64().unwrap()).collect::<Vec<_>>(),
        [u64::from(a), u64::from(connector), u64::from(b)],
        "an ID named twice is told once"
    );
    assert_eq!(shapes[0]["edges"].as_array().unwrap().len(), 4);
    assert!(shapes[1]["refused"].as_str().unwrap().contains("a line or a connector"));
    assert_eq!(shapes[2]["edges"].as_array().unwrap().len(), 1);
}

#[test]
fn the_calls_params_are_checked_in_words() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 2.0);
    let msg = |s: &mut Session, params: Value| s.execute("shape.connection_edges", &params).unwrap_err().to_string();
    assert!(msg(&mut s, json!({})).contains("give `id` or `ids`"));
    assert!(msg(&mut s, json!({"id": id, "ids": [id]})).contains("not both"));
    assert!(msg(&mut s, json!({"id": 77})).contains("page 1 has no shape 77"));
    let too_many: Vec<u32> = (0..4097).collect();
    assert!(msg(&mut s, json!({"ids": too_many})).contains("between 1 and 4096"));
    for bad in [0.0099, 2.01, -1.0] {
        let words = msg(&mut s, json!({"id": id, "min_gap": bad}));
        assert!(words.contains("`min_gap` must be a number of paper inches from 0.01 to 2"), "{words}");
    }
    assert!(msg(&mut s, json!({"id": id, "min_gap": "wide"})).contains("`min_gap` must be a number"));
    // The ends of the range are allowed.
    for ok in [0.01, 2.0] {
        query(&mut s, json!({"id": id, "min_gap": ok}));
    }
    assert_eq!(check_session(&s), Ok(()));
}

// ---- design 3.8: the minimum distance --------------------------------------------------------------------------------

/// A shape holding one straight edge of `length` inches along the bottom, with a point at each of `shares` (the shares of its length).
fn edge_with_points(id: u32, length: f64, shares: &[f64]) -> ShapeBuilder {
    let mut s = at(at(base(id, length, 1.0), 0, 1, "MoveTo", 0.0, 0.0), 0, 2, "LineTo", length, 0.0);
    for (i, share) in shares.iter().enumerate() {
        s = point(s, u32::try_from(i).unwrap(), share * length, 0.0);
    }
    s
}

/// The `adds` and `gap` of the one edge of a shape holding `shares`, under `min_gap`.
fn room(shares: &[f64], min_gap: Option<f64>) -> (u64, f64) {
    let mut s = session_with(page_of(vec![edge_with_points(1, 1.0, shares)]));
    let params = min_gap.map_or(json!({"id": 1}), |g| json!({"id": 1, "min_gap": g}));
    let answer = query(&mut s, params);
    let edge = &answer["shapes"][0]["edges"][0];
    assert_eq!(answer["shapes"][0]["can_double"], edge["adds"].as_u64().unwrap() > 0, "can_double follows the one edge: {answer}");
    (edge["adds"].as_u64().unwrap(), f(&edge["gap"]))
}

fn halves(n: u32) -> Vec<f64> {
    // `n` points at the middles of the portions a doubling makes: the places k/(n+1).
    (1..=n).map(|k| f64::from(k) / f64::from(n + 1)).collect()
}

#[test]
fn a_one_inch_edge_is_doubled_to_three_and_to_seven_at_three_millimetres_and_then_has_no_room() {
    // One point in the middle: 1 -> 3 puts gaps of 6.35 mm (0.25 inch) and is allowed.
    let (adds, gap) = room(&halves(1), None);
    assert_eq!(adds, 2);
    assert_near("gap", gap, 0.25);
    // Three points: 3 -> 7 makes gaps of 3.175 mm, which is over 3 mm.
    let (adds, gap) = room(&halves(3), None);
    assert_eq!(adds, 4);
    assert_near("gap", gap, 0.125);
    // Seven points: the next doubling would make gaps of 1.5875 mm, under 3 mm: no room.
    let (adds, gap) = room(&halves(7), None);
    assert_eq!(adds, 0);
    assert_near("gap", gap, 0.0625);
}

#[test]
fn at_one_and_a_half_millimetres_the_same_edge_reaches_fifteen_points() {
    let min = 1.5 / 25.4;
    assert_eq!(room(&halves(7), Some(min)), (8, 0.0625), "7 -> 15 makes gaps of 1.5875 mm, over 1.5 mm");
    let (adds, gap) = room(&halves(15), Some(min));
    assert_eq!(adds, 0, "the next would make 0.79 mm gaps");
    assert_near("gap", gap, 0.03125);
}

#[test]
fn an_empty_edge_is_given_one_point_then_three_then_seven() {
    // An empty open edge of 1 inch: 0 -> 1 (gap 12.7 mm), and so on as above.
    let (adds, gap) = room(&[], None);
    assert_eq!(adds, 1);
    assert_near("gap", gap, 0.5);
    assert_eq!(room(&halves(1), None).0, 2);
    assert_eq!(room(&halves(3), None).0, 4);
    assert_eq!(room(&halves(7), None).0, 0);
}

#[test]
fn the_gap_is_the_least_of_half_the_portions_and_a_short_one_stops_the_whole_edge() {
    // Points at 4 mm and at 0.6 inch: the portions are 4 mm, 11.24 mm and 10.16 mm, so the least new gap is 2 mm, under 3 mm: the edge is not doubled, though two
    // of its portions have room.
    let four_mm = 4.0 / 25.4;
    let (adds, gap) = room(&[four_mm, 0.6], None);
    assert_eq!(adds, 0);
    assert_near("gap", gap, four_mm / 2.0);
    // The same with the near point at 7 mm: all three new gaps are over 3 mm.
    let (adds, gap) = room(&[7.0 / 25.4, 0.6], None);
    assert_eq!(adds, 3);
    assert!(gap > 3.0 / 25.4, "{gap}");
}

#[test]
fn a_gap_exactly_the_minimum_is_room() {
    // 7 -> 15 on a 1 inch edge makes gaps of exactly 1/16 inch: a minimum of 1/16 inch is met.
    assert_eq!(room(&halves(7), Some(0.0625)).0, 8);
    assert_eq!(room(&halves(7), Some(0.0626)).0, 0);
}

#[test]
fn a_page_drawn_at_one_to_a_hundred_measures_the_gap_on_the_paper() {
    // A 2 metre wall (78.74 drawing inches) drawn at 1:100 is 20 mm on paper. Under 3 mm: 0 -> 1 (10 mm) -> 3 (5 mm), then no room (2.5 mm).
    let wall = 2000.0 / 25.4;
    let mut counts = Vec::new();
    for shares in [halves(0), halves(1), halves(3)] {
        let page = PageBuilder::new().cell("PageScale", "1").cell("DrawingScale", "100").shape(edge_with_points(1, wall, &shares));
        let mut s = session_with(DocBuilder::new().page(1, page).build());
        let answer = query(&mut s, json!({"id": 1}));
        assert_eq!(answer["units_per_inch"], 100.0);
        assert_eq!(answer["scale_note"], Value::Null);
        let edge = &answer["shapes"][0]["edges"][0];
        assert_near("a length on paper", f(&edge["length"]), 20.0 / 25.4);
        counts.push((edge["adds"].as_u64().unwrap(), f(&edge["gap"])));
    }
    assert_eq!(counts.iter().map(|c| c.0).collect::<Vec<_>>(), [1, 2, 0]);
    assert_near("0 -> 1", counts[0].1, 10.0 / 25.4);
    assert_near("1 -> 3", counts[1].1, 5.0 / 25.4);
    assert_near("3 -> 7", counts[2].1, 2.5 / 25.4);
}

#[test]
fn a_page_scale_that_is_no_scale_is_read_as_one_to_one_and_says_so_once() {
    for (paper, drawing) in [("0", "100"), ("-1", "100"), ("1", "0"), ("1", "-5")] {
        let page = PageBuilder::new().cell("PageScale", paper).cell("DrawingScale", drawing).shape(edge_with_points(1, 1.0, &[0.5]));
        let mut s = session_with(DocBuilder::new().page(1, page).build());
        let answer = query(&mut s, json!({"ids": [1]}));
        assert_eq!(answer["units_per_inch"], 1.0, "{paper}:{drawing}");
        let note = answer["scale_note"].as_str().unwrap_or_else(|| panic!("{paper}:{drawing}: no note: {answer}"));
        assert!(note.contains("1:1"), "{note}");
        assert_near("compared in drawing inches", f(&answer["shapes"][0]["edges"][0]["gap"]), 0.25);
    }
    // A page with no scale cells at all is plainly one to one: nothing to say.
    let mut s = session_with(page_of(vec![edge_with_points(1, 1.0, &[0.5])]));
    assert_eq!(query(&mut s, json!({"id": 1}))["scale_note"], Value::Null);
}

#[test]
fn a_closed_edge_with_points_has_as_many_gaps_as_points_and_an_empty_one_halves_its_length() {
    // The ellipse's four points are a quarter of its length apart: a doubling makes gaps of an eighth of 3.9663 inch, 0.4958 inch.
    let mut s = blank_session();
    let id = drop_shape(&mut s, "ellipse", 3.0, 2.0);
    let edge = edges_of(&mut s, id).remove(0);
    assert_eq!(edge["adds"], 4, "a closed edge of N points gets N");
    assert_near("gap", f(&edge["gap"]), f(&edge["length"]) / 8.0);
    // An empty closed edge gets two points, its start and halfway round: the new gap is half its length.
    let circle = circle_of_four_arcs(1);
    let mut s = session_with(page_of(vec![circle]));
    let edge = edges_of(&mut s, 1).remove(0);
    assert_eq!(edge["adds"], 2);
    assert_near("gap", f(&edge["gap"]), std::f64::consts::PI);
    // One point on a closed edge has one gap round the whole curve: it gains one, and N = 1 gives 2 in all.
    let mut s = session_with(page_of(vec![point(circle_of_four_arcs(1), 0, 2.0, 1.0)]));
    let edge = edges_of(&mut s, 1).remove(0);
    assert_eq!(edge["adds"], 1);
    assert_near("gap", f(&edge["gap"]), std::f64::consts::PI);
}

#[test]
fn a_selection_of_two_shapes_tells_each_ones_room() {
    // One edge with room and one without: each is told on its own, and `can_double` is per shape.
    let mut s = session_with(page_of(vec![edge_with_points(1, 1.0, &halves(1)), edge_with_points(2, 1.0, &halves(7))]));
    let answer = query(&mut s, json!({"ids": [1, 2]}));
    assert_eq!(answer["shapes"][0]["can_double"], true);
    assert_eq!(answer["shapes"][0]["edges"][0]["adds"], 2);
    assert_eq!(answer["shapes"][1]["can_double"], false);
    assert_eq!(answer["shapes"][1]["edges"][0]["adds"], 0);
}

#[test]
fn a_shape_whose_doubling_would_pass_the_cap_cannot_be_doubled() {
    // 128 points on one open edge of 100 inches: a doubling makes 257, past the 256 a connector looks at. 127 make 255.
    let mut crowded = at(at(base(1, 100.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0), 0, 2, "LineTo", 100.0, 0.0);
    for i in 0..128u32 {
        crowded = point(crowded, i, 100.0 * f64::from(i + 1) / 129.0, 0.0);
    }
    let mut s = session_with(page_of(vec![crowded]));
    let shape = shape_answer(&mut s, 1);
    assert_eq!(shape["edges"][0]["adds"], 129, "the count is what a doubling would make");
    assert_eq!(shape["can_double"], false, "but 128 + 129 is past 256");
    let mut few = at(at(base(1, 100.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0), 0, 2, "LineTo", 100.0, 0.0);
    for i in 0..127u32 {
        few = point(few, i, 100.0 * f64::from(i + 1) / 128.0, 0.0);
    }
    let mut s = session_with(page_of(vec![few]));
    let shape = shape_answer(&mut s, 1);
    assert_eq!((shape["edges"][0]["adds"].clone(), shape["can_double"].clone()), (json!(128), json!(true)), "127 + 128 is 255");
}

// ---- design 9: the counts ----------------------------------------------------------------------------------------------------------

/// A zigzag of `lines` `LineTo` rows after a move, every piece over an inch long.
fn zigzag(id: u32, lines: u32) -> ShapeBuilder {
    let mut s = at(base(id, 1.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0);
    for i in 1..=lines {
        s = at(s, 0, i + 1, "LineTo", f64::from(i) * 0.5, f64::from(i % 2));
    }
    s
}

#[test]
fn a_shape_of_4097_lineto_rows_is_truncated() {
    let edges_cap = u32::try_from(MAX_EDGES_PER_SHAPE).unwrap();
    let mut s = session_with(page_of(vec![zigzag(1, edges_cap + 1)]));
    let answer = shape_answer(&mut s, 1);
    assert_eq!(answer["truncated"], true);
    assert_eq!(answer["edges"].as_array().unwrap().len(), MAX_EDGES_PER_SHAPE);
    // Exactly the most is not truncated.
    let mut s = session_with(page_of(vec![zigzag(1, edges_cap)]));
    let answer = shape_answer(&mut s, 1);
    assert_eq!(answer["truncated"], false);
    assert_eq!(answer["edges"].as_array().unwrap().len(), MAX_EDGES_PER_SHAPE);
}

#[test]
fn rows_and_steps_are_counted_on_a_section_of_200000_rows() {
    // A move and 199,999 lines: every row is read, every step is drawn, 4,096 edges are made and the rest only counted as more.
    let built = page_of(vec![zigzag(1, 199_999)]);
    let answer = query_edges(&built, 1, &[1], DEFAULT_MIN_GAP).unwrap();
    assert_eq!(answer.work.rows, 200_000);
    assert_eq!(answer.work.steps, 200_000);
    assert_eq!(answer.work.arc_calls, 0, "straight edges cost no arc call");
    assert!(answer.shapes[0].truncated);
    assert_eq!(answer.shapes[0].edges.len(), MAX_EDGES_PER_SHAPE);
    // One row more is past what is read, and none is merged.
    diacraft_doc::work::reset();
    let built = page_of(vec![zigzag(1, 200_000)]);
    let answer = query_edges(&built, 1, &[1], DEFAULT_MIN_GAP).unwrap();
    let words = answer.shapes[0].refused.as_deref().unwrap();
    assert!(words.contains("200001 rows") && words.contains("more than the 200000"), "{words}");
    assert_eq!((answer.work.rows, answer.work.steps), (0, 0));
    assert_eq!(diacraft_doc::work::rows_merged(), 0, "a section past the cap is refused by its count and merges none");
}

#[test]
fn a_section_of_200000_zero_length_steps_has_no_edge_and_its_steps_are_counted() {
    // A move and 199,999 lines to the same place: every step is drawn and counted, none is an edge, and the shape draws no outline.
    let mut shape = at(base(1, 1.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0);
    for i in 0..199_999u32 {
        shape = at(shape, 0, i + 2, "LineTo", 0.0, 0.0);
    }
    let answer = query_edges(&page_of(vec![shape]), 1, &[1], DEFAULT_MIN_GAP).unwrap();
    assert_eq!((answer.work.rows, answer.work.steps), (200_000, 200_000));
    assert_eq!(answer.shapes[0].refused.as_deref(), Some("shape 1 draws no outline: it has no edge to put connection points on"));
}

#[test]
fn a_master_of_100000_connection_rows_is_refused_by_its_count_and_merges_none() {
    let mut master_shape = rectangle(1, 1.0, 1.0);
    for i in 0..100_000u32 {
        master_shape = master_shape.row_cell("Connection", None, RowKey::Ix(i), "X", "0");
    }
    let page = PageBuilder::new().shape(ShapeBuilder::new(5).master(1).cell("PinX", "0").cell("PinY", "0"));
    let built = DocBuilder::new().master(1, diacraft_testkit::build::MasterBuilder::new().shape(master_shape)).page(1, page).build();
    diacraft_doc::work::reset();
    let answer = query_edges(&built, 1, &[5], DEFAULT_MIN_GAP).unwrap();
    let words = answer.shapes[0].refused.as_deref().unwrap();
    assert_eq!(words, "shape 5 has more than 256 connection points, counted over the shape and its master: it is not read");
    assert_eq!(answer.work.point_rows, 0, "no row was charged");
    assert_eq!(diacraft_doc::work::rows_merged(), 0, "and none was merged");
    // A section of exactly 256 rows is read; 257 are not.
    for (rows, read) in [(256u32, true), (257, false)] {
        let mut shape = rectangle(1, 1.0, 1.0);
        for i in 0..rows {
            shape = point(shape, i, f64::from(i) / 256.0, 0.0);
        }
        let answer = query_edges(&page_of(vec![shape]), 1, &[1], DEFAULT_MIN_GAP).unwrap();
        assert_eq!(answer.shapes[0].refused.is_none(), read, "{rows} rows");
        assert_eq!(answer.shapes[0].points, if read { 256 } else { 0 });
    }
}

#[test]
fn a_shape_with_more_than_64_geometry_sections_is_refused_by_its_count() {
    let mut shape = base(1, 1.0, 1.0);
    for section in 0..65u32 {
        shape = at(at(shape, section, 1, "MoveTo", 0.0, 0.0), section, 2, "LineTo", 1.0, 0.0);
    }
    let answer = query_edges(&page_of(vec![shape]), 1, &[1], DEFAULT_MIN_GAP).unwrap();
    let words = answer.shapes[0].refused.as_deref().unwrap();
    assert!(words.contains("more than 64 Geometry sections"), "{words}");
    assert_eq!(answer.work.rows, 0);
}

/// A shape of `rows` `PolylineTo` rows of 50,000 points each after a move: 50,001 steps for each row.
fn polylines(id: u32, rows: u32) -> ShapeBuilder {
    let pairs: String = (0..49_999).map(|i| if i % 2 == 0 { "0,1," } else { "1,0," }).collect();
    let text = format!("POLYLINE(1,1,{}0,0)", pairs);
    let mut s = at(base(id, 1.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0);
    for i in 0..rows {
        s = row(s, 0, i + 2, "PolylineTo", &[("X", "1"), ("Y", "1"), ("A", &text)]);
    }
    s
}

#[test]
fn the_calls_budget_of_steps_is_charged_over_all_its_shapes() {
    // 19 rows of 50,001 steps and the move are 950,020 steps: within the 1,000,000 a call may draw. A second shape of two such rows would be 100,002 more: refused,
    // and so is the plain rectangle after it, because the call's steps are spent.
    let built = page_of(vec![polylines(1, 19), polylines(2, 2), rectangle(3, 2.0, 1.0)]);
    let answer = query_edges(&built, 1, &[1, 2, 3], DEFAULT_MIN_GAP).unwrap();
    assert!(answer.shapes[0].refused.is_none(), "{:?}", answer.shapes[0].refused);
    assert!(answer.shapes[0].truncated);
    assert_eq!(answer.shapes[0].edges.len(), MAX_EDGES_PER_SHAPE);
    for shape in &answer.shapes[1..] {
        let words = shape.refused.as_deref().unwrap();
        assert!(words.contains("steps") && words.contains(&MAX_OUTLINE_STEPS.to_string()), "{words}");
    }
    assert!(answer.work.steps <= MAX_OUTLINE_STEPS, "{}", answer.work.steps);
}

#[test]
fn arc_calls_are_counted_by_the_pieces_each_point_is_tried_against() {
    // The ellipse is `n` cubics. Reading it costs a length for each, each of its four points is tried against every piece and placed by one more length, and its path
    // is 32 places: the units the loops walk, counted.
    let mut s = blank_session();
    let id = drop_shape(&mut s, "ellipse", 3.0, 2.0);
    let n = edges_of(&mut s, id)[0]["steps"].as_u64().unwrap() as usize;
    assert!(n >= 4, "a full turn is at least four cubics: {n}");
    let answer = query_edges(doc(&s), 1, &[id], DEFAULT_MIN_GAP).unwrap();
    assert_eq!(answer.work.arc_calls, n + 4 * (n + 1) + MAX_PATH_POINTS, "{n} pieces");
    // A rectangle's sides are straight: no length, and its path is its two ends, so its points cost the visits only.
    let id = drop_shape(&mut s, "rectangle", 6.0, 2.0);
    let answer = query_edges(doc(&s), 1, &[id], DEFAULT_MIN_GAP).unwrap();
    assert_eq!(answer.work.arc_calls, 4, "each point is tried against the one piece of the one side whose box it is in");
}

/// A spline of `knots` knot rows, uniform and clamped, drawn as `knots - 1` cubics in one run, with 256 points inside its box.
fn long_spline(id: u32, knots: u32, points: u32) -> ShapeBuilder {
    let mut s = at(base(id, f64::from(knots), 1.0), 0, 1, "MoveTo", 0.0, 0.0);
    // `SplineStart` X, Y, second knot A, first knot B, last knot C, degree D; then the knots.
    let last = (knots - 1).to_string();
    s = row(s, 0, 2, "SplineStart", &[("X", "1"), ("Y", "1"), ("A", "0"), ("B", "0"), ("C", &last), ("D", "3")]);
    for i in 1..=knots {
        let knot = i.saturating_sub(2);
        let (x, y) = (f64::from(i) + 1.0, f64::from(i % 2));
        s = row(s, 0, i + 2, "SplineKnot", &[("X", &x.to_string()), ("Y", &y.to_string()), ("A", &knot.to_string())]);
    }
    for p in 0..points {
        s = point(s, p, f64::from(p) * f64::from(knots) / f64::from(points), 0.5);
    }
    s
}

#[test]
fn the_calls_budget_of_arc_calls_stops_a_curve_of_thousands_of_pieces_with_256_points() {
    let built = page_of(vec![long_spline(1, 4_100, 256)]);
    let answer = query_edges(&built, 1, &[1], DEFAULT_MIN_GAP).unwrap();
    let words = answer.shapes[0].refused.as_deref().unwrap_or_else(|| panic!("not refused: {:?}", answer.shapes[0].edges.first().map(|e| e.kind)));
    assert!(words.contains("arc") && words.contains(&MAX_ARC_CALLS.to_string()), "{words}");
    assert!(answer.work.arc_calls <= MAX_ARC_CALLS, "{}", answer.work.arc_calls);
}

#[test]
fn a_cusp_and_a_cubic_with_control_points_a_million_inches_apart_are_edges_of_finite_length() {
    // A cubic along one line that turns back on itself (a cusp), and one whose control points are 1E6 apart.
    for (cells, what) in [
        ([("A", "1"), ("B", "0"), ("C", "0"), ("D", "0")], "a cusp"),
        ([("A", "1000000"), ("B", "1000000"), ("C", "-1000000"), ("D", "-1000000")], "far control points"),
    ] {
        let s = at(base(1, 1.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0);
        let s = row(s, 0, 2, "RelCubBezTo", &[("X", "1"), ("Y", "0"), cells[0], cells[1], cells[2], cells[3]]);
        let answer = query_edges(&page_of(vec![s]), 1, &[1], DEFAULT_MIN_GAP).unwrap();
        let shape = &answer.shapes[0];
        assert_eq!(shape.edges.len(), 1, "{what}: {:?}", shape.refused);
        assert!(shape.edges[0].length.is_finite() && shape.edges[0].length > 0.0, "{what}");
        // One length for the curve and a place for each of the 32 points of its path.
        assert_eq!(answer.work.arc_calls, 1 + MAX_PATH_POINTS, "{what}");
    }
}

#[test]
fn an_ellipse_with_a_zero_axis_and_one_with_cells_that_are_no_numbers_do_not_hang_or_panic() {
    // The second point on the centre: a flat ellipse, a line walked there and back, or nothing: any answer, in a count.
    let flat = row(base(1, 2.0, 2.0), 0, 1, "Ellipse", &[("X", "1"), ("Y", "1"), ("A", "2"), ("B", "1"), ("C", "1"), ("D", "1")]);
    let answer = query_edges(&page_of(vec![flat]), 1, &[1], DEFAULT_MIN_GAP).unwrap();
    assert!(answer.shapes[0].edges.len() <= 1);
    assert!(answer.shapes[0].edges.iter().all(|e| e.length.is_finite()));
    // Cells of text that are no numbers read 0: an ellipse of no size draws nothing.
    let words = row(base(2, 2.0, 2.0), 0, 1, "Ellipse", &[("X", "one"), ("Y", "two"), ("A", "three"), ("B", "four"), ("C", "five"), ("D", "six")]);
    let answer = query_edges(&page_of(vec![words]), 1, &[2], DEFAULT_MIN_GAP).unwrap();
    assert_eq!(answer.shapes[0].refused.as_deref(), Some("shape 2 draws no outline: it has no edge to put connection points on"));
}

#[test]
fn numbers_too_big_to_add_and_a_huge_scale_are_told_and_never_panic() {
    // A line from -1E308 to 1E308 is 2E308 long, which is no number: the shape is refused in words, or its edges are finite. Never a panic.
    let wide = at(at(base(1, 1.0, 1.0), 0, 1, "MoveTo", -1.0e308, 0.0), 0, 2, "LineTo", 1.0e308, 0.0);
    let answer = query_edges(&page_of(vec![wide]), 1, &[1], DEFAULT_MIN_GAP).unwrap();
    let shape = &answer.shapes[0];
    assert!(shape.refused.is_some() || shape.edges.iter().all(|e| e.length.is_finite()), "{shape:?}");
    // A scale of 1E300: every length on paper is tiny and finite, and nothing has room.
    let page = PageBuilder::new().cell("PageScale", "1").cell("DrawingScale", "1e300").shape(edge_with_points(1, 1.0, &[0.5]));
    let mut s = session_with(DocBuilder::new().page(1, page).build());
    let answer = query(&mut s, json!({"id": 1}));
    assert_eq!(answer["shapes"][0]["edges"][0]["adds"], 0);
    assert_eq!(answer["shapes"][0]["can_double"], false);
    assert_eq!(check_session(&s), Ok(()));
}

// ---- the timed hostile inputs, beside a yardstick ----------------------------------------------------------------------------------

/// Times `f` and a yardstick (reading the same page's rectangle 1,000 times), and says both. Run with `cargo test -- --ignored --nocapture`.
fn timed(what: &str, f: impl FnOnce()) {
    let yard = page_of(vec![rectangle(1, 2.0, 1.0)]);
    let started = std::time::Instant::now();
    for _ in 0..1_000 {
        query_edges(&yard, 1, &[1], DEFAULT_MIN_GAP).unwrap();
    }
    let yardstick = started.elapsed();
    let started = std::time::Instant::now();
    f();
    println!("{what}: {:?} (1,000 rectangles: {yardstick:?})", started.elapsed());
}

#[test]
#[ignore = "timed, beside a yardstick"]
fn timed_a_section_of_200000_rows_and_four_shapes_of_4096_edges() {
    let built = page_of(vec![zigzag(1, 199_999)]);
    timed("200,000 LineTo rows", || {
        query_edges(&built, 1, &[1], DEFAULT_MIN_GAP).unwrap();
    });
    let built = page_of((1..=4).map(|id| zigzag(id, 4_096)).collect());
    timed("16,384 edges in four shapes", || {
        let answer = query_edges(&built, 1, &[1, 2, 3, 4], DEFAULT_MIN_GAP).unwrap();
        assert_eq!(answer.shapes.iter().map(|s| s.edges.len()).sum::<usize>(), 16_384);
    });
}

#[test]
#[ignore = "timed, beside a yardstick"]
fn timed_256_points_on_4096_edges_and_200000_zero_length_steps_and_a_huge_scale() {
    let mut crowded = zigzag(1, 4_096);
    for i in 0..256u32 {
        crowded = point(crowded, i, f64::from(i) * 8.0, 0.5);
    }
    let built = page_of(vec![crowded]);
    timed("256 points on 4,096 edges", || {
        let answer = query_edges(&built, 1, &[1], DEFAULT_MIN_GAP).unwrap();
        assert_eq!(answer.shapes[0].points, 256);
    });
    let mut flat = at(base(2, 1.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0);
    for i in 0..199_999u32 {
        flat = at(flat, 0, i + 2, "LineTo", 0.0, 0.0);
    }
    let built = page_of(vec![flat]);
    timed("200,000 zero-length steps", || {
        query_edges(&built, 1, &[2], DEFAULT_MIN_GAP).unwrap();
    });
    let page = PageBuilder::new().cell("PageScale", "1").cell("DrawingScale", "1e300").shape(zigzag(3, 4_096));
    let built = DocBuilder::new().page(1, page).build();
    timed("4,096 edges at a scale of 1E300", || {
        let answer = query_edges(&built, 1, &[3], DEFAULT_MIN_GAP).unwrap();
        assert!(answer.shapes[0].edges.iter().all(|e| e.length.is_finite()));
    });
}

#[test]
#[ignore = "timed, beside a yardstick"]
fn timed_256_points_on_a_curve_of_a_thousand_pieces() {
    let built = page_of(vec![long_spline(1, 1_000, 256)]);
    timed("256 points on a spline of a thousand pieces", || {
        let answer = query_edges(&built, 1, &[1], DEFAULT_MIN_GAP).unwrap();
        assert!(answer.shapes[0].refused.is_none());
    });
}
