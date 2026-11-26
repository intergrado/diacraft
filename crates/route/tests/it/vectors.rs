//! The composed vectors: each is a small scene whose answer is worked out by hand from the design (§1.2 to §2.7), and each names the wrong result it catches.

use diacraft_route::{
    Clearance, Dir, MAX_COORD, MAX_QUERY_VISITS, Obstacle, Obstacles, Outcome, Overlap, Point, Rect, Settings, Wire, Wires, Work, enters_any, route,
};

use crate::support::{bends, end, free, go, length, obstacle, p, points, request, settings};

fn same(got: &[Point], want: &[Point]) {
    assert_eq!(got.len(), want.len(), "got {got:?}, wanted {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g.x - w.x).abs() < 1e-9 && (g.y - w.y).abs() < 1e-9, "got {got:?}, wanted {want:?}");
    }
}

/// Two boxes 3 apart (A at x 0 to 2, B at x 5 to 7, each 2 tall) and a wall between them, taller than both and reaching farther below than above.
fn the_wall() -> Vec<Obstacle> {
    vec![obstacle(1, 0.0, 0.0, 2.0, 2.0), obstacle(2, 5.0, 0.0, 7.0, 2.0), obstacle(3, 3.0, -2.0, 4.0, 5.0)]
}

#[test]
fn the_wall_is_gone_round_with_four_bends_at_the_clearance() {
    // Catches: a route through the wall, a route that hugs the wall's box (no clearance), a route over the wall (the longer way), or one with more bends.
    let req = request(end(2.0, 1.0, Dir::Right, Some(1)), end(5.0, 1.0, Dir::Left, Some(2)));
    let got = go(the_wall(), &req, &settings(0.5));
    assert!(matches!(got, Outcome::Routed { clearance: Clearance::Full, cut: false, .. }), "{got:?}");
    same(&points(&got), &[p(2.0, 1.0), p(2.5, 1.0), p(2.5, -2.5), p(4.5, -2.5), p(4.5, 1.0), p(5.0, 1.0)]);
    assert_eq!(bends(&points(&got)), 4);
}

#[test]
fn a_route_through_a_shape_that_lets_level_legs_through() {
    // Catches: a pass flag that is ignored (the route goes round a shape it may run through), or one that lets upright legs through too.
    let req = request(free(0.0, 4.0), free(8.0, 4.0));
    let mut through = obstacle(1, 3.0, 3.5, 5.0, 6.0);
    through.pass_level = true;
    same(&points(&go(vec![through], &req, &settings(0.25))), &[p(0.0, 4.0), p(8.0, 4.0)]);
    // A free end may leave any way, so the route turns at once: down, along the box's grown edge, and up.
    let round = points(&go(vec![obstacle(1, 3.0, 3.5, 5.0, 6.0)], &req, &settings(0.25)));
    same(&round, &[p(0.0, 4.0), p(0.0, 3.25), p(8.0, 3.25), p(8.0, 4.0)]);
    assert!((length(&round) - 9.5).abs() < 1e-9, "round the near side, under the box: {round:?}");
}

#[test]
fn an_upright_route_is_not_let_through_by_the_level_flag() {
    // Catches: one pass flag read for both axes.
    let req = request(free(4.0, 0.0), free(4.0, 8.0));
    let mut level = obstacle(1, 3.5, 3.0, 6.0, 5.0);
    level.pass_level = true;
    assert!(points(&go(vec![level], &req, &settings(0.25))).len() > 2);
    let mut upright = obstacle(1, 3.5, 3.0, 6.0, 5.0);
    upright.pass_upright = true;
    same(&points(&go(vec![upright], &req, &settings(0.25))), &[p(4.0, 0.0), p(4.0, 8.0)]);
}

#[test]
fn an_end_walled_in_has_no_way() {
    // A closed frame of four overlapping walls round the begin. Catches: a route that slips through a corner, and `OutOfWork` or a panic for a plain dead end.
    let walls = vec![
        obstacle(1, -2.0, -2.0, -1.0, 2.0),
        obstacle(2, 1.0, -2.0, 2.0, 2.0),
        obstacle(3, -2.0, -2.0, 2.0, -1.0),
        obstacle(4, -2.0, 1.0, 2.0, 2.0),
    ];
    let req = request(free(0.0, 0.0), end(10.0, 0.0, Dir::Left, None));
    assert_eq!(go(walls.clone(), &req, &settings(0.25)), Outcome::NoWay);
    // The same with the end facing a wall that touches it.
    let facing = request(end(-1.0, 0.0, Dir::Left, None), end(10.0, 0.0, Dir::Left, None));
    assert_eq!(go(walls, &facing, &settings(0.0)), Outcome::NoWay);
}

#[test]
fn a_stub_blocked_by_a_near_box_is_shortened() {
    // The begin's stub would be 1 long (the clearance) but the box ahead is grown to within 0.6 of it. Catches: a stub that ignores other boxes (a route that starts inside
    // the clearance), and a stub of the full length that makes the whole request fail.
    let near = obstacle(1, 1.6, -0.5, 3.0, 1.0);
    let req = request(end(0.0, 0.0, Dir::Right, None), end(6.0, 0.0, Dir::Left, None));
    let got = go(vec![near], &req, &settings(1.0));
    assert!(matches!(got, Outcome::Routed { clearance: Clearance::Full, .. }), "{got:?}");
    // The leg that goes back up lies half way between the box's grown edge (4) and the end's stub (5).
    same(&points(&got), &[p(0.0, 0.0), p(0.6, 0.0), p(0.6, -1.5), p(4.5, -1.5), p(4.5, 0.0), p(6.0, 0.0)]);
}

#[test]
fn a_gap_narrower_than_two_clearances_is_passed_at_half() {
    // Two walls with a gap of 1.5 and a clearance of 1: grown, they leave nothing. Catches: no route (NoWay) where half the clearance would pass, and a route at the
    // full clearance through the gap.
    let walls = vec![obstacle(1, -100.0, -100.0, 100.0, 4.0), obstacle(2, -100.0, 5.5, 100.0, 100.0)];
    let req = request(end(-3.0, 4.75, Dir::Right, None), end(13.0, 4.75, Dir::Left, None));
    let got = go(walls, &req, &settings(1.0));
    assert!(matches!(got, Outcome::Routed { clearance: Clearance::Half, .. }), "{got:?}");
    same(&points(&got), &[p(-3.0, 4.75), p(13.0, 4.75)]);
}

#[test]
fn a_gap_narrower_than_the_boxes_themselves_is_no_way() {
    // The last of the three clearances: boxes that overlap leave no gap at any of them.
    let walls = vec![obstacle(1, -100.0, -100.0, 100.0, 5.0), obstacle(2, -100.0, 4.0, 100.0, 100.0)];
    let req = request(end(-3.0, 4.75, Dir::Right, None), end(13.0, 4.75, Dir::Left, None));
    assert_eq!(go(walls, &req, &settings(1.0)), Outcome::NoWay);
}

#[test]
fn the_middle_leg_of_a_z_is_half_way_and_a_u_keeps_the_clearance() {
    // A Z between two boxes: the search puts its middle leg on a grown edge, and centring takes it half way between the stubs' ends. A U's middle leg stays where the
    // clearance puts it, since a slide would make it longer. Catches: a Z that hugs one shape, and a U that is pulled away from them.
    let z = vec![obstacle(1, 0.0, 0.0, 2.0, 2.0), obstacle(2, 8.0, 5.0, 10.0, 7.0)];
    let req = request(end(2.0, 1.0, Dir::Right, Some(1)), end(8.0, 6.0, Dir::Left, Some(2)));
    same(&points(&go(z, &req, &settings(0.5))), &[p(2.0, 1.0), p(5.0, 1.0), p(5.0, 6.0), p(8.0, 6.0)]);
    let u = vec![obstacle(1, 0.0, 0.0, 2.0, 2.0), obstacle(2, 4.0, 0.0, 6.0, 2.0)];
    let req = request(end(1.0, 2.0, Dir::Up, Some(1)), end(5.0, 2.0, Dir::Up, Some(2)));
    same(&points(&go(u, &req, &settings(0.5))), &[p(1.0, 2.0), p(1.0, 2.5), p(5.0, 2.5), p(5.0, 2.0)]);
}

#[test]
fn an_obstacle_that_holds_an_end_is_exempt() {
    // A container holds the begin. Catches: an end that is walled in by its own container, and an exemption that is not read.
    let container = vec![obstacle(1, 0.0, 0.0, 10.0, 10.0)];
    let mut req = request(free(5.0, 5.0), end(15.0, 5.0, Dir::Left, None));
    assert_eq!(go(container.clone(), &req, &settings(0.25)), Outcome::NoWay);
    req.exempt = vec![1];
    same(&points(&go(container, &req, &settings(0.25))), &[p(5.0, 5.0), p(15.0, 5.0)]);
}

#[test]
fn a_route_of_more_than_48_points_is_refused() {
    // Thirty walls, each reaching far to one side, so the route must wind through the gaps at their other ends: sixty bends. Catches: a route with no cap on its points.
    let mut walls = Vec::new();
    for i in 0..30 {
        let (x0, x1) = if i % 2 == 0 { (-1000.0, 18.0) } else { (2.0, 1000.0) };
        let y = f64::from(i) * 2.0 + 1.0;
        walls.push(obstacle(i, x0, y, x1, y + 0.5));
    }
    let req = request(end(10.0, 0.0, Dir::Up, None), end(10.0, 62.0, Dir::Down, None));
    let got = go(walls, &req, &settings(0.25));
    assert_eq!(got, Outcome::Refused("the route has more than 48 points"));
}

#[test]
fn a_few_gaps_are_wound_through() {
    // The same winding with six walls: a route, with the bends the gaps make (two for each wall, and the ends' own).
    let mut walls = Vec::new();
    for i in 0..6 {
        let (x0, x1) = if i % 2 == 0 { (-1000.0, 18.0) } else { (2.0, 1000.0) };
        let y = f64::from(i) * 2.0 + 1.0;
        walls.push(obstacle(i, x0, y, x1, y + 0.5));
    }
    let req = request(end(10.0, 0.0, Dir::Up, None), end(10.0, 14.0, Dir::Down, None));
    let got = points(&go(walls, &req, &settings(0.25)));
    assert!(got.len() > 6 && got.len() <= 48, "{got:?}");
}

#[test]
fn a_leg_that_ends_on_the_clearance_edge_is_allowed_and_a_nudge_inside_is_not() {
    // Catches: boxes read as open or closed the wrong way (a route that cannot run along a grown edge, or one that cuts a corner).
    let boxes = vec![obstacle(1, 0.0, 0.0, 4.0, 4.0)];
    let req = request(free(-2.0, 2.0), free(6.0, 2.0));
    let got = points(&go(boxes, &req, &settings(1.0)));
    assert!(got.iter().all(|q| !(q.x > -1.0 && q.x < 5.0 && q.y > -1.0 && q.y < 5.0)), "{got:?}");
    assert!((length(&got) - 8.0 - 2.0 * 1.0 - 2.0 * 2.0 - 2.0 * 0.0).abs() < 4.0, "{got:?}");
}

#[test]
fn ends_that_are_not_numbers_are_refused_in_words() {
    for bad in [f64::NAN, f64::INFINITY, MAX_COORD + 1.0] {
        let req = request(free(bad, 0.0), free(1.0, 1.0));
        assert_eq!(go(Vec::new(), &req, &settings(0.25)), Outcome::Refused("an end is not a finite point within the limit"), "{bad}");
        let req = request(free(0.0, 0.0), free(1.0, bad));
        assert_eq!(go(Vec::new(), &req, &settings(0.25)), Outcome::Refused("an end is not a finite point within the limit"), "{bad}");
    }
    let one = request(free(1.0, 1.0), free(1.0, 1.0));
    assert_eq!(go(Vec::new(), &one, &settings(0.25)), Outcome::Refused("the two ends are at one point"));
}

#[test]
fn a_route_with_nothing_in_its_way_is_the_straight_one_or_one_bend() {
    let req = request(end(0.0, 0.0, Dir::Right, None), end(10.0, 0.0, Dir::Left, None));
    same(&points(&go(Vec::new(), &req, &settings(0.5))), &[p(0.0, 0.0), p(10.0, 0.0)]);
    let req = request(end(0.0, 0.0, Dir::Right, None), end(10.0, 5.0, Dir::Down, None));
    same(&points(&go(Vec::new(), &req, &settings(0.5))), &[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 5.0)]);
}

#[test]
fn the_stub_of_an_end_is_the_clearance_and_the_turn_comes_after_it() {
    // Catches: a route that turns at the end's own point (no stub), or one that leaves against its way.
    let req = request(end(0.0, 0.0, Dir::Right, None), end(5.0, 5.0, Dir::Left, None));
    let got = points(&go(Vec::new(), &req, &settings(1.0)));
    assert!((got[1].x - got[0].x - 1.0).abs() < 1e-9 || got[1].x - got[0].x >= 1.0 - 1e-9, "{got:?}");
    assert!(got[0].y == got[1].y, "{got:?}");
    let last = got.len() - 1;
    assert!(got[last].y == got[last - 1].y && got[last - 1].x <= got[last].x - 1.0 + 1e-9, "{got:?}");
}

// --- wires -----------------------------------------------------------------------------------------------------------------------------------------------------

type Leg = ((f64, f64), (f64, f64));

fn wire(id: u32, legs: &[Leg], glued: Option<u32>) -> Wire {
    Wire { id, legs: legs.iter().map(|(a, b)| (p(a.0, a.1), p(b.0, b.1))).collect(), glued: [glued, None] }
}

fn go_wired(wires: Vec<Wire>, req: &diacraft_route::Request, settings: &Settings) -> Outcome {
    route(&Obstacles::new(Vec::new()), &Wires::new(wires), req, settings, &mut Work::new(10_000_000))
}

#[test]
fn of_two_routes_of_one_length_the_one_with_fewer_crossings_wins() {
    // From (0, 0) to (10, 10) either L is 20 long with one bend. A wire at x = 5 up to y = 5 is crossed by the one along the bottom. Catches: a crossing that costs nothing.
    let req = request(free(0.0, 0.0), free(10.0, 10.0));
    let mut s = settings(0.0);
    s.cross = 0.25;
    let low = wire(1, &[((5.0, -5.0), (5.0, 5.0))], None);
    same(&points(&go_wired(vec![low], &req, &s)), &[p(0.0, 0.0), p(0.0, 10.0), p(10.0, 10.0)]);
    let high = wire(1, &[((5.0, 5.0), (5.0, 15.0))], None);
    same(&points(&go_wired(vec![high], &req, &s)), &[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)]);
}

#[test]
fn a_route_keeps_its_spacing_from_an_unrelated_wire_and_may_lie_on_a_related_one() {
    // A wire along y = 0.3 and a route along y = 0.5: with a spacing of 1 the route runs 10 units inside it. Catches: the near cost read for related wires, the
    // spacing ignored, and the overlap setting ignored.
    let req = {
        let mut r = request(free(-5.0, 0.5), free(15.0, 0.5));
        r.glued = [Some(7), None];
        r
    };
    let mut s = settings(0.0);
    s.spacing = (1.0, 1.0);
    let unrelated = wire(1, &[((0.0, 0.3), (10.0, 0.3))], None);
    let got = points(&go_wired(vec![unrelated], &req, &s));
    assert!(got.len() > 2, "an unrelated wire is kept apart from: {got:?}");
    let related = wire(1, &[((0.0, 0.3), (10.0, 0.3))], Some(7));
    same(&points(&go_wired(vec![related.clone()], &req, &s)), &[p(-5.0, 0.5), p(15.0, 0.5)]);
    s.overlap = Overlap::Never;
    assert!(points(&go_wired(vec![related], &req, &s)).len() > 2, "no overlap at all keeps apart from related wires too");
}

// --- the obstacles and wires themselves --------------------------------------------------------------------------------------------------------------------------

#[test]
fn boxes_that_are_no_obstacle_are_left_out_and_counted() {
    let list = vec![
        obstacle(1, 0.0, 0.0, 1.0, 1.0),
        obstacle(2, f64::NAN, 0.0, 1.0, 1.0),
        obstacle(3, 0.0, 0.0, f64::INFINITY, 1.0),
        obstacle(4, 0.0, 0.0, MAX_COORD + 1.0, 1.0),
        obstacle(5, 2.0, 2.0, 2.0, 5.0),
        obstacle(6, 3.0, 4.0, 2.0, 1.0),
        obstacle(1, 8.0, 8.0, 9.0, 9.0),
    ];
    let all = Obstacles::new(list);
    assert_eq!(all.len(), 2);
    assert_eq!(all.rejected(), 5);
    // A box given the wrong way round is put right; a repeated id keeps its first box.
    assert_eq!(all.get(6).map(|o| o.rect), Some(Rect { x0: 2.0, y0: 1.0, x1: 3.0, y1: 4.0 }));
    assert_eq!(all.get(1).map(|o| o.rect), Some(Rect { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 }));
}

#[test]
fn a_query_finds_the_boxes_that_meet_its_window_and_replace_moves_them() {
    let mut all = Obstacles::new(vec![obstacle(1, 0.0, 0.0, 1.0, 1.0), obstacle(2, 10.0, 10.0, 11.0, 11.0)]);
    let mut work = Work::new(1000);
    let hit = |all: &Obstacles, w: Rect| all.query(&w, &mut Work::new(1000)).hits.iter().map(|o| o.id).collect::<Vec<_>>();
    let near_first = Rect { x0: 0.5, y0: 0.5, x1: 2.0, y1: 2.0 };
    assert_eq!(hit(&all, near_first), vec![1]);
    assert_eq!(hit(&all, Rect { x0: -1.0, y0: -1.0, x1: 20.0, y1: 20.0 }), vec![1, 2]);
    all.replace(1, Some(obstacle(1, 10.5, 10.5, 12.0, 12.0)));
    assert_eq!(hit(&all, near_first), Vec::<u32>::new());
    assert_eq!(hit(&all, Rect { x0: 11.5, y0: 11.5, x1: 13.0, y1: 13.0 }), vec![1]);
    all.replace(2, None);
    assert_eq!(all.len(), 1);
    let q = all.query(&near_first, &mut work);
    assert!(!q.cut && work.spent() > 0);
}

#[test]
fn a_query_that_visits_too_many_entries_is_cut_and_says_so() {
    // Ten thousand boxes at one spot: one bucket holds them all. Catches: a query whose cost grows with the crowd.
    let crowd: Vec<Obstacle> = (0..10_000).map(|i| obstacle(i, 0.0, 0.0, 1.0, 1.0)).collect();
    let all = Obstacles::new(crowd);
    let mut work = Work::new(1_000_000);
    let q = all.query(&Rect { x0: 0.25, y0: 0.25, x1: 0.75, y1: 0.75 }, &mut work);
    assert!(q.cut);
    assert!(work.spent() as usize <= MAX_QUERY_VISITS + 4, "spent {}", work.spent());
    assert!(q.hits.len() <= MAX_QUERY_VISITS);
}

#[test]
fn a_query_stops_when_the_work_does() {
    let all = Obstacles::new((0..100).map(|i| obstacle(i, 0.0, 0.0, 1.0, 1.0)).collect());
    let mut work = Work::new(10);
    let q = all.query(&Rect { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 }, &mut work);
    assert!(q.cut && work.is_spent() && work.spent() == 10);
}

#[test]
fn wires_are_kept_by_their_level_and_upright_legs_only() {
    let slanted = wire(2, &[((0.0, 0.0), (3.0, 4.0))], None);
    let fine = wire(1, &[((0.0, 0.0), (5.0, 0.0)), ((5.0, 0.0), (5.0, 5.0)), ((f64::NAN, 0.0), (1.0, 0.0))], None);
    let all = Wires::new(vec![fine, slanted]);
    assert_eq!(all.leg_count(), 2);
    assert_eq!(all.skipped_legs(), 2);
    let mut work = Work::new(1000);
    assert_eq!(all.meeting(&Rect { x0: 4.0, y0: -1.0, x1: 6.0, y1: 1.0 }, &mut work), vec![1]);
    let mut all = all;
    all.replace(1, None);
    assert!(all.meeting(&Rect { x0: -9.0, y0: -9.0, x1: 9.0, y1: 9.0 }, &mut work).is_empty());
}

// --- the first try's test ------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn enters_any_says_whether_a_leg_goes_into_a_box() {
    let all = Obstacles::new(vec![obstacle(1, 0.0, 0.0, 4.0, 4.0)]);
    let mut work = Work::new(10_000);
    let mut test = |pts: &[Point], exempt: &[u32]| enters_any(&all, pts, exempt, &mut work);
    assert!(test(&[p(-1.0, 2.0), p(5.0, 2.0)], &[]), "through it");
    assert!(!test(&[p(-1.0, 4.0), p(5.0, 4.0)], &[]), "along its edge");
    assert!(!test(&[p(-1.0, 5.0), p(5.0, 5.0)], &[]), "beside it");
    assert!(!test(&[p(-1.0, 2.0), p(5.0, 2.0)], &[1]), "exempt");
    assert!(test(&[p(2.0, -1.0), p(2.0, 1.0)], &[]), "ending inside");
    assert!(!test(&[p(-1.0, 2.0), p(0.0, 2.0), p(0.0, 9.0)], &[]), "leaving along an edge");
}

#[test]
fn enters_any_lets_a_leg_through_a_pass_flag() {
    let mut o = obstacle(1, 0.0, 0.0, 4.0, 4.0);
    o.pass_level = true;
    let all = Obstacles::new(vec![o]);
    let mut work = Work::new(10_000);
    assert!(!enters_any(&all, &[p(-1.0, 2.0), p(5.0, 2.0)], &[], &mut work));
    assert!(enters_any(&all, &[p(2.0, -1.0), p(2.0, 5.0)], &[], &mut work));
}

#[test]
fn work_that_runs_out_is_said() {
    // A budget of nothing: the router says so and never overspends. Catches: a route made on no work, and a panic on an empty budget.
    let req = request(end(2.0, 1.0, Dir::Right, Some(1)), end(5.0, 1.0, Dir::Left, Some(2)));
    let mut work = Work::new(0);
    let got = route(&Obstacles::new(the_wall()), &Wires::new(Vec::new()), &req, &settings(0.5), &mut work);
    assert_eq!(got, Outcome::OutOfWork);
    assert_eq!(work.spent(), 0);
    let mut small = Work::new(25);
    let got = route(&Obstacles::new(the_wall()), &Wires::new(Vec::new()), &req, &settings(0.5), &mut small);
    assert_eq!(got, Outcome::OutOfWork);
    assert_eq!(small.spent(), 25);
}

#[test]
fn a_lean_never_forbids_a_route() {
    // The begin sits under a lid that is open only below. A lean for the flow upward costs a first leg down: the route found leaves sideways and turns down at once, and
    // the one with no lean leaves down.
    let pocket = vec![obstacle(1, -2.0, 1.0, 2.0, 2.0), obstacle(2, -2.0, -5.0, -1.0, 2.0), obstacle(3, 1.0, -5.0, 2.0, 2.0)];
    let req = request(free(0.0, 0.0), free(0.0, 10.0));
    let plain = points(&go(pocket.clone(), &req, &settings(0.0)));
    let mut s = settings(0.0);
    s.lean = Some((Dir::Up, 3.0));
    let leaning = points(&go(pocket, &req, &s));
    assert_eq!(plain[1], p(0.0, -5.0));
    assert!((leaning[1].y - 0.0).abs() < 1e-9 && leaning.len() > 2, "{leaning:?}");
    assert_eq!((leaning[0], leaning[leaning.len() - 1]), (plain[0], plain[plain.len() - 1]));
}
