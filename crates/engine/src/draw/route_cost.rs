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
    // The connector added is glued to a shape of its own (D) and to B: moving D moves no end of the 10,000 fixed routes, which a move of A would follow, each a try, and the
    // tries of the edit are counted in the order of the `ID`s (the crowd's come first): only the one connector is routed, as the test means.
    let page = |route: Route| {
        let (mut s, _, b, _) = crowded(CROWD, route, "never");
        let d = drop_rectangle(&mut s, 6.0, 9.0);
        run(&mut s, "connector.add", json!({"from": {"id": d, "point": 2}, "to": {"id": b, "point": 1}}));
        (s, d)
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
    // The ends of the connectors moved with A, and a route whose ends moved is followed square, which is a try; a route of 4,000 rows is not read, so it
    // is not followed and not written (`a_fixed_route_of_more_rows_than_the_crossing_test_reads_is_not_followed_and_not_read`). Before, a connector set never to reroute was no try.
    assert_eq!((crowded.attempted, crowded.routed), (CROWD as usize, 0));
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

/// The scan's finding 3: the pages one commit reached are routed within **one budget of the evaluator's steps**, as the tries are one budget for the edit, and not a budget
/// for each page: a move of a shape that three pages read hands the routings of those pages 20 million steps in all, and the page of the move 20 million more.
#[test]
fn the_pages_a_commit_reached_are_routed_within_one_budget_of_steps() {
    let mut s = blank();
    run(&mut s, "page.rename", json!({"id": 1, "name": "One"}));
    let one = drop_rectangle(&mut s, 1.0, 1.0);
    for k in 0..3 {
        let page = id_of(&run(&mut s, "page.add", json!({"name": format!("Reader-{k}")})));
        let drop = |s: &mut Session, x: f64, y: f64| id_of(&run(s, "shape.drop", json!({"master": "rectangle", "page": page, "x": x, "y": y})));
        let (a, b) = (drop(&mut s, 2.0, 2.0), drop(&mut s, 6.0, 4.0));
        run(&mut s, "connector.add", json!({"page": page, "from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}}));
        run(&mut s, "sheet.set_cell", json!({"page": page, "id": a, "cell": "PinX", "formula": format!("Pages[One]!Sheet.{one}!PinX+1")}));
    }
    super::settle::GRANTED.with(|c| c.set(0));
    super::settle::ATTEMPTED.with(|c| c.set(0));
    run(&mut s, "shape.move", json!({"ids": [one], "page": 1, "dx": 3.0, "dy": 0.0}));
    let tried = super::settle::ATTEMPTED.with(std::cell::Cell::get);
    assert!(tried >= 3, "the connector of each of the three pages was tried: {tried}");
    let granted = super::settle::GRANTED.with(std::cell::Cell::get);
    assert!(
        granted <= 2 * diacraft_calc::Budget::DEFAULT_STEPS,
        "{granted} steps handed to the routings: the page of the move and one budget for the three pages it reached"
    );
}

/// A fixed route whose ends moved is **followed square**, and the follow is a **try**, counted as the crossing test is. A move of the shape
/// ten thousand connectors set to reroute never are glued to tries each once, writes each route once, searches the page for no shape, and passes over the page's `Connect` rows once;
/// a route of more rows than the crossing test reads is not read at all and is not written. Against the yardstick of the same move with the connectors set freely, which are each
/// routed again.
#[test]
fn following_ten_thousand_fixed_routes_is_a_try_each_and_searches_the_page_for_none() {
    let (mut s, a, _, _) = crowded(CROWD, Route::Hand, "freely");
    let routed_again = moved(&mut s, a, "10,000 connectors of 6 rows, freely");
    drop(s);
    let (mut s, a, _, ids) = crowded(CROWD, Route::Hand, "never");
    let connects = s.document().unwrap().page(1).unwrap().connects.len();
    let followed = moved(&mut s, a, "10,000 connectors of 6 rows, never");
    assert_eq!((followed.attempted, followed.routed), (CROWD as usize, CROWD as usize), "every fixed route is a try, and is followed");
    assert!(
        followed.finds <= routed_again.finds,
        "the follow searched the page: {} shapes visited, {} routing them again",
        followed.finds,
        routed_again.finds
    );
    assert!(followed.connects <= connects, "the follow passed over {} `Connect` rows, the page holds {connects}", followed.connects);
    // Each route is read twice, within the rows of a hand path: as it was (its six rows) to follow it, and as it is now to see whether its rows already give the followed route
    // (a file's one-leg route with `Width*1`); and the jump pass reads the routes of at most one more connector than take part in the jumps.
    let jumps = (super::MAX_JUMP_PARTICIPANTS as u64 + 1) * 8;
    let most = routed_again.rows + 2 * u64::from(CROWD) * 6 + jumps;
    assert!(followed.rows <= most, "the follow merged {} rows, {} routing them again, at most {most}", followed.rows, routed_again.rows);
    // The route is followed square: the first corner went up with the begin, the rest stayed (the routes of the copies are one hand path, jogging up an inch).
    for id in [ids[0], ids[ids.len() / 2], ids[ids.len() - 1]] {
        assert_eq!(own_route_rows(s.document().unwrap(), id), 6, "connector {id} keeps its six corners");
    }
}

/// The follow reads **at most the crossing test's rows** of the route it had, and a route of more is not read, not followed and not written: it keeps the rows it has, as a route past
/// the test's rows does for the test. A try all the same, so the edit's budget counts it.
#[test]
fn a_fixed_route_of_more_rows_than_the_crossing_test_reads_is_not_followed_and_not_read() {
    let (mut s, a, _, _) = crowded(CROWD, Route::Hand, "never");
    let yardstick = moved(&mut s, a, "10,000 connectors of 6 rows, never");
    drop(s);
    let rows = u32::try_from(super::MAX_CROSSING_ROWS).unwrap() + 1;
    let (mut s, a, _, ids) = crowded(CROWD, Route::Zigzag(rows), "never");
    let long = moved(&mut s, a, &format!("10,000 connectors of {rows} rows, never"));
    assert_eq!((long.attempted, long.routed), (CROWD as usize, 0), "tried, and none is followed or written");
    assert!(long.rows <= yardstick.rows, "a route past the rows is not read: {} rows merged, {} for routes of six", long.rows, yardstick.rows);
    for id in [ids[0], ids[ids.len() - 1]] {
        assert_eq!(own_route_rows(s.document().unwrap(), id), rows as usize, "connector {id} keeps its rows");
    }
}

/// A move that moves no end a connector is glued to is no try for a fixed route set to never: a connector whose end stayed (the shape was resized about the glued point) costs
/// nothing and is not written.
#[test]
fn a_fixed_route_whose_ends_did_not_move_is_no_try() {
    let (mut s, a, _, _) = crowded(CROWD, Route::Hand, "never");
    // A is wider to the right, its right point (3, glued) where the points of its right side are: the corner at (0, 0) of A's frame is its lower left, (2.25, 3.5).
    let cost = counted(&mut s, "A wider to the left", "shape.resize", json!({"id": a, "x": 2.25, "y": 3.5, "width": 1.5, "height": 1.0}));
    assert_eq!((cost.attempted, cost.routed), (0, 0), "no end moved: no try and no write");
}

/// The tries are the edit's budget: of the fixed routes past it the ends follow and the rows stay, and the answer says them, as it says the connectors it did not route again.
#[test]
fn a_move_follows_at_most_the_budget_of_fixed_routes_and_says_the_connectors_it_left() {
    let budget = super::MAX_ROUTED_PER_EDIT;
    let extra = 100_u32;
    let (mut s, a, _, ids) = crowded(u32::try_from(budget).unwrap() + extra, Route::Hand, "never");
    let cost = moved(&mut s, a, "a move of the shape 10,100 fixed connectors are glued to");
    assert_eq!(cost.attempted, budget, "the tries of the edit");
    let (listed, unlisted) = said_left(&cost.answer);
    assert_eq!(listed.len() as u64 + unlisted, u64::from(extra), "every connector left is said: {}", cost.answer);
    let last = *ids.last().unwrap();
    assert!(listed.iter().all(|l| l["id"].as_u64().is_some_and(|id| id > u64::from(last - extra))), "{listed:?}");
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
    // The connector added is glued to a shape of its own (D) and to B: moving D moves no end of the 10,000 fixed routes, which a move of A would follow, each a try, and the
    // tries of the edit are counted in the order of the `ID`s (the crowd's come first): only the one connector is routed, as the test means.
    let page = |route: Route| {
        let (mut s, _, b, _) = crowded(CROWD, route, "never");
        let d = drop_rectangle(&mut s, 6.0, 9.0);
        run(&mut s, "connector.add", json!({"from": {"id": d, "point": 2}, "to": {"id": b, "point": 1}}));
        (s, d)
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

// ---- the connectors a recalculation moved ----

/// The shape ten thousand connectors are glued to is resized by **a formula** and not named: a write to the anchor it reads. The connectors are routed again once each, as they are
/// by the move of the shape named (the yardstick, same page, same count), by the comparison of the drawing before and after the recalculation: the routing **passes over the page's
/// `Connect` rows once**, the page is searched for the anchor and no more (a search for each connector would be ten thousand walks of ten thousand shapes), and the points of the shape
/// are read once for the call.
#[test]
fn routing_ten_thousand_connectors_of_a_shape_a_formula_resized_costs_what_the_move_of_it_costs() {
    let (mut s, a, _, _) = crowded(CROWD, Route::Hand, "freely");
    let yardstick = moved(&mut s, a, &format!("{CROWD} connectors, A named in a move"));
    drop(s);
    let (mut s, a, _, ids) = crowded(CROWD, Route::Hand, "freely");
    let anchor = drop_rectangle(&mut s, 1.0, 9.0);
    run(&mut s, "sheet.set_cell", json!({"id": a, "cell": "Width", "formula": format!("Sheet.{anchor}!Width")}));
    let connects = s.document().unwrap().page(1).unwrap().connects.len();
    let width = number(s.document().unwrap(), a, "Width");
    let recalculated = counted(&mut s, "a write to the anchor A reads", "sheet.set_cell", json!({"id": anchor, "cell": "Width", "formula": "3"}));
    assert!((number(s.document().unwrap(), a, "Width") - 3.0).abs() < 1.0e-9 && width != 3.0, "A follows the anchor: it was {width}");
    assert_eq!((recalculated.attempted, recalculated.routed), (CROWD as usize, CROWD as usize), "every connector of A is routed again, once");
    assert!(recalculated.connects <= connects, "the routing passed over {} `Connect` rows, the page holds {connects}", recalculated.connects);
    // A few searches of the page for the anchor and for what the write names, and none for a connector.
    assert!(
        recalculated.finds <= yardstick.finds + 4 * (u64::from(CROWD) + 8),
        "{} shapes visited by searches, {} with A named: the routing searched the page",
        recalculated.finds,
        yardstick.finds
    );
    let jumps = (super::MAX_JUMP_PARTICIPANTS as u64 + 1) * 8;
    assert!(
        recalculated.rows <= yardstick.rows + 2 * 4 + jumps + 64,
        "the routing merged {} rows, {} with A named: the points of a glued shape are read once for the call",
        recalculated.rows,
        yardstick.rows
    );
    // Each is routed: the route it had (a hand path of six rows) is replaced by the route of its own ends.
    for id in [ids[0], ids[ids.len() / 2], ids[ids.len() - 1]] {
        assert!(own_route_rows(s.document().unwrap(), id) < 6, "connector {id} has a new route");
    }
}

/// The faces a page's text is measured in, process-wide: the tests that set them take turns (nextest gives each a process of its own).
static FACES: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A narrow default face and a wide one the themes of the catalogue name, so that a theme that changes the face changes the width of text on every machine. Held while the test runs.
struct Faces(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

impl Faces {
    fn narrow_default_and_wide_dejavu() -> Faces {
        let held = FACES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut db = diacraft_fonts::FontDb::empty();
        for (family, advance) in [("Liberation Sans", 450), ("DejaVu Sans", 800)] {
            assert_eq!(db.add_font_data(diacraft_fonts::synth::SynthFont::new(family).ascii(advance).build()).added, 1, "{family}");
        }
        crate::measure::set_measure_fonts(Some(Arc::new(db)));
        Faces(held)
    }
}

impl Drop for Faces {
    fn drop(&mut self) {
        crate::measure::set_measure_fonts(None);
    }
}

/// A page of `n` pairs: a box B whose width is its text's (`TEXTWIDTH(TheText)` and a margin), a plain box D and a connector from B to D. A theme that changes the face changes the width
/// of every B. → the session, the connectors' `ID`s, and the `ID` of the first B.
fn text_sized_pairs(n: u32) -> (Session, Vec<u32>, u32) {
    let mut s = blank();
    let mut wires = Vec::new();
    let mut first = 0;
    let mut writes = Vec::new();
    for k in 0..n {
        let (x, y) = (2.0 + f64::from(k % 10) * 2.5, 2.0 + f64::from(k / 10) * 2.0);
        let b = drop_rectangle(&mut s, x, y);
        let d = drop_rectangle(&mut s, x, y + 1.0);
        if k == 0 {
            first = b;
        }
        run(&mut s, "shape.set_text", json!({"id": b, "text": "Hello, world"}));
        writes.push(json!({"id": b, "cell": "Width", "formula": "TEXTWIDTH(TheText)+0.4"}));
        wires.push(id_of(&run(&mut s, "connector.add", json!({"from": {"id": b, "point": 1}, "to": {"id": d, "point": 3}}))));
    }
    run(&mut s, "sheet.set_cells", json!({"writes": writes}));
    (s, wires, first)
}

/// A theme that changes the face every box's width is made of routes the connector of each box once and **reads the page's glue once** (twice the pairs, twice the work and no more: a
/// pass for each connector would be four times), a theme that changes no size (the face the boxes had) routes none, and the same theme again routes none either.
#[test]
fn a_theme_that_resizes_every_box_routes_each_connector_once_and_one_that_resizes_none_routes_none() {
    let _faces = Faces::narrow_default_and_wide_dejavu();
    let mut costs: Vec<(u32, Cost)> = Vec::new();
    for n in [20, 40] {
        let (mut s, wires, first) = text_sized_pairs(n);
        let width = number(s.document().unwrap(), first, "Width");
        let page_connects = s.document().unwrap().page(1).unwrap().connects.len();
        // The face the boxes have already (the default, Liberation Sans): the theme links every shape to a style and changes no size.
        let same = counted(&mut s, &format!("theme.apply tidepool, {n} pairs"), "theme.apply", json!({"theme": "tidepool"}));
        assert!((number(s.document().unwrap(), first, "Width") - width).abs() < 1.0e-9, "the face is the default's: no width changes");
        assert_eq!(
            (same.attempted, same.routed),
            (0, 0),
            "{n} pairs: a theme that resizes nothing routes nothing, though it links every shape: {}",
            same.answer
        );
        // The wide face: every box is wider, and the connector of each is routed again.
        let wide = counted(&mut s, &format!("theme.apply fernwood, {n} pairs"), "theme.apply", json!({"theme": "fernwood"}));
        assert!(number(s.document().unwrap(), first, "Width") > width + 0.1, "the wider face widens the boxes");
        assert_eq!((wide.attempted, wide.routed), (n as usize, n as usize), "{n} pairs: each connector is routed again, once");
        assert!(
            wide.connects <= page_connects,
            "{n} pairs: the routing passed over {} `Connect` rows, the page holds {page_connects}",
            wide.connects
        );
        let again = counted(&mut s, &format!("theme.apply fernwood again, {n} pairs"), "theme.apply", json!({"theme": "fernwood"}));
        assert_eq!((again.attempted, again.routed), (0, 0), "{n} pairs: the same theme again resizes nothing");
        assert_eq!(wires.len(), n as usize);
        costs.push((n, wide));
    }
    // Twice the pairs, no more than about twice the work: the counts that would show a pass over the page for each connector or for each shape moved.
    let ((_, small), (_, large)) = (&costs[0], &costs[1]);
    let linear = |what: &str, a: u64, b: u64| {
        assert!(b <= 2 * a + 64, "{what}: {b} for 40 pairs, {a} for 20: more than twice, the work grows faster than the pairs");
    };
    linear("shapes visited by searches", small.finds, large.finds);
    linear("rows merged", small.rows, large.rows);
    linear("cells walked", small.cells, large.cells);
    linear("`Connect` rows passed over", small.connects as u64, large.connects as u64);
}

// ---- the pages a recalculation moved shapes on share one budget of steps (the security scan's finding) ----

/// A drawing of `pages` pages that are each a small diagram: a box B as wide as its text (`TEXTWIDTH(TheText)` and a margin), a box D, a connector from B to D, and `readers` cells of D
/// that read the route of the connector (each is worked out again when the connector is routed, and in the page's whole recalculation). B is shape 1 of its page.
fn text_diagrams(pages: u32, readers: u32) -> Session {
    let mut s = blank();
    for p in 1..=pages {
        let page = if p == 1 { 1 } else { id_of(&run(&mut s, "page.add", json!({"name": format!("Page-{p}")}))) };
        let drop = |s: &mut Session, x: f64, y: f64| id_of(&run(s, "shape.drop", json!({"master": "rectangle", "page": page, "x": x, "y": y})));
        let (b, d) = (drop(&mut s, 2.0, 2.0), drop(&mut s, 7.0, 4.0));
        run(&mut s, "shape.set_text", json!({"page": page, "id": b, "text": "Hi"}));
        run(&mut s, "sheet.set_cell", json!({"page": page, "id": b, "cell": "Width", "formula": "TEXTWIDTH(TheText)+0.4"}));
        let wire = id_of(&run(&mut s, "connector.add", json!({"page": page, "from": {"id": b}, "to": {"id": d}})));
        s.edit("readers of the route", |doc| {
            let at = doc.find_shape(Owner::Page(page), d).unwrap();
            let shape = super::shape_mut(doc, page, &at.path).unwrap();
            let sheet = Arc::make_mut(&mut shape.sheet);
            for k in 0..readers {
                let addr = diacraft_doc::CellAddr::in_row("User", None, diacraft_doc::RowKey::name(&format!("R{k}")), "Value");
                let cell = Cell::with_formula(&format!("Sheet.{wire}!Geometry1.X2+{k}"), Some(Value::from_text("0")));
                sheet.set_cell(&addr, cell).unwrap();
            }
            Ok(())
        })
        .unwrap();
    }
    s
}

/// The rows of the first `Geometry` section of the connector of page `page` (the page's only connector).
fn route_rows_on(doc: &Document, page: u32) -> usize {
    let wire = doc.page(page).unwrap().connects.iter().next().unwrap().from_sheet;
    let at = doc.find_shape(Owner::Page(page), wire).unwrap();
    doc.shape(&at).unwrap().sheet.sections.iter().find(|s| s.name.as_str() == "Geometry" && s.ix == Some(0)).map_or(0, |s| s.rows.len())
}

/// Writes the text of box B (shape 1) of every page and works the cells that read it out, with no command, so that nothing routes. → the pages.
fn widened(doc: &mut Document) -> Vec<u32> {
    use diacraft_doc::{Text, TextPart};
    let pages: Vec<u32> = doc.pages.iter().map(|p| p.id).collect();
    for page in &pages {
        let at = doc.find_shape(Owner::Page(*page), 1).unwrap();
        let shape = super::shape_mut(doc, *page, &at.path).unwrap();
        shape.text = Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from("Hello, wonderful world"))], ext: Default::default() }));
        let cells = vec![(SheetRef::Shape(at), diacraft_doc::CellAddr::direct("TheText"))];
        super::settle::commit(doc, *page, &diacraft_calc::Changes { cells, ..diacraft_calc::Changes::default() }).unwrap();
    }
    pages
}

/// **The pages a recalculation resized shapes on are routed within one budget of the evaluator's steps** (`Budget::DEFAULT_STEPS`, the budget of a whole drawing), and not a budget for
/// each page: the refresh of the fields of a drawing of many pages routes every page it changed, and each routing was handed 20 million steps (`route_recalculated_pages`). The same for a
/// replacement over the pages of a drawing (`edit.replace`, which routes after its writes).
#[test]
fn the_pages_a_recalculation_resized_shapes_on_are_routed_within_one_budget_of_steps() {
    let pages = 6;
    let s = text_diagrams(pages, 20);
    let was = s.document().unwrap().clone();
    let mut doc = was.clone();
    widened(&mut doc);
    super::settle::GRANTED.with(|c| c.set(0));
    super::settle::ATTEMPTED.with(|c| c.set(0));
    super::settle::route_recalculated_pages(&mut doc, &was).unwrap();
    assert_eq!(super::settle::ATTEMPTED.with(std::cell::Cell::get), pages as usize, "the connector of each page is routed again");
    let granted = super::settle::GRANTED.with(std::cell::Cell::get);
    assert!(granted <= diacraft_calc::Budget::DEFAULT_STEPS, "{granted} steps handed to the routings of {pages} pages: one budget for all of them");
    // The replacement of a text over the pages.
    let mut s = text_diagrams(pages, 20);
    super::settle::GRANTED.with(|c| c.set(0));
    super::settle::ATTEMPTED.with(|c| c.set(0));
    let answer = run(&mut s, "edit.replace", json!({"what": "Hi", "with": "Hello, wonderful world", "scope": "all"}));
    assert_eq!(answer["shapes"], json!(pages), "{answer}");
    assert_eq!(super::settle::ATTEMPTED.with(std::cell::Cell::get), pages as usize, "each page's connector is routed again, once");
    // The commit of each page's cells is handed its budget (a drawing of more than one page keeps the pages a commit reaches within one, which may be none), and the routing of all
    // the pages one more: a routing for each page was twice as many.
    let granted = super::settle::GRANTED.with(std::cell::Cell::get);
    let most = (u64::from(pages) + 1) * diacraft_calc::Budget::DEFAULT_STEPS;
    assert!(granted <= most, "{granted} steps handed to the commits and routings of the replacement over {pages} pages, at most {most}");
}

/// **A theme over many pages is refused when the routing of all of them passes the budget together**, though each page's routing is under it: the pages of a drawing that each cost a
/// share of the steps and together cost more are one call with one budget (the theme's recalculation of each page is its own, as it always was). The budget is a third of what the whole
/// theme takes, so that the routing of the pages (about as much as the recalculation of them) passes it and the recalculation or the routing of one page does not.
#[test]
fn a_theme_over_many_pages_whose_routings_together_pass_the_budget_is_refused() {
    let _faces = Faces::narrow_default_and_wide_dejavu();
    let pages = 8;
    let s = text_diagrams(pages, 300);
    let all: Vec<u32> = s.document().unwrap().pages.iter().map(|p| p.id).collect();
    let fernwood = crate::theme::find("fernwood").unwrap();
    let apply = |budget: &diacraft_calc::Budget| {
        let mut doc = s.document().unwrap().clone();
        diacraft_calc::work::reset();
        let done = crate::theme::apply_within(&mut doc, &all, crate::theme::Choice::Theme(fernwood, 0), crate::theme::Base::Root, budget);
        (done, diacraft_calc::work::steps(), doc)
    };
    let (done, total, themed) = apply(&diacraft_calc::Budget::default());
    done.unwrap();
    eprintln!("a theme over {pages} pages of 300 readers: {total} steps of the evaluator");
    for page in &all {
        assert!(route_rows_on(&themed, *page) > 0, "page {page} has a route");
    }
    let (done, used, _) = apply(&diacraft_calc::Budget::with_steps(total / 3));
    let refused = done.unwrap_err();
    assert!(refused.message.contains("too large to recalculate"), "{}", refused.message);
    assert!(used <= total, "the refused theme took {used} steps, the whole {total}");
}
