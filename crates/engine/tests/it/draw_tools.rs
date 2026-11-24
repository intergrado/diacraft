//! The drawing tools: `shape.draw` makes a shape with real geometry rows from the points of a drag, or goes on from the end vertex of a path, and `shape.close` closes an open
//! path, through `Session::execute` as the control channel, the CLI and MCP call it. Lengths are paper inches on pages of the default scale, where they are the cells' own.

use diacraft_doc::{CellAddr, Document, Links, Lookup, Owner, Resolver, SectionKey, SheetRef};
use diacraft_engine::draw::connection_points;
use diacraft_engine::{EngineError, Session};
use diacraft_geom::{build_path, kurbo};
use serde_json::{Value as Json, json};

use crate::edit::{doc, id_of, num};
use crate::native_common::{Dir, opened};
use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

/// The id of the first page of the drawing: 1 for a drawing made here, 0 for one read from an `.odg`.
fn first_page(d: &Document) -> u32 {
    d.pages.get(0).map_or(1, |p| p.id)
}

/// What a cell of shape `id` on the first page is now, by inheritance.
fn num_of(d: &Document, id: u32, cell: &str) -> f64 {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap();
    Resolver::new(d)
        .resolve(&SheetRef::Shape(at), &CellAddr::direct(cell))
        .and_then(|found| found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()))
        .unwrap_or_else(|| panic!("shape {id}: {cell} holds no number"))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

fn near_p(a: (f64, f64), b: (f64, f64)) -> bool {
    near(a.0, b.0) && near(a.1, b.1)
}

fn shape_of(d: &Document, id: u32) -> &diacraft_doc::Shape {
    d.shape(&d.find_shape(Owner::Page(first_page(d)), id).unwrap()).unwrap()
}

/// What shape `id` holds locally in a cell: its formula (if any) and its value text.
fn local(d: &Document, id: u32, cell: &str) -> Option<(Option<String>, Option<String>)> {
    match shape_of(d, id).sheet.lookup(&CellAddr::direct(cell)) {
        Lookup::Found(c) => Some((c.formula.text().map(str::to_owned), c.value.as_ref().map(|v| v.text().to_owned()))),
        _ => None,
    }
}

/// The resolved rows of shape `id`'s first `Geometry` section: the row type and each cell's value, by name.
fn rows(d: &Document, id: u32) -> Vec<(String, std::collections::BTreeMap<String, f64>)> {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap();
    let res = Resolver::new(d);
    let found = res.effective_rows(&SheetRef::Shape(at), &SectionKey::new("Geometry", Some(0))).expect("a Geometry section");
    found
        .iter()
        .map(|row| {
            let cells = row.cells.iter().filter_map(|(n, r)| Some((n.as_str().to_owned(), r.value.cell?.value.as_ref()?.number()?))).collect();
            (row.row_type.as_deref().unwrap_or("").to_owned(), cells)
        })
        .collect()
}

fn kinds(d: &Document, id: u32) -> Vec<String> {
    rows(d, id).into_iter().map(|(kind, _)| kind).collect()
}

/// The first `Geometry` section's flag `name` (`NoFill` and the others): a flag no cell holds is 0, the default.
fn flag(d: &Document, id: u32, name: &str) -> f64 {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap();
    Resolver::new(d)
        .resolve(&SheetRef::Shape(at), &CellAddr::in_section("Geometry", Some(0), name))
        .and_then(|found| found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()))
        .unwrap_or(0.0)
}

/// A cell of shape `id` as a number: a cell no sheet holds is 0, the default (the pose cells a format does not write when they are 0: `Angle`, `FlipX`, `FlipY`).
fn num0(d: &Document, id: u32, cell: &str) -> f64 {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap();
    Resolver::new(d)
        .resolve(&SheetRef::Shape(at), &CellAddr::direct(cell))
        .and_then(|found| found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()))
        .unwrap_or(0.0)
}

/// The path shape `id` draws, in inches on the page: the rows as the renderer builds them, carried through the shape's pose.
fn on_page(d: &Document, id: u32) -> Vec<(f64, f64)> {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap();
    let res = Resolver::new(d);
    let effective = res.effective_rows(&SheetRef::Shape(at), &SectionKey::new("Geometry", Some(0))).expect("a Geometry section");
    let (geom, _) = diacraft_render::geom_rows(&effective);
    let path = build_path(&geom, kurbo::Size::new(num_of(d, id, "Width"), num_of(d, id, "Height"))).unwrap();
    let pose = diacraft_engine::draw::Pose {
        pin: (num_of(d, id, "PinX"), num_of(d, id, "PinY")),
        loc: (num_of(d, id, "LocPinX"), num_of(d, id, "LocPinY")),
        angle: num0(d, id, "Angle"),
        flip_x: num0(d, id, "FlipX") != 0.0,
        flip_y: num0(d, id, "FlipY") != 0.0,
    };
    let mut out = Vec::new();
    kurbo::flatten(path.elements().iter().copied(), 0.001, |el| {
        if let kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) = el {
            out.push(pose.up((p.x, p.y)));
        }
    });
    out
}

fn draw(s: &mut Session, tool: &str, points: Json) -> Json {
    run(s, "shape.draw", json!({"page": 1, "tool": tool, "points": points}))
}

#[test]
fn a_rectangle_is_a_shape_with_no_master_whose_rows_are_shares_of_its_box_and_whose_sides_can_be_glued_to() {
    let mut s = blank_session();
    let r = draw(&mut s, "rectangle", json!([[1, 2], [4, 3.5]]));
    assert_eq!(r, json!({"page": 1, "id": 1, "tool": "rectangle", "rows": 5, "closed": true, "continued": false, "width": 3.0, "height": 1.5}));
    let d = doc(&s);
    let shape = shape_of(d, 1);
    assert_eq!((shape.master, shape.master_shape), (None, None), "No master");
    assert!(d.masters.is_empty(), "and none is added to the drawing");
    assert_eq!(shape.sheet.links, Links { line: Some(0), fill: Some(0), text: Some(0) }, "linked to the root style, as the shapes of files are");
    for (name, formula, value) in [
        ("PinX", None, "2.5"),
        ("PinY", None, "2.75"),
        ("Width", None, "3"),
        ("Height", None, "1.5"),
        ("LocPinX", Some("Width*0.5"), "1.5"),
        ("LocPinY", Some("Height*0.5"), "0.75"),
        ("Angle", None, "0"),
        ("FlipX", None, "0"),
        ("TxtWidth", Some("Width*1"), "3"),
    ] {
        assert_eq!(local(d, 1, name), Some((formula.map(str::to_owned), Some(value.to_owned()))), "{name}");
    }
    // The rows are the Rectangle master's: a move and four lines back to the start, each coordinate a share of the box.
    assert_eq!(kinds(d, 1), ["MoveTo", "LineTo", "LineTo", "LineTo", "LineTo"]);
    let rows = rows(d, 1);
    let xy = |i: usize| (rows[i].1["X"], rows[i].1["Y"]);
    assert_eq!([xy(0), xy(1), xy(2), xy(3), xy(4)], [(0.0, 0.0), (3.0, 0.0), (3.0, 1.5), (0.0, 1.5), (0.0, 0.0)]);
    let section = shape_of(d, 1).sheet.sections.iter().find(|sec| sec.name.as_str() == "Geometry").unwrap();
    let formula_of = |row: usize, cell: &str| {
        section.rows.get(row).unwrap().cells.get(&diacraft_doc::CellName::new(cell)).unwrap().formula.text().map(str::to_owned)
    };
    assert_eq!((formula_of(1, "X"), formula_of(2, "Y"), formula_of(1, "Y")), (Some("Width".to_owned()), Some("Height".to_owned()), None));
    assert_eq!(flag(d, 1, "NoFill"), 0.0, "a closed path takes the fill");
    let points = connection_points(d, 1, 1);
    assert_eq!(points.len(), 4);
    let ends = on_page(d, 1);
    assert!(near_p(ends[0], (1.0, 2.0)) && near_p(ends[2], (4.0, 3.5)), "{ends:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn an_ellipse_is_the_ellipse_row_of_the_master_and_shift_makes_the_circle() {
    let mut s = blank_session();
    draw(&mut s, "ellipse", json!([[1, 1], [5, 3]]));
    let d = doc(&s);
    assert_eq!(kinds(d, 1), ["Ellipse"]);
    let row = &rows(d, 1)[0].1;
    assert_eq!([row["X"], row["Y"], row["A"], row["B"], row["C"], row["D"]], [2.0, 1.0, 4.0, 1.0, 2.0, 2.0]);
    assert_eq!(flag(d, 1, "NoFill"), 0.0);
    assert_eq!(connection_points(d, 1, 1).len(), 4);
    let r = run(&mut s, "shape.draw", json!({"page": 1, "tool": "ellipse", "points": [[6, 1], [10, 3]], "shift": true}));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(4.0), Some(4.0)), "a circle");
    assert!(near(num(doc(&s), 2, "PinX"), 8.0) && near(num(doc(&s), 2, "PinY"), 3.0));
}

#[test]
fn a_line_is_an_open_path_that_takes_no_fill_and_has_a_box_round_its_two_ends() {
    let mut s = blank_session();
    let r = draw(&mut s, "line", json!([[4, 5], [1, 1]]));
    assert_eq!((r["rows"].as_u64(), r["closed"].as_bool(), r["width"].as_f64(), r["height"].as_f64()), (Some(2), Some(false), Some(3.0), Some(4.0)));
    let d = doc(&s);
    assert_eq!(kinds(d, 1), ["MoveTo", "LineTo"]);
    let rows = rows(d, 1);
    // From the upper right to the lower left: the first vertex is at the box's top right.
    assert_eq!(((rows[0].1["X"], rows[0].1["Y"]), (rows[1].1["X"], rows[1].1["Y"])), ((3.0, 4.0), (0.0, 0.0)));
    assert_eq!(flag(d, 1, "NoFill"), 1.0, "an open path cannot take a fill");
    assert!(connection_points(d, 1, 1).is_empty());
    assert_eq!(on_page(d, 1).iter().map(|p| (p.0.round(), p.1.round())).collect::<Vec<_>>(), [(4.0, 5.0), (1.0, 1.0)]);
    // A level line has a box of no height: its rows are constants where there is nothing to be a share of.
    let level = draw(&mut s, "line", json!([[1, 7], [5, 7]]));
    assert_eq!((level["width"].as_f64(), level["height"].as_f64()), (Some(4.0), Some(0.0)));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn an_arc_is_a_move_and_an_arc_row_bowed_to_the_left_of_the_way_the_pointer_went() {
    let mut s = blank_session();
    let r = draw(&mut s, "arc", json!([[1, 1], [5, 1]]));
    let d = doc(&s);
    assert_eq!(kinds(d, 1), ["MoveTo", "ArcTo"]);
    let arc = &rows(d, 1)[1].1;
    assert!(near(arc["A"], -0.207_106_781 * 4.0), "{arc:?}");
    // The curve arches over the chord: its box is about a fifth of the chord high, with the chord along the bottom.
    assert!(near(r["width"].as_f64().unwrap(), 4.0) && (r["height"].as_f64().unwrap() - 0.828).abs() < 0.01, "{r}");
    let path = on_page(d, 1);
    assert!(path.iter().all(|p| p.1 >= 1.0 - 1.0e-6) && path.iter().any(|p| p.1 > 1.7), "{path:?}");
    assert_eq!(flag(d, 1, "NoFill"), 1.0);
}

#[test]
fn the_pencil_turns_a_stroke_into_lines_and_an_arc_and_the_freeform_tool_into_one_nurbs_row() {
    let mut s = blank_session();
    // A line along the bottom, then half a circle round the right: a line and an arc.
    let mut stroke: Vec<(f64, f64)> = (0..=30).map(|i| (1.0 + 3.0 * f64::from(i) / 30.0, 1.0)).collect();
    stroke.extend((1..=40).map(|i| {
        let a = (-90.0 + 180.0 * f64::from(i) / 40.0_f64).to_radians();
        (4.0 + a.cos(), 2.0 + a.sin())
    }));
    let r = draw(&mut s, "pencil", json!(stroke));
    assert_eq!((r["tool"].as_str(), r["rows"].as_u64()), (Some("pencil"), Some(3)));
    assert_eq!(kinds(doc(&s), 1), ["MoveTo", "LineTo", "ArcTo"]);
    // A smooth wave drawn freehand: a move and one NURBSTo row of degree 3.
    let wave: Vec<(f64, f64)> = (0..120).map(|i| (1.0 + f64::from(i) * 0.05, 6.0 + (f64::from(i) * 0.1).sin())).collect();
    let r = draw(&mut s, "freeform", json!(wave));
    assert_eq!(r["rows"].as_u64(), Some(2));
    let d = doc(&s);
    assert_eq!(kinds(d, 2), ["MoveTo", "NURBSTo"]);
    let section = shape_of(d, 2).sheet.sections.iter().find(|sec| sec.name.as_str() == "Geometry").unwrap();
    let e = section.rows.get(1).unwrap().cells.get(&diacraft_doc::CellName::new("E")).unwrap().value.as_ref().unwrap().text().to_owned();
    assert!(e.starts_with("NURBS(") && e.contains(",3,0,0,"), "the constructor of a composite cubic in shares of the box: {e}");
    // What is drawn stays within the tolerance of the stroke.
    let path = on_page(d, 2);
    let worst = wave.iter().map(|p| path.windows(2).map(|w| seg_distance(*p, w[0], w[1])).fold(f64::INFINITY, f64::min)).fold(0.0, f64::max);
    assert!(worst < 0.1, "the curve is within {worst} of the stroke");
    assert!(near_p(path[0], wave[0]) && near_p(*path.last().unwrap(), wave[119]), "it ends where the stroke does");
}

fn seg_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0) };
    (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
}

#[test]
fn a_stroke_closed_with_close_ends_on_its_first_point_and_takes_the_fill() {
    let mut s = blank_session();
    let circle: Vec<(f64, f64)> =
        (0..=60).map(|i| (3.0 + 1.5 * (f64::from(i) * 6.0).to_radians().cos(), 3.0 + 1.5 * (f64::from(i) * 6.0).to_radians().sin())).collect();
    // The loop's last point is a hair from its first; `close` puts it on it.
    let mut loose = circle.clone();
    if let Some(last) = loose.last_mut() {
        *last = (circle[0].0 + 0.01, circle[0].1 - 0.01);
    }
    let r = run(&mut s, "shape.draw", json!({"page": 1, "tool": "pencil", "points": loose, "close": true}));
    assert_eq!(r["closed"], json!(true));
    let d = doc(&s);
    assert_eq!(flag(d, 1, "NoFill"), 0.0);
    let path = on_page(d, 1);
    assert!(near_p(path[0], *path.last().unwrap()), "the path ends on its start: {:?} {:?}", path[0], path.last());
    // An open stroke is not closed.
    let r = draw(&mut s, "pencil", json!(&circle[..40]));
    assert_eq!(r["closed"], json!(false));
    assert_eq!(flag(doc(&s), 2, "NoFill"), 1.0);
}

#[test]
fn a_drawing_is_one_undo_step_labelled_for_its_tool_and_an_id_is_never_given_twice() {
    let mut s = blank_session();
    let before = doc(&s).clone();
    let steps = s.history().unwrap().undo_len();
    draw(&mut s, "ellipse", json!([[1, 1], [3, 2]]));
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Draw ellipse"));
    let after = doc(&s).clone();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), &before, "undo takes the whole shape back");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(doc(&s), &after);
    for (tool, label) in
        [("rectangle", "Draw rectangle"), ("line", "Draw line"), ("arc", "Draw arc"), ("pencil", "Draw with pencil"), ("freeform", "Draw freeform")]
    {
        draw(&mut s, tool, json!([[1, 1], [3, 2]]));
        assert_eq!(s.history().unwrap().next_undo_label(), Some(label));
    }
    // The ids of shapes that went are not given again.
    let last = id_of(&draw(&mut s, "line", json!([[1, 5], [3, 5]])));
    run(&mut s, "shape.delete", json!({"ids": [last]}));
    assert_eq!(id_of(&draw(&mut s, "line", json!([[1, 6], [3, 6]]))), last + 1);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn lengths_are_paper_inches_whatever_the_pages_scale() {
    use diacraft_doc::{CellEdit, Value, WriteOrigin};
    let mut s = blank_session();
    // Two drawing units to a paper inch.
    s.edit("scale", |d| {
        d.write_cell(&SheetRef::Page(1), &CellAddr::direct("DrawingScale"), CellEdit::Value(Value::from_text("2")), WriteOrigin::Drawing)
            .map(|_| ())
            .map_err(|e| EngineError::Failed { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
    let r = draw(&mut s, "rectangle", json!([[1, 1], [3, 2]]));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(2.0), Some(1.0)), "the answer is in inches");
    assert!(near(num(doc(&s), 1, "Width"), 4.0) && near(num(doc(&s), 1, "PinX"), 4.0), "the cells hold drawing units");
}

#[test]
fn a_drawing_that_cannot_be_made_is_refused_in_words_and_changes_nothing() {
    let many: Vec<[f64; 2]> = (0..5_001).map(|i| [f64::from(i) * 0.001, 0.0]).collect();
    let cases: Vec<(Json, &str)> = vec![
        (json!({"page": 1, "tool": "brush", "points": [[0, 0], [1, 1]]}), "no tool `brush`"),
        (json!({"page": 1, "tool": "line"}), "`points` must be an array"),
        (json!({"page": 1, "tool": "line", "points": "x"}), "`points` must be an array"),
        (json!({"page": 1, "tool": "line", "points": [[0, 0], [1]]}), "pair of numbers"),
        (json!({"page": 1, "tool": "line", "points": [[0, 0], ["a", 1]]}), "pair of numbers"),
        (json!({"page": 1, "tool": "line", "points": [[0, 0], 5]}), "pair of numbers"),
        (json!({"page": 1, "tool": "line", "points": [[0, 0]]}), "at least two points"),
        (json!({"page": 1, "tool": "line", "points": []}), "at least two points"),
        (json!({"page": 1, "tool": "pencil", "points": many}), "at most 5000 points"),
        (json!({"page": 1, "tool": "line", "points": [[0, 0], [1.0e9, 1]]}), "within"),
        (json!({"page": 1, "tool": "rectangle", "points": [[1, 1], [1, 1]]}), "drag out a rectangle"),
        (json!({"page": 1, "tool": "line", "points": [[1, 1], [1, 1]]}), "drag out a line"),
        (json!({"page": 1, "tool": "pencil", "points": [[1, 1], [3, 1]], "tolerance": 0}), "`tolerance`"),
        (json!({"page": 1, "tool": "freeform", "points": [[1, 1], [3, 1]], "smoothing": 2}), "`smoothing`"),
        (json!({"page": 1, "tool": "line", "points": [[0, 0], [1, 1]], "shift": "yes"}), "`shift` must be true or false"),
        (json!({"page": 1, "tool": "line", "points": [[0, 0], [1, 1]], "close": 1}), "`close` must be true or false"),
        (json!({"page": 1, "tool": "line", "points": [[0, 0], [1, 1]], "close": true}), "`close` closes the stroke of the pencil"),
        (json!({"page": 1, "tool": "rectangle", "points": [[0, 0], [1, 1]], "continue": 1}), "`continue` goes on"),
        (json!({"page": 99, "tool": "line", "points": [[0, 0], [1, 1]]}), "no page 99"),
        (json!({"tool": "line", "points": [[0, 0], [1, 1]]}), "missing `page`"),
        (json!({"page": 1, "points": [[0, 0], [1, 1]]}), "missing `tool`"),
        (json!({"page": 1, "tool": "line", "points": [[0, 0], [1, 1]], "continue": 7}), "no shape 7"),
    ];
    for (params, needle) in cases {
        let mut s = blank_session();
        let before = snapshot(&s);
        let e = s.execute("shape.draw", &params).unwrap_err();
        assert!(bad_params_msg(&e).contains(needle), "{}: {e}", params.to_string().chars().take(80).collect::<String>());
        assert_eq!(snapshot(&s), before, "{needle}: nothing changed");
    }
    // A session with no document.
    let mut empty = Session::new();
    assert!(matches!(
        empty.execute("shape.draw", &json!({"page": 1, "tool": "line", "points": [[0, 0], [1, 1]]})),
        Err(EngineError::Disabled { .. })
    ));
}

#[test]
fn a_drawn_shape_follows_the_pages_theme_and_a_page_with_auto_size_grows_to_take_it_in() {
    let mut s = blank_session();
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    let style = diacraft_engine::theme::theme_style_id(doc(&s)).unwrap();
    let steps = s.history().unwrap().undo_len();
    draw(&mut s, "rectangle", json!([[1, 1], [3, 2]]));
    draw(&mut s, "line", json!([[4, 1], [6, 2]]));
    for id in [1, 2] {
        let links = shape_of(doc(&s), id).sheet.links;
        assert_eq!((links.line, links.fill, links.text), (Some(style), Some(style), Some(style)), "Shape {id} follows the theme");
    }
    assert_eq!(s.history().unwrap().undo_len(), steps + 2, "in the same undo step as the drawing");
    let mut grown = blank_session();
    run(&mut grown, "page.auto_size", json!({"on": true}));
    let before = num_of_page(&grown, "PageWidth");
    draw(&mut grown, "rectangle", json!([[1, 1], [before + 5.0, 3]]));
    assert!(num_of_page(&grown, "PageWidth") > before, "the page grew to take the shape in");
}

fn num_of_page(s: &Session, cell: &str) -> f64 {
    let d = doc(s);
    Resolver::new(d)
        .resolve(&SheetRef::Page(1), &CellAddr::direct(cell))
        .and_then(|found| found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()))
        .unwrap()
}

// ---- carrying a path on ----------------------------------------------------------------------------------------------------------

fn cont(s: &mut Session, id: u32, tool: &str, points: Json) -> Json {
    run(s, "shape.draw", json!({"page": 1, "tool": tool, "points": points, "continue": id}))
}

#[test]
fn a_line_goes_on_from_the_end_vertex_of_a_path_as_one_shape_and_the_stroke_is_tied_to_the_vertex() {
    let mut s = blank_session();
    draw(&mut s, "line", json!([[1, 1], [3, 1]]));
    // The new stroke begins a hair from the vertex: it is taken to begin on it. The box grows up and to the right.
    let r = cont(&mut s, 1, "line", json!([[3.02, 1.01], [3, 4]]));
    assert_eq!((r["id"].as_u64(), r["rows"].as_u64(), r["continued"].as_bool()), (Some(1), Some(3), Some(true)));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(2.0), Some(3.0)));
    let d = doc(&s);
    assert_eq!(kinds(d, 1), ["MoveTo", "LineTo", "LineTo"]);
    let path = on_page(d, 1);
    assert!(near_p(path[0], (1.0, 1.0)) && near_p(path[1], (3.0, 1.0)) && near_p(path[2], (3.0, 4.0)), "{path:?}");
    assert_eq!(d.page(1).unwrap().shapes.len(), 1, "one shape");
    assert_eq!(flag(d, 1, "NoFill"), 1.0, "still open");
    // One undo step takes the segment back, as Ctrl+Z removes segments in reverse order.
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Draw line"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(kinds(doc(&s), 1), ["MoveTo", "LineTo"]);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn tools_are_mixed_in_one_shape_and_a_path_goes_on_down_and_to_the_left_of_where_it_began() {
    let mut s = blank_session();
    draw(&mut s, "line", json!([[4, 4], [6, 4]]));
    cont(&mut s, 1, "arc", json!([[6, 4], [6, 6]]));
    let wave: Vec<(f64, f64)> = (0..40).map(|i| (6.0 - f64::from(i) * 0.1, 6.0 + (f64::from(i) * 0.3).sin() * 0.3)).collect();
    cont(&mut s, 1, "freeform", json!(wave));
    // The pencil's stroke goes on from where the curve ended.
    let from = *on_page(doc(&s), 1).last().unwrap();
    let stroke: Vec<(f64, f64)> = (0..=20).map(|i| (from.0 - f64::from(i) * 0.05, from.1 - f64::from(i) * 0.1)).collect();
    cont(&mut s, 1, "pencil", json!(stroke));
    let d = doc(&s);
    assert_eq!(kinds(d, 1), ["MoveTo", "LineTo", "ArcTo", "NURBSTo", "LineTo"]);
    let path = on_page(d, 1);
    assert!(near_p(path[0], (4.0, 4.0)), "what was drawn first has not moved: {:?}", path[0]);
    let end = *path.last().unwrap();
    assert!(near_p(end, stroke[20]), "{end:?}");
    // The box holds the whole path.
    let (w, h) = (num(d, 1, "Width"), num(d, 1, "Height"));
    assert!(w >= 4.0 && h >= 2.0, "{w} by {h}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn going_on_from_a_shape_that_is_turned_leaves_what_was_drawn_where_it_was_on_the_page() {
    let mut s = blank_session();
    draw(&mut s, "line", json!([[2, 2], [4, 2]]));
    run(&mut s, "shape.rotate", json!({"ids": [1], "by": 30}));
    let before = on_page(doc(&s), 1);
    let end = *before.last().unwrap();
    cont(&mut s, 1, "line", json!([[end.0, end.1], [end.0 + 1.0, end.1 + 2.0]]));
    let after = on_page(doc(&s), 1);
    assert_eq!(after.len(), 3);
    assert!(near_p(after[0], before[0]) && near_p(after[1], before[1]), "{before:?} then {after:?}");
    assert!(near_p(after[2], (end.0 + 1.0, end.1 + 2.0)), "{after:?}");
    assert!(near(num(doc(&s), 1, "Angle"), 30.0_f64.to_radians()), "the shape keeps its turn");
    // A shape that is flipped: an arc goes on bowed the way it was asked on the page.
    let mut m = blank_session();
    draw(&mut m, "line", json!([[2, 2], [4, 2]]));
    run(&mut m, "shape.flip", json!({"ids": [1], "axis": "horizontal"}));
    let before = on_page(doc(&m), 1);
    let end = *before.last().unwrap();
    cont(&mut m, 1, "arc", json!([[end.0, end.1], [end.0, end.1 + 2.0]]));
    let after = on_page(doc(&m), 1);
    assert!(near_p(after[0], before[0]) && near_p(after[1], before[1]), "{before:?} then {after:?}");
    let upright_arc = {
        let mut u = blank_session();
        draw(&mut u, "line", json!([[2, 2], [4, 2]]));
        cont(&mut u, 1, "arc", json!([[4, 2], [4, 4]]));
        on_page(doc(&u), 1)
    };
    // The arc part, relative to its own chord: the same side whether the shape was flipped or not (a flip of the line along its own axis changes nothing the arc has to follow).
    let side = |p: &[(f64, f64)]| {
        let (a, b) = (p[p.len() - 20], *p.last().unwrap());
        let mid = p[p.len() - 10];
        (b.0 - a.0) * (mid.1 - a.1) - (b.1 - a.1) * (mid.0 - a.0) > 0.0
    };
    assert_eq!(side(&after), side(&upright_arc), "the bow is on the same side of the chord");
}

#[test]
fn a_path_that_goes_on_to_its_first_vertex_closes_and_takes_the_fill_and_close_does_it_whatever_the_end() {
    let mut s = blank_session();
    draw(&mut s, "line", json!([[1, 1], [4, 1]]));
    cont(&mut s, 1, "line", json!([[4, 1], [4, 3]]));
    // The last end is let go a hair from the first vertex: with `close` it is on it.
    let r = run(&mut s, "shape.draw", json!({"page": 1, "tool": "line", "points": [[4, 3], [1.02, 1.01]], "continue": 1, "close": true}));
    assert_eq!((r["closed"].as_bool(), r["rows"].as_u64()), (Some(true), Some(4)));
    let d = doc(&s);
    assert_eq!(flag(d, 1, "NoFill"), 0.0, "a closed shape is filled");
    let path = on_page(d, 1);
    assert!(near_p(path[0], *path.last().unwrap()), "{path:?}");
    // A closed path has no end to go on from.
    let e = s.execute("shape.draw", &json!({"page": 1, "tool": "line", "points": [[1, 1], [0, 0]], "continue": 1})).unwrap_err();
    assert!(e.to_string().contains("is closed"), "{e}");
    // `shape.close` on an open path of two segments makes the triangle.
    let mut t = blank_session();
    draw(&mut t, "line", json!([[1, 1], [4, 1]]));
    cont(&mut t, 1, "line", json!([[4, 1], [4, 3]]));
    let r = run(&mut t, "shape.close", json!({"id": 1}));
    assert_eq!((r["closed"].as_bool(), r["rows"].as_u64()), (Some(true), Some(4)));
    assert_eq!(flag(doc(&t), 1, "NoFill"), 0.0);
    assert_eq!(t.history().unwrap().next_undo_label(), Some("Close shape"));
    let e = t.execute("shape.close", &json!({"id": 1})).unwrap_err();
    assert!(e.to_string().contains("closed"), "{e}");
}

#[test]
fn a_path_is_not_carried_on_for_a_shape_that_has_no_end_vertex_to_go_on_from() {
    let mut s = blank_session();
    draw(&mut s, "rectangle", json!([[1, 1], [3, 3]]));
    draw(&mut s, "ellipse", json!([[5, 1], [7, 3]]));
    let dropped = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3, "y": 6})));
    let line = id_of(&draw(&mut s, "line", json!([[1, 8], [3, 8]])));
    let grouped = {
        let other = id_of(&draw(&mut s, "line", json!([[5, 8], [7, 8]])));
        run(&mut s, "shape.group", json!({"ids": [line, other]}));
        other
    };
    for (id, needle) in [(1, "is closed"), (2, "ellipse has no end vertex"), (dropped, "instance of a master"), (grouped, "is in a group")] {
        let before = snapshot(&s);
        let e = s.execute("shape.draw", &json!({"page": 1, "tool": "line", "points": [[0, 0], [1, 1]], "continue": id})).unwrap_err();
        assert!(e.to_string().contains(needle), "shape {id}: {e}");
        assert_eq!(snapshot(&s), before, "shape {id}: nothing changed");
    }
    // Shape.close needs an open path too.
    let e = s.execute("shape.close", &json!({"id": 2})).unwrap_err();
    assert!(e.to_string().contains("ellipse has no end vertex"), "{e}");
    // A shape of two paths (a Combine result) is refused.
    let a = id_of(&draw(&mut s, "rectangle", json!([[10, 1], [12, 3]])));
    let b = id_of(&draw(&mut s, "rectangle", json!([[11, 2], [13, 4]])));
    run(&mut s, "shape.combine", json!({"ids": [a, b]}));
    let combined = doc(&s).page(1).unwrap().shapes.iter().map(|sh| sh.id).max().unwrap();
    let e = s.execute("shape.close", &json!({"id": combined})).unwrap_err();
    assert!(e.to_string().contains("paths") || e.to_string().contains("closed"), "{e}");
}

// ---- files -------------------------------------------------------------------------------------------------------------------------

/// A drawing with each tool's shape, and a path carried on.
fn made_with_every_tool() -> Session {
    let mut s = blank_session();
    draw(&mut s, "rectangle", json!([[1, 1], [3, 2]]));
    draw(&mut s, "ellipse", json!([[4, 1], [6, 2]]));
    draw(&mut s, "line", json!([[1, 3], [3, 4]]));
    draw(&mut s, "arc", json!([[4, 3], [6, 3]]));
    let wave: Vec<(f64, f64)> = (0..60).map(|i| (1.0 + f64::from(i) * 0.08, 6.0 + (f64::from(i) * 0.15).sin())).collect();
    draw(&mut s, "freeform", json!(wave));
    let stroke: Vec<(f64, f64)> = (0..=30).map(|i| (7.0, 1.0 + f64::from(i) * 0.1)).collect();
    draw(&mut s, "pencil", json!(stroke));
    cont(&mut s, 3, "line", json!([[3, 4], [3, 5]]));
    s
}

fn fingerprint(d: &Document, id: u32) -> (Vec<String>, Vec<(f64, f64)>, Vec<f64>) {
    let frame: Vec<f64> = ["PinX", "PinY", "Width", "Height"].iter().map(|c| num_of(d, id, c)).collect();
    let flags = flag(d, id, "NoFill");
    (kinds(d, id), on_page(d, id), [frame, vec![flags]].concat())
}

#[test]
fn every_tools_shape_saves_and_reopens_in_diacraft_vsdx_and_odg_with_its_rows_and_what_it_draws() {
    let s = made_with_every_tool();
    let want: Vec<_> = (1..=6).map(|id| fingerprint(doc(&s), id)).collect();
    let dir = Dir::new();
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut saving = made_with_every_tool();
        let path = dir.path(&format!("drawn.{ext}"));
        run(&mut saving, "file.save_as", json!({"path": path}));
        let again = opened(&path);
        let d = doc(&again);
        for (n, id) in (1..=6_u32).enumerate() {
            let got = fingerprint(d, id);
            assert_eq!(got.0, want[n].0, "{ext}: the rows of shape {id}");
            assert_eq!(got.1.len(), want[n].1.len(), "{ext}: shape {id} draws as many points");
            for (a, b) in got.1.iter().zip(&want[n].1) {
                assert!(near_p(*a, *b), "{ext}: shape {id} draws {a:?} for {b:?}");
            }
            for (a, b) in got.2.iter().zip(&want[n].2) {
                assert!(near(*a, *b), "{ext}: shape {id}: {a} for {b}");
            }
        }
        // The reopened drawing keeps what it was: a path can go on from the end of a shape that was read from the file.
        let mut more = again;
        let r = run(&mut more, "shape.draw", json!({"page": 1, "tool": "line", "points": [[3, 5], [5, 5]], "continue": 3}));
        assert_eq!(r["rows"].as_u64(), Some(4), "{ext}");
        assert_eq!(check_session(&more), Ok(()), "{ext}");
    }
}

#[test]
fn a_drawn_shape_renders_its_fill_and_its_line() {
    use diacraft_render::{Renderer, View};
    let mut s = blank_session();
    draw(&mut s, "rectangle", json!([[2, 2], [4, 3]]));
    draw(&mut s, "line", json!([[6, 2], [8, 2]]));
    let d = doc(&s);
    let view = View::fit_page(d, 1, 100.0).unwrap();
    let r = Renderer::new().render(d, 1, &view);
    let (w, h) = (r.width as usize, r.height as usize);
    let at = |x: f64, y: f64| {
        let (col, row) = ((x * 100.0) as usize, ((8.5 - y) * 100.0) as usize);
        let i = (row.min(h - 1) * w + col.min(w - 1)) * 4;
        [r.rgba_premul[i], r.rgba_premul[i + 1], r.rgba_premul[i + 2], r.rgba_premul[i + 3]]
    };
    // The inside of the closed rectangle is painted (the root style's fill), and the page beside it is not.
    assert!(at(3.0, 2.5)[3] > 0, "the rectangle is filled");
    assert_eq!(at(5.0, 2.5), at(9.5, 6.0), "the page shows through between the shapes");
    // The line is drawn along its length, and an open path has no fill to show above it.
    let page = at(9.5, 6.0);
    assert!((1..=3).any(|i| at(7.0, 2.0 + f64::from(i - 2) * 0.01) != page), "the line is drawn");
    assert_eq!(at(7.0, 2.4), page, "an open path has no fill");
}

// ---- cost ----------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_stroke_of_the_most_points_is_drawn_with_each_tool_and_a_scribble_is_refused_in_words() {
    let wave: Vec<(f64, f64)> = (0..5_000).map(|i| (f64::from(i) * 0.002, 3.0 + (f64::from(i) * 0.01).sin())).collect();
    for tool in ["pencil", "freeform"] {
        let mut s = blank_session();
        let r = draw(&mut s, tool, json!(wave));
        assert!(r["rows"].as_u64().unwrap() >= 2, "{tool}: {r}");
        assert_eq!(check_session(&s), Ok(()));
    }
    let zigzag: Vec<(f64, f64)> = (0..2_000).map(|i| (f64::from(i) * 0.01, if i % 2 == 0 { 1.0 } else { 2.0 })).collect();
    let mut s = blank_session();
    let before = snapshot(&s);
    let e = s.execute("shape.draw", &json!({"page": 1, "tool": "pencil", "points": zigzag, "tolerance": 0.001})).unwrap_err();
    assert!(bad_params_msg(&e).contains("too many corners"), "{e}");
    assert_eq!(snapshot(&s), before);
}

#[test]
fn the_window_can_ask_which_shapes_a_path_goes_on_from_and_where_their_ends_are() {
    use diacraft_engine::draw::pen::continuable;
    let mut s = blank_session();
    let line = id_of(&draw(&mut s, "line", json!([[1, 1], [4, 2]])));
    let rectangle = id_of(&draw(&mut s, "rectangle", json!([[1, 4], [3, 6]])));
    let ellipse = id_of(&draw(&mut s, "ellipse", json!([[5, 4], [7, 6]])));
    let dropped = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 8, "y": 2})));
    let d = doc(&s);
    let ends = continuable(d, 1, line).unwrap();
    assert!(near_p(ends.start, (1.0, 1.0)) && near_p(ends.end, (4.0, 2.0)), "{ends:?}");
    for (id, what) in [(rectangle, "a closed path"), (ellipse, "an ellipse"), (dropped, "an instance of a master"), (99, "no shape")] {
        assert_eq!(continuable(d, 1, id), None, "{what}");
    }
    // The ends are where the page has them for a shape that is turned, and in inches on a page of another scale.
    run(&mut s, "shape.rotate", json!({"ids": [line], "by": 90}));
    let turned = continuable(doc(&s), 1, line).unwrap();
    let path = on_page(doc(&s), line);
    assert!(near_p(turned.start, path[0]) && near_p(turned.end, *path.last().unwrap()), "{turned:?} against {path:?}");
}

#[test]
fn a_random_run_of_drawing_commands_leaves_the_session_valid_and_each_failure_changes_nothing() {
    // A small deterministic generator: the same run every time.
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut s = blank_session();
    let tools = ["rectangle", "ellipse", "line", "arc", "pencil", "freeform"];
    let mut made = 0_u32;
    for _ in 0..250 {
        let tool = tools[(next() % 6) as usize];
        let n = 2 + (next() % 40) as usize;
        let (ox, oy) = ((next() % 900) as f64 / 100.0, (next() % 700) as f64 / 100.0);
        let points: Vec<[f64; 2]> =
            (0..n).map(|i| [ox + (next() % 300) as f64 / 100.0 + i as f64 * 0.02, oy + (next() % 300) as f64 / 100.0]).collect();
        let mut params = json!({"page": 1, "tool": tool, "points": points, "shift": next() % 4 == 0});
        // About a third of the strokes go on from a shape drawn before, which may or may not be a path that can be carried on.
        if made > 0 && next() % 3 == 0 {
            params["continue"] = json!(1 + (next() % u64::from(made)) as u32);
            if next() % 3 == 0 {
                params["close"] = json!(true);
            }
        }
        let before = snapshot(&s);
        match s.execute("shape.draw", &params) {
            Ok(r) => made = made.max(id_of(&r)),
            Err(_) => assert_eq!(snapshot(&s), before, "a refusal changes nothing"),
        }
        assert_eq!(check_session(&s), Ok(()));
    }
    assert!(made > 20, "{made} shapes were made");
    // Every shape that is there is drawn: a render of the page does not lose one.
    let d = doc(&s);
    let view = diacraft_render::View::fit_page(d, 1, 50.0).unwrap();
    let rendered = diacraft_render::Renderer::new().render(d, 1, &view);
    assert!(rendered.skipped.is_empty(), "{:?}", rendered.skipped);
}

/// Everything the tools can draw, on the first page of the session, and the ids made: a rectangle, an ellipse, a line with a segment going on from it, an arc, a freeform stroke and a pencil stroke.
fn draw_everything_on(s: &mut Session, page: u32, from: f64) -> Vec<u32> {
    let mut ids = Vec::new();
    let make = |s: &mut Session, params: Json| id_of(&run(s, "shape.draw", params));
    ids.push(make(s, json!({"page": page, "tool": "rectangle", "points": [[from, 1], [from + 2.0, 2]]})));
    ids.push(make(s, json!({"page": page, "tool": "ellipse", "points": [[from + 3.0, 1], [from + 5.0, 2]]})));
    let line = make(s, json!({"page": page, "tool": "line", "points": [[from, 3], [from + 2.0, 4]]}));
    ids.push(line);
    ids.push(make(s, json!({"page": page, "tool": "arc", "points": [[from + 3.0, 3], [from + 5.0, 3]]})));
    let wave: Vec<(f64, f64)> = (0..60).map(|i| (from + f64::from(i) * 0.08, 6.0 + (f64::from(i) * 0.15).sin())).collect();
    ids.push(make(s, json!({"page": page, "tool": "freeform", "points": wave})));
    let stroke: Vec<(f64, f64)> = (0..=30).map(|i| (from + 6.0, 1.0 + f64::from(i) * 0.1)).collect();
    ids.push(make(s, json!({"page": page, "tool": "pencil", "points": stroke})));
    // The first line goes on: one shape of three rows, not a seventh shape.
    run(s, "shape.draw", json!({"page": page, "tool": "line", "points": [[from + 2.0, 4], [from + 2.0, 5]], "continue": line}));
    ids
}

#[test]
fn shapes_drawn_on_a_drawing_that_was_read_are_spliced_into_the_vsdx_and_the_odg_it_came_from() {
    use crate::native_common::vsdx_drawing;
    use crate::odg_build::{Dir as OdgDir, open_drawing};
    use crate::odg_file::four;
    // A VSDX file of two pages: the tools draw on its first page, `file.save` splices the shapes into the file, and a session that opens it again draws them as drawn.
    let dir = Dir::new();
    let path = dir.write("a.vsdx", &vsdx_drawing());
    let mut s = opened(&path);
    let page = first_page(doc(&s));
    let ids = draw_everything_on(&mut s, page, 1.0);
    let want: Vec<_> = ids.iter().map(|id| fingerprint(doc(&s), *id)).collect();
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["variant"].as_str(), Some("vsdx"), "{saved}");
    assert!(saved["parts"]["rewritten"].as_u64().is_some_and(|n| n > 0), "the page's part was spliced: {saved}");
    let again = opened(&path);
    for (id, want) in ids.iter().zip(&want) {
        assert_same_shape(doc(&again), *id, want, "vsdx");
    }
    assert_eq!(check_session(&again), Ok(()));
    // An OpenDocument drawing of four rectangles: the same.
    let odg_dir = OdgDir::new();
    let mut s = open_drawing(&odg_dir, &four());
    let page = first_page(doc(&s));
    let ids = draw_everything_on(&mut s, page, 1.0);
    let want: Vec<_> = ids.iter().map(|id| fingerprint(doc(&s), *id)).collect();
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!((saved["variant"].as_str(), saved["edited"].as_bool()), (Some("odg"), Some(true)), "{saved}");
    let mut again = Session::new();
    run(&mut again, "file.open", json!({"path": odg_dir.path("a.odg")}));
    // The reader numbers the shapes of a file by their place on the page, so the shapes are found by their place from the end of the list.
    let count = doc(&again).pages.get(0).unwrap().shapes.len();
    assert_eq!(count, 4 + ids.len(), "the four of the file and the six drawn");
    let read: Vec<u32> = doc(&again).pages.get(0).unwrap().shapes.iter().skip(4).map(|sh| sh.id).collect();
    for ((id, want), read_id) in ids.iter().zip(&want).zip(&read) {
        let _ = id;
        assert_same_shape(doc(&again), *read_id, want, "odg");
    }
}

/// Shape `id` of `d` has the rows, the frame, the flag and what is drawn that `want` (a [`fingerprint`]) has.
fn assert_same_shape(d: &Document, id: u32, want: &(Vec<String>, Vec<(f64, f64)>, Vec<f64>), what: &str) {
    let got = fingerprint(d, id);
    assert_eq!(got.0, want.0, "{what}: the rows of shape {id}");
    assert_eq!(got.1.len(), want.1.len(), "{what}: shape {id} draws as many points");
    for (a, b) in got.1.iter().zip(&want.1) {
        assert!(near_p(*a, *b), "{what}: shape {id} draws {a:?} for {b:?}");
    }
    for (a, b) in got.2.iter().zip(&want.2) {
        assert!(near(*a, *b), "{what}: shape {id}: {a} for {b}");
    }
}
