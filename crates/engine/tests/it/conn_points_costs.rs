//! What a call and the points it makes cost, counted (design §4.2, §9). No test asserts elapsed time: they count the units the loops walk
//! (arc calls, rows written, formulas scanned, ends renumbered, calc's steps), and the timed ones are behind `#[ignore]` beside a yardstick.
//!
//! The costliest shape the caps allow, by calc's steps for a resize, and the numbers the report quotes, are printed by `measured_costs` (`cargo test -- --ignored --nocapture`).

use diacraft_calc::{Budget, EvalOptions};
use diacraft_doc::{Cell, CellAddr, CellEdit, Connect, Connects, Document, Owner, RowKey, SheetRef, Value as CellValue, WriteOrigin};
use diacraft_engine::draw::conn_points::{
    Action, MAX_ARC_CALLS, MAX_FOLLOW_COST, MAX_FOLLOW_STEPS, MAX_POINTS_PER_SHAPE, MAX_REGLUED_ENDS, MAX_ROWS_WRITTEN, Request, change_points,
};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::json;

use crate::conn_points::{act, circle, params, places, refused};
use crate::conn_points_edges::{at, base, long_spline, page_of, point, rectangle, row, session_with, zigzag};
use crate::edit::{doc, drop_shape};
use crate::{blank_session, check_session, run};

/// What calc spends when the cell `cell` of shape `id` becomes `value`, as a commit counts it: its steps and the formula cells it worked out (a commit that is not a session's
/// builds the page's index of readers anew, which costs steps of its own and works out no cell).
fn commit_stats(before: &Document, id: u32, cell: &str, value: f64) -> (u64, u64) {
    let mut after = before.clone();
    let sheet = SheetRef::Shape(before.find_shape(Owner::Page(1), id).unwrap());
    let written = Cell::constant(CellValue::from_text(&value.to_string())).with_unit("IN");
    after.write_cell(&sheet, &CellAddr::direct(cell), CellEdit::Cell(written), WriteOrigin::Drawing).unwrap();
    let changes = diacraft_calc::changes_between(before, &after, 1);
    let report = diacraft_calc::commit_changed(&mut after, &Budget::with_steps(Budget::DEFAULT_STEPS), EvalOptions::default(), 1, &changes);
    assert!(!report.budget_exhausted, "an edit stays within calc's budget");
    (report.stats.steps, report.stats.cells_worked)
}

/// The steps calc spends on the cells that read `Width` of shape `id` when it becomes `width`: the path cells of its doubled points, and nothing else.
fn resize_steps(d: &Document, id: u32, width: f64) -> u64 {
    commit_stats(d, id, "Width", width).0
}

/// What calc spends when shape `id` is moved: an edit of the shape's pin, which no path cell reads.
fn move_stats(d: &Document, id: u32) -> (u64, u64) {
    commit_stats(d, id, "PinX", 9.0)
}

fn request(ids: &[u32], action: Action) -> Request {
    Request { ids: ids.to_vec(), action, edge: None, at: None, min_gap: diacraft_engine::draw::conn_points::DEFAULT_MIN_GAP }
}

/// What a doubling of every edge of shape `id` at the smallest minimum distance does to `d`: the points the shape holds and what its path-form points are charged.
fn double_small(d: &mut Document, id: u32) -> (usize, u64) {
    let answer = change_points(d, 1, &Request { min_gap: 0.01, ..request(&[id], Action::Double) }).unwrap();
    (answer.shapes[0].points_after, answer.follow_cost)
}

/// DiaCraft's Ellipse doubled until it holds 256 points (4 -> 8 -> ... -> 256), at the smallest minimum distance, on a page that holds a Rectangle too: the document, the two
/// shapes and the last charge.
fn ellipse_of_256() -> (Document, u32, u32, u64) {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "ellipse", 1.0, 1.0);
    let other = drop_shape(&mut s, "rectangle", 6.0, 6.0);
    let mut d = doc(&s).clone();
    let mut charged = 0;
    for want in [8, 16, 32, 64, 128, 256] {
        let (points, cost) = double_small(&mut d, id);
        assert_eq!(points, want);
        charged = cost;
    }
    (d, id, other, charged)
}

#[test]
fn a_resize_of_an_ellipse_of_256_points_spends_under_the_charge_and_a_move_of_another_shape_works_out_none_of_its_path_cells() {
    let (d, id, other, charged) = ellipse_of_256();
    let (steps, worked) = commit_stats(&d, id, "Width", 3.0);
    assert!(worked >= 2 * 252, "the 252 path cells of each axis are worked out again when the size changes: {worked}");
    assert!(steps <= charged, "the charge of {charged} calc steps is never less than the {steps} a resize spends");
    assert!(charged <= MAX_FOLLOW_COST, "{charged}");
    // Two walks of a 15-step section for each of the 252 points: well under a fiftieth of the cap.
    assert!(steps < MAX_FOLLOW_COST / 20, "{steps}");
    // A move of the other shape works out what it works out on a page that has no ellipse (none of its cells, nor any of the 252 path cells).
    let mut alone = blank_session();
    let lone = drop_shape(&mut alone, "rectangle", 6.0, 6.0);
    assert_eq!(move_stats(&d, other).1, move_stats(doc(&alone), lone).1, "an edit of another shape works out none of the path cells");
    // Nor does a move of the ellipse itself: its path cells read its size and its curve, not where it is.
    assert_eq!(move_stats(&d, id).1, 0, "a move of the ellipse works out no cell");
}

#[test]
fn a_section_of_1025_steps_cannot_be_followed_by_formula_a_section_of_1024_can_and_a_straight_edge_of_the_long_one_needs_no_walk() {
    // `long_spline(id, knots, points)` draws `knots - 1` cubics after a move: the move is a step too.
    let steps_cap = u32::try_from(MAX_FOLLOW_STEPS).unwrap();
    let mut s = session_with(page_of(vec![long_spline(1, steps_cap, 0), long_spline(2, steps_cap + 1, 0)]));
    let answer = act(&mut s, 1, "double", json!({}));
    assert_eq!(answer["shapes"][0]["points_after"], 1, "a section of exactly the most is followed");
    let words = refused(&mut s, &params(2, "double", json!({})));
    assert!(words.contains("the curve of shape 2 draws") && words.contains("steps, past the 1024 a point can follow by formula"), "{words}");
    // A straight edge of such a section is doubled: it needs no walk.
    let line = at(long_spline(3, steps_cap + 1, 0), 0, 5_000, "LineTo", 0.0, 3.0);
    let mut t = session_with(page_of(vec![line]));
    let edges = run(&mut t, "shape.connection_edges", json!({"id": 3}))["shapes"][0]["edges"].clone();
    let straight = edges.as_array().unwrap().iter().position(|e| e["kind"] == "straight").unwrap();
    let answer = act(&mut t, 3, "double", json!({"edge": straight}));
    assert_eq!(answer["changed"], true);
    assert_eq!(check_session(&t), Ok(()));
}

#[test]
fn the_costliest_shape_the_caps_allow_is_charged_at_least_what_a_resize_spends_and_the_next_doubling_is_refused_by_its_charge() {
    // A spline of 1,024 steps with no point: 0 -> 1 -> 3 -> ... -> 127 adds 1, 2, 4, ... 64 path-form points (127 in all), each two walks of a section that is charged some 7,600
    // calc steps a walk: 1.93 million, under the follow cap of 2 million and over the 1.87 million a resize measured. The next doubling would add 128 more: 255 points are charged
    // past the cap (and fit the 256 points a shape may hold), so the shape is skipped in words with the 127 it has.
    let steps_cap = u32::try_from(MAX_FOLLOW_STEPS).unwrap();
    let mut d = page_of(vec![long_spline(1, steps_cap, 0)]);
    let mut charged = 0;
    for want in [1, 3, 7, 15, 31, 63, 127] {
        let (points, cost) = double_small(&mut d, 1);
        assert_eq!(points, want);
        assert!(cost <= MAX_FOLLOW_COST, "{cost}");
        charged = cost;
    }
    let steps = resize_steps(&d, 1, f64::from(steps_cap) * 1.5);
    assert!(
        steps > 0 && steps <= charged,
        "a resize spends {steps} calc steps on 127 path-form points on a section of 1,024 steps, and the charge is {charged}"
    );
    assert!(charged > MAX_FOLLOW_COST / 10 * 9, "the costliest shape the caps allow is charged within a tenth of the cap: {charged}");
    let words = change_points(&mut d, 1, &Request { min_gap: 0.01, ..request(&[1], Action::Double) }).unwrap_err().message;
    assert!(words.contains("calc steps") && words.contains(&format!("past the {MAX_FOLLOW_COST}")), "{words}");
    assert_eq!(doc_points(&d), 127, "nothing was written");
}

/// How many connection points shape 1 of `d` holds.
fn doc_points(d: &Document) -> usize {
    let mut s = blank_session();
    s.edit("test edit", |doc| {
        *doc = d.clone();
        Ok(())
    })
    .unwrap();
    places(&s, 1).len()
}

// ---- counted work of the edit -----------------------------------------------------------------------------------------------------------

#[test]
fn the_arc_calls_of_a_doubling_are_the_reading_and_one_place_for_each_new_point() {
    // The ellipse is `n` cubics. Reading costs a length for each, each of its four points is tried against every piece and placed by one more length (`n + 4 (n + 1)`, as the
    // query counts it); each of the four new points is placed by one more.
    let mut s = blank_session();
    let id = drop_shape(&mut s, "ellipse", 3.0, 2.0);
    let n = run(&mut s, "shape.connection_edges", json!({"id": id}))["shapes"][0]["edges"][0]["steps"].as_u64().unwrap() as usize;
    let mut d: Document = doc(&s).clone();
    let answer = change_points(&mut d, 1, &request(&[id], Action::Double)).unwrap();
    assert_eq!(answer.work.arc_calls, n + 4 * (n + 1) + 4, "{n} pieces");
    assert!(answer.work.arc_calls <= MAX_ARC_CALLS);
}

#[test]
fn a_call_on_two_hundred_and_fifty_eight_shapes_of_127_points_is_refused_for_the_rows_it_would_write() {
    // 258 shapes of 127 points on one open edge each double to 255: 258 * 255 rows is past the 65,536 a call writes. Refused in words, nothing written.
    let shapes: Vec<ShapeBuilder> = (1..=258).map(|id| crate::conn_points::crowded(id, 127)).collect();
    let built = page_of(shapes);
    let mut d = built.clone();
    let ids: Vec<u32> = (1..=258).collect();
    let words = change_points(&mut d, 1, &request(&ids, Action::Double)).unwrap_err().message;
    assert!(words.contains("65536") && words.contains("rows"), "{words}");
    assert_eq!(d, built, "nothing was written");
    // 257 shapes make 65,535 rows: within the cap.
    let mut d = built.clone();
    let ids: Vec<u32> = (1..=257).collect();
    let answer = change_points(&mut d, 1, &request(&ids, Action::Double)).unwrap();
    assert!(answer.changed && 257 * usize::try_from(MAX_POINTS_PER_SHAPE - 1).unwrap() <= MAX_ROWS_WRITTEN);
}

/// A rectangle (shape 1) of 2 by 1 with one original point at the top's middle and one added by a doubling (so a halving removes it), and `connectors` plain connectors glued to
/// the added point (row 1) with their `Connect` rows.
fn crowd_of_connectors(connectors: u32) -> Document {
    let mut shapes = vec![point(rectangle(1, 2.0, 1.0), 0, 1.0, 1.0)];
    for i in 0..connectors {
        shapes.push(
            ShapeBuilder::new(10 + i)
                .cell("BeginX", "1.5")
                .cell("BeginY", "1")
                .cell("EndX", "5")
                .cell("EndY", "3")
                .cell("PinX", "3.25")
                .cell("PinY", "2")
                .cell("Width", "3.8")
                .cell("Height", "0")
                .cell("LocPinX", "1.9")
                .cell("LocPinY", "0"),
        );
    }
    let mut d = DocBuilder::new().page(1, shapes.into_iter().fold(PageBuilder::new(), PageBuilder::shape)).build();
    // The doubling makes the added point, whose connectors are then glued to it.
    let answer = change_points(&mut d, 1, &Request { edge: Some(2), ..request(&[1], Action::Double) }).unwrap();
    assert!(answer.changed);
    let page = std::sync::Arc::make_mut(d.pages.get_mut(0).unwrap());
    let rows: Vec<Connect> = (0..connectors)
        .map(|i| Connect {
            from_sheet: 10 + i,
            from_cell: Some("BeginX".into()),
            from_part: Some(9),
            to_sheet: 1,
            to_cell: Some("Connections.X2".into()),
            to_part: Some(101),
            ..Connect::default()
        })
        .collect();
    page.connects = Connects::from_vec(rows);
    d
}

#[test]
fn four_thousand_ninety_six_connectors_on_a_removed_point_are_reglued_and_one_more_is_refused_in_words() {
    let built = crowd_of_connectors(u32::try_from(MAX_REGLUED_ENDS).unwrap());
    let mut d = built.clone();
    let answer = change_points(&mut d, 1, &Request { edge: Some(2), ..request(&[1], Action::Halve) }).unwrap();
    assert_eq!(answer.reglued.len(), MAX_REGLUED_ENDS);
    assert_eq!(answer.scan.ends, MAX_REGLUED_ENDS, "each Connect row is counted once");
    assert!(answer.reglued.iter().all(|r| r.to == Some(0)), "to the one point that is left on the edge");
    let built = crowd_of_connectors(u32::try_from(MAX_REGLUED_ENDS).unwrap() + 1);
    let mut d = built.clone();
    let words = change_points(&mut d, 1, &Request { edge: Some(2), ..request(&[1], Action::Halve) }).unwrap_err().message;
    assert!(words.contains("4096 connector ends") && words.contains("shape 1"), "{words}");
    assert_eq!(d, built, "nothing was changed");
}

#[test]
fn a_page_of_shapes_naming_a_row_is_walked_once_for_the_halving_and_counted() {
    // 3,000 shapes each hold one formula that names `Sheet.1!Connections.X1` (row 0, which stays): the walk looks at each formula once, and rewrites none.
    let mut shapes = vec![point(point(rectangle(1, 2.0, 1.0), 0, 1.0, 1.0), 1, 1.5, 1.0)];
    for i in 0..3_000u32 {
        shapes.push(ShapeBuilder::new(10 + i).formula("Width", "Sheet.1!Connections.X1", "1"));
    }
    let mut d = DocBuilder::new().page(1, shapes.into_iter().fold(PageBuilder::new(), PageBuilder::shape)).build();
    // Mark the second point as added (the marker names row 2, `Connections.X2`) by hand.
    {
        let page = std::sync::Arc::make_mut(d.pages.get_mut(0).unwrap());
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
        let marker = diacraft_doc::CellAddr::in_row("User", None, RowKey::name("DiaCraftAddedPoints"), "Value");
        sheet.set_cell(&marker, Cell::with_formula("0*(1*Connections.X2)", Some(CellValue::from_text("0")))).unwrap();
    }
    let answer = change_points(&mut d, 1, &Request { edge: Some(2), ..request(&[1], Action::Halve) }).unwrap();
    assert!(answer.scan.cells >= 3_000, "every formula of the page was looked at: {}", answer.scan.cells);
    assert!(answer.scan.cells <= 3_100, "and only once: {}", answer.scan.cells);
    assert_eq!(answer.renumbered, 0);
}

// ---- degenerate shapes through the command ---------------------------------------------------------------------------------------------

#[test]
fn degenerate_curves_do_not_hang_or_panic_and_leave_the_document_valid() {
    // An ellipse with a zero axis, one of cells that are no numbers, a cusp and a cubic with control points a million inches apart, doubled through the command.
    let flat = row(base(1, 2.0, 2.0), 0, 1, "Ellipse", &[("X", "1"), ("Y", "1"), ("A", "2"), ("B", "1"), ("C", "1"), ("D", "1")]);
    let words = row(base(2, 2.0, 2.0), 0, 1, "Ellipse", &[("X", "one"), ("Y", "two"), ("A", "three"), ("B", "four"), ("C", "five"), ("D", "six")]);
    let cubic = |id: u32, cells: [(&str, &str); 4]| {
        let s = at(base(id, 1.0, 1.0), 0, 1, "MoveTo", 0.0, 0.0);
        row(s, 0, 2, "RelCubBezTo", &[("X", "1"), ("Y", "0"), cells[0], cells[1], cells[2], cells[3]])
    };
    let cusp = cubic(3, [("A", "1"), ("B", "0"), ("C", "0"), ("D", "0")]);
    let far = cubic(4, [("A", "1000000"), ("B", "1000000"), ("C", "-1000000"), ("D", "-1000000")]);
    let mut s = session_with(page_of(vec![flat, words, cusp, far]));
    for id in 1..=4 {
        let _ = s.execute("shape.connection_points", &params(id, "double", json!({})));
        assert_eq!(check_session(&s), Ok(()), "shape {id}");
    }
}

#[test]
fn a_shape_of_200000_rows_is_skipped_by_its_edges_and_an_edge_named_in_it_is_still_doubled_within_the_caps() {
    let mut d = page_of(vec![zigzag(1, 199_999)]);
    let words = change_points(&mut d, 1, &request(&[1], Action::Double)).unwrap_err().message;
    assert!(words.contains("4096") || words.contains("edges"), "{words}");
    let answer = change_points(&mut d, 1, &Request { edge: Some(10), ..request(&[1], Action::Double) }).unwrap();
    assert!(answer.changed);
    assert_eq!(answer.work.rows, 400_000, "the section is read for the outline and read again for the formulas of the one straight edge named");
    assert!(answer.work.steps <= 200_000 + 1);
}

#[test]
fn a_circle_doubled_through_the_command_costs_its_reading_and_nothing_per_edge_of_other_shapes() {
    let mut s = session_with(page_of(vec![circle(1), rectangle(2, 2.0, 1.0)]));
    let answer = act(&mut s, 1, "double", json!({}));
    assert_eq!(answer["shapes"][0]["points_after"], 2);
    assert_eq!(places(&s, 2).len(), 0, "the other shape was not touched");
}

// ---- the numbers the report quotes, and the timed runs, beside a yardstick ---------------------------------------------------------------------

#[test]
#[ignore = "prints the measured costs for the report"]
fn measured_costs() {
    let (d, id, other, charged) = ellipse_of_256();
    println!(
        "Ellipse of 256 points: a resize to 3 inches spends {:?} (calc steps, cells worked), charged {charged} (cap {MAX_FOLLOW_COST})",
        commit_stats(&d, id, "Width", 3.0)
    );
    println!("a move of another shape: {:?} (steps, cells worked); of the ellipse: {:?}", move_stats(&d, other), move_stats(&d, id));
    let steps_cap = u32::try_from(MAX_FOLLOW_STEPS).unwrap();
    let mut t = page_of(vec![long_spline(1, steps_cap, 0)]);
    for _ in 0..7 {
        let (points, cost) = double_small(&mut t, 1);
        println!("spline of {steps_cap} steps: {points} points, charged {cost}");
    }
    println!("a resize of it spends {} calc steps", resize_steps(&t, 1, f64::from(steps_cap) * 1.5));
}

/// Times `f` and a yardstick (a doubling of the Rectangle 200 times), and says both. Run with `cargo test -- --ignored --nocapture`.
fn timed(what: &str, f: impl FnOnce()) {
    let started = std::time::Instant::now();
    for _ in 0..200 {
        let mut s = blank_session();
        let id = drop_shape(&mut s, "rectangle", 2.0, 1.0);
        act(&mut s, id, "double", json!({}));
    }
    let yardstick = started.elapsed();
    let started = std::time::Instant::now();
    f();
    println!("{what}: {:?} (200 doublings of the Rectangle: {yardstick:?})", started.elapsed());
}

#[test]
#[ignore = "timed, beside a yardstick"]
fn timed_a_shape_of_200000_rows_and_4096_connectors_on_a_removed_point() {
    let mut d = page_of(vec![zigzag(1, 199_999)]);
    timed("200,000 rows, an edge doubled", || {
        change_points(&mut d, 1, &Request { edge: Some(10), ..request(&[1], Action::Double) }).unwrap();
    });
    let built = crowd_of_connectors(u32::try_from(MAX_REGLUED_ENDS).unwrap());
    timed("4,096 connectors on a removed point", || {
        let mut d = built.clone();
        change_points(&mut d, 1, &Request { edge: Some(2), ..request(&[1], Action::Halve) }).unwrap();
    });
}

#[test]
#[ignore = "timed, beside a yardstick"]
fn timed_a_page_of_12000_shapes_each_naming_the_halved_shape() {
    let mut shapes = vec![point(point(rectangle(5, 2.0, 1.0), 0, 1.0, 1.0), 1, 1.5, 1.0)];
    for i in 0..12_000u32 {
        shapes.push(ShapeBuilder::new(100 + i).formula("Width", "Sheet.5!Connections.X1", "1"));
    }
    let mut d = DocBuilder::new().page(1, shapes.into_iter().fold(PageBuilder::new(), PageBuilder::shape)).build();
    {
        let page = std::sync::Arc::make_mut(d.pages.get_mut(0).unwrap());
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
        let marker = diacraft_doc::CellAddr::in_row("User", None, RowKey::name("DiaCraftAddedPoints"), "Value");
        sheet.set_cell(&marker, Cell::with_formula("0*(1*Connections.X2)", Some(CellValue::from_text("0")))).unwrap();
    }
    timed("12,000 shapes naming Sheet.5!Connections.X1", || {
        change_points(&mut d, 1, &Request { edge: Some(2), ..request(&[5], Action::Halve) }).unwrap();
    });
}
