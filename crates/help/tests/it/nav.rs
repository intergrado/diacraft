//! The history of places and the movement between rows (design sections 4.2 and 5.4).

use diacraft_help::MAX_HISTORY;
use diacraft_help::nav::{History, next_row};

#[test]
fn a_new_history_has_no_place_and_nowhere_to_go() {
    let mut h: History<u32> = History::new();
    assert!(h.is_empty() && h.current().is_none());
    assert_eq!(h.len(), 0);
    assert!(!h.can_back() && !h.can_forward());
    assert_eq!(h.back(), None);
    assert_eq!(h.forward(), None);
}

#[test]
fn places_are_walked_back_and_forward() {
    let mut h = History::new();
    for place in [1, 2, 3] {
        assert!(h.push(place));
    }
    assert_eq!((h.len(), h.current().copied()), (3, Some(3)));
    assert_eq!(h.back().copied(), Some(2));
    assert_eq!(h.back().copied(), Some(1));
    assert_eq!(h.back(), None, "the first place has nothing before it");
    assert!(!h.can_back() && h.can_forward());
    assert_eq!(h.current().copied(), Some(1));
    assert_eq!(h.forward().copied(), Some(2));
    assert_eq!(h.forward().copied(), Some(3));
    assert_eq!(h.forward(), None);
    assert!(h.can_back() && !h.can_forward());
}

#[test]
fn back_then_forward_returns_to_the_same_place() {
    let mut h = History::new();
    for place in 0..10 {
        h.push(place);
    }
    let here = h.current().copied();
    h.back();
    h.forward();
    assert_eq!(h.current().copied(), here);
}

#[test]
fn a_new_place_after_back_drops_the_forward_part() {
    let mut h = History::new();
    for place in [1, 2, 3] {
        h.push(place);
    }
    h.back();
    h.back();
    assert!(h.push(9));
    assert_eq!((h.len(), h.current().copied()), (2, Some(9)));
    assert!(!h.can_forward());
    assert_eq!(h.back().copied(), Some(1));
}

#[test]
fn going_to_the_place_already_shown_is_no_step() {
    let mut h = History::new();
    h.push("home");
    assert!(!h.push("home"));
    assert_eq!(h.len(), 1);
    h.push("page");
    assert!(!h.push("page"));
    assert_eq!(h.len(), 2);
    h.back();
    assert!(!h.push("home"), "the place shown after Back is home: no step, and the forward part stays");
    assert!(h.can_forward());
}

#[test]
fn sixty_five_places_keep_sixty_four_and_drop_the_oldest() {
    let mut h = History::new();
    for place in 0..65u32 {
        h.push(place);
    }
    assert_eq!(h.len(), MAX_HISTORY);
    assert_eq!(h.current().copied(), Some(64));
    let mut steps = 0;
    while h.back().is_some() {
        steps += 1;
    }
    assert_eq!(steps, 63);
    assert_eq!(h.current().copied(), Some(1), "place 0 was dropped");
    // pushing after Back at the start still holds the cap
    for place in 100..200u32 {
        h.push(place);
        assert!(h.len() <= MAX_HISTORY);
    }
}

#[test]
fn the_row_reached_stays_inside_the_list_and_up_from_the_first_row_is_the_box() {
    assert_eq!(next_row(None, 5, true), Some(0));
    assert_eq!(next_row(Some(0), 5, true), Some(1));
    assert_eq!(next_row(Some(3), 5, true), Some(4));
    assert_eq!(next_row(Some(4), 5, true), Some(4), "Down from the last row stays");
    assert_eq!(next_row(Some(4), 5, false), Some(3));
    assert_eq!(next_row(Some(1), 5, false), Some(0));
    assert_eq!(next_row(Some(0), 5, false), None, "Up from the first row is the box");
    assert_eq!(next_row(None, 5, false), None);
    assert_eq!(next_row(None, 0, true), None);
    assert_eq!(next_row(Some(0), 0, true), None);
    assert_eq!(next_row(Some(0), 0, false), None);
}

#[test]
fn a_row_that_is_gone_is_read_as_the_last_one() {
    assert_eq!(next_row(Some(9), 3, true), Some(2));
    assert_eq!(next_row(Some(9), 3, false), Some(1));
    assert_eq!(next_row(Some(usize::MAX), 3, true), Some(2));
    assert_eq!(next_row(Some(usize::MAX), 3, false), Some(1));
    assert_eq!(next_row(Some(usize::MAX), usize::MAX, true), Some(usize::MAX - 1));
}

#[test]
fn the_row_reached_is_none_or_below_the_rows_for_every_small_case() {
    for rows in 0..8usize {
        for current in std::iter::once(None).chain((0..12usize).map(Some)) {
            for down in [true, false] {
                if let Some(row) = next_row(current, rows, down) {
                    assert!(row < rows, "rows {rows}, current {current:?}, down {down}: {row}");
                }
            }
        }
    }
}
