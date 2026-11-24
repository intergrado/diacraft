//! Drawing a page off the UI thread, so the window never waits for a picture.
//!
//! The window sends a [`Job`] (a page, a view of it and the limits to draw within) and later takes back a [`Done`]. The worker
//! thread owns the renderer, builds it there (the machine's font database is scanned on that thread, never on the UI's:
//! `Renderer::with_fonts(FontDb::system_and_bundled())`), draws only the newest job when several wait, and puts a panic in drawing
//! into the answer as an error instead of ending: the next job is drawn as if nothing had happened.
//!
//! **A newer request stops the job in progress.** The worker keeps the newest serial asked for in a shared counter, and hands each job a
//! [`Cancel`] on it: the renderer reads it between shapes and ends a frame that a newer request has made pointless, so the current view
//! never waits behind a picture nobody wants any more. **The pixels are made here too**: the worker turns a finished frame into the egui
//! image and counts its lit pixels, so the window's thread only uploads it.
//!
//! Where a thread cannot be had (the web) or a test wants the order of events to be fixed, the same drawer runs inline: the job is
//! drawn on the calling thread, at once, and the answer is waiting for the next [`RenderWorker::poll`].
//!
//! The shape, a thread that keeps only the newest job and a `Done` for each, follows VectorCraft's `render_worker.rs`
//! (`docs/provenance.md`); the drawer behind it, the panic net, the stop and the inline mode are written for this window.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use diacraft_engine::doc::Document;
use diacraft_engine::guard;
use diacraft_engine::measure::SharedFonts;
use diacraft_fonts::FontDb;
use diacraft_render::{Cancel, Limits, Parallel, Rendered, Renderer, View, default_threads};
use egui::ColorImage;
use web_time::Instant;

/// One picture to draw.
pub struct Job {
    /// Which request this answers: the window numbers its requests and drops an answer that is not the latest it wants.
    pub serial: u64,
    pub doc: Arc<Document>,
    pub page: u32,
    pub view: View,
    pub limits: Limits,
    /// Whether this request makes the job in progress pointless, so that the job is stopped: a request for another view does, and the full
    /// picture of the view a rough one is being drawn for does not.
    pub supersedes: bool,
    /// Set by [`RenderWorker::submit`]: the stop a newer request makes. A job a test builds may leave it `None`.
    pub cancel: Option<Cancel>,
    /// A sheet of a print to draw, the pixels to a paper inch to draw it at and its colours (black and white draws it in greys), in place of the
    /// page through `view`: the Print view's preview (`print_preview.rs`). `view` then only says how large the picture is.
    pub sheet: Option<Box<(diacraft_render::print::Sheet, f64, diacraft_render::print::PrintColor)>>,
}

/// The answer to a [`Job`].
pub struct Done {
    pub serial: u64,
    /// How long drawing took.
    pub took: Duration,
    /// The picture, or the message of the panic that stopped it. The pixels were taken out ([`Done::image`]), so `rgba_premul` is empty;
    /// the size and what was left out are here.
    pub result: Result<Rendered, String>,
    /// The picture as an egui image, made on the worker's thread. `None` for a frame that was stopped, refused or failed.
    pub image: Option<ColorImage>,
    /// How many pixels of the picture are not clear (counted on the worker's thread).
    pub lit: usize,
}

impl Done {
    /// An answer that is a failure or has no picture.
    fn bare(serial: u64, took: Duration, result: Result<Rendered, String>) -> Done {
        Done { serial, took, result, image: None, lit: 0 }
    }
}

/// What draws a job. The real one is the renderer; a test supplies one that panics or counts.
pub trait Drawer: Send {
    fn draw(&mut self, job: &Job) -> Rendered;

    /// Drops what the drawer keeps between jobs (laid-out text, decoded pictures): another document is open. The default keeps nothing.
    fn clear_caches(&mut self) {}
}

/// The renderer, with the page's paper left clear: the window paints the paper and the grid first, and the picture goes over them
/// (`canvas/page_drawing.rs`).
struct RendererDrawer(Renderer);

impl RendererDrawer {
    /// A drawer over `renderer`, which draws a frame on the machine's cores less one (`DIACRAFT_THREADS` says otherwise: `diacraft_render::default_threads`).
    /// The frame is the same bytes at any count, and a page of a few shapes is drawn on one thread whatever the count (`Parallel::with_threads`).
    fn new(mut renderer: Renderer) -> RendererDrawer {
        renderer.set_parallel(Parallel::with_threads(default_threads()));
        RendererDrawer(renderer)
    }
}

impl Drawer for RendererDrawer {
    fn draw(&mut self, job: &Job) -> Rendered {
        self.0.set_limits(job.limits);
        self.0.set_paper(None);
        self.0.set_cancel(job.cancel.clone());
        match job.sheet.as_deref() {
            Some((sheet, px_per_inch, color)) => {
                self.0.set_print_color(*color);
                self.0.render_sheet(&job.doc, sheet, *px_per_inch)
            }
            None => self.0.render(&job.doc, job.page, &job.view),
        }
    }

    fn clear_caches(&mut self) {
        self.0.clear_caches();
    }
}

/// Builds a drawer; it runs on the worker thread, where scanning the machine's fonts costs the window nothing.
pub type MakeDrawer = Box<dyn FnOnce() -> Box<dyn Drawer> + Send>;

/// A drawer over the faces the application found once and shares with everything else that sets text (`DiacraftApp::with_fonts`): the renderer
/// of the page, the one of the Print view's sheets, a print, and the commits that measure text for the blocks the renderer draws (the engine's
/// `measure` module). Nothing is read from the machine here. The faces may still be being read when the window opens: the drawer is made on the
/// worker's thread, which waits for them there, so the window never does.
pub fn shared_drawer(fonts: SharedFonts) -> MakeDrawer {
    Box::new(move || Box::new(RendererDrawer::new(Renderer::with_shared_fonts(fonts.get()))))
}

/// A drawer over the machine's fonts and the bundled ones, read by the worker's thread for this drawer alone. For a window that was given no
/// shared database (`DiacraftApp::with_fonts`): it shares nothing with the engine's commits, which a drawer of its own must not set (two
/// drawers would each set theirs, and the last would win).
pub fn system_drawer() -> MakeDrawer {
    Box::new(|| Box::new(RendererDrawer::new(Renderer::with_fonts(FontDb::system_and_bundled()))))
}

/// The renderer over the bundled fonts only: nothing is read from the machine. For the web, and for tests that want the same
/// letters on every machine.
pub fn bundled_drawer() -> MakeDrawer {
    Box::new(|| Box::new(RendererDrawer::new(Renderer::new())))
}

/// What the window tells the thread.
enum Msg {
    Draw(Job),
    /// Everything asked for before this is of a document that is gone: drop what is kept for it.
    ClearCaches,
}

enum Mode {
    Thread { jobs: Sender<Msg>, done: Receiver<Done> },
    Inline { drawer: Box<dyn Drawer>, done: VecDeque<Done> },
}

/// Where pictures are drawn.
pub struct RenderWorker {
    mode: Mode,
    /// The newest serial asked for, which every job's [`Cancel`] reads.
    newest: Arc<AtomicU64>,
    /// The highest serial submitted so far.
    submitted: u64,
}

impl RenderWorker {
    /// A worker thread that builds its drawer with `make` and asks the window for a frame whenever a picture is ready. `None` when
    /// no thread can be had (the web): the caller then uses [`RenderWorker::inline`].
    pub fn spawn(ctx: egui::Context, make: MakeDrawer) -> Option<RenderWorker> {
        if cfg!(target_arch = "wasm32") {
            return None;
        }
        let (jobs, job_rx) = channel::<Msg>();
        let (done_tx, done) = channel::<Done>();
        std::thread::Builder::new()
            .name("diacraft-render".into())
            .spawn(move || {
                let mut drawer = match guard::catch_panic(make) {
                    Ok(drawer) => drawer,
                    Err(msg) => {
                        // The machine's fonts could not be read: draw with the bundled ones rather than not at all. The window is told once.
                        log::error!("the page renderer could not start with the machine's fonts: {msg}");
                        let _ = done_tx.send(Done::bare(0, Duration::ZERO, Err(format!("the machine's fonts could not be read: {msg}"))));
                        ctx.request_repaint();
                        (bundled_drawer())()
                    }
                };
                while let Ok(first) = job_rx.recv() {
                    // Only the newest request matters: the ones it replaces are not worth drawing. A clearing of the caches makes every job
                    // asked for before it one of a document that is gone, so those are dropped.
                    let mut job = None;
                    let mut next = Some(first);
                    while let Some(msg) = next.take().or_else(|| job_rx.try_recv().ok()) {
                        match msg {
                            Msg::Draw(newer) => job = Some(newer),
                            Msg::ClearCaches => {
                                drawer.clear_caches();
                                job = None;
                            }
                        }
                    }
                    let Some(job) = job else { continue };
                    let answer = draw_guarded(drawer.as_mut(), &job);
                    if done_tx.send(answer).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .ok()?;
        Some(RenderWorker { mode: Mode::Thread { jobs, done }, newest: Arc::new(AtomicU64::new(0)), submitted: 0 })
    }

    /// A worker that draws on the calling thread, inside [`RenderWorker::submit`].
    pub fn inline(make: MakeDrawer) -> RenderWorker {
        let drawer = guard::catch_panic(make).unwrap_or_else(|_| (bundled_drawer())());
        RenderWorker { mode: Mode::Inline { drawer, done: VecDeque::new() }, newest: Arc::new(AtomicU64::new(0)), submitted: 0 }
    }

    /// Whether pictures are drawn on the calling thread (the web, which has no thread to give), so that a slow one is a frozen window.
    pub fn is_inline(&self) -> bool {
        matches!(self.mode, Mode::Inline { .. })
    }

    /// Asks for a picture. A job in progress that this one makes pointless is stopped between shapes; one that waits is replaced.
    pub fn submit(&mut self, mut job: Job) {
        self.submitted = self.submitted.max(job.serial);
        if job.supersedes {
            self.newest.fetch_max(job.serial, Ordering::Relaxed);
        }
        job.cancel = Some(Cancel::new(Arc::clone(&self.newest), job.serial));
        match &mut self.mode {
            Mode::Thread { jobs, .. } => {
                // The thread is gone only if the process is ending; there is then nobody to draw for.
                let _ = jobs.send(Msg::Draw(job));
            }
            Mode::Inline { drawer, done } => {
                let answer = draw_guarded(drawer.as_mut(), &job);
                done.push_back(answer);
            }
        }
    }

    /// Another document replaced the old one: the job in progress is stopped, the jobs waiting are dropped, and what the drawer keeps for
    /// the old document (laid-out text, decoded pictures and the parts of the file they hold on to) is dropped as soon as it is idle.
    pub fn clear_caches(&mut self) {
        // One past the newest submitted: the job in progress is stopped, and the next one asked for (a serial above the last) is not.
        self.newest.fetch_max(self.submitted.saturating_add(1), Ordering::Relaxed);
        match &mut self.mode {
            Mode::Thread { jobs, .. } => {
                let _ = jobs.send(Msg::ClearCaches);
            }
            Mode::Inline { drawer, .. } => drawer.clear_caches(),
        }
    }

    /// The pictures that have arrived since the last call, oldest first.
    pub fn poll(&mut self) -> Vec<Done> {
        match &mut self.mode {
            Mode::Thread { done, .. } => done.try_iter().collect(),
            Mode::Inline { done, .. } => done.drain(..).collect(),
        }
    }
}

/// Draws `job`; a panic in drawing is the answer's error, and the drawer is used again for the next job. A finished picture is made into
/// the egui image here, with its lit pixels counted, so that the window's thread has only to upload it.
fn draw_guarded(drawer: &mut dyn Drawer, job: &Job) -> Done {
    let start = Instant::now();
    let result = guard::catch_panic(|| drawer.draw(job));
    let took = start.elapsed();
    match result {
        Ok(mut rendered) => {
            let (image, lit) = make_image(&mut rendered);
            Done { serial: job.serial, took, result: Ok(rendered), image, lit }
        }
        Err(msg) => Done::bare(job.serial, took, Err(msg)),
    }
}

/// The picture of a finished frame as an egui image, and the count of its pixels that are not clear. The pixels leave the frame (the
/// image holds them now). `None` for a frame with no pixels, or whose pixels are not as many as its size says.
fn make_image(rendered: &mut Rendered) -> (Option<ColorImage>, usize) {
    let (w, h) = (rendered.width as usize, rendered.height as usize);
    if w == 0 || h == 0 || rendered.rgba_premul.len() != w.saturating_mul(h).saturating_mul(4) {
        return (None, 0);
    }
    let lit = rendered.rgba_premul.as_chunks::<4>().0.iter().filter(|px| px[3] != 0).count();
    let image = ColorImage::from_rgba_premultiplied([w, h], &rendered.rgba_premul);
    rendered.rgba_premul = Vec::new();
    (Some(image), lit)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use diacraft_testkit::build::{DocBuilder, PageBuilder};
    use kurbo::Affine;

    use super::*;

    fn job(serial: u64) -> Job {
        let doc = DocBuilder::new().page(1, PageBuilder::new()).build();
        Job {
            serial,
            doc: Arc::new(doc),
            page: 1,
            view: View { to_px: Affine::IDENTITY, width: 4, height: 4 },
            limits: Limits::default(),
            supersedes: true,
            cancel: None,
            sheet: None,
        }
    }

    /// The window finds one database and every renderer it starts draws in that one: a shared drawer holds the very `Arc` the faces give (a
    /// copy of the database would not count as a holder), and reads nothing from the machine.
    #[test]
    fn a_shared_drawer_draws_in_the_database_it_was_given() {
        let db = Arc::new(FontDb::bundled());
        let fonts = diacraft_engine::measure::SharedFonts::ready(Arc::clone(&db));
        let before = Arc::strong_count(&db);
        let make = shared_drawer(fonts.clone());
        assert_eq!(fonts.holders(), 2, "the drawer waits with the faces");
        let drawer = make();
        assert_eq!(Arc::strong_count(&db), before + 1, "the renderer holds the same database and not a copy");
        drop(drawer);
        assert_eq!(Arc::strong_count(&db), before);
    }

    /// The drawer is made on the worker's thread, and there it waits for a scan of the machine's faces that is still going: the window that
    /// started the worker never does. Made after the scan, it draws in what the scan found.
    #[test]
    fn a_shared_drawer_waits_for_the_scan_on_its_own_thread_and_not_the_windows() {
        use std::sync::mpsc::channel;
        use std::time::Duration;

        let (release, gate) = channel::<()>();
        let fonts = diacraft_engine::measure::scan_fonts_with(move || {
            let _ = gate.recv();
            FontDb::bundled()
        });
        // Handing the drawer over is at once; making it waits.
        let make = shared_drawer(fonts.clone());
        assert!(!fonts.is_ready(), "the scan is held back");
        let (built, answered) = channel();
        let worker = std::thread::spawn(move || {
            let drawer = make();
            let _ = built.send(());
            drawer
        });
        assert!(answered.recv_timeout(Duration::from_millis(200)).is_err(), "the drawer waits for the faces");
        release.send(()).unwrap();
        let _drawer = worker.join().unwrap();
        assert!(fonts.is_ready());
        diacraft_engine::measure::set_measure_fonts(None);
    }

    /// A drawer of the window's own sets nothing for the engine: with a worker for the page and one for the Print view each setting its own
    /// database, the last to start would have decided what the commits measure in. Only the application's start sets it.
    #[test]
    fn a_window_without_shared_fonts_leaves_the_commits_fonts_alone() {
        diacraft_engine::measure::set_measure_fonts(None);
        let _drawer = system_drawer()();
        assert!(diacraft_engine::measure::measure_fonts().is_none(), "reading the machine's faces for a drawer sets nothing");
    }

    fn waiting_for(worker: &mut RenderWorker, n: usize) -> Vec<Done> {
        let mut all = Vec::new();
        for _ in 0..2000 {
            all.extend(worker.poll());
            if all.len() >= n {
                return all;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("{} answers of {n} arrived", all.len());
    }

    /// Panics on the calls the test names, and draws a clear 4 by 4 picture otherwise.
    struct Flaky {
        calls: Arc<AtomicUsize>,
        panic_on: Vec<usize>,
    }

    impl Drawer for Flaky {
        fn draw(&mut self, job: &Job) -> Rendered {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if self.panic_on.contains(&call) {
                panic!("the drawer broke on call {call}");
            }
            Rendered {
                rgba_premul: vec![0; (job.view.width * job.view.height * 4) as usize],
                width: job.view.width,
                height: job.view.height,
                skipped: Vec::new(),
            }
        }
    }

    fn flaky(panic_on: &[usize]) -> (MakeDrawer, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let (made, panic_on) = (Arc::clone(&calls), panic_on.to_vec());
        (Box::new(move || Box::new(Flaky { calls: made, panic_on })), calls)
    }

    /// The brief's case: a panic in drawing on the worker is an error in the answer, and the thread draws the next job.
    #[test]
    fn a_panic_in_drawing_is_an_error_in_the_answer_and_the_thread_draws_on() {
        let (make, calls) = flaky(&[0]);
        let mut worker = RenderWorker::spawn(egui::Context::default(), make).expect("a thread");
        worker.submit(job(1));
        let first = waiting_for(&mut worker, 1);
        assert_eq!(first[0].serial, 1);
        assert!(first[0].result.as_ref().unwrap_err().contains("the drawer broke on call 0"));
        worker.submit(job(2));
        let second = waiting_for(&mut worker, 1);
        assert!(second[0].result.is_ok(), "the next job is drawn as if nothing had happened");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn inline_mode_draws_inside_submit_and_contains_a_panic_too() {
        let (make, _) = flaky(&[1]);
        let mut worker = RenderWorker::inline(make);
        worker.submit(job(1));
        worker.submit(job(2));
        worker.submit(job(3));
        let done = worker.poll();
        assert_eq!(done.iter().map(|d| (d.serial, d.result.is_ok())).collect::<Vec<_>>(), [(1, true), (2, false), (3, true)]);
        assert!(worker.poll().is_empty(), "each answer is given once");
    }

    /// While the thread is busy with one job, only the newest of those that wait is drawn.
    #[test]
    fn only_the_newest_waiting_job_is_drawn() {
        struct Slow(Arc<Mutex<Vec<u64>>>);
        impl Drawer for Slow {
            fn draw(&mut self, job: &Job) -> Rendered {
                self.0.lock().unwrap().push(job.serial);
                std::thread::sleep(Duration::from_millis(150));
                Rendered { rgba_premul: Vec::new(), width: 0, height: 0, skipped: Vec::new() }
            }
        }
        let drawn = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&drawn);
        let mut worker = RenderWorker::spawn(egui::Context::default(), Box::new(move || Box::new(Slow(seen)))).expect("a thread");
        worker.submit(job(1));
        std::thread::sleep(Duration::from_millis(40));
        for serial in 2..=6 {
            worker.submit(job(serial));
        }
        let done = waiting_for(&mut worker, 2);
        assert_eq!(done.iter().map(|d| d.serial).collect::<Vec<_>>(), [1, 6]);
        assert_eq!(*drawn.lock().unwrap(), [1, 6], "jobs 2 to 5 were never drawn");
    }

    /// A drawer that cannot be made (the machine's fonts) is told once, as serial 0, and the bundled fonts draw instead.
    #[test]
    fn a_drawer_that_cannot_be_made_is_told_once_and_the_bundled_one_draws() {
        let mut worker = RenderWorker::spawn(egui::Context::default(), Box::new(|| panic!("no fonts here"))).expect("a thread");
        worker.submit(job(7));
        let done = waiting_for(&mut worker, 2);
        assert_eq!(done[0].serial, 0);
        assert!(done[0].result.as_ref().unwrap_err().contains("no fonts here"));
        assert_eq!(done[1].serial, 7);
        let picture = done[1].result.as_ref().unwrap();
        assert_eq!((picture.width, picture.height), (4, 4));
    }

    /// The real drawer leaves the paper clear: the window paints the paper and the grid under the picture.
    #[test]
    fn the_real_drawer_leaves_the_page_clear_where_nothing_is_drawn() {
        let mut drawer = bundled_drawer()();
        let done = drawer.draw(&job(1));
        assert!(done.rgba_premul.iter().all(|b| *b == 0), "a page with no shapes is clear, not white");
    }

    /// A drawer that spins until it is told to stop, as the renderer does between shapes, and says how it ended.
    struct Spinning {
        started: Arc<AtomicUsize>,
    }

    impl Drawer for Spinning {
        fn draw(&mut self, job: &Job) -> Rendered {
            let cancel = job.cancel.clone().expect("the worker hands every job a stop");
            // Only the first job spins; the others say at once whether they were stopped.
            if job.serial == 1 {
                self.started.fetch_add(1, Ordering::SeqCst);
                let begun = Instant::now();
                // Without a stop this would run for the whole of thirty seconds.
                while !cancel.is_stale() && begun.elapsed() < Duration::from_secs(30) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            let skipped = if cancel.is_stale() {
                vec![diacraft_render::Skipped { kind: diacraft_render::SkipKind::Cancelled, count: 1, first: "stopped".to_owned() }]
            } else {
                Vec::new()
            };
            Rendered { rgba_premul: Vec::new(), width: job.view.width, height: job.view.height, skipped }
        }
    }

    /// The review's case: a view that waited behind a job in progress for up to 1.5 s. The job in progress is told to stop the moment a
    /// newer one is asked for, and the newer one is drawn at once.
    #[test]
    fn a_newer_request_stops_the_job_in_progress_and_is_drawn_at_once() {
        let started = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&started);
        let mut worker = RenderWorker::spawn(egui::Context::default(), Box::new(move || Box::new(Spinning { started: seen }))).expect("a thread");
        worker.submit(job(1));
        for _ in 0..1000 {
            if started.load(Ordering::SeqCst) == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(started.load(Ordering::SeqCst), 1, "the first job is in progress");
        let asked = Instant::now();
        worker.submit(job(2));
        let done = waiting_for(&mut worker, 2);
        // Not a measure of cost: the first job is stopped at once (it says so below), and a job that was not stopped runs its thirty seconds out,
        // which twenty seconds tells from a stop on a machine at load 50.
        assert!(asked.elapsed() < Duration::from_secs(20), "the first job did not run its thirty seconds out: {:?}", asked.elapsed());
        assert_eq!(done[0].serial, 1);
        assert_eq!(done[0].result.as_ref().unwrap().count(diacraft_render::SkipKind::Cancelled), 1, "it was stopped, and says so");
        assert!(done[0].image.is_none() && done[0].lit == 0, "a stopped frame has no picture");
        // The newer job was drawn whole: nothing stops the newest.
        assert_eq!(done[1].serial, 2);
        assert_eq!(done[1].result.as_ref().unwrap().count(diacraft_render::SkipKind::Cancelled), 0);
    }

    /// Two openings in a row must not leave the next picture stopped before it starts.
    #[test]
    fn clearing_twice_does_not_stop_the_picture_asked_for_next() {
        let mut worker = RenderWorker::inline(Box::new(|| Box::new(Spinning { started: Arc::new(AtomicUsize::new(0)) })));
        worker.submit(job(5));
        worker.clear_caches();
        worker.clear_caches();
        worker.submit(job(6));
        let done = worker.poll();
        assert_eq!(done.len(), 2);
        assert_eq!(done[1].result.as_ref().unwrap().count(diacraft_render::SkipKind::Cancelled), 0, "job 6 is the newest");
    }

    /// The worker makes the egui image and counts the lit pixels, and the frame it hands over has given its pixels up.
    #[test]
    fn the_worker_makes_the_image_and_counts_the_lit_pixels() {
        struct Half;
        impl Drawer for Half {
            fn draw(&mut self, job: &Job) -> Rendered {
                let (w, h) = (job.view.width as usize, job.view.height as usize);
                let mut pixels = vec![0u8; w * h * 4];
                // The first row is lit, the others are clear.
                pixels.chunks_mut(4).take(w).for_each(|p| p.copy_from_slice(&[10, 20, 30, 255]));
                Rendered { rgba_premul: pixels, width: job.view.width, height: job.view.height, skipped: Vec::new() }
            }
        }
        let mut worker = RenderWorker::inline(Box::new(|| Box::new(Half)));
        worker.submit(job(1));
        let done = worker.poll();
        assert_eq!(done[0].lit, 4, "one row of a 4 by 4 picture");
        let image = done[0].image.as_ref().expect("an image was made");
        assert_eq!(image.size, [4, 4]);
        assert!(done[0].result.as_ref().unwrap().rgba_premul.is_empty(), "the bytes live in the image only");
    }

    /// Clearing the caches reaches the drawer, in order with the jobs: what was asked for before it is dropped, what comes after is drawn.
    #[test]
    fn clearing_the_caches_reaches_the_drawer_and_drops_the_jobs_asked_for_before_it() {
        struct Counting {
            cleared: Arc<AtomicUsize>,
            drawn: Arc<Mutex<Vec<u64>>>,
        }
        impl Drawer for Counting {
            fn draw(&mut self, job: &Job) -> Rendered {
                self.drawn.lock().unwrap().push(job.serial);
                std::thread::sleep(Duration::from_millis(100));
                Rendered { rgba_premul: Vec::new(), width: 0, height: 0, skipped: Vec::new() }
            }
            fn clear_caches(&mut self) {
                self.cleared.fetch_add(1, Ordering::SeqCst);
            }
        }
        let (cleared, drawn) = (Arc::new(AtomicUsize::new(0)), Arc::new(Mutex::new(Vec::new())));
        let (c, d) = (Arc::clone(&cleared), Arc::clone(&drawn));
        let mut worker =
            RenderWorker::spawn(egui::Context::default(), Box::new(move || Box::new(Counting { cleared: c, drawn: d }))).expect("a thread");
        worker.submit(job(1));
        std::thread::sleep(Duration::from_millis(30));
        // Job 2 waits behind job 1, then the old document goes, then job 3 is of the new one.
        worker.submit(job(2));
        worker.clear_caches();
        worker.submit(job(3));
        let done = waiting_for(&mut worker, 2);
        assert_eq!(done.iter().map(|d| d.serial).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(*drawn.lock().unwrap(), [1, 3], "job 2 was of the old document and is never drawn");
        assert_eq!(cleared.load(Ordering::SeqCst), 1);
        // And inline, at once.
        let (c, d) = (Arc::new(AtomicUsize::new(0)), Arc::new(Mutex::new(Vec::new())));
        let mut inline = RenderWorker::inline({
            let (c, d) = (Arc::clone(&c), Arc::clone(&d));
            Box::new(move || Box::new(Counting { cleared: c, drawn: d }))
        });
        inline.clear_caches();
        assert_eq!(c.load(Ordering::SeqCst), 1);
        assert!(drawn.lock().unwrap().len() == 2 && d.lock().unwrap().is_empty());
    }

    #[test]
    fn a_thread_worker_is_not_inline_and_an_inline_one_is() {
        let (make, _) = flaky(&[]);
        let thread = RenderWorker::spawn(egui::Context::default(), make).expect("a thread");
        assert!(!thread.is_inline());
        let (make, _) = flaky(&[]);
        assert!(RenderWorker::inline(make).is_inline());
    }

    /// What the review's item 9 counted: making the egui image of a picture of 24 million pixels (4,899 by 4,899) and counting its lit pixels,
    /// which now happens on the worker's thread and no longer on the window's. `cargo test -p diacraft-ui-egui --lib -- --ignored --nocapture`.
    #[test]
    #[ignore = "prints a time"]
    fn what_a_picture_of_24_million_pixels_costs_to_turn_into_an_image() {
        let side = 4_899u32;
        let mut rendered = Rendered {
            rgba_premul: (0..(side as usize * side as usize * 4)).map(|i| if i % 7 < 3 { 255 } else { 0 }).collect(),
            width: side,
            height: side,
            skipped: Vec::new(),
        };
        let started = Instant::now();
        let (image, lit) = make_image(&mut rendered);
        println!("image of {} pixels, {lit} lit, made in {:?} (on the worker's thread)", side * side, started.elapsed());
        assert!(image.is_some());
    }
}
