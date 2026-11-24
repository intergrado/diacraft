//! Shape operations on crafted inputs (SHAPEOPS-CAPS): many crossing shapes, a pie of many slices, shapes nested deep, coordinates far out and a number that
//! is not a number. Each is refused in words or finished, and in counted, bounded work: the counts are the sections, rows and shapes the command was given and made,
//! the library's own limits (`craft_pathops::Limits`, never raised here) decide the rest, and each test holds the whole call to a time far above what it takes and far
//! below a hang. The times a run measured are printed with `--no-capture` and written in the doc of each test.

use std::sync::Arc;
use std::time::{Duration, Instant};

use diacraft_doc::{Cell, CellName, Row, Section, Shape, Value};
use diacraft_engine::{EngineError, Session};
use serde_json::{Value as Json, json};

use crate::shapeops::{Build, area, doc, ids, made, num, rect, section_count, session_with, top};
use crate::{check_session, run};

/// Ten times what any measured run took here at worst, and a hang by any account.
const LIMIT: Duration = Duration::from_secs(60);

/// Runs a command and how long it took, whatever the outcome.
fn timed(s: &mut Session, command: &str, params: &Json) -> (Result<Json, EngineError>, Duration) {
    let started = Instant::now();
    let result = s.execute(command, params);
    let took = started.elapsed();
    eprintln!("{command}: {took:?}, {}", result.as_ref().map_or_else(|e| format!("refused: {e}"), |v| format!("ok {}", v["ids"])));
    (result, took)
}

/// A refusal that is the engine's words and not a bug that was caught.
fn refused_in_words(result: &Result<Json, EngineError>) -> String {
    match result {
        Err(EngineError::Failed { msg, .. } | EngineError::BadParams { msg, .. }) => msg.clone(),
        other => panic!("expected a refusal in words, got {other:?}"),
    }
}

fn numbered(shapes: Vec<Shape>) -> Session {
    session_with(shapes)
}

// ---- many crossing shapes -------------------------------------------------------------------------------------------------------

/// A grid: `n` thin bars across and `n` thin bars up, 0.2 apart, 0.1 thick.
fn grid(n: u32) -> Vec<Shape> {
    let length = f64::from(n) * 0.2;
    let mut shapes = Vec::new();
    for i in 0..n {
        let at = f64::from(i) * 0.2;
        shapes.push(rect(1 + 2 * i, (0.0, at, length, at + 0.1)).build());
        shapes.push(rect(2 + 2 * i, (at, 0.0, at + 0.1, length)).build());
    }
    shapes
}

/// 120 bars crossing in 3,600 places: finishes in about a second and gives one shape with a hole in every gap, `59 * 59` of them, and the outline. The area is the
/// bars' less what two of them share.
#[test]
fn many_crossing_bars_unite_in_bounded_time_to_a_shape_with_a_hole_in_every_gap() {
    let n = 60;
    let mut s = numbered(grid(n));
    let all: Vec<u32> = (1..=2 * n).collect();
    let (result, took) = timed(&mut s, "shape.union", &json!({"ids": all}));
    assert!(took < LIMIT, "{took:?}");
    let id = made(&result.unwrap());
    let d = doc(&s);
    assert_eq!(section_count(d, id), 59 * 59 + 1, "a hole in every gap and the outline");
    let bars = f64::from(2 * n) * 12.0 * 0.1;
    let crossings = f64::from(n * n) * 0.01;
    assert!((area(d, id) - (bars - crossings)).abs() < 1.0e-6, "{}", area(d, id));
    assert_eq!(top(d), [id]);
}

/// 300 thin bars all turned about one centre: each pair crosses, 45,000 crossings in all, and all the long edges pass near one point. Refused or finished, and
/// quickly: this run was refused in words.
#[test]
fn a_fan_of_bars_through_one_point_is_refused_or_finished_in_bounded_time() {
    let n = 300_u32;
    let shapes: Vec<Shape> = (0..n)
        .map(|i| {
            Build::new(1 + i, 8.0, 0.02)
                .pin(0.0, 0.0)
                .angle_degrees(180.0 * f64::from(i) / f64::from(n))
                .section(&[(0.0, 0.0), (8.0, 0.0), (8.0, 0.02), (0.0, 0.02)], true, false)
                .build()
        })
        .collect();
    let mut s = numbered(shapes);
    let all: Vec<u32> = (1..=n).collect();
    let (result, took) = timed(&mut s, "shape.union", &json!({"ids": all}));
    assert!(took < LIMIT, "{took:?}");
    match &result {
        Ok(done) => assert_eq!(made(done), n + 1),
        Err(_) => {
            let message = refused_in_words(&result);
            assert!(message.contains("too many") || message.contains("degenerate"), "{message}");
        }
    }
    check_session(&s).unwrap();
}

// ---- a pie of many slices -------------------------------------------------------------------------------------------------------

/// One shape that is a pie of `slices` triangles meeting at (5, 5), and a square across it.
fn pie(slices: u32) -> Vec<Shape> {
    let mut pie = Build::new(1, 10.0, 10.0).pin(5.0, 5.0);
    for i in 0..slices {
        let (a, b) = (std::f64::consts::TAU * f64::from(i) / f64::from(slices), std::f64::consts::TAU * f64::from(i + 1) / f64::from(slices));
        pie = pie.section(&[(5.0, 5.0), (5.0 + 5.0 * a.cos(), 5.0 + 5.0 * a.sin()), (5.0 + 5.0 * b.cos(), 5.0 + 5.0 * b.sin())], true, false);
    }
    vec![pie.build(), rect(2, (3.0, 3.0, 7.0, 7.0)).build()]
}

/// 256 slices have 512 ends at the centre, the library's limit: the union is made, in about a second.
#[test]
fn a_pie_of_256_slices_is_united_in_bounded_time() {
    let mut s = numbered(pie(256));
    let (result, took) = timed(&mut s, "shape.union", &json!({"ids": [1, 2]}));
    assert!(took < LIMIT, "{took:?}");
    let id = made(&result.unwrap());
    assert!(area(doc(&s), id) > 70.0, "about the disc of radius 5, 78.5, less what 256 chords leave: {}", area(doc(&s), id));
}

/// 300 slices have 600 ends at the centre, past the 512 the library takes: refused in a few milliseconds, in words, and nothing changes.
#[test]
fn a_pie_of_300_slices_is_refused_in_words_and_changes_nothing() {
    let mut s = numbered(pie(300));
    let before = crate::snapshot(&s);
    let (result, took) = timed(&mut s, "shape.union", &json!({"ids": [1, 2]}));
    assert!(took < LIMIT, "{took:?}");
    let message = refused_in_words(&result);
    assert!(message.contains("too many"), "{message}");
    crate::shapeops::assert_unchanged(&s, &before, "a pie of 300 slices");
}

// ---- nested shapes --------------------------------------------------------------------------------------------------------------

/// `n` squares, each inside the last.
fn nested(n: u32) -> Vec<Shape> {
    (0..n)
        .map(|i| rect(1 + i, (f64::from(i) * 0.001, f64::from(i) * 0.001, 100.0 - f64::from(i) * 0.001, 100.0 - f64::from(i) * 0.001)).build())
        .collect()
}

/// 2,000 and 3,000 squares nested: the union is the outermost, each in about a tenth of a second (a union sweeps the shapes' outlines with one winding number).
#[test]
fn squares_nested_two_thousand_and_three_thousand_deep_unite_to_the_outermost() {
    for n in [2_000_u32, 3_000] {
        let mut s = numbered(nested(n));
        let all: Vec<u32> = (1..=n).collect();
        let (result, took) = timed(&mut s, "shape.union", &json!({"ids": all}));
        assert!(took < LIMIT, "{took:?}");
        let id = made(&result.unwrap());
        assert_eq!(section_count(doc(&s), id), 1);
        assert!((area(doc(&s), id) - 100.0 * 100.0).abs() < 1.0, "{}", area(doc(&s), id));
    }
}

/// An Intersect and a Fragment of 3,000 squares nested keep, for each side of each piece, the shapes that cover it: about 4 n (n + 1) entries, 36 million, past the
/// 33 million the library takes. Refused in words, and quickly; or finished, in bounded time.
#[test]
fn an_intersect_and_a_fragment_of_three_thousand_nested_squares_are_refused_or_finished_in_bounded_time() {
    for (command, n) in [("shape.intersect", 3_000_u32), ("shape.fragment", 3_000)] {
        let mut s = numbered(nested(n));
        let all: Vec<u32> = (1..=n).collect();
        let (result, took) = timed(&mut s, command, &json!({"ids": all}));
        assert!(took < LIMIT, "{command}: {took:?}");
        if result.is_err() {
            assert!(refused_in_words(&result).contains("too many"), "{command}: {result:?}");
        }
        check_session(&s).unwrap();
    }
}

#[test]
fn nested_squares_subtract_in_bounded_time() {
    let mut s = numbered(nested(1_500));
    let all: Vec<u32> = (1..=1_500).collect();
    let (result, took) = timed(&mut s, "shape.subtract", &json!({"ids": all}));
    assert!(took < LIMIT, "{took:?}");
    let _ = result.map(|_| ()).map_err(|e| e.to_string());
    check_session(&s).unwrap();
}

/// Operands that are members of groups 85 levels deep: the walks are iterative and the result is the last member of the innermost group.
#[test]
fn shapes_in_groups_eighty_five_deep_are_worked_on_where_they_are() {
    let mut inner = Shape::new(1000);
    inner.kind = diacraft_doc::ShapeType::Group;
    inner.sheet = Arc::new(Build::new(1000, 4.0, 4.0).pin(2.0, 2.0).build().sheet.as_ref().clone());
    inner.children =
        diacraft_doc::ShapeList::from_vec(vec![Arc::new(rect(1, (0.0, 0.0, 2.0, 2.0)).build()), Arc::new(rect(2, (1.0, 1.0, 3.0, 3.0)).build())]);
    let mut shape = inner;
    for level in 0..85_u32 {
        let mut group = Shape::new(1001 + level);
        group.kind = diacraft_doc::ShapeType::Group;
        group.sheet = Arc::new(Build::new(1001 + level, 4.0, 4.0).pin(2.0, 2.0).build().sheet.as_ref().clone());
        group.children = diacraft_doc::ShapeList::from_vec(vec![Arc::new(shape)]);
        shape = group;
    }
    let mut s = numbered(vec![shape]);
    let (result, took) = timed(&mut s, "shape.union", &json!({"ids": [1, 2]}));
    assert!(took < LIMIT, "{took:?}");
    let produced = made(&result.unwrap());
    assert!(produced > 1085);
    assert!(doc(&s).find_shape(diacraft_doc::Owner::Page(1), produced).is_some());
    check_session(&s).unwrap();
}

// ---- constructors, overflow and the cap on one shape's sections -------------------------------------------------------------------

/// A shape of one section whose rows are the ones given.
fn with_rows(id: u32, rows: Vec<Arc<Row>>) -> Shape {
    let mut shape = Build::new(id, 10.0, 10.0).pin(5.0, 5.0).build();
    let mut section = Section::new("Geometry", Some(0));
    for flag in ["NoFill", "NoLine", "NoShow", "NoSnap", "NoQuickDrag"] {
        section.cells.insert(CellName::new(flag), Cell::constant(Value::from_text("0")));
    }
    section.rows = rows.into();
    Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(section));
    shape
}

fn row(ix: u32, kind: &str, cells: &[(&str, &str)], text: &[(&str, String, &str)]) -> Arc<Row> {
    let mut row = Row { ix: Some(ix), row_type: Some(Arc::from(kind)), ..Row::default() };
    for (name, value) in cells {
        row.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)));
    }
    for (name, value, unit) in text {
        row.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)).with_unit(unit));
    }
    Arc::new(row)
}

/// The constructor text of the rows is charged by its bytes (1 MiB for all the shapes) before any is parsed: three polylines of 45,000 pairs, or eight NURBS of 12,000
/// tuples, is more, and is refused by the count; one polyline of the same size is not.
#[test]
fn constructor_text_is_charged_by_its_bytes_before_it_is_parsed() {
    let polyline = |points: usize| format!("POLYLINE(0,0{})", ",0.55,0.55".repeat(points));
    let nurbs = |tuples: usize| format!("NURBS(1,3,0,0{})", ",0.5,0.5,0,1".repeat(tuples));
    let polylines = |rows: u32| {
        let mut all = vec![row(1, "MoveTo", &[("X", "0"), ("Y", "0")], &[])];
        for i in 0..rows {
            all.push(row(2 + i, "PolylineTo", &[("X", "9"), ("Y", "9")], &[("A", polyline(45_000), "POLYLINE")]));
        }
        with_rows(1, all)
    };
    let nurbses = |rows: u32| {
        let mut all = vec![row(1, "MoveTo", &[("X", "0"), ("Y", "0")], &[])];
        for i in 0..rows {
            all.push(row(
                2 + i,
                "NURBSTo",
                &[("X", "9"), ("Y", "9"), ("A", "0"), ("B", "1"), ("C", "0"), ("D", "1")],
                &[("E", nurbs(12_000), "NURBS")],
            ));
        }
        with_rows(1, all)
    };
    for (what, over, under) in [("polylines", polylines(3), polylines(1)), ("NURBS", nurbses(8), nurbses(1))] {
        let mut s = numbered(vec![over, rect(2, (0.0, 0.0, 1.0, 1.0)).build()]);
        let before = crate::snapshot(&s);
        let (result, took) = timed(&mut s, "shape.combine", &json!({"ids": [1, 2]}));
        assert!(took < Duration::from_secs(10), "{what}: refused by a count, not after the work: {took:?}");
        assert!(refused_in_words(&result).contains("too much constructor text"), "{what}: {result:?}");
        crate::shapeops::assert_unchanged(&s, &before, what);
        let mut s = numbered(vec![under, rect(2, (0.0, 0.0, 1.0, 1.0)).build()]);
        let (result, _) = timed(&mut s, "shape.combine", &json!({"ids": [1, 2]}));
        made(&result.unwrap());
    }
}

/// A position so far out that the pose's own arithmetic overflows, and geometry that overflows when the pose is applied: refused in words and nothing changes.
#[test]
fn a_pose_or_a_geometry_that_overflows_is_refused_in_words() {
    // The pin and the local pin are as far apart as a number goes: the shape's move to its container is not a number.
    let pose = Build::new(1, 1.0, 1.0).pin(1.7e308, 0.0).cell("LocPinX", Cell::constant(Value::from_text("-1.7e308"))).section(
        &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)],
        true,
        false,
    );
    // Rows a little under the greatest number, carried by a pin of the same size: the points are not numbers on the page.
    let geometry = Build::new(1, 1.0e308, 1.0).pin(1.7e308, 0.0).section(&[(0.0, 0.0), (1.0e308, 0.0), (1.0e308, 1.0), (0.0, 1.0)], true, false);
    for (what, shape, message) in [
        ("pose", pose.build(), "shape 1 has a position or a size that is not a number"),
        ("geometry", geometry.build(), "shape 1 has geometry with a number that is not finite"),
    ] {
        let mut s = numbered(vec![shape, rect(2, (0.0, 0.0, 1.0, 1.0)).build()]);
        let before = crate::snapshot(&s);
        let (result, took) = timed(&mut s, "shape.union", &json!({"ids": [1, 2]}));
        assert!(took < LIMIT, "{took:?}");
        assert!(refused_in_words(&result).contains(message), "{what}: {result:?}");
        crate::shapeops::assert_unchanged(&s, &before, what);
    }
}

/// A shape holds up to 1,024 `Geometry` sections to be an operand: 1,024 are.
#[test]
fn a_shape_of_exactly_the_most_sections_an_operand_may_have_is_one() {
    let mut b = Build::new(1, 100.0, 100.0).pin(50.0, 50.0);
    for i in 0..1_024_u32 {
        let at = f64::from(i % 90);
        b = b.section(&[(at, at), (at + 1.0, at), (at + 1.0, at + 1.0)], true, false);
    }
    let mut s = numbered(vec![b.build(), rect(2, (0.0, 0.0, 1.0, 1.0)).build()]);
    let (result, _) = timed(&mut s, "shape.combine", &json!({"ids": [1, 2]}));
    let id = made(&result.unwrap());
    assert_eq!(section_count(doc(&s), id), 1_025);
}

// ---- far out --------------------------------------------------------------------------------------------------------------------

/// Two small overlapping squares at `at`: the union is the same shape as near the origin, to the precision of the numbers, and quickly.
fn far_out_union(at: f64) -> (Session, u32) {
    let mut s = numbered(vec![rect(1, (at + 1.0, at + 1.0, at + 3.0, at + 3.0)).build(), rect(2, (at + 2.0, at + 2.0, at + 4.0, at + 4.0)).build()]);
    let (result, took) = timed(&mut s, "shape.union", &json!({"ids": [1, 2]}));
    assert!(took < LIMIT, "{took:?}");
    let id = made(&result.unwrap());
    (s, id)
}

#[test]
fn small_shapes_a_hundred_million_out_unite_as_they_do_at_the_origin() {
    let (s, id) = far_out_union(1.0e8);
    let d = doc(&s);
    assert_eq!((num(d, id, "Width"), num(d, id, "Height")), (3.0, 3.0));
    assert!((num(d, id, "PinX") - 100_000_002.5).abs() < 1.0e-6 && (num(d, id, "PinY") - 100_000_002.5).abs() < 1.0e-6);
    // The area is read relative to the shape, which the page coordinates of 1e8 would lose to rounding in a sum of products.
    assert_eq!(section_count(d, id), 1);
    assert_eq!(crate::shapeops::local_points(d, id)[0].len(), 8, "the eight corners of V2");
}

#[test]
fn small_shapes_a_million_million_out_unite_too() {
    // 5E13 is as far out as the sweep can be taken: it refuses coordinates past about 7E13, and a union worked on at twice that would be refused. The work is
    // done near the origin, so a small shape at 5E13 is as easy as one at 1.
    for at in [1.0e12, 5.0e13] {
        let (s, id) = far_out_union(at);
        assert_eq!((num(doc(&s), id, "Width"), num(doc(&s), id, "Height")), (3.0, 3.0), "at {at:e}");
    }
}

/// A shape of 1E300 is finite, and past what the sweep takes: refused in words, or finished, and no bug is caught.
#[test]
fn a_shape_of_a_million_million_million_is_refused_in_words_or_finished_and_nothing_panics() {
    for at in [1.0e300, 1.0e30, 5.0e14] {
        let mut s = numbered(vec![rect(1, (at, at, at * 2.0, at * 2.0)).build(), rect(2, (at * 1.5, at * 1.5, at * 3.0, at * 3.0)).build()]);
        let (result, took) = timed(&mut s, "shape.union", &json!({"ids": [1, 2]}));
        assert!(took < LIMIT, "{took:?}");
        if let Err(error) = &result {
            assert!(matches!(error, EngineError::Failed { .. } | EngineError::BadParams { .. }), "{at:e}: {error:?}");
            // What the sweep cannot take is `Degenerate`, which is said so.
            assert!(refused_in_words(&result).contains("too degenerate"), "{at:e}: {error:?}");
        }
        check_session(&s).unwrap();
    }
}

/// A number that is not a number: the cells read 0 and the shapes are as they read, or the command is refused in words. Not a panic.
#[test]
fn a_cell_that_is_not_a_number_is_read_as_the_engine_reads_it_and_nothing_panics() {
    for bad in ["NaN", "nan", "inf", "-inf", "1e999", "", "text"] {
        let mut a = rect(1, (1.0, 1.0, 3.0, 3.0)).build();
        // The width, the pin and a coordinate of a row.
        let sheet = Arc::make_mut(&mut a.sheet);
        for name in ["Width", "PinX"] {
            sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(bad)));
        }
        let section = Arc::make_mut(sheet.sections.get_mut(0).unwrap());
        let row = Arc::make_mut(section.rows.get_mut(1).unwrap());
        row.cells.insert(CellName::new("X"), Cell::constant(Value::from_text(bad)));
        let mut s = numbered(vec![a, rect(2, (2.0, 2.0, 4.0, 4.0)).build()]);
        for command in ["shape.union", "shape.combine", "shape.fragment", "shape.intersect", "shape.subtract"] {
            let (result, took) = timed(&mut s, command, &json!({"ids": [1, 2]}));
            assert!(took < LIMIT, "{took:?}");
            if let Err(error) = &result {
                assert!(matches!(error, EngineError::Failed { .. } | EngineError::BadParams { .. }), "{bad}: {command}: {error:?}");
            }
            check_session(&s).unwrap();
            // Whatever it did, put the page back for the next command.
            if result.is_ok() {
                run(&mut s, "edit.undo", json!({}));
            }
        }
    }
}

// ---- many shapes, many sections, many rows --------------------------------------------------------------------------------------

/// 4,096 tiny squares, the most one command takes, in a row: united into one shape of 4,096 sections.
#[test]
fn four_thousand_ninety_six_disjoint_squares_unite_to_one_shape_of_as_many_sections() {
    let shapes: Vec<Shape> = (0..4_096_u32).map(|i| rect(1 + i, (f64::from(i) * 2.0, 0.0, f64::from(i) * 2.0 + 1.0, 1.0)).build()).collect();
    let mut s = numbered(shapes);
    let all: Vec<u32> = (1..=4_096).collect();
    let (result, took) = timed(&mut s, "shape.union", &json!({"ids": all}));
    assert!(took < LIMIT, "{took:?}");
    let id = made(&result.unwrap());
    assert_eq!(section_count(doc(&s), id), 4_096);
    // One more than a command takes is refused by its parameters, before anything is looked up.
    let mut s = numbered(vec![rect(1, (0.0, 0.0, 1.0, 1.0)).build()]);
    let too_many: Vec<u32> = (1..=4_097).collect();
    let error = s.execute("shape.union", &json!({"ids": too_many})).unwrap_err();
    assert!(matches!(error, EngineError::BadParams { .. }), "{error:?}");
}

/// A rectangle cut by `n` lines across it and `n` up it: `(n + 1)` squared regions.
fn cut_into_squares(n: u32) -> Session {
    let size = f64::from(n) + 1.0;
    let mut shapes = vec![rect(1, (0.0, 0.0, size, size)).build()];
    for i in 0..n {
        let at = 1.0 + f64::from(i);
        shapes.push(crate::shapeops::line(2 + 2 * i, (-1.0, at), (size + 1.0, at)).build());
        shapes.push(crate::shapeops::line(3 + 2 * i, (at, -1.0), (at, size + 1.0)).build());
    }
    numbered(shapes)
}

/// Gives shape 1 a look that is large: `character` rows of `Character`, `paragraph` rows of `Paragraph` and `gradient` rows of `FillGradient`, each row with three cells of
/// its own. (A section holds at most 1,024 rows to be read at all, so these are the largest a shape can show a command.)
fn give_look(s: &mut Session, character: u32, paragraph: u32, gradient: u32) {
    let section = |name: &str, rows: u32, cells: [&str; 3]| {
        let mut section = Section::new(name, None);
        for ix in 0..rows {
            let mut row = Row { ix: Some(ix), ..Row::default() };
            for (k, cell) in cells.iter().enumerate() {
                row.cells.insert(CellName::new(cell), Cell::constant(Value::from_text(&format!("{}", ix + u32::try_from(k).unwrap()))));
            }
            section.rows.push(Arc::new(row));
        }
        Arc::new(section)
    };
    s.edit("a large look", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let first = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let sheet = Arc::make_mut(&mut first.sheet);
        sheet.sections.push(section("Character", character, ["Size", "Color", "Style"]));
        sheet.sections.push(section("Paragraph", paragraph, ["HorzAlign", "SpLine", "IndFirst"]));
        sheet.sections.push(section("FillGradient", gradient, ["GradientStopColor", "GradientStopColorTrans", "GradientStopPosition"]));
        Ok(())
    })
    .unwrap();
}

/// What a made shape holds besides its geometry: the sections, as they are shared.
fn look_sections(s: &Session, id: u32) -> Vec<Arc<Section>> {
    let d = doc(s);
    let shape = d.shape(&crate::shapeops::at(d, id)).unwrap();
    shape.sheet.sections.iter().filter(|sec| sec.name.as_str() != "Geometry").cloned().collect()
}

/// A Fragment gives its pieces the first shape's formatting and no text, so the `Character`, `Paragraph` and `Tabs` rows, which are the text's, are not copied to any
/// piece; and the formatting sections it does give are **one section, shared** by all the pieces, and not a copy for each. Counted over 400 pieces of a first shape that holds
/// 1,000 rows of each of `Character` and `Paragraph` and a gradient of 20: the rows of the text's sections built are none, the distinct sections, rows and cells in all the
/// pieces are those of the one gradient, and so are their bytes. (Cloned for each piece, as it was, they were 800,000 rows more and 400 times the gradient.)
#[test]
fn a_fragment_copies_no_text_rows_and_shares_the_formatting_sections_between_its_pieces() {
    let mut s = cut_into_squares(19);
    give_look(&mut s, 1_000, 1_000, 20);
    let all: Vec<u32> = (1..=39).collect();
    let (result, took) = timed(&mut s, "shape.fragment", &json!({"ids": all}));
    assert!(took < LIMIT, "{took:?}");
    let made = ids(&result.unwrap(), "ids");
    assert_eq!(made.len(), 400);
    let (mut text_rows, mut referenced_rows) = (0_usize, 0_usize);
    let mut sections = std::collections::HashSet::new();
    let mut rows = std::collections::HashSet::new();
    let mut cells = 0_usize;
    for id in &made {
        for section in look_sections(&s, *id) {
            if matches!(section.name.as_str(), "Character" | "Paragraph" | "Tabs") {
                text_rows += section.rows.len();
            }
            referenced_rows += section.rows.len();
            sections.insert(Arc::as_ptr(&section));
            for row in section.rows.iter() {
                if rows.insert(Arc::as_ptr(row)) {
                    cells += row.cells.len();
                }
            }
        }
    }
    let bytes = rows.len() * std::mem::size_of::<Row>() + cells * std::mem::size_of::<Cell>() + sections.len() * std::mem::size_of::<Section>();
    eprintln!(
        "400 pieces: {text_rows} text rows, {referenced_rows} rows referenced, {} sections, {} rows and {cells} cells allocated, about {bytes} bytes; {took:?}",
        sections.len(),
        rows.len()
    );
    assert_eq!(text_rows, 0, "no text, so none of the text's rows");
    assert_eq!(referenced_rows, 400 * 20, "each piece has the gradient");
    assert_eq!((sections.len(), rows.len(), cells), (1, 20, 60), "one gradient, shared: not one for each piece");
    assert!(bytes < 20_000, "the formatting sections of 400 pieces are the bytes of one: {bytes}");
}

/// A `FillGradient` section that costs exactly `units` rows and cells to write: one for it, and for each row one and one for each cell. (`units` is at least 4.)
fn gradient_of_units(units: usize) -> Arc<Section> {
    let mut section = Section::new("FillGradient", None);
    let mut remaining = units - 1;
    let mut ix = 0_u32;
    while remaining >= 2 {
        let row_units = if remaining > 9 { 8 } else { remaining };
        let mut row = Row { ix: Some(ix), ..Row::default() };
        for k in 0..row_units - 1 {
            row.cells.insert(CellName::new(&format!("Stop{k}")), Cell::constant(Value::from_text("1")));
        }
        section.rows.push(Arc::new(row));
        remaining -= row_units;
        ix += 1;
    }
    assert_eq!(remaining, 0);
    Arc::new(section)
}

/// What a refusal says each shape would hold: the number after "holds".
fn units_in(message: &str) -> usize {
    message.split("holds ").nth(1).and_then(|rest| rest.split_whitespace().next()).and_then(|n| n.parse().ok()).unwrap_or_else(|| panic!("{message}"))
}

/// Fragments 625 pieces (24 lines each way) of a first shape that has `gradient` as its only look section.
fn fragment_625_with(gradient: Arc<Section>) -> Result<Json, EngineError> {
    let mut s = cut_into_squares(24);
    s.edit("a gradient", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let first = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        Arc::make_mut(&mut first.sheet).sections.push(gradient);
        Ok(())
    })
    .unwrap();
    let all: Vec<u32> = (1..=49).collect();
    let (result, took) = timed(&mut s, "shape.fragment", &json!({"ids": all}));
    assert!(took < LIMIT, "{took:?}");
    result
}

/// The charge is exact. What each shape is given besides the gradient (its frame's cells and the first shape's own) is read from what a refusal says, and a gradient is
/// made to bring each of the 625 pieces to exactly 800 cells and rows, which across them is the 500,000 a command writes: made. One cell more is 501 thousand and a quarter:
/// refused.
#[test]
fn the_charge_for_a_look_is_exact_at_the_most_a_command_writes() {
    let refused = fragment_625_with(gradient_of_units(1_000));
    let base = units_in(&refused_in_words(&refused)) - 1_000;
    assert!((10..=40).contains(&base), "a frame and a few cells: {base}");
    let made = ids(&fragment_625_with(gradient_of_units(800 - base)).unwrap(), "ids").len();
    assert_eq!(made, 625, "800 cells and rows each is 500,000 in all: made");
    let message = refused_in_words(&fragment_625_with(gradient_of_units(801 - base)));
    assert!(message.contains("625 shapes") && units_in(&message) == 801, "801 each is over: {message}");
}

/// A command that gives text keeps the text's rows: a Union of a first shape of 1,000 rows of each of `Character` and `Paragraph` and a gradient of 20 makes one shape that
/// holds them all (the sections are shared with no one, there being one shape).
#[test]
fn a_union_keeps_the_text_rows_of_the_first_shape() {
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build(), rect(2, (1.0, 1.0, 3.0, 3.0)).build()]);
    give_look(&mut s, 1_000, 1_000, 20);
    let id = made(&run(&mut s, "shape.union", json!({"ids": [1, 2]})));
    let rows: Vec<(String, usize)> = look_sections(&s, id).iter().map(|sec| (sec.name.as_str().to_owned(), sec.rows.len())).collect();
    assert_eq!(rows, [("FillGradient".to_owned(), 20), ("Character".to_owned(), 1_000), ("Paragraph".to_owned(), 1_000)]);
}

/// What a command writes of the first shape's formatting sections is charged, as rows and cells of them times the shapes made, **before the shapes are built**: a gradient of
/// 1,000 rows (the most a section is read with) of three cells, across 2,116 pieces, is 8.5 million, past the 500,000 a command writes in all, and is refused in words, quickly,
/// with the document as it was. A gradient of 20 rows across 400 pieces is what the test above makes.
#[test]
fn the_rows_of_a_look_times_the_shapes_made_are_charged_before_they_are_built() {
    let mut s = cut_into_squares(45);
    give_look(&mut s, 0, 0, 1_000);
    let before = crate::snapshot(&s);
    let all: Vec<u32> = (1..=91).collect();
    let (result, took) = timed(&mut s, "shape.fragment", &json!({"ids": all}));
    assert!(took < LIMIT, "{took:?}");
    let message = refused_in_words(&result);
    assert!(message.contains("formatting") && message.contains("at most 500000"), "{message}");
    crate::shapeops::assert_unchanged(&s, &before, "a look of 4,000 rows and cells across 2,116 pieces");
}

/// A hundred rows of lines across a rectangle and as many up it: 10,201 regions, past the 10,000 shapes a command makes. Refused in words, and nothing changes.
#[test]
fn a_fragment_that_would_make_more_than_ten_thousand_shapes_is_refused() {
    let mut s = cut_into_squares(100);
    let before = crate::snapshot(&s);
    let all: Vec<u32> = (1..=201).collect();
    let (result, took) = timed(&mut s, "shape.fragment", &json!({"ids": all}));
    assert!(took < LIMIT, "{took:?}");
    let message = refused_in_words(&result);
    // It stops at the cap as it builds the pieces, so it does not say how many there were.
    assert!(message.contains("more than 10000") && message.contains("at most 10000") && !message.contains("10201"), "{message}");
    crate::shapeops::assert_unchanged(&s, &before, "a Fragment of 10,201 regions");
}

/// 99 rows and 99 columns: 10,000 regions, the most a command makes, and 200 operands: all made, in about a second, one undo step, each with the first shape's
/// formatting.
#[test]
fn a_fragment_of_ten_thousand_regions_is_made_in_bounded_time_and_undone_in_one_step() {
    let mut s = cut_into_squares(99);
    let before = crate::snapshot(&s);
    let all: Vec<u32> = (1..=199).collect();
    let (result, took) = timed(&mut s, "shape.fragment", &json!({"ids": all}));
    assert!(took < LIMIT, "{took:?}");
    let made = ids(&result.unwrap(), "ids");
    assert_eq!(made.len(), 10_000);
    assert_eq!(top(doc(&s)), made, "nothing of the 199 originals is left");
    let started = Instant::now();
    run(&mut s, "edit.undo", json!({}));
    eprintln!("undo of 10,000 shapes: {:?}", started.elapsed());
    assert_eq!(doc(&s), before.document.as_ref().unwrap(), "undo brings back the 199 originals, as they were");
}

/// Operands that together hold more sections than one command reads (8,192): refused in words by the count, before any section is merged or built.
#[test]
fn operands_with_more_sections_than_a_command_reads_are_refused_by_the_count() {
    let many = |id: u32, n: u32| {
        let mut b = Build::new(id, 100.0, 100.0).pin(50.0, 50.0);
        for i in 0..n {
            let at = f64::from(i % 90);
            b = b.section(&[(at, at), (at + 1.0, at), (at + 1.0, at + 1.0)], true, false);
        }
        b.build()
    };
    // A shape holds at most 1,024 sections to be an operand; nine shapes of 1,000 are 9,000.
    let shapes: Vec<Shape> = (1..=9).map(|id| many(id, 1_000)).collect();
    let mut s = numbered(shapes);
    let before = crate::snapshot(&s);
    let all: Vec<u32> = (1..=9).collect();
    let (result, took) = timed(&mut s, "shape.union", &json!({"ids": all}));
    assert!(took < Duration::from_secs(10), "refused by a count, not after the work: {took:?}");
    let message = refused_in_words(&result);
    assert!(message.contains("too many sections"), "{message}");
    crate::shapeops::assert_unchanged(&s, &before, "9,000 sections");
    // A shape of more than 1,024 sections is not an operand.
    let mut s = numbered(vec![many(1, 1_100), rect(2, (0.0, 0.0, 1.0, 1.0)).build()]);
    let (result, _) = timed(&mut s, "shape.union", &json!({"ids": [1, 2]}));
    assert!(refused_in_words(&result).contains("1100 sections"));
}

/// Sections that are hidden, or have no rows, are looked at too and each costs a lookup: they are charged to the count as the ones drawn are, so 4,096 shapes of 1,024
/// hidden sections are refused by the count and are not four million lookups.
#[test]
fn hidden_and_empty_sections_are_charged_to_the_count_as_drawn_ones_are() {
    let hidden = |id: u32| {
        let mut b = Build::new(id, 10.0, 10.0).pin(5.0, 5.0);
        for i in 0..1_000_u32 {
            b = b.section(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], true, false).section_flag(i, "NoShow", true);
        }
        b.build()
    };
    let mut s = numbered((1..=9).map(hidden).collect());
    let before = crate::snapshot(&s);
    let all: Vec<u32> = (1..=9).collect();
    let (result, took) = timed(&mut s, "shape.combine", &json!({"ids": all}));
    assert!(took < Duration::from_secs(10), "{took:?}");
    assert!(refused_in_words(&result).contains("too many sections"), "{result:?}");
    crate::shapeops::assert_unchanged(&s, &before, "9,000 hidden sections");
}

/// Exactly the 8,192 sections a command reads (eight shapes of 1,024, the most a shape may have) are combined into a shape of 8,192 sections, the most a shape holds;
/// one section more is refused by the count.
#[test]
fn combine_takes_exactly_the_most_sections_a_command_reads_and_one_more_is_refused() {
    let many = |id: u32, n: u32| {
        let mut b = Build::new(id, 100.0, 100.0).pin(50.0, 50.0);
        for i in 0..n {
            let at = f64::from(i % 90);
            b = b.section(&[(at, at), (at + 1.0, at), (at + 1.0, at + 1.0)], true, false);
        }
        b.build()
    };
    let mut s = numbered((1..=8).map(|id| many(id, 1_024)).collect());
    let all: Vec<u32> = (1..=8).collect();
    let (result, took) = timed(&mut s, "shape.combine", &json!({"ids": all}));
    assert!(took < LIMIT, "{took:?}");
    let id = made(&result.unwrap());
    assert_eq!(section_count(doc(&s), id), 8_192);
    let mut shapes: Vec<Shape> = (1..=8).map(|id| many(id, 1_024)).collect();
    shapes.push(many(9, 1));
    let mut s = numbered(shapes);
    let all: Vec<u32> = (1..=9).collect();
    let (result, _) = timed(&mut s, "shape.combine", &json!({"ids": all}));
    assert!(refused_in_words(&result).contains("too many sections"), "{result:?}");
}

/// Eight shapes of 1,024 disjoint squares each unite to a shape of 8,192 sections, the most a shape holds: made. (One more would be a ninth shape's, past what a command
/// reads, so the cap on what a shape holds is the one reached here and not crossed.)
#[test]
fn a_union_of_exactly_the_most_sections_a_shape_holds_is_made() {
    let squares = |id: u32| {
        let mut b = Build::new(id, 64.0, 64.0).pin(32.0, 32.0 + 70.0 * f64::from(id));
        for i in 0..1_024_u32 {
            let (x, y) = (2.0 * f64::from(i % 32), 2.0 * f64::from(i / 32));
            b = b.section(&[(x, y), (x + 1.0, y), (x + 1.0, y + 1.0), (x, y + 1.0)], true, false);
        }
        b.build()
    };
    let mut s = numbered((1..=8).map(squares).collect());
    let all: Vec<u32> = (1..=8).collect();
    let (result, took) = timed(&mut s, "shape.union", &json!({"ids": all}));
    assert!(took < LIMIT, "{took:?}");
    let id = made(&result.unwrap());
    assert_eq!(section_count(doc(&s), id), 8_192);
    assert!((area(doc(&s), id) - 8_192.0).abs() < 1.0e-6, "{}", area(doc(&s), id));
}

/// One section of more than the 200,000 rows a command reads: refused by the count `rows_held` gives, without merging a row.
#[test]
fn a_section_of_more_rows_than_a_command_reads_is_refused_by_the_count() {
    let mut huge = Section::new("Geometry", Some(0));
    for flag in ["NoFill", "NoLine", "NoShow", "NoSnap", "NoQuickDrag"] {
        huge.cells.insert(CellName::new(flag), Cell::constant(Value::from_text("0")));
    }
    for i in 0..200_001_u32 {
        let mut row = Row { ix: Some(i + 1), row_type: Some(Arc::from(if i == 0 { "MoveTo" } else { "LineTo" })), ..Row::default() };
        row.cells.insert(CellName::new("X"), Cell::constant(Value::from_text(&format!("{}", f64::from(i % 100)))));
        row.cells.insert(CellName::new("Y"), Cell::constant(Value::from_text(&format!("{}", f64::from(i % 7)))));
        huge.rows.push(Arc::new(row));
    }
    let mut a = Build::new(1, 100.0, 7.0).pin(50.0, 3.5).build();
    Arc::make_mut(&mut a.sheet).sections.push(Arc::new(huge));
    let mut s = numbered(vec![a, rect(2, (0.0, 0.0, 1.0, 1.0)).build()]);
    let before = crate::snapshot(&s);
    let (result, took) = timed(&mut s, "shape.union", &json!({"ids": [1, 2]}));
    assert!(took < Duration::from_secs(10), "{took:?}");
    assert!(refused_in_words(&result).contains("too many rows"));
    crate::shapeops::assert_unchanged(&s, &before, "200,001 rows");
}

// ----: the star, the first shape's direct cells, a look section past the cap, and Fragment's cap -----------------------

/// One shape whose only section is a closed polygon of `n` vertices on a circle of radius 5, vertex `i` at the angle 2 * pi * (`step` * `i` mod `n`) / `n` (a star whose
/// every edge crosses nearly every other: 2,001 vertices and a step of 1,000 is the review's), and a small square across its middle.
fn star(n: u32, step: u32) -> Vec<Shape> {
    let points: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let k = (u64::from(step) * u64::from(i)) % u64::from(n);
            let angle = std::f64::consts::TAU * (k as f64) / f64::from(n);
            (5.0 + 5.0 * angle.cos(), 5.0 + 5.0 * angle.sin())
        })
        .collect();
    vec![Build::new(1, 10.0, 10.0).pin(5.0, 5.0).section(&points, true, false).build(), rect(2, (4.0, 4.0, 6.0, 6.0)).build()]
}

/// A command asked only whether it could run on a star of 2,001 vertices pays no sweep: it reads the rows (bounded by the counts) and looks at each ring, and that is all.
/// Before the round the engine ran the library's `try_area`, a full sweep, on each operand to judge "has a fill area": 19 s and 3.5 GB for the star alone, and the window
/// is meant to ask for each selection.
#[test]
fn asking_whether_a_command_could_run_on_a_star_of_2001_vertices_pays_no_sweep() {
    use diacraft_engine::draw::{Op, check_shape_ops};
    let s = numbered(star(2_001, 1_000));
    for op in [Op::Union, Op::Intersect, Op::Fragment, Op::Subtract, Op::Combine] {
        let started = Instant::now();
        let verdict = check_shape_ops(doc(&s), 1, op, &[1, 2]);
        let took = started.elapsed();
        eprintln!("check {}: {took:?}", op.id());
        assert_eq!(verdict, Ok(()), "{}", op.id());
        assert!(took < Duration::from_secs(3), "{}: {took:?}", op.id());
    }
}

/// The star of 2,001 vertices passes every limit the path library has (its sweep takes it 17 s and 4 GB in one call), so a command on it is bounded by nothing here and by
/// the library's own limits alone: the fix is the library's (craft-libs PR #677 bounds the crossings a sweep makes), and this test is un-ignored when the pin moves to it.
/// Each command must then be refused in words, or finished, within 10 s.
#[test]
#[ignore = "the 2,001-vertex star passes the path library's limits and one sweep of it takes 17 s and 4 GB: un-ignore when the pin moves to the craft-libs fix (PR #677, a bound on the crossings of a sweep)"]
fn a_star_of_2001_vertices_is_refused_in_words_or_finished_in_ten_seconds_by_every_command() {
    for command in ["shape.union", "shape.intersect", "shape.fragment", "shape.subtract"] {
        let mut s = numbered(star(2_001, 1_000));
        let (result, took) = timed(&mut s, command, &json!({"ids": [1, 2]}));
        assert!(took < Duration::from_secs(10), "{command}: {took:?}");
        if result.is_err() {
            refused_in_words(&result);
        }
        check_session(&s).unwrap();
    }
}

/// Gives shape 1 every direct cell of the three families the look copies, each as a formula; how many it set.
fn give_every_look_cell(s: &mut Session) -> usize {
    use diacraft_doc::{CellClass, listed_cells};
    let names: Vec<&'static str> = listed_cells()
        .filter(|(section, _, class)| section.is_none() && matches!(class, CellClass::FillFamily | CellClass::LineFamily | CellClass::TextFamily))
        .map(|(_, name, _)| name)
        .collect();
    let count = names.len();
    s.edit("every look cell", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let first = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let sheet = Arc::make_mut(&mut first.sheet);
        for name in names {
            sheet.cells.insert(CellName::new(name), Cell::with_formula("Width*0.5", Some(Value::from_text("1"))));
        }
        Ok(())
    })
    .unwrap();
    count
}

/// The cells a first shape sets itself are copied into every piece, and are charged as the sections are: a first shape with every direct cell of the three families set by a
/// formula (97 and more), fragmented into 10,000 pieces, is a million cells and more, past what a command writes (and past what the recalculation that follows holds).
/// It is refused in words by the command, before any shape is built; it was built, held 1.6 GB and was refused by the recalculation as "too large to recalculate".
#[test]
fn the_direct_cells_of_the_first_shape_times_the_shapes_made_are_charged_before_they_are_built() {
    let mut s = cut_into_squares(99);
    let set = give_every_look_cell(&mut s);
    assert!(set >= 90, "the families have many cells: {set}");
    let before = crate::snapshot(&s);
    let all: Vec<u32> = (1..=199).collect();
    let (result, took) = timed(&mut s, "shape.fragment", &json!({"ids": all}));
    assert!(took < LIMIT, "{took:?}");
    let message = refused_in_words(&result);
    assert!(message.contains("formatting") && message.contains("at most 500000"), "the command's own words, not the recalculation's: {message}");
    crate::shapeops::assert_unchanged(&s, &before, "10,000 pieces of a first shape of 97 cells");
}

/// A `Character`, `Paragraph`, `Tabs` or gradient section the command would copy and that holds more rows than a command copies (1,024) is refused in words and not left out
/// whole and silently, as a shape with fewer rows keeps them all. A Fragment gives no text and does not look at the text's sections.
#[test]
fn a_look_section_past_the_rows_a_command_copies_is_refused_in_words_and_never_dropped() {
    let two = || session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build(), rect(2, (1.0, 1.0, 3.0, 3.0)).build()]);
    for (character, paragraph, gradient, section) in [(1_100, 0, 0, "Character"), (0, 1_100, 0, "Paragraph"), (0, 0, 1_100, "FillGradient")] {
        let mut s = two();
        give_look(&mut s, character, paragraph, gradient);
        let before = crate::snapshot(&s);
        let (result, _) = timed(&mut s, "shape.union", &json!({"ids": [1, 2]}));
        let message = refused_in_words(&result);
        assert!(message.contains(&format!("`{section}`")) && message.contains("at most 1024"), "{message}");
        crate::shapeops::assert_unchanged(&s, &before, section);
    }
    // The text's sections are not looked at by a command that gives no text.
    let mut s = two();
    give_look(&mut s, 1_100, 1_100, 0);
    let (result, _) = timed(&mut s, "shape.fragment", &json!({"ids": [1, 2]}));
    assert_eq!(ids(&result.unwrap(), "ids").len(), 3);
    // And 1,000 rows (under the cap) are copied whole.
    let mut s = two();
    give_look(&mut s, 1_000, 0, 0);
    let id = made(&run(&mut s, "shape.union", json!({"ids": [1, 2]})));
    let rows: Vec<usize> = look_sections(&s, id).iter().map(|sec| sec.rows.len()).collect();
    assert_eq!(rows, [1_000]);
}
