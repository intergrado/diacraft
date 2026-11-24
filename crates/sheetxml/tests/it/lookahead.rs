//! **What a reader sets aside for work ahead of the read is never charged, is never paid out for memory that is still held, and never makes a refusal** (`SheetReader::set_aside`): the
//! figures of the budget are the ones a reader that sets nothing aside has, a charge that the rest of the budget cannot pay ends the work ahead first (the work lets go of what it holds)
//! and only then takes the set-aside back, and a charge that nothing can pay is refused with the numbers it is refused with when nothing is set aside.

use std::sync::{Arc, Mutex};

use diacraft_sheetxml::{Caps, LookaheadHook, SheetReader};

fn reader(bytes: u64) -> SheetReader {
    SheetReader::new(Caps { cells: 1_000, items: 1_000, formula_nodes: 1_000 }).with_bytes(bytes)
}

/// A work ahead that says what it was asked, in order: `drain`, and each change of what is set aside.
#[derive(Default)]
struct Log(Mutex<Vec<(&'static str, u64)>>);

impl LookaheadHook for Log {
    fn drain(&self) {
        self.0.lock().unwrap().push(("drain", 0));
    }

    fn changed(&self, set_aside: u64) {
        self.0.lock().unwrap().push(("changed", set_aside));
    }
}

#[test]
fn what_is_set_aside_is_not_what_the_budget_can_pay_and_is_what_a_read_with_none_has() {
    let mut r = reader(1_000);
    assert!(r.set_aside(400));
    assert_eq!(
        (r.bytes_left(), r.bytes_left_settled(), r.lookahead()),
        (600, 1_000, 400),
        "paid out is only what is not set aside; the settled figure is the one with no work ahead"
    );
    assert_eq!(r.stats().bytes, 0, "nothing is charged");
    assert!(!r.set_aside(700), "not more than the budget can pay beyond what is set aside");
    assert!(r.set_aside(600));
    assert_eq!((r.bytes_left(), r.lookahead()), (0, 1_000));
    r.set_aside_back(250);
    assert_eq!((r.bytes_left(), r.bytes_left_settled(), r.lookahead()), (250, 1_000, 750));
    r.set_aside_back(10_000);
    assert_eq!((r.bytes_left(), r.lookahead()), (1_000, 0), "no more is given back than was set aside");
}

#[test]
fn a_charge_the_rest_of_the_budget_cannot_pay_ends_the_work_ahead_before_it_takes_the_set_aside() {
    let mut r = reader(1_000);
    let log = Arc::new(Log::default());
    r.set_lookahead_hook(log.clone());
    assert!(r.set_aside(700));
    r.take_exact(200).unwrap();
    assert!(!r.lookahead_taken_back(), "a charge that the rest of the budget pays leaves what is set aside");
    assert_eq!(r.lookahead(), 700);
    log.0.lock().unwrap().clear();
    r.take_exact(500).unwrap();
    assert!(r.lookahead_taken_back(), "this one needed it");
    assert!(!r.lookahead_taken_back(), "and the flag is read once");
    assert_eq!((r.lookahead(), r.bytes_left(), r.stats().bytes), (0, 300, 700), "the charge is what it is without the work ahead");
    // **The work lets go of what it holds before the bytes are paid out**: `drain` comes before the change to nothing set aside, never after it.
    assert_eq!(*log.0.lock().unwrap(), vec![("drain", 0), ("changed", 0)]);
}

#[test]
fn a_refusal_is_the_same_with_and_without_what_is_set_aside() {
    for aside in [0u64, 300, 900, 1_000] {
        let mut with = reader(1_000);
        let mut without = reader(1_000);
        assert!(with.set_aside(aside), "set aside {aside}");
        with.take_exact(100).unwrap();
        without.take_exact(100).unwrap();
        assert_eq!(with.take_exact(1_200), without.take_exact(1_200), "set aside {aside}: the same refusal, with the same numbers");
        assert_eq!(with.bytes_left_settled(), without.bytes_left_settled());
        assert_eq!(with.stats(), without.stats());
    }
}

#[test]
fn bytes_given_back_later_are_a_charge_less_for_the_report_and_stay_set_aside_until_they_are_given_back() {
    let mut r = reader(1_000);
    r.take_exact(600).unwrap();
    // A tree of 400 bytes that another thread is dropping: the report counts 200 charged, and the budget pays out nothing for the 400 until the thread has dropped it.
    r.give_bytes_later(400);
    assert_eq!((r.stats().bytes, r.bytes_left(), r.lookahead(), r.bytes_left_settled()), (200, 400, 400, 800));
    r.set_aside_back(400);
    assert_eq!((r.stats().bytes, r.bytes_left(), r.lookahead()), (200, 800, 0));
}

#[test]
fn a_reader_with_no_byte_budget_sets_nothing_aside() {
    let mut r = SheetReader::new(Caps { cells: 10, items: 10, formula_nodes: 10 });
    assert!(r.set_aside(u64::MAX));
    assert_eq!((r.lookahead(), r.bytes_left(), r.bytes_left_settled()), (0, u64::MAX, u64::MAX));
}
