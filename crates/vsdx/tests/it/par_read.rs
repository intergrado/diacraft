//! **A drawing read on threads is the drawing read on one** (`Limits::threads`, `ahead.rs`): the document, the report and every refusal are the single-threaded read's at 2, 7 and
//! 31 threads. The pages and masters of these drawings are parts large enough to be parsed ahead of the read's turn for them (16 KiB and over), so the threads do work.

use diacraft_testkit::package::{MasterSpec, PageSpec, Pkg, VsdxBuilder, typical_shape};
use diacraft_vsdx::{Limits, VsdxError, read_partial};

const THREADS: [usize; 4] = [1, 2, 7, 31];

/// A drawing of `pages` pages of `per_page` shapes each and `masters` masters of 40 shapes, in which every page part is far over the size a thread is asked for.
fn drawing(pages: u32, per_page: u32, masters: u32) -> Vec<u8> {
    let mut builder = VsdxBuilder::new();
    for m in 1..=masters {
        let body: String = (0..40).map(|i| typical_shape(m * 1_000 + i)).collect();
        builder = builder.master(MasterSpec::new(m, &format!("Master {m}")).shapes(&body));
    }
    for page in 1..=pages {
        let body: String = (0..per_page).map(|i| typical_shape((page - 1) * per_page + i + 1)).collect();
        builder = builder.page(PageSpec::new(page, &format!("Page {page}")).shapes(&body));
    }
    builder.build()
}

fn with_threads(limits: &Limits, threads: usize) -> Limits {
    Limits { threads, ..limits.clone() }
}

/// What a read comes to: the document or the error, and the report either way.
type Outcome = (Result<diacraft_doc::Document, VsdxError>, diacraft_vsdx::ReadReport);

fn outcome(bytes: &[u8], limits: &Limits, threads: usize) -> Outcome {
    read_partial(bytes, &with_threads(limits, threads))
}

/// The trees a read took from the threads that parsed them ahead, of the parts of `bytes` (a count of work and not a time: this thread's).
fn taken_ahead(bytes: &[u8], limits: &Limits, threads: usize) -> u64 {
    diacraft_vsdx::work::reset();
    let _ = outcome(bytes, limits, threads);
    diacraft_vsdx::work::trees_taken_ahead()
}

/// The reads at every count of threads are the read at one: the document (by value), the report (all of it: the figures of the budgets and the peak included, since what a thread has set aside is
/// not charged) or the error, and where the read was when it refused.
fn assert_alike(bytes: &[u8], limits: &Limits, what: &str) -> Outcome {
    let one = outcome(bytes, limits, 1);
    for threads in THREADS.iter().copied().filter(|t| *t > 1) {
        let many = outcome(bytes, limits, threads);
        assert_eq!(many.0, one.0, "{what}: the result at {threads} threads");
        assert_eq!(many.1, one.1, "{what}: the report at {threads} threads");
    }
    one
}

#[test]
fn a_drawing_of_many_pages_and_masters_is_the_same_document_and_report_at_any_number_of_threads() {
    let bytes = drawing(24, 60, 6);
    let (doc, report) = assert_alike(&bytes, &Limits::default(), "24 pages, 6 masters");
    let doc = doc.expect("the drawing reads");
    assert_eq!((doc.pages.len(), doc.masters.len()), (24, 6));
    assert_eq!(report.stats.shapes, 24 * 60 + 6 * 40);
    // The threads did the work: at one thread the read parses every part itself, and at more it takes the trees of most of them (a part whose thread had not begun when the read came for it is
    // the read's, so not all of them are taken, and which depends on the machine: the figure is a floor).
    assert_eq!(taken_ahead(&bytes, &Limits::default(), 1), 0);
    let taken = taken_ahead(&bytes, &Limits::default(), 7);
    assert!(taken >= 2, "7 threads took {taken} trees of 30 parts");
}

#[test]
fn a_drawing_of_a_few_large_pages_and_one_of_one_small_page_are_alike_too() {
    let _ = assert_alike(&drawing(3, 400, 0), &Limits::default(), "3 pages of 400");
    let _ = assert_alike(&drawing(1, 3, 0), &Limits::default(), "one page of 3 shapes: no part is large enough to be parsed ahead");
}

#[test]
fn a_page_part_that_two_pages_name_and_one_that_is_not_xml_are_read_alike() {
    // Two rows name one part (`parse_content_part` shares it and charges the budgets again), and a third row's part is not well-formed: the refusal is the same.
    let page: String = (0..80).map(typical_shape).collect();
    let shared = VsdxBuilder::new()
        .page(PageSpec::new(1, "One").shapes(&page))
        .page(PageSpec::new(2, "Two").shapes(&page))
        .page(PageSpec::new(3, "Three").shapes(&page))
        .build();
    let _ = assert_alike(&shared, &Limits::default(), "pages with equal parts");
    let broken = VsdxBuilder::new()
        .page(PageSpec::new(1, "One").shapes(&page))
        .page(
            PageSpec::new(2, "Two")
                .part(&format!("<PageContents xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\"><Shapes>{page}</Shapes>")),
        )
        .page(PageSpec::new(3, "Three").shapes(&page))
        .build();
    let (result, report) = assert_alike(&broken, &Limits::default(), "a page part that is not well formed");
    let error = result.expect_err("the damaged part refuses the file");
    assert_eq!(error.class(), "xml_malformed", "{error}");
    assert!(report.refused_at.is_some());
}

#[test]
fn a_part_denser_than_a_thread_is_given_the_room_for_is_parsed_by_the_read_and_is_the_same() {
    // Elements of one letter with no attribute are four bytes an item: the thread is held to what a real page is (one item to nine bytes), refuses the parse, and the read parses it itself.
    let dense: String = "<S/>".repeat(40_000);
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "Dense").part(&format!(
            "<PageContents xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\"><Shapes>{}</Shapes>{dense}</PageContents>",
            typical_shape(1)
        )))
        .page(PageSpec::new(2, "Real").shapes(&(0..120).map(typical_shape).collect::<String>()))
        .build();
    let _ = assert_alike(&bytes, &Limits::default(), "a part of 40,000 elements of four bytes");
}

#[test]
fn a_refusal_for_any_cap_is_the_same_words_and_the_same_report_at_any_number_of_threads() {
    // **A read that is refused at one thread is refused at 31, with the same words and the same report, and one that is read at one is read at 31**: the caps that refuse inside a part (items,
    // cells, the shapes of the document, the bytes of XML, the nodes of formulas) with parts that the threads parse ahead. (The memory budget is walked from too little to enough by the test
    // of what the threads hold, below.)
    let bytes = small_pages(40, 24, 3);
    assert!(taken_ahead(&bytes, &Limits::default(), 7) > 5, "the threads parse these parts ahead");
    for (what, limits) in [
        ("items", Limits { max_items: 9_000, ..Limits::default() }),
        ("cells", Limits { max_cells: 3_000, ..Limits::default() }),
        ("shapes", Limits { max_shapes: 300, ..Limits::default() }),
        ("xml bytes", Limits { max_xml_bytes: 700_000, ..Limits::default() }),
        ("formula nodes", Limits { max_formula_nodes: 2_000, ..Limits::default() }),
    ] {
        let (result, _) = assert_alike(&bytes, &limits, what);
        assert!(result.is_err() || what == "formula nodes", "{what} should refuse");
    }
}

#[test]
fn a_package_with_a_damaged_entry_is_refused_alike_at_any_number_of_threads() {
    // A byte of the compressed data of a part is flipped: the entry does not inflate to what it says, whichever thread inflates it, and the error is the first entry's in the directory.
    let bytes = drawing(8, 60, 2);
    let mut damaged = bytes.clone();
    let at = damaged.len() / 2;
    damaged[at] ^= 0xFF;
    let (result, _) = assert_alike(&damaged, &Limits::default(), "a damaged entry");
    assert!(result.is_err());
}

#[test]
fn a_thread_that_panics_is_an_error_in_words_and_the_read_goes_on_at_one_thread() {
    let page: String = (0..80).map(typical_shape).collect();
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "One").shapes(&page)).page(PageSpec::new(2, "Two").shapes(&page)).with(Pkg::new()).build();
    // A comment in the part makes the thread that parses it panic (`ahead.rs`, with the `test-support` feature).
    let marked = format!(
        "<PageContents xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\"><!--panic-in-ahead--><Shapes>{page}</Shapes></PageContents>"
    );
    let panicking = VsdxBuilder::new().page(PageSpec::new(1, "One").shapes(&page)).page(PageSpec::new(2, "Two").part(&marked)).build();
    let (alone, _) = outcome(&panicking, &Limits::default(), 1);
    assert!(alone.is_ok(), "no thread, no panic: the read parses the part itself");
    // The read waits for the threads to parse the parts it asked them for, so that a thread is the one that meets the mark: a read parses a part no thread has begun itself, and on one
    // core the read came to the second page before any thread had begun it, read it whole and failed this test (under `taskset -c 0`).
    let _waits = WaitForTheWorkAhead::on();
    for threads in [2, 7, 31] {
        let (result, report) = outcome(&panicking, &Limits::default(), threads);
        let error = result.expect_err("a thread that panicked is an error");
        assert_eq!(error.class(), "read_worker_failed", "{error}");
        assert!(error.to_string().contains("the file was not read"), "{error}");
        assert!(report.refused_at.is_some(), "the report says where the read was");
    }
    let (fine, _) = outcome(&bytes, &Limits::default(), 7);
    assert!(fine.is_ok(), "a read of a file that makes no thread panic is not affected");
}

/// Makes a read on this thread wait for the threads to parse the parts it asked them for while it is held (`diacraft_vsdx::work::wait_for_the_work_ahead`), and stops that when it is dropped,
/// so that a test that fails leaves no switch on for the next test on the thread.
struct WaitForTheWorkAhead;

impl WaitForTheWorkAhead {
    fn on() -> WaitForTheWorkAhead {
        diacraft_vsdx::work::wait_for_the_work_ahead(true);
        WaitForTheWorkAhead
    }
}

impl Drop for WaitForTheWorkAhead {
    fn drop(&mut self) {
        diacraft_vsdx::work::wait_for_the_work_ahead(false);
    }
}

/// Pages of `per` shapes each, small enough (about 2.7 KB a shape) that a budget at the edge of what the read needs still has room for a tree or two beside the model, and a row that is no page
/// (its `ID` is no number, so the read skips it) after every `skip_every`th page, whose part is as large and **is parsed ahead and never asked for by the read**: its tree is abandoned in flight.
fn small_pages(pages: u32, per: u32, skip_every: u32) -> Vec<u8> {
    let body = |n: u32| -> String { (0..per).map(|i| typical_shape(n * 1_000 + i)).collect() };
    let mut builder = VsdxBuilder::new();
    for n in 1..=pages {
        builder = builder.page(PageSpec::new(n, &format!("Page {n}")).shapes(&body(n)));
        if skip_every > 0 && n % skip_every == 0 {
            let mut skipped = PageSpec::new(n, "Not a page").shapes(&body(n + 500));
            skipped.id = "not-a-number".to_owned();
            builder = builder.page(skipped);
        }
    }
    builder.build()
}

#[test]
fn the_memory_the_threads_hold_is_never_more_than_the_budget_set_aside_for_it_whatever_the_read_abandons() {
    // The work ahead tells the budget's door every time what is set aside changes, and a test build holds the bytes of the trees in flight (parsed and not taken, and handed over to be dropped and
    // not dropped) to it: **the budget's bytes left plus the memory of every tree in flight are never more than the budget**, which is to say the bytes of a tree in flight are always set
    // aside. It is counted at every change, over reads that abandon parts (rows the read skips), reads that are refused at each edge of the budget with trees in flight, and reads whose charges
    // end the work early: the budget is walked from half of what the model needs to twice that, so that each of these happens, which the test counts.
    diacraft_vsdx::work::reset();
    let before = diacraft_vsdx::work::lookahead_violations();
    let (mut refused, mut read, mut taken, mut abandoned, mut ended) = (0u32, 0u32, 0u64, 0u64, 0u64);
    for (what, bytes) in [("abandoned parts", small_pages(40, 24, 3)), ("many pages", small_pages(50, 28, 0))] {
        let (ok, report) = outcome(&bytes, &Limits::default(), 1);
        assert!(ok.is_ok(), "{what}");
        // What the read needs of the budget at the least: the file, the package and the room a growing list takes, beside the model and the tree at their peak.
        let base = bytes.len() as u64 + report.stats.uncompressed_bytes + diacraft_vsdx::cost::GROWTH_BYTES;
        for threads in [2, 31] {
            for step in 0..=30u64 {
                let max_memory = base + report.stats.memory_peak / 2 + report.stats.memory_peak * 3 / 2 * step / 30;
                let limits = Limits { max_memory, ..Limits::default() };
                let (alone, alone_report) = outcome(&bytes, &limits, 1);
                diacraft_vsdx::work::reset();
                let (result, report) = outcome(&bytes, &limits, threads);
                (taken, abandoned, ended) = (
                    taken + diacraft_vsdx::work::trees_taken_ahead(),
                    abandoned + diacraft_vsdx::work::trees_abandoned(),
                    ended + diacraft_vsdx::work::works_ended_for_memory(),
                );
                assert_eq!(result, alone, "{what}: {threads} threads, max_memory {max_memory}");
                assert_eq!(report, alone_report, "{what}: {threads} threads, max_memory {max_memory}");
                if result.is_ok() { read += 1 } else { refused += 1 }
            }
        }
    }
    assert!(refused > 10 && read > 10, "the walk crosses the edge of the budget: {refused} refused, {read} read");
    assert!(taken > 40, "the threads did the work in the reads of the walk: {taken} trees taken");
    assert!(abandoned > 8, "trees were in flight when the read ended the work: {abandoned} abandoned");
    assert!(ended > 3, "charges ended the work for want of memory: {ended}");
    assert_eq!(diacraft_vsdx::work::lookahead_violations(), before, "a tree was in flight with its bytes not set aside");
}

const CHILD: &str = "PAR_READ_CHILD";

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0)
}

/// The child of the test below: a drawing of `shapes:pages` read on `threads` with `memory` bytes of budget (0 is the reader's own), under `ulimit -v 3000000`; prints how it ended, the trees the
/// threads parsed ahead of the read and the address space it reached.
#[test]
#[ignore = "run by the test below in a process of its own, under an address-space limit"]
fn par_read_child() {
    let Ok(spec) = std::env::var(CHILD) else { return };
    let parts: Vec<u64> = spec.split(':').map(|p| p.parse().unwrap()).collect();
    let (shapes, pages, threads, memory) = (parts[0] as usize, parts[1] as usize, parts[2] as usize, parts[3]);
    let per = shapes.div_ceil(pages);
    let mut builder = VsdxBuilder::new();
    for page in 0..pages {
        let body: String = (page * per..((page + 1) * per).min(shapes)).map(|i| typical_shape(i as u32 + 1)).collect();
        builder = builder.page(PageSpec::new(page as u32 + 1, &format!("Page {}", page + 1)).shapes(&body));
    }
    let bytes = builder.build();
    drop(builder);
    let build_peak = status_kib("VmPeak") >> 10;
    let mut limits = Limits { threads, ..Limits::default() };
    if memory > 0 {
        limits.max_memory = memory;
    }
    diacraft_vsdx::work::reset();
    let (result, _) = read_partial(&bytes, &limits);
    let ended = match &result {
        Ok(doc) => format!("read {} shapes", doc.pages.iter().map(|p| p.shapes.len()).sum::<usize>()),
        Err(e) => format!("refused {}", e.class()),
    };
    println!(
        "PARREAD threads={threads} build_peak={build_peak} taken={} vm_peak_mib={} ended={ended}",
        diacraft_vsdx::work::trees_taken_ahead(),
        status_kib("VmPeak") >> 10
    );
    println!("par read child: no abort");
}

/// What a child ended as, the trees its threads parsed ahead, the address space it reached, and the address space its drawing took to build.
fn child(shapes: usize, pages: usize, threads: usize, memory: u64) -> (String, u64, u64, u64) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = "ulimit -v 3000000 && exec \"$0\" --exact par_read::par_read_child --ignored --nocapture --test-threads=1";
    let out = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg(exe)
        .env(CHILD, format!("{shapes}:{pages}:{threads}:{memory}"))
        .output()
        .expect("run /bin/sh");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("no abort"), "{shapes}:{pages}:{threads}:{memory}: the child aborted or failed: {text}");
    let line = text.lines().find_map(|l| l.find("PARREAD ").map(|at| l[at..].to_owned())).expect("a line");
    eprintln!("{line}");
    let field = |name: &str| line.split_whitespace().find_map(|w| w.strip_prefix(name)).unwrap().parse::<u64>().unwrap();
    let ended = line.split_once("ended=").unwrap().1.to_owned();
    (ended, field("taken="), field("vm_peak_mib="), field("build_peak="))
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_large_drawing_is_read_alike_on_one_thread_and_on_thirty_one_under_the_address_limit_and_never_aborts() {
    // **At the reader's own budget the address space has room for at most a thread beside what the read may hold** (2.6 GB of the 3.07 GB): a read that is asked for 31 threads makes none or one, and
    // ends as the single-threaded read does, in words, in the address space of it and of the thread's own arena and the trees that thread has in flight (a drop window of 64 MiB and a window of parsed trees), for the largest drawing the budget admits and for one it refuses.
    for (shapes, pages) in [(76_000usize, 250usize), (90_000, 300)] {
        let (alone, _, alone_peak, build) = child(shapes, pages, 1, 0);
        let (many, _, many_peak, _) = child(shapes, pages, 31, 0);
        assert_eq!(many, alone, "{shapes} shapes: the same end at 31 threads as at 1");
        assert!(many_peak <= alone_peak + 400, "{shapes} shapes: {many_peak} MiB at 31 threads, {alone_peak} at 1 (the drawing alone takes {build})");
    }
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_read_with_room_for_threads_beside_its_budget_uses_them_and_reaches_no_more_than_the_threads_and_the_trees_take() {
    // A budget of 1.2 GB leaves room under 3.07 GB for the threads that work ahead, and a drawing that fits it is read by them (the trees they parsed are counted): the same document at 1 and 31
    // threads, and the address space of the read at 31 is the one at 1 and the stacks and arenas of at most five workers (66 MiB of address space each), and the trees in flight, which the
    // budget set aside (at most a quarter of what it had left).
    let memory = 1_200_000_000;
    let (alone, _, alone_peak, build) = child(30_000, 120, 1, memory);
    let (many, taken, many_peak, _) = child(30_000, 120, 31, memory);
    assert_eq!(many, alone, "the same end at 31 threads as at 1");
    assert!(taken > 20, "the threads parsed {taken} trees ahead of the read");
    assert!(many_peak <= alone_peak + 5 * 70 + 300, "{many_peak} MiB at 31 threads, {alone_peak} at 1 (the drawing alone takes {build})");
}
