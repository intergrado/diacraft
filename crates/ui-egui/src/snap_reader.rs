//! Reading a page for a drag's snap off the window's thread.
//!
//! The first snap of a gesture makes what the rest of its moves ask: the index of the page's shapes (a frame and a box round the paths of each, up to
//! `MAX_INDEXED`) and the set of targets the library is given (every line of every shape, sorted). On a page of thousands of shapes that costs tens of
//! milliseconds and more, which a frame of the window must never wait for. So the window asks this reader for it when a drag begins, **on a thread of its own**,
//! and the first frames of the drag go as the pointer made them (nothing is snapped, nothing is sent with `snap`: the preview is the release) until the
//! answer is there. A drag's own work after that is a query.
//!
//! One thread works through the requests in order and keeps only the newest gesture's of those that wait (as the picture's worker does,
//! `render_worker.rs`; a gesture may ask for two kinds of query, and both are made): two requests for one page never build its targets at once, so the set a gesture asks for is the one the thread left. Where a thread
//! cannot be had (the web) or a test wants the order of events fixed, the same work is done on the calling thread, inside [`SnapReader::submit`]: **there a
//! page of thousands of shapes does freeze the window for the time it takes**, as a picture drawn inline does.
//!
//! What the thread does is [`warm`]: one query, of the kind the drag will ask, at a place nothing is near, whose answer is thrown away. The library's index
//! and target set live on the shared [`SnapPage`] and are keyed as a drag's own queries key them (the shapes that move, whether shapes are read, whether the
//! subject has sides of its own), so the queries that come after find them made.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use diacraft_engine::doc::Document;
use diacraft_engine::guard;
use diacraft_engine::snap::{SnapPage, SnapSettings, make_ready};

/// What a drag will ask of its page: which kind of query it makes (the engine's: `diacraft_engine::snap::Kind`).
pub use diacraft_engine::snap::Kind;

/// What a page was made ready for: the kind of query, and the shapes that move (sorted, each once; they are never targets).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warm {
    pub moving: Vec<u32>,
    pub kind: Kind,
}

impl Warm {
    pub fn new(moving: &[u32], kind: Kind) -> Warm {
        let mut moving = moving.to_vec();
        moving.sort_unstable();
        moving.dedup();
        Warm { moving, kind }
    }
}

/// The drawing, the revision and the page a reading is of.
pub type PageKey = (u64, u64, u32);

/// One request: make `page` ready, by `warm`, for the document as it is.
pub struct Job {
    pub key: PageKey,
    pub warm: Warm,
    pub doc: Arc<Document>,
    pub page: Arc<SnapPage>,
    pub settings: SnapSettings,
}

/// The answer to a [`Job`]: the page and what it is now ready for. `made` is false when the work did not finish (it panicked).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    pub key: PageKey,
    pub warm: Warm,
    pub made: bool,
}

/// What is done on the thread: a hook a test gives, to hold the work back and to count it, and the work itself.
pub type Hook = Arc<dyn Fn() + Send + Sync>;

/// Makes `job.page` ready for the queries of `job.warm` (`diacraft_engine::snap::make_ready`: one query whose answer is thrown away).
pub fn warm(job: &Job) {
    make_ready(&job.doc, &job.page, &job.settings, &job.warm.moving, job.warm.kind);
}

/// What the thread is told.
struct Msg {
    job: Job,
    hook: Option<Hook>,
}

enum Mode {
    Thread { jobs: Sender<Msg>, done: Receiver<Done> },
    Inline { done: VecDeque<Done> },
}

/// Where pages are made ready.
pub struct SnapReader {
    mode: Mode,
    hook: Option<Hook>,
    /// The window's context, to ask for a frame when an answer is ready; set by [`SnapReader::attach`] before the thread is started.
    ctx: Option<egui::Context>,
    /// Whether the thread has been started: it is, the first time it has work and a context to wake.
    started: bool,
    /// What was asked for and has not been answered: the key of the page and what it is to be ready for, **each kind of query a landing asks apart** (the Text tool's drag asks for
    /// two: a request for the second must not take the first for answered, nor ask it again at every frame while it waits). A request for another gesture (another page, revision or
    /// set of moving shapes) takes the place of these, as the thread makes only the newest of the requests that wait.
    pending: Vec<(PageKey, Warm)>,
    /// Requests made, for tests that count them.
    asked: usize,
}

impl Default for SnapReader {
    fn default() -> SnapReader {
        if cfg!(target_arch = "wasm32") { SnapReader::inline() } else { SnapReader::threaded() }
    }
}

impl SnapReader {
    /// A reader that works on a thread of its own, started when it first has something to do.
    pub fn threaded() -> SnapReader {
        // The channels are made when the thread starts; until then these are ends of a pair that nothing sends on.
        let (jobs, _) = channel::<Msg>();
        let (_, done) = channel::<Done>();
        SnapReader { mode: Mode::Thread { jobs, done }, hook: None, ctx: None, started: false, pending: Vec::new(), asked: 0 }
    }

    /// A threaded reader whose thread calls `hook` before each request it works on: a test holds the work back with it, and counts it.
    pub fn threaded_with(hook: Hook) -> SnapReader {
        SnapReader { hook: Some(hook), ..SnapReader::threaded() }
    }

    /// A reader that does the work on the calling thread, at once.
    pub fn inline() -> SnapReader {
        SnapReader { mode: Mode::Inline { done: VecDeque::new() }, hook: None, ctx: None, started: false, pending: Vec::new(), asked: 0 }
    }

    /// Whether the work is done on the calling thread.
    pub fn is_inline(&self) -> bool {
        matches!(self.mode, Mode::Inline { .. })
    }

    /// Tells the reader the window's context, which it wakes when an answer is ready.
    pub fn attach(&mut self, ctx: &egui::Context) {
        if self.ctx.is_none() {
            self.ctx = Some(ctx.clone());
        }
    }

    /// Whether a request is waiting for its answer.
    pub fn busy(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Whether the request for `page` made ready for `warm` has been asked for and not answered.
    pub fn is_pending(&self, key: PageKey, warm: &Warm) -> bool {
        self.pending.iter().any(|(k, w)| *k == key && w == warm)
    }

    /// How many requests have been made.
    pub fn asked(&self) -> usize {
        self.asked
    }

    /// Asks for a page to be made ready. On the thread it is queued and the answer comes through [`SnapReader::poll`]; inline it is made now.
    pub fn submit(&mut self, job: Job) {
        self.asked += 1;
        crate::work::count(|w| w.reads_asked += 1);
        self.pending.retain(|(k, w)| *k == job.key && w.moving == job.warm.moving && *w != job.warm);
        self.pending.push((job.key, job.warm.clone()));
        match &mut self.mode {
            Mode::Inline { done } => {
                crate::work::count(|w| w.reads_inline += 1);
                let answer = make(&job, None);
                done.push_back(answer);
            }
            Mode::Thread { jobs, done } => {
                if !self.started {
                    let (job_tx, job_rx) = channel::<Msg>();
                    let (done_tx, done_rx) = channel::<Done>();
                    let wake = self.ctx.clone();
                    let spawned = std::thread::Builder::new().name("diacraft-snap".into()).spawn(move || {
                        'work: while let Ok(first) = job_rx.recv() {
                            let mut batch = vec![first];
                            while let Ok(newer) = job_rx.try_recv() {
                                batch.push(newer);
                            }
                            // Only the newest gesture's requests matter: a drag that began after another (another page, revision or set of moving shapes) has no use for what the
                            // other asked. **The requests of that one gesture are all made, in order**: a landing may ask for two kinds of query (the Text tool's drag does), and
                            // a request for the second must not do away with the first.
                            let newest = batch.last().map(|m| (m.job.key, m.job.warm.moving.clone()));
                            batch.retain(|m| Some((m.job.key, m.job.warm.moving.clone())) == newest);
                            for msg in batch {
                                let answer = make(&msg.job, msg.hook.as_ref());
                                // A send that fails means the window is gone: there is nobody to tell.
                                if done_tx.send(answer).is_err() {
                                    break 'work;
                                }
                                if let Some(ctx) = &wake {
                                    ctx.request_repaint();
                                }
                            }
                        }
                    });
                    match spawned {
                        Ok(_) => {
                            (*jobs, *done) = (job_tx, done_rx);
                            self.started = true;
                        }
                        Err(_) => {
                            // No thread to be had: the work is done here, as inline does.
                            crate::work::count(|w| w.reads_inline += 1);
                            let answer = make(&job, None);
                            self.mode = Mode::Inline { done: VecDeque::from([answer]) };
                            return;
                        }
                    }
                }
                let msg = Msg { job, hook: self.hook.clone() };
                // The thread is gone only if the process is ending; there is then nobody to make a page ready for.
                if jobs.send(msg).is_err() {
                    self.pending.pop();
                }
            }
        }
    }

    /// The answers that have arrived since the last call, oldest first.
    pub fn poll(&mut self) -> Vec<Done> {
        let answers: Vec<Done> = match &mut self.mode {
            Mode::Thread { done, .. } => done.try_iter().collect(),
            Mode::Inline { done } => done.drain(..).collect(),
        };
        for done in &answers {
            self.pending.retain(|(k, w)| !(*k == done.key && *w == done.warm));
        }
        answers
    }
}

/// The work of one request, under the panic net: a request that panics is an answer that says it did not finish, and the thread goes on.
fn make(job: &Job, hook: Option<&Hook>) -> Done {
    let made = guard::catch_panic(|| {
        if let Some(hook) = hook {
            hook();
        }
        warm(job);
    })
    .is_ok();
    Done { key: job.key, warm: job.warm.clone(), made }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use diacraft_engine::snap::{Edges, Extras, OwnFrame, Query, Rect, Subject, Turn, Use, snap_ext, snap_turn};
    use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

    use super::*;

    /// A page of three turned rectangles (the first is the one that moves), with a slanted guide.
    fn doc() -> Document {
        let shape = |id: u32, x: f64, degrees: f64| {
            ShapeBuilder::new(id)
                .cell("PinX", &x.to_string())
                .cell("PinY", "4")
                .cell("Width", "2")
                .cell("Height", "1")
                .cell("LocPinX", "1")
                .cell("LocPinY", "0.5")
                .cell("Angle", &f64::to_radians(degrees).to_string())
        };
        DocBuilder::new()
            .page(
                1,
                PageBuilder::new()
                    .cell("PageWidth", "20")
                    .cell("PageHeight", "20")
                    .shape(shape(1, 3.0, 30.0))
                    .shape(shape(2, 7.0, 30.0))
                    .shape(shape(3, 11.0, 0.0)),
            )
            .build()
    }

    fn settings() -> SnapSettings {
        let mut s = SnapSettings::none();
        s.snap = true;
        s.guides = true;
        s.alignment_box = true;
        s.dynamic_grid = true;
        s.glue = true;
        s.glue_connection_points = true;
        s
    }

    fn job(doc: &Document, kind: Kind) -> Job {
        Job {
            key: (1, 1, 1),
            warm: Warm::new(&[1], kind),
            doc: Arc::new(doc.clone()),
            page: Arc::new(SnapPage::read(doc, 1).unwrap()),
            settings: settings(),
        }
    }

    fn frame() -> OwnFrame {
        OwnFrame { centre: (3.0, 4.0), width: 2.0, height: 1.0, angle: f64::to_radians(30.0) }
    }

    /// What `warm` makes ready is what a query of that kind asks for: the shapes' index and a set of targets built once, and the query that comes after builds none
    /// (the set is keyed by the shapes that move, whether shapes are read at all, and whether the subject has sides of its own). A kind that warmed another's set would leave
    /// the work to the window's thread, which is what the reader is for.
    #[test]
    fn what_warm_makes_ready_is_what_the_query_of_that_kind_asks_for() {
        let doc = doc();
        let rect = Rect { x0: 2.0, y0: 3.5, x1: 4.0, y1: 4.5 };
        let ask = |page: &SnapPage, kind: Kind| match kind {
            Kind::Box { framed } => {
                let query = Query { subject: Subject::Box(rect), using: Use::Place, moving: &[1], zoom: 96.0, own_points: &[] };
                snap_ext(&doc, page, &settings(), &query, Extras { frame: framed.then(frame), ..Extras::default() });
            }
            Kind::Edges { framed } => {
                let query = Query {
                    subject: Subject::Edges(rect, Edges { right: true, ..Edges::default() }),
                    using: Use::Place,
                    moving: &[1],
                    zoom: 96.0,
                    own_points: &[],
                };
                snap_ext(&doc, page, &settings(), &query, Extras { frame: framed.then(frame), ..Extras::default() });
            }
            Kind::Point(using) => {
                let query = Query { subject: Subject::Point((3.0, 4.0)), using, moving: &[1], zoom: 96.0, own_points: &[] };
                snap_ext(&doc, page, &settings(), &query, Extras::default());
            }
            Kind::Turn => {
                snap_turn(&doc, page, &settings(), &[1], Turn { pivot: (3.0, 4.0), angle: 0.7, radius: 1.5, zoom: 96.0 });
            }
        };
        for kind in [
            Kind::Box { framed: false },
            Kind::Box { framed: true },
            Kind::Edges { framed: false },
            Kind::Edges { framed: true },
            Kind::Point(Use::Place),
            Kind::Point(Use::Connect),
            Kind::Turn,
        ] {
            let job = job(&doc, kind);
            assert!(!job.page.shapes_read() && job.page.target_builds() == 0, "{kind:?}: nothing is made before the work");
            warm(&job);
            assert!(job.page.shapes_read(), "{kind:?}: the index of the shapes is made");
            assert_eq!(job.page.target_builds(), 1, "{kind:?}: one set of targets");
            ask(&job.page, kind);
            assert_eq!(job.page.target_builds(), 1, "{kind:?}: the query after it builds no other set");
        }
    }

    /// A set made for other shapes is not the drag's: a page made ready for shape 1 is built again for shape 2, which says why the window asks again for each drag.
    #[test]
    fn a_page_made_ready_for_other_shapes_builds_its_targets_again() {
        let doc = doc();
        let job = job(&doc, Kind::Box { framed: false });
        warm(&job);
        let other = Job { warm: Warm::new(&[2], Kind::Box { framed: false }), ..job };
        warm(&other);
        assert_eq!(other.page.target_builds(), 2);
    }

    #[test]
    fn an_inline_reader_makes_the_page_ready_inside_submit_and_says_so_to_the_window_that_counts() {
        crate::work::reset();
        let doc = doc();
        let mut reader = SnapReader::inline();
        let job = job(&doc, Kind::Turn);
        let page = Arc::clone(&job.page);
        reader.submit(job);
        assert!(reader.is_inline() && reader.busy() && page.shapes_read(), "made at once, answer waiting");
        assert_eq!(crate::work::work().reads_inline, 1, "counted on this thread");
        let answers = reader.poll();
        assert_eq!(answers, [Done { key: (1, 1, 1), warm: Warm::new(&[1], Kind::Turn), made: true }]);
        assert!(!reader.busy() && reader.poll().is_empty(), "each answer is given once");
        assert_eq!(reader.asked(), 1);
    }

    /// A thread of its own: the work is not done on this thread, only the newest of the requests that wait is made, and the answers come in order.
    #[test]
    fn a_threaded_reader_works_apart_and_only_the_newest_waiting_request_is_made() {
        crate::work::reset();
        let doc = doc();
        let (let_go, held) = std::sync::mpsc::channel::<()>();
        let held = Mutex::new(held);
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        // The first request waits to be let go; the others do not.
        let hook: Hook = Arc::new(move || {
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                let _ = held.lock().map(|rx| rx.recv());
            }
        });
        let mut reader = SnapReader::threaded_with(hook);
        let mut jobs: Vec<Job> = (1..=4u32).map(|n| Job { key: (1, u64::from(n), 1), ..job(&doc, Kind::Turn) }).collect();
        let first = jobs.remove(0);
        reader.submit(first);
        // The thread has the first and waits in its hook; the three after it queue.
        for _ in 0..5_000 {
            if calls.load(Ordering::SeqCst) == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        for job in jobs {
            reader.submit(job);
        }
        assert!(reader.busy() && reader.poll().is_empty(), "nothing answered, and nothing waited for");
        let_go.send(()).unwrap();
        let mut answered = Vec::new();
        for _ in 0..5_000 {
            answered.extend(reader.poll());
            if reader.is_pending((1, 4, 1), &Warm::new(&[1], Kind::Turn)) {
                std::thread::sleep(Duration::from_millis(1));
            } else {
                break;
            }
        }
        // The thread made the first and then the newest of the three that waited: requests 2 and 3 were never made.
        answered.extend(reader.poll());
        assert_eq!(answered.iter().map(|d| d.key.1).collect::<Vec<_>>(), [1, 4]);
        assert_eq!(calls.load(Ordering::SeqCst), 2, "two requests were made, of four");
        assert!(answered.iter().all(|d| d.made) && !reader.busy());
        assert_eq!(crate::work::work().reads_inline, 0, "none on this thread");
        assert_eq!(reader.asked(), 4);
    }

    /// Work that panics is an answer that says it was not made, and the thread makes the next request.
    #[test]
    fn work_that_panics_is_told_and_the_thread_goes_on() {
        let doc = doc();
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let hook: Hook = Arc::new(move || {
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("the work broke");
            }
        });
        let mut reader = SnapReader::threaded_with(hook);
        let mut taken = Vec::new();
        for n in 1..=2u64 {
            reader.submit(Job { key: (1, n, 1), ..job(&doc, Kind::Turn) });
            for _ in 0..5_000 {
                taken.extend(reader.poll());
                if taken.len() == usize::try_from(n).unwrap() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert_eq!(taken.iter().map(|d| (d.key.1, d.made)).collect::<Vec<_>>(), [(1, false), (2, true)]);
    }

    /// The kinds of query one landing asks for wait together (the Text tool's drag asks for two): a request for the second does not take the first for answered, and the reader
    /// is busy until both are. A request for another gesture (here, another revision of the page) takes their place; each answer takes off its own request.
    #[test]
    fn the_kinds_of_one_landing_wait_together_and_another_gesture_takes_their_place() {
        let doc = doc();
        let mut reader = SnapReader::inline();
        let (a, b) = (Kind::Box { framed: false }, Kind::Point(Use::Place));
        reader.submit(job(&doc, a));
        reader.submit(job(&doc, b));
        assert!(reader.is_pending((1, 1, 1), &Warm::new(&[1], a)) && reader.is_pending((1, 1, 1), &Warm::new(&[1], b)), "both wait");
        assert!(!reader.is_pending((1, 1, 1), &Warm::new(&[1], Kind::Turn)) && !reader.is_pending((1, 2, 1), &Warm::new(&[1], a)));
        // The same request again is one request waiting, not two.
        reader.submit(job(&doc, a));
        assert_eq!(reader.pending.iter().filter(|(_, w)| *w == Warm::new(&[1], a)).count(), 1);
        // Another revision of the page: the older requests are no use any more.
        reader.submit(Job { key: (1, 2, 1), ..job(&doc, a) });
        assert!(!reader.is_pending((1, 1, 1), &Warm::new(&[1], a)) && !reader.is_pending((1, 1, 1), &Warm::new(&[1], b)));
        assert!(reader.is_pending((1, 2, 1), &Warm::new(&[1], a)) && reader.busy());
        // Every answer takes its own request off; the inline reader answered them all in `submit`.
        let answers = reader.poll();
        assert_eq!(answers.len(), 4);
        assert!(!reader.busy(), "nothing waits once the newest is answered");
    }

    /// Two kinds of query asked for one gesture while the thread is busy are both made (the second does not do away with the first), in the order they were asked.
    #[test]
    fn the_thread_makes_every_kind_a_gesture_asked_and_only_the_newest_gestures() {
        let doc = doc();
        let (let_go, held) = std::sync::mpsc::channel::<()>();
        let held = Mutex::new(held);
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let hook: Hook = Arc::new(move || {
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                let _ = held.lock().map(|rx| rx.recv());
            }
        });
        let mut reader = SnapReader::threaded_with(hook);
        let (a, b) = (Kind::Box { framed: false }, Kind::Point(Use::Place));
        reader.submit(Job { key: (1, 1, 1), ..job(&doc, Kind::Turn) });
        for _ in 0..5_000 {
            if calls.load(Ordering::SeqCst) == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1, "the thread has the first request");
        // Revision 2 is asked for two kinds while the thread holds the first request: when it lets go it drains both, and makes them in order.
        reader.submit(Job { key: (1, 2, 1), ..job(&doc, a) });
        reader.submit(Job { key: (1, 2, 1), ..job(&doc, b) });
        let_go.send(()).unwrap();
        let mut answered = Vec::new();
        for _ in 0..5_000 {
            answered.extend(reader.poll());
            if reader.busy() {
                std::thread::sleep(Duration::from_millis(1));
            } else {
                break;
            }
        }
        assert_eq!(
            answered.iter().map(|d| (d.key.1, d.warm.kind)).collect::<Vec<_>>(),
            [(1, Kind::Turn), (2, a), (2, b)],
            "the first, then both kinds of the gesture that waited, in the order they were asked"
        );
        assert!(!reader.busy() && answered.iter().all(|d| d.made));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    /// **M15 of the re-review**: requests for one page and revision that differ in the shapes that move are requests of different gestures, and the thread makes the newest only, so a
    /// request it skips is never answered. A request that waits takes the place of the older requests of other shapes in the list of what is asked and not answered; if it did not,
    /// the skipped one would wait for ever and the reader would be busy for ever (the window never at rest). The thread holds the first request; the second (shape 2) and third
    /// (shape 3) of one page and revision queue behind it, and only the third is made.
    #[test]
    fn requests_for_other_shapes_that_move_take_the_place_of_the_older_ones_and_the_reader_is_not_left_busy() {
        let doc = doc();
        let (let_go, held) = std::sync::mpsc::channel::<()>();
        let held = Mutex::new(held);
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let hook: Hook = Arc::new(move || {
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                let _ = held.lock().map(|rx| rx.recv());
            }
        });
        let mut reader = SnapReader::threaded_with(hook);
        let turn = |moving: &[u32]| Job { warm: Warm::new(moving, Kind::Turn), ..job(&doc, Kind::Turn) };
        reader.submit(turn(&[1]));
        for _ in 0..5_000 {
            if calls.load(Ordering::SeqCst) == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1, "the thread has the first request");
        reader.submit(turn(&[2]));
        reader.submit(turn(&[3]));
        assert!(
            !reader.is_pending((1, 1, 1), &Warm::new(&[2], Kind::Turn)) && !reader.is_pending((1, 1, 1), &Warm::new(&[1], Kind::Turn)),
            "a request for other shapes takes the place of the older ones of the page"
        );
        assert!(reader.is_pending((1, 1, 1), &Warm::new(&[3], Kind::Turn)));
        let_go.send(()).unwrap();
        let mut answered = Vec::new();
        for _ in 0..5_000 {
            answered.extend(reader.poll());
            if reader.busy() {
                std::thread::sleep(Duration::from_millis(1));
            } else {
                break;
            }
        }
        assert!(!reader.busy(), "the request the thread skipped is not waited for: {:?}", reader.pending);
        assert_eq!(
            answered.iter().map(|d| d.warm.moving.clone()).collect::<Vec<_>>(),
            [vec![1], vec![3]],
            "the first, and the newest of the two that waited"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn warm_sorts_and_dedups_the_shapes_that_move_so_that_two_orders_are_one() {
        assert_eq!(Warm::new(&[5, 2, 5, 9, 2], Kind::Turn), Warm::new(&[9, 5, 2], Kind::Turn));
        assert_eq!(Warm::new(&[5, 2], Kind::Turn).moving, [2, 5]);
        assert_ne!(Warm::new(&[1], Kind::Turn), Warm::new(&[1], Kind::Box { framed: false }));
        assert_ne!(Warm::new(&[1], Kind::Box { framed: true }), Warm::new(&[1], Kind::Box { framed: false }));
    }
}
