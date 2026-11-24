//! Threads for work that is made in order and used in order: how many a piece of work may use ([`default_threads`]), and a [`Lane`], which runs tasks on workers and gives
//! their results back **in the order the tasks were given**, whatever order they finished in.
//!
//! **What a lane promises**, each of which a test reads back:
//!
//! * **The results come in the order of the tasks**, so a caller that writes each result as it comes writes the same bytes at one worker as at thirty: the work of a task runs
//!   anywhere, and its place in the output is fixed by its place in the order.
//! * **A task that panics is an error and not a panic** ([`Failed`]): the worker catches it, the lane goes on, and the caller says in its own words what failed. A panic that crosses
//!   a joined thread is not an error a caller can handle, and the web build aborts on any panic, so nothing here relies on one.
//! * **The caller keeps the budget**: a task is given a *charge* (the bytes it holds while it is pending), the lane adds the charges of the tasks not yet taken
//!   ([`Lane::charged`]), and the caller, which knows its own budget, takes results (which gives their charge back) before it gives more work. The lane never waits for a charge
//!   itself, so no budget can make it wait on itself.
//! * **No thread is made that is not needed**: a worker is started when a task is given and more tasks wait than there are workers free to take them (never more than the count
//!   asked for). A worker is free from when it is started until it takes a task, and again in the hold of the lock that makes its result seen, so **the workers are never more than
//!   the most tasks that were in flight at once** (given and not yet made), whatever order the system runs the threads in (a worker that had made a result and not yet
//!   gone back to wait, or one the system had not yet started, was counted busy, and a caller that took one task at a time had three to eight workers under load). So a save of three
//!   parts makes three threads, not thirty-one (each thread of a process costs its stack and its allocator's arena, address space that a process under a limit does not have to
//!   spare). A thread that the system will not make is not an error: the task runs where it was given, on the caller's thread.
//! * **On the web nothing is spawned** (`target_arch = "wasm32"`): every task runs where it was given. So does a lane of no workers, which is the single-threaded path.

use std::collections::{BTreeMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{Builder, Scope};

/// The most threads a count is held to.
pub const MAX_THREADS: usize = 64;

/// The name of the variable that says how many threads work may use: for tests and measurement, `1` being the single-threaded path.
pub const THREADS_VARIABLE: &str = "DIACRAFT_THREADS";

/// How many threads a piece of work may use when its caller has no count of its own: **`DIACRAFT_THREADS`** when it holds a whole number (held to 1 to [`MAX_THREADS`]), else the
/// cores the machine has less one (a core is left to the window), and at least one. On the web it is always 1.
pub fn default_threads() -> usize {
    if cfg!(target_arch = "wasm32") {
        return 1;
    }
    from_setting(std::env::var(THREADS_VARIABLE).ok().as_deref(), std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get))
}

/// The count that a setting of [`THREADS_VARIABLE`] and `cores` give: the setting when it is a whole number, held to 1 to [`MAX_THREADS`], else the cores less one. The renderer's count of
/// the threads of a frame is this one (`diacraft_render::threads_from`), as a save's and an open's are.
pub fn from_setting(setting: Option<&str>, cores: usize) -> usize {
    match setting.and_then(|text| text.trim().parse::<usize>().ok()) {
        Some(n) => n.clamp(1, MAX_THREADS),
        None => cores.saturating_sub(1).clamp(1, MAX_THREADS),
    }
}

/// The address-space limit of this process, in bytes: `None` when it has none, which is when a thread costs no more than its stack, or when the system does not say (only Linux does, in `/proc/self/limits`, and
/// only there is the arena of a thread a reserve of address space). Read once for a process (a frame asks for it every time it is drawn), as the renderer read it.
pub fn address_space_limit() -> Option<u64> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    static LIMIT: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *LIMIT.get_or_init(|| std::fs::read_to_string("/proc/self/limits").ok().and_then(|text| limit_in(&text)))
}

/// The soft limit of address space that the text of `/proc/self/limits` says, in bytes: `None` for `unlimited`.
fn limit_in(limits: &str) -> Option<u64> {
    let line = limits.lines().find(|line| line.starts_with("Max address space"))?;
    let soft = line.trim_start_matches("Max address space").split_whitespace().next()?;
    soft.parse::<u64>().ok()
}

/// A task of a lane that did not give a result: it panicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failed;

type Task<'scope, T> = Box<dyn FnOnce() -> T + Send + 'scope>;

struct State<'scope, T> {
    /// Tasks given and not yet taken by a worker, with the number each was given.
    queue: VecDeque<(u64, Task<'scope, T>)>,
    /// Results made and not yet taken by the caller.
    done: BTreeMap<u64, Result<T, Failed>>,
    /// The charge of each task given and not yet taken by the caller, in the order given.
    charges: VecDeque<usize>,
    /// The number the next task is given, and the number of the next result the caller takes.
    given: u64,
    taken: u64,
    /// The sum of `charges`.
    charged: usize,
    /// Workers started, and workers that hold no task ([`Workers`]).
    workers: Workers,
    /// The system would not make another thread.
    no_more_threads: bool,
    /// The lane is closed: workers go when the queue is empty.
    closing: bool,
}

/// The workers of a lane, as counts that each step changes in one hold of the lock (the harness of `kani_proofs.rs` runs every order of the steps on them): **workers
/// started, and workers that hold no task** (started and not yet looking for one, waiting for one, or with a result made and not yet looking again). A worker is busy only from when
/// it takes a task to when its result is made, so the tasks waiting that the free workers do not cover are the ones that need a new worker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Workers {
    pub(crate) started: usize,
    pub(crate) free: usize,
}

impl Workers {
    /// Whether a task given when `queued` tasks wait (it among them) needs a worker started: more wait than the free workers take, and the count allows one more than `started`.
    pub(crate) fn wants_one(self, queued: usize, most: usize) -> bool {
        queued > self.free && self.started < most
    }

    /// A worker was started: free until it takes a task, which it looks for when it starts.
    pub(crate) fn started_one(&mut self) {
        self.started = self.started.saturating_add(1);
        self.free = self.free.saturating_add(1);
    }

    /// A worker took a task: busy until its result is made.
    pub(crate) fn took(&mut self) {
        self.free = self.free.saturating_sub(1);
    }

    /// A worker made its result: free again, in the hold of the lock that makes the result seen.
    pub(crate) fn made(&mut self) {
        self.free = self.free.saturating_add(1);
    }

    /// A free worker went, the lane closing and no task waiting.
    pub(crate) fn went(&mut self) {
        self.free = self.free.saturating_sub(1);
    }
}

struct Shared<'scope, T> {
    state: Mutex<State<'scope, T>>,
    /// Workers wait here for a task.
    work: Condvar,
    /// The caller waits here for a result.
    ready: Condvar,
}

impl<'scope, T> Shared<'scope, T> {
    fn lock(&self) -> MutexGuard<'_, State<'scope, T>> {
        // A worker never holds the lock while a task runs, so a poisoned lock is one whose holder was stopped between two plain assignments: the state is whole.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Runs the tasks given to it on worker threads and gives their results back in order. See the [module documentation](self); made by [`lane`].
pub struct Lane<'scope, 'env, T> {
    shared: Arc<Shared<'scope, T>>,
    /// The scope the workers are started in, or none for a lane that runs its tasks where they are given.
    scope: Option<&'scope Scope<'scope, 'env>>,
    max_workers: usize,
    starter: Starter,
}

/// What a worker does as it starts, before it looks for a task: nothing, except in a test, which holds a worker back there as a loaded system is slow to start a thread (given
/// the worker's place in the order they were started, 0 for the first).
#[derive(Clone, Default)]
struct Starter(#[cfg(test)] Option<Arc<dyn Fn(usize) + Send + Sync>>);

impl Starter {
    #[cfg(test)]
    fn start(&self, place: usize) {
        if let Some(hold) = &self.0 {
            hold(place);
        }
    }

    #[cfg(not(test))]
    fn start(&self, _place: usize) {}
}

/// Runs `body` with a lane of at most `workers` threads. The threads are joined before this returns: a task that was given and not taken is run to its end, or dropped unrun when
/// it was still waiting for a worker and `body` is done (the caller took the results it wanted, or failed).
pub fn lane<'env, T, R>(workers: usize, body: impl for<'scope> FnOnce(&Lane<'scope, 'env, T>) -> R) -> R
where
    T: Send + 'env,
{
    lane_with(workers, Starter::default(), body)
}

/// [`lane`], with what each worker does as it starts.
fn lane_with<'env, T, R>(workers: usize, starter: Starter, body: impl for<'scope> FnOnce(&Lane<'scope, 'env, T>) -> R) -> R
where
    T: Send + 'env,
{
    let workers = if cfg!(target_arch = "wasm32") { 0 } else { workers.min(MAX_THREADS) };
    if workers == 0 {
        return body(&Lane { shared: Arc::new(Shared::new()), scope: None, max_workers: 0, starter });
    }
    std::thread::scope(|scope| {
        let lane = Lane { shared: Arc::new(Shared::new()), scope: Some(scope), max_workers: workers, starter };
        // Whatever ends `body` (it returns, or it unwinds), the workers are told to go, so that the scope can join them.
        let _closer = Closer(Arc::clone(&lane.shared));
        body(&lane)
    })
}

impl<T> Shared<'_, T> {
    fn new() -> Self {
        Shared {
            state: Mutex::new(State {
                queue: VecDeque::new(),
                done: BTreeMap::new(),
                charges: VecDeque::new(),
                given: 0,
                taken: 0,
                charged: 0,
                workers: Workers::default(),
                no_more_threads: false,
                closing: false,
            }),
            work: Condvar::new(),
            ready: Condvar::new(),
        }
    }
}

/// Closes the lane when it is dropped: the tasks still waiting for a worker are dropped, and the workers go.
struct Closer<'scope, T>(Arc<Shared<'scope, T>>);

impl<T> Drop for Closer<'_, T> {
    fn drop(&mut self) {
        let mut state = self.0.lock();
        state.closing = true;
        state.queue.clear();
        drop(state);
        self.0.work.notify_all();
    }
}

fn run<T>(task: Task<'_, T>) -> Result<T, Failed> {
    // A task owns what it works on and leaves nothing half made behind it when it panics: the lane reports it and goes on.
    catch_unwind(AssertUnwindSafe(task)).map_err(|_| Failed)
}

/// The loop of a worker, which was counted free when it was started.
fn work<T>(shared: &Shared<'_, T>) {
    let mut state = shared.lock();
    loop {
        if let Some((number, task)) = state.queue.pop_front() {
            // Busy from here until its result is made.
            state.workers.took();
            drop(state);
            let result = run(task);
            state = shared.lock();
            state.done.insert(number, result);
            // Free again in the hold of the lock that makes the result seen: a caller that takes it and gives another task finds this worker free, and starts no other for it.
            // The lock is held on to the next task or to the wait, so the worker is never between the two unseen.
            state.workers.made();
            shared.ready.notify_all();
        } else if state.closing {
            state.workers.went();
            return;
        } else {
            state = shared.work.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl<'scope, T: Send + 'scope> Lane<'scope, '_, T> {
    /// Gives a task, which holds `charge` bytes while it is pending. It runs on a worker, started if the tasks waiting are more than the workers free to take them and the count
    /// allows, or on this thread when no worker can be had.
    pub fn submit(&self, charge: usize, task: impl FnOnce() -> T + Send + 'scope) {
        let mut state = self.shared.lock();
        let number = state.given;
        state.given += 1;
        state.charges.push_back(charge);
        state.charged = state.charged.saturating_add(charge);
        state.queue.push_back((number, Box::new(task)));
        if let Some(scope) = self.scope
            && state.workers.wants_one(state.queue.len(), self.max_workers)
            && !state.no_more_threads
        {
            let shared = Arc::clone(&self.shared);
            let starter = self.starter.clone();
            let place = state.workers.started;
            match Builder::new().name("diacraft-lane".to_owned()).spawn_scoped(scope, move || {
                starter.start(place);
                work(&shared);
            }) {
                // Free until it takes a task, which it looks for when it starts: a task given before then is its to take, and needs no other worker.
                Ok(_) => state.workers.started_one(),
                Err(_) => state.no_more_threads = true,
            }
        }
        if state.workers.started == 0 {
            // No worker, and none can be had: the task runs here, now (the first in the queue, which this task is, being alone in it).
            if let Some((number, task)) = state.queue.pop_front() {
                drop(state);
                let result = run(task);
                self.shared.lock().done.insert(number, result);
            }
            return;
        }
        drop(state);
        self.shared.work.notify_one();
    }

    /// Gives a result that is made already, to be taken in its place in the order (a task that is no work, or work the caller has done itself).
    pub fn submit_done(&self, charge: usize, result: T) {
        let mut state = self.shared.lock();
        let number = state.given;
        state.given += 1;
        state.charges.push_back(charge);
        state.charged = state.charged.saturating_add(charge);
        state.done.insert(number, Ok(result));
    }

    /// How many tasks were given and not taken.
    pub fn pending(&self) -> usize {
        let state = self.shared.lock();
        usize::try_from(state.given - state.taken).unwrap_or(usize::MAX)
    }

    /// The sum of the charges of the tasks given and not taken.
    pub fn charged(&self) -> usize {
        self.shared.lock().charged
    }

    /// How many workers were started.
    #[cfg(test)]
    fn started(&self) -> usize {
        self.shared.lock().workers.started
    }

    /// The result of the oldest task not taken **when it is made**, and `None` when it is not or when every task given has been taken (it never waits).
    pub fn try_take(&self) -> Option<Result<T, Failed>> {
        let mut state = self.shared.lock();
        let number = state.taken;
        let result = state.done.remove(&number)?;
        state.taken += 1;
        let charge = state.charges.pop_front().unwrap_or(0);
        state.charged = state.charged.saturating_sub(charge);
        Some(result)
    }

    /// The result of the oldest task not taken, waiting for it: `None` when every task given has been taken. The charge of the task is given back.
    pub fn take(&self) -> Option<Result<T, Failed>> {
        let mut state = self.shared.lock();
        if state.taken == state.given {
            return None;
        }
        loop {
            let number = state.taken;
            if let Some(result) = state.done.remove(&number) {
                state.taken += 1;
                let charge = state.charges.pop_front().unwrap_or(0);
                state.charged = state.charged.saturating_sub(charge);
                return Some(result);
            }
            state = self.shared.ready.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// A task of [`run_ordered`]: the closure, and the bytes it holds while it runs.
pub struct Ordered<'env, T> {
    pub charge: usize,
    pub task: Box<dyn FnOnce() -> T + Send + 'env>,
}

/// Runs `tasks` on at most `workers` threads and gives their results **in the order of the tasks**, whatever order they finished in; a task that panicked is [`Failed`] in its place.
/// The charges of the tasks in flight are held to `window` (a task charged more than `window` runs when no other is in flight), as the caller of a [`Lane`] does by hand.
pub fn run_ordered<'env, T: Send + 'env>(workers: usize, window: usize, tasks: Vec<Ordered<'env, T>>) -> Vec<Result<T, Failed>> {
    let count = tasks.len();
    lane(workers, |lane: &Lane<'_, '_, T>| {
        let mut out: Vec<Result<T, Failed>> = Vec::new();
        // A list that cannot be had is the caller's out of memory: the results that do not fit are not kept, and the count says so.
        if out.try_reserve_exact(count).is_err() {
            return out;
        }
        for Ordered { charge, task } in tasks {
            while lane.pending() > 0 && lane.charged().saturating_add(charge) > window {
                out.extend(lane.take());
            }
            lane.submit(charge, task);
        }
        while let Some(result) = lane.take() {
            out.push(result);
        }
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_is_the_setting_when_it_is_a_whole_number_held_to_the_range_and_else_the_cores_less_one() {
        assert_eq!(from_setting(Some("4"), 32), 4);
        assert_eq!(from_setting(Some(" 31 "), 32), 31);
        assert_eq!(from_setting(Some("0"), 32), 1, "none is the single-threaded path");
        assert_eq!(from_setting(Some("1"), 32), 1);
        assert_eq!(from_setting(Some("64"), 32), 64);
        assert_eq!(from_setting(Some("65"), 32), MAX_THREADS);
        assert_eq!(from_setting(Some("99999999999999999999999"), 32), 31, "a number past a word is no number");
        assert_eq!(from_setting(Some("-3"), 32), 31);
        assert_eq!(from_setting(Some("many"), 32), 31);
        assert_eq!(from_setting(None, 32), 31);
        assert_eq!(from_setting(None, 2), 1);
        assert_eq!(from_setting(None, 1), 1, "a core is left for the window, and at least one is had");
        assert_eq!(from_setting(None, 0), 1);
        assert_eq!(from_setting(None, 500), MAX_THREADS);
    }

    #[test]
    fn the_address_space_limit_is_read_from_the_text_of_the_systems_limits() {
        let text = "Limit                     Soft Limit           Hard Limit           Units     \nMax cpu time              unlimited            unlimited            seconds   \nMax address space         3072000000           unlimited            bytes     \nMax locked memory         8388608              8388608              bytes     \n";
        assert_eq!(limit_in(text), Some(3_072_000_000));
        assert_eq!(limit_in(&text.replace("3072000000", "unlimited")), None);
        assert_eq!(limit_in("Max cpu time unlimited unlimited seconds\n"), None);
        assert_eq!(limit_in(""), None);
        assert_eq!(limit_in("Max address space"), None);
        assert_eq!(limit_in("Max address space         -3 unlimited bytes"), None);
    }

    fn all(workers: usize, count: usize) -> Vec<usize> {
        lane(workers, |lane: &Lane<'_, '_, usize>| {
            for i in 0..count {
                // Tasks of uneven length finish out of order.
                lane.submit(1, move || {
                    if i % 3 == 0 {
                        std::thread::sleep(std::time::Duration::from_millis(3));
                    }
                    i
                });
            }
            let mut out = Vec::new();
            while let Some(result) = lane.take() {
                out.push(result.unwrap_or(usize::MAX));
            }
            out
        })
    }

    #[test]
    fn results_come_in_the_order_the_tasks_were_given_at_any_number_of_workers() {
        let expected: Vec<usize> = (0..40).collect();
        for workers in [0, 1, 2, 7, 31, 64, 500] {
            assert_eq!(all(workers, 40), expected, "{workers} workers");
        }
    }

    #[test]
    fn a_lane_with_nothing_given_has_nothing_to_take_and_nothing_charged() {
        lane(4, |lane: &Lane<'_, '_, u8>| {
            assert!(lane.take().is_none());
            assert_eq!((lane.pending(), lane.charged()), (0, 0));
        });
    }

    #[test]
    fn the_charge_of_a_task_is_held_until_its_result_is_taken_and_then_given_back() {
        lane(3, |lane: &Lane<'_, '_, u32>| {
            lane.submit(100, || 1);
            lane.submit(20, || 2);
            lane.submit_done(3, 3);
            assert_eq!((lane.pending(), lane.charged()), (3, 123));
            assert_eq!(lane.take(), Some(Ok(1)));
            assert_eq!((lane.pending(), lane.charged()), (2, 23));
            assert_eq!(lane.take(), Some(Ok(2)));
            assert_eq!(lane.take(), Some(Ok(3)));
            assert_eq!((lane.pending(), lane.charged()), (0, 0));
            assert_eq!(lane.take(), None);
        });
    }

    #[test]
    fn try_take_never_waits_and_gives_only_the_oldest_when_it_is_made() {
        lane(2, |lane: &Lane<'_, '_, u32>| {
            assert!(lane.try_take().is_none(), "nothing given");
            let gate = Arc::new((Mutex::new(false), Condvar::new()));
            let held = Arc::clone(&gate);
            lane.submit(5, move || {
                let (open, signal) = &*held;
                let mut now = open.lock().unwrap_or_else(PoisonError::into_inner);
                while !*now {
                    now = signal.wait(now).unwrap_or_else(PoisonError::into_inner);
                }
                1
            });
            lane.submit_done(1, 2);
            // The second is made and the first is not: nothing is given, in order.
            assert!(lane.try_take().is_none());
            assert_eq!((lane.pending(), lane.charged()), (2, 6));
            *gate.0.lock().unwrap_or_else(PoisonError::into_inner) = true;
            gate.1.notify_all();
            assert_eq!(lane.take(), Some(Ok(1)));
            assert_eq!(lane.try_take(), Some(Ok(2)));
            assert!(lane.try_take().is_none());
            assert_eq!((lane.pending(), lane.charged()), (0, 0));
        });
    }

    #[test]
    fn a_task_that_panics_is_an_error_in_its_place_and_the_tasks_round_it_are_not_lost() {
        for workers in [0, 1, 4] {
            let seen = lane(workers, |lane: &Lane<'_, '_, u32>| {
                lane.submit(1, || 10);
                lane.submit(1, || -> u32 { panic!("a task of a test that panics") });
                lane.submit(1, || 30);
                let mut out = Vec::new();
                while let Some(result) = lane.take() {
                    out.push(result);
                }
                out
            });
            assert_eq!(seen, vec![Ok(10), Err(Failed), Ok(30)], "{workers} workers");
        }
    }

    #[test]
    fn a_lane_that_is_left_with_tasks_waiting_ends_and_joins_its_workers() {
        // One worker, one task that holds it and many behind it: the body ends without taking any, and the lane drops the ones that never began instead of waiting for them.
        // The first task is let go only when a task behind it is dropped, and only the lane's close drops one unrun, so exactly one runs, whatever the schedule (
        // it was a sleep of 20 ms in each task, which a caller stalled for a second would have let all run).
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        let ran = Arc::new(AtomicUsize::new(0));
        let (let_go, running) = (Gate::new(), Gate::new());
        lane(1, |lane: &Lane<'_, '_, ()>| {
            let (first, gate, started) = (Arc::clone(&ran), Arc::clone(&let_go), Arc::clone(&running));
            lane.submit(1, move || {
                first.fetch_add(1, SeqCst);
                started.open();
                gate.wait();
            });
            for _ in 1..50 {
                let (ran, opens) = (Arc::clone(&ran), OpenAll(vec![Arc::clone(&let_go)]));
                lane.submit(1, move || {
                    ran.fetch_add(1, SeqCst);
                    drop(opens);
                });
            }
            // The worker holds the first task when the body ends.
            running.wait();
        });
        assert_eq!(ran.load(SeqCst), 1, "the tasks that were waiting when the lane closed were dropped unrun");
    }

    /// A gate a thread of a test waits at until the test opens it.
    struct Gate {
        open: Mutex<bool>,
        signal: Condvar,
    }

    impl Gate {
        fn new() -> Arc<Gate> {
            Arc::new(Gate { open: Mutex::new(false), signal: Condvar::new() })
        }

        fn wait(&self) {
            let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
            while !*open {
                open = self.signal.wait(open).unwrap_or_else(PoisonError::into_inner);
            }
        }

        fn open(&self) {
            *self.open.lock().unwrap_or_else(PoisonError::into_inner) = true;
            self.signal.notify_all();
        }
    }

    /// Opens its gates when it is dropped, so that a test that fails with threads at a gate ends instead of waiting to join them.
    struct OpenAll(Vec<Arc<Gate>>);

    impl Drop for OpenAll {
        fn drop(&mut self) {
            for gate in &self.0 {
                gate.open();
            }
        }
    }

    #[test]
    fn no_more_workers_are_started_than_the_tasks_in_flight_need() {
        // One task given and taken at a time: one worker, however many are allowed and in whatever order the system runs the threads. Before a change a worker that had
        // made its result and not yet gone back to wait was counted busy, and under load the next task started another (3 to 6 threads in 22 of 200 runs at load 50).
        let names = lane(31, |lane: &Lane<'_, '_, String>| {
            let mut names = std::collections::BTreeSet::new();
            for round in 0..200 {
                lane.submit(1, || format!("{:?}", std::thread::current().id()));
                if let Some(Ok(name)) = lane.take() {
                    names.insert(name);
                }
                assert_eq!(lane.started(), 1, "workers started by round {round}, each task taken before the next was given");
            }
            names
        });
        assert_eq!(names.len(), 1, "threads that ran 200 tasks that were each taken before the next was given");
    }

    #[test]
    fn a_caller_that_keeps_at_most_k_tasks_pending_has_at_most_k_workers() {
        // The tasks in flight are never more than the ones given and not taken, and the workers are never more than the most tasks in flight at once, under any schedule.
        for most in [2usize, 3, 5] {
            lane(31, |lane: &Lane<'_, '_, usize>| {
                for i in 0..300 {
                    if lane.pending() == most {
                        assert_eq!(lane.take().map(|result| result.is_ok()), Some(true));
                    }
                    lane.submit(1, move || i);
                    assert!(lane.started() <= most, "{} workers with at most {most} tasks pending, task {i}", lane.started());
                }
                while lane.take().is_some() {}
            });
        }
    }

    #[test]
    fn a_burst_of_tasks_that_all_wait_starts_a_worker_for_each_up_to_the_count() {
        for (count, tasks) in [(5usize, 8usize), (8, 8), (31, 4)] {
            let gate = Gate::new();
            let out = lane(count, |lane: &Lane<'_, '_, usize>| {
                let _open = OpenAll(vec![Arc::clone(&gate)]);
                for i in 0..tasks {
                    let gate = Arc::clone(&gate);
                    lane.submit(1, move || {
                        gate.wait();
                        i
                    });
                    assert_eq!(lane.started(), (i + 1).min(count), "{count} workers allowed, {} tasks in flight", i + 1);
                }
                gate.open();
                let mut out = Vec::new();
                while let Some(result) = lane.take() {
                    out.push(result);
                }
                out
            });
            assert_eq!(out, (0..tasks).map(Ok).collect::<Vec<_>>(), "{count} workers allowed");
        }
    }

    #[test]
    fn a_worker_the_system_is_slow_to_start_is_counted_free_and_no_other_is_started_for_the_task_it_will_take() {
        // The second worker is held back as it starts, as a loaded system holds a new thread back. The first runs the first task and then the second, and is never seen
        // waiting. A third task given then is the held worker's to take: no third worker is started for it, since two tasks are the most that were ever in flight.
        let held = Gate::new();
        let (first, second, running) = (Gate::new(), Gate::new(), Gate::new());
        let hold: Arc<dyn Fn(usize) + Send + Sync> = {
            let held = Arc::clone(&held);
            Arc::new(move |place: usize| {
                if place == 1 {
                    held.wait();
                }
            })
        };
        let out = lane_with(31, Starter(Some(hold)), |lane: &Lane<'_, '_, u32>| {
            let _open = OpenAll(vec![Arc::clone(&held), Arc::clone(&first), Arc::clone(&second), Arc::clone(&running)]);
            let (gate, started) = (Arc::clone(&first), Arc::clone(&running));
            lane.submit(1, move || {
                started.open();
                gate.wait();
                1
            });
            // The first worker holds the first task.
            running.wait();
            let gate = Arc::clone(&second);
            lane.submit(1, move || {
                gate.wait();
                2
            });
            assert_eq!(lane.started(), 2, "a second task while the first runs starts a second worker");
            first.open();
            assert_eq!(lane.take(), Some(Ok(1)));
            // The first worker took the second task in the hold of the lock that showed the first result; the held one has not started.
            lane.submit(1, || 3);
            assert_eq!(lane.started(), 2, "a worker that is starting is free: no third is started for the task it will take");
            held.open();
            second.open();
            vec![lane.take(), lane.take(), lane.take()]
        });
        assert_eq!(out, vec![Some(Ok(2)), Some(Ok(3)), None]);
    }

    #[test]
    fn run_ordered_gives_every_result_in_order_and_a_panic_in_its_place() {
        for workers in [0, 1, 5, 31] {
            let tasks: Vec<Ordered<'_, usize>> = (0..30usize)
                .map(|i| Ordered {
                    charge: 10,
                    task: Box::new(move || {
                        if i == 7 {
                            panic!("a task of a test that panics");
                        }
                        i
                    }),
                })
                .collect();
            let out = run_ordered(workers, 25, tasks);
            assert_eq!(out.len(), 30, "{workers} workers");
            for (i, result) in out.iter().enumerate() {
                assert_eq!(*result, if i == 7 { Err(Failed) } else { Ok(i) }, "{workers} workers, task {i}");
            }
        }
    }

    #[test]
    fn tasks_may_borrow_what_the_caller_holds() {
        let data: Vec<u32> = (1..=100).collect();
        let sums = lane(4, |lane: &Lane<'_, '_, u32>| {
            for chunk in data.chunks(10) {
                lane.submit(0, move || chunk.iter().sum());
            }
            let mut out = Vec::new();
            while let Some(Ok(sum)) = lane.take() {
                out.push(sum);
            }
            out
        });
        assert_eq!(sums.iter().sum::<u32>(), 5050);
        assert_eq!(sums.len(), 10);
    }
}
