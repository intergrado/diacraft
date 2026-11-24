//! The window's clock: **the one place the window's own logic reads the time**.
//!
//! The window decides some things by how long ago something happened: a view that has stood still for [`crate::canvas::picture::REST_AFTER`] is drawn again in
//! full, a drawing that failed is not asked for again for a second, the outline of a finished edit is kept for a second and a half while the picture
//! catches up, the progress card counts the seconds a file takes. If those read the machine's clock, **what a test sees depends on how fast the machine is**:
//! on a busy runner the time runs out in the middle of a test's frames and the window does work (and repaints) a quick machine never did, and the test
//! fails at random (GitHub run 42398195820: "Harness::run exceeded max_steps (4). It would have settled after 7 steps").
//!
//! So the window takes a [`Clock`]. The application's is the machine's ([`Clock::real`]). A test's is a [`Clock::manual`]: **it stands still until the test
//! advances it** (`Rest::advance` in the tests' support), so the window's timed work happens when the test says time has passed and at no other moment, and
//! never because the machine was slow. `tests/it/clock.rs` holds the window to it, and scans the sources so that nothing else reads the time.
//!
//! The control channel's quit delay (`app.quit`) and screenshot timeout are under the clock too: each is a reading of the window's clock plus the delay, so a
//! test lets the delay pass by advancing the clock (`Rest::advance`) and asserts the close, or the refusal, by that time and by frames, and never by the
//! machine's time (a test that measured the delay in real time from a moment after the window had started counting it failed on a busy machine).
//!
//! Not under the clock: what is about real time on purpose (the render worker's own limits on its thread, and the timed repaints that wake the window,
//! to look at the clock again or to poll a thread: the tests wait for those threads by what they hold, `Rest::rest`).

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use web_time::Instant;

/// A source of time: how long since it began. Cloned, it is the same clock.
#[derive(Clone, Debug)]
pub struct Clock(Kind);

#[derive(Clone, Debug)]
enum Kind {
    /// The machine's clock, from the moment the clock was made.
    Real(Instant),
    /// How long a test has let pass.
    Manual(Arc<Mutex<Duration>>),
    /// A clock that goes `step` further each time it is read: a test's way of making the machine look slow (frames that each take `step`, so that the
    /// window's timers run out in the middle of a test, whatever machine it runs on).
    Ticking(Arc<Mutex<Duration>>, Duration),
}

impl Default for Clock {
    fn default() -> Clock {
        Clock::real()
    }
}

impl Clock {
    /// The machine's clock.
    pub fn real() -> Clock {
        Clock(Kind::Real(Instant::now()))
    }

    /// A clock that goes `step` further each time it is read, to see what a slow machine does to the window: the window reads it once a frame, so every
    /// frame takes `step` more of the window's time, and a timer of 150 ms runs out after a few frames **on any machine, every time**. For the tests' hook
    /// (`DIACRAFT_TEST_CLOCK=tick:60`), never for the application.
    pub fn ticking(step: Duration) -> Clock {
        Clock(Kind::Ticking(Arc::new(Mutex::new(Duration::ZERO)), step))
    }

    /// A clock that stays where it is until [`Clock::advance`] moves it.
    pub fn manual() -> Clock {
        Clock(Kind::Manual(Arc::new(Mutex::new(Duration::ZERO))))
    }

    /// How long since the clock began.
    pub fn now(&self) -> Duration {
        match &self.0 {
            Kind::Real(began) => began.elapsed(),
            Kind::Ticking(passed, step) => {
                let mut passed = passed.lock().unwrap_or_else(PoisonError::into_inner);
                *passed = passed.saturating_add(*step);
                *passed
            }
            Kind::Manual(passed) => *passed.lock().unwrap_or_else(PoisonError::into_inner),
        }
    }

    /// Lets `by` pass on a manual clock. The machine's clock goes by itself: this does nothing to it, nor to a ticking one.
    pub fn advance(&self, by: Duration) {
        if let Kind::Manual(passed) = &self.0 {
            let mut passed = passed.lock().unwrap_or_else(PoisonError::into_inner);
            *passed = passed.saturating_add(by);
        }
    }

    /// Whether the clock is a test's.
    pub fn is_manual(&self) -> bool {
        matches!(self.0, Kind::Manual(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manual_clock_stands_still_until_it_is_advanced_and_clones_share_it() {
        let clock = Clock::manual();
        let same = clock.clone();
        assert!(clock.is_manual());
        assert_eq!(clock.now(), Duration::ZERO);
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(clock.now(), Duration::ZERO, "real time does not move it");
        same.advance(Duration::from_millis(150));
        assert_eq!(clock.now(), Duration::from_millis(150));
        clock.advance(Duration::MAX);
        clock.advance(Duration::from_secs(1));
        assert!(clock.now() >= Duration::from_secs(1), "an advance past the end is held there and does not wrap");
    }

    #[test]
    fn a_ticking_clock_goes_the_same_step_further_each_time_it_is_read_on_any_machine() {
        let clock = Clock::ticking(Duration::from_millis(60));
        assert!(!clock.is_manual());
        let seen: Vec<Duration> = (0..4).map(|_| clock.now()).collect();
        assert_eq!(seen, [60, 120, 180, 240].map(Duration::from_millis), "a frame is 60 ms, whatever the machine does");
        clock.advance(Duration::from_secs(9));
        assert_eq!(clock.now(), Duration::from_millis(300), "and nothing else moves it");
    }

    #[test]
    fn the_real_clock_goes_by_itself_and_cannot_be_advanced() {
        let clock = Clock::real();
        assert!(!clock.is_manual());
        let before = clock.now();
        clock.advance(Duration::from_secs(3600));
        std::thread::sleep(Duration::from_millis(2));
        assert!(clock.now() > before && clock.now() < Duration::from_secs(60), "an advance does nothing to the machine's clock");
    }
}
