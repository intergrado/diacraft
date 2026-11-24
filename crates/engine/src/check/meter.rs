//! The steps a comparison takes, counted in one place and charged **before the work is done** (the security scan's findings on a hostile file).
//!
//! The checker reads back a file that may be hostile, so every loop it makes is either linear in what it walks or paid for first. A step is a unit of what is walked (a shape, a cell, a row, a node
//! compared); a scan of a list for each item of another list is `len * len` steps, charged before the first comparison. A comparison that is past its budget does not go on: the functions that decide
//! whether a thing is kept answer "kept" (never a difference that was not found), and [`Meter::exhausted`] says the walk is to stop with [`Stop`].

use std::cell::Cell;

/// The comparison is past what it may take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stop;

/// The steps taken so far and the most the comparison may take.
#[derive(Debug)]
pub(crate) struct Meter {
    steps: Cell<u64>,
    max: u64,
}

impl Meter {
    /// A meter with nothing taken and `max` to take.
    pub(crate) fn new(max: u64) -> Meter {
        Meter { steps: Cell::new(0), max }
    }

    /// Takes `n` steps. → whether the comparison is still within its budget: **a caller that is not may not do the work it was charging for**.
    pub(crate) fn charge(&self, n: usize) -> bool {
        let taken = self.steps.get().saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
        self.steps.set(taken);
        taken <= self.max
    }

    /// Whether the steps taken are past the budget.
    pub(crate) fn exhausted(&self) -> bool {
        self.steps.get() > self.max
    }

    /// The steps taken.
    #[cfg(test)]
    pub(crate) fn steps(&self) -> u64 {
        self.steps.get()
    }

    /// Ends the walk with [`Stop`] when the budget is gone.
    pub(crate) fn check(&self) -> Result<(), Stop> {
        if self.exhausted() { Err(Stop) } else { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_charge_past_the_budget_is_refused_and_the_walk_stops() {
        let meter = Meter::new(10);
        assert!(meter.charge(4) && meter.charge(6));
        assert_eq!((meter.steps(), meter.exhausted(), meter.check()), (10, false, Ok(())));
        assert!(!meter.charge(1));
        assert_eq!((meter.exhausted(), meter.check()), (true, Err(Stop)));
        assert!(!meter.charge(usize::MAX), "a charge that overflows is past the budget, and does not wrap");
        assert_eq!(meter.steps(), u64::MAX);
    }
}
