//! The memory the process holds that a read of a file must leave room for (for the reads in flight).
//!
//! **The budget of a read is the process's, not a session's.** `diacraft_vsdx::Limits::max_memory` is what one read may use at the most, 2.6 GB of the 3.07 GB that an address-space limit
//! of `ulimit -v 3000000` leaves. A drawing that is open, a stencil that is open, and a second drawing read beside them are in one process, whatever session holds each: the window
//! reads a file in a session of its own while its own session holds the drawing, the window draws the tiles of a stencil from a session it keeps for the purpose, and the command
//! line reads a file three times. So **what is held is counted in one place for the process**, and every read is given what is left of the budget.
//!
//! **How to hold.** A holder makes a [`Hold`] for the bytes it keeps, **as long as it keeps them**: the hold adds its bytes to the process's total and gives them back when it is dropped (and
//! on an unwind: a panic in a thread gives them back too). [`Hold::set`] changes the figure when the holder grows or shrinks. The open drawing's file and model ([`crate::file::OpenFile`]),
//! each open stencil ([`crate::stencil::OpenedStencil`]) and a caller that keeps a model beside a session (the command line's round trip) make one; so does the document-and-history count of
//! a session ([`crate::Session`]). [`held`] is the total, [`reserve_read`] what a read takes, and [`crate::Session::held_memory`] what a session says of its own beside the total.
//!
//! **How to read: a reservation.** A read that is running holds the memory it uses before it has a model to count, so **a read takes a [`Reservation`] before it parses** (`reserve_read`): all of
//! what is left of the budget, in one step that no other thread can take a share of, so that two reads that are running at the same moment never have the same room. The reader is given that
//! figure as the memory it may use ([`Reservation::limits`]), and the reservation is a [`Hold`] on the process's total like any other: **a second read that is made while the first is
//! running finds nothing left, waits for it where it may, and is refused in words where it may not or the wait ends** and not aborted (it was given the whole budget too, and two reads of 80,000 shapes were a process of 3.3 GB). The real figure replaces the
//! reservation when the read ends: whoever keeps the model makes its own [`Hold`] first, and **drops the reservation after** (a moment that is counted twice is safe, one that is counted
//! not at all is not), and a read that fails, panics or is cancelled drops it where it stands. A reservation that is taken after the file is loaded, and not before, holds nothing while a disk
//! that does not answer is waited for (a hung read thread would otherwise hold the budget and refuse every file after it).
//!
//! **A read that finds the budget held by a read that is running waits for it** ([`reserve_read_waiting`], up to [`read_wait`]): the window's next Open after a Cancel, while the cancelled thread
//! still parses, is read when that ends, against what it left, and is refused in words only if the two do not fit together. **While a read holds the budget the waiter takes no part of what
//! is free** (a drawing that was closed gives some back, and a read that took it would be given too little): it takes what there is only when no read holds the budget, or when its wait is over.
//! Room that drawings and stencils hold is not waited for.
//!
//! **A read that must not wait is given a slice** ([`reserve_read_brief`]): `stencil.open` runs on the window's own thread, and a wait there would freeze the window. While a read holds the
//! budget it takes [`SLICE_BYTES`] beside it (one slice at a time, outside what the budget counts: it is a part of the 0.47 GB that the budget leaves for what no price counts), which is room
//! for the file, the package and the model of a stencil of some thousands of masters; a stencil past it is refused in words, as it is where no room is left.
//!
//! **A reservation is not in [`held`].** The edit guard of a session takes [`held`] less its own from the room of its model (`Session::edit_if`), and a reservation is all that is left
//! of the budget: it counts for the reads ([`Reservation::beside`], [`read_limits`], the total of the ledger) and not for an edit, whose room is as it was before reads held anything.
//!
//! **What a figure is.** The bytes the reader *charged* for the model (`Stats::memory_bytes`: the price, not the resident memory) and the file's bytes. A holder that cannot say gives what it
//! can: a figure too low lets a read through that may not fit, one too high refuses a file that would.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use web_time::Instant;

/// How long a read that finds the budget held by a read that is running waits for it, unless the caller sets another ([`set_read_wait`]): the longest a parse is let to run
/// (`diacraft_vsdx::timing`'s cases end in seconds), and more than the window's own progress card is willing to show.
pub const READ_WAIT: Duration = Duration::from_secs(30);

/// [`read_wait`], in milliseconds.
static READ_WAIT_MS: AtomicU64 = AtomicU64::new(30_000);

/// How long a read waits for the read that holds the budget: [`READ_WAIT`] unless it was set ([`set_read_wait`]).
#[must_use]
pub fn read_wait() -> Duration {
    Duration::from_millis(READ_WAIT_MS.load(Ordering::Relaxed))
}

/// Sets how long a read waits for the read that holds the budget, for the whole process (a test that holds a reservation on purpose, in the thread that then reads, sets none).
pub fn set_read_wait(wait: Duration) {
    READ_WAIT_MS.store(u64::try_from(wait.as_millis()).unwrap_or(u64::MAX), Ordering::Relaxed);
}

/// How often a reader that waits looks again at the ledger if nothing woke it: a wake that came before the wait began is not lost for longer than this.
const POLL: Duration = Duration::from_millis(20);

/// The room a read that must not wait is given beside a read that holds the budget ([`reserve_read_brief`]): what the reader keeps back from its pool for a list that grows
/// ([`diacraft_vsdx::cost::GROWTH_BYTES`], 64 MiB) and 128 MiB for the file, the package and the model.
pub const SLICE_BYTES: u64 = diacraft_vsdx::cost::GROWTH_BYTES + (128 << 20);

/// A total of held bytes: the process has one ([`PROCESS`]), and a test makes its own so that what it counts is its own.
#[derive(Debug)]
struct Ledger {
    /// Every byte that is held: what the holders of drawings and stencils hold, and what the reads that are running have reserved.
    total: AtomicU64,
    /// The part of `total` that the reads that are running have reserved, the slice of a read that does not wait included.
    reserved: AtomicU64,
    /// The part of `reserved` that is a slice ([`SLICE_BYTES`]): room beside the budget, which no read waits for.
    slice: AtomicU64,
    /// How many readers are waiting for the room a read that is running holds.
    waiting: AtomicU64,
    /// What a waiting reader sleeps on, and what a reservation that is given back wakes ([`Ledger::reserve`]).
    gate: Mutex<()>,
    freed: Condvar,
}

/// Changes `counter` by `change`, with a compare-and-swap loop: the other threads of the process add and take at the same time.
fn update(counter: &AtomicU64, change: impl Fn(u64) -> u64) {
    let mut seen = counter.load(Ordering::Acquire);
    while let Err(now) = counter.compare_exchange_weak(seen, change(seen), Ordering::AcqRel, Ordering::Acquire) {
        seen = now;
    }
}

/// What a hold is in the ledger besides the total.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// The memory a drawing, a stencil or a caller's model holds.
    Held,
    /// The memory a read that is running has reserved for itself.
    Reserved,
    /// The room a read that does not wait was given beside the budget.
    Slice,
}

/// What a reservation took.
struct Taken {
    /// What the process held and had reserved when it was looked at, before this: the figure a refusal is beside.
    beside: u64,
    /// The part of that which reads that held the budget had reserved.
    reads: u64,
    /// What was taken.
    bytes: u64,
    kind: Kind,
}

impl Ledger {
    const fn new() -> Ledger {
        Ledger {
            total: AtomicU64::new(0),
            reserved: AtomicU64::new(0),
            slice: AtomicU64::new(0),
            waiting: AtomicU64::new(0),
            gate: Mutex::new(()),
            freed: Condvar::new(),
        }
    }

    fn total(&self) -> u64 {
        self.total.load(Ordering::Acquire)
    }

    fn reserved(&self) -> u64 {
        self.reserved.load(Ordering::Acquire)
    }

    fn waiting(&self) -> u64 {
        self.waiting.load(Ordering::Acquire)
    }

    /// What the holders of drawings, stencils and models hold: the total less what the reads that are running have reserved.
    fn held(&self) -> u64 {
        self.total().saturating_sub(self.reserved())
    }

    /// What the reads that are running hold **of the budget**: what is reserved, less the slices (which are beside it).
    fn holding(&self) -> u64 {
        self.reserved().saturating_sub(self.slice.load(Ordering::Acquire))
    }

    /// The gate, locked. **Every change of the reserved part is made under it, with the total's**, so that a reader that looks at the two under it sees them as one moment: a total
    /// that has the room of a read that is running and a reserved part that has not yet is not seen, and a reader is not told that no read is running when one is. (Nothing that runs with the
    /// gate held can panic, and a gate that was poisoned all the same still guards the same figures.)
    fn locked(&self) -> MutexGuard<'_, ()> {
        self.gate.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Adds `n` to the total for a hold of a drawing, a stencil or a model, and never wraps.
    fn add(&self, n: u64) {
        if n > 0 {
            update(&self.total, |total| total.saturating_add(n));
        }
    }

    /// Takes `n` from the total (and from the reserved part, for a reservation), and never goes under nothing. A reservation of the budget that is given back wakes the readers that wait for it.
    fn take(&self, kind: Kind, n: u64) {
        if n == 0 {
            return;
        }
        match kind {
            Kind::Held => update(&self.total, |total| total.saturating_sub(n)),
            Kind::Reserved | Kind::Slice => {
                {
                    let _gate = self.locked();
                    update(&self.total, |total| total.saturating_sub(n));
                    update(&self.reserved, |reserved| reserved.saturating_sub(n));
                    if kind == Kind::Slice {
                        update(&self.slice, |slice| slice.saturating_sub(n));
                    }
                }
                if kind == Kind::Reserved {
                    self.freed.notify_all();
                }
            }
        }
    }

    /// Takes all that is left of `budget` for a read, **in one step**: what was held (the figure the reservation is beside), what the reads that held the budget had reserved, and what is taken.
    /// What is held when this is asked and what is taken are one figure, so that two threads never take the same room. The gate is held (the guard is the proof): the drawings' holds change the
    /// total without it, and the compare-and-swap is what is exact against them.
    fn take_the_rest(&self, _gate: &MutexGuard<'_, ()>, budget: u64) -> Taken {
        let reads = self.holding();
        let mut seen = self.total.load(Ordering::Acquire);
        loop {
            let left = budget.saturating_sub(seen);
            match self.total.compare_exchange_weak(seen, seen.saturating_add(left), Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => {
                    update(&self.reserved, |reserved| reserved.saturating_add(left));
                    return Taken { beside: seen, reads, bytes: left, kind: Kind::Reserved };
                }
                Err(now) => seen = now,
            }
        }
    }

    /// Takes a slice of `slice` bytes beside the budget, if no slice is out: the gate is held.
    fn take_a_slice(&self, _gate: &MutexGuard<'_, ()>, slice: u64) -> Option<Taken> {
        if slice == 0 || self.slice.load(Ordering::Acquire) > 0 {
            return None;
        }
        let reads = self.holding();
        let beside = self.total();
        update(&self.total, |total| total.saturating_add(slice));
        update(&self.reserved, |reserved| reserved.saturating_add(slice));
        update(&self.slice, |out| out.saturating_add(slice));
        Some(Taken { beside, reads, bytes: slice, kind: Kind::Slice })
    }

    /// Reserves the room for a read of a file out of `budget`.
    ///
    /// **A read that finds the budget held by a read that is running waits for it, up to `wait`**: the read that ends gives its reservation back and wakes the readers that wait, and the
    /// first to look takes all that is left. **While a read holds the budget it takes no part of what is free** (a drawing that is closed gives some back, and a reader that took it would be
    /// refused for want of room that is not the room it was to have). Room that drawings and stencils hold is not waited for (nothing a read does gives it back). When the wait is over, or
    /// was none, it takes what there is, which may be nothing: **and a read that is given a `slice` and finds the budget held takes the slice instead, when what is free is less**.
    /// The room and the reads that hold it are looked at under the gate, and the sleep gives the gate up in the same step, so that a read that ends is never missed.
    fn reserve(&self, budget: u64, wait: Duration, slice: u64) -> Taken {
        // A wait too long to add to the clock is no wait.
        let deadline = Instant::now().checked_add(wait);
        let mut gate = self.locked();
        let mut counted = false;
        while self.holding() > 0 {
            let Some(remaining) = deadline.and_then(|d| d.checked_duration_since(Instant::now())).filter(|d| !d.is_zero()) else { break };
            if !counted {
                self.waiting.fetch_add(1, Ordering::AcqRel);
                counted = true;
            }
            // Woken, spuriously or at the end of the wait, the room is looked at again at the top.
            gate = match self.freed.wait_timeout(gate, remaining.min(POLL)) {
                Ok((gate, _)) => gate,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        let held_by_reads = self.holding() > 0;
        let free = budget.saturating_sub(self.total());
        let taken = match self.take_a_slice(&gate, if held_by_reads && free < slice { slice } else { 0 }) {
            Some(taken) => taken,
            None => self.take_the_rest(&gate, budget),
        };
        drop(gate);
        if counted {
            self.waiting.fetch_sub(1, Ordering::AcqRel);
        }
        taken
    }
}

/// What every [`Hold`] of the process holds, summed.
static PROCESS: Ledger = Ledger::new();

/// The bytes the process holds that a read leaves room for: every [`Hold`] of a drawing, a stencil or a model that is alive, summed. **The reservations of the reads that are running are
/// not in it** ([`reserved`]): what a session's edit may leave of its model is the room the rest of the process leaves it, and a read that is running holds all that is left of the budget,
/// so that counting it there would take from an edit all its room, and an edit made while a cancelled Open still parses would let go of every undo step it kept.
#[must_use]
pub fn held() -> u64 {
    PROCESS.held()
}

/// How many reads are waiting for the room a read that is running holds ([`reserve_read_waiting`]).
#[must_use]
pub fn waiting_reads() -> u64 {
    PROCESS.waiting()
}

/// What the reads that are running have reserved ([`reserve_read`]) and the holds of drawings and stencils do not include: nothing when no file is being read. A read is given the budget less
/// [`held`] **and** this.
#[must_use]
pub fn reserved() -> u64 {
    PROCESS.reserved()
}

/// The limits of a read made now: the reader's own, with the memory it may use [`diacraft_vsdx::MAX_MEMORY`] less what the process holds and what the reads that are running have reserved.
/// **A read made with these holds nothing while it runs**, so another read that is running at the same moment is given the same room: a read takes a [`reserve_read`] and is given its
/// [`Reservation::limits`].
#[must_use]
pub fn read_limits() -> diacraft_vsdx::Limits {
    limits_beside(PROCESS.total())
}

/// The limits of a read made beside `held` bytes: the reader's own, with `max_memory` what is left of the budget (nothing, when `held` is all of it or more).
#[must_use]
pub fn limits_beside(held: u64) -> diacraft_vsdx::Limits {
    let defaults = diacraft_vsdx::Limits::default();
    diacraft_vsdx::Limits { max_memory: defaults.max_memory.saturating_sub(held), ..defaults }
}

/// Bytes held for a read to leave room for, for as long as the `Hold` lives. See the [module](self).
///
/// **One hold changes by one thread at a time** ([`Hold::set`]): its figure and the total move together. (A hold that was changed from two threads took its new figure and then moved the total by
/// the difference, and a thread that took before the other had added was cut at nothing: what the other added was left in the total for good.)
#[derive(Debug)]
pub struct Hold {
    ledger: &'static Ledger,
    kind: Kind,
    /// What this hold has put in the ledger; the lock is held while the ledger is changed by the difference.
    bytes: Mutex<u64>,
}

/// The figure of a hold, locked. Nothing that runs with the lock held can panic, and a lock that was poisoned all the same still holds the figure.
fn locked(bytes: &Mutex<u64>) -> MutexGuard<'_, u64> {
    bytes.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Hold {
    /// A hold of `bytes`: the process's total is `bytes` more until it is dropped.
    #[must_use]
    pub fn new(bytes: u64) -> Hold {
        Hold::on(&PROCESS, bytes)
    }

    fn on(ledger: &'static Ledger, bytes: u64) -> Hold {
        ledger.add(bytes);
        Hold { ledger, kind: Kind::Held, bytes: Mutex::new(bytes) }
    }

    /// What this hold holds.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        *locked(&self.bytes)
    }

    /// Makes this hold `bytes`: the process's total moves by the difference.
    pub fn set(&self, bytes: u64) {
        let mut own = locked(&self.bytes);
        let before = std::mem::replace(&mut *own, bytes);
        if bytes >= before {
            self.ledger.add(bytes - before);
        } else {
            self.ledger.take(self.kind, before - bytes);
        }
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        let own = match self.bytes.get_mut() {
            Ok(bytes) => std::mem::take(bytes),
            Err(poisoned) => std::mem::take(poisoned.into_inner()),
        };
        self.ledger.take(self.kind, own);
    }
}

/// What a read that is running has reserved of the budget: all that was left of it when the read began, held until the reservation is dropped. See the [module](self).
#[derive(Debug)]
pub struct Reservation {
    hold: Hold,
    /// What the process held and had reserved when the reservation was made (the reservations of other reads included), before this one.
    beside: u64,
    /// The part of that which reads that were running had reserved.
    beside_reads: u64,
}

/// Reserves what is left of the budget for a read that is about to parse a file ([`Reservation`]), **without waiting**. Nothing is left when the process holds the budget or more, or when
/// another read has reserved it: the reservation is then of nothing, and the reader refuses the file in words.
#[must_use]
pub fn reserve_read() -> Reservation {
    Reservation::waiting(&PROCESS, diacraft_vsdx::MAX_MEMORY, Duration::ZERO)
}

/// [`reserve_read`] that **waits up to `wait` for a read that is running to end** when it holds the room: the read that is made after it (the window's next Open after a Cancel, while the
/// cancelled thread still parses) is then read against what the first leaves, as the real holds say, and is refused in words if the two do not fit together. It takes no part of what is free
/// while a read holds the budget.
#[must_use]
pub fn reserve_read_waiting(wait: Duration) -> Reservation {
    Reservation::waiting(&PROCESS, diacraft_vsdx::MAX_MEMORY, wait)
}

/// [`reserve_read`] for a read that **must not wait** (a stencil, read on the window's own thread): where a read holds the budget and less than a slice is free, it is given a slice
/// ([`SLICE_BYTES`]) beside the budget, one at a time, so that it is read beside the read that runs when it is small, as it was before reads held the budget.
#[must_use]
pub fn reserve_read_brief() -> Reservation {
    Reservation::brief(&PROCESS, diacraft_vsdx::MAX_MEMORY)
}

impl Reservation {
    fn taken(ledger: &'static Ledger, taken: Taken) -> Reservation {
        Reservation { hold: Hold { ledger, kind: taken.kind, bytes: Mutex::new(taken.bytes) }, beside: taken.beside, beside_reads: taken.reads }
    }

    /// A reservation that does not wait, on a ledger of a test's own.
    #[cfg(test)]
    fn on(ledger: &'static Ledger, budget: u64) -> Reservation {
        Reservation::waiting(ledger, budget, Duration::ZERO)
    }

    fn waiting(ledger: &'static Ledger, budget: u64, wait: Duration) -> Reservation {
        Reservation::taken(ledger, ledger.reserve(budget, wait, 0))
    }

    fn brief(ledger: &'static Ledger, budget: u64) -> Reservation {
        Reservation::taken(ledger, ledger.reserve(budget, Duration::ZERO, SLICE_BYTES))
    }

    /// What was reserved: the memory the read may use, the file it holds included.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.hold.bytes()
    }

    /// What the process held, beside this reservation, when it was made.
    #[must_use]
    pub fn beside(&self) -> u64 {
        self.beside
    }

    /// The part of [`Reservation::beside`] that reads which were running had reserved: a refusal says that a file is being read, and to wait for it.
    #[must_use]
    pub fn beside_reads(&self) -> u64 {
        self.beside_reads
    }

    /// The limits of the read: the reader's own, with the memory it may use what was reserved.
    #[must_use]
    pub fn limits(&self) -> diacraft_vsdx::Limits {
        diacraft_vsdx::Limits { max_memory: self.bytes(), ..diacraft_vsdx::Limits::default() }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    // Each test counts on a ledger of its own: the process's is shared with every other test of the binary, which holds and releases at the same time.

    #[test]
    fn a_hold_adds_its_bytes_and_gives_them_back_when_it_is_dropped() {
        static LEDGER: Ledger = Ledger::new();
        assert_eq!(LEDGER.total(), 0);
        let one = Hold::on(&LEDGER, 1_000);
        let two = Hold::on(&LEDGER, 250);
        assert_eq!((one.bytes(), two.bytes(), LEDGER.total()), (1_000, 250, 1_250));
        drop(one);
        assert_eq!(LEDGER.total(), 250);
        drop(two);
        assert_eq!(LEDGER.total(), 0);
        // A hold of nothing is nothing.
        drop(Hold::on(&LEDGER, 0));
        assert_eq!(LEDGER.total(), 0);
    }

    #[test]
    fn set_moves_the_total_by_the_difference_up_and_down_and_a_drop_gives_back_the_last_figure() {
        static LEDGER: Ledger = Ledger::new();
        let other = Hold::on(&LEDGER, 100);
        let hold = Hold::on(&LEDGER, 5_000);
        hold.set(8_000);
        assert_eq!((hold.bytes(), LEDGER.total()), (8_000, 8_100));
        hold.set(2_000);
        assert_eq!((hold.bytes(), LEDGER.total()), (2_000, 2_100));
        hold.set(2_000);
        assert_eq!(LEDGER.total(), 2_100, "the same figure changes nothing");
        hold.set(0);
        assert_eq!(LEDGER.total(), 100);
        hold.set(7);
        drop(hold);
        assert_eq!(LEDGER.total(), 100, "a drop gives back the last figure");
        drop(other);
        assert_eq!(LEDGER.total(), 0);
    }

    #[test]
    fn a_hold_in_a_thread_that_panics_is_given_back() {
        static LEDGER: Ledger = Ledger::new();
        let outcome = std::thread::spawn(|| {
            let _hold = Hold::on(&LEDGER, 4_321);
            assert_eq!(LEDGER.total(), 4_321);
            panic!("a thread that ends in a panic while it holds");
        })
        .join();
        assert!(outcome.is_err(), "the thread panicked");
        assert_eq!(LEDGER.total(), 0, "the unwind dropped the hold");
    }

    #[test]
    fn holds_made_and_dropped_in_many_threads_leave_nothing() {
        static LEDGER: Ledger = Ledger::new();
        let threads: Vec<_> = (1..=8u64)
            .map(|n| {
                std::thread::spawn(move || {
                    for k in 0..2_000u64 {
                        let hold = Hold::on(&LEDGER, n);
                        hold.set(n + k % 7);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(LEDGER.total(), 0);
    }

    #[test]
    fn one_hold_set_from_two_threads_at_once_leaves_the_total_at_its_own_figure() {
        // Two threads change one hold (a figure that goes up and down with nothing else held): the total must be the hold's own figure when they are done. The hold took its new figure
        // and then moved the total, so a thread that took before the other had added was cut at nothing, and the total was left with what the other added for good.
        static LEDGER: Ledger = Ledger::new();
        let hold = Hold::on(&LEDGER, 0);
        std::thread::scope(|scope| {
            scope.spawn(|| (0..1_500_000u64).for_each(|k| hold.set(1_000_000 + k % 3)));
            scope.spawn(|| (0..1_500_000u64).for_each(|_| hold.set(0)));
        });
        assert_eq!(LEDGER.total(), hold.bytes(), "the total is the figure of the one hold that is in it");
        drop(hold);
        assert_eq!(LEDGER.total(), 0, "and a drop gives back all of it");
    }

    #[test]
    fn the_total_neither_wraps_nor_goes_under_nothing() {
        static LEDGER: Ledger = Ledger::new();
        let a = Hold::on(&LEDGER, u64::MAX - 5);
        let b = Hold::on(&LEDGER, 100);
        assert_eq!(LEDGER.total(), u64::MAX, "it stops at the most");
        drop(b);
        drop(a);
        assert_eq!(LEDGER.total(), 0, "and a total that was cut at the most is never given back more than it has");
    }

    #[test]
    fn limits_beside_what_is_held_are_the_budget_less_it_and_never_under_nothing() {
        let budget = diacraft_vsdx::MAX_MEMORY;
        assert_eq!(limits_beside(0).max_memory, budget);
        assert_eq!(limits_beside(1_234).max_memory, budget - 1_234);
        assert_eq!(limits_beside(budget).max_memory, 0);
        assert_eq!(limits_beside(u64::MAX).max_memory, 0);
        // Every other limit is the reader's own.
        assert_eq!(limits_beside(1_000).max_items, diacraft_vsdx::Limits::default().max_items);
    }

    // ---------------------------------------------------------------------------------------------------------------------------------- the reservation of a read in flight

    #[test]
    fn a_reservation_takes_what_is_left_of_the_budget_and_a_second_finds_nothing_until_the_first_is_dropped() {
        static LEDGER: Ledger = Ledger::new();
        let drawing = Hold::on(&LEDGER, 1_000);
        let first = Reservation::on(&LEDGER, 10_000);
        assert_eq!((first.bytes(), first.beside(), first.beside_reads()), (9_000, 1_000, 0));
        assert_eq!(LEDGER.held(), 1_000, "what is held is what the drawings hold, and the reservation is not in it");
        assert_eq!(
            (LEDGER.total(), LEDGER.reserved()),
            (10_000, 9_000),
            "the reservation is in the total, and is told apart from what a drawing holds"
        );
        // A read that is running at the same moment is given nothing, and says that a read is running.
        let second = Reservation::on(&LEDGER, 10_000);
        assert_eq!((second.bytes(), second.beside(), second.beside_reads()), (0, 10_000, 9_000));
        assert_eq!((LEDGER.total(), LEDGER.reserved()), (10_000, 9_000), "a reservation of nothing changes nothing");
        drop(second);
        assert_eq!((LEDGER.total(), LEDGER.reserved()), (10_000, 9_000), "and gives back nothing");
        drop(first);
        assert_eq!((LEDGER.total(), LEDGER.reserved()), (1_000, 0), "the first gives back all it took, and the drawing is still held");
        let third = Reservation::on(&LEDGER, 10_000);
        assert_eq!(third.bytes(), 9_000, "what was given back can be taken again");
        drop(third);
        drop(drawing);
        assert_eq!((LEDGER.total(), LEDGER.reserved()), (0, 0));
    }

    #[test]
    fn a_reservation_beside_the_whole_budget_or_more_is_of_nothing_and_nothing_wraps() {
        static LEDGER: Ledger = Ledger::new();
        let drawing = Hold::on(&LEDGER, 20_000);
        let none = Reservation::on(&LEDGER, 10_000);
        assert_eq!((none.bytes(), none.beside()), (0, 20_000));
        assert_eq!(LEDGER.total(), 20_000, "nothing was taken from a total that is over the budget");
        assert_eq!(none.limits().max_memory, 0);
        drop((none, drawing));
        assert_eq!(LEDGER.total(), 0);
    }

    #[test]
    fn the_limits_of_a_reservation_are_the_readers_own_with_the_memory_it_took() {
        static LEDGER: Ledger = Ledger::new();
        let _drawing = Hold::on(&LEDGER, 1_234_567);
        let reservation = Reservation::on(&LEDGER, diacraft_vsdx::MAX_MEMORY);
        let limits = reservation.limits();
        assert_eq!(limits.max_memory, diacraft_vsdx::MAX_MEMORY - 1_234_567);
        assert_eq!(limits.max_memory, reservation.bytes());
        let own = diacraft_vsdx::Limits::default();
        assert_eq!((limits.max_items, limits.max_xml_bytes), (own.max_items, own.max_xml_bytes), "every other limit is the reader's own");
    }

    #[test]
    fn a_reservation_in_a_thread_that_panics_or_that_is_cancelled_is_given_back() {
        static LEDGER: Ledger = Ledger::new();
        let outcome = std::thread::spawn(|| {
            let reservation = Reservation::on(&LEDGER, 7_000);
            assert_eq!((reservation.bytes(), LEDGER.reserved()), (7_000, 7_000));
            panic!("a read that ends in a panic while it holds its reservation");
        })
        .join();
        assert!(outcome.is_err(), "the thread panicked");
        assert_eq!((LEDGER.total(), LEDGER.reserved()), (0, 0), "the unwind dropped the reservation");
        // A read that is cancelled still runs to its end (a thread cannot be stopped); the reservation goes with it, whether or not anyone waits for the answer.
        let (done, wait) = std::sync::mpsc::channel::<()>();
        let reader = std::thread::spawn(move || {
            let _reservation = Reservation::on(&LEDGER, 7_000);
            let _ = wait.recv();
        });
        while LEDGER.reserved() == 0 {
            std::thread::yield_now();
        }
        assert_eq!(LEDGER.total(), 7_000, "while it runs it holds");
        drop(done);
        reader.join().unwrap();
        assert_eq!((LEDGER.total(), LEDGER.reserved()), (0, 0), "and when it ends it gives it back");
    }

    #[test]
    fn a_reservation_that_waits_is_given_the_room_when_the_read_that_holds_it_ends() {
        static LEDGER: Ledger = Ledger::new();
        let running = Reservation::on(&LEDGER, 10_000);
        assert_eq!(running.bytes(), 10_000);
        let waiter = std::thread::spawn(|| Reservation::waiting(&LEDGER, 10_000, std::time::Duration::from_secs(600)));
        // The reader is in the wait before the read that holds the room ends (the waiter is counted from the moment it waits).
        while LEDGER.waiting() == 0 {
            std::thread::yield_now();
        }
        assert_eq!(LEDGER.reserved(), 10_000, "the read that runs still holds, and the waiter has taken nothing");
        drop(running);
        let got = waiter.join().unwrap();
        assert_eq!(got.bytes(), 10_000, "the room the read held is the waiter's when it ends");
        assert_eq!(LEDGER.waiting(), 0, "and nobody waits");
        drop(got);
        assert_eq!((LEDGER.total(), LEDGER.reserved()), (0, 0));
    }

    #[test]
    fn a_reservation_that_waits_gives_up_when_the_wait_ends_and_the_room_is_still_held() {
        static LEDGER: Ledger = Ledger::new();
        let running = Reservation::on(&LEDGER, 10_000);
        let none = Reservation::waiting(&LEDGER, 10_000, std::time::Duration::from_millis(30));
        assert_eq!((none.bytes(), none.beside(), none.beside_reads()), (0, 10_000, 10_000), "nothing was taken, and it says a read holds the room");
        assert_eq!(LEDGER.waiting(), 0);
        drop((none, running));
        assert_eq!(LEDGER.total(), 0);
    }

    #[test]
    fn a_reservation_does_not_wait_for_room_that_drawings_hold_and_no_read_gives_back() {
        static LEDGER: Ledger = Ledger::new();
        let drawing = Hold::on(&LEDGER, 20_000);
        // A wait of ten minutes that is not cut short would stay in this test until the runner ends it: no read holds, so there is nothing to wait for.
        let none = Reservation::waiting(&LEDGER, 10_000, std::time::Duration::from_secs(600));
        assert_eq!((none.bytes(), none.beside(), none.beside_reads()), (0, 20_000, 0));
        drop((none, drawing));
        assert_eq!(LEDGER.total(), 0);
    }

    #[test]
    fn readers_that_wait_for_each_other_never_share_the_room_and_every_one_is_given_it_in_turn() {
        static LEDGER: Ledger = Ledger::new();
        static LIVE: AtomicU64 = AtomicU64::new(0);
        const BUDGET: u64 = 1_000;
        let threads: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    let mut turns = 0u32;
                    for _ in 0..200 {
                        let reservation = Reservation::waiting(&LEDGER, BUDGET, std::time::Duration::from_secs(600));
                        let bytes = reservation.bytes();
                        assert_eq!(bytes, BUDGET, "with no drawing held, each is given all of it in its turn");
                        let live = LIVE.fetch_add(bytes, Ordering::SeqCst) + bytes;
                        assert!(live <= BUDGET, "the reservations that are alive together are {live} of a budget of {BUDGET}");
                        LIVE.fetch_sub(bytes, Ordering::SeqCst);
                        turns += 1;
                    }
                    turns
                })
            })
            .collect();
        let turns: u32 = threads.into_iter().map(|t| t.join().unwrap()).sum();
        assert_eq!(turns, 8 * 200);
        assert_eq!((LEDGER.total(), LEDGER.reserved(), LEDGER.waiting()), (0, 0, 0));
    }

    #[test]
    fn a_waiter_takes_no_part_of_the_room_a_drawing_gives_back_while_a_read_still_holds_the_budget() {
        // A drawing that is closed while a read waits gives part of the budget back, and the read that holds the rest has not ended. A waiter that took that part
        // was given too little, and was refused at once where main opens the file.
        static LEDGER: Ledger = Ledger::new();
        let drawing = Hold::on(&LEDGER, 4_000);
        let running = Reservation::on(&LEDGER, 10_000);
        assert_eq!(running.bytes(), 6_000);
        let waiter = std::thread::spawn(|| Reservation::waiting(&LEDGER, 10_000, std::time::Duration::from_secs(600)));
        while LEDGER.waiting() == 0 {
            std::thread::yield_now();
        }
        drop(drawing);
        // The waiter is given every chance to take what is free (it looks again every `POLL`, and one that took it would have stopped waiting).
        let at = Instant::now();
        while LEDGER.waiting() == 1 && at.elapsed() < Duration::from_millis(300) {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(LEDGER.waiting(), 1, "it is waiting still");
        assert_eq!((LEDGER.total(), LEDGER.reserved()), (6_000, 6_000), "and has taken nothing of the 4,000 that are free");
        drop(running);
        let got = waiter.join().unwrap();
        assert_eq!((got.bytes(), got.beside(), got.beside_reads()), (10_000, 0, 0), "it is given all of it when the read that held it has ended");
        assert_eq!(LEDGER.waiting(), 0);
    }

    #[test]
    fn a_waiter_whose_wait_is_over_takes_what_there_is_and_says_a_read_holds_the_rest() {
        static LEDGER: Ledger = Ledger::new();
        let drawing = Hold::on(&LEDGER, 4_000);
        let running = Reservation::on(&LEDGER, 10_000);
        drop(drawing);
        let got = Reservation::waiting(&LEDGER, 10_000, Duration::from_millis(30));
        assert_eq!((got.bytes(), got.beside(), got.beside_reads()), (4_000, 6_000, 6_000));
        drop((got, running));
        assert_eq!((LEDGER.total(), LEDGER.reserved(), LEDGER.waiting()), (0, 0, 0));
    }

    #[test]
    fn a_brief_reservation_is_given_a_slice_beside_a_read_that_holds_the_budget_one_at_a_time_and_no_read_waits_for_a_slice() {
        static LEDGER: Ledger = Ledger::new();
        // A budget of ten slices, and a drawing of one.
        const SLICE: u64 = SLICE_BYTES;
        let (budget, drawing) = (10 * SLICE, Hold::on(&LEDGER, SLICE));
        // Nothing holds the budget: a brief read is given what is left, as any read is, and no slice.
        let alone = Reservation::brief(&LEDGER, budget);
        assert_eq!((alone.bytes(), alone.beside_reads()), (9 * SLICE, 0));
        drop(alone);
        let running = Reservation::on(&LEDGER, budget);
        assert_eq!(running.bytes(), 9 * SLICE);
        // A read holds it all: a brief read is given a slice, beside the budget, and a second finds none while the first is out.
        let first = Reservation::brief(&LEDGER, budget);
        assert_eq!((first.bytes(), first.beside(), first.beside_reads()), (SLICE, budget, 9 * SLICE));
        assert_eq!(
            (LEDGER.held(), LEDGER.holding(), LEDGER.reserved()),
            (SLICE, 9 * SLICE, 10 * SLICE),
            "the slice is reserved, and is not held and is not of the budget"
        );
        let second = Reservation::brief(&LEDGER, budget);
        assert_eq!(second.bytes(), 0);
        assert_eq!(LEDGER.reserved(), 10 * SLICE, "a second slice is not taken");
        // A read that waits does not wait for a slice: with the read that held the budget gone, it is given all that is left at once (ten minutes would be a test that did not end).
        drop((second, running));
        let waiter = Reservation::waiting(&LEDGER, budget, Duration::from_secs(600));
        assert_eq!(waiter.bytes(), 8 * SLICE, "it did not wait for the slice that is out, and is given what is left beside it and the drawing");
        drop((waiter, first));
        assert_eq!(
            (LEDGER.total(), LEDGER.reserved(), LEDGER.holding()),
            (SLICE, 0, 0),
            "and the slice that is given back leaves the figures as they were"
        );
        // The slice is taken again once it has been given back.
        let running = Reservation::on(&LEDGER, budget);
        let again = Reservation::brief(&LEDGER, budget);
        assert_eq!(again.bytes(), SLICE);
        drop((again, running, drawing));
        assert_eq!((LEDGER.total(), LEDGER.reserved()), (0, 0));
    }

    #[test]
    fn a_brief_reservation_is_given_no_slice_when_drawings_hold_the_budget_and_no_read_runs() {
        // A slice is room beside a read that holds the budget. Drawings that hold the budget, and no read running, leave nothing for a read and no slice: the slice would be room beside
        // nothing, on top of a budget that is held (the review, W2).
        static LEDGER: Ledger = Ledger::new();
        const BUDGET: u64 = 10 * SLICE_BYTES;
        let drawings = Hold::on(&LEDGER, BUDGET);
        let brief = Reservation::brief(&LEDGER, BUDGET);
        assert_eq!((brief.bytes(), brief.beside(), brief.beside_reads()), (0, BUDGET, 0));
        assert_eq!(
            (LEDGER.total(), LEDGER.reserved(), LEDGER.slice.load(Ordering::Acquire)),
            (BUDGET, 0, 0),
            "nothing was taken, and no slice is out"
        );
        let past = Hold::on(&LEDGER, 3 * SLICE_BYTES);
        let brief_past = Reservation::brief(&LEDGER, BUDGET);
        assert_eq!(brief_past.bytes(), 0, "drawings past the budget leave nothing either");
        drop((brief, brief_past, past, drawings));
        assert_eq!(LEDGER.total(), 0);
    }

    #[test]
    fn a_brief_reservation_is_given_what_is_free_and_not_a_slice_when_that_is_a_slice_or_more() {
        static LEDGER: Ledger = Ledger::new();
        let drawing = Hold::on(&LEDGER, 5 * SLICE_BYTES);
        let running = Reservation::on(&LEDGER, 10 * SLICE_BYTES);
        assert_eq!(running.bytes(), 5 * SLICE_BYTES);
        drop(drawing);
        let brief = Reservation::brief(&LEDGER, 10 * SLICE_BYTES);
        assert_eq!((brief.bytes(), brief.beside_reads()), (5 * SLICE_BYTES, 5 * SLICE_BYTES), "what is free, which is more than a slice");
        assert_eq!(LEDGER.slice.load(Ordering::Acquire), 0, "and no slice is out");
        drop((brief, running));
        assert_eq!(LEDGER.total(), 0);
    }

    #[test]
    fn the_wait_a_read_is_given_is_the_one_that_was_set_and_is_thirty_seconds_until_then() {
        assert_eq!(read_wait(), READ_WAIT);
        assert_eq!(READ_WAIT, Duration::from_secs(30));
        set_read_wait(Duration::from_millis(1_234));
        assert_eq!(read_wait(), Duration::from_millis(1_234));
        set_read_wait(Duration::ZERO);
        assert_eq!(read_wait(), Duration::ZERO);
        set_read_wait(READ_WAIT);
        assert_eq!(read_wait(), READ_WAIT);
    }

    #[test]
    fn the_process_figures_follow_the_reads_that_run_the_reads_that_wait_and_the_holds() {
        // The process's own ledger (nextest gives this test a process: nothing else holds in it).
        assert_eq!((held(), reserved(), waiting_reads()), (0, 0, 0));
        let drawing = Hold::new(5_000);
        assert_eq!((held(), reserved()), (5_000, 0));
        assert_eq!(read_limits().max_memory, diacraft_vsdx::MAX_MEMORY - 5_000, "a read made now is given the budget less what is held");
        let running = reserve_read();
        assert_eq!(running.bytes(), diacraft_vsdx::MAX_MEMORY - 5_000);
        assert_eq!((held(), reserved()), (5_000, running.bytes()), "the reservation is not held, and is reserved");
        assert_eq!(read_limits().max_memory, 0, "and a read made now is given nothing");
        let waiter = std::thread::spawn(|| reserve_read_waiting(Duration::from_secs(600)));
        while waiting_reads() == 0 {
            std::thread::yield_now();
        }
        assert_eq!(waiting_reads(), 1);
        drop(running);
        let got = waiter.join().unwrap();
        assert_eq!((got.bytes(), waiting_reads(), reserved()), (diacraft_vsdx::MAX_MEMORY - 5_000, 0, got.bytes()));
        drop((got, drawing));
        assert_eq!((held(), reserved(), waiting_reads()), (0, 0, 0));
    }

    #[test]
    fn reservations_made_in_many_threads_never_share_the_same_room() {
        static LEDGER: Ledger = Ledger::new();
        static LIVE: AtomicU64 = AtomicU64::new(0);
        const BUDGET: u64 = 1_000;
        let threads: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    let mut took = 0u64;
                    for _ in 0..20_000 {
                        let reservation = Reservation::on(&LEDGER, BUDGET);
                        let bytes = reservation.bytes();
                        took += bytes;
                        let live = LIVE.fetch_add(bytes, Ordering::SeqCst) + bytes;
                        assert!(live <= BUDGET, "the reservations that are alive together are {live} of a budget of {BUDGET}");
                        assert!(LEDGER.total() <= BUDGET, "the total is never past the budget when only reads hold");
                        LIVE.fetch_sub(bytes, Ordering::SeqCst);
                    }
                    took
                })
            })
            .collect();
        let took: u64 = threads.into_iter().map(|t| t.join().unwrap()).sum();
        assert!(took > 0, "some read was given room");
        assert_eq!((LEDGER.total(), LEDGER.reserved(), LIVE.load(Ordering::SeqCst)), (0, 0, 0));
    }
}
