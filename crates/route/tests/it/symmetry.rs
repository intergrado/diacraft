//! The search favours no side: a scene mirrored left for right, top for bottom, or turned a quarter turn is routed at the same cost, at the same clearance, and by the same
//! search: the same states expanded, the same states pushed and the same lines made. The bucket visits are left out of the comparison: the buckets are a grid anchored at the
//! origin, so a box that lies on a bucket's edge is in two buckets in one image and in one in another, and a query reads a different count of entries (measured: 180, 190,
//! 171 and 168 visits for the four images of the wall, with 10 states and 21 pushes in each).

use diacraft_route::{Clearance, Dir, DirSet, End, Obstacle, Obstacles, Outcome, Point, Rect, Request, Settings, Wire, Wires, Work, route};
use proptest::prelude::*;

use crate::oracle::{Case, cost_of};
use crate::props::case_strategy;
use crate::support::{end, free, obstacle, p, request, settings};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Turn {
    MirrorX,
    MirrorY,
    Quarter,
}

fn point(t: Turn, q: Point) -> Point {
    match t {
        Turn::MirrorX => Point::new(-q.x, q.y),
        Turn::MirrorY => Point::new(q.x, -q.y),
        Turn::Quarter => Point::new(-q.y, q.x),
    }
}

fn dir(t: Turn, d: Dir) -> Dir {
    match (t, d) {
        (Turn::MirrorX, Dir::Right) => Dir::Left,
        (Turn::MirrorX, Dir::Left) => Dir::Right,
        (Turn::MirrorY, Dir::Up) => Dir::Down,
        (Turn::MirrorY, Dir::Down) => Dir::Up,
        (Turn::Quarter, d) => Dir::from_index(d.index() + 1),
        (_, d) => d,
    }
}

fn ways(t: Turn, w: DirSet) -> DirSet {
    match w.single() {
        Some(d) => DirSet::only(dir(t, d)),
        None => DirSet::ANY,
    }
}

fn end_of(t: Turn, e: &End) -> End {
    End { at: point(t, e.at), ways: ways(t, e.ways), stands_on: e.stands_on }
}

/// The same scene, mirrored or turned.
pub(crate) fn transform(case: &Case, t: Turn) -> Case {
    let swap = |v: (f64, f64)| if matches!(t, Turn::Quarter) { (v.1, v.0) } else { v };
    let obstacles = case
        .obstacles
        .iter()
        .map(|o| {
            let (a, b) = (point(t, Point::new(o.rect.x0, o.rect.y0)), point(t, Point::new(o.rect.x1, o.rect.y1)));
            let (pl, pu) = if matches!(t, Turn::Quarter) { (o.pass_upright, o.pass_level) } else { (o.pass_level, o.pass_upright) };
            Obstacle { id: o.id, rect: Rect::of(a, b), pass_level: pl, pass_upright: pu }
        })
        .collect();
    let wires = case
        .wires
        .iter()
        .map(|w| Wire { id: w.id, legs: w.legs.iter().map(|(a, b)| (point(t, *a), point(t, *b))).collect(), glued: w.glued })
        .collect();
    let settings = Settings {
        clearance: swap(case.settings.clearance),
        spacing: swap(case.settings.spacing),
        lean: case.settings.lean.map(|(d, c)| (dir(t, d), c)),
        ..case.settings
    };
    Case {
        obstacles,
        wires,
        request: Request {
            begin: end_of(t, &case.request.begin),
            end: end_of(t, &case.request.end),
            exempt: case.request.exempt.clone(),
            glued: case.request.glued,
        },
        settings,
    }
}

/// The answer and what the search did: the states expanded, the states pushed and the lines made.
fn run(case: &Case) -> (Outcome, (u64, u64, u64)) {
    let mut work = Work::new(50_000_000);
    let outcome = route(&Obstacles::new(case.obstacles.clone()), &Wires::new(case.wires.clone()), &case.request, &case.settings, &mut work);
    let c = work.charged();
    (outcome, (c.states, c.pushes, c.lines))
}

/// The cost of the route of an outcome, or the outcome's kind.
fn kind(case: &Case, outcome: &Outcome) -> (u8, Option<Clearance>, f64) {
    match outcome {
        Outcome::Routed { points, clearance, .. } => (0, Some(*clearance), cost_of(case, points)),
        Outcome::NoWay => (1, None, 0.0),
        Outcome::OutOfWork => (2, None, 0.0),
        Outcome::Refused(_) => (3, None, 0.0),
    }
}

/// `Err` with the words of what differs between a scene and its image: the answer's kind, the clearance and the cost always; the search's work exactly when `exact_work`
/// (the vectors, whose optimum is not one of several equal ones), and else to within a factor of two and eight steps (a scene with equal routes and equal states: the order
/// the equal states are expanded in can differ between the images by a few, and nothing may grow with the side it is on).
fn same_under(case: &Case, t: Turn, exact_work: bool) -> Result<(), String> {
    let image = transform(case, t);
    let ((a, wa), (b, wb)) = (run(case), run(&image));
    let (ka, kb) = (kind(case, &a), kind(&image, &b));
    if ka.0 != kb.0 || ka.1 != kb.1 || (ka.2 - kb.2).abs() > 1.0e-9 {
        return Err(format!("{t:?}: {a:?} (cost {}) and its image {b:?} (cost {})", ka.2, kb.2));
    }
    let near = |a: u64, b: u64| a.max(b) <= 2 * a.min(b) + 8;
    let ok = if exact_work { wa == wb } else { near(wa.0, wb.0) && near(wa.1, wb.1) && wa.2 == wb.2 };
    if !ok {
        return Err(format!("{t:?}: the search expanded, pushed and made {wa:?} and its image {wb:?}"));
    }
    Ok(())
}

const TURNS: [Turn; 3] = [Turn::MirrorX, Turn::MirrorY, Turn::Quarter];

/// The vectors of `vectors.rs` as cases.
fn vectors() -> Vec<Case> {
    let case = |obstacles: Vec<Obstacle>, begin: End, end: End, clearance: f64| Case {
        obstacles,
        wires: Vec::new(),
        request: request(begin, end),
        settings: settings(clearance),
    };
    let wall = vec![obstacle(1, 0.0, 0.0, 2.0, 2.0), obstacle(2, 5.0, 0.0, 7.0, 2.0), obstacle(3, 3.0, -2.0, 4.0, 5.0)];
    let mut pass = obstacle(1, 3.0, 3.5, 5.0, 6.0);
    pass.pass_level = true;
    vec![
        case(wall, end(2.0, 1.0, Dir::Right, Some(1)), end(5.0, 1.0, Dir::Left, Some(2)), 0.5),
        case(
            vec![obstacle(1, 0.0, 0.0, 2.0, 2.0), obstacle(2, 8.0, 5.0, 10.0, 7.0)],
            end(2.0, 1.0, Dir::Right, Some(1)),
            end(8.0, 6.0, Dir::Left, Some(2)),
            0.5,
        ),
        case(
            vec![obstacle(1, 0.0, 0.0, 2.0, 2.0), obstacle(2, 4.0, 0.0, 6.0, 2.0)],
            end(1.0, 2.0, Dir::Up, Some(1)),
            end(5.0, 2.0, Dir::Up, Some(2)),
            0.5,
        ),
        case(vec![obstacle(1, 1.6, -0.5, 3.0, 1.0)], end(0.0, 0.0, Dir::Right, None), end(6.0, 0.0, Dir::Left, None), 1.0),
        case(
            vec![obstacle(1, -100.0, -100.0, 100.0, 4.0), obstacle(2, -100.0, 5.5, 100.0, 100.0)],
            end(-3.0, 4.75, Dir::Right, None),
            end(13.0, 4.75, Dir::Left, None),
            1.0,
        ),
        case(vec![obstacle(1, 3.0, 3.5, 5.0, 6.0)], free(0.0, 4.0), free(8.0, 4.0), 0.25),
        case(vec![pass], free(0.0, 4.0), free(8.0, 4.0), 0.25),
        case(vec![obstacle(1, 0.0, 0.0, 4.0, 4.0)], free(-2.0, 2.0), free(6.0, 2.0), 1.0),
        case(vec![obstacle(1, 0.0, 0.0, 3.0, 3.0), obstacle(2, 4.0, 1.0, 7.0, 7.0)], free(1.0, -1.0), free(6.0, 8.0), 0.5),
    ]
}

#[test]
fn every_vector_is_routed_the_same_mirrored_and_turned() {
    for (i, case) in vectors().iter().enumerate() {
        for t in TURNS {
            if let Err(words) = same_under(case, t, true) {
                panic!("vector {i}: {words}");
            }
        }
    }
}

#[test]
fn a_route_through_a_pass_flag_turns_with_the_flag() {
    // A turn of a quarter swaps the two axes, so a flag for level legs becomes one for upright legs: the image of the straight route is straight.
    let mut pass = obstacle(1, 3.0, 3.5, 5.0, 6.0);
    pass.pass_level = true;
    let case = Case { obstacles: vec![pass], wires: Vec::new(), request: request(free(0.0, 4.0), free(8.0, 4.0)), settings: settings(0.25) };
    let image = transform(&case, Turn::Quarter);
    let (out, _) = run(&image);
    let Outcome::Routed { points, .. } = out else { panic!("{out:?}") };
    assert_eq!(points, vec![p(-4.0, 0.0), p(-4.0, 8.0)]);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 500, ..ProptestConfig::default() })]

    #[test]
    fn random_scenes_are_routed_the_same_mirrored_and_turned(case in case_strategy()) {
        for t in TURNS {
            if let Err(words) = same_under(&case, t, false) {
                prop_assert!(false, "{}\n{:#?}", words, case);
            }
        }
    }
}
