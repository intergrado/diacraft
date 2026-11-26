//! Random scenes. Every route found holds the fuzz target's invariants (the law of `fuzz/src/lib.rs`, included below) and **costs what the independent oracle says the
//! cheapest route costs** (`oracle.rs`): a Dijkstra over the whole Hanan grid at the clearance the outcome names, and none at the clearances before it; a scene the router
//! calls `NoWay` has no route at any of the three. Scenes of whole numbers with no clearance are also held to a search over the unit lattice.

use diacraft_route::{
    Clearance, Dir, DirSet, End, Obstacle, Obstacles, Outcome, Overlap, Point, Rect, Request, Separate, Settings, Wire, Wires, Work, route,
};
use proptest::prelude::*;

use crate::oracle::{Case, best, cost_of, lattice};

#[allow(dead_code)]
#[path = "../../fuzz/src/lib.rs"]
mod law;

fn lattice_point() -> impl Strategy<Value = (i32, i32)> {
    (-1..=17, -1..=17)
}

fn dir() -> impl Strategy<Value = Dir> {
    (0usize..4).prop_map(Dir::from_index)
}

fn obstacle_strategy() -> impl Strategy<Value = (i32, i32, i32, i32, bool, bool)> {
    (0..=16, 0..=16, 1..=6, 1..=6, prop::bool::weighted(0.1), prop::bool::weighted(0.1))
}

/// An end: free, one way out, or standing on a side of box `b`.
fn end_strategy(boxes: Vec<Rect>) -> impl Strategy<Value = End> {
    (0u8..4, lattice_point(), dir(), any::<prop::sample::Index>(), 0u8..4, 0u32..=20).prop_map(move |(kind, (x, y), way, which, side, along)| {
        let free = Point::new(f64::from(x), f64::from(y));
        match kind {
            0 => End { at: free, ways: DirSet::ANY, stands_on: None },
            1 => End { at: free, ways: DirSet::only(way), stands_on: None },
            _ if boxes.is_empty() => End { at: free, ways: DirSet::only(way), stands_on: None },
            _ => {
                let i = which.index(boxes.len());
                let r = boxes[i];
                let (w, h) = (r.x1 - r.x0, r.y1 - r.y0);
                let (at, way) = match side {
                    0 => (Point::new(r.x1, r.y0 + f64::from(along % (h as u32 + 1))), Dir::Right),
                    1 => (Point::new(r.x0, r.y0 + f64::from(along % (h as u32 + 1))), Dir::Left),
                    2 => (Point::new(r.x0 + f64::from(along % (w as u32 + 1)), r.y1), Dir::Up),
                    _ => (Point::new(r.x0 + f64::from(along % (w as u32 + 1)), r.y0), Dir::Down),
                };
                let ways = if kind == 2 { DirSet::only(way) } else { DirSet::ANY };
                End { at, ways, stands_on: Some(i as u32) }
            }
        }
    })
}

fn wire_strategy() -> impl Strategy<Value = Wire> {
    (prop::collection::vec((lattice_point(), any::<bool>(), 1i32..=8), 1..=3), prop::option::of(0u32..3)).prop_map(|(legs, glued)| Wire {
        id: 0,
        legs: legs
            .into_iter()
            .map(|((x, y), level, len)| {
                let a = Point::new(f64::from(x), f64::from(y));
                (a, if level { Point::new(a.x + f64::from(len), a.y) } else { Point::new(a.x, a.y + f64::from(len)) })
            })
            .collect(),
        glued: [glued, None],
    })
}

fn settings_strategy() -> impl Strategy<Value = Settings> {
    let clear = prop::sample::select(vec![0.0, 0.5, 1.0]);
    (
        (clear.clone(), clear),
        (prop::sample::select(vec![0.0, 0.5, 1.0]), prop::sample::select(vec![0.0, 0.5, 1.0])),
        prop::sample::select(vec![0.5, 1.0, 2.0]),
        prop::sample::select(vec![0.0, 0.25, 1.0]),
        prop::sample::select(vec![0.0, 1.0]),
        prop::option::weighted(0.25, (dir(), prop::sample::select(vec![1.0, 3.0]))),
        (0u8..3, 0u8..3),
    )
        .prop_map(|(clearance, spacing, bend, cross, near, lean, (sep, ovl))| Settings {
            clearance,
            spacing,
            bend,
            cross,
            near,
            lean,
            separate: [Separate::Unrelated, Separate::All, Separate::None][usize::from(sep)],
            overlap: [Overlap::Related, Overlap::Never, Overlap::Any][usize::from(ovl)],
            margin: 1000.0,
        })
}

pub(crate) fn case_strategy() -> impl Strategy<Value = Case> {
    prop::collection::vec(obstacle_strategy(), 0..=12)
        .prop_flat_map(|raw| {
            let obstacles: Vec<Obstacle> = raw
                .iter()
                .enumerate()
                .map(|(i, (x, y, w, h, pl, pu))| Obstacle {
                    id: i as u32,
                    rect: Rect { x0: f64::from(*x), y0: f64::from(*y), x1: f64::from(x + w), y1: f64::from(y + h) },
                    pass_level: *pl,
                    pass_upright: *pu,
                })
                .collect();
            let rects: Vec<Rect> = obstacles.iter().map(|o| o.rect).collect();
            (
                Just(obstacles),
                prop::collection::vec(wire_strategy(), 0..=3),
                end_strategy(rects.clone()),
                end_strategy(rects),
                settings_strategy(),
                prop::collection::vec(prop::bool::weighted(0.15), 12),
                (prop::option::of(0u32..3), prop::option::of(0u32..3)),
            )
        })
        .prop_map(|(obstacles, mut wires, begin, mut end, settings, exempt, glued)| {
            for (i, w) in wires.iter_mut().enumerate() {
                w.id = i as u32;
            }
            if begin.at == end.at {
                end.at.x += 1.0;
            }
            let exempt = obstacles.iter().filter(|o| exempt.get(o.id as usize) == Some(&true)).map(|o| o.id).collect();
            Case { obstacles, wires, request: Request { begin, end, exempt, glued: [glued.0, glued.1] }, settings }
        })
}

fn scene_of(case: &Case) -> law::Scene {
    law::Scene {
        obstacles: case.obstacles.clone(),
        wires: case.wires.clone(),
        request: case.request.clone(),
        settings: case.settings,
        budget: 50_000_000,
    }
}

/// The router and the oracle on one case: `Err` with the words of what differs.
fn compare(case: &Case) -> Result<(), String> {
    let obstacles = Obstacles::new(case.obstacles.clone());
    let wires = Wires::new(case.wires.clone());
    let mut work = Work::new(50_000_000);
    let outcome = route(&obstacles, &wires, &case.request, &case.settings, &mut work);
    let levels = [Clearance::Full, Clearance::Half, Clearance::None];
    let wants: Vec<Option<f64>> = levels.iter().map(|l| best(case, *l)).collect();
    match outcome {
        Outcome::Routed { points, clearance, .. } => {
            law::invariants(&scene_of(case), &obstacles, &points, clearance)?;
            let at = levels.iter().position(|l| *l == clearance).unwrap_or(0);
            if let Some(i) = wants.iter().take(at).position(Option::is_some) {
                return Err(format!("the oracle finds a route at {:?} that the router passed over for {clearance:?}", levels[i]));
            }
            let (got, want) = (cost_of(case, &points), wants[at]);
            match want {
                Some(w) if (w - got).abs() <= 1.0e-9 => Ok(()),
                _ => Err(format!("the router's route {points:?} costs {got}, the oracle's cheapest at {clearance:?} costs {want:?}")),
            }
        }
        Outcome::NoWay if wants.iter().all(Option::is_none) => Ok(()),
        other => Err(format!("the router says {other:?}, the oracle says {wants:?}")),
    }
}

/// The router and the lattice search, for whole numbers with no clearance and no wires.
fn compare_lattice(case: &Case) -> Result<(), String> {
    let obstacles = Obstacles::new(case.obstacles.clone());
    let outcome = route(&obstacles, &Wires::new(Vec::new()), &case.request, &case.settings, &mut Work::new(50_000_000));
    let want = lattice(case, 25, 3);
    match (outcome, want) {
        (Outcome::Routed { points, .. }, Some(w)) if (cost_of(case, &points) - w).abs() <= 1.0e-9 => Ok(()),
        (Outcome::NoWay, None) => Ok(()),
        (other, want) => Err(format!("the router says {other:?}, the lattice says {want:?}")),
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2_000, ..ProptestConfig::default() })]

    #[test]
    fn a_route_costs_what_the_oracle_says_and_holds_the_invariants(case in case_strategy()) {
        if let Err(words) = compare(&case) {
            prop_assert!(false, "{}\n{:#?}", words, case);
        }
    }

    #[test]
    fn the_cost_equals_the_lattice_searchs_where_the_scene_is_whole_numbers(case in case_strategy()) {
        let mut case = case;
        case.wires.clear();
        case.settings.clearance = (0.0, 0.0);
        case.settings.lean = None;
        // Free ends: a route between ends that face each other the wrong way needs lines the unit lattice has and a grid of the ends and boxes does not.
        for e in [&mut case.request.begin, &mut case.request.end] {
            e.ways = DirSet::ANY;
            e.stands_on = None;
        }
        if let Err(words) = compare_lattice(&case) {
            prop_assert!(false, "{}\n{:#?}", words, case);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 600, ..ProptestConfig::default() })]

    #[test]
    fn bytes_of_any_kind_hold_the_fuzz_targets_law(data in prop::collection::vec(any::<u8>(), 0..400)) {
        if let Err(words) = law::check_scene(&law::scene_from_bytes(&data)) {
            prop_assert!(false, "{}", words);
        }
    }
}

proptest! {
    // The same laws on far more scenes, for the gate's machine: `cargo test -p diacraft-route --test it -- --ignored long`.
    #![proptest_config(ProptestConfig { cases: 60_000, ..ProptestConfig::default() })]

    #[test]
    #[ignore = "long: for the gate's machine"]
    fn long_a_route_costs_what_the_oracle_says_and_holds_the_invariants(case in case_strategy()) {
        if let Err(words) = compare(&case) {
            prop_assert!(false, "{}\n{:#?}", words, case);
        }
    }

    #[test]
    #[ignore = "long: for the gate's machine"]
    fn long_the_cost_equals_the_lattice_searchs(case in case_strategy()) {
        let mut case = case;
        case.wires.clear();
        case.settings.clearance = (0.0, 0.0);
        case.settings.lean = None;
        for e in [&mut case.request.begin, &mut case.request.end] {
            e.ways = DirSet::ANY;
            e.stands_on = None;
        }
        if let Err(words) = compare_lattice(&case) {
            prop_assert!(false, "{}\n{:#?}", words, case);
        }
    }

    #[test]
    #[ignore = "long: for the gate's machine"]
    fn long_bytes_of_any_kind_hold_the_fuzz_targets_law(data in prop::collection::vec(any::<u8>(), 0..600)) {
        if let Err(words) = law::check_scene(&law::scene_from_bytes(&data)) {
            prop_assert!(false, "{}", words);
        }
    }
}
