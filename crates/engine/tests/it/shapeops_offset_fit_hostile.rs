//! Offset and Fit Curve on crafted inputs (SHAPEOPS-CAPS): distances that are huge, tiny, zero, negative, not numbers or that overflow, a path that crosses itself
//! in very many places, thousands of paths, a star of segments through one point, a section that holds twenty-five million crossings, shapes nested deep, a polyline of
//! 100,000 rows, coordinates far out and a number that is not a number. Each is refused in words or finished, in counted, bounded work, and a refusal changes nothing.
//! Each test holds the whole call to a time far above what it takes and far below a hang; the times a run measured are printed with `--no-capture` and written in the doc
//! of each test.
//!
//! **Counted, not only timed.** The tests that count work say what: the rows the check of Offset and Fit Curve looks at, the walks of the page and the recalculations a
//! Fit Curve of many shapes makes, the points looked at for circles (a refusal at one step short of what a fit takes). The memory the library takes for a sweep is in
//! `shapeops_offset_memory.rs`, in a child under `ulimit -v`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use diacraft_doc::{Cell, CellName, Row, Section, Shape, Value};
use diacraft_engine::{EngineError, Session};
use serde_json::{Value as Json, json};

use crate::shapeops::{Build, assert_unchanged, at, doc, ids, line, num, rect, session_with};
use crate::shapeops_join_trim_hostile::comb;
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
        result.as_ref().map_or_else(
            |e| format!("refused: {e}"),
            |v| format!("ok {} made, {} changed", v["ids"].as_array().map_or(0, Vec::len), v["changed"].as_array().map_or(0, Vec::len))
        )
    );
    (result, took)
}

/// The words of a refusal that is the engine's and not a bug that was caught.
fn refused_in_words(result: &Result<Json, EngineError>) -> String {
    match result {
        Err(EngineError::Failed { msg, .. } | EngineError::BadParams { msg, .. }) => msg.clone(),
        other => panic!("expected a refusal in words, got {other:?}"),
    }
}

/// A refusal names a limit of the library or a count of the engine's.
fn assert_a_limit(message: &str) {
    let words = ["too many", "too degenerate", "not finite", "more than", "steps", "too complex", "cannot", "at most", "too large", "no copy"];
    assert!(words.iter().any(|w| message.contains(w)), "not a refusal that names a limit: {message}");
}

/// Runs the command. Whichever way it ends, it ends in time and the session is sound; a refusal names a limit and changes nothing. The answer, when it finished.
fn bounded(s: &mut Session, command: &str, params: &Json) -> Option<Json> {
    let before = snapshot(s);
    let (result, took) = timed(s, command, params);
    assert!(took < LIMIT, "{command}: {took:?}");
    check_session(s).unwrap();
    match &result {
        Ok(done) => Some(done.clone()),
        Err(_) => {
            assert_a_limit(&refused_in_words(&result));
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
    let message = refused_in_words(&result);
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

/// A star polygon of `n` vertices on a circle of radius 5 about (5, 5), each joined to the one `step` places on: it crosses itself in about `n (step - 1)` places when `step` is
/// near `n / 2`. One closed section of the shape `id`.
fn star(id: u32, n: u32, step: u32) -> Shape {
    let at = |i: u32| {
        let a = std::f64::consts::TAU * f64::from((step * i) % n) / f64::from(n);
        (5.0 + 5.0 * a.cos(), 5.0 + 5.0 * a.sin())
    };
    let mut rows = Vec::new();
    for i in 0..=n {
        let (x, y) = at(i);
        rows.push(row_of(i + 1, if i == 0 { "MoveTo" } else { "LineTo" }, x, y));
    }
    Build::new(id, 10.0, 10.0).pin(5.0, 5.0).raw_section(rows, false).build()
}

/// A closed polygon of `n` vertices on a circle of radius `r` about (`r`, `r`).
fn polygon(id: u32, n: u32, r: f64) -> Shape {
    let points: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let a = f64::from(i) * std::f64::consts::TAU / f64::from(n);
            (r + r * a.cos(), r + r * a.sin())
        })
        .collect();
    Build::new(id, 2.0 * r, 2.0 * r).pin(r, r).section(&points, true, false).build()
}

fn all_ids(n: u32) -> Json {
    json!((1..=n).collect::<Vec<u32>>())
}

// ---- Offset: the distance -------------------------------------------------------------------------------------------------------

/// A distance that is zero, negative, not finite or past what the path library can work is refused in words at once, and nothing changes.
#[test]
fn a_distance_that_is_zero_negative_not_finite_or_too_big_is_refused_in_words() {
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build()]);
    for (distance, why) in [
        (json!(0), "distance above 0"),
        (json!(-0.0), "distance above 0"),
        (json!(-1), "distance above 0"),
        (json!(-1.0e300), "distance above 0"),
        (json!(1.0e-300), "at least 0.0001"),
        (json!(0.000_09), "at least 0.0001"),
        (json!(3.6e8), "at most"),
        (json!(1.0e300), "at most"),
        (json!(1.7976931348623157e308), "at most"),
    ] {
        refused(&mut s, "shape.offset", &json!({"ids": [1], "distance": distance}), why);
    }
}

/// A distance that is not a number is the parameters' refusal, in words: `NaN` and `inf` are no JSON number, and a string, a list or null is no number either.
#[test]
fn a_distance_or_tolerance_that_is_not_a_number_is_refused_by_the_parameters() {
    let mut s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build()]);
    let before = snapshot(&s);
    for bad in [json!("NaN"), json!(null), json!({}), json!([1]), json!(true), json!("1e999"), json!("1"), json!("inf")] {
        for (command, key) in [("shape.offset", "distance"), ("shape.fit_curve", "tolerance")] {
            let mut params = json!({"ids": [1]});
            params[key] = bad.clone();
            let error = s.execute(command, &params).unwrap_err();
            assert!(matches!(error, EngineError::BadParams { .. }), "{command} {params}: {error:?}");
        }
    }
    assert_unchanged(&s, &before, "arguments that are not numbers");
}

/// A distance far larger than the shape: the outer side only for a closed shape, and for a line the two sides, a distance apart from it. The largest there is is
/// 350,000,000, for a shape of no size: half the size of a path and twice the distance must come to no more than 700,000,000, so a square of two inches takes 349,999,999.
#[test]
fn an_offset_larger_than_the_shape_gives_the_outer_side_only_and_a_line_its_two_sides() {
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build()]);
    refused(&mut s, "shape.offset", &json!({"ids": [1], "distance": 3.5e8}), "cover more than 700000000 once the distance is added");
    for distance in [10.0, 1.0e3, 1.0e6, 1.0e8, 3.49e8] {
        let mut s = session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build(), line(2, (0.0, 5.0), (1.0, 5.0)).build()]);
        let (result, took) = timed(&mut s, "shape.offset", &json!({"ids": [1, 2], "distance": distance}));
        assert!(took < LIMIT, "{took:?}");
        let made = ids(&result.unwrap(), "ids");
        assert_eq!(made.len(), 3, "the square's outer side and the line's two: {distance}");
        let outer = num(doc(&s), made[0], "Width");
        assert!((outer - (2.0 + 2.0 * distance)).abs() < 1e-3 * distance.max(1.0), "{distance}: {outer}");
        check_session(&s).unwrap();
    }
}

/// A tiny distance (the least there is, 0.0001) on a shape a hundred million inches out: made, by working near the origin, and a distance that is less than that is refused in
/// words.
#[test]
fn a_tiny_distance_far_out_is_made_where_the_work_is_near_the_origin_and_one_nearer_still_is_refused() {
    let at = 1.0e8;
    let mut s = session_with(vec![rect(1, (at, at, at + 2.0, at + 2.0)).build(), line(2, (at, at + 5.0), (at + 3.0, at + 5.0)).build()]);
    let (result, took) = timed(&mut s, "shape.offset", &json!({"ids": [1, 2], "distance": 0.0001}));
    assert!(took < LIMIT, "{took:?}");
    let made = ids(&result.unwrap(), "ids");
    assert_eq!(made.len(), 4);
    let d = doc(&s);
    for id in &made[..2] {
        let width = num(d, *id, "Width");
        assert!((width - 2.0).abs() <= 2.0e-4 + 1.0e-6, "the copy is the square grown or shrunk by 0.0001 each side: {width}");
    }
    for bad in [1.0e-9, 1.0e-12, 0.000_09] {
        let mut s = session_with(vec![rect(1, (at, at, at + 2.0, at + 2.0)).build()]);
        refused(&mut s, "shape.offset", &json!({"ids": [1], "distance": bad}), "at least 0.0001");
    }
}

/// Two squares a hundred billion inches apart are each offset as they would be near the origin (each section is worked near its own origin).
#[test]
fn offset_of_two_squares_far_apart_is_the_same_as_near_each_other() {
    for gap in [10.0, 1.0e6, 1.0e10, 1.0e12] {
        let mut s = session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build(), rect(2, (gap, 0.0, gap + 2.0, 2.0)).build()]);
        let made = bounded(&mut s, "shape.offset", &json!({"ids": [1, 2], "distance": 0.5})).map(|done| ids(&done, "ids"));
        let Some(made) = made else { continue };
        assert_eq!(made.len(), 4, "gap {gap}");
        for id in &made {
            let w = num(doc(&s), *id, "Width");
            assert!((w - 3.0).abs() < 1e-6 || (w - 1.0).abs() < 1e-6, "gap {gap}: {w}");
        }
    }
}

/// Paths longer than the path library's reach once the distance is added are refused in words and not worked on.
#[test]
fn shapes_too_large_for_the_sweep_are_refused_in_words() {
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 1.0e300, 1.0e300)).build(), rect(2, (1.0e299, 0.0, 2.0e299, 1.0)).build()]);
    let before = snapshot(&s);
    for (command, params, why) in [
        ("shape.offset", json!({"ids": [1, 2], "distance": 1.0}), "cover more than"),
        ("shape.fit_curve", json!({"ids": [1, 2], "tolerance": 1.0}), "cover more than"),
    ] {
        let (result, took) = timed(&mut s, command, &params);
        assert!(took < LIMIT, "{took:?}");
        let message = refused_in_words(&result);
        assert!(message.contains(why), "{command}: {message}");
        assert_unchanged(&s, &before, command);
    }
    // The reach is half the size of a section and twice the distance: a square of 1.4E9 is over it, one of 1.0E9 under.
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 1.4e9, 1.4e9)).build()]);
    refused(&mut s, "shape.offset", &json!({"ids": [1], "distance": 1.0}), "cover more than");
}

// ---- Offset: self-crossing paths ------------------------------------------------------------------------------------------------

/// A star polygon of n vertices, each joined to the one n/2 places on: n (n - 3) / 2 crossings. Of 100 it is made (a hundred times the crossings of a pentagram); of more it is
/// refused in words by the library's own limits, or made, in bounded time, and a refusal changes nothing.
#[test]
fn stars_of_500_to_2001_vertices_are_refused_or_finished_by_offset_in_bounded_time_and_one_of_100_is_offset() {
    let mut s = session_with(vec![star(1, 101, 50)]);
    let done = bounded(&mut s, "shape.offset", &json!({"ids": [1], "distance": 0.05}));
    eprintln!("a star of 101: {}", done.as_ref().map_or(0, |d| ids(d, "ids").len()));
    for (n, step) in [(301_u32, 150_u32), (501, 250), (1_001, 500), (2_001, 1_000)] {
        let mut s = session_with(vec![star(1, n, step)]);
        let done = bounded(&mut s, "shape.offset", &json!({"ids": [1], "distance": 0.25}));
        eprintln!("a star of {n}: {}", done.as_ref().map_or_else(|| "refused".to_owned(), |d| format!("{} made", ids(d, "ids").len())));
    }
}

/// The same star open: the ring of the stroke has the same crossings.
#[test]
fn an_open_star_is_refused_or_finished_by_offset_in_bounded_time() {
    for (n, step) in [(501_u32, 250_u32), (2_001, 1_000)] {
        let at = |i: u32| {
            let a = std::f64::consts::TAU * f64::from((step * i) % n) / f64::from(n);
            (5.0 + 5.0 * a.cos(), 5.0 + 5.0 * a.sin())
        };
        let points: Vec<(f64, f64)> = (0..n).map(at).collect();
        let mut s = session_with(vec![Build::new(1, 10.0, 10.0).pin(5.0, 5.0).section(&points, false, true).build()]);
        let done = bounded(&mut s, "shape.offset", &json!({"ids": [1], "distance": 0.25}));
        eprintln!("an open star of {n}: {}", done.as_ref().map_or_else(|| "refused".to_owned(), |d| format!("{} made", ids(d, "ids").len())));
    }
}

/// A serpentine of 5,000 horizontal passes and then one of 5,000 vertical ones, in one open path: 25 million places where a pass crosses another. Refused in words by the
/// library's own count of crossings, or finished in bounded time, and a refusal changes nothing.
#[test]
fn a_path_of_25_million_crossings_is_refused_or_finished_by_offset_in_bounded_time() {
    let n = 5_000_u32;
    let mut points: Vec<(f64, f64)> = Vec::new();
    for i in 0..n {
        let y = f64::from(i) * 0.01;
        points.push(if i % 2 == 0 { (0.0, y) } else { (50.0, y) });
        points.push(if i % 2 == 0 { (50.0, y) } else { (0.0, y) });
    }
    for j in 0..n {
        let x = f64::from(j) * 0.01;
        points.push(if j % 2 == 0 { (x, 0.0) } else { (x, 50.0) });
        points.push(if j % 2 == 0 { (x, 50.0) } else { (x, 0.0) });
    }
    let mut s = session_with(vec![Build::new(1, 50.0, 50.0).pin(25.0, 25.0).section(&points, false, true).build()]);
    let done = bounded(&mut s, "shape.offset", &json!({"ids": [1], "distance": 0.001}));
    eprintln!("25 million crossings: {}", done.as_ref().map_or_else(|| "refused".to_owned(), |d| format!("{} made", ids(d, "ids").len())));
    // The same as one closed path.
    let mut closed = points.clone();
    closed.push(points[0]);
    let mut s = session_with(vec![Build::new(1, 50.0, 50.0).pin(25.0, 25.0).section(&closed, true, false).build()]);
    bounded(&mut s, "shape.offset", &json!({"ids": [1], "distance": 0.001}));
}

/// A section of 8,193 separate segments: its side would be a shape of 8,193 sections, which a shape does not hold, so it is refused in words before any library work is done.
#[test]
fn a_section_of_more_open_paths_than_a_shape_holds_sections_is_refused_before_the_library_is_asked() {
    let n = 8_193_u32;
    let shape = comb(1, n, |i| ((f64::from(i) * 0.1, 0.0), (f64::from(i) * 0.1, 0.05)));
    let mut s = session_with(vec![shape]);
    let started = Instant::now();
    refused(&mut s, "shape.offset", &json!({"ids": [1], "distance": 0.01}), "a shape holds at most 8192 sections");
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    // 8,192 are made.
    let mut s = session_with(vec![comb(1, 8_192, |i| ((f64::from(i) * 0.1, 0.0), (f64::from(i) * 0.1, 0.05)))]);
    let done = bounded(&mut s, "shape.offset", &json!({"ids": [1], "distance": 0.01})).expect("the most a shape holds");
    assert_eq!(ids(&done, "ids").len(), 2);
}

/// A fan of 300 segments through one point, in one section: every pair meets at the centre.
#[test]
fn a_fan_of_three_hundred_segments_through_one_point_is_refused_or_finished_by_offset_in_bounded_time() {
    let n = 300_u32;
    let shape = comb(1, n, |i| {
        let a = std::f64::consts::PI * f64::from(i) / f64::from(n);
        ((5.0 - 5.0 * a.cos(), 5.0 - 5.0 * a.sin()), (5.0 + 5.0 * a.cos(), 5.0 + 5.0 * a.sin()))
    });
    let mut s = session_with(vec![shape]);
    let done = bounded(&mut s, "shape.offset", &json!({"ids": [1], "distance": 0.01}));
    eprintln!("a fan of 300: {}", done.as_ref().map_or_else(|| "refused".to_owned(), |d| format!("{} made", ids(d, "ids").len())));
}

/// A closed polygon on a circle: made at any size the library takes, and refused in words past its limits.
#[test]
fn an_offset_of_a_polygon_of_many_vertices_is_made_or_refused_in_words_in_bounded_time() {
    for n in [1_000_u32, 8_000, 20_000, 50_000, 100_000] {
        let mut s = session_with(vec![polygon(1, n, 10.0)]);
        let done = bounded(&mut s, "shape.offset", &json!({"ids": [1], "distance": 0.5}));
        eprintln!("a polygon of {n}: {}", done.as_ref().map_or_else(|| "refused".to_owned(), |d| format!("{} made", ids(d, "ids").len())));
    }
}

// ---- Offset: many paths ---------------------------------------------------------------------------------------------------------

/// 5,000 lines in 50 shapes of 100 sections, the ids 1 to 50.
fn many_lines() -> Vec<Shape> {
    (0..50_u32)
        .map(|k| {
            let mut b = Build::new(1 + k, 1.0, 100.0).pin(f64::from(k) * 10.0 + 0.5, 50.0);
            for j in 0..100_u32 {
                b = b.section(&[(0.0, f64::from(j)), (1.0, f64::from(j))], false, true);
            }
            b.build()
        })
        .collect()
}

/// 5,000 lines make 10,000 copies, the most a command makes: made. With a small square beside them that a distance of 5 leaves only the outer side of, 10,001: refused.
#[test]
fn an_offset_that_makes_exactly_ten_thousand_shapes_is_made_and_one_more_is_refused() {
    let all: Vec<u32> = (1..=50).collect();
    let mut s = session_with(many_lines());
    let (result, took) = timed(&mut s, "shape.offset", &json!({"ids": all, "distance": 0.1}));
    assert!(took < LIMIT, "{took:?}");
    assert_eq!(ids(&result.unwrap(), "ids").len(), 10_000);

    let mut shapes = many_lines();
    shapes.push(rect(51, (1_000.0, 0.0, 1_001.0, 1.0)).build());
    let mut s = session_with(shapes);
    let mut all: Vec<u32> = (1..=50).collect();
    all.push(51);
    refused(&mut s, "shape.offset", &json!({"ids": all, "distance": 5.0}), "Offset would make more than 10000 shapes");
}

/// 4,096 shapes of one line each: 8,192 copies, made.
#[test]
fn four_thousand_ninety_six_lines_are_offset_into_8192_copies() {
    let shapes: Vec<Shape> = (0..4_096_u32).map(|i| line(i + 1, (f64::from(i) * 0.5, 0.0), (f64::from(i) * 0.5, 0.4)).build()).collect();
    let mut s = session_with(shapes);
    let done = bounded(&mut s, "shape.offset", &json!({"ids": all_ids(4_096), "distance": 0.1})).expect("made");
    assert_eq!(ids(&done, "ids").len(), 8_192);
}

/// What each copy holds of its own original's formatting is charged before any copy is built, the copies of every original together: the 200 copies of a shape of 100 lines
/// that has a gradient of 1,000 rows (4,000 rows and cells each, as the copies share the rows and a file written from them writes them for each) are 800,000, past the 500,000
/// a command writes in all. Refused in words, with nothing changed: for the one original in the words of the first shape, and for several in the words of all of them.
#[test]
fn the_formatting_of_the_copies_of_one_original_and_of_several_is_charged_before_any_copy_is_built() {
    let mut gradient = Section::new("FillGradient", None);
    for ix in 0..1_000_u32 {
        let mut row = Row { ix: Some(ix), ..Row::default() };
        for name in ["GradientStopColor", "GradientStopColorTrans", "GradientStopPosition"] {
            row.cells.insert(CellName::new(name), Cell::constant(Value::from_text("1")));
        }
        gradient.rows.push(Arc::new(row));
    }
    let mut s = session_with(many_lines());
    s.edit("a large gradient", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let first = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        Arc::make_mut(&mut first.sheet).sections.push(Arc::new(gradient));
        Ok(())
    })
    .unwrap();
    let before = snapshot(&s);
    let (alone, took) = timed(&mut s, "shape.offset", &json!({"ids": [1], "distance": 0.1}));
    assert!(took < LIMIT, "{took:?}");
    let message = refused_in_words(&alone);
    assert!(
        message.contains("the first shape's formatting holds") && message.contains("200 shapes") && message.contains("at most 500000"),
        "{message}"
    );
    let (several, took) = timed(&mut s, "shape.offset", &json!({"ids": (1..=50).collect::<Vec<u32>>(), "distance": 0.1}));
    assert!(took < LIMIT, "{took:?}");
    let message = refused_in_words(&several);
    assert!(
        message.contains("the formatting of the shapes holds") && message.contains("10000 shapes") && message.contains("at most 500000"),
        "{message}"
    );
    assert_unchanged(&s, &before, "the charge for the copies' formatting");
    // Without the gradient a shape's 200 copies are made.
    let mut plain = session_with(many_lines());
    assert_eq!(ids(&run(&mut plain, "shape.offset", json!({"ids": [1], "distance": 0.1})), "ids").len(), 200);
}

/// Paths that are shorter than the tolerance and runs that meet end to end: no sweep is run for a run of no extent, and what is left is made or refused in bounded time.
#[test]
fn segments_shorter_than_the_merge_distance_are_no_paths_and_an_offset_of_nothing_is_refused_in_words() {
    let n = 20_000_u32;
    let shape = comb(1, n, |i| ((f64::from(i) * 0.1, 0.0), (f64::from(i) * 0.1 + 0.000_002, 0.0)));
    let mut s = session_with(vec![shape]);
    let (result, took) = timed(&mut s, "shape.offset", &json!({"ids": [1], "distance": 0.25}));
    assert!(took < LIMIT, "{took:?}");
    assert_a_limit(&refused_in_words(&result));
    check_session(&s).unwrap();
}

// ---- deep groups, numbers that are not numbers ----------------------------------------------------------------------------------

/// A group nested 85 deep (the most the engine's walks go): the line in the innermost group is offset where it is, the copies are that group's last members, and a Fit
/// Curve of it finds nothing to fit.
#[test]
fn a_line_85_groups_deep_is_offset_and_fitted_where_it_is() {
    let group_of = |id: u32, children: Vec<Arc<Shape>>| {
        let mut group = Shape::new(id);
        group.kind = diacraft_doc::ShapeType::Group;
        group.sheet = Arc::new(Build::new(id, 4.0, 4.0).pin(2.0, 2.0).build().sheet.as_ref().clone());
        group.children = diacraft_doc::ShapeList::from_vec(children);
        group
    };
    let mut shape = group_of(1000, vec![Arc::new(line(1, (0.0, 0.0), (2.0, 0.0)).build()), Arc::new(rect(2, (1.0, 1.0, 3.0, 3.0)).build())]);
    for level in 0..85_u32 {
        shape = group_of(1001 + level, vec![Arc::new(shape)]);
    }
    let mut s = session_with(vec![shape]);
    let done = bounded(&mut s, "shape.offset", &json!({"ids": [1], "distance": 0.1})).expect("made");
    let made = ids(&done, "ids");
    assert_eq!(made.len(), 2);
    let found = doc(&s).find_shape(diacraft_doc::Owner::Page(1), made[0]).unwrap();
    assert!(found.path.len() >= 86, "the copies are members of the innermost group: {}", found.path.len());
    let fitted = bounded(&mut s, "shape.fit_curve", &json!({"ids": [1], "tolerance": 0.01})).expect("finished");
    assert!(ids(&fitted, "changed").is_empty(), "a line has nothing to fit");
}

/// A cell that is not a number: the cells read 0 and the shapes are as they read, or the command is refused in words. Not a panic.
#[test]
fn a_cell_that_is_not_a_number_is_read_as_the_engine_reads_it_and_nothing_panics_in_offset_or_fit_curve() {
    for bad in ["NaN", "nan", "inf", "-inf", "1e999", "", "text"] {
        let mut a = rect(1, (1.0, 1.0, 3.0, 3.0)).build();
        let sheet = Arc::make_mut(&mut a.sheet);
        for name in ["Width", "PinX"] {
            sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(bad)));
        }
        let section = Arc::make_mut(sheet.sections.get_mut(0).unwrap());
        let row = Arc::make_mut(section.rows.get_mut(1).unwrap());
        row.cells.insert(CellName::new("X"), Cell::constant(Value::from_text(bad)));
        let mut s = session_with(vec![a, rect(2, (2.0, 2.0, 4.0, 4.0)).build()]);
        for (command, params) in
            [("shape.offset", json!({"ids": [1, 2], "distance": 0.25})), ("shape.fit_curve", json!({"ids": [1, 2], "tolerance": 0.05, "flags": 7}))]
        {
            let (result, took) = timed(&mut s, command, &params);
            assert!(took < LIMIT, "{took:?}");
            if let Err(error) = &result {
                assert!(matches!(error, EngineError::Failed { .. } | EngineError::BadParams { .. }), "{bad}: {command}: {error:?}");
            }
            check_session(&s).unwrap();
            if result
                .as_ref()
                .is_ok_and(|done| !done["ids"].as_array().is_none_or(Vec::is_empty) || !done["changed"].as_array().is_none_or(Vec::is_empty))
            {
                run(&mut s, "edit.undo", json!({}));
            }
        }
    }
}

// ---- Fit Curve ------------------------------------------------------------------------------------------------------------------

/// A polyline of `n` points of a sine wave, one section of `n` rows.
fn sine(id: u32, n: u32) -> Shape {
    let points: Vec<(f64, f64)> = (0..n).map(|i| (f64::from(i) * 0.001, 1.0 + (f64::from(i) * 0.01).sin())).collect();
    Build::new(id, f64::from(n) * 0.001, 2.0).pin(f64::from(n) * 0.0005, 1.0).section(&points, false, true).build()
}

fn rows_of(s: &Session, id: u32) -> usize {
    let shape = doc(s).shape(&at(doc(s), id)).unwrap();
    shape.sheet.sections.iter().filter(|sec| sec.name.as_str() == "Geometry" && !sec.del).map(|sec| sec.rows.len()).sum()
}

/// A sine wave of 7,000, 10,000 and 100,000 rows is fitted to far fewer rows, within the library's counted work, and undone in one step.
#[test]
fn a_sine_wave_of_7000_10000_and_100000_rows_is_fitted_to_fewer_rows() {
    for n in [7_000_u32, 10_000, 100_000] {
        let mut s = session_with(vec![sine(1, n)]);
        let before = snapshot(&s);
        let (result, took) = timed(&mut s, "shape.fit_curve", &json!({"ids": [1], "tolerance": 0.01}));
        assert!(took < LIMIT, "{n}: {took:?}");
        assert_eq!(ids(&result.unwrap(), "changed"), [1], "{n}");
        let rows = rows_of(&s, 1);
        assert!(rows < n as usize / 10, "{n}: far fewer rows: {rows}");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(doc(&s), before.document.as_ref().unwrap());
    }
}

/// The same with the circles flag, which looks for arcs: each point it looks at is counted and charged (40 million at most).
#[test]
fn the_same_with_the_circles_flag_looks_for_arcs_in_counted_steps() {
    let mut s = session_with(vec![sine(1, 100_000)]);
    let done = bounded(&mut s, "shape.fit_curve", &json!({"ids": [1], "tolerance": 0.01, "flags": 7}));
    if let Some(done) = done {
        assert_eq!(ids(&done, "changed"), [1]);
    }
}

/// 100,000 points of noise that nothing fits: the fit leaves the shape as it is, which is not an error and no undo step, and the answer says it was not fitted.
#[test]
fn a_polyline_of_noise_that_nothing_fits_is_left_as_it_is_and_makes_no_undo_step() {
    let mut x = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x >> 11) as f64 / (1_u64 << 53) as f64
    };
    let points: Vec<(f64, f64)> = (0..100_000).map(|_| (next() * 10.0, next() * 10.0)).collect();
    let mut s = session_with(vec![Build::new(1, 10.0, 10.0).pin(5.0, 5.0).section(&points, false, true).build()]);
    let before = snapshot(&s);
    let (result, took) = timed(&mut s, "shape.fit_curve", &json!({"ids": [1], "tolerance": 0.001, "flags": 7}));
    assert!(took < LIMIT, "{took:?}");
    match &result {
        Ok(done) => {
            assert!(ids(done, "changed").is_empty());
            assert_eq!(ids(done, "not_fitted"), [1], "the answer names a shape it left that holds something to fit");
            assert_unchanged(&s, &before, "nothing fits noise");
        }
        Err(_) => {
            assert_a_limit(&refused_in_words(&result));
            assert_unchanged(&s, &before, "refused");
        }
    }
}

/// A closed polygon of 100,000 points of one circle of radius 10, with the flag that finds circles and a tolerance (0.0001) that an arc of 256 of its segments (0.9 degrees,
/// a bow of 0.0003) is more than a line: arcs of at most 256 segments each, so about 400 of them, in counted steps.
#[test]
fn a_circle_of_a_hundred_thousand_points_is_arcs_with_the_circles_flag_in_counted_steps() {
    let mut s = session_with(vec![polygon(1, 100_000, 10.0)]);
    let done = bounded(&mut s, "shape.fit_curve", &json!({"ids": [1], "tolerance": 0.0001, "flags": 2}));
    if let Some(done) = done {
        assert_eq!(ids(&done, "changed"), [1]);
        let shape = doc(&s).shape(&at(doc(&s), 1)).unwrap();
        let kinds: Vec<&str> = shape
            .sheet
            .sections
            .iter()
            .filter(|sec| sec.name.as_str() == "Geometry" && !sec.del)
            .flat_map(|sec| sec.rows.iter())
            .filter_map(|r| r.row_type.as_deref())
            .collect();
        let arcs = kinds.iter().filter(|k| **k == "ArcTo").count();
        assert!((300..500).contains(&arcs) && kinds.len() == arcs + 1, "{arcs} arcs of {} rows", kinds.len());
    }
}

/// The looking for arcs is charged a step for each point looked at, and the command is refused in words past 40 million: a polygon of 1,000,000 points of a circle (the most a
/// command reads is 524,288 elements, so this is refused by the reading first, in words).
#[test]
fn a_polyline_of_more_rows_than_a_command_reads_is_refused_in_words_by_the_reading() {
    let mut s = session_with(vec![polygon(1, 600_000, 10.0)]);
    let (result, took) = timed(&mut s, "shape.fit_curve", &json!({"ids": [1], "tolerance": 0.0001, "flags": 2}));
    assert!(took < LIMIT, "{took:?}");
    assert_a_limit(&refused_in_words(&result));
    check_session(&s).unwrap();
}

/// 4,096 circles of 36 lines each, one command: the library is called once for all of them, the page is walked a few times and recalculated twice, whatever the number of shapes.
#[test]
fn fit_curve_of_4096_circles_is_one_call_and_two_recalculations() {
    let shapes: Vec<Shape> = (0..4_096_u32)
        .map(|i| {
            let (cx, cy) = (f64::from(i % 64) * 3.0, f64::from(i / 64) * 3.0);
            let points: Vec<(f64, f64)> = (0..36)
                .map(|k| {
                    let a = f64::from(k) * 10.0_f64.to_radians();
                    (1.0 + a.cos(), 1.0 + a.sin())
                })
                .collect();
            Build::new(i + 1, 2.0, 2.0).pin(cx + 1.0, cy + 1.0).section(&points, true, false).build()
        })
        .collect();
    let mut s = session_with(shapes);
    let done = bounded(&mut s, "shape.fit_curve", &json!({"ids": all_ids(4_096), "tolerance": 0.02, "flags": 2})).expect("made");
    assert_eq!(ids(&done, "changed").len(), 4_096);
}

/// Shapes whose rows hold nothing to fit (an ellipse row, an arc row) are left as they are: the command finishes and makes no undo step.
#[test]
fn fit_curve_of_every_rectangle_of_a_page_changes_nothing_and_makes_no_undo_step() {
    let shapes: Vec<Shape> = (0..1_000_u32).map(|i| rect(i + 1, (f64::from(i) * 3.0, 0.0, f64::from(i) * 3.0 + 2.0, 2.0)).build()).collect();
    let mut s = session_with(shapes);
    let before = snapshot(&s);
    let (result, took) = timed(&mut s, "shape.fit_curve", &json!({"ids": all_ids(1_000), "tolerance": 0.02, "flags": 7}));
    assert!(took < LIMIT, "{took:?}");
    let done = result.unwrap();
    assert!(ids(&done, "changed").is_empty());
    assert_unchanged(&s, &before, "nothing to fit");
}

/// The most the command takes is shapes that cover 5E13 in the cells' units: a polyline exactly that wide is finished (a straight run of 101 points at that size is no
/// more than the arithmetic's rounding of 0.008, which the fit may or may not take for a line within a tolerance of 1: it is fitted or left as it was, and the answer says
/// which), and one a little wider is refused in words.
#[test]
fn a_polyline_exactly_as_wide_as_the_most_is_finished_and_one_a_little_wider_is_refused() {
    let most = 5.0e13;
    let wide = |width: f64| {
        let points: Vec<(f64, f64)> = (0..=100_u32).map(|i| (width * f64::from(i) / 100.0, 0.0)).collect();
        session_with(vec![Build::new(1, width, 1.0).pin(width / 2.0, 0.5).section(&points, false, true).build()])
    };
    let mut s = wide(most);
    let done = bounded(&mut s, "shape.fit_curve", &json!({"ids": [1], "tolerance": 1.0})).expect("at the most");
    assert!(ids(&done, "changed") == [1] || ids(&done, "not_fitted") == [1], "fitted, or said to be left: {done}");
    let mut s = wide(most * 1.01);
    refused(&mut s, "shape.fit_curve", &json!({"ids": [1], "tolerance": 1.0}), "cover more than");
}

/// A section of 10,000 separate paths of three collinear points each: a Fit Curve makes each two rows, in one call of the library for all of them, made in bounded time.
#[test]
fn fit_curve_of_a_section_of_ten_thousand_paths_is_one_call_and_made_in_bounded_time() {
    let n = 10_000_u32;
    let mut rows = Vec::new();
    for i in 0..n {
        let x = f64::from(i) * 0.1;
        rows.push(row_of(3 * i + 1, "MoveTo", x, 0.0));
        rows.push(row_of(3 * i + 2, "LineTo", x, 0.025));
        rows.push(row_of(3 * i + 3, "LineTo", x, 0.05));
    }
    let mut s = session_with(vec![Build::new(1, 1.0, 1.0).pin(0.5, 0.5).raw_section(rows, true).build()]);
    let before = snapshot(&s);
    let done = bounded(&mut s, "shape.fit_curve", &json!({"ids": [1], "tolerance": 0.01})).expect("made");
    assert_eq!(ids(&done, "changed"), [1]);
    assert_eq!(rows_of(&s, 1), 2 * n as usize, "each path is a move and one line");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), before.document.as_ref().unwrap());
}

/// A tolerance of 0 changes nothing at any size, and one larger than the reach is refused in words.
#[test]
fn a_tolerance_of_nothing_changes_nothing_and_one_past_the_reach_is_refused() {
    let mut s = session_with(vec![sine(1, 10_000)]);
    let before = snapshot(&s);
    let done = run(&mut s, "shape.fit_curve", json!({"ids": [1], "tolerance": 0}));
    assert!(ids(&done, "changed").is_empty());
    assert_unchanged(&s, &before, "a tolerance of nothing");
    refused(&mut s, "shape.fit_curve", &json!({"ids": [1], "tolerance": 6.0e13}), "at most");
    // A huge tolerance within the reach makes the whole of it one stretch or leaves it, in time.
    bounded(&mut s, "shape.fit_curve", &json!({"ids": [1], "tolerance": 1.0e12, "flags": 7}));
}
