//! The pages a session keeps read for the snap (the review), and the counts of the work a thread does reading them.
//!
//! **Why.** A snapped command (a release, an arrow key, a turn, a resize, a drop) used to read the page for itself: the guides and the grid, then, on its first query, the index of
//! the page's shapes and the targets the library is given. On a page of 10,000 shapes that is 240 milliseconds on the thread that runs the command, against 10 for the same
//! move with no snap (the review's measurement). The window makes the index and the targets off its own thread (`ui-egui` `snap_reader.rs`), so what the commands need is to
//! **find that read, and not make it again.**
//!
//! **What is kept.** A [`SnapPage`] with the index and the target set the reader left on it, **one for each page**, keyed by the document's revision ([`crate::Session::revision`]),
//! the page, and the document's Snap and Glue settings (the options that decide which targets a query reads). An edit moves the revision, so the read is no longer
//! the document's and is dropped the next time it is asked for; undo and redo, a new document and an opened file move it too. **A read taken while a command runs is never kept**:
//! a command that fails puts the revision back, and the revision it reached on the way may be reached again by another document.
//!
//! **Who makes it and who finds it.** [`crate::Session::snap_page`] makes the read and keeps it: the window asks for it when a gesture begins, and its reader's thread makes the index
//! and the targets on that very read. A command looks with [`crate::Session::kept_snap_page`] and, **finding none (a cold page), reads as it did before**: correct, and slower. A
//! command that finds a read the reader is still working on waits for the reader's work (the index is made once, whoever asks first), which is no longer than making it itself.
//!
//! **What it costs in memory** is charged: [`SnapPage::held_bytes`] counts the index and the target set, the cache holds **about** [`MAX_KEPT_BYTES`] of them and drops the page used
//! longest ago when the sum is over. It is an estimate counted by the number of entries, at prices **measured from the process's resident memory** (the first
//! prices were 2.4 times too low) and rounded up, not the allocator's figure. **"About"**: the sum is checked when a read is kept and when one is found, and a read grows after it
//! is kept (the window's reader makes the index and the targets then), so between two asks the sum may pass the cap by what one page of [`super::index::MAX_INDEXED`] shapes comes to
//! (about 19 MiB for the review's page of 10,000 turned stock shapes), and no more: the reader works on the page being dragged, and one page is asked for at a time. A new or opened
//! drawing drops every read ([`PageReads::clear`]).
//!
//! **To check the prices** when they change (the object, point and line prices of `adapter.rs`, the index's of `index.rs`): the gate holds each price and the sum of a page's parts by
//! pure counts (`each_price_of_a_set_of_targets_is_pinned_and_they_add_up` and its fellows), which do not depend on the allocator. What a page really holds is measured by four
//! tests that are **ignored** because a process's resident memory is the allocator's (the charge over the memory held of turned stock shapes is 1.26 here, and 1.05 with
//! `GLIBC_TUNABLES=glibc.malloc.hugetlb=1`): from the repository, with the engine's lib tests under nextest,
//! `cargo nextest run -p diacraft-engine --lib --run-ignored only --no-capture -E 'test(/snap::reads::tests::(measure_|a_page_of_.*in_memory)/)'` runs all four. The two named
//! `measure_...` print the bytes a shape (`SNAP_MEASURE_KIND` 0 to 3 chooses the kind of page and `SNAP_MEASURE_N` the number of shapes; the kinds are in `measured_doc`) and the
//! two named `a_page_of_..._in_memory` assert that the charge of a page is over what it holds. Set the prices from the first, and see the second two pass.
//!
//! **Counts.** [`counts`] says, for the calling thread, how many pages were read, how many indexes and target sets were built and how many reads were found: a test holds the work of
//! the thread that runs a command to a number, never to a time (the counters are per thread, so the reader's thread is not the window's).

use std::cell::Cell;
use std::sync::{Arc, Mutex, MutexGuard};

use diacraft_doc::Document;

use super::{DEFAULT_ZOOM, Edges, Extras, OwnFrame, Query, Rect, SnapPage, SnapSettings, Subject, Turn, Use, snap_ext, snap_turn};

/// About the most bytes the kept reads of a session hold in all (checked when a read is kept or found: see the module's "about"). A page of [`super::index::MAX_INDEXED`] shapes is
/// about 19 MiB by [`SnapPage::held_bytes`] (the review's page of 10,000 turned stock shapes, at the measured prices), so three of them fit and a drawing of several big pages keeps the
/// ones in use and none of the rest.
pub const MAX_KEPT_BYTES: usize = 64 * 1024 * 1024;

/// What a thread has done reading pages for the snap since [`reset_counts`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Pages read (`SnapPage::read`: the guides and the grid).
    pub page_reads: usize,
    /// Indexes of a page's shapes built (the 10 to 20 microseconds a shape).
    pub index_builds: usize,
    /// Target sets built for the library (every line of every indexed shape, sorted).
    pub target_builds: usize,
    /// Times a command looked for the kept read of its page and found it, and times it found none and read the page itself.
    pub kept_found: usize,
    pub kept_missing: usize,
    /// Drops made on a copy of the document to find the shape a master's drop makes (`snap::drop_preview`: the window's ghost and the places it is pulled by, U49).
    pub drops: usize,
}

thread_local! {
    static COUNTS: Cell<Counts> = const { Cell::new(Counts { page_reads: 0, index_builds: 0, target_builds: 0, kept_found: 0, kept_missing: 0, drops: 0 }) };
}

/// What the calling thread has read since [`reset_counts`].
pub fn counts() -> Counts {
    COUNTS.with(Cell::get)
}

/// Starts the calling thread's count again from nothing.
pub fn reset_counts() {
    COUNTS.with(|c| c.set(Counts::default()));
}

/// Changes the calling thread's count.
pub(crate) fn count(change: impl FnOnce(&mut Counts)) {
    COUNTS.with(|c| {
        let mut now = c.get();
        change(&mut now);
        c.set(now);
    });
}

/// What a drag will ask of its page: which kind of query it makes. A subject with sides of its own turned off the page's axes (`framed`) is told the guides that lie along no axis
/// too, so its set of targets is not the one of a subject without.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The box of shapes that move (or of a shape dropped, or of a text box) is snapped.
    Box { framed: bool },
    /// A resize's edges.
    Edges { framed: bool },
    /// A point: a drawing tool's, or the end of a connector.
    Point(Use),
    /// A turn by the rotation handle.
    Turn,
}

/// Makes `page` ready for the queries of `kind` by the shapes `moving` (sorted, each once): **one query whose answer is thrown away**, at the origin where a box of no size is near
/// nothing. The library's index and the target set live on the shared [`SnapPage`] and are keyed as a query keys them (the shapes that move, whether shapes are read, whether the
/// subject has sides of its own), so the queries that come after, by a window's drag or by the command that sends its release, find them made. It is the work of the window's reader
/// thread (`ui-egui` `snap_reader.rs`), and of a test that makes a page ready as the window does.
pub fn make_ready(doc: &Document, page: &SnapPage, settings: &SnapSettings, moving: &[u32], kind: Kind) {
    let at = Rect { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 };
    // Any frame that has a size and numbers: the targets are keyed by whether there is one, not by what it is.
    let frame = |framed: bool| framed.then_some(OwnFrame { centre: (0.0, 0.0), width: 1.0, height: 1.0, angle: 0.5 });
    match kind {
        Kind::Box { framed } => {
            let query = Query { subject: Subject::Box(at), using: Use::Place, moving, zoom: DEFAULT_ZOOM, own_points: &[] };
            snap_ext(doc, page, settings, &query, Extras { frame: frame(framed), ..Extras::default() });
        }
        Kind::Edges { framed } => {
            // A resize that moves one side, spelled out: leaving the side out is a build error, and not a query that asks of no side.
            let edges = Edges { left: false, bottom: false, right: true, top: false };
            let query = Query { subject: Subject::Edges(at, edges), using: Use::Place, moving, zoom: DEFAULT_ZOOM, own_points: &[] };
            snap_ext(doc, page, settings, &query, Extras { frame: frame(framed), ..Extras::default() });
        }
        Kind::Point(using) => {
            let query = Query { subject: Subject::Point((0.0, 0.0)), using, moving, zoom: DEFAULT_ZOOM, own_points: &[] };
            snap_ext(doc, page, settings, &query, Extras::default());
        }
        Kind::Turn => {
            snap_turn(doc, page, settings, moving, Turn { pivot: (0.0, 0.0), angle: 0.0, radius: 1.0, zoom: DEFAULT_ZOOM });
        }
    }
}

/// One kept read.
struct Entry {
    page: u32,
    revision: u64,
    settings: SnapSettings,
    read: Arc<SnapPage>,
    /// The tick of the last time it was asked for, for the one used longest ago to go first.
    used: u64,
}

#[derive(Default)]
struct Kept {
    entries: Vec<Entry>,
    clock: u64,
}

/// The reads a session keeps: at most one for each page, and at most `cap` bytes of them by [`SnapPage::held_bytes`].
pub struct PageReads {
    cap: usize,
    kept: Mutex<Kept>,
}

impl Default for PageReads {
    fn default() -> PageReads {
        PageReads::with_cap(MAX_KEPT_BYTES)
    }
}

impl PageReads {
    /// A cache that holds at most `cap` bytes (the engine's is [`MAX_KEPT_BYTES`]; a test gives a small one to see what goes).
    pub fn with_cap(cap: usize) -> PageReads {
        PageReads { cap, kept: Mutex::new(Kept::default()) }
    }

    /// The lock, taken even when a thread panicked holding it: what it guards is a list of reads, each of which is whole or not there.
    fn lock(&self) -> MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The read of `page` kept for this revision and these settings. **A kept read for the page of another revision or settings is dropped here**: the page has one entry, and an
    /// edit has made the old one of no use.
    pub fn get(&self, revision: u64, page: u32, settings: &SnapSettings) -> Option<Arc<SnapPage>> {
        let mut kept = self.lock();
        let at = kept.entries.iter().position(|e| e.page == page)?;
        let current = kept.entries.get(at).is_some_and(|e| e.revision == revision && e.settings == *settings);
        if !current {
            kept.entries.swap_remove(at);
            return None;
        }
        kept.clock = kept.clock.saturating_add(1);
        let clock = kept.clock;
        let entry = kept.entries.get_mut(at)?;
        entry.used = clock;
        let read = Arc::clone(&entry.read);
        // The read may have grown since it was kept (the reader makes its index after the window has it): charged again, whoever is asked.
        trim(&mut kept, self.cap);
        Some(read)
    }

    /// Keeps `read` as the read of `page` for this revision and these settings, in place of the one the page had, and drops what is over the cap.
    pub fn keep(&self, revision: u64, page: u32, settings: SnapSettings, read: Arc<SnapPage>) {
        let mut kept = self.lock();
        kept.entries.retain(|e| e.page != page);
        kept.clock = kept.clock.saturating_add(1);
        let used = kept.clock;
        kept.entries.push(Entry { page, revision, settings, read, used });
        trim(&mut kept, self.cap);
    }

    /// What the kept reads hold, by [`SnapPage::held_bytes`].
    pub fn held_bytes(&self) -> usize {
        self.lock().entries.iter().map(|e| e.read.held_bytes()).sum()
    }

    /// How many pages have a kept read: at most one each.
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    /// Whether no page has a kept read.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drops every kept read.
    pub fn clear(&self) {
        self.lock().entries.clear();
    }
}

/// Drops the entry used longest ago until the sum is within `cap`. The entry used last goes only when it is over the cap alone.
fn trim(kept: &mut Kept, cap: usize) {
    let mut sizes: Vec<usize> = kept.entries.iter().map(|e| e.read.held_bytes()).collect();
    while sizes.iter().sum::<usize>() > cap {
        let Some(oldest) = kept.entries.iter().enumerate().min_by_key(|(_, e)| e.used).map(|(i, _)| i) else { break };
        kept.entries.swap_remove(oldest);
        sizes.swap_remove(oldest);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

    use super::*;

    fn doc(shapes: u32) -> diacraft_doc::Document {
        let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20");
        for i in 0..shapes {
            page = page.shape(
                ShapeBuilder::new(i + 1)
                    .cell("PinX", &(f64::from(i % 10) * 1.5 + 1.0).to_string())
                    .cell("PinY", &(f64::from(i / 10) * 1.5 + 1.0).to_string())
                    .cell("Width", "1")
                    .cell("Height", "1")
                    .cell("LocPinX", "0.5")
                    .cell("LocPinY", "0.5"),
            );
        }
        DocBuilder::new().page(1, page).build()
    }

    fn read_of(doc: &diacraft_doc::Document) -> Arc<SnapPage> {
        Arc::new(SnapPage::read(doc, 1).unwrap())
    }

    /// One entry for each page: a second read kept for the page takes the place of the first, and a read of another revision or settings is not given back (and is dropped).
    #[test]
    fn a_page_has_one_entry_and_a_read_of_another_revision_or_settings_is_dropped() {
        let doc = doc(4);
        let cache = PageReads::default();
        let settings = SnapSettings::default();
        let first = read_of(&doc);
        cache.keep(1, 1, settings, Arc::clone(&first));
        assert_eq!(cache.len(), 1);
        assert!(Arc::ptr_eq(&cache.get(1, 1, &settings).unwrap(), &first), "found for the revision, the page and the settings it was kept for");
        let second = read_of(&doc);
        cache.keep(2, 1, settings, Arc::clone(&second));
        assert_eq!(cache.len(), 1, "the page has one entry: the new one took the old one's place");
        assert!(cache.get(1, 1, &settings).is_none(), "the old revision is not found");
        assert!(cache.get(2, 1, &settings).is_none(), "and asking for it dropped the entry of the page, which was another revision's");
        assert!(cache.is_empty());
        cache.keep(2, 1, settings, Arc::clone(&second));
        let other = SnapSettings { grid: !settings.grid, ..settings };
        assert!(cache.get(2, 1, &other).is_none(), "settings that differ are another read");
        assert!(cache.is_empty());
        assert!(cache.get(2, 7, &settings).is_none(), "a page with no entry has none");
    }

    /// The memory is charged: a read that is made ready holds more, the cache's sum is the sum of its reads', and over the cap the page used longest ago goes first.
    #[test]
    fn the_sum_of_the_reads_is_charged_and_the_one_used_longest_ago_goes_over_the_cap() {
        let big = doc(400);
        let bare = read_of(&big);
        let cold = bare.held_bytes();
        let ready = read_of(&big);
        // The index and a set of targets, as the reader makes them.
        let settings = SnapSettings::default();
        let query = crate::snap::Query {
            subject: crate::snap::Subject::Box(crate::snap::Rect { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 }),
            using: crate::snap::Use::Place,
            moving: &[],
            zoom: 96.0,
            own_points: &[],
        };
        crate::snap::snap(&big, &ready, &SnapSettings { alignment_box: true, ..settings }, &query);
        let warm = ready.held_bytes();
        assert!(ready.shapes_read() && warm > cold + 400 * 100, "400 shapes in the index are charged for: {cold} bytes cold, {warm} ready");
        let cache = PageReads::with_cap(warm * 2 + warm / 2);
        for page in [1u32, 2, 3] {
            cache.keep(5, page, settings, Arc::clone(&ready));
        }
        assert_eq!(cache.len(), 2, "two reads of {warm} bytes fit in the cap, three do not");
        assert_eq!(cache.held_bytes(), warm * 2, "the sum is the reads' sum");
        assert!(cache.get(5, 1, &settings).is_none(), "the page kept first went");
        assert!(cache.get(5, 2, &settings).is_some() && cache.get(5, 3, &settings).is_some());
        // A page used again is the youngest: the next one over takes the one not used.
        let _ = cache.get(5, 2, &settings);
        cache.keep(5, 4, settings, Arc::clone(&ready));
        assert!(cache.get(5, 3, &settings).is_none(), "page 3 was used longest ago");
        assert!(cache.get(5, 2, &settings).is_some() && cache.get(5, 4, &settings).is_some());
        // A read over the cap alone is not kept at all.
        let tiny = PageReads::with_cap(warm / 2);
        tiny.keep(5, 1, settings, ready);
        assert!(tiny.is_empty());
    }

    #[test]
    fn the_counts_are_the_threads_own_and_start_again_on_reset() {
        reset_counts();
        count(|c| c.index_builds += 2);
        assert_eq!(counts().index_builds, 2);
        std::thread::spawn(|| assert_eq!(counts(), Counts::default(), "another thread has its own")).join().unwrap();
        reset_counts();
        assert_eq!(counts(), Counts::default());
    }

    /// Each price of a set of targets, one at a time and together: the charge was held only to a bound on its sum, and a price changed by a mutant passed (the
    /// re-review's survivors). The prices are measured (`adapter.rs`), and a change of them is a change of these numbers.
    #[test]
    fn each_price_of_a_set_of_targets_is_pinned_and_they_add_up() {
        use crate::snap::adapter::price;
        use craft_snap::BuildReport;
        let only = |change: fn(&mut BuildReport)| {
            let mut report = BuildReport::default();
            change(&mut report);
            price(&report)
        };
        assert_eq!(price(&BuildReport::default()), 0, "an empty set holds nothing");
        assert_eq!(only(|r| r.objects = 1), 1_600, "an object");
        assert_eq!(only(|r| r.points = 1), 160, "a point");
        assert_eq!(only(|r| r.lines = 1), 240, "a line");
        assert_eq!(only(|r| r.paths = 1), 96, "a path");
        assert_eq!(only(|r| r.path_elements = 1), 48, "an element of a path");
        let all = BuildReport { objects: 2, points: 3, lines: 5, paths: 7, path_elements: 11, ..BuildReport::default() };
        assert_eq!(price(&all), 2 * 1_600 + 3 * 160 + 5 * 240 + 7 * 96 + 11 * 48, "each is counted at its own price and they add");
        let huge =
            BuildReport { objects: u32::MAX, points: u32::MAX, lines: u32::MAX, paths: u32::MAX, path_elements: u64::MAX, ..BuildReport::default() };
        assert_eq!(price(&huge), usize::MAX, "a count past the range saturates and does not wrap");
    }

    /// Each price of an index, one at a time and together, and the half again over the sum.
    #[test]
    fn each_price_of_an_index_is_pinned_and_they_add_up() {
        use crate::snap::index::{Item, price_index};
        let item = std::mem::size_of::<Item>() + 16;
        assert_eq!(price_index(0, 0, 0, 0, 0), 0);
        assert_eq!(price_index(1, 0, 0, 0, 0), item * 3 / 2, "an item, with its path's block");
        assert_eq!(price_index(0, 1, 0, 0, 0), 96, "a bucket: 64 bytes and the half again");
        assert_eq!(price_index(0, 0, 1, 0, 0), 6, "an entry of a bucket: 4 bytes and the half again");
        assert_eq!(price_index(0, 0, 0, 1, 0), 6, "a big shape: 4 bytes and the half again");
        assert_eq!(price_index(0, 0, 0, 0, 1), 24, "an entry of the table of IDs: 16 bytes and the half again");
        assert_eq!(price_index(0, 0, 3, 0, 0), 18, "the half again is of the sum: 12 bytes make 18");
        assert_eq!(price_index(0, 0, 1, 1, 0), 12, "an entry of a bucket and a big shape: 8 bytes make 12");
        let sum = 2 * item + 3 * 64 + 5 * 4 + 7 * 4 + 11 * 16;
        assert_eq!(price_index(2, 3, 5, 7, 11), sum + sum / 2, "each at its own price, and the half again over their sum");
        assert_eq!(price_index(usize::MAX, usize::MAX, usize::MAX, usize::MAX, usize::MAX), usize::MAX, "saturates");
    }

    /// A read is charged for its own size, each of its guides, its index when it is made and its set of targets when it is built, each once: the figures of the three are added, and
    /// none is a product, a difference or counted twice.
    #[test]
    fn a_read_is_charged_for_each_of_its_guides_its_index_and_its_targets() {
        use crate::snap::{GuideLine, SlantedGuide};
        use diacraft_doc::ShapeType;
        let guide = |id: u32, degrees: f64| {
            ShapeBuilder::new(id)
                .kind(ShapeType::Guide)
                .cell("PinX", "3")
                .cell("PinY", "4")
                .cell("Width", "0")
                .cell("Height", "0")
                .cell("LocPinX", "0")
                .cell("LocPinY", "0")
                .cell("Angle", &degrees.to_radians().to_string())
        };
        let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20");
        for (id, degrees) in [(1u32, 0.0), (2, 90.0), (3, 0.0), (4, 30.0), (5, 60.0)] {
            page = page.shape(guide(id, degrees));
        }
        for i in 0..6u32 {
            page = page.shape(
                ShapeBuilder::new(10 + i)
                    .cell("PinX", &(f64::from(i) * 2.0 + 1.0).to_string())
                    .cell("PinY", "8")
                    .cell("Width", "1")
                    .cell("Height", "1")
                    .cell("LocPinX", "0.5")
                    .cell("LocPinY", "0.5"),
            );
        }
        let doc = DocBuilder::new().page(1, page).build();
        let read = SnapPage::read(&doc, 1).unwrap();
        assert_eq!((read.guides.len(), read.slanted.len()), (3, 2), "three guides along an axis and two along none");
        let own = std::mem::size_of::<SnapPage>() + 3 * std::mem::size_of::<GuideLine>() + 2 * std::mem::size_of::<SlantedGuide>();
        assert_eq!(read.held_bytes(), own, "the index and the targets are not made yet: the read, its three guides and its two slanted ones");
        let index = read.shapes(&doc).held_bytes();
        // The index of six shapes holds at least the six items and their six `ID`s, one bucket and an entry of it for each, at the prices of `price_index`, and no more than the most
        // buckets six small boxes can fall in (a bucket for each of the four corners of each box, at the most): not the figure of a function that says 0 or 1.
        let (least, most) = (crate::snap::index::price_index(6, 1, 6, 0, 6), crate::snap::index::price_index(6, 24, 24, 6, 6));
        assert!((least..=most).contains(&index), "the index of six shapes is charged {index} bytes, between {least} and {most}");
        assert_eq!(read.held_bytes(), own + index, "the index, once made, is added");
        let settings = SnapSettings { alignment_box: true, ..SnapSettings::default() };
        crate::snap::make_ready(&doc, &read, &settings, &[10], crate::snap::Kind::Box { framed: false });
        let set = crate::snap::adapter::price(&read.target_report().unwrap());
        assert!(set >= 5 * 1_600, "five shapes are targets: {set}");
        assert_eq!(read.held_bytes(), own + index + set, "the set of targets, once built, is added: the three are one sum");
    }

    /// The cap of a session's reads is the constant, and the constant is 64 MiB: a cache made by default drops the page used longest ago when the reads it holds pass that, and not
    /// before.
    #[test]
    fn the_cap_of_the_reads_a_session_keeps_is_sixty_four_mebibytes() {
        assert_eq!(MAX_KEPT_BYTES, 67_108_864);
        assert_eq!(PageReads::default().cap, MAX_KEPT_BYTES);
        let big = doc(400);
        let ready = read_of(&big);
        let settings = SnapSettings { alignment_box: true, ..SnapSettings::default() };
        crate::snap::make_ready(&big, &ready, &settings, &[1], crate::snap::Kind::Box { framed: false });
        let each = ready.held_bytes();
        let fit = 67_108_864 / each;
        assert!((20..400).contains(&fit), "a page of 400 shapes is a small part of the cap: {each} bytes, {fit} fit");
        let cache = PageReads::default();
        for page in 1..=u32::try_from(fit).unwrap() {
            cache.keep(5, page, SnapSettings::default(), Arc::clone(&ready));
        }
        assert_eq!(cache.len(), fit, "as many as fit are kept");
        cache.keep(5, u32::try_from(fit).unwrap() + 1, SnapSettings::default(), Arc::clone(&ready));
        assert_eq!(cache.len(), fit, "one more takes the place of the one used longest ago");
        assert!(cache.held_bytes() <= 67_108_864 && cache.held_bytes() + each > 67_108_864, "the sum is within the cap, and one more would pass it");
    }

    /// `make_ready` makes, for each kind of query, what a query of that kind asks for: the index of the shapes and **one** set of targets, which the query that comes after finds
    /// (the survivor of the re-review that left `right` out of the edges a ready page is asked for: a page made ready for a resize that asked of no side built no set).
    #[test]
    fn make_ready_makes_the_index_and_the_one_set_of_targets_each_kind_of_query_asks_for() {
        let doc = doc(30);
        let settings = SnapSettings { alignment_box: true, ..SnapSettings::default() };
        let frame = OwnFrame { centre: (3.0, 3.0), width: 1.0, height: 1.0, angle: 0.5 };
        let rect = Rect { x0: 2.0, y0: 2.0, x1: 3.0, y1: 3.0 };
        let ask = |page: &SnapPage, kind: Kind| match kind {
            Kind::Box { framed } => {
                let query = Query { subject: Subject::Box(rect), using: Use::Place, moving: &[1], zoom: 96.0, own_points: &[] };
                snap_ext(&doc, page, &settings, &query, Extras { frame: framed.then_some(frame), ..Extras::default() });
            }
            Kind::Edges { framed } => {
                let edges = Edges { right: true, ..Edges::default() };
                let query = Query { subject: Subject::Edges(rect, edges), using: Use::Place, moving: &[1], zoom: 96.0, own_points: &[] };
                snap_ext(&doc, page, &settings, &query, Extras { frame: framed.then_some(frame), ..Extras::default() });
            }
            Kind::Point(using) => {
                let query = Query { subject: Subject::Point((3.0, 3.0)), using, moving: &[1], zoom: 96.0, own_points: &[] };
                snap_ext(&doc, page, &settings, &query, Extras::default());
            }
            Kind::Turn => {
                snap_turn(&doc, page, &settings, &[1], Turn { pivot: (3.0, 3.0), angle: 0.7, radius: 1.5, zoom: 96.0 });
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
            let read = read_of(&doc);
            assert!(!read.shapes_read() && read.target_builds() == 0, "{kind:?}: nothing is made before the work");
            make_ready(&doc, &read, &settings, &[1], kind);
            assert!(read.shapes_read(), "{kind:?}: the index of the shapes is made");
            assert_eq!(read.target_builds(), 1, "{kind:?}: one set of targets");
            ask(&read, kind);
            assert_eq!(read.target_builds(), 1, "{kind:?}: the query that comes after builds no other set");
        }
    }

    /// A cache holds reads up to its cap and not a byte more: two reads of `each` bytes are kept by a cap of exactly `2 * each`, and one is dropped by a cap of a byte under it.
    #[test]
    fn a_cache_holds_reads_up_to_its_cap_exactly_and_not_a_byte_more() {
        let big = doc(400);
        let ready = read_of(&big);
        let settings = SnapSettings { alignment_box: true, ..SnapSettings::default() };
        crate::snap::make_ready(&big, &ready, &settings, &[1], crate::snap::Kind::Box { framed: false });
        let each = ready.held_bytes();
        let kept = |cap: usize| {
            let cache = PageReads::with_cap(cap);
            for page in [1u32, 2] {
                cache.keep(5, page, SnapSettings::default(), Arc::clone(&ready));
            }
            cache.len()
        };
        assert_eq!(kept(each * 2), 2, "two reads of {each} bytes are within a cap of {}", each * 2);
        assert_eq!(kept(each * 2 - 1), 1, "and not within a cap of a byte less");
    }

    /// The process's anonymous resident memory in bytes (`RssAnon` of its status file): what the heap holds, which is what the charge is for.
    fn anon_bytes() -> u64 {
        let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        text.lines()
            .find_map(|l| l.strip_prefix("RssAnon:"))
            .and_then(|r| r.split_whitespace().next())
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0)
            * 1024
    }

    /// A measurement, not a test: what the library's set of targets holds for each thing the adapter gives it, by the process's resident memory, building `sets` sets of `n` of the
    /// thing `what` (0 objects upright, 1 objects turned, 2 points, 3 lines) and keeping them. Run with `--ignored --nocapture`.
    #[test]
    #[ignore = "a measurement, not a test: SNAP_MEASURE_KIND=0..3 SNAP_MEASURE_N=<things> prints bytes a thing; run with --ignored --nocapture"]
    fn measure_what_the_library_holds_for_each_part() {
        use craft_snap as cs;
        let n: u32 = std::env::var("SNAP_MEASURE_N").ok().and_then(|v| v.parse().ok()).unwrap_or(10_000);
        let what: u32 = std::env::var("SNAP_MEASURE_KIND").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        let sets = 9u32;
        let mut kept = Vec::new();
        let mut bytes = 0u64;
        for s in 0..sets {
            let before = anon_bytes();
            let mut b = cs::TargetsBuilder::new(cs::Limits::default());
            for i in 0..n {
                let (x, y) = (f64::from(i % 100) * 1.5, f64::from(i / 100) * 1.5);
                match what {
                    0 | 1 => {
                        let frame = cs::TurnedBox {
                            centre: kurbo::Point::new(x + 0.5, y + 0.5),
                            size: kurbo::Size::new(1.0, 1.0),
                            angle: if what == 1 { 0.5 } else { 0.0 },
                        };
                        let parts = cs::Parts {
                            bounds_lines: true,
                            own_lines: true,
                            spacing: true,
                            size: true,
                            angle: true,
                            corners: true,
                            side_middles: true,
                            detail: true,
                        };
                        let _ = b.object(cs::Id(u64::from(i)), cs::Object { frame, bounds: Some(frame.bounds()), angle: Some(frame.angle), parts });
                    }
                    2 => {
                        let _ = b.point(Some(cs::Id(u64::from(i))), kurbo::Point::new(x, y), cs::Kind::Corner, 0);
                    }
                    _ => {
                        let _ = b.line(Some(cs::Id(u64::from(i))), cs::Axis::X, x + f64::from(i) * 1e-3, None, cs::Kind::Guide);
                    }
                }
            }
            let built = b.build();
            let after = anon_bytes();
            if s > 0 {
                bytes += after.saturating_sub(before);
            }
            kept.push(built);
        }
        let report = *kept.last().unwrap().report();
        eprintln!("MEASURE part={what} n={n}: {:.0} bytes a thing; report {report:?}", bytes as f64 / f64::from(sets - 1) / f64::from(n));
    }

    /// `pages` pages of `n` shapes each, of the kind `kind`: 0 upright 1 by 1 boxes, 1 the same turned (no paths), 2 stock shapes turned, 3 stock shapes upright.
    fn measured_doc(pages: u32, n: u32, kind: u32) -> diacraft_doc::Document {
        let masters: Vec<_> = crate::stencil::all_masters().filter(|m| !m.is_connector()).collect();
        let mut doc = diacraft_testkit::fixtures::blank_document();
        let held: Vec<u32> = masters.iter().map(|m| crate::draw::drop_shape(&mut doc, 1, m, 0.0, 0.0).unwrap().master).collect();
        let mut built = DocBuilder::new();
        for p in 1..=pages {
            let mut page = PageBuilder::new().cell("PageWidth", "400").cell("PageHeight", "400");
            for i in 0..n {
                let mut shape = ShapeBuilder::new(i + 1)
                    .cell("PinX", &(f64::from(i % 100) * 1.5 + 1.0).to_string())
                    .cell("PinY", &(f64::from(i / 100) * 1.5 + 1.0).to_string());
                shape = match kind {
                    0 | 1 => shape.cell("Width", "1").cell("Height", "1").cell("LocPinX", "0.5").cell("LocPinY", "0.5"),
                    _ => shape.master(held[i as usize % held.len()]),
                };
                if kind == 1 || kind == 2 {
                    shape = shape.cell("Angle", &(0.2 + f64::from(i % 7) * 0.19).to_string());
                }
                page = page.shape(shape);
            }
            built = built.page(p, page);
        }
        doc.pages = built.build().pages;
        doc
    }

    /// What `pages` pages of `n` shapes of a kind hold when read and made ready, page after page in one process: by the process's resident memory (bytes a page, the first page's work
    /// left out, since it makes the heap's own growth), and by the estimate the cache charges.
    struct Held {
        index_real: f64,
        targets_real: f64,
        charged: usize,
        charged_index: usize,
        report: craft_snap::BuildReport,
        truncated: bool,
        by_corners: usize,
    }

    fn held_by_pages(pages: u32, n: u32, kind: u32) -> Held {
        let doc = measured_doc(pages, n, kind);
        let settings = SnapSettings { alignment_box: true, snap_handles: true, ..SnapSettings::default() };
        let mut reads = Vec::new();
        let (mut index_bytes, mut target_bytes, mut counted) = (0u64, 0u64, 0u64);
        for p in 1..=pages {
            let page = Arc::new(SnapPage::read(&doc, p).unwrap());
            let after_read = anon_bytes();
            let _ = page.shapes(&doc);
            let after_index = anon_bytes();
            crate::snap::make_ready(&doc, &page, &settings, &[1], crate::snap::Kind::Box { framed: false });
            let after_targets = anon_bytes();
            if p > 1 {
                counted += 1;
                index_bytes += after_index.saturating_sub(after_read);
                target_bytes += after_targets.saturating_sub(after_index);
            }
            reads.push(page);
        }
        let last = reads.last().unwrap();
        let index = last.shapes(&doc);
        Held {
            index_real: index_bytes as f64 / counted as f64,
            targets_real: target_bytes as f64 / counted as f64,
            charged: last.held_bytes(),
            charged_index: index.held_bytes(),
            report: last.target_report().unwrap(),
            truncated: index.truncated,
            by_corners: index.bounds_by_corners,
        }
    }

    /// A measurement, not a test: what a page's index and set of targets hold, by the estimate and by the process's resident memory, page after page in one process. Run with
    /// `--ignored --nocapture`; the prices of `adapter.rs` and `index.rs` were set from it, and it is run again to see that the charge is over what is held.
    #[test]
    #[ignore = "a measurement, not a test: SNAP_MEASURE_KIND=0..3 SNAP_MEASURE_N=<shapes> prints bytes per page by the estimate and by RssAnon; run with --ignored --nocapture"]
    fn measure_what_a_kept_read_holds() {
        let n: u32 = std::env::var("SNAP_MEASURE_N").ok().and_then(|v| v.parse().ok()).unwrap_or(10_000);
        let kind: u32 = std::env::var("SNAP_MEASURE_KIND").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        let held = held_by_pages(9, n, kind);
        let items = std::mem::size_of::<crate::snap::index::Item>();
        eprintln!(
            "MEASURE kind={kind} n={n}: index real {:.0} bytes a shape ({} held by the estimate), targets real {:.0} bytes a shape; estimate of the page {} bytes ({:.1} MiB); the real index and targets {:.1} MiB a page; size_of Item {items}; report {:?}; truncated {} by corners {}",
            held.index_real / f64::from(n),
            held.charged_index / n as usize,
            held.targets_real / f64::from(n),
            held.charged,
            held.charged as f64 / 1048576.0,
            (held.index_real + held.targets_real) / 1048576.0,
            held.report,
            held.truncated,
            held.by_corners,
        );
    }

    /// **The charge is over what a page holds** (of the re-review, which measured 12 to 18 MiB held a page against 6.1 charged): five pages of 4,000 shapes
    /// of the kinds the measurement found dearest and cheapest, in one process (a test of nextest is a process of its own), the index and the targets by the resident memory against
    /// the charge. Where the process cannot say its memory (no `/proc`), there is nothing to compare. **Run by hand** (the two tests below are ignored: what a process holds depends
    /// on its allocator, and the margin of the dearest kind, 1.26 on this machine's, is 1.05 under `GLIBC_TUNABLES=glibc.malloc.hugetlb=1`); see the module's "To check the prices".
    fn assert_charged_over_held(kind: u32) {
        let held = held_by_pages(6, 4_000, kind);
        let real = held.index_real + held.targets_real;
        if real == 0.0 {
            return;
        }
        assert!(
            real <= held.charged as f64,
            "kind {kind}: a page holds {:.1} MiB (index {:.1}, targets {:.1}) and is charged {:.1} MiB",
            real / 1048576.0,
            held.index_real / 1048576.0,
            held.targets_real / 1048576.0,
            held.charged as f64 / 1048576.0
        );
    }

    #[test]
    #[ignore = "resident-memory measurement; allocator-dependent"]
    fn a_page_of_upright_shapes_is_charged_more_than_it_holds_in_memory() {
        assert_charged_over_held(0);
    }

    #[test]
    #[ignore = "resident-memory measurement; allocator-dependent"]
    fn a_page_of_turned_stock_shapes_is_charged_more_than_it_holds_in_memory() {
        assert_charged_over_held(2);
    }
}
