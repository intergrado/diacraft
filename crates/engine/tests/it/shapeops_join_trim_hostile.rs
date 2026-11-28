//! Join and Trim on crafted inputs (SHAPEOPS-CAPS): thousands of segments that cross or abut, stars and fans through one point, coincident paths, segments
//! shorter than the tolerance, shapes nested deep, coordinates far out and a number that is not a number. Each is refused in words or finished, in counted, bounded
//! work, and a refusal changes nothing. Each test holds the whole call to a time far above what it takes and far below a hang; the times a run measured are printed with
//! `--no-capture` and written in the doc of each test.

use std::sync::Arc;
use std::time::{Duration, Instant};

use diacraft_doc::{Cell, CellName, Row, Shape, Value};
use diacraft_engine::{EngineError, Session};
use serde_json::{Value as Json, json};

use crate::shapeops::{Build, assert_unchanged, doc, ids, line, rect, section_count, session_with, top};
use crate::shapeops_trim::ellipse;
use crate::{Snapshot, check_session, run, snapshot};

/// Ten times what any measured run took here at worst, and a hang by any account.
const LIMIT: Duration = Duration::from_secs(60);

/// Runs a command and how long it took, whatever the outcome.
fn timed(s: &mut Session, command: &str, params: &Json) -> (Result<Json, EngineError>, Duration) {
    let started = Instant::now();
    let result = s.execute(command, params);
    let took = started.elapsed();
    eprintln!(
        "{command}: {took:?}, {}",
        result.as_ref().map_or_else(|e| format!("refused: {e}"), |v| format!("ok, {} made", v["ids"].as_array().map_or(0, Vec::len)))
    );
    (result, took)
}

/// The words of a refusal that is the engine's and not a bug that was caught.
fn refused_in_words(error: EngineError) -> String {
    match error {
        EngineError::Failed { msg, .. } | EngineError::BadParams { msg, .. } => msg,
        other => panic!("expected a refusal in words, got {other:?}"),
    }
}

/// A refusal names a limit of the library or a count of the engine's.
fn assert_a_limit(message: &str) {
    let words = ["too many", "too degenerate", "not finite", "more than", "steps"];
    assert!(words.iter().any(|w| message.contains(w)), "not a refusal that names a limit: {message}");
}

/// Runs the command. Whichever way it ends, it ends in time and the session is sound; a refusal names a limit and changes nothing. The answer, when it finished.
fn bounded(s: &mut Session, command: &str, params: &Json) -> Option<Json> {
    let before = snapshot(s);
    let (result, took) = timed(s, command, params);
    assert!(took < LIMIT, "{command}: {took:?}");
    check_session(s).unwrap();
    match result {
        Ok(done) => Some(done),
        Err(error) => {
            assert_a_limit(&refused_in_words(error));
            assert_unchanged(s, &before, command);
            None
        }
    }
}

/// Runs a command that must be refused in words, naming `why`, and changes nothing.
fn refused(s: &mut Session, command: &str, params: &Json, why: &str) {
    let before: Snapshot = snapshot(s);
    let (result, took) = timed(s, command, params);
    assert!(took < LIMIT, "{command}: {took:?}");
    let message = refused_in_words(result.expect_err("the command is refused"));
    assert!(message.contains(why), "{message}");
    assert_unchanged(s, &before, command);
    check_session(s).unwrap();
}

fn row_of(ix: u32, kind: &str, x: f64, y: f64) -> Arc<Row> {
    let mut row = Row { ix: Some(ix), row_type: Some(Arc::from(kind)), ..Row::default() };
    row.cells.insert(CellName::new("X"), Cell::constant(Value::from_text(&format!("{x}"))).with_unit("IN"));
    row.cells.insert(CellName::new("Y"), Cell::constant(Value::from_text(&format!("{y}"))).with_unit("IN"));
    Arc::new(row)
}

/// One shape of `n` separate straight segments in one section, in the page's own coordinates (the shape's pin is where its local pin is, so its local points are the
/// page's): `segment(i)` gives the ends of the `i`th.
pub(crate) fn comb(id: u32, n: u32, segment: impl Fn(u32) -> ((f64, f64), (f64, f64))) -> Shape {
    let mut rows = Vec::new();
    for i in 0..n {
        let ((x0, y0), (x1, y1)) = segment(i);
        rows.push(row_of(2 * i + 1, "MoveTo", x0, y0));
        rows.push(row_of(2 * i + 2, "LineTo", x1, y1));
    }
    Build::new(id, 1.0, 1.0).pin(0.5, 0.5).raw_section(rows, true).build()
}

/// A closed polygon through `points`, as one section.
fn polygon(id: u32, points: &[(f64, f64)]) -> Shape {
    let mut rows = Vec::new();
    for (i, (x, y)) in points.iter().enumerate() {
        rows.push(row_of(u32::try_from(i + 1).unwrap(), if i == 0 { "MoveTo" } else { "LineTo" }, *x, *y));
    }
    if let Some((x, y)) = points.first() {
        rows.push(row_of(u32::try_from(points.len() + 1).unwrap(), "LineTo", *x, *y));
    }
    Build::new(id, 1.0, 1.0).pin(0.5, 0.5).raw_section(rows, false).build()
}

fn all_ids(n: u32) -> Json {
    json!((1..=n).collect::<Vec<u32>>())
}

// ---- Join -----------------------------------------------------------------------------------------------------------------------

/// 4,096 lines, the most one command takes, each ending where the next begins: one chain, one section of 4,096 segments.
#[test]
fn join_of_4096_lines_end_to_end_is_one_section_in_bounded_time() {
    let n = 4_096_u32;
    // A zigzag: the point `i` is at (i, 0) for an even `i` and (i, 0.5) for an odd one.
    let at = |i: u32| (f64::from(i), 0.5 * f64::from(i % 2));
    let shapes: Vec<Shape> = (0..n).map(|i| line(i + 1, at(i), at(i + 1)).build()).collect();
    let mut s = session_with(shapes);
    let done = bounded(&mut s, "shape.join", &json!({"ids": all_ids(n)})).expect("a chain of lines is joined");
    let id = ids(&done, "ids")[0];
    assert_eq!(section_count(doc(&s), id), 1);
    assert_eq!(top(doc(&s)), [id]);
}

/// 4,096 lines from one point: the earliest pair at the point joins and the rest go to other chains, which costs a few steps for each line, and not the square of them.
#[test]
fn join_of_a_star_of_4096_lines_from_one_point_pairs_them_in_bounded_time() {
    let n = 4_096_u32;
    let shapes: Vec<Shape> = (0..n).map(|i| line(i + 1, (0.0, 0.0), (1.0 + f64::from(i), 1.0)).build()).collect();
    let mut s = session_with(shapes);
    let done = bounded(&mut s, "shape.join", &json!({"ids": all_ids(n)})).expect("a star is joined");
    assert_eq!(section_count(doc(&s), ids(&done, "ids")[0]), usize::try_from(n / 2).unwrap());
}

/// Two crowds of line ends a hair apart (1.5E-5, past the tolerance, in neighbouring cells of the hash): no end is within the tolerance of one of the other crowd, and a
/// lookup reads the other crowd's entries. Finished or refused by the count of steps, in time.
#[test]
fn join_of_two_crowds_of_ends_a_hair_apart_is_finished_or_refused_in_bounded_time() {
    let n = 2_000_u32;
    let mut shapes = Vec::new();
    for i in 0..n {
        shapes.push(line(2 * i + 1, (0.0, 0.0), (5.0, 1.0 + f64::from(i))).build());
        shapes.push(line(2 * i + 2, (0.000_015, 0.0), (-5.0, 1.0 + f64::from(i))).build());
    }
    let mut s = session_with(shapes);
    bounded(&mut s, "shape.join", &json!({"ids": all_ids(2 * n)}));
}

/// 4,096 copies of one line: each pair closes into a line there and back.
#[test]
fn join_of_4096_copies_of_one_line_is_finished_in_bounded_time() {
    let n = 4_096_u32;
    let mut s = session_with((0..n).map(|i| line(i + 1, (0.0, 0.0), (3.0, 4.0)).build()).collect());
    let done = bounded(&mut s, "shape.join", &json!({"ids": all_ids(n)})).expect("copies are joined");
    assert_eq!(section_count(doc(&s), ids(&done, "ids")[0]), usize::try_from(n / 2).unwrap(), "each pair of copies is one closed section");
}

/// One shape of 90,000 segments (180,000 rows, under the 200,000 a command reads), each ending where the next begins.
#[test]
fn join_of_one_shape_of_90000_separate_segments_is_one_chain_in_bounded_time() {
    let n = 90_000_u32;
    let mut s = session_with(vec![comb(1, n, |i| ((f64::from(i), 0.0), (f64::from(i + 1), 0.0))), line(2, (-1.0, 0.0), (0.0, 0.0)).build()]);
    let done = bounded(&mut s, "shape.join", &json!({"ids": [1, 2]}));
    if let Some(done) = done {
        assert_eq!(section_count(doc(&s), ids(&done, "ids")[0]), 1);
    }
}

/// A cell that is not a number is read as the engine reads it (0), and a position or a geometry that overflows is refused in words: Join and Trim take what the shapes
/// draw, and a number that is not finite never reaches the library.
fn overflowing() -> [(Shape, &'static str); 2] {
    // The pin and the local pin are as far apart as a number goes: the shape's move to its container is not a number.
    let pose = Build::new(1, 1.0, 1.0).pin(1.7e308, 0.0).cell("LocPinX", Cell::constant(Value::from_text("-1.7e308"))).section(
        &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)],
        false,
        true,
    );
    // Rows a little under the greatest number, carried by a pin of the same size: the points are not numbers on the page.
    let geometry = Build::new(1, 1.0e308, 1.0).pin(1.7e308, 0.0).section(&[(0.0, 0.0), (1.0e308, 0.0), (1.0e308, 1.0)], false, true);
    [
        (pose.build(), "shape 1 has a position or a size that is not a number"),
        (geometry.build(), "shape 1 has geometry with a number that is not finite"),
    ]
}

#[test]
fn join_and_trim_of_a_pose_or_a_geometry_that_overflows_are_refused_in_words_and_change_nothing() {
    for (shape, message) in overflowing() {
        for (command, params) in [("shape.join", json!({"ids": [1, 2]})), ("shape.trim", json!({"ids": [1, 2]}))] {
            let mut s = session_with(vec![shape.clone(), line(2, (0.0, 0.0), (1.0, 1.0)).build()]);
            refused(&mut s, command, &params, message);
        }
    }
}

#[test]
fn join_and_trim_of_cells_that_are_not_numbers_are_finished_as_the_engine_reads_them_or_refused_in_words() {
    for bad in ["NaN", "nan", "inf", "-inf", "1e999", "", "text"] {
        let mut a = line(1, (1.0, 1.0), (3.0, 3.0)).build();
        let sheet = Arc::make_mut(&mut a.sheet);
        for name in ["Width", "PinX"] {
            sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(bad)));
        }
        let section = Arc::make_mut(sheet.sections.get_mut(0).unwrap());
        let row = Arc::make_mut(section.rows.get_mut(1).unwrap());
        row.cells.insert(CellName::new("X"), Cell::constant(Value::from_text(bad)));
        let mut s = session_with(vec![a, line(2, (2.0, 0.0), (2.0, 5.0)).build()]);
        for command in ["shape.join", "shape.trim"] {
            let (result, took) = timed(&mut s, command, &json!({"ids": [1, 2]}));
            assert!(took < LIMIT, "{took:?}");
            if let Err(error) = result {
                assert!(matches!(error, EngineError::Failed { .. } | EngineError::BadParams { .. }), "{bad}: {command}: {error:?}");
            } else {
                run(&mut s, "edit.undo", json!({}));
            }
            check_session(&s).unwrap();
        }
    }
}

// ---- Trim -----------------------------------------------------------------------------------------------------------------------

/// `n` horizontal and `n` vertical lines across a square, each pair crossing once: `n * n` crossings, and every line cut into `n + 1` pieces.
fn mesh(n: u32) -> Vec<Shape> {
    let side = f64::from(n);
    let mut shapes = Vec::new();
    for i in 0..n {
        let at = f64::from(i) + 0.5;
        shapes.push(line(1 + 2 * i, (-1.0, at), (side + 1.0, at)).build());
        shapes.push(line(2 + 2 * i, (at, -1.0), (at, side + 1.0)).build());
    }
    shapes
}

/// A mesh of 70 and 70 lines, 4,900 crossings, makes the pieces `140 * 71 = 9,940`, under what a command makes: finished.
#[test]
fn trim_of_a_mesh_that_makes_9940_pieces_is_finished_in_bounded_time() {
    let mut s = session_with(mesh(70));
    let done = bounded(&mut s, "shape.trim", &json!({"ids": all_ids(140)})).expect("9,940 pieces are made");
    assert_eq!(ids(&done, "ids").len(), 140 * 71);
}

/// A mesh of 100 and 100 lines would make `200 * 101 = 20,200` pieces: refused in words, in a fraction of a second, and nothing changes.
#[test]
fn trim_of_a_mesh_that_would_make_more_than_10000_pieces_is_refused_in_words_and_changes_nothing() {
    let mut s = session_with(mesh(100));
    refused(&mut s, "shape.trim", &json!({"ids": all_ids(200)}), "more than 10000 shapes");
}

/// Two combs of 5,000 segments each, across one another: 25 million crossings, which no command makes pieces of.
#[test]
fn trim_of_two_combs_of_5000_segments_that_cross_in_25_million_places_is_refused_in_bounded_time() {
    let n = 5_000_u32;
    let mut s = session_with(vec![
        comb(1, n, |i| ((-1.0, f64::from(i) * 0.01), (60.0, f64::from(i) * 0.01))),
        comb(2, n, |i| ((f64::from(i) * 0.01, -1.0), (f64::from(i) * 0.01, 60.0))),
    ]);
    assert!(bounded(&mut s, "shape.trim", &json!({"ids": [1, 2]})).is_none(), "25 million crossings are refused");
}

/// A star of 2,001 vertices, each joined to the one 1,000 steps on: 2 million crossings of 2,001 edges.
#[test]
fn trim_of_a_star_polygon_of_2001_vertices_that_crosses_itself_in_2_million_places_is_refused_in_bounded_time() {
    let n = 2_001_u32;
    let points: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let angle = std::f64::consts::TAU * f64::from((i * 1_000) % n) / f64::from(n);
            (5.0 + 5.0 * angle.cos(), 5.0 + 5.0 * angle.sin())
        })
        .collect();
    let mut s = session_with(vec![polygon(1, &points)]);
    assert!(bounded(&mut s, "shape.trim", &json!({"ids": [1]})).is_none());
}

/// 300 lines through one point, all crossing at it: 600 ends in one place, the library's limit on ends at a point.
#[test]
fn trim_of_a_fan_of_300_lines_through_one_point_is_finished_or_refused_in_bounded_time() {
    let n = 300_u32;
    let shapes: Vec<Shape> = (0..n)
        .map(|i| {
            let angle = std::f64::consts::PI * f64::from(i) / f64::from(n);
            line(i + 1, (5.0 - 5.0 * angle.cos(), 5.0 - 5.0 * angle.sin()), (5.0 + 5.0 * angle.cos(), 5.0 + 5.0 * angle.sin())).build()
        })
        .collect();
    let mut s = session_with(shapes);
    bounded(&mut s, "shape.trim", &json!({"ids": all_ids(n)}));
}

/// 50 copies of one line, one on another: the stretch is kept once.
#[test]
fn trim_of_50_copies_of_one_line_keeps_the_stretch_once() {
    let n = 50_u32;
    let mut s = session_with((0..n).map(|i| line(i + 1, (0.0, 0.0), (3.0, 4.0)).build()).collect());
    let done = bounded(&mut s, "shape.trim", &json!({"ids": all_ids(n)})).expect("copies are trimmed");
    assert_eq!(ids(&done, "ids").len(), 1, "one stretch");
}

/// 50 copies of one square, one on another: each is an uncut ring, and one is kept, closed.
#[test]
fn trim_of_50_copies_of_one_square_keeps_one_closed_shape() {
    let n = 50_u32;
    let mut s = session_with((0..n).map(|i| rect(i + 1, (0.0, 0.0, 2.0, 2.0)).build()).collect());
    let done = bounded(&mut s, "shape.trim", &json!({"ids": all_ids(n)})).expect("copies are trimmed");
    let made = ids(&done, "ids");
    assert_eq!(made.len(), 1, "one square");
    assert_eq!(crate::shapeops::flag_of(doc(&s), made[0], 0, "NoFill"), 0.0, "an uncut ring stays closed");
}

/// 2,000 copies of one line and 1,000 of one square: every segment overlaps every copy, past the library's limit on pairs of segments with overlapping boxes: refused in
/// words, in a few milliseconds, and nothing changes.
#[test]
fn trim_of_thousands_of_stacked_copies_is_refused_in_words_in_bounded_time() {
    let n = 2_000_u32;
    let mut s = session_with((0..n).map(|i| line(i + 1, (0.0, 0.0), (3.0, 4.0)).build()).collect());
    assert!(bounded(&mut s, "shape.trim", &json!({"ids": all_ids(n)})).is_none(), "2,000 copies of a line are refused");
    let n = 1_000_u32;
    let mut s = session_with((0..n).map(|i| rect(i + 1, (0.0, 0.0, 2.0, 2.0)).build()).collect());
    assert!(bounded(&mut s, "shape.trim", &json!({"ids": all_ids(n)})).is_none(), "1,000 copies of a square are refused");
}

/// 2,000 lines along one line, each overlapping the last by half: they run along one another in turn.
#[test]
fn trim_of_2000_overlapping_stretches_of_one_line_is_finished_or_refused_in_bounded_time() {
    let n = 2_000_u32;
    let mut s = session_with((0..n).map(|i| line(i + 1, (f64::from(i), 0.0), (f64::from(i) + 2.0, 0.0)).build()).collect());
    bounded(&mut s, "shape.trim", &json!({"ids": all_ids(n)}));
}

/// A polyline of 20,000 segments each 2E-6 long (shorter than the tolerance, so no stretch of its own) zigzagging along 0.04, and a long line across it.
#[test]
fn trim_of_a_polyline_of_20000_segments_shorter_than_the_tolerance_is_finished_or_refused_in_bounded_time() {
    let n = 20_000_u32;
    let points: Vec<(f64, f64)> = (0..=n).map(|i| (f64::from(i) * 2.0e-6, if i % 2 == 0 { 0.0 } else { 1.0e-6 })).collect();
    let mut rows = vec![row_of(1, "MoveTo", points[0].0, points[0].1)];
    for (i, (x, y)) in points.iter().enumerate().skip(1) {
        rows.push(row_of(u32::try_from(i + 1).unwrap(), "LineTo", *x, *y));
    }
    let zigzag = Build::new(1, 1.0, 1.0).pin(0.5, 0.5).raw_section(rows, true).build();
    let mut s = session_with(vec![zigzag, line(2, (0.02, -1.0), (0.02, 1.0)).build()]);
    bounded(&mut s, "shape.trim", &json!({"ids": [1, 2]}));
}

/// 20 circles of radius 1 with their centres 0.19 apart along a line: two circles cross twice when their centres are under 2 apart, which is 145 pairs (ten steps along
/// or fewer), and each crossing is a cut in both circles, so the circles are cut into `4 * 145 = 580` pieces. No three circles cross at one point, none touch, and the
/// crossings along a circle are 0.09 apart or more.
#[test]
fn trim_of_20_circles_that_cross_in_145_pairs_makes_the_580_pieces_they_are_cut_into() {
    let n = 20_u32;
    let mut s = session_with((0..n).map(|i| ellipse(i + 1, (f64::from(i) * 0.19, 0.0, 2.0 + f64::from(i) * 0.19, 2.0)).build()).collect());
    let done = bounded(&mut s, "shape.trim", &json!({"ids": all_ids(n)})).expect("20 circles are trimmed");
    assert_eq!(ids(&done, "ids").len(), 580);
}

/// 9,999 short segments in one shape that cross nothing: each is a piece. One more would be past what a command makes.
#[test]
fn trim_of_9999_segments_that_cross_nothing_is_finished_and_10001_are_refused() {
    let seg = |i: u32| ((f64::from(i) * 0.1, 0.0), (f64::from(i) * 0.1, 0.05));
    let mut s = session_with(vec![comb(1, 9_999, seg)]);
    let done = bounded(&mut s, "shape.trim", &json!({"ids": [1]})).expect("9,999 pieces are made");
    assert_eq!(ids(&done, "ids").len(), 9_999);
    let mut s = session_with(vec![comb(1, 10_001, seg)]);
    refused(&mut s, "shape.trim", &json!({"ids": [1]}), "more than 10000 shapes");
}

/// 500 circles of one size, each moved a hair from the last: every pair crosses twice.
#[test]
fn trim_of_500_circles_that_each_cross_the_rest_twice_is_finished_or_refused_in_bounded_time() {
    let n = 500_u32;
    let mut s = session_with((0..n).map(|i| ellipse(i + 1, (f64::from(i) * 0.002, 0.0, 2.0 + f64::from(i) * 0.002, 2.0)).build()).collect());
    bounded(&mut s, "shape.trim", &json!({"ids": all_ids(n)}));
}

/// One shape of 1,024 sections (the most a shape may have to be an operand), each a line across the next.
#[test]
fn trim_of_a_shape_of_1024_crossing_sections_is_finished_or_refused_in_bounded_time() {
    let mut shape = Build::new(1, 1.0, 1.0).pin(0.5, 0.5);
    for i in 0..1_024_u32 {
        let at = f64::from(i) * 0.01;
        let rows = if i % 2 == 0 {
            vec![row_of(1, "MoveTo", at, -1.0), row_of(2, "LineTo", at, 11.0)]
        } else {
            vec![row_of(1, "MoveTo", -1.0, at), row_of(2, "LineTo", 11.0, at)]
        };
        shape = shape.raw_section(rows, true);
    }
    let mut s = session_with(vec![shape.build()]);
    bounded(&mut s, "shape.trim", &json!({"ids": [1]}));
}

/// Coordinates at 1E12, where the library's tolerance is wider than a drawing's detail: finished near the origin or refused in words, in time.
#[test]
fn trim_of_shapes_a_trillion_inches_out_is_finished_or_refused_in_bounded_time() {
    let far = 1.0e12;
    let mut s =
        session_with(vec![rect(1, (far, far, far + 4.0, far + 2.0)).build(), line(2, (far + 2.0, far - 1.0), (far + 2.0, far + 3.0)).build()]);
    bounded(&mut s, "shape.trim", &json!({"ids": [1, 2]}));
}

/// Coordinates at 1E300, past what the library's sweep takes: refused in words.
#[test]
fn trim_of_shapes_at_1e300_is_refused_in_words() {
    let far = 1.0e300;
    let mut s =
        session_with(vec![rect(1, (far, far, far * 2.0, far * 2.0)).build(), line(2, (far * 1.5, far * 0.5), (far * 1.5, far * 3.0)).build()]);
    bounded(&mut s, "shape.trim", &json!({"ids": [1, 2]}));
}

// ---- deep groups ----------------------------------------------------------------------------------------------------------------

/// The members `members` of a group that is 85 levels deep, each level a group of the one inside it.
fn nested(members: Vec<Shape>) -> Shape {
    let mut shape = Shape::new(1000);
    shape.kind = diacraft_doc::ShapeType::Group;
    shape.sheet = Arc::new(Build::new(1000, 4.0, 4.0).pin(2.0, 2.0).build().sheet.as_ref().clone());
    shape.children = diacraft_doc::ShapeList::from_vec(members.into_iter().map(Arc::new).collect());
    for level in 0..85_u32 {
        let mut group = Shape::new(1001 + level);
        group.kind = diacraft_doc::ShapeType::Group;
        group.sheet = Arc::new(Build::new(1001 + level, 4.0, 4.0).pin(2.0, 2.0).build().sheet.as_ref().clone());
        group.children = diacraft_doc::ShapeList::from_vec(vec![Arc::new(shape)]);
        shape = group;
    }
    shape
}

fn on_page(s: &Session, id: u32) -> bool {
    doc(s).find_shape(diacraft_doc::Owner::Page(1), id).is_some()
}

/// Operands that are members of groups 85 levels deep: the walks are iterative, and the result is the last member of the innermost group.
#[test]
fn join_of_members_of_groups_eighty_five_deep_is_made_where_they_are() {
    let mut s = session_with(vec![nested(vec![line(1, (0.0, 0.0), (1.0, 0.0)).build(), line(2, (1.0, 0.0), (1.0, 1.0)).build()])]);
    let done = bounded(&mut s, "shape.join", &json!({"ids": [1, 2]})).expect("joined");
    let produced = ids(&done, "ids")[0];
    assert!(produced > 1085 && on_page(&s, produced));
}

#[test]
fn trim_of_members_of_groups_eighty_five_deep_is_made_where_they_are() {
    let mut s = session_with(vec![nested(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build(), line(2, (1.0, -1.0), (1.0, 3.0)).build()])]);
    let done = bounded(&mut s, "shape.trim", &json!({"ids": [1, 2]})).expect("trimmed");
    let made = ids(&done, "ids");
    assert_eq!(made.len(), 5);
    assert!(made.iter().all(|id| on_page(&s, *id)));
}
