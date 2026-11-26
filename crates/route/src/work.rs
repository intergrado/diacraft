//! The work budget: a count of the steps a search takes, charged by every loop, shared by every search of an edit.

/// What a step was: the units the loops walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// A state taken off the heap and expanded.
    State,
    /// A state pushed on the heap.
    Push,
    /// A bucket entry, a big box or a bucket looked at by a query.
    Visit,
    /// A grid line made, sorted or merged, and a point of a route handled by centring or tidying.
    Line,
}

/// A count for each kind of step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub states: u64,
    pub pushes: u64,
    pub visits: u64,
    pub lines: u64,
}

impl Counts {
    /// All the steps of all the kinds, saturating.
    pub fn total(&self) -> u64 {
        self.states.saturating_add(self.pushes).saturating_add(self.visits).saturating_add(self.lines)
    }

    fn add(&mut self, step: Step, n: u64) {
        let slot = match step {
            Step::State => &mut self.states,
            Step::Push => &mut self.pushes,
            Step::Visit => &mut self.visits,
            Step::Line => &mut self.lines,
        };
        *slot = slot.saturating_add(n);
    }
}

/// A budget of steps. Every loop of the router charges it **before** it runs (the whole of what it is about to walk), and a charge that would pass the budget is
/// refused: the work then stays spent, and every later charge is refused too. The sum charged never passes the budget given.
#[derive(Clone, Debug)]
pub struct Work {
    budget: u64,
    spent: u64,
    over: bool,
    charged: Counts,
    walked: Counts,
}

impl Work {
    /// A budget of `budget` steps.
    pub fn new(budget: u64) -> Work {
        Work { budget, spent: 0, over: false, charged: Counts::default(), walked: Counts::default() }
    }

    /// Charges `n` steps. `false` when they would pass the budget (the work is then spent, and stays spent) or the work was spent already.
    pub fn charge(&mut self, n: u64) -> bool {
        self.charge_for(Step::Visit, n)
    }

    /// [`Work::charge`], saying what the steps were.
    pub fn charge_for(&mut self, step: Step, n: u64) -> bool {
        if self.over {
            return false;
        }
        match self.spent.checked_add(n) {
            Some(total) if total <= self.budget => {
                self.spent = total;
                self.charged.add(step, n);
                true
            }
            _ => {
                self.spent = self.budget;
                self.over = true;
                false
            }
        }
    }

    /// The steps charged so far, never more than the budget.
    pub fn spent(&self) -> u64 {
        self.spent
    }

    /// The steps still to charge: 0 once the work is spent.
    pub fn left(&self) -> u64 {
        if self.over { 0 } else { self.budget.saturating_sub(self.spent) }
    }

    /// Whether a charge was refused.
    pub fn is_spent(&self) -> bool {
        self.over
    }

    /// The steps charged, by kind.
    pub fn charged(&self) -> Counts {
        self.charged
    }

    /// What the loops themselves counted as they walked (an independent count beside the charges: a test holds the two equal, so that a loop that walks more than it
    /// charges is found).
    pub fn walked(&self) -> Counts {
        self.walked
    }

    /// A loop's own count of one kind of step.
    pub(crate) fn walk(&mut self, step: Step, n: u64) {
        self.walked.add(step, n);
    }

    /// Spends what is left: the work is spent from now on.
    pub(crate) fn exhaust(&mut self) {
        self.spent = self.budget;
        self.over = true;
    }

    /// Takes in the work of a search run on a budget of its own: its charges are charged here, kind by kind.
    pub(crate) fn absorb(&mut self, part: &Work) -> bool {
        let ok = self.charge_for(Step::State, part.charged.states)
            && self.charge_for(Step::Push, part.charged.pushes)
            && self.charge_for(Step::Visit, part.charged.visits)
            && self.charge_for(Step::Line, part.charged.lines);
        self.walked.states = self.walked.states.saturating_add(part.walked.states);
        self.walked.pushes = self.walked.pushes.saturating_add(part.walked.pushes);
        self.walked.visits = self.walked.visits.saturating_add(part.walked.visits);
        self.walked.lines = self.walked.lines.saturating_add(part.walked.lines);
        ok
    }
}
