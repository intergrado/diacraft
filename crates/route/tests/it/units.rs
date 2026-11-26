//! The small functions the router stands on, each held to the property the proof task will prove: the work budget, the state numbers, the turn cost, the buckets of a
//! coordinate and the least bends two ways force.

use diacraft_route::{DIRS, Dir, MAX_COORD, Work, bucket_of, forced_bends, state_index, turn_cost};

#[test]
fn work_never_passes_its_budget_and_stays_spent() {
    let mut w = Work::new(10);
    assert!(w.charge(4) && w.charge(6));
    assert_eq!((w.spent(), w.left(), w.is_spent()), (10, 0, false));
    assert!(w.charge(0), "nothing is always affordable until a charge is refused");
    assert!(!w.charge(1));
    assert_eq!((w.spent(), w.left(), w.is_spent()), (10, 0, true));
    assert!(!w.charge(0), "once spent, always spent");
    let mut big = Work::new(u64::MAX);
    assert!(big.charge(u64::MAX - 1));
    assert!(!big.charge(u64::MAX), "no overflow");
    assert_eq!(big.spent(), u64::MAX);
    let mut part = Work::new(100);
    assert!(!part.charge(101));
    assert_eq!((part.spent(), part.left()), (100, 0), "a refused charge spends what is left");
}

#[test]
fn state_numbers_are_checked_and_different_states_have_different_numbers() {
    assert_eq!(state_index(0, 0, 0, 1), Some(0));
    assert_eq!(state_index(2, 1, 3, 5), Some(((2 * 5) + 1) * 4 + 3));
    assert_eq!(state_index(0, 5, 0, 5), None, "a line past the last");
    assert_eq!(state_index(0, 0, 4, 5), None, "a way that is not one");
    assert_eq!(state_index(u32::MAX, 0, 0, 2), None, "overflow in the product");
    assert_eq!(state_index(u32::MAX / 4, 0, 0, 1), Some(u32::MAX / 4 * 4));
    assert_eq!(state_index(u32::MAX / 4 + 1, 0, 0, 1), None, "overflow in the last product");
    let mut seen = std::collections::BTreeSet::new();
    for x in 0..7 {
        for y in 0..5 {
            for way in 0..4 {
                assert!(seen.insert(state_index(x, y, way, 5).unwrap()), "{x} {y} {way}");
            }
        }
    }
    assert_eq!(seen.len(), 7 * 5 * 4);
}

#[test]
fn a_turn_costs_nothing_straight_one_for_a_quarter_and_is_never_a_reversal() {
    for a in DIRS {
        for b in DIRS {
            let want = if a == b {
                Some(0)
            } else if a == b.opposite() {
                None
            } else {
                Some(1)
            };
            assert_eq!(turn_cost(a, b), want, "{a:?} to {b:?}");
        }
    }
}

#[test]
fn a_coordinate_goes_to_a_bucket_without_overflow() {
    assert_eq!(bucket_of(0.0, 1.0), 0);
    assert_eq!(bucket_of(-0.5, 1.0), -1);
    assert_eq!(bucket_of(2.0, 2.0), 1);
    assert_eq!(bucket_of(f64::NAN, 1.0), 0);
    assert_eq!(bucket_of(f64::INFINITY, 1.0), bucket_of(MAX_COORD, 1.0));
    assert_eq!(bucket_of(f64::NEG_INFINITY, 1.0), bucket_of(-MAX_COORD, 1.0));
    assert_eq!(bucket_of(1.0e300, 1.0e-6), bucket_of(MAX_COORD, 1.0e-6));
    assert_eq!(bucket_of(1.0, 0.0), 0, "no cell is no bucket");
    assert_eq!(bucket_of(1.0, f64::NAN), 0);
    assert_eq!(bucket_of(1.0, f64::INFINITY), 0);
    assert!(bucket_of(MAX_COORD, 1.0e-6).abs() < i64::MAX / 2);
}

#[test]
fn the_bends_two_ways_force_are_a_lower_bound_that_turning_the_plane_does_not_change() {
    // Straight on to a point ahead: none. Level out, to a point above it, arriving upward: one. Back the way it came, arriving the way it was going: two or more.
    assert_eq!(forced_bends(Dir::Right, Dir::Right, 5.0, 0.0), 0);
    assert_eq!(forced_bends(Dir::Right, Dir::Up, 5.0, 5.0), 1);
    assert_eq!(forced_bends(Dir::Right, Dir::Right, 5.0, 5.0), 2);
    assert_eq!(forced_bends(Dir::Right, Dir::Right, 0.0, 0.0), 0, "already there");
    assert!(forced_bends(Dir::Right, Dir::Right, -5.0, 0.0) >= 2, "behind it, it must turn about");
    assert!(forced_bends(Dir::Right, Dir::Left, 5.0, 0.0) >= 2);
    for h in DIRS {
        for a in DIRS {
            for dx in [-3.0, -0.5, 0.0, 0.5, 3.0] {
                for dy in [-3.0, -0.5, 0.0, 0.5, 3.0] {
                    let turned = forced_bends(Dir::from_index(h.index() + 1), Dir::from_index(a.index() + 1), -dy, dx);
                    assert_eq!(forced_bends(h, a, dx, dy), turned, "{h:?} {a:?} {dx} {dy}: a quarter turn of the plane changes it");
                    let mirrored = forced_bends(
                        Dir::from_index(if h.is_level() { h.index() + 2 } else { h.index() }),
                        Dir::from_index(if a.is_level() { a.index() + 2 } else { a.index() }),
                        -dx,
                        dy,
                    );
                    assert_eq!(forced_bends(h, a, dx, dy), mirrored, "{h:?} {a:?} {dx} {dy}: mirroring changes it");
                    assert!(forced_bends(h, a, dx, dy) <= 5);
                }
            }
        }
    }
}
