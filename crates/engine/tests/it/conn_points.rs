//! `shape.connection_points` doubles the connection points of an edge or of every edge and halves the newest doubling, keeping old points where they are (design §3 to §8).
//! The counts and places are the vectors;
//! the glue of a halving and the keepers of the marker are in `conn_points_glue`, the costs and the hostile inputs in `conn_points_costs`.
//!
//! Shapes the tests make by hand sit at the page's origin with their local pin at (0, 0), so a place in the shape's frame is the same place on the page; the Rectangle is
//! DiaCraft's master resized to its size at the origin of the page.

use diacraft_calc::row_refs::RowName;
use diacraft_doc::{Cell, CellAddr, CellEdit, Owner, Resolver, RowKey, SectionKey, ShapeRef, SheetRef, Value as CellValue, WriteOrigin};
use diacraft_engine::Session;
use diacraft_engine::draw::conn_marker;
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use serde_json::{Value, json};

use crate::conn_points_edges::{at, base, edge_with_points, page_of, point, polygon, rectangle, row, session_with};
use crate::edit::{doc, drop_shape};
use crate::{blank_session, check_session, run, snapshot};

// ---- helpers (shared with `conn_points_glue` and `conn_points_costs`) ------------------------------------------------------------------

/// The Rectangle master resized to `w` by `h` with its lower left corner at the page's origin: its frame is the page's.
pub(crate) fn rect(s: &mut Session, w: f64, h: f64) -> u32 {
    let id = drop_shape(s, "rectangle", w / 2.0, h / 2.0);
    run(s, "shape.resize", json!({"id": id, "x": 0.0, "y": 0.0, "width": w, "height": h}));
    id
}

/// A row of the `Connection` section as the reader sees it, merged through the master.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pt {
    /// The row's `IX`, or its place when the section is keyed by name.
    pub ix: u32,
    pub name: Option<String>,
    pub x: f64,
    pub y: f64,
    pub dir: (f64, f64),
    pub kind: f64,
    pub x_formula: Option<String>,
    pub y_formula: Option<String>,
}

pub(crate) fn shape_at(s: &Session, id: u32) -> ShapeRef {
    // The first page: its id is 1 in a drawing made in a session, and the file's own in one read from a file.
    let page = doc(s).pages.get(0).map_or(1, |p| p.id);
    doc(s).find_shape(Owner::Page(page), id).unwrap_or_else(|| panic!("no shape {id} on page {page}"))
}

/// The effective `Connection` rows of shape `id`, in the section's order.
pub(crate) fn points(s: &Session, id: u32) -> Vec<Pt> {
    let res = Resolver::new(doc(s));
    let sheet = SheetRef::Shape(shape_at(s, id));
    let rows = res.effective_rows(&sheet, &SectionKey::new("Connection", None)).unwrap_or_default();
    rows.iter()
        .enumerate()
        .map(|(place, row)| {
            let find = |name: &str| row.cells.iter().find(|(n, _)| n.as_str() == name).map(|(_, found)| found);
            let num = |name: &str| find(name).and_then(|f| f.value.cell).and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or(0.0);
            let formula = |name: &str| find(name).and_then(|f| f.formula.cell).and_then(|c| c.formula.text().map(str::to_owned));
            let (ix, name) = match &row.key {
                RowKey::Ix(ix) => (*ix, None),
                RowKey::Name(n) => (u32::try_from(place).unwrap(), Some(n.to_string())),
            };
            Pt {
                ix,
                name,
                x: num("X"),
                y: num("Y"),
                dir: (num("DirX"), num("DirY")),
                kind: num("Type"),
                x_formula: formula("X"),
                y_formula: formula("Y"),
            }
        })
        .collect()
}

pub(crate) fn places(s: &Session, id: u32) -> Vec<(f64, f64)> {
    points(s, id).iter().map(|p| (p.x, p.y)).collect()
}

pub(crate) fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

pub(crate) fn assert_places(what: &str, got: &[(f64, f64)], want: &[(f64, f64)]) {
    assert_eq!(got.len(), want.len(), "{what}: {got:?} against {want:?}");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!(near(g.0, w.0) && near(g.1, w.1), "{what}: row {i} is at {g:?}, wanted {w:?} (all: {got:?})");
    }
}

pub(crate) fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

/// The call's params: the shape, the action and whatever else `extra` holds.
pub(crate) fn params(id: u32, action: &str, extra: Value) -> Value {
    let mut p = json!({"id": id, "action": action});
    if let (Some(map), Value::Object(more)) = (p.as_object_mut(), extra) {
        map.extend(more);
    }
    p
}

/// A call that must succeed.
pub(crate) fn act(s: &mut Session, id: u32, action: &str, extra: Value) -> Value {
    run(s, "shape.connection_points", params(id, action, extra))
}

/// A call that must be refused: its words, with the document, the history and the revision as they were.
pub(crate) fn refused(s: &mut Session, p: &Value) -> String {
    let before = snapshot(s);
    let words = s.execute("shape.connection_points", p).unwrap_err().to_string();
    let after = snapshot(s);
    assert_eq!(after.document, before.document, "{p}: a refused call changed the document");
    assert_eq!((after.undo_steps, after.redo_steps, after.revision), (before.undo_steps, before.redo_steps, before.revision), "{p}: no undo step");
    words
}

pub(crate) fn query(s: &mut Session, id: u32) -> Value {
    run(s, "shape.connection_edges", json!({"id": id}))["shapes"][0].clone()
}

pub(crate) fn edge_points(s: &mut Session, id: u32, edge: usize) -> Vec<u64> {
    query(s, id)["edges"][edge]["points"].as_array().unwrap().iter().map(|p| p.as_u64().unwrap()).collect()
}

/// The marker's terms as (`IX`, generation), in the formula's order.
pub(crate) fn marker(s: &Session, id: u32) -> Vec<(u32, u8)> {
    let m = conn_marker::read_in(doc(s), &shape_at(s, id));
    m.terms()
        .iter()
        .map(|t| match &t.row {
            RowName::Number(n) => (n - 1, t.generation),
            other => panic!("a marker term by a name: {other:?}"),
        })
        .collect()
}

pub(crate) fn user_cell(row: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(row), "Value")
}

/// The formula text a cell of shape `id` holds, by inheritance.
pub(crate) fn formula_of(s: &Session, id: u32, addr: &CellAddr) -> Option<String> {
    let found = Resolver::new(doc(s)).resolve(&SheetRef::Shape(shape_at(s, id)), addr)?;
    found.formula.cell.and_then(|c| c.formula.text().map(str::to_owned))
}

pub(crate) fn marker_text(s: &Session, id: u32) -> Option<String> {
    formula_of(s, id, &conn_marker::marker_cell())
}

/// Writes one cell of shape `id` as an edit of the session (an undo step of its own).
pub(crate) fn put_cell(s: &mut Session, id: u32, addr: &CellAddr, cell: Cell) {
    s.edit("test edit", |d| {
        let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), id).unwrap());
        d.write_cell(&sheet, addr, CellEdit::Cell(cell), WriteOrigin::Drawing).unwrap();
        Ok(())
    })
    .unwrap();
}

/// A connection point written by hand as row `ix`: the five cells of a row, constants.
pub(crate) fn put_point(s: &mut Session, id: u32, ix: u32, (x, y): (f64, f64), dir: (f64, f64)) {
    for (name, value, unit) in [("X", x, Some("IN")), ("Y", y, Some("IN")), ("DirX", dir.0, None), ("DirY", dir.1, None), ("Type", 0.0, None)] {
        let mut cell = Cell::constant(CellValue::from_text(&value.to_string()));
        if let Some(unit) = unit {
            cell = cell.with_unit(unit);
        }
        put_cell(s, id, &CellAddr::in_row("Connection", None, RowKey::Ix(ix), name), cell);
    }
}

pub(crate) fn put_marker(s: &mut Session, id: u32, text: &str) {
    put_cell(s, id, &conn_marker::marker_cell(), Cell::with_formula(text, Some(CellValue::from_text("0"))));
}

pub(crate) fn undo_label(s: &Session) -> Option<String> {
    s.history().and_then(|h| h.next_undo_label()).map(str::to_owned)
}

pub(crate) fn undo_steps(s: &Session) -> usize {
    s.history().map_or(0, |h| h.undo_len())
}

/// A rectangle `w` by `h` whose corner rows are shares of the box (`Width`, `Height`, the constant 0), drawn counterclockwise from (0, 0) as the Rectangle master is.
pub(crate) fn share_rectangle(id: u32, w: f64, h: f64) -> ShapeBuilder {
    let corner = |s: ShapeBuilder, ix: u32, kind: &str, x: (&str, f64), y: (&str, f64)| shaped(s, 0, ix, kind, x, y);
    let s = corner(base(id, w, h), 1, "MoveTo", ("", 0.0), ("", 0.0));
    let s = corner(s, 2, "LineTo", ("Width", w), ("", 0.0));
    let s = corner(s, 3, "LineTo", ("Width", w), ("Height", h));
    let s = corner(s, 4, "LineTo", ("", 0.0), ("Height", h));
    corner(s, 5, "LineTo", ("", 0.0), ("", 0.0))
}

/// A `Geometry` row whose `X` and `Y` are each a formula (or a constant, for an empty formula text) and its value.
pub(crate) fn shaped(s: ShapeBuilder, section: u32, ix: u32, kind: &str, x: (&str, f64), y: (&str, f64)) -> ShapeBuilder {
    let key = RowKey::Ix(ix);
    let mut s = s.row_type("Geometry", Some(section), key.clone(), kind);
    for (name, (text, value)) in [("X", x), ("Y", y)] {
        s = if text.is_empty() {
            s.row_cell("Geometry", Some(section), key.clone(), name, &value.to_string())
        } else {
            s.row_formula("Geometry", Some(section), key.clone(), name, text, &value.to_string())
        };
    }
    s
}

/// A circle of radius 1 about (1, 1): one `Ellipse` row from (2, 1) toward (1, 2).
pub(crate) fn circle(id: u32) -> ShapeBuilder {
    row(base(id, 2.0, 2.0), 0, 1, "Ellipse", &[("X", "1"), ("Y", "1"), ("A", "2"), ("B", "1"), ("C", "1"), ("D", "2")])
}

/// A point on the circle at the share `t` of its length from (2, 1) and counterclockwise.
fn on_circle(t: f64) -> (f64, f64) {
    let a = std::f64::consts::TAU * t;
    (1.0 + a.cos(), 1.0 + a.sin())
}

/// The first of the shape's edges: `{edge, kind, before, after, generation}` of the answer's shape `shape`.
pub(crate) fn edge_of(answer: &Value, shape: usize, edge: usize) -> Value {
    answer["shapes"][shape]["edges"][edge].clone()
}

// ---- counts and places: the Rectangle's top ---------------------------------------------------------------------------------------

#[test]
fn the_rectangles_top_goes_one_three_seven_and_halves_back_to_three_to_one_and_then_nothing_more() {
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    let old = points(&s, id);
    assert_places("the Rectangle's points", &places(&s, id), &[(1.0, 1.0), (0.0, 0.5), (1.0, 0.0), (2.0, 0.5)]);
    let steps = undo_steps(&s);

    // 1 -> 3: rows 4 and 5, generation 1; the four old rows are as they were, cell for cell.
    let first = act(&mut s, id, "double", json!({"edge": 2}));
    assert_eq!(first["changed"], true);
    assert_eq!(first["action"], "double");
    assert!((f(&first["min_gap"]) - 0.11811).abs() < 1.0e-5, "{first}");
    assert_eq!(first["shapes"][0]["edges"], json!([{"edge": 2, "kind": "straight", "before": 1, "after": 3, "generation": 1}]));
    assert_eq!(first["shapes"][0]["skipped_edges"], json!([]));
    assert_eq!((first["shapes"][0]["points_before"].clone(), first["shapes"][0]["points_after"].clone()), (json!(4), json!(6)));
    assert_eq!(first["shapes"][0]["skipped"], Value::Null);
    assert_eq!((first["reglued"].clone(), first["renumbered"].clone()), (json!([]), json!(0)));
    assert_eq!(points(&s, id)[..4], old[..], "Old points never move");
    assert_places("after the first doubling", &places(&s, id)[4..], &[(1.5, 1.0), (0.5, 1.0)]);
    assert_eq!(marker(&s, id), [(4, 1), (5, 1)]);
    assert_eq!(edge_points(&mut s, id, 2), [4, 0, 5], "the top's points in their order along it, from (2, 1)");
    assert_eq!(undo_steps(&s), steps + 1);
    assert_eq!(undo_label(&s).as_deref(), Some("Double connection points"));

    // 3 -> 7: rows 6 to 9, generation 2.
    let second = act(&mut s, id, "double", json!({"edge": 2}));
    assert_eq!(second["shapes"][0]["edges"], json!([{"edge": 2, "kind": "straight", "before": 3, "after": 7, "generation": 2}]));
    assert_places("after the second", &places(&s, id)[6..], &[(1.75, 1.0), (1.25, 1.0), (0.75, 1.0), (0.25, 1.0)]);
    assert_eq!(marker(&s, id), [(4, 1), (5, 1), (6, 2), (7, 2), (8, 2), (9, 2)]);

    // Halving takes the newest doubling off first, and never an original.
    let third = act(&mut s, id, "halve", json!({"edge": 2}));
    assert_eq!(third["shapes"][0]["edges"], json!([{"edge": 2, "kind": "straight", "before": 7, "after": 3, "generation": 2}]));
    assert_eq!(places(&s, id).len(), 6);
    assert_eq!(marker(&s, id), [(4, 1), (5, 1)]);
    assert_eq!(undo_label(&s).as_deref(), Some("Halve connection points"));
    act(&mut s, id, "halve", json!({"edge": 2}));
    assert_eq!(points(&s, id), old, "back to the Rectangle's four, cell for cell");
    assert_eq!(marker_text(&s, id), None, "the marker goes with its last term");

    // And then nothing more: refused for a program, in words, with no undo step.
    let words = refused(&mut s, &params(id, "halve", json!({"edge": 2})));
    assert!(words.contains("nothing to halve: the edge has no points a doubling made"), "{words}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_doubling_is_one_undo_step_that_undo_and_redo_take_back_and_do_again() {
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    let old = points(&s, id);
    act(&mut s, id, "double", json!({}));
    let doubled = points(&s, id);
    assert_eq!(doubled.len(), 12);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(points(&s, id), old);
    assert_eq!(marker_text(&s, id), None);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(points(&s, id), doubled);
    assert_eq!(marker(&s, id).len(), 8);
}

#[test]
fn every_edge_of_the_rectangle_gets_one_point_in_the_middle_of_each_portion_and_halves_back_to_four() {
    // No `edge`: every edge. Rows 4 to 11, generation 1, each way out the side's own.
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    let answer = act(&mut s, id, "double", json!({}));
    assert_eq!(answer["shapes"][0]["edges"].as_array().unwrap().len(), 4);
    assert_eq!(edge_of(&answer, 0, 3), json!({"edge": 3, "kind": "straight", "before": 1, "after": 3, "generation": 1}));
    let p = points(&s, id);
    assert_places("V5", &places(&s, id)[4..], &[(0.5, 0.0), (1.5, 0.0), (2.0, 0.25), (2.0, 0.75), (1.5, 1.0), (0.5, 1.0), (0.0, 0.75), (0.0, 0.25)]);
    let dirs: Vec<(f64, f64)> = p[4..].iter().map(|q| q.dir).collect();
    assert_eq!(dirs, [(0.0, -1.0), (0.0, -1.0), (1.0, 0.0), (1.0, 0.0), (0.0, 1.0), (0.0, 1.0), (-1.0, 0.0), (-1.0, 0.0)]);
    assert!(p[4..].iter().all(|q| q.kind == 0.0));
    assert_eq!(marker(&s, id), (4..12).map(|ix| (ix, 1)).collect::<Vec<_>>());
    act(&mut s, id, "halve", json!({}));
    assert_eq!(places(&s, id).len(), 4);
    assert_eq!(marker_text(&s, id), None);
}

// ---- an empty edge, an empty closed edge, the Ellipse, the Diamond ------------------------------------------------------------

#[test]
fn an_empty_open_edge_gets_one_point_in_its_middle_and_halves_to_none() {
    // A 2 by 1 rectangle with no `Connection` section; edge 2 is the top.
    let mut s = session_with(page_of(vec![rectangle(1, 2.0, 1.0)]));
    assert!(points(&s, 1).is_empty());
    let answer = act(&mut s, 1, "double", json!({"edge": 2}));
    assert_eq!(edge_of(&answer, 0, 0), json!({"edge": 2, "kind": "straight", "before": 0, "after": 1, "generation": 1}));
    let p = points(&s, 1);
    assert_eq!(p.len(), 1);
    assert!(near(p[0].x, 1.0) && near(p[0].y, 1.0), "{p:?}");
    assert_eq!(p[0].dir, (0.0, 1.0), "the outward normal: there were no points");
    assert_eq!(marker(&s, 1), [(0, 1)]);
    act(&mut s, 1, "halve", json!({"edge": 2}));
    assert!(points(&s, 1).is_empty());
    assert_eq!(marker_text(&s, 1), None, "no marker row is left");
    let user_left = shape_has_user_row(&s, 1);
    assert!(!user_left, "the User section went with its last row");
}

pub(crate) fn shape_has_user_row(s: &Session, id: u32) -> bool {
    let at = shape_at(s, id);
    doc(s)
        .shape(&at)
        .unwrap()
        .sheet
        .sections
        .iter()
        .any(|sec| sec.name.as_str() == "User" && sec.rows.iter().any(|r| r.name.as_deref() == Some("DiaCraftAddedPoints")))
}

#[test]
fn an_empty_closed_curve_gets_two_points_then_four_then_eight_and_halves_back_to_none() {
    // The Circle with no points. 0 -> 2 (its start and halfway round) -> 4 -> 8.
    let mut s = session_with(page_of(vec![circle(1)]));
    let first = act(&mut s, 1, "double", json!({}));
    assert_eq!(edge_of(&first, 0, 0), json!({"edge": 0, "kind": "closed", "before": 0, "after": 2, "generation": 1}));
    assert_places("0 -> 2", &places(&s, 1), &[(2.0, 1.0), (0.0, 1.0)]);
    let p = points(&s, 1);
    assert_eq!((p[0].dir, p[1].dir), ((1.0, 0.0), (-1.0, 0.0)), "the outward normals of a counterclockwise curve");
    assert_eq!(p[0].x_formula.as_deref(), Some("PNTX(POINTALONGPATH(Geometry1.Path,0,0,1))"));
    assert_eq!(p[1].y_formula.as_deref(), Some("PNTY(POINTALONGPATH(Geometry1.Path,0.5,0,1))"));
    assert_eq!(marker(&s, 1), [(0, 1), (1, 1)]);

    act(&mut s, 1, "double", json!({}));
    assert_places("2 -> 4", &places(&s, 1)[2..], &[(1.0, 2.0), (1.0, 0.0)]);
    let way = points(&s, 1)[2].dir;
    assert!(near(way.0, 0.0) && near(way.1, 1.0), "{way:?}");
    assert_eq!(marker(&s, 1), [(0, 1), (1, 1), (2, 2), (3, 2)]);

    act(&mut s, 1, "double", json!({}));
    let eighths: Vec<(f64, f64)> = [0.125, 0.375, 0.625, 0.875].map(on_circle).to_vec();
    assert_places("4 -> 8", &places(&s, 1)[4..], &eighths);
    assert_eq!(marker(&s, 1)[4..], [(4, 3), (5, 3), (6, 3), (7, 3)]);

    // Halved: 8 -> 4 -> 2 -> 0, the last halving taking both points of generation 1.
    let counts: Vec<usize> = (0..3)
        .map(|_| {
            act(&mut s, 1, "halve", json!({}));
            points(&s, 1).len()
        })
        .collect();
    assert_eq!(counts, [4, 2, 0]);
    assert_eq!(marker_text(&s, 1), None);
}

#[test]
fn diacrafts_ellipse_goes_from_four_points_to_eight_at_the_eighths_and_back() {
    // Its four points at a quarter, a half, three quarters and the start; rows 4 to 7 at the eighths. The ellipse is resized to a circle of radius 1.
    let mut s = blank_session();
    let id = drop_shape(&mut s, "ellipse", 1.0, 1.0);
    run(&mut s, "shape.resize", json!({"id": id, "x": 0.0, "y": 0.0, "width": 2.0, "height": 2.0}));
    let old = points(&s, id);
    assert_eq!(old.len(), 4);
    let answer = act(&mut s, id, "double", json!({}));
    assert_eq!(edge_of(&answer, 0, 0), json!({"edge": 0, "kind": "closed", "before": 4, "after": 8, "generation": 1}));
    let p = points(&s, id);
    assert_eq!(p[..4], old[..]);
    for (row, travel) in p[4..].iter().zip(["0.125", "0.375", "0.625", "0.875"]) {
        assert_eq!(row.x_formula.as_deref(), Some(format!("PNTX(POINTALONGPATH(Geometry1.Path,{travel},0,1))").as_str()));
        assert_eq!(row.y_formula.as_deref(), Some(format!("PNTY(POINTALONGPATH(Geometry1.Path,{travel},0,1))").as_str()));
        let radius = (row.x - 1.0).hypot(row.y - 1.0);
        assert!((radius - 1.0).abs() < 1.0e-4, "a point of the curve: {row:?}");
    }
    assert_eq!(marker(&s, id), [(4, 1), (5, 1), (6, 1), (7, 1)]);
    act(&mut s, id, "halve", json!({}));
    assert_eq!(points(&s, id), old);
}

/// A diamond 2 by 2 with a point at each vertex (rows 0 to 3), which are on no edge.
pub(crate) fn diamond(id: u32) -> ShapeBuilder {
    let mut s = polygon(base(id, 2.0, 2.0), 0, &[(1.0, 0.0), (2.0, 1.0), (1.0, 2.0), (0.0, 1.0)]);
    for (i, (x, y)) in [(1.0, 0.0), (2.0, 1.0), (1.0, 2.0), (0.0, 1.0)].into_iter().enumerate() {
        s = point(s, u32::try_from(i).unwrap(), x, y);
    }
    s
}

#[test]
fn a_diamonds_every_edge_gets_one_point_in_its_middle_and_its_corner_points_are_kept() {
    // 4 -> 8 and back; the vertex points are on no edge.
    let mut s = session_with(page_of(vec![diamond(1)]));
    let old = points(&s, 1);
    act(&mut s, 1, "double", json!({}));
    assert_places("V9", &places(&s, 1)[4..], &[(1.5, 0.5), (1.5, 1.5), (0.5, 1.5), (0.5, 0.5)]);
    assert_eq!(points(&s, 1)[..4], old[..]);
    let way = points(&s, 1)[4].dir;
    assert!(
        near(way.0, std::f64::consts::FRAC_1_SQRT_2) && near(way.1, -std::f64::consts::FRAC_1_SQRT_2),
        "the outward normal of the first side, unit length: {way:?}"
    );
    assert_eq!(query(&mut s, 1)["other"], json!([0, 1, 2, 3]));
    act(&mut s, 1, "halve", json!({}));
    assert_eq!(points(&s, 1), old);
}

#[test]
fn a_clockwise_run_has_its_outward_normal_on_the_other_side_of_the_way_it_goes() {
    // A rectangle drawn clockwise from (0, 0): up, right, down, back. Its top (edge 1) runs from (0, 1) to (2, 1) and still leaves upward; its left side (edge 0) leaves left.
    let s = polygon(base(1, 2.0, 1.0), 0, &[(0.0, 0.0), (0.0, 1.0), (2.0, 1.0), (2.0, 0.0)]);
    let mut s = session_with(page_of(vec![s]));
    act(&mut s, 1, "double", json!({"edge": 1}));
    let p = points(&s, 1);
    assert!(near(p[0].x, 1.0) && near(p[0].y, 1.0), "{p:?}");
    assert_eq!(p[0].dir, (0.0, 1.0));
    act(&mut s, 1, "double", json!({"edge": 0}));
    assert_eq!(points(&s, 1)[1].dir, (-1.0, 0.0));
}

#[test]
fn an_uneven_edge_stays_uneven_and_a_plain_rectangle_gets_constants() {
    // Points on the top of a 4 by 1 rectangle at x = 3.2 and 2.8, 0.2 and 0.3 of the way: five points, the new ones at 3.6, 3.0 and 1.4.
    let uneven = |s: ShapeBuilder| point(point(s, 0, 3.2, 1.0), 1, 2.8, 1.0);
    let mut s = session_with(page_of(vec![uneven(share_rectangle(1, 4.0, 1.0)), uneven(rectangle(2, 4.0, 1.0))]));
    let answer = act(&mut s, 1, "double", json!({"edge": 2}));
    assert_eq!(edge_of(&answer, 0, 0), json!({"edge": 2, "kind": "straight", "before": 2, "after": 5, "generation": 1}));
    assert_places("five points", &places(&s, 1), &[(3.2, 1.0), (2.8, 1.0), (3.6, 1.0), (3.0, 1.0), (1.4, 1.0)]);
    let p = points(&s, 1);
    assert_eq!(
        p[2..].iter().map(|q| q.x_formula.clone().unwrap()).collect::<Vec<_>>(),
        ["Width*0.9", "Width*0.75", "Width*0.35"],
        "ends that are shares of the box give shares"
    );
    assert!(p[2..].iter().all(|q| q.y_formula.as_deref() == Some("Height")));
    // The same edge drawn with plain lengths holds plain lengths: constants in inches.
    act(&mut s, 2, "double", json!({"edge": 2}));
    let q = points(&s, 2);
    assert_places("constants", &places(&s, 2)[2..], &[(3.6, 1.0), (3.0, 1.0), (1.4, 1.0)]);
    assert!(q[2..].iter().all(|r| r.x_formula.is_none() && r.y_formula.is_none()), "{q:?}");
}

// ---- the minimum distance ---------------------------------------------------------------------------------------------

pub(crate) fn bar(id: u32, shares: &[f64]) -> ShapeBuilder {
    edge_with_points(id, 1.0, shares)
}

#[test]
fn a_one_inch_bar_with_a_point_in_the_middle_goes_one_three_seven_and_then_has_no_room() {
    // V11 at 3 mm: 1 -> 3 (6.35 mm gaps) -> 7 (3.175 mm), then the next would make 1.5875 mm.
    let mut s = session_with(page_of(vec![bar(1, &[0.5])]));
    assert_eq!(act(&mut s, 1, "double", json!({}))["shapes"][0]["points_after"], 3);
    assert_eq!(act(&mut s, 1, "double", json!({}))["shapes"][0]["points_after"], 7);
    let words = refused(&mut s, &params(1, "double", json!({})));
    assert!(words.contains("no edge has room") && words.contains("1.6 mm apart") && words.contains("under the minimum of 3 mm (Options)"), "{words}");
    let shape = query(&mut s, 1);
    assert_eq!(shape["edges"][0]["adds"], 0);
    assert!((f(&shape["edges"][0]["gap"]) - 0.0625).abs() < 1.0e-9);
    assert_eq!(shape["can_double"], false);
    assert_eq!(points(&s, 1).len(), 7, "nothing was written");
}

#[test]
fn a_smaller_minimum_lets_the_bar_reach_fifteen_and_a_larger_one_stops_it_at_three() {
    let mut s = session_with(page_of(vec![bar(1, &[0.5])]));
    let small = json!({"min_gap": 1.5 / 25.4});
    let counts: Vec<u64> = (0..3).map(|_| act(&mut s, 1, "double", small.clone())["shapes"][0]["points_after"].as_u64().unwrap()).collect();
    assert_eq!(counts, [3, 7, 15]);
    let words = refused(&mut s, &params(1, "double", small));
    assert!(words.contains("0.8 mm apart") && words.contains("minimum of 1.5 mm"), "{words}");
    // At a quarter inch: 1 -> 3 has gaps of exactly the minimum, which is room; the next, 0.125, is not.
    let mut s = session_with(page_of(vec![bar(1, &[0.5])]));
    assert_eq!(act(&mut s, 1, "double", json!({"min_gap": 0.25}))["shapes"][0]["points_after"], 3);
    refused(&mut s, &params(1, "double", json!({"min_gap": 0.25})));
}

#[test]
fn an_empty_bar_goes_one_three_seven_and_then_is_refused() {
    // V12.
    let mut s = session_with(page_of(vec![bar(1, &[])]));
    let counts: Vec<u64> = (0..3).map(|_| act(&mut s, 1, "double", json!({}))["shapes"][0]["points_after"].as_u64().unwrap()).collect();
    assert_eq!(counts, [1, 3, 7]);
    refused(&mut s, &params(1, "double", json!({})));
}

#[test]
fn a_crowded_portion_stops_the_whole_edge() {
    // Points at 0.3 and 0.45748, 4 mm apart: the middle portion's half is 2 mm, under 3 mm, though its neighbours have room.
    let mut s = session_with(page_of(vec![bar(1, &[0.3, 0.457_48])]));
    let words = refused(&mut s, &params(1, "double", json!({})));
    assert!(words.contains("no edge has room") && words.contains("2 mm apart"), "{words}");
}

#[test]
fn the_minimum_is_measured_on_the_paper() {
    // A 2 metre wall at 1:100 is 20 mm on paper: 0 -> 1 (10 mm) -> 3 (5 mm), then 2.5 mm is refused.
    let wall = 2000.0 / 25.4;
    let page = PageBuilder::new().cell("PageScale", "1").cell("DrawingScale", "100").shape(edge_with_points(1, wall, &[]));
    let mut s = session_with(DocBuilder::new().page(1, page).build());
    let counts: Vec<u64> = (0..2).map(|_| act(&mut s, 1, "double", json!({}))["shapes"][0]["points_after"].as_u64().unwrap()).collect();
    assert_eq!(counts, [1, 3]);
    let words = refused(&mut s, &params(1, "double", json!({})));
    assert!(words.contains("2.5 mm apart"), "{words}");
    // A page scale that is no scale is read as 1:1 and the answer says so once.
    let page = PageBuilder::new().cell("PageScale", "0").cell("DrawingScale", "100").shape(edge_with_points(1, 1.0, &[]));
    let mut s = session_with(DocBuilder::new().page(1, page).build());
    let answer = act(&mut s, 1, "double", json!({}));
    assert!(answer["scale_note"].as_str().is_some_and(|n| n.contains("1:1")), "{answer}");
}

#[test]
fn a_selection_of_two_shapes_doubles_the_one_with_room_and_lists_the_edges_of_the_other() {
    // Shape 3 the Rectangle-like box with four side middles, shape 4 a 0.2 inch square with its four: every edge, default minimum.
    let sides = |s: ShapeBuilder, w: f64, h: f64| {
        let s = point(s, 0, w / 2.0, h);
        let s = point(s, 1, 0.0, h / 2.0);
        let s = point(s, 2, w / 2.0, 0.0);
        point(s, 3, w, h / 2.0)
    };
    let mut s = session_with(page_of(vec![sides(rectangle(3, 2.0, 1.0), 2.0, 1.0), sides(rectangle(4, 0.2, 0.2), 0.2, 0.2)]));
    let steps = undo_steps(&s);
    let answer = run(&mut s, "shape.connection_points", json!({"ids": [3, 4], "action": "double"}));
    assert_eq!(answer["changed"], true);
    assert_eq!(answer["shapes"][0]["points_after"], 12);
    let small = &answer["shapes"][1];
    assert_eq!((small["points_before"].clone(), small["points_after"].clone()), (json!(4), json!(4)));
    assert_eq!(small["edges"], json!([]));
    let skipped = small["skipped_edges"].as_array().unwrap();
    assert_eq!(skipped.len(), 4);
    assert!(skipped.iter().all(|e| (f(&e["gap"]) - 0.05).abs() < 1.0e-9 && (f(&e["min_gap"]) - 0.11811).abs() < 1.0e-5), "{skipped:?}");
    assert_eq!(undo_steps(&s), steps + 1, "one undo step for both");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!((points(&s, 3).len(), points(&s, 4).len()), (4, 4));
}

#[test]
fn the_other_target_edges_with_room_are_doubled_beside_one_without() {
    // One shape with two edges: the bottom has a point in the middle (room), the right side is 0.2 long with one (no room).
    let s = point(point(rectangle(1, 2.0, 0.2), 0, 1.0, 0.0), 1, 2.0, 0.1);
    let mut s = session_with(page_of(vec![s]));
    let answer = act(&mut s, 1, "double", json!({}));
    let shape = &answer["shapes"][0];
    let edges: Vec<u64> = shape["edges"].as_array().unwrap().iter().map(|e| e["edge"].as_u64().unwrap()).collect();
    assert!(edges.contains(&0) && !edges.contains(&1), "{answer}");
    let skipped: Vec<u64> = shape["skipped_edges"].as_array().unwrap().iter().map(|e| e["edge"].as_u64().unwrap()).collect();
    assert!(skipped.contains(&1), "{answer}");
}

#[test]
fn min_gap_is_checked_in_words() {
    let mut s = session_with(page_of(vec![bar(1, &[0.5])]));
    for ok in [0.01, 2.0] {
        // The range's ends are allowed (and may say there is no room; the params are fine).
        let _ = s.execute("shape.connection_points", &params(1, "double", json!({"min_gap": ok})));
    }
    for bad in [json!(0.005), json!(2.5), json!(-1), json!("three")] {
        let words = refused(&mut s, &params(1, "double", json!({"min_gap": bad.clone()})));
        assert!(words.contains("min_gap"), "{bad}: {words}");
    }
}

// ---- the cap ---------------------------------------------------------------------------------------------------------------------

pub(crate) fn crowded(id: u32, points: u32) -> ShapeBuilder {
    let mut s = at(at(base(id, 40.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0), 0, 2, "LineTo", 40.0, 0.0);
    for i in 0..points {
        s = point(s, i, 40.0 * f64::from(i + 1) / f64::from(points + 1), 0.0);
    }
    s
}

#[test]
fn a_shape_whose_only_points_are_127_on_one_open_edge_doubles_to_255_and_with_128_it_is_skipped_by_its_words() {
    let mut s = session_with(page_of(vec![crowded(1, 127), crowded(2, 128)]));
    let answer = act(&mut s, 1, "double", json!({}));
    assert_eq!(answer["shapes"][0]["points_after"], 255);
    let words = refused(&mut s, &params(2, "double", json!({})));
    assert!(words.ends_with("shape 2 would have 257 connection points, past the 256 a connector looks at: it was left as it was"), "{words}");
    // The call that names both changes the first and tells the second's words.
    let mut s = session_with(page_of(vec![crowded(1, 127), crowded(2, 128)]));
    let answer = run(&mut s, "shape.connection_points", json!({"ids": [1, 2], "action": "double"}));
    assert_eq!(answer["shapes"][0]["points_after"], 255);
    assert_eq!(answer["shapes"][1]["skipped"], "shape 2 would have 257 connection points, past the 256 a connector looks at: it was left as it was");
    assert_eq!(points(&s, 2).len(), 128);
}

#[test]
fn the_sum_counts_every_target_edge_of_the_shape() {
    // 127 on the bottom and the other three edges empty: every edge adds 128 + 1 + 1 + 1: 258 in all, skipped; the bottom alone makes 255.
    let mut s = session_with(page_of(vec![{
        let mut r = rectangle(1, 40.0, 1.0);
        for i in 0..127u32 {
            r = point(r, i, 40.0 * f64::from(i + 1) / 128.0, 0.0);
        }
        r
    }]));
    let words = refused(&mut s, &params(1, "double", json!({})));
    assert!(words.contains("would have 258 connection points"), "{words}");
    let answer = act(&mut s, 1, "double", json!({"edge": 0}));
    assert_eq!(answer["shapes"][0]["points_after"], 255);
}

// ---- the marker and the rows ---------------------------------------------------------------------------------------

#[test]
fn the_marker_is_a_user_row_whose_formula_names_each_added_point_and_whose_value_is_nothing() {
    // V20.
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    act(&mut s, id, "double", json!({"edge": 2}));
    assert_eq!(marker_text(&s, id).as_deref(), Some("0*(1*Connections.X5+1*Connections.X6)"));
    let at = shape_at(&s, id);
    let sheet = &doc(&s).shape(&at).unwrap().sheet;
    let marker_row = sheet
        .sections
        .iter()
        .find(|sec| sec.name.as_str() == "User")
        .unwrap()
        .rows
        .iter()
        .find(|r| r.name.as_deref() == Some("DiaCraftAddedPoints"))
        .unwrap();
    assert_eq!(marker_row.cells.keys().map(|k| k.as_str().to_owned()).collect::<Vec<_>>(), ["Value"], "the row holds no other cell");
    let value = marker_row.cells.values().next().unwrap().value.as_ref().unwrap();
    assert_eq!(value.number(), Some(0.0));
    act(&mut s, id, "halve", json!({"edge": 2}));
    assert!(!shape_has_user_row(&s, id));
}

/// The state of the step (b): the Rectangle with its top doubled, then every edge doubled.
pub(crate) fn v21_b() -> (Session, u32) {
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    act(&mut s, id, "double", json!({"edge": 2}));
    act(&mut s, id, "double", json!({}));
    (s, id)
}

#[test]
fn a_sequence_on_the_rectangle_numbers_its_rows_as_the_design_says() {
    // V21 (a) to (c).
    let (mut s, id) = v21_b();
    assert_places(
        "(b)",
        &places(&s, id)[4..],
        &[
            (1.5, 1.0),
            (0.5, 1.0),
            (0.5, 0.0),
            (1.5, 0.0),
            (2.0, 0.25),
            (2.0, 0.75),
            (1.75, 1.0),
            (1.25, 1.0),
            (0.75, 1.0),
            (0.25, 1.0),
            (0.0, 0.75),
            (0.0, 0.25),
        ],
    );
    let gens: Vec<(u32, u8)> = [(4, 1), (5, 1), (6, 1), (7, 1), (8, 1), (9, 1), (10, 2), (11, 2), (12, 2), (13, 2), (14, 1), (15, 1)].to_vec();
    assert_eq!(marker(&s, id), gens);
    // (c): halve every edge: rows 6 to 15 go, rows 0 to 5 as they were, the marker as after (a).
    let before_c = points(&s, id);
    act(&mut s, id, "halve", json!({}));
    assert_eq!(points(&s, id), before_c[..6], "nothing was renumbered");
    assert_eq!(marker(&s, id), [(4, 1), (5, 1)]);
}

#[test]
fn halving_the_top_only_removes_its_newest_generation_and_the_rows_after_it_move_up() {
    // V21 (d): rows 10 to 13 go; rows 14 and 15 become 10 and 11 at the same places.
    let (mut s, id) = v21_b();
    let before = points(&s, id);
    let answer = act(&mut s, id, "halve", json!({"edge": 2}));
    assert_eq!(edge_of(&answer, 0, 0), json!({"edge": 2, "kind": "straight", "before": 7, "after": 3, "generation": 2}));
    let after = points(&s, id);
    assert_eq!(after.len(), 12);
    assert_eq!(after[..10], before[..10]);
    assert_eq!(
        after[10..].iter().map(|p| (p.ix, p.x, p.y)).collect::<Vec<_>>(),
        [(10, 0.0, 0.75), (11, 0.0, 0.25)],
        "the left side's two points moved up and stayed where they were"
    );
    assert_eq!(marker(&s, id), [(4, 1), (5, 1), (6, 1), (7, 1), (8, 1), (9, 1), (10, 1), (11, 1)]);
    assert_eq!(
        marker_text(&s, id).as_deref(),
        Some(
            "0*(1*Connections.X5+1*Connections.X6+1*Connections.X7+1*Connections.X8+1*Connections.X9+1*Connections.X10+1*Connections.X11+1*Connections.X12)"
        )
    );
}

#[test]
fn a_point_added_by_hand_after_a_doubling_is_an_original_and_a_doubling_after_it_inserts_round_it() {
    // The top doubled (rows 4, 5), a row 6 by hand at (1.25, 1).
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    act(&mut s, id, "double", json!({"edge": 2}));
    put_point(&mut s, id, 6, (1.25, 1.0), (0.0, 1.0));
    assert_eq!(marker(&s, id), [(4, 1), (5, 1)], "the hand row is no term: an original on the top");
    assert_eq!(edge_points(&mut s, id, 2), [4, 6, 0, 5]);
    act(&mut s, id, "double", json!({"edge": 2}));
    assert_places("rows 7 to 11", &places(&s, id)[7..], &[(1.75, 1.0), (1.375, 1.0), (1.125, 1.0), (0.75, 1.0), (0.25, 1.0)]);
    // Halve: rows 7 to 11 go; halve again: rows 4 and 5 go and row 6 moves up to 4 and stays; then nothing.
    act(&mut s, id, "halve", json!({"edge": 2}));
    assert_eq!(points(&s, id).len(), 7);
    act(&mut s, id, "halve", json!({"edge": 2}));
    let p = points(&s, id);
    assert_eq!(p.len(), 5);
    assert!(near(p[4].x, 1.25) && near(p[4].y, 1.0), "{:?}", p[4]);
    assert_eq!(marker_text(&s, id), None);
    let words = refused(&mut s, &params(id, "halve", json!({"edge": 2})));
    assert!(words.contains("nothing to halve"), "{words}");
}

#[test]
fn a_moved_added_point_stays_added_on_its_edge_and_one_moved_off_every_edge_stays_for_good() {
    // The last case: row 4's X rewritten to `Width*0.9` (1.8, still on the top): the next halving takes rows 4 and 5.
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    act(&mut s, id, "double", json!({"edge": 2}));
    put_cell(
        &mut s,
        id,
        &CellAddr::in_row("Connection", None, RowKey::Ix(4), "X"),
        Cell::with_formula("Width*0.9", Some(CellValue::from_text("1.8"))).with_unit("IN"),
    );
    act(&mut s, id, "halve", json!({"edge": 2}));
    assert_eq!(points(&s, id).len(), 4);
    // Row 4 moved to (1, 0.5): on no edge, so it belongs to none and the halving of the top takes only row 5.
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    act(&mut s, id, "double", json!({"edge": 2}));
    put_point(&mut s, id, 4, (1.0, 0.5), (0.0, 1.0));
    act(&mut s, id, "halve", json!({"edge": 2}));
    let p = points(&s, id);
    assert_eq!(p.len(), 5);
    assert!(near(p[4].x, 1.0) && near(p[4].y, 0.5));
}

#[test]
fn the_marker_is_read_safely() {
    // V22 on a Rectangle with rows 0 to 5 (the top doubled): which of rows 4 and 5 a halving of the top takes under each marker.
    let cases: [(&str, &[u32]); 9] = [
        ("0*(0*Connections.X5+1*Connections.X6)", &[5]),
        ("0*(256*Connections.X5+1*Connections.X6)", &[5]),
        ("0*(4294967296*Connections.X5+1*Connections.X6)", &[5]),
        ("0*(1*Connections.X5+2*Connections.X5+1*Connections.X6)", &[5]),
        ("0*(1*Connections.X99+1*Connections.X6)", &[5]),
        ("0*(1*Sheet.1!Connections.X5)", &[]),
        ("0*(1*#REF!+1*Connections.X6)", &[]),
        ("1*(1*Connections.X5)", &[]),
        ("0*(1*Connections.Y5)", &[]),
    ];
    for (text, takes) in cases {
        let mut s = blank_session();
        let id = rect(&mut s, 2.0, 1.0);
        act(&mut s, id, "double", json!({"edge": 2}));
        put_marker(&mut s, id, text);
        let p = params(id, "halve", json!({"edge": 2}));
        if takes.is_empty() {
            let words = refused(&mut s, &p);
            assert!(words.contains("nothing to halve"), "{text}: {words}");
        } else {
            let before = places(&s, id);
            run(&mut s, "shape.connection_points", p);
            let after = places(&s, id);
            assert_eq!(after.len(), before.len() - takes.len(), "{text}");
            assert_eq!(after[..5], before[..5], "{text}: row 4 stays, and only row 5 goes");
        }
    }
    // A marker of 257 terms, and one of 9 KiB, are void: every point an original.
    let many = format!("0*({})", vec!["1*Connections.X5"; 257].join("+"));
    let wide = format!("0*(1*Connections.X5+{})", "1*Connections.X6+".repeat(500) + "1*Connections.X6");
    for text in [many, wide] {
        let mut s = blank_session();
        let id = rect(&mut s, 2.0, 1.0);
        act(&mut s, id, "double", json!({"edge": 2}));
        put_marker(&mut s, id, &text);
        let words = refused(&mut s, &params(id, "halve", json!({"edge": 2})));
        assert!(words.contains("nothing to halve"), "{words}");
    }
}

#[test]
fn a_section_keyed_by_name_keeps_its_names_and_a_new_row_takes_the_first_free_one() {
    // A Rectangle whose rows are named Row_1 to Row_4 (the top is Row_1): doubling the top adds Row_5 and Row_6.
    let mut shape = rectangle(1, 2.0, 1.0);
    for (i, (x, y, dx, dy)) in [(1.0, 1.0, 0.0, 1.0), (0.0, 0.5, -1.0, 0.0), (1.0, 0.0, 0.0, -1.0), (2.0, 0.5, 1.0, 0.0)].into_iter().enumerate() {
        let key = RowKey::name(&format!("Row_{}", i + 1));
        for (name, value) in [("X", x), ("Y", y), ("DirX", dx), ("DirY", dy), ("Type", 0.0)] {
            shape = shape.row_cell("Connection", None, key.clone(), name, &value.to_string());
        }
    }
    let mut s = session_with(page_of(vec![shape]));
    act(&mut s, 1, "double", json!({"edge": 2}));
    let p = points(&s, 1);
    assert_eq!(p.iter().map(|q| q.name.clone().unwrap()).collect::<Vec<_>>(), ["Row_1", "Row_2", "Row_3", "Row_4", "Row_5", "Row_6"]);
    assert_places("the new rows", &places(&s, 1)[4..], &[(1.5, 1.0), (0.5, 1.0)]);
    assert_eq!(marker_text(&s, 1).as_deref(), Some("0*(1*Connections.Row_5.X+1*Connections.Row_6.X)"));
    act(&mut s, 1, "halve", json!({"edge": 2}));
    assert_eq!(points(&s, 1).iter().map(|q| q.name.clone().unwrap()).collect::<Vec<_>>(), ["Row_1", "Row_2", "Row_3", "Row_4"]);
}

#[test]
fn a_guarded_point_is_not_removed_and_a_doubling_is_not_stopped_by_one() {
    // The last case: row 5 of a doubled top has `GUARD(Width*0.25)` in X.
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    act(&mut s, id, "double", json!({"edge": 2}));
    put_cell(
        &mut s,
        id,
        &CellAddr::in_row("Connection", None, RowKey::Ix(5), "X"),
        Cell::with_formula("GUARD(Width*0.25)", Some(CellValue::from_text("0.5"))).with_unit("IN"),
    );
    let words = refused(&mut s, &params(id, "halve", json!({"edge": 2})));
    assert!(words.ends_with(&format!("connection point 5 of shape {id} is guarded (GUARD) and was not changed")), "{words}");
    act(&mut s, id, "double", json!({"edge": 2}));
    assert_eq!(points(&s, id).len(), 10, "a doubling moves no point, so a guard does not stop it");
}

// ---- the formulas ---------------------------------------------------------------------------------------------------------

#[test]
fn a_triangles_empty_edge_gets_shares_of_the_box() {
    // (0, 0), (Width, 0), (Width*0.5, Height) at 2 by 1: edge 1 from (2, 0) to (1, 1) gets X `Width*0.75` (1.5) and Y `Height*0.5` (0.5).
    let s = shaped(base(1, 2.0, 1.0), 0, 1, "MoveTo", ("", 0.0), ("", 0.0));
    let s = shaped(s, 0, 2, "LineTo", ("Width", 2.0), ("", 0.0));
    let s = shaped(s, 0, 3, "LineTo", ("Width*0.5", 1.0), ("Height", 1.0));
    let s = shaped(s, 0, 4, "LineTo", ("", 0.0), ("", 0.0));
    let mut s = session_with(page_of(vec![s]));
    act(&mut s, 1, "double", json!({"edge": 1}));
    let p = points(&s, 1);
    assert_eq!(p.len(), 1);
    assert_eq!((p[0].x_formula.as_deref(), p[0].y_formula.as_deref()), (Some("Width*0.75"), Some("Height*0.5")));
    assert!(near(p[0].x, 1.5) && near(p[0].y, 0.5));
}

#[test]
fn width_times_one_and_nought_read_as_shares_and_a_zero_share_is_the_constant() {
    // An edge from (Width*1, Height*0) to (Width*0, Height*0), empty: X `Width*0.5`, Y the constant 0.
    let s = shaped(base(1, 2.0, 1.0), 0, 1, "MoveTo", ("Width*1", 2.0), ("Height*0", 0.0));
    let s = shaped(s, 0, 2, "LineTo", ("Width*0", 0.0), ("Height*0", 0.0));
    let mut s = session_with(page_of(vec![s]));
    act(&mut s, 1, "double", json!({"edge": 0}));
    let p = points(&s, 1);
    assert_eq!((p[0].x_formula.as_deref(), p[0].y_formula.as_deref()), (Some("Width*0.5"), None), "{p:?}");
    assert!(near(p[0].x, 1.0) && near(p[0].y, 0.0));
}

#[test]
fn an_edge_of_plain_lengths_gets_constants_in_inches() {
    // From (0.5, 0.25) to (1.5, 0.25).
    let s = shaped(base(1, 2.0, 1.0), 0, 1, "MoveTo", ("", 0.5), ("", 0.25));
    let s = shaped(s, 0, 2, "LineTo", ("", 1.5), ("", 0.25));
    let mut s = session_with(page_of(vec![s]));
    act(&mut s, 1, "double", json!({"edge": 0}));
    let p = points(&s, 1);
    assert_eq!((p[0].x_formula.clone(), p[0].y_formula.clone()), (None, None));
    assert!(near(p[0].x, 1.0) && near(p[0].y, 0.25));
}

#[test]
fn an_end_that_reads_a_control_gives_row_form_and_the_other_axis_stays_in_share_form() {
    // Row `IX` 3 whose X reads `Controls.X1`, drawn from the pen of row `IX` 2.
    let s = shaped(base(1, 2.0, 1.0), 0, 2, "MoveTo", ("", 0.0), ("Height*0", 0.0));
    let s = shaped(s, 0, 3, "LineTo", ("Controls.X1", 2.0), ("Height", 1.0)).row_cell("Controls", None, RowKey::Ix(0), "X", "2");
    let mut s = session_with(page_of(vec![s]));
    act(&mut s, 1, "double", json!({"edge": 0}));
    let p = points(&s, 1);
    assert_eq!(p[0].x_formula.as_deref(), Some("Geometry1.X2+(Geometry1.X3-Geometry1.X2)*0.5"));
    assert_eq!(p[0].y_formula.as_deref(), Some("Height*0.5"));
    assert!(near(p[0].x, 1.0) && near(p[0].y, 0.5), "{p:?}");
}

#[test]
fn a_piece_of_a_polyline_is_in_share_form_when_its_points_are_shares_and_a_constant_otherwise() {
    // The middle piece of a polyline of three. Absolute vertices: constants; shares of the box: `Width*k` and `Height*k`.
    let absolute = row(
        shaped(base(1, 4.0, 1.0), 0, 1, "MoveTo", ("", 0.0), ("", 0.0)),
        0,
        2,
        "PolylineTo",
        &[("X", "3"), ("Y", "0"), ("A", "POLYLINE(1,1,1,1,2,0)")],
    );
    let shares = row(
        shaped(base(2, 4.0, 1.0), 0, 1, "MoveTo", ("", 0.0), ("", 0.0)),
        0,
        2,
        "PolylineTo",
        &[("X", "4"), ("Y", "0"), ("A", "POLYLINE(0,0,0.25,1,0.5,0.5)")],
    );
    let mut s = session_with(page_of(vec![absolute, shares]));
    act(&mut s, 1, "double", json!({"edge": 1}));
    let p = points(&s, 1);
    assert_eq!((p[0].x_formula.clone(), p[0].y_formula.clone()), (None, None), "constants");
    assert!(near(p[0].x, 1.5) && near(p[0].y, 0.5));
    act(&mut s, 2, "double", json!({"edge": 1}));
    let q = points(&s, 2);
    assert_eq!((q[0].x_formula.as_deref(), q[0].y_formula.as_deref()), (Some("Width*0.375"), Some("Height*0.75")), "{q:?}");
    assert!(near(q[0].x, 1.5) && near(q[0].y, 0.75));
}

#[test]
fn a_resize_after_a_doubling_keeps_straight_points_at_their_shares() {
    // The Rectangle resized to 3 by 1 after the top's first doubling: rows 4 and 5 at (2.25, 1) and (0.75, 1).
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    act(&mut s, id, "double", json!({"edge": 2}));
    run(&mut s, "shape.resize", json!({"id": id, "x": 0.0, "y": 0.0, "width": 3.0, "height": 1.0}));
    assert_places("after the resize", &places(&s, id)[4..], &[(2.25, 1.0), (0.75, 1.0)]);
}

#[test]
fn the_ways_out_and_the_types_follow_the_edges_points_or_its_normal() {
    // The Circle with four points whose ways out differ gives the new ones its outward normals at 45 degrees; with all four ways out nought, nought.
    let circle_with = |id: u32, dirs: [(f64, f64); 4], kinds: [f64; 4]| {
        let mut s = circle(id);
        for (i, (place, dir)) in [(2.0, 1.0), (1.0, 2.0), (0.0, 1.0), (1.0, 0.0)].into_iter().zip(dirs).enumerate() {
            let ix = u32::try_from(i).unwrap();
            s = point(s, ix, place.0, place.1);
            s = s.row_cell("Connection", None, RowKey::Ix(ix), "DirX", &dir.0.to_string()).row_cell(
                "Connection",
                None,
                RowKey::Ix(ix),
                "DirY",
                &dir.1.to_string(),
            );
            s = s.row_cell("Connection", None, RowKey::Ix(ix), "Type", &kinds[i].to_string());
        }
        s
    };
    let out = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)];
    let mut s = session_with(page_of(vec![
        circle_with(1, out, [0.0; 4]),
        circle_with(2, [(0.0, 0.0); 4], [0.0; 4]),
        circle_with(3, out, [0.0, 2.0, 0.0, 2.0]),
    ]));
    act(&mut s, 1, "double", json!({}));
    let r = std::f64::consts::FRAC_1_SQRT_2;
    let dirs: Vec<(f64, f64)> = points(&s, 1)[4..].iter().map(|p| p.dir).collect();
    for (got, want) in dirs.iter().zip([(r, r), (-r, r), (-r, -r), (r, -r)]) {
        // The circle is drawn by 15 cubics, whose tangents are those of the circle to some 1E-5.
        assert!((got.0 - want.0).abs() < 1.0e-4 && (got.1 - want.1).abs() < 1.0e-4, "{dirs:?}");
    }
    act(&mut s, 2, "double", json!({}));
    assert!(points(&s, 2)[4..].iter().all(|p| p.dir == (0.0, 0.0)));
    // Points of Type 0 and 2 give new points Type 0 (the types disagree).
    act(&mut s, 3, "double", json!({}));
    assert!(points(&s, 3)[4..].iter().all(|p| p.kind == 0.0));
    // An open run that encloses nothing gives no way out.
    let open = at(at(at(base(4, 1.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0), 0, 2, "LineTo", 1.0, 0.0), 0, 3, "LineTo", 1.0, 1.0)
        .set(CellAddr::in_section("Geometry", Some(0), "NoFill"), Cell::constant(CellValue::from_text("1")));
    let mut t = session_with(page_of(vec![open]));
    act(&mut t, 4, "double", json!({"edge": 0}));
    let p = points(&t, 4);
    assert!(near(p[0].x, 0.5) && near(p[0].y, 0.0));
    assert_eq!(p[0].dir, (0.0, 0.0));
}

#[test]
fn a_new_row_holds_exactly_x_y_dirx_diry_and_type() {
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    act(&mut s, id, "double", json!({"edge": 2}));
    let at = shape_at(&s, id);
    let section = doc(&s).shape(&at).unwrap().sheet.sections.iter().find(|sec| sec.name.as_str() == "Connection").unwrap().clone();
    assert_eq!(section.rows.len(), 6, "the shape took its own copy of the section: rows 0 to 3 and the two new ones");
    for new in &section.rows[4..] {
        assert_eq!(new.cells.keys().map(|k| k.as_str().to_owned()).collect::<Vec<_>>(), ["DirX", "DirY", "Type", "X", "Y"]);
    }
}

#[test]
fn the_rounded_corners_do_not_change_where_a_straight_edges_points_stand() {
    // Rounding 0.25 on the Rectangle, its top doubled three times (1 -> 3 -> 7 -> 15): the points are shares of the row as written, spaced 0.125 apart.
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    put_cell(&mut s, id, &CellAddr::direct("Rounding"), Cell::constant(CellValue::from_text("0.25")).with_unit("IN"));
    for _ in 0..3 {
        act(&mut s, id, "double", json!({"edge": 2}));
    }
    let p = places(&s, id);
    assert_eq!(p.len(), 4 + 14);
    assert!(p.iter().any(|q| near(q.0, 1.875) && near(q.1, 1.0)), "{p:?}");
}

// ---- path form ---------------------------------------------------------------------------------------------------------------------

/// What calc itself says a formula of shape `id` comes to.
pub(crate) fn calc_says(s: &Session, id: u32, text: &str) -> f64 {
    let sheet = SheetRef::Shape(shape_at(s, id));
    let evaluated = diacraft_calc::evaluate_text(doc(s), &sheet, None, text, &diacraft_calc::Budget::with_steps(10_000_000));
    match evaluated.result {
        diacraft_calc::EvalResult::Value(diacraft_calc::Value::Number(q)) => q.stored,
        other => panic!("{text}: {other:?}"),
    }
}

#[test]
fn a_path_form_points_value_is_calcs_own_and_within_a_millionth_of_the_curve() {
    let mut s = session_with(page_of(vec![circle(1)]));
    act(&mut s, 1, "double", json!({}));
    act(&mut s, 1, "double", json!({}));
    for p in points(&s, 1) {
        let (xf, yf) = (p.x_formula.clone().unwrap(), p.y_formula.clone().unwrap());
        assert!((calc_says(&s, 1, &xf) - p.x).abs() < 1.0e-9, "{xf}");
        assert!((calc_says(&s, 1, &yf) - p.y).abs() < 1.0e-9, "{yf}");
    }
}

#[test]
fn a_resize_keeps_a_curves_points_on_the_curve_at_the_same_share() {
    // The Ellipse resized to 3 by 1: every path-form point within 1E-9 of POINTALONGPATH's place for its share.
    let mut s = blank_session();
    let id = drop_shape(&mut s, "ellipse", 1.0, 1.0);
    act(&mut s, id, "double", json!({}));
    run(&mut s, "shape.resize", json!({"id": id, "x": 0.0, "y": 0.0, "width": 3.0, "height": 1.0}));
    for p in &points(&s, id)[4..] {
        let (xf, yf) = (p.x_formula.clone().unwrap(), p.y_formula.clone().unwrap());
        assert!((calc_says(&s, id, &xf) - p.x).abs() < 1.0e-9, "{xf}");
        assert!((calc_says(&s, id, &yf) - p.y).abs() < 1.0e-9, "{yf}");
        // And on the ellipse: x²/1.5² + y²/0.5² = 1 about the centre (1.5, 0.5), to the accuracy of its drawing.
        let (dx, dy) = (p.x - 1.5, p.y - 0.5);
        assert!(((dx / 1.5).powi(2) + (dy / 0.5).powi(2) - 1.0).abs() < 1.0e-3, "{p:?}");
    }
}

#[test]
fn a_half_discs_arc_gets_a_point_at_its_apex_and_its_chord_a_share() {
    // A move to (0, 0), an arc to (2, 0) with a bow of 1 and a line back: edge 0 the arc, edge 1 the chord.
    let s = at(base(1, 2.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0);
    let s = row(s, 0, 2, "ArcTo", &[("X", "2"), ("Y", "0"), ("A", "1")]);
    let s = at(s, 0, 3, "LineTo", 0.0, 0.0);
    let mut s = session_with(page_of(vec![s]));
    let edges = run(&mut s, "shape.connection_edges", json!({"id": 1}))["shapes"][0]["edges"].clone();
    assert_eq!((edges[0]["kind"].clone(), edges[1]["kind"].clone()), (json!("curved"), json!("straight")));
    act(&mut s, 1, "double", json!({"edge": 0}));
    let p = points(&s, 1);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].x_formula.as_deref(), Some("PNTX(POINTALONGPATH(Geometry1.Path,0.5,0,1))"));
    assert!(near(p[0].x, 1.0) && (p[0].y.abs() - 1.0).abs() < 1.0e-6, "the apex: {p:?}");
}

#[test]
fn a_nurbs_edge_is_doubled_in_path_form_by_its_drawn_row_and_the_value_is_calcs_own() {
    // A freehand curve is a move and one `NURBSTo` row: its drawn row is 1, since moves are left out.
    let mut s = blank_session();
    let wave: Vec<(f64, f64)> = (0..120).map(|i| (1.0 + f64::from(i) * 0.05, 6.0 + (f64::from(i) * 0.1).sin())).collect();
    let id = crate::edit::id_of(&run(&mut s, "shape.draw", json!({"page": 1, "tool": "freeform", "points": wave})));
    let edges = query(&mut s, id)["edges"].clone();
    assert_eq!(edges.as_array().unwrap().len(), 1, "{edges}");
    act(&mut s, id, "double", json!({}));
    let p = points(&s, id);
    assert_eq!(p.len(), 1);
    let (xf, yf) = (p[0].x_formula.clone().unwrap(), p[0].y_formula.clone().unwrap());
    assert!(xf.starts_with("PNTX(POINTALONGPATH(Geometry1.Path,0.5,0,1))"), "{xf}");
    assert!((calc_says(&s, id, &xf) - p[0].x).abs() < 1.0e-9 && (calc_says(&s, id, &yf) - p[0].y).abs() < 1.0e-9);
}

// ---- masters -----------------------------------------------------------------------------------------------------------------------

/// A master whose one shape is a 2 by 1 rectangle with the four side middles as constants, and an instance of it (shape 5) at the origin.
pub(crate) fn instance_page(master_points: u32, extra: impl FnOnce(ShapeBuilder) -> ShapeBuilder) -> Session {
    let mut shape = rectangle(1, 2.0, 1.0);
    let sides =
        [(1.0, 1.0, 0.0, 1.0), (0.0, 0.5, -1.0, 0.0), (1.0, 0.0, 0.0, -1.0), (2.0, 0.5, 1.0, 0.0), (1.5, 1.0, 0.0, 1.0), (0.5, 1.0, 0.0, 1.0)];
    for (i, (x, y, dx, dy)) in sides.into_iter().take(master_points as usize).enumerate() {
        let ix = u32::try_from(i).unwrap();
        shape = point(shape, ix, x, y).row_cell("Connection", None, RowKey::Ix(ix), "DirX", &dx.to_string()).row_cell(
            "Connection",
            None,
            RowKey::Ix(ix),
            "DirY",
            &dy.to_string(),
        );
    }
    let shape = extra(shape);
    let instance = ShapeBuilder::new(5).master(1).cell("PinX", "0").cell("PinY", "0");
    let built = DocBuilder::new().master(1, MasterBuilder::new().shape(shape)).page(1, PageBuilder::new().shape(instance)).build();
    session_with(built)
}

pub(crate) fn own_connection_rows(s: &Session, id: u32) -> Vec<(Option<u32>, bool)> {
    let at = shape_at(s, id);
    doc(s)
        .shape(&at)
        .unwrap()
        .sheet
        .sections
        .iter()
        .find(|sec| sec.name.as_str() == "Connection")
        .map(|sec| sec.rows.iter().map(|r| (r.ix, r.del)).collect())
        .unwrap_or_default()
}

#[test]
fn an_instance_takes_its_own_copy_of_the_points_when_its_top_is_doubled_and_the_masters_later_edit_does_not_reach_it() {
    let mut s = instance_page(4, |m| m);
    assert!(own_connection_rows(&s, 5).is_empty(), "the instance holds no point of its own");
    act(&mut s, 5, "double", json!({"edge": 2}));
    let own = own_connection_rows(&s, 5);
    assert_eq!(own, (0..6).map(|ix| (Some(ix), false)).collect::<Vec<_>>(), "rows 0 to 5 of its own, no deleted row");
    assert_places("the instance", &places(&s, 5), &[(1.0, 1.0), (0.0, 0.5), (1.0, 0.0), (2.0, 0.5), (1.5, 1.0), (0.5, 1.0)]);
    // The master's row 0 edited afterwards does not reach the instance, which holds a row of its own there.
    s.edit("test edit", |d| {
        let sheet = SheetRef::Shape(d.find_shape(Owner::Master(1), 1).unwrap());
        d.write_cell(
            &sheet,
            &CellAddr::in_row("Connection", None, RowKey::Ix(0), "X"),
            CellEdit::Cell(Cell::constant(CellValue::from_text("0.4")).with_unit("IN")),
            WriteOrigin::Drawing,
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    assert!(near(points(&s, 5)[0].x, 1.0));
}

#[test]
fn an_instance_that_deleted_a_row_of_the_master_gets_new_rows_over_it_with_every_cell() {
    // The instance's own row 3 is deleted (effective rows 0 to 2): doubling its top writes rows 3 and 4 of its own.
    let instance = ShapeBuilder::new(5).master(1).cell("PinX", "0").cell("PinY", "0").del_row("Connection", None, RowKey::Ix(3));
    let mut shape = rectangle(1, 2.0, 1.0);
    for (i, (x, y)) in [(1.0, 1.0), (0.0, 0.5), (1.0, 0.0), (2.0, 0.5)].into_iter().enumerate() {
        shape = point(shape, u32::try_from(i).unwrap(), x, y);
    }
    let built = DocBuilder::new().master(1, MasterBuilder::new().shape(shape)).page(1, PageBuilder::new().shape(instance)).build();
    let mut s = session_with(built);
    assert_eq!(points(&s, 5).len(), 3);
    act(&mut s, 5, "double", json!({"edge": 2}));
    assert_places("rows 0 to 4", &places(&s, 5), &[(1.0, 1.0), (0.0, 0.5), (1.0, 0.0), (1.5, 1.0), (0.5, 1.0)]);
    let own = own_connection_rows(&s, 5);
    assert!(own.iter().all(|(_, del)| !del), "no row is left deleted: {own:?}");
}

#[test]
fn an_instance_that_deleted_the_masters_whole_section_does_not_get_its_rows_back_under_the_new_ones() {
    // The instance's own section is deleted (`Del`), so it holds no point; doubling its top gives it one row of its own, and the master's other three stay hidden by deleted rows.
    let instance = ShapeBuilder::new(5).master(1).cell("PinX", "0").cell("PinY", "0").del_section("Connection", None);
    let mut shape = rectangle(1, 2.0, 1.0);
    for (i, (x, y)) in [(1.0, 1.0), (0.0, 0.5), (1.0, 0.0), (2.0, 0.5)].into_iter().enumerate() {
        shape = point(shape, u32::try_from(i).unwrap(), x, y);
    }
    let built = DocBuilder::new().master(1, MasterBuilder::new().shape(shape)).page(1, PageBuilder::new().shape(instance)).build();
    let mut s = session_with(built);
    assert!(points(&s, 5).is_empty(), "the master's points are hidden by the instance's deleted section");
    act(&mut s, 5, "double", json!({"edge": 2}));
    assert_places("the one point", &places(&s, 5), &[(1.0, 1.0)]);
    assert_eq!(own_connection_rows(&s, 5), vec![(Some(0), false), (Some(1), true), (Some(2), true), (Some(3), true)]);
}

#[test]
fn halving_an_instance_that_inherits_a_doubled_master_writes_deleted_rows_and_leaves_the_master_alone() {
    // A master whose own top was doubled (rows 0 to 5, its marker naming 4 and 5), and an instance inheriting all of it.
    let mut s = instance_page(6, |m| {
        m.set(user_cell("DiaCraftAddedPoints"), Cell::with_formula("0*(1*Connections.X5+1*Connections.X6)", Some(CellValue::from_text("0"))))
    });
    assert_eq!(marker(&s, 5), [(4, 1), (5, 1)], "the instance reads the master's marker");
    let master_before = doc(&s).master(1).cloned();
    act(&mut s, 5, "halve", json!({"edge": 2}));
    assert_eq!(places(&s, 5).len(), 4);
    let own = own_connection_rows(&s, 5);
    assert_eq!(
        own,
        vec![(Some(0), false), (Some(1), false), (Some(2), false), (Some(3), false), (Some(4), true), (Some(5), true)],
        "rows 0 to 3 of its own and deleted rows for 4 and 5"
    );
    assert_eq!(marker(&s, 5), Vec::<(u32, u8)>::new(), "its effective marker names no point");
    assert_eq!(doc(&s).master(1).cloned(), master_before, "the master is unchanged");
}

// ---- the command: edge, at, ids, refusals -----------------------------------------------------------------------------

#[test]
fn the_answer_of_a_call_on_the_top_edge_is_as_the_spec_says() {
    // V37.
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    let answer = act(&mut s, id, "double", json!({"edge": 2}));
    assert_eq!(answer["changed"], true);
    assert_eq!(answer["page"], 1);
    assert_eq!(answer["shapes"][0]["id"], id);
    assert_eq!(answer["shapes"][0]["edges"], json!([{"edge": 2, "kind": "straight", "before": 1, "after": 3, "generation": 1}]));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(places(&s, id).len(), 4);
    assert_eq!(marker_text(&s, id), None);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(places(&s, id).len(), 6);
}

#[test]
fn at_names_the_edge_nearest_a_place_within_its_reach_and_every_edge_when_none_is() {
    // `at` (1, 1.03) with the default reach doubles edge 2 only; (1, 0.5) has no edge within reach: every edge.
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    let answer = act(&mut s, id, "double", json!({"at": {"x": 1.0, "y": 1.03}}));
    assert_eq!(answer["shapes"][0]["edges"].as_array().unwrap().len(), 1);
    assert_eq!(answer["shapes"][0]["edges"][0]["edge"], 2);
    let answer = act(&mut s, id, "double", json!({"at": {"x": 1.0, "y": 0.5}}));
    assert_eq!(answer["shapes"][0]["edges"].as_array().unwrap().len(), 4);
    // A reach wide enough to hold the edge from the middle picks the nearest one: the top from (1, 0.9).
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    let answer = act(&mut s, id, "double", json!({"at": {"x": 1.0, "y": 0.9}, "reach": 0.2}));
    assert_eq!(answer["shapes"][0]["edges"][0]["edge"], 2);
}

#[test]
fn the_calls_params_are_refused_in_words_and_change_nothing() {
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    let other = rect(&mut s, 2.0, 1.0);
    let words = refused(&mut s, &params(id, "set_count", json!({})));
    assert!(words.contains("set_count was dropped: double or halve"), "{words}");
    // With the `count` it once took, the registry's own check of the keys refuses first, in words that name what the command takes.
    let words = refused(&mut s, &params(id, "set_count", json!({"count": 5})));
    assert!(words.contains("unknown parameter `count`"), "{words}");
    let words = refused(&mut s, &params(id, "triple", json!({})));
    assert!(words.contains("`action` must be `double` or `halve`, not `triple`"), "{words}");
    assert!(refused(&mut s, &json!({"id": id})).contains("missing `action`"));
    assert!(refused(&mut s, &json!({"action": "double"})).contains("give `id` or `ids`"));
    assert!(refused(&mut s, &params(id, "double", json!({"edge": 4}))).contains("no edge 4"), "past the last edge");
    let words = refused(&mut s, &json!({"ids": [id, other], "action": "double", "edge": 0}));
    assert!(words.contains("`edge` names an edge of one shape"), "{words}");
    let words = refused(&mut s, &params(id, "double", json!({"edge": 0, "at": {"x": 1, "y": 1}})));
    assert!(words.contains("not both"), "{words}");
    let words = refused(&mut s, &params(id, "double", json!({"at": {"x": "one", "y": 1}})));
    assert!(words.contains("`at` must be an object {x, y}"), "{words}");
    let words = refused(&mut s, &params(id, "double", json!({"reach": 0.1})));
    assert!(words.contains("`reach` goes with `at`"), "{words}");
    let words = refused(&mut s, &params(id, "double", json!({"at": {"x": 1, "y": 1}, "reach": -1})));
    assert!(words.contains("`reach` must be a number of inches more than 0"), "{words}");
    assert!(refused(&mut s, &params(id, "double", json!({"min_gap": 3}))).contains("min_gap"));
    let too_many: Vec<u32> = (0..4097).collect();
    assert!(refused(&mut s, &json!({"ids": too_many, "action": "double"})).contains("between 1 and 4096"));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_is_skipped_by_its_words_beside_a_rectangle_and_refused_alone() {
    // V2.
    let mut s = blank_session();
    let a = rect(&mut s, 2.0, 1.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 6.0);
    let connector = crate::edit::id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    let steps = undo_steps(&s);
    let answer = run(&mut s, "shape.connection_points", json!({"ids": [connector, a], "action": "double"}));
    assert_eq!(
        answer["shapes"][0]["skipped"],
        format!("shape {connector} is a line or a connector: connection points are put on two-dimensional shapes")
    );
    assert_eq!(answer["shapes"][1]["points_after"], 12);
    assert_eq!(undo_steps(&s), steps + 1);
    let words = refused(&mut s, &json!({"ids": [connector], "action": "double"}));
    assert!(words.contains("a line or a connector"), "{words}");
}

#[test]
fn a_group_and_a_text_box_are_refused_in_words() {
    let mut s = blank_session();
    let a = rect(&mut s, 2.0, 1.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 6.0);
    let group = crate::edit::id_of(&run(&mut s, "shape.group", json!({"ids": [a, b]})));
    let words = refused(&mut s, &params(group, "double", json!({})));
    assert!(words.ends_with(&format!("shape {group} is a group: select a shape inside it to change its connection points")), "{words}");
    // A member of the group is a shape like any other.
    let answer = act(&mut s, a, "double", json!({"edge": 2}));
    assert_eq!(answer["changed"], true);
    let mut t = session_with(page_of(vec![ShapeBuilder::new(4).cell("PinX", "1").cell("PinY", "1").cell("Width", "2").cell("Height", "1")]));
    let words = refused(&mut t, &params(4, "double", json!({})));
    assert!(words.ends_with("shape 4 draws no outline: it has no edge to put connection points on"), "{words}");
}

#[test]
fn the_command_is_in_the_registry_on_home_tools() {
    let s = blank_session();
    let listed = s.commands();
    let found = listed.iter().find(|c| c.id == "shape.connection_points").expect("registered");
    assert_eq!(found.menu, ["Home", "Tools"]);
    assert!(found.shortcut.is_none(), "the window's command carries the keys, not this one");
}

#[test]
fn the_same_shape_named_twice_is_counted_once() {
    // `ids` naming a shape twice does not double it twice.
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    run(&mut s, "shape.connection_points", json!({"ids": [id, id], "action": "double"}));
    assert_eq!(places(&s, id).len(), 12);
}

#[test]
fn halving_a_shape_with_nothing_to_halve_beside_one_with_something_is_told_and_changes_the_other() {
    let mut s = blank_session();
    let a = rect(&mut s, 2.0, 1.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 6.0);
    act(&mut s, a, "double", json!({}));
    let answer = run(&mut s, "shape.connection_points", json!({"ids": [a, b], "action": "halve"}));
    assert_eq!(answer["shapes"][0]["points_after"], 4);
    assert!(answer["shapes"][1]["skipped"].as_str().is_some_and(|w| w.contains("nothing to halve")), "{answer}");
}
