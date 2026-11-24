//! What the connector commands of cost (AutoConnect, a style, a hand edit, Reset, line jumps), on the page of `edit_cost.rs` (500 rectangles of the
//! built-in stencil and 300 connectors of ours), and what a crafted file can make them cost. Counted, not timed (`diacraft_calc::work`): the
//! commits each command makes and the steps they take, against the steps of the recalculation of the whole page, which is what each would cost if it
//! did not work out only what it changed. The pairs of legs the jump pass tests are counted by the unit tests of `draw/jumps.rs`. The one bound on
//! time is a hang guard of a minute, no measure of cost.

use std::sync::Arc;
use std::time::{Duration, Instant};

use diacraft_doc::{Cell, CellName, Cells, Connect, Connects, Row, Rows, Section, Shape, Value};
use diacraft_engine::Session;
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
use serde_json::json;

use crate::connect_style::cell;
use crate::edit::{drop_shape, geometry, id_of};
use crate::edit_cost::{COLUMNS, CONNECTORS, big_page, steps_of_the_whole_page};
use crate::edit_foreign::Dir;
use crate::{blank_session, check_session, run};

/// Runs `f`, which is one command, and counts the recalculation it made: the commits and their steps. A hang guard of a minute is the one time.
fn counted<T>(what: &str, f: impl FnOnce() -> T) -> (T, u64, u64) {
    diacraft_calc::work::reset();
    let started = Instant::now();
    let out = f();
    let took = started.elapsed();
    let (commits, steps) = (diacraft_calc::work::commits(), diacraft_calc::work::steps());
    eprintln!("{what}: {commits} commits of {steps} steps");
    assert!(took < Duration::from_secs(60), "{what} took {took:?}: a hang");
    (out, commits, steps)
}

#[test]
fn the_connector_commands_on_a_page_of_500_shapes_and_300_connectors_work_out_what_they_changed_and_not_the_page() {
    let mut s = big_page();
    let whole = steps_of_the_whole_page(&s);
    let middle = 5 * COLUMNS + 4 + 1;
    let connectors: Vec<u32> = (501..501 + 100).collect();
    let check = |what: &str, most_commits: u64, commits: u64, steps: u64| {
        assert!(commits <= most_commits, "{what} made {commits} commits");
        assert!(steps < whole, "{what} took {steps} steps, the whole page {whole}");
    };
    // AutoConnect from a shape with a connector on each side: a drop, a move and a connector, each worked out for what it changed.
    let (r, commits, steps) =
        counted("autoconnect (a copy, the place taken)", || run(&mut s, "connector.autoconnect", json!({"from": middle, "direction": "up"})));
    assert!(r["id"].as_u64().is_some());
    check("autoconnect of a copy", 3, commits, steps);
    let (_, commits, steps) =
        counted("autoconnect (a master)", || run(&mut s, "connector.autoconnect", json!({"from": middle, "direction": "down", "master": "hexagon"})));
    check("autoconnect of a master", 3, commits, steps);
    // A style on 100 connectors, Reset on the same, and the hand edit of one.
    let (_, commits, steps) =
        counted("connector.style of 100 connectors (curved)", || run(&mut s, "connector.style", json!({"style": "curved", "ids": connectors})));
    check("connector.style of 100", 1, commits, steps);
    let (_, commits, steps) = counted("connector.style of 100 connectors (right angle)", || {
        run(&mut s, "connector.style", json!({"style": "right_angle", "ids": connectors}))
    });
    check("connector.style of 100 back", 1, commits, steps);
    let first = connectors[0];
    run(&mut s, "shape.move", json!({"ids": [middle], "dx": 0.0, "dy": 0.3}));
    let (_, commits, steps) =
        counted("connector.edit_path of one connector", || run(&mut s, "connector.edit_path", json!({"id": first, "leg": 0, "to": 3.0})));
    check("connector.edit_path", 0, commits, steps);
    let (_, commits, steps) = counted("connector.reset of 100 connectors", || run(&mut s, "connector.reset", json!({"ids": connectors})));
    check("connector.reset of 100", 1, commits, steps);
    // The page's jump cells: every one of the 300 connectors is rebuilt, and those that come out as they were are not written.
    let (_, commits, steps) =
        counted("connector.line_jumps (the page's style, 300 connectors rebuilt)", || run(&mut s, "connector.line_jumps", json!({"style": 2})));
    check("connector.line_jumps", 1, commits, steps);
    let (_, commits, steps) = counted("one move with the jump pass", || run(&mut s, "shape.move", json!({"ids": [middle], "dx": 0.05, "dy": -0.1})));
    check("one move", 1, commits, steps);
    // The reroute setting of 100 connectors is a cell each and works nothing out; a move of a shape they are glued to then asks each whether a leg of the
    // route it has crosses a shape it is glued to, which is geometry and no recalculation, so the move is still one commit of the steps it was.
    let (r, commits, steps) = counted("connector.reroute of 100 connectors (on crossover)", || {
        run(&mut s, "connector.reroute", json!({"ids": connectors, "mode": "on_crossover"}))
    });
    assert_eq!(r["changed"], json!(100));
    check("connector.reroute of 100", 0, commits, steps);
    let (_, commits, steps) = counted("a move with 100 connectors set to reroute on crossover", || {
        run(&mut s, "shape.move", json!({"ids": [middle], "dx": -0.05, "dy": 0.1}))
    });
    check("a move with 100 connectors on crossover", 1, commits, steps);
    assert_eq!(check_session(&s), Ok(()));
    assert_eq!(s.document().unwrap().page(1).unwrap().connects.len(), 2 * CONNECTORS + 4);
}

/// A drawing of `count` connectors of ours, each a zigzag of `legs` slanted legs across the same box, so that every pair of them could cross.
fn zigzags(count: u32, legs: u32) -> Vec<u8> {
    let rows: String = (0..=legs)
        .map(|i| {
            let (kind, y) = (if i == 0 { "MoveTo" } else { "LineTo" }, if i % 2 == 0 { 0.0 } else { 1.0 });
            format!(r#"<Row T="{kind}" IX="{}"><Cell N="X" V="{}"/><Cell N="Y" V="{y}"/></Row>"#, i + 1, f64::from(i) * 0.02)
        })
        .collect();
    let width = f64::from(legs) * 0.02;
    let shapes: String = (0..count)
        .map(|n| {
            shape(
                100 + n,
                &format!(
                    r#"<Cell N="BeginX" V="1"/><Cell N="BeginY" V="1"/><Cell N="EndX" V="{ex}"/><Cell N="EndY" V="1"/>
<Cell N="PinX" V="{px}"/><Cell N="PinY" V="1"/><Cell N="Width" V="{width}"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="{lx}"/><Cell N="LocPinY" V="0"/><Cell N="Angle" V="0"/>
<Section N="User"><Row N="DiaCraftRoute"><Cell N="Value" V="1"/></Row></Section>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/>{rows}</Section>"#,
                    ex = 1.0 + width,
                    px = 1.0 + width / 2.0,
                    lx = width / 2.0
                ),
            )
        })
        .collect();
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shapes)).build()
}

fn open_bytes(bytes: &[u8]) -> (Session, Dir) {
    let dir = Dir::new();
    let path = dir.path("zigzags.vsdx");
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    (s, dir)
}

/// The shape with this `ID` as the document holds it.
fn shape_of(d: &diacraft_doc::Document, id: u32) -> std::sync::Arc<diacraft_doc::Shape> {
    d.shape(&d.find_shape(diacraft_doc::Owner::Page(1), id).unwrap()).unwrap().clone()
}

#[test]
fn a_page_of_connectors_that_could_all_cross_each_other_is_given_up_by_the_budget_and_no_connector_is_rewritten() {
    // 300 connectors of 200 legs: 60,000 legs, every pair's boxes overlap, and the pairs of legs are 3.6e9; the budget of a rebuild is 4e6 pairs (the
    // unit test of `draw/jumps.rs` counts them). Nothing is written but the settings, and the recalculation is of the cells the command wrote.
    let (mut s, _dir) = open_bytes(&zigzags(300, 200));
    let before = s.document().unwrap().clone();
    let whole = steps_of_the_whole_page(&s);
    let (_, commits, steps) =
        counted("line_jumps over 300 connectors of 200 legs that all overlap", || run(&mut s, "connector.line_jumps", json!({"code": 4})));
    // The page holds no formula, so nothing is worked out: the whole page is 0 steps and so is the command.
    assert!(commits <= 1 && steps <= whole, "{commits} commits of {steps} steps, the whole page {whole}");
    let after = s.document().unwrap();
    for id in [100, 250, 399] {
        assert_eq!(shape_of(after, id), shape_of(&before, id), "connector {id}");
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_page_with_more_connectors_than_the_cap_or_connectors_of_more_legs_than_a_route_has_no_jumps_made() {
    // 2,100 connectors of 3 legs: past the cap of 2,000 that take part. Nothing is rewritten.
    let (mut s, _dir) = open_bytes(&zigzags(2_100, 3));
    let before = s.document().unwrap().clone();
    let (_, commits, _) = counted("line_jumps over 2,100 connectors", || run(&mut s, "connector.line_jumps", json!({"code": 4})));
    assert!(commits <= 1);
    for id in [100, 1_000, 2_199] {
        assert_eq!(shape_of(s.document().unwrap(), id), shape_of(&before, id), "connector {id}");
    }
    // 20 connectors of 1,000 legs: past the 256 legs of one that takes part. Nothing is rewritten either.
    let (mut t, _dir2) = open_bytes(&zigzags(20, 1_000));
    let before = t.document().unwrap().clone();
    let (_, commits, _) = counted("line_jumps over 20 connectors of 1,000 legs", || run(&mut t, "connector.line_jumps", json!({"code": 4})));
    assert!(commits <= 1);
    for id in [100, 119] {
        assert_eq!(shape_of(t.document().unwrap(), id), shape_of(&before, id), "connector {id}");
    }
    assert_eq!(check_session(&s), Ok(()));
    assert_eq!(check_session(&t), Ok(()));
}

/// The neighbour a click joins is read off the page and never recalculated, and joining a shape that is there (a click's neighbour, a drag's target) is one
/// connector, worked out for what it changed and not for the page. On the page of 500 shapes and 300 connectors.
#[test]
fn finding_and_joining_a_neighbour_on_a_page_of_500_shapes_and_300_connectors_recalculates_nothing_but_the_connector() {
    let mut s = big_page();
    let whole = steps_of_the_whole_page(&s);
    // The shapes of the page are in rows of COLUMNS: the shape after `middle` is its neighbour on the right, unless a connector joins them.
    let middle = 5 * COLUMNS + 4 + 1;
    let (r, commits, steps) = counted("connector.neighbour", || run(&mut s, "connector.neighbour", json!({"from": middle, "direction": "right"})));
    assert_eq!((commits, steps), (0, 0), "a query recalculates nothing: {r}");
    // A shape far from the middle, with nothing above it.
    let (_, commits, steps) =
        counted("connector.neighbour (nothing there)", || run(&mut s, "connector.neighbour", json!({"from": 1, "direction": "up"})));
    assert_eq!((commits, steps), (0, 0));
    // A shape on the same row but a few places along, joined by name (a drag).
    let (r, commits, steps) = counted("connector.autoconnect (to)", || {
        run(&mut s, "connector.autoconnect", json!({"from": middle, "direction": "right", "to": middle + 2}))
    });
    assert_eq!(r["created"], false);
    assert!(commits <= 2 && steps < whole, "joining took {commits} commits of {steps} steps, the whole page {whole}");
    assert_eq!(check_session(&s), Ok(()));
}

// ----: the crossing test of a fixed route, on pages a crafted file can make ----

/// How many connectors the crowded pages hold.
const CROWD: u32 = 10_000;

/// A page of `n` connectors set to reroute `mode`, copies of one that `connector.add` made from the right side of box A (at 3, 4) to the left side of box B (at 9, 4)
/// and glued as it is, whose route is `rows` rows: a zigzag a twentieth of an inch high from end to end between the boxes, which enters neither, so that no connector
/// is routed again and a move of A costs what the test of each route costs (the security scan's findings). With `rows` 6 the route is
/// a hand path instead, a jog up an inch and along, which is what a person makes. Every copy holds the one section of rows (the model shares it, as it shares any copy),
/// and A and B come after the connectors in the page's order, so that a search of the page for either walks every connector first. → the session, A's `ID`, the
/// connectors' `ID`s.
fn crowded(n: u32, rows: u32, mode: &str) -> (Session, u32, Vec<u32>) {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 4.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    run(&mut s, "connector.reroute", json!({"ids": [c], "mode": mode}));
    let width = cell(s.document().unwrap(), c, "Width").unwrap();
    let points: Vec<(f64, f64)> = if rows == 6 {
        vec![(0.0, 0.0), (0.5, 0.0), (0.5, 1.0), (width - 0.5, 1.0), (width - 0.5, 0.0), (width, 0.0)]
    } else {
        let last = rows - 1;
        (0..rows).map(|k| (width * f64::from(k) / f64::from(last), if k % 2 == 1 && k != last { 0.05 } else { 0.0 })).collect()
    };
    let route: Rows = points
        .iter()
        .enumerate()
        .map(|(k, (x, y))| {
            let mut cells = Cells::new();
            cells.insert(CellName::new("X"), Cell::constant(Value::from_text(&x.to_string())));
            cells.insert(CellName::new("Y"), Cell::constant(Value::from_text(&y.to_string())));
            let kind = if k == 0 { "MoveTo" } else { "LineTo" };
            Arc::new(Row { ix: Some(u32::try_from(k).unwrap() + 1), row_type: Some(Arc::from(kind)), cells, ..Row::default() })
        })
        .collect();
    let ids: Vec<u32> = (c..c + n).collect();
    s.edit("a crowded page", |doc| {
        let page = doc.pages.get_mut(0).map(Arc::make_mut).unwrap();
        let mut original = page.shapes.iter().find(|x| x.id == c).map(|x| Shape::clone(x)).unwrap();
        let sheet = Arc::make_mut(&mut original.sheet);
        let at = sheet.sections.iter().position(|x| x.name.as_str() == "Geometry" && x.ix == Some(0)).unwrap();
        let mut section = Section::clone(&sheet.sections[at]);
        section.rows = route;
        sheet.sections[at] = Arc::new(section);
        let glue: Vec<Connect> = page.connects.iter().filter(|x| x.from_sheet == c).cloned().collect();
        let mut connects: Vec<Connect> = page.connects.iter().filter(|x| x.from_sheet != c).cloned().collect();
        let mut shapes: Vec<Arc<Shape>> = Vec::new();
        for id in &ids {
            shapes.push(Arc::new(Shape { id: *id, ..original.clone() }));
            connects.extend(glue.iter().map(|x| Connect { from_sheet: *id, ..x.clone() }));
        }
        shapes.extend(page.shapes.iter().filter(|x| x.id != c).cloned());
        page.shapes = shapes.into_iter().collect();
        page.connects = Connects::from_vec(connects);
        Ok(())
    })
    .unwrap();
    (s, a, ids)
}

/// What a move of A up half an inch costs on a crowded page: the rows the model merged to read sections and the shapes the searches of the page visited
/// (`diacraft_doc::work`), and the time, with the hang guard of a minute.
fn move_cost(s: &mut Session, a: u32, what: &str) -> (u64, u64, Duration) {
    diacraft_doc::work::reset();
    let started = Instant::now();
    run(s, "shape.move", json!({"ids": [a], "dx": 0.0, "dy": 0.5}));
    let took = started.elapsed();
    let (rows, finds) = (diacraft_doc::work::rows_merged(), diacraft_doc::work::finds_visited());
    eprintln!("{what}: {rows} rows merged, {finds} shapes visited by searches, {took:?}");
    assert!(took < Duration::from_secs(60), "{what} took {took:?}: a hang");
    (rows, finds, took)
}

/// The security scan's three findings: a move of a shape that ten thousand connectors set to reroute on crossover are glued to asks each whether a leg of
/// its route enters a box, and the test reads **at most a hand path's scale of rows** of the route (16 times [`MAX_HAND_POINTS`], line jumps and all;
/// a route of more is held not to cross and is not read at all), **searches the page for no shape** (the connectors and the shapes they are glued to are found with the
/// rest of the move, and the glue of each end in one pass over the page's `Connect` rows), and reads the connection points of a shape an end is glued to once, within
/// [`MAX_CONNECTION_ROWS`]. Against the floor: the same page with the connectors set never to reroute, which nothing tests.
#[test]
fn a_move_of_the_shape_ten_thousand_connectors_on_crossover_are_glued_to_reads_each_route_within_a_hand_paths_rows_and_never_searches_the_page() {
    use diacraft_engine::draw::MAX_HAND_POINTS;
    use diacraft_engine::snap::MAX_CONNECTION_ROWS;
    let cap = u64::try_from(16 * MAX_HAND_POINTS).unwrap();
    for rows in [4_000_u32, 6] {
        let (mut s, a, _) = crowded(CROWD, rows, "never");
        let (floor_rows, floor_finds, _) = move_cost(&mut s, a, &format!("{CROWD} connectors of {rows} rows, never"));
        drop(s);
        let (mut s, a, ids) = crowded(CROWD, rows, "on_crossover");
        let (checked_rows, checked_finds, _) = move_cost(&mut s, a, &format!("{CROWD} connectors of {rows} rows, on crossover"));
        assert!(
            checked_finds <= floor_finds,
            "{rows} rows: the tests searched the page ({checked_finds} shapes visited, {floor_finds} without them)"
        );
        let read = checked_rows.saturating_sub(floor_rows);
        let route_rows = if u64::from(rows) > cap { 0 } else { u64::from(CROWD) * u64::from(rows) };
        let most = route_rows + 2 * u64::try_from(MAX_CONNECTION_ROWS).unwrap();
        assert!(read <= most, "{rows} rows: the tests read {read} rows ({checked_rows} in all, {floor_rows} without them), at most {most}");
        // No route crosses a box, so none is routed again: each keeps its rows.
        for id in [ids[0], ids[ids.len() / 2], ids[ids.len() - 1]] {
            assert_eq!(geometry(s.document().unwrap(), id).len(), rows as usize, "{rows} rows: connector {id} keeps its route");
        }
        assert_eq!(check_session(&s), Ok(()));
    }
}

/// Timed: the same move against **a yardstick of plain connectors** (the same page with the connectors set to reroute freely, each routed again by the
/// move: what routing them costs), and the floor (set never to reroute: nothing tested, nothing routed). The test of a fixed route costs no more than routing it would.
/// A time moves with the load on the machine, so this is run by hand: `cargo test -p diacraft-engine --test it -- --ignored --nocapture connector_tests_timed`.
#[test]
#[ignore = "a timing, run by hand"]
fn connector_tests_timed_against_plain_connectors() {
    for rows in [4_000_u32, 6] {
        let mut times = Vec::new();
        for mode in ["freely", "never", "on_crossover"] {
            let (mut s, a, _) = crowded(CROWD, rows, mode);
            let (_, _, took) = move_cost(&mut s, a, &format!("{CROWD} connectors of {rows} rows, {mode}"));
            times.push(took);
        }
        let (plain, floor, checked) = (times[0], times[1], times[2]);
        eprintln!("RESULT {rows} rows: plain (routed) {plain:?}, floor (never) {floor:?}, on crossover {checked:?}");
        assert!(checked <= plain, "{rows} rows: the tests took {checked:?}, routing the same connectors {plain:?}");
    }
}
