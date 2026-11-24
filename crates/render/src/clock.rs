//! The clock a frame's time budgets are spent against.
//!
//! A frame reads the time in a few places: between shapes, against [`Limits::frame_time`](crate::Limits::frame_time); around each layout of
//! text, against [`Limits::text_time`](crate::Limits::text_time) and [`Limits::label_time`](crate::Limits::label_time); and while it decodes the
//! bitmaps of a metafile picture. **Every one of those readings goes through the renderer's [`Clock`]**: the machine's own by default, or one
//! that moves only when it is read, so that a test can say what a frame does when every deadline has passed, whatever the load of the machine.
//! A frame whose limits hold no time ([`Limits::preview`](crate::Limits::preview), [`Limits::unlimited_time`](crate::Limits::unlimited_time))
//! is the same frame at any reading of any clock.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use web_time::Instant;

/// Where a frame reads the time. A copy reads the same time as the clock it was copied from.
#[derive(Clone, Debug)]
pub struct Clock(Kind);

#[derive(Clone, Debug)]
enum Kind {
    /// The machine's monotonic clock, read from the moment the clock was made.
    System(Instant),
    /// A clock that moves only when it is read: each reading is `step` nanoseconds later than the one before it, and it stops at the most a
    /// `u64` of nanoseconds holds (about 584 years) rather than going round.
    Stepping { ticks: Arc<Mutex<u64>>, step: u64 },
}

impl Default for Clock {
    fn default() -> Clock {
        Clock::system()
    }
}

impl Clock {
    /// The machine's clock.
    pub fn system() -> Clock {
        Clock(Kind::System(Instant::now()))
    }

    /// A clock for a test: **each reading is `step` later than the one before**, and nothing reads the machine's time. With a step longer than
    /// any limit, every deadline a frame looks at has passed by the time it looks.
    pub fn stepping(step: Duration) -> Clock {
        let step = u64::try_from(step.as_nanos()).unwrap_or(u64::MAX);
        Clock(Kind::Stepping { ticks: Arc::new(Mutex::new(0)), step })
    }

    /// The time since the clock began.
    pub(crate) fn now(&self) -> Duration {
        match &self.0 {
            Kind::System(origin) => origin.elapsed(),
            Kind::Stepping { ticks, step } => {
                // A test's clock: a lock is cheap beside what a frame does between two readings, and a poisoned one still holds the count.
                let mut ticks = ticks.lock().unwrap_or_else(PoisonError::into_inner);
                *ticks = ticks.saturating_add(*step);
                Duration::from_nanos(*ticks)
            }
        }
    }

    /// The time from `earlier` (a reading of this clock) to now; none for a reading that is not before now.
    pub(crate) fn since(&self, earlier: Duration) -> Duration {
        self.now().saturating_sub(earlier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stepping_clock_moves_by_its_step_at_each_reading_and_stops_at_the_end_of_its_count() {
        let clock = Clock::stepping(Duration::from_secs(3_600));
        let first = clock.now();
        assert_eq!(first, Duration::from_secs(3_600));
        assert_eq!(clock.since(first), Duration::from_secs(3_600), "the next reading is a step later");
        // A copy reads the same clock.
        let copy = clock.clone();
        assert_eq!(copy.now(), Duration::from_secs(3 * 3_600));
        // A step past what the count holds stops at its end, and never goes round to an earlier time.
        let far = Clock::stepping(Duration::MAX);
        assert_eq!(far.now(), Duration::from_nanos(u64::MAX));
        assert_eq!(far.now(), Duration::from_nanos(u64::MAX));
        assert_eq!(far.since(Duration::from_nanos(u64::MAX)), Duration::ZERO);
    }

    #[test]
    fn the_system_clock_does_not_go_back() {
        let clock = Clock::system();
        let first = clock.now();
        assert!(clock.now() >= first);
        assert_eq!(clock.since(Duration::MAX), Duration::ZERO, "a reading after now is no time ago");
    }
}
