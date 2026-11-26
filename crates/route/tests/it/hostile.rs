//! The crafted inputs of design §2.11, counted: each is routed on a budget, and the work it cost is a number the test holds, never a time. Each has an ignored timed twin
//! beside the yardstick (the wall routed 10,000 times), for the gate's machine and.

use std::time::Instant;

use diacraft_route::{
    Dir, MAX_AREA_OBSTACLES, MAX_AREA_WIRES, MAX_COORD, MAX_EXPANSIONS, MAX_ROUTE_WORK, Obstacle, Obstacles, Outcome, Point, Request, Settings, Wire,
    Wires, Work, route,
};

use crate::support::{end, free, obstacle, p, request, settings};

const BUDGET: u64 = 50_000_000;

/// Routes on `BUDGET` steps and hands back the answer and the work, after the checks that hold for every input: the work never passes the budget, and (while the work lasts)
/// each loop walked what it charged.
fn run(obstacles: Vec<Obstacle>, wires: Vec<Wire>, req: &Request, s: &Settings) -> (Outcome, Work) {
    let mut work = Work::new(BUDGET);
    let outcome = route(&Obstacles::new(obstacles), &Wires::new(wires), req, s, &mut work);
    assert!(work.spent() <= BUDGET);
    if !work.is_spent() {
        assert_eq!(work.charged(), work.walked(), "a loop walked what it did not charge, or charged what it did not walk");
        assert_eq!(work.charged().total(), work.spent());
    }
    (outcome, work)
}

fn upright_wire(id: u32, x: f64) -> Wire {
    Wire { id, legs: vec![(p(x, 10.0), p(x, 11.0))], glued: [None, None] }
}

/// A square spiral of `n` walls: each wall a thin box along the leg, each leg longer than the one before by a corridor's width.
fn spiral(n: u32) -> Vec<Obstacle> {
    let (mut x, mut y) = (0.0, 0.0);
    let mut walls = Vec::new();
    for i in 0..n {
        let len = 3.0 + 2.0 * f64::from(i / 2);
        let (dx, dy) = Dir::from_index(i as usize).vector();
        let (nx, ny) = (x + dx * len, y + dy * len);
        let (x0, x1, y0, y1) = (x.min(nx) - 0.25, x.max(nx) + 0.25, y.min(ny) - 0.25, y.max(ny) + 0.25);
        walls.push(obstacle(i, x0, y0, x1, y1));
        (x, y) = (nx, ny);
    }
    walls
}

#[test]
fn a_spiral_of_a_thousand_walls_costs_what_the_caps_say() {
    let req = request(end(1.5, 1.5, Dir::Right, None), end(5000.0, 5000.0, Dir::Left, None));
    let (outcome, work) = run(spiral(1000), Vec::new(), &req, &settings(0.25));
    assert!(matches!(outcome, Outcome::OutOfWork | Outcome::Refused(_) | Outcome::NoWay), "{outcome:?}");
    assert!(work.spent() <= BUDGET);
}

#[test]
fn a_comb_of_two_thousand_teeth_is_gone_under() {
    let mut comb = vec![obstacle(5000, 0.0, 100.0, 4000.0, 101.0)];
    comb.extend((0..2000).map(|i| obstacle(i, f64::from(i) * 2.0, 0.0, f64::from(i) * 2.0 + 0.5, 100.0)));
    let req = request(end(-5.0, 50.0, Dir::Right, None), end(4005.0, 50.0, Dir::Left, None));
    let (outcome, work) = run(comb, Vec::new(), &req, &settings(0.25));
    assert!(matches!(outcome, Outcome::Routed { .. } | Outcome::OutOfWork), "{outcome:?}");
    assert!(work.spent() <= MAX_ROUTE_WORK + 8, "one route takes at most its own cap: {}", work.spent());
}

#[test]
fn ten_thousand_boxes_on_one_spot_are_past_the_area_cap_and_cost_the_visits_of_one_query() {
    let crowd: Vec<Obstacle> = (0..10_000).map(|i| obstacle(i, 0.0, 0.0, 1.0, 1.0)).collect();
    let req = request(end(-3.0, 0.5, Dir::Right, None), end(4.0, 0.5, Dir::Left, None));
    let (outcome, work) = run(crowd, Vec::new(), &req, &settings(0.25));
    assert_eq!(outcome, Outcome::OutOfWork);
    assert!(work.spent() < 20_000, "the area query is cut at its visits: {}", work.spent());
}

#[test]
fn ten_thousand_boxes_along_the_route_are_past_the_area_cap() {
    let line: Vec<Obstacle> = (0..10_000).map(|i| obstacle(i, f64::from(i), 0.0, f64::from(i) + 0.1, 0.1)).collect();
    let req = request(end(-5.0, 0.05, Dir::Right, None), end(10_005.0, 0.05, Dir::Left, None));
    let (outcome, work) = run(line, Vec::new(), &req, &settings(0.25));
    assert_eq!(outcome, Outcome::OutOfWork);
    assert!(work.spent() < 50_000, "{}", work.spent());
}

#[test]
fn tiny_boxes_far_from_the_origin_are_routed_or_refused_and_never_overflow() {
    let far = MAX_COORD - 2.0;
    let boxes: Vec<Obstacle> =
        (0..1000).map(|i| obstacle(i, far + f64::from(i) * 1.0e-3, far, far + f64::from(i) * 1.0e-3 + 1.0e-6, far + 1.0e-6)).collect();
    let req = request(end(far - 1.0, far, Dir::Right, None), end(far + 1.5, far, Dir::Left, None));
    let (outcome, _) = run(boxes, Vec::new(), &req, &settings(1.0e-4));
    assert!(matches!(outcome, Outcome::Routed { .. } | Outcome::NoWay | Outcome::OutOfWork), "{outcome:?}");
}

#[test]
fn boxes_that_are_not_numbers_cost_nothing_and_change_nothing() {
    let bad = vec![
        obstacle(1, f64::NAN, 0.0, 1.0, 1.0),
        obstacle(2, 0.0, f64::NAN, 1.0, 1.0),
        obstacle(3, 0.0, 0.0, f64::INFINITY, 1.0),
        obstacle(4, f64::NEG_INFINITY, 0.0, 1.0, 1.0),
        obstacle(5, 0.0, 0.0, 1.0e300, 1.0),
    ];
    let req = request(end(-3.0, 0.5, Dir::Right, None), end(4.0, 0.5, Dir::Left, None));
    let (outcome, _) = run(bad, Vec::new(), &req, &settings(0.25));
    assert_eq!(outcome, run(Vec::new(), Vec::new(), &req, &settings(0.25)).0);
}

#[test]
fn a_clearance_as_large_as_the_page_is_clamped_and_costs_no_more() {
    let mut s = settings(1.0e9);
    s.spacing = (1.0e9, 1.0e9);
    let req = request(end(0.0, 0.0, Dir::Right, None), end(10.0, 5.0, Dir::Left, None));
    let (outcome, work) = run(vec![obstacle(1, 3.0, -1.0, 4.0, 6.0)], vec![upright_wire(1, 5.0)], &req, &s);
    assert!(matches!(outcome, Outcome::Routed { .. } | Outcome::NoWay | Outcome::OutOfWork), "{outcome:?}");
    assert!(work.spent() < 1_000_000, "{}", work.spent());
    let mut s = settings(0.25);
    s.bend = f64::NAN;
    s.cross = -3.0;
    s.near = f64::INFINITY;
    s.margin = f64::NAN;
    let (outcome, _) = run(Vec::new(), Vec::new(), &req, &s);
    assert!(matches!(outcome, Outcome::Routed { .. }), "{outcome:?}");
}

#[test]
fn one_huge_shape_round_everything_is_gone_round() {
    let big = vec![obstacle(1, -900_000.0, -900_000.0, 900_000.0, 900_000.0), obstacle(2, -3.0, -3.0, 3.0, 3.0)];
    let req = request(end(950_000.0, 0.0, Dir::Left, None), end(960_000.0, 5.0, Dir::Left, None));
    let (outcome, work) = run(big, Vec::new(), &req, &settings(0.25));
    assert!(matches!(outcome, Outcome::Routed { .. } | Outcome::NoWay | Outcome::OutOfWork), "{outcome:?}");
    assert!(work.spent() < 1_000_000);
}

// --- every cap, at its limit and one past it --------------------------------------------------------------------------------------------------------------------

/// `n` boxes on a diagonal band above the straight route from (-5, 0) to (far, 0): the route is straight, but each box is in the area.
fn band(n: u32) -> (Vec<Obstacle>, Request) {
    let boxes = (0..n).map(|i| obstacle(i, f64::from(i) * 3.0, 10.0, f64::from(i) * 3.0 + 1.0, 11.0)).collect();
    (boxes, request(free(-5.0, 0.0), free(f64::from(n) * 3.0 + 5.0, 0.0)))
}

#[test]
fn the_area_obstacle_cap_itself_is_a_route_and_one_past_it_is_out_of_work() {
    let (boxes, req) = band(MAX_AREA_OBSTACLES as u32);
    let (outcome, work) = run(boxes, Vec::new(), &req, &settings(0.25));
    assert!(matches!(outcome, Outcome::Routed { .. }), "{outcome:?}");
    assert!(work.spent() <= MAX_ROUTE_WORK);
    let (boxes, req) = band(MAX_AREA_OBSTACLES as u32 + 1);
    let (outcome, work) = run(boxes, Vec::new(), &req, &settings(0.25));
    assert_eq!(outcome, Outcome::OutOfWork);
    assert!(work.spent() < 50_000, "{}", work.spent());
}

fn wires_band(n: u32) -> (Vec<Wire>, Request) {
    ((0..n).map(|i| upright_wire(i, f64::from(i) * 3.0 + 0.5)).collect(), request(free(-5.0, 0.0), free(f64::from(n) * 3.0 + 5.0, 0.0)))
}

#[test]
fn the_area_wire_cap_itself_is_a_route_and_one_past_it_is_out_of_work() {
    let (wires, req) = wires_band(MAX_AREA_WIRES as u32);
    let (outcome, work) = run(Vec::new(), wires, &req, &settings(0.25));
    assert!(matches!(outcome, Outcome::Routed { .. }), "{outcome:?}");
    assert!(work.spent() <= MAX_ROUTE_WORK);
    let (wires, req) = wires_band(MAX_AREA_WIRES as u32 + 1);
    let (outcome, _) = run(Vec::new(), wires, &req, &settings(0.25));
    assert_eq!(outcome, Outcome::OutOfWork);
}

#[test]
fn the_grid_line_cap_is_out_of_work_and_a_grid_under_it_is_a_route() {
    // Every wire the route keeps its spacing from adds two lines on its axis.
    let mut s = settings(0.25);
    s.spacing = (0.5, 0.5);
    let (wires, req) = wires_band(4_000);
    let (outcome, _) = run(Vec::new(), wires, &req, &s);
    assert!(matches!(outcome, Outcome::Routed { .. }), "{outcome:?}");
    let (wires, req) = wires_band(MAX_AREA_WIRES as u32);
    let (outcome, _) = run(Vec::new(), wires, &req, &s);
    assert_eq!(outcome, Outcome::OutOfWork);
}

#[test]
fn the_edit_work_is_a_cap_across_routes_and_a_route_cannot_pass_it() {
    // The work of one call is charged to the edit's: a budget of 30,000 is spent by the second route of the wall, never passed, and the route after that is `OutOfWork`.
    let walls = vec![obstacle(1, 0.0, 0.0, 2.0, 2.0), obstacle(2, 5.0, 0.0, 7.0, 2.0), obstacle(3, 3.0, -2.0, 4.0, 5.0)];
    let req = request(end(2.0, 1.0, Dir::Right, Some(1)), end(5.0, 1.0, Dir::Left, Some(2)));
    let (o, w) = (Obstacles::new(walls), Wires::new(Vec::new()));
    let mut work = Work::new(30_000);
    let mut costs = Vec::new();
    for _ in 0..200 {
        let before = work.spent();
        let outcome = route(&o, &w, &req, &settings(0.5), &mut work);
        costs.push((outcome, work.spent() - before));
        assert!(work.spent() <= 30_000);
    }
    assert!(matches!(costs[0].0, Outcome::Routed { .. }));
    assert_eq!(costs[199].0, Outcome::OutOfWork);
    assert!(costs.iter().filter(|(o, _)| matches!(o, Outcome::Routed { .. })).count() >= 2);
}

#[test]
fn the_expansion_cap_ends_a_search_that_would_never_end() {
    // A serpentine the route must wind through: each leg is long, so the grid is large and the search reaches the state cap or the work cap, never more.
    let mut walls = Vec::new();
    for i in 0..60 {
        let (x0, x1) = if i % 2 == 0 { (-5000.0, 4990.0) } else { (-4990.0, 5000.0) };
        let y = f64::from(i) * 2.0 + 1.0;
        walls.push(obstacle(i, x0, y, x1, y + 0.5));
    }
    let req = request(end(0.0, 0.0, Dir::Up, None), end(0.0, 122.0, Dir::Down, None));
    let (outcome, work) = run(walls, Vec::new(), &req, &settings(0.25));
    assert!(matches!(outcome, Outcome::OutOfWork | Outcome::Refused(_) | Outcome::Routed { .. }), "{outcome:?}");
    assert!(work.charged().states <= MAX_EXPANSIONS, "{:?}", work.charged());
}

// --- the timed twins, ignored: for the gate's machine, beside the yardstick ----------------------------------------------------------------------------------

/// The yardstick: the wall of the vectors routed 10,000 times.
fn yardstick() -> f64 {
    let walls = vec![obstacle(1, 0.0, 0.0, 2.0, 2.0), obstacle(2, 5.0, 0.0, 7.0, 2.0), obstacle(3, 3.0, -2.0, 4.0, 5.0)];
    let req = request(end(2.0, 1.0, Dir::Right, Some(1)), end(5.0, 1.0, Dir::Left, Some(2)));
    let (o, w) = (Obstacles::new(walls), Wires::new(Vec::new()));
    let start = Instant::now();
    for _ in 0..10_000 {
        let outcome = route(&o, &w, &req, &settings(0.5), &mut Work::new(BUDGET));
        assert!(matches!(outcome, Outcome::Routed { .. }));
    }
    start.elapsed().as_secs_f64()
}

fn timed(name: &str, make: impl Fn() -> (Vec<Obstacle>, Vec<Wire>, Request, Settings)) {
    let base = yardstick();
    let (o, w, req, s) = make();
    let start = Instant::now();
    let (outcome, work) = run(o, w, &req, &s);
    let took = start.elapsed().as_secs_f64();
    println!(
        "{name}: {outcome:?} in {took:.3} s ({:.1} yardsticks), {} steps; the yardstick (the wall, 10,000 times) took {base:.3} s",
        took / base,
        work.spent()
    );
}

#[test]
#[ignore = "timed: for the gate's machine"]
fn timed_the_spiral() {
    timed("spiral of 1,000 walls", || {
        (spiral(1000), Vec::new(), request(end(1.5, 1.5, Dir::Right, None), end(5000.0, 5000.0, Dir::Left, None)), settings(0.25))
    });
}

#[test]
#[ignore = "timed: for the gate's machine"]
fn timed_the_comb() {
    timed("comb of 2,000 teeth", || {
        let mut comb = vec![obstacle(5000, 0.0, 100.0, 4000.0, 101.0)];
        comb.extend((0..2000).map(|i| obstacle(i, f64::from(i) * 2.0, 0.0, f64::from(i) * 2.0 + 0.5, 100.0)));
        (comb, Vec::new(), request(end(-5.0, 50.0, Dir::Right, None), end(4005.0, 50.0, Dir::Left, None)), settings(0.25))
    });
}

#[test]
#[ignore = "timed: for the gate's machine"]
fn timed_the_largest_areas() {
    timed("2,048 obstacles in the area", || {
        let (b, r) = band(MAX_AREA_OBSTACLES as u32);
        (b, Vec::new(), r, settings(0.25))
    });
    timed("4,096 wires in the area", || {
        let (w, r) = wires_band(MAX_AREA_WIRES as u32);
        (Vec::new(), w, r, settings(0.25))
    });
    timed("10,000 boxes on one spot", || {
        ((0..10_000).map(|i| obstacle(i, 0.0, 0.0, 1.0, 1.0)).collect(), Vec::new(), request(free(-3.0, 0.5), free(4.0, 0.5)), settings(0.25))
    });
}

#[test]
#[ignore = "timed: for the gate's machine"]
fn timed_a_chain_of_ten_thousand_routes_each_a_wire_for_the_next() {
    let base = yardstick();
    let mut work = Work::new(u64::MAX / 4);
    let start = Instant::now();
    let mut wires: Vec<Wire> = Vec::new();
    let mut routed = 0usize;
    for i in 0..10_000u32 {
        let y = f64::from(i) * 0.5;
        let req = Request { begin: free(0.0, y), end: free(20.0, y + 0.25), exempt: Vec::new(), glued: [None, None] };
        let outcome = route(&Obstacles::new(Vec::new()), &Wires::new(wires.clone()), &req, &settings(0.25), &mut work);
        if let Outcome::Routed { points, .. } = outcome {
            routed += 1;
            let legs = points.windows(2).map(|w| (w[0], w[1])).collect::<Vec<(Point, Point)>>();
            wires.push(Wire { id: i, legs, glued: [None, None] });
        }
        if wires.len() > 64 {
            wires.remove(0);
        }
    }
    println!("chain: {routed} routes in {:.3} s, {} steps; the yardstick took {base:.3} s", start.elapsed().as_secs_f64(), work.spent());
}
