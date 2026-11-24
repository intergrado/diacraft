//! What routing connectors again costs, counted on the pages a crafted file can make (after the findings measured): ten thousand
//! connectors glued to one shape that a move routes again, a shape of ten thousand connection points the ends of ten thousand connectors are placed on, ten
//! thousand connectors whose routes the jump pass reads when one connector is routed, the floor of a move that routes none, and the most a plain move routes.
//! Each is held to a **yardstick** of plain content of the same size (the same page with routes of a hand path's six rows, shapes of four connection points,
//! connectors that are not routed), by counts that do not move with the load of the machine: the shapes searches of the page visited, the `Connect` rows the
//! routing passed over, the rows the model merged, the cells the recalculation's walks passed over and the rows the pricing of the history looked at.
//! The times of the same pages are the ignored tests of `tests/it/connect_cost.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Cells, Connect, Connects, Document, Owner, Row, Rows, Section, Shape, SheetRef, Value};
use serde_json::{Value as Json, json};

use super::read::Reader;
use crate::Session;

/// How many connectors the crowded pages hold.
const CROWD: u32 = 10_000;

fn run(s: &mut Session, id: &str, params: Json) -> Json {
    s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
}

fn id_of(v: &Json) -> u32 {
    u32::try_from(v["id"].as_u64().unwrap()).unwrap()
}

fn blank() -> Session {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    s
}

fn drop_rectangle(s: &mut Session, x: f64, y: f64) -> u32 {
    id_of(&run(s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": x, "y": y})))
}

/// A cell of shape `id` of page 1, by inheritance.
fn number(doc: &Document, id: u32, name: &str) -> f64 {
    let at = doc.find_shape(Owner::Page(1), id).unwrap();
    Reader::new(doc).number(&SheetRef::Shape(at), name)
}

/// The rows of the first `Geometry` section a shape holds itself.
fn own_route_rows(doc: &Document, id: u32) -> usize {
    let at = doc.find_shape(Owner::Page(1), id).unwrap();
    let shape = doc.shape(&at).unwrap();
    shape.sheet.sections.iter().find(|s| s.name.as_str() == "Geometry" && s.ix == Some(0)).map_or(0, |s| s.rows.len())
}

/// The route the copies of the crowded pages hold.
#[derive(Clone, Copy, Debug)]
enum Route {
    /// A hand path of six rows: a jog up an inch and along, which is what a person makes (the yardstick).
    Hand,
    /// A zigzag a twentieth of an inch high from end to end, of this many rows: more points than a connector that takes part in the jumps has.
    Zigzag(u32),
    /// A straight line of this many rows, every point on it: a route of two points once they are dropped, which takes part in the jumps.
    Line(u32),
}

impl Route {
    fn rows(self) -> u32 {
        match self {
            Route::Hand => 6,
            Route::Zigzag(n) | Route::Line(n) => n,
        }
    }

    fn points(self, width: f64) -> Vec<(f64, f64)> {
        match self {
            Route::Hand => vec![(0.0, 0.0), (0.5, 0.0), (0.5, 1.0), (width - 0.5, 1.0), (width - 0.5, 0.0), (width, 0.0)],
            Route::Zigzag(rows) => {
                let last = rows - 1;
                (0..rows).map(|k| (width * f64::from(k) / f64::from(last), if k % 2 == 1 && k != last { 0.05 } else { 0.0 })).collect()
            }
            Route::Line(rows) => {
                let last = rows - 1;
                (0..rows).map(|k| (width * f64::from(k) / f64::from(last), 0.0)).collect()
            }
        }
    }
}

fn route_rows(points: &[(f64, f64)]) -> Rows {
    points
        .iter()
        .enumerate()
        .map(|(k, (x, y))| {
            let mut cells = Cells::new();
            cells.insert(CellName::new("X"), Cell::constant(Value::from_text(&x.to_string())));
            cells.insert(CellName::new("Y"), Cell::constant(Value::from_text(&y.to_string())));
            let kind = if k == 0 { "MoveTo" } else { "LineTo" };
            Arc::new(Row { ix: Some(u32::try_from(k).unwrap() + 1), row_type: Some(Arc::from(kind)), cells, ..Row::default() })
        })
        .collect()
}

/// A crowded page, as the `tests/it/connect_cost.rs` makes it: `n` connectors set to reroute `mode`, copies of one that `connector.add` made from the right
/// side of box A (at 3, 4) to the left side of box B (at 9, 4) and glued as it is, whose route is `route`, which enters neither box. Every copy holds the one section of
/// rows (the model shares it, as it shares any copy), and A and B come after the connectors in the page's order, so that a search of the page for either walks every
/// connector first. → the session, A's and B's `ID`s, the connectors' `ID`s.
fn crowded(n: u32, route: Route, mode: &str) -> (Session, u32, u32, Vec<u32>) {
    let mut s = blank();
    let a = drop_rectangle(&mut s, 3.0, 4.0);
    let b = drop_rectangle(&mut s, 9.0, 4.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    run(&mut s, "connector.reroute", json!({"ids": [c], "mode": mode}));
    let width = number(s.document().unwrap(), c, "Width");
    let rows = route_rows(&route.points(width));
    let ids: Vec<u32> = (c..c + n).collect();
    s.edit("a crowded page", |doc| {
        let page = doc.pages.get_mut(0).map(Arc::make_mut).unwrap();
        let mut original = page.shapes.iter().find(|x| x.id == c).map(|x| Shape::clone(x)).unwrap();
        let sheet = Arc::make_mut(&mut original.sheet);
        let at = sheet.sections.iter().position(|x| x.name.as_str() == "Geometry" && x.ix == Some(0)).unwrap();
        let mut section = Section::clone(&sheet.sections[at]);
        section.rows = rows;
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
    (s, a, b, ids)
}

/// What one command cost, counted.
#[derive(Debug)]
struct Cost {
    /// Shapes the searches of the page visited (`Document::find_shape`).
    finds: u64,
    /// Rows the model merged to read sections (routes, connection points).
    rows: u64,
    /// `Connect` rows the routing passed over to find the glue of the ends.
    connects: usize,
    /// Cells the recalculation's walks of sheets passed over.
    cells: u64,
    /// Rows the pricing of the edit's undo step looked at.
    priced: u64,
    /// Connectors tried, and routed.
    attempted: usize,
    routed: usize,
    answer: Json,
}

fn reset() {
    diacraft_doc::work::reset();
    diacraft_calc::work::reset();
    super::reroute::CONNECT_ROWS_READ.with(|c| c.set(0));
    crate::odgfile::hold::ROWS_LOOKED_AT.with(|c| c.set(0));
    super::settle::ATTEMPTED.with(|c| c.set(0));
    super::settle::ROUTED.with(|c| c.set(0));
}

fn counted(s: &mut Session, what: &str, id: &str, params: Json) -> Cost {
    reset();
    let started = std::time::Instant::now();
    let answer = run(s, id, params);
    let cost = Cost {
        finds: diacraft_doc::work::finds_visited(),
        rows: diacraft_doc::work::rows_merged(),
        connects: super::reroute::CONNECT_ROWS_READ.with(std::cell::Cell::get),
        cells: diacraft_calc::work::cells_walked(),
        priced: crate::odgfile::hold::ROWS_LOOKED_AT.with(std::cell::Cell::get),
        attempted: super::settle::ATTEMPTED.with(std::cell::Cell::get),
        routed: super::settle::ROUTED.with(std::cell::Cell::get),
        answer,
    };
    eprintln!(
        "{what}: {} finds, {} rows merged, {} connects passed, {} cells walked, {} rows priced, {} tried, {} routed, {:?}",
        cost.finds,
        cost.rows,
        cost.connects,
        cost.cells,
        cost.priced,
        cost.attempted,
        cost.routed,
        started.elapsed()
    );
    cost
}

/// A move of shape `id` up half an inch.
fn moved(s: &mut Session, id: u32, what: &str) -> Cost {
    counted(s, what, "shape.move", json!({"ids": [id], "dx": 0.0, "dy": 0.5}))
}

/// The connectors the answer of a command says were not routed again: the ones listed, and the count of the others.
fn said_left(answer: &Json) -> (Vec<Json>, u64) {
    let listed: Vec<Json> =
        answer["groups_left"].as_array().cloned().unwrap_or_default().into_iter().filter(|l| l["kind"] == json!("connector")).collect();
    (listed, answer["groups_left_unlisted"].as_u64().unwrap_or(0))
}

/// Finding 1: a move of the shape ten thousand plain connectors are glued to routes each again, and the routing **searches the page for no shape** (the connectors and
/// the shapes their ends are glued to are found with the rest of the edit), **passes over the page's `Connect` rows once** for the glue of every end (not twice for
/// each connector), and reads the connection points of a glued shape once for the move, not once for each end. Against the yardstick of the same move with the
/// connectors set never to reroute, which routes none.
#[test]
fn routing_ten_thousand_connectors_again_searches_the_page_for_none_and_reads_the_glue_once() {
    for route in [Route::Hand, Route::Zigzag(4_000)] {
        let (mut s, a, _, _) = crowded(CROWD, route, "never");
        let floor = moved(&mut s, a, &format!("{CROWD} connectors of {} rows, never", route.rows()));
        drop(s);
        let (mut s, a, _, ids) = crowded(CROWD, route, "freely");
        let connects = s.document().unwrap().page(1).unwrap().connects.len();
        let plain = moved(&mut s, a, &format!("{CROWD} connectors of {} rows, freely", route.rows()));
        assert_eq!((plain.attempted, plain.routed), (CROWD as usize, CROWD as usize), "{route:?}: every connector is routed again");
        assert!(plain.finds <= floor.finds, "{route:?}: the routing searched the page: {} shapes visited, {} without it", plain.finds, floor.finds);
        assert!(plain.connects <= connects, "{route:?}: the routing passed over {} `Connect` rows, the page holds {connects}", plain.connects);
        // The connection points of A and B, four each, read once; and the jump pass, which reads the new routes of at most one more connector than take part.
        let jumps = (super::MAX_JUMP_PARTICIPANTS as u64 + 1) * 8;
        assert!(
            plain.rows <= floor.rows + 2 * 4 + jumps,
            "{route:?}: the routing merged {} rows ({} without it): the points of a glued shape are read once for the move",
            plain.rows,
            floor.rows
        );
        // Each is routed: the route it had is replaced by a route of its own ends.
        for id in [ids[0], ids[ids.len() / 2], ids[ids.len() - 1]] {
            assert!(own_route_rows(s.document().unwrap(), id) < 16, "{route:?}: connector {id} has a new route");
        }
    }
}

/// A page of `n` connectors of a file whose ends the engine places (the end cells hold the places as values, with no formula calc could work out: `glue.rs`), copies of
/// one `connector.add` made from point 3 of box T (at 3, 4) to point 1 of box B (at 9, 4): the begin glued to that point of T, or to the whole of T with `whole`, the end to
/// B's point. T has `points` connection points of its own (four as a rectangle's are, or as many as asked, the first four where the rectangle's are and the others along
/// its bottom edge), and T and B come after the connectors in the page's order. → the session, T's `ID`, the connectors' `ID`s.
fn placed(n: u32, points: u32, whole: bool) -> (Session, u32, Vec<u32>) {
    let mut s = blank();
    let t = drop_rectangle(&mut s, 3.0, 4.0);
    let b = drop_rectangle(&mut s, 9.0, 4.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": t, "point": 3}, "to": {"id": b, "point": 1}})));
    let ends: Vec<(&str, f64)> = ["BeginX", "BeginY", "EndX", "EndY"].iter().map(|name| (*name, number(s.document().unwrap(), c, name))).collect();
    let corners = super::shape_corners(s.document().unwrap(), 1, t).unwrap();
    let (w, h) = (corners[1].0 - corners[0].0, corners[2].1 - corners[1].1);
    let ids: Vec<u32> = (c..c + n).collect();
    s.edit("a page of placed ends", |doc| {
        let page = doc.pages.get_mut(0).map(Arc::make_mut).unwrap();
        let mut original = page.shapes.iter().find(|x| x.id == c).map(|x| Shape::clone(x)).unwrap();
        let sheet = Arc::make_mut(&mut original.sheet);
        // A connector of a file: no marker of ours, and its ends are values.
        sheet.sections.retain(|x| x.name.as_str() != "User");
        for (name, value) in &ends {
            sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(&value.to_string())).with_unit("IN"));
        }
        let mut glue: Vec<Connect> = page.connects.iter().filter(|x| x.from_sheet == c).cloned().collect();
        if whole {
            for row in glue.iter_mut().filter(|x| x.to_sheet == t) {
                row.to_part = Some(super::glue::WHOLE_SHAPE);
                row.to_cell = None;
            }
        }
        let mut connects: Vec<Connect> = page.connects.iter().filter(|x| x.from_sheet != c).cloned().collect();
        let mut shapes: Vec<Arc<Shape>> = Vec::new();
        for id in &ids {
            shapes.push(Arc::new(Shape { id: *id, ..original.clone() }));
            connects.extend(glue.iter().map(|x| Connect { from_sheet: *id, ..x.clone() }));
        }
        for shape in page.shapes.iter().filter(|x| x.id != c) {
            if shape.id == t && points > 4 {
                let mut target = Shape::clone(shape);
                let mut section = Section::new("Connection", None);
                let at = |k: u32| -> (f64, f64, f64, f64) {
                    match k {
                        0 => (0.0, h / 2.0, -1.0, 0.0),
                        1 => (w / 2.0, h, 0.0, 1.0),
                        2 => (w, h / 2.0, 1.0, 0.0),
                        3 => (w / 2.0, 0.0, 0.0, -1.0),
                        _ => (w * f64::from(k) / f64::from(points), 0.0, 0.0, -1.0),
                    }
                };
                for k in 0..points {
                    let (x, y, dx, dy) = at(k);
                    let mut cells = Cells::new();
                    for (name, v) in [("X", x), ("Y", y), ("DirX", dx), ("DirY", dy)] {
                        cells.insert(CellName::new(name), Cell::constant(Value::from_text(&v.to_string())));
                    }
                    section.rows.push(Arc::new(Row { ix: Some(k), cells, ..Row::default() }));
                }
                let sheet = Arc::make_mut(&mut target.sheet);
                sheet.sections.retain(|x| x.name.as_str() != "Connection");
                sheet.sections.push(Arc::new(section));
                shapes.push(Arc::new(target));
            } else {
                shapes.push(Arc::clone(shape));
            }
        }
        page.shapes = shapes.into_iter().collect();
        page.connects = Connects::from_vec(connects);
        Ok(())
    })
    .unwrap();
    (s, t, ids)
}

/// Finding 2: the ends of ten thousand connectors of a file glued to a shape of ten thousand connection points are placed by the engine when the shape moves
/// And routed again: the points of the shape are read **once for the move**, not once for each end, whether an end is glued to one of its points
/// or to the whole of it. Against the yardstick of the same page with a shape of four points.
#[test]
fn placing_and_routing_ends_on_a_shape_of_ten_thousand_points_reads_its_points_once() {
    for whole in [false, true] {
        let (mut s, t, _) = placed(CROWD, 4, whole);
        let yardstick = moved(&mut s, t, &format!("{CROWD} ends on a shape of 4 points (whole {whole})"));
        drop(s);
        let (mut s, t, ids) = placed(CROWD, 10_000, whole);
        let crowded = moved(&mut s, t, &format!("{CROWD} ends on a shape of 10,000 points (whole {whole})"));
        assert!(
            crowded.finds <= yardstick.finds,
            "whole {whole}: {} shapes visited by searches, {} with four points",
            crowded.finds,
            yardstick.finds
        );
        // The placement and the routing read the shape's points once each.
        assert!(
            crowded.rows <= yardstick.rows + 2 * 10_000,
            "whole {whole}: {} rows merged, {} with four points: the points are read once for the move",
            crowded.rows,
            yardstick.rows
        );
        assert_eq!(crowded.attempted, CROWD as usize, "whole {whole}: every connector is routed again");
        // The ends followed the shape: at its point 3 (half way along the bottom edge), or at the place of it nearest the other end.
        let doc = s.document().unwrap();
        let corners = super::shape_corners(doc, 1, t).unwrap();
        for id in [ids[0], ids[ids.len() - 1]] {
            let begin = (number(doc, id, "BeginX"), number(doc, id, "BeginY"));
            if whole {
                assert!(begin.1 >= corners[0].1 - 1e-6 && begin.1 <= corners[2].1 + 1e-6, "connector {id}: {begin:?} is on the moved shape");
            } else {
                let want = ((corners[0].0 + corners[1].0) / 2.0, corners[0].1);
                assert!((begin.0 - want.0).abs() < 1e-6 && (begin.1 - want.1).abs() < 1e-6, "connector {id}: {begin:?}, point 3 is at {want:?}");
            }
        }
    }
}

/// Finding 3: one connector routed again on a page of ten thousand others that take part in the jumps (each a route of four thousand rows on one line, two points once
/// the points on the line are dropped) or that do not (a zigzag of four thousand points) makes the jump pass read the routes of the page **within a budget of rows**, not
/// every route whole. Against the yardstick of the same page with routes of a hand path's six rows.
#[test]
fn the_jumps_of_one_connector_routed_read_the_routes_of_the_page_within_a_budget() {
    let page = |route: Route| {
        let (mut s, a, b, _) = crowded(CROWD, route, "never");
        run(&mut s, "connector.add", json!({"from": {"id": a, "point": 1}, "to": {"id": b, "point": 1}}));
        (s, a)
    };
    let (mut s, a) = page(Route::Hand);
    let yardstick = moved(&mut s, a, "one connector routed beside 10,000 of 6 rows");
    drop(s);
    for route in [Route::Line(4_000), Route::Zigzag(4_000)] {
        let (mut s, a) = page(route);
        let crowded = moved(&mut s, a, &format!("one connector routed beside {CROWD} of {route:?}"));
        assert_eq!(crowded.routed, 1, "{route:?}");
        let most = super::MAX_JUMP_ROWS as u64;
        assert!(
            crowded.rows <= yardstick.rows + most,
            "{route:?}: the jump pass merged {} rows ({} with routes of six rows), more than its budget",
            crowded.rows,
            yardstick.rows
        );
    }
}

/// Finding 4: the floor of a move that routes none of the ten thousand connectors glued to the moved shape (set never to reroute) does not grow with the rows of the
/// route they share: the recalculation's walks of the connectors' sheets, and the pricing of the undo step, look at a section the copies share once, not once for each
/// copy. Against the yardstick of the same page with routes of six rows.
#[test]
fn the_floor_of_a_move_does_not_grow_with_the_rows_of_a_route_its_connectors_share() {
    let (mut s, a, _, _) = crowded(CROWD, Route::Hand, "never");
    let yardstick = moved(&mut s, a, "never, 6 rows");
    drop(s);
    let (mut s, a, _, _) = crowded(CROWD, Route::Zigzag(4_000), "never");
    let crowded = moved(&mut s, a, "never, 4,000 rows");
    // The shared section's 4,000 rows of two cells are walked once by the making of the page's index and once for the cells that read the ends (the comparison of the
    // versions of a sheet passes it by: it is the same section in both); the pricing looks at its rows once, for the formulas the next versions hold.
    assert!(
        crowded.cells <= yardstick.cells + 2 * 4_000 * 2,
        "{} cells walked, {} with routes of six rows: the shared section was walked for each copy",
        crowded.cells,
        yardstick.cells
    );
    assert!(
        crowded.priced <= yardstick.priced + 4_000,
        "{} rows priced, {} with routes of six rows: the shared section was priced for each copy",
        crowded.priced,
        yardstick.priced
    );
    assert_eq!(crowded.attempted, 0);
}

/// Finding 5: a plain move has a budget of tries for the edit, as a refit has: of the connectors past it the ends follow their shapes, the routes stay as they were,
/// and **the answer says them**, in words.
#[test]
fn a_plain_move_routes_at_most_its_budget_and_says_the_connectors_it_left() {
    let budget = super::MAX_ROUTED_PER_EDIT;
    let extra = 100_u32;
    let (mut s, a, _, ids) = crowded(u32::try_from(budget).unwrap() + extra, Route::Hand, "freely");
    let cost = moved(&mut s, a, "a move of the shape 10,100 connectors are glued to");
    assert_eq!(cost.attempted, budget, "the tries of the edit");
    let (listed, unlisted) = said_left(&cost.answer);
    assert_eq!(listed.len() as u64 + unlisted, u64::from(extra), "every connector left is said: {}", cost.answer);
    for entry in &listed {
        assert!(entry["why"].as_str().unwrap().contains("not routed again"), "{entry}");
    }
    // The ones left are the last in the order of their `ID`s: they keep the route they had, and their ends followed A.
    let doc = s.document().unwrap();
    let last = *ids.last().unwrap();
    assert_eq!(own_route_rows(doc, last), 6, "connector {last} keeps its route");
    assert!(own_route_rows(doc, ids[0]) < 6, "connector {} was routed again", ids[0]);
    assert!(listed.iter().all(|l| l["id"].as_u64().is_some_and(|id| id > u64::from(last - extra))), "{listed:?}");
}

/// Finding 5: the tries are **one budget for the edit**, however many times it routes: two moves in one edit (as a command that settles twice makes) share them, the
/// second has what the first left, and every connector either left is said once. A budget taken while another is held leaves the other in place.
#[test]
fn the_routings_of_one_edit_share_its_tries_and_say_what_they_left() {
    let mut s = blank();
    let a = drop_rectangle(&mut s, 3.0, 4.0);
    let b = drop_rectangle(&mut s, 9.0, 4.0);
    for _ in 0..8 {
        run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}}));
    }
    let mut doc = s.document().unwrap().clone();
    super::settle::ATTEMPTED.with(|c| c.set(0));
    let routes = super::settle::EditRoutes::enter_with(5);
    {
        let inner = super::settle::EditRoutes::enter_with(100);
        super::move_shapes(&mut doc, 1, &[a], 0.0, 0.5).unwrap();
        assert_eq!(inner.take_left(), (Vec::new(), 0), "the inner budget is none: the edit's is the one in place");
    }
    super::move_shapes(&mut doc, 1, &[a], 0.0, 0.5).unwrap();
    assert_eq!(super::settle::ATTEMPTED.with(std::cell::Cell::get), 5, "five tries for the edit, all taken by the first move");
    let (left, unlisted) = routes.take_left();
    assert_eq!((left.len(), unlisted), (3 + 8, 0), "three left by the first move and eight by the second: {left:?}");
    assert!(left.iter().all(|l| l.kind == "connector" && l.page == 1 && l.why.contains("not routed again")), "{left:?}");
    assert_eq!(routes.take_left(), (Vec::new(), 0), "each is said once");
    drop(routes);
    // With no edit's budget held (a library call), each call has the most.
    super::settle::ATTEMPTED.with(|c| c.set(0));
    super::move_shapes(&mut doc, 1, &[a], 0.0, 0.5).unwrap();
    assert_eq!(super::settle::ATTEMPTED.with(std::cell::Cell::get), 8);
}

// ---- the same pages, timed ----

/// The time of a move of shape `id` up half an inch.
fn timed_move(s: &mut Session, id: u32, what: &str) -> std::time::Duration {
    let started = std::time::Instant::now();
    run(s, "shape.move", json!({"ids": [id], "dx": 0.0, "dy": 0.5}));
    let took = started.elapsed();
    eprintln!("TIMED {what}: {took:?}");
    took
}

/// `crafted` against `yardstick`, made back to back: within `most` times it (a time moves with the load of the machine, so these are run by hand:
/// `cargo test -p diacraft-engine --lib -- --ignored --nocapture route_cost::timed`).
fn within(what: &str, crafted: std::time::Duration, yardstick: std::time::Duration, most: f64) {
    let ratio = crafted.as_secs_f64() / yardstick.as_secs_f64().max(1.0e-3);
    eprintln!("RESULT {what}: {crafted:?} against {yardstick:?}, {ratio:.2} times");
    assert!(ratio <= most, "{what}: {crafted:?} against a yardstick of {yardstick:?}, {ratio:.2} times, more than {most}");
}

/// Finding 1, timed: ten thousand plain connectors routed again by a move, against the same move with the connectors set never to reroute.
#[test]
#[ignore = "a timing, run by hand"]
fn timed_routing_ten_thousand_connectors_against_the_move_that_routes_none() {
    for route in [Route::Hand, Route::Zigzag(4_000)] {
        let (mut s, a, _, _) = crowded(CROWD, route, "never");
        let yardstick = timed_move(&mut s, a, &format!("{route:?}, never"));
        drop(s);
        let (mut s, a, _, _) = crowded(CROWD, route, "freely");
        let crafted = timed_move(&mut s, a, &format!("{route:?}, freely"));
        within(&format!("{CROWD} connectors of {route:?} routed again"), crafted, yardstick, 3.0);
    }
}

/// Finding 2, timed: ten thousand ends placed on a shape of ten thousand points and routed, against the same page with a shape of four.
#[test]
#[ignore = "a timing, run by hand"]
fn timed_ends_on_a_shape_of_ten_thousand_points_against_one_of_four() {
    for whole in [false, true] {
        let (mut s, t, _) = placed(CROWD, 4, whole);
        let yardstick = timed_move(&mut s, t, &format!("4 points, whole {whole}"));
        drop(s);
        let (mut s, t, _) = placed(CROWD, 10_000, whole);
        let crafted = timed_move(&mut s, t, &format!("10,000 points, whole {whole}"));
        within(&format!("{CROWD} ends on a shape of 10,000 points (whole {whole})"), crafted, yardstick, 2.0);
    }
}

/// Finding 3, timed: one connector routed beside ten thousand routes of 4,000 rows, against ten thousand of six.
#[test]
#[ignore = "a timing, run by hand"]
fn timed_jumps_beside_ten_thousand_long_routes_against_short_ones() {
    let page = |route: Route| {
        let (mut s, a, b, _) = crowded(CROWD, route, "never");
        run(&mut s, "connector.add", json!({"from": {"id": a, "point": 1}, "to": {"id": b, "point": 1}}));
        (s, a)
    };
    for route in [Route::Line(4_000), Route::Zigzag(4_000)] {
        let (mut s, a) = page(Route::Hand);
        let yardstick = timed_move(&mut s, a, "beside routes of 6 rows");
        drop(s);
        let (mut s, a) = page(route);
        let crafted = timed_move(&mut s, a, &format!("beside {route:?}"));
        within(&format!("one connector routed beside {CROWD} of {route:?}"), crafted, yardstick, 2.0);
    }
}

/// Finding 4, timed: the floor of a move (ten thousand connectors set never to reroute) with a shared route of 4,000 rows, against one of six.
#[test]
#[ignore = "a timing, run by hand"]
fn timed_floor_with_long_shared_routes_against_short_ones() {
    let (mut s, a, _, _) = crowded(CROWD, Route::Hand, "never");
    let yardstick = timed_move(&mut s, a, "never, 6 rows");
    drop(s);
    let (mut s, a, _, _) = crowded(CROWD, Route::Zigzag(4_000), "never");
    let crafted = timed_move(&mut s, a, "never, 4,000 rows");
    within("the floor with routes of 4,000 rows", crafted, yardstick, 1.5);
}

/// Finding 5, timed: a move of the shape twenty thousand plain connectors are glued to (the budget routed, the others said), against the same move that routes none.
#[test]
#[ignore = "a timing, run by hand"]
fn timed_a_move_past_the_budget_against_the_move_that_routes_none() {
    let n = 2 * CROWD;
    let (mut s, a, _, _) = crowded(n, Route::Hand, "never");
    let yardstick = timed_move(&mut s, a, "20,000, never");
    drop(s);
    let (mut s, a, _, _) = crowded(n, Route::Hand, "freely");
    let crafted = timed_move(&mut s, a, "20,000, freely");
    within("a move of the shape 20,000 connectors are glued to", crafted, yardstick, 3.0);
}
