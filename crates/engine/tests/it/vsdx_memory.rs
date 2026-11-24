//! What the engine reads beside what the process already holds: **the memory a read may use is one budget for the process, and each
//! read is given what the open drawings, their files and the open stencils of every session in it leave of it.** The reader charges its own read against `Limits::max_memory`; a read
//! with the default budget (2.6 GB) beside a drawing of 80,000 shapes (2.2 GB in its budget) was a process of 3.3 GB, and a process under `ulimit -v 3000000` aborted, whether the drawing
//! was held by the session that reads or by another one (the window reads a file in a session of its own, and draws the tiles of a stencil from a second one). Each case runs in a
//! child under the limit, as `odg_memory.rs` runs its children, and ends as a read or as a refusal in words that say what is held, and never as an abort.

#![cfg(target_os = "linux")]

use std::process::Command;
use std::sync::{Arc, Barrier};

use diacraft_engine::memory;
use diacraft_engine::{EngineError, HeldMemory, Session};
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder, typical_drawing, typical_shape};
use serde_json::json;

use crate::odg_build::Dir;

const SCENARIO: &str = "VSDX_ENGINE_MEMORY_SCENARIO";
const FIRST: &str = "VSDX_ENGINE_MEMORY_FIRST";
const SECOND: &str = "VSDX_ENGINE_MEMORY_SECOND";
const STENCIL_TYPE: &str = "application/vnd.ms-visio.stencil.main+xml";

/// The address space a child may reach, in KiB, and the share of it a refusal must stay under: the budget leaves what no price counts (`limits.rs`), and **a process that has
/// reached more than 90 percent of the limit has used that share and more**. The widest of these cases is 56 percent.
const LIMIT_KIB: u64 = 3_000_000;
const MOST_KIB: u64 = LIMIT_KIB / 100 * 90;

/// A line of `/proc/self/status`, in kibibytes: a missing one is a failure and not a zero.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("this test needs /proc/self/status (Linux): {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or_else(|| panic!("no {key} line in /proc/self/status"))
}

/// A drawing of twenty pages of 4,000 shapes of a diagram each (80,000 shapes, about the most the reader admits of them: `limits.rs`): a model of some 2.2 GB in the reader's
/// budget and a file of a few megabytes. The reader admits it alone, in a process of 1.7 GB of address space, and not beside another like it (the second read, with the budget
/// it had, was 3.3 GB).
fn heavy(document_type: Option<&str>) -> Vec<u8> {
    typical_drawing(20, 4_000, document_type)
}

/// A drawing of one shape of a diagram, and the file it is written as.
fn small(document_type: Option<&str>) -> Vec<u8> {
    let mut builder = VsdxBuilder::new();
    if let Some(content_type) = document_type {
        builder = builder.document_type(content_type);
    }
    builder.page(PageSpec::new(1, "P").shapes(&typical_shape(1))).build()
}

const TEMPLATE_TYPE: &str = "application/vnd.ms-visio.template.main+xml";

/// `n` bytes that do not compress.
fn noise(n: usize, mut seed: u64) -> Vec<u8> {
    (0..n)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 24) as u8
        })
        .collect()
}

/// A picture on a page, by the relationship `rId{k}`.
fn foreign(k: u32) -> String {
    format!("<Shape ID=\"{k}\" Type=\"Foreign\"><ForeignData ForeignType=\"Bitmap\"><Rel r:id=\"rId{k}\"/></ForeignData></Shape>")
}

/// `pages` pages of `per_page` shapes of a diagram, and **a last page that shows `pictures` pictures of `mib` MiB each**: the XML is first and the pictures are last, so that a read refused
/// on the way has taken in none of them.
fn shapes_then_pictures(pages: u32, per_page: u32, pictures: u32, mib: usize) -> Vec<u8> {
    let mut builder = VsdxBuilder::new();
    // Every shape of every page has an `ID` of its own, and with it a sheet and a path of its own: pages of equal shapes would be one page to a reader that shares what is equal
    // And the file is meant to be the most that XML costs.
    for page in 1..=pages {
        let shapes: String = (0..per_page).map(|i| typical_shape((page - 1) * per_page + i)).collect();
        builder = builder.page(PageSpec::new(page, "P").shapes(&shapes));
    }
    let shown: String = (1..=pictures).map(foreign).collect();
    let mut page = PageSpec::new(pages + 1, "Photos").shapes(&shown);
    let mut package = Pkg::new();
    for k in 1..=pictures {
        let ty = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
        page = page.rel(&format!("rId{k}"), ty, &format!("../media/image{k}.png"));
        package = package.part(&format!("/visio/media/image{k}.png"), "image/png", noise(mib * 1024 * 1024, 0x2545_F491_4F6C_DD1D + u64::from(k)));
    }
    builder.page(page).with(package).build()
}

/// What a command ended as, for the `RESULT` line.
fn how(result: &Result<serde_json::Value, EngineError>) -> &'static str {
    if result.is_ok() { "ok" } else { "refused" }
}

/// The kind of trouble a refused open was, as a word (`out_of_memory`, `too_big`...): what a frontend reads and does not parse out of the words.
fn problem(result: &Result<serde_json::Value, EngineError>) -> &'static str {
    match result {
        Err(EngineError::CannotOpen { problem, .. }) => problem.word(),
        Err(_) => "other",
        Ok(_) => "-",
    }
}

/// The words of a refusal, on one line.
fn words(result: &Result<serde_json::Value, EngineError>) -> String {
    match result {
        Ok(_) => "-".to_owned(),
        Err(e) => e.to_string().replace(['\n', '\r'], " "),
    }
}

/// The `RESULT` line of a case: how the read ended, what the session that read held, how many pages its drawing has, the address space, and the words.
fn result(scenario: &str, read: &Result<serde_json::Value, EngineError>, s: &Session) -> String {
    let held = s.held_memory();
    format!(
        "RESULT {scenario} second={} problem={} held_drawing={} held_stencils={} held_process={} pages={} VmPeak={}KiB words={}",
        how(read),
        problem(read),
        held.drawing,
        held.stencils,
        held.process,
        s.document().map_or(0, |d| d.pages.len()),
        status_kib("VmPeak"),
        words(read)
    )
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    let (first, second) = (std::env::var(FIRST).expect("the first file"), std::env::var(SECOND).expect("the second file"));
    let open =
        |s: &mut Session, path: &str| s.execute("file.open", &json!({ "path": path })).unwrap_or_else(|e| panic!("the first file did not open: {e}"));
    let mut s = Session::new();
    match scenario.as_str() {
        // One session holds a drawing and reads a second file, a drawing or a stencil.
        "drawing_then_drawing" => {
            open(&mut s, &first);
            let read = s.execute("file.open", &json!({ "path": second }));
            println!("\n{}", result(&scenario, &read, &s));
        }
        "drawing_then_stencil" => {
            open(&mut s, &first);
            let read = s.execute("stencil.open", &json!({ "path": second }));
            println!("\n{}", result(&scenario, &read, &s));
        }
        "stencil_then_drawing" => {
            s.execute("stencil.open", &json!({ "path": first })).unwrap_or_else(|e| panic!("the stencil did not open: {e}"));
            let read = s.execute("file.open", &json!({ "path": second }));
            println!("\n{}", result(&scenario, &read, &s));
        }
        // The window's cases: a session holds the drawing, and a second session, new, reads (File > Open, `diacraft FILE`, the control channel's `file.open`: a session of its
        // own on a worker thread; and `Scratch`, which reads an open stencil again in a session it keeps).
        "two_sessions_drawing" | "two_sessions_stencil" => {
            open(&mut s, &first);
            let mut other = Session::new();
            let read = if scenario == "two_sessions_drawing" {
                other.execute("file.open", &json!({ "path": second }))
            } else {
                other.execute("stencil.open", &json!({ "path": second }))
            };
            println!("\n{}", result(&scenario, &read, &other));
            println!("\nWINDOW pages={} held_process={}", s.document().map_or(0, |d| d.pages.len()), s.held_memory().process);
        }
        "scratch" => {
            s.execute("file.new", &json!({})).expect("a drawing to open the stencil beside");
            s.execute("stencil.open", &json!({ "path": first })).unwrap_or_else(|e| panic!("the stencil did not open in the window's session: {e}"));
            let mut scratch = Session::new();
            let read = scratch.execute("stencil.open", &json!({ "path": first }));
            println!("\n{}", result(&scenario, &read, &scratch));
        }
        // Two reads at the same moment, each in a session of its own (a cancelled read that is still parsing, and the next Open; a control script that sends two): the first to
        // take the budget reads, and the other is refused in words. Both were given the whole budget when nothing was held while a read ran.
        "race" => race(&first),
        // A read that is running holds the budget, so a read made beside it is refused in words that say a file is being read, and is read when that one has ended.
        "in_flight" => {
            // A read waits for the one that runs, as long as the wait it is given: none here, so that it is refused where it stands.
            memory::set_read_wait(std::time::Duration::ZERO);
            let reservation = memory::reserve_read();
            let read = s.execute("file.open", &json!({ "path": first }));
            println!("\n{}", result(&scenario, &read, &s));
            assert_eq!(memory::reserved(), reservation.bytes(), "the read that is running holds, and the refused read held nothing");
            drop(reservation);
            assert_eq!((memory::reserved(), memory::held()), (0, 0), "a reservation that is dropped gives back all it held");
            let again = s.execute("file.open", &json!({ "path": first }));
            println!("AGAIN {}", how(&again));
        }
        // A read made while another is running waits for it, and is read when it has ended (the next Open after a Cancel, while the cancelled thread still parses).
        "waits_for_the_read" => {
            let reservation = memory::reserve_read();
            // The read that runs ends when the one made after it is waiting: nothing here depends on how long either takes. **And when that read has been answered, whether it waited
            // or not**: a read that does not wait (a wait that was taken out) is refused, and the child ends there, where the holder would otherwise wait for it for ever.
            let answered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let seen = std::sync::Arc::clone(&answered);
            let holder = std::thread::spawn(move || {
                while memory::waiting_reads() == 0 && !seen.load(std::sync::atomic::Ordering::SeqCst) {
                    std::thread::yield_now();
                }
                drop(reservation);
            });
            let read = s.execute("file.open", &json!({ "path": first }));
            answered.store(true, std::sync::atomic::Ordering::SeqCst);
            holder.join().expect("the thread that held the budget");
            println!("\n{}", result(&scenario, &read, &s));
            println!("WAITED waiting={} reserved={}", memory::waiting_reads(), memory::reserved());
        }
        // An edit made while a read is running (the person cancels an Open and goes on, and the cancelled thread still parses): the read holds the budget, and takes nothing from the
        // edit's room, whose steps are kept.
        "edit_beside_a_read" => {
            s.execute("file.new", &json!({})).expect("a drawing");
            let page = s.document().and_then(|d| d.pages.get(0).map(|p| p.id)).expect("a page");
            let mut ids = Vec::new();
            for n in 0..5 {
                let dropped =
                    s.execute("shape.drop", &json!({"master": "rectangle", "page": page, "x": 2.0 + f64::from(n), "y": 3.0})).expect("a shape");
                ids.push(dropped["id"].as_u64().expect("an id"));
            }
            let before = s.history().map_or(0, |h| h.undo_len());
            let reading = memory::reserve_read();
            let moved = s.execute("shape.move", &json!({"ids": [ids[0]], "dx": 1.0, "dy": 0.0}));
            let after = s.history().map_or(0, |h| h.undo_len());
            println!(
                "\nEDIT ok={} undo_before={before} undo_after={after} reserved={} words={}",
                moved.is_ok(),
                memory::reserved() > 0,
                words(&moved)
            );
            drop(reading);
        }
        // `first` is held by one session and `second` is read in another: beside it, and then alone.
        "beside_then_alone" => {
            open(&mut s, &first);
            let mut other = Session::new();
            let beside = other.execute("file.open", &json!({ "path": second }));
            println!("\n{}", result(&scenario, &beside, &other));
            drop(s);
            let alone = other.execute("file.open", &json!({ "path": second }));
            println!("ALONE {} pages={}", how(&alone), other.document().map_or(0, |d| d.pages.len()));
        }
        // A read waits for the read that holds the budget, and **takes no part of what a drawing gives back meanwhile**: the drawing a session holds is closed while the read waits, and
        // the read that holds the rest has not ended.
        "partial_room" => {
            open(&mut s, &first);
            let reservation = memory::reserve_read();
            let path = second.clone();
            let reader = std::thread::spawn(move || Session::new().execute("file.open", &json!({ "path": path })).map(|_| ()));
            while memory::waiting_reads() == 0 {
                std::thread::yield_now();
            }
            s.execute("file.new", &json!({})).expect("a new drawing: the first one's file and model are given back");
            let at = std::time::Instant::now();
            while !reader.is_finished() && at.elapsed() < std::time::Duration::from_millis(400) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            println!("\nPARTIAL finished_before_the_read_ended={}", reader.is_finished());
            drop(reservation);
            let read = reader.join().expect("the reading thread");
            println!("PARTIAL read={}", if read.is_ok() { "ok" } else { "refused" });
        }
        // A template read (`file.new {template}`) waits as `file.open` does.
        "template_waits" => {
            let reservation = memory::reserve_read();
            let answered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let seen = std::sync::Arc::clone(&answered);
            let holder = std::thread::spawn(move || {
                while memory::waiting_reads() == 0 && !seen.load(std::sync::atomic::Ordering::SeqCst) {
                    std::thread::yield_now();
                }
                drop(reservation);
            });
            let read = s.execute("file.new", &json!({ "template": first }));
            answered.store(true, std::sync::atomic::Ordering::SeqCst);
            holder.join().expect("the thread that held the budget");
            println!("\n{}", result(&scenario, &read, &s));
            println!("WAITED waiting={} reserved={}", memory::waiting_reads(), memory::reserved());
        }
        // A stencil is read on the window's thread, which must not wait: it is read beside a drawing that is being read, where the two fit (a stencil is small).
        "stencil_beside_a_read" => {
            let path = first.clone();
            let reading = std::thread::spawn(move || {
                let mut a = Session::new();
                let read = a.execute("file.open", &json!({ "path": path }));
                (read.is_ok(), a)
            });
            while memory::reserved() == 0 {
                std::thread::yield_now();
            }
            let mut w = Session::new();
            let read = w.execute("stencil.open", &json!({ "path": second }));
            println!("\n{}", result(&scenario, &read, &w));
            let (ok, a) = reading.join().expect("the reading thread");
            println!("BIG ok={ok} VmPeak={}KiB", status_kib("VmPeak"));
            drop(a);
        }
        // Nothing is left of the budget for a file this size: it is refused before the package is inflated (a file that is no package at all is told as no room, and not as no package).
        "no_room" => {
            let _all = memory::Hold::new(diacraft_vsdx::MAX_MEMORY - 10);
            let read = s.execute("file.open", &json!({ "path": first }));
            println!("\n{}", result(&scenario, &read, &s));
        }
        // The ledger in a process of its own, where nothing else holds: exact figures.
        "ledger" => ledger(&first, &second),
        other => panic!("no scenario {other}"),
    }
}

/// Two sessions on two threads open the same drawing of 80,000 shapes at the same moment.
fn race(path: &str) {
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let (path, barrier) = (path.to_owned(), Arc::clone(&barrier));
            std::thread::spawn(move || {
                let mut s = Session::new();
                barrier.wait();
                let read = s.execute("file.open", &json!({ "path": path }));
                (read, s)
            })
        })
        .collect();
    let reads: Vec<_> = threads.into_iter().map(|t| t.join().expect("a reading thread that did not panic")).collect();
    let (ok, refused): (Vec<_>, Vec<_>) = reads.iter().partition(|(read, _)| read.is_ok());
    println!("\nRACE ok={} refused={} VmPeak={}KiB", ok.len(), refused.len(), status_kib("VmPeak"));
    for (read, _) in &refused {
        println!("RACE refused problem={} words={}", problem(read), words(read));
    }
    // What was held while they read is what the one drawing that was read holds, and nothing is left of the reservations.
    let drawing: u64 = ok.iter().map(|(_, s)| s.held_memory().drawing).sum();
    println!("RACE held={} drawing={drawing}", memory::held());
    drop(reads);
    println!("RACE after={}", memory::held());
}

/// Whether the process holds `want` and what a session's count has grown by (some kilobytes): the count is held beside the file's hold, and the ledger of a process that opens drawings
/// holds both.
fn near(held: u64, want: u64) -> bool {
    held >= want && held - want < 1 << 20
}

/// What the ledger says as sessions open and close files, with nothing else in the process: `first` is a drawing and `second` a stencil, both small.
fn ledger(first: &str, second: &str) {
    assert_eq!(memory::held(), 0, "a process that has opened nothing holds nothing");
    let mut a = Session::new();
    a.execute("file.open", &json!({ "path": first })).expect("the drawing opens");
    let drawing = a.held_memory().drawing;
    assert!(drawing > 10_000, "the file and the model of a drawing are held: {drawing}");
    assert_eq!(memory::held(), drawing, "the process holds what the session holds");
    // Another session sees it, and holds none of it.
    let mut b = Session::new();
    assert_eq!((b.held_memory().drawing, b.held_memory().process), (0, drawing));
    b.execute("stencil.open", &json!({ "path": second })).expect("the stencil opens in the other session");
    let stencil = b.held_memory().stencils;
    assert!(stencil > 10_000);
    assert_eq!(memory::held(), drawing + stencil);
    assert_eq!(
        (a.held_memory().stencils, a.held_memory().elsewhere()),
        (0, stencil),
        "the first session holds none of the stencil and says what is elsewhere"
    );
    // A command clones the session's state and holds nothing more: the bytes are one.
    a.execute("shape.move", &json!({ "ids": [1], "dx": 1.0, "dy": 0.0 })).expect("the shape moves");
    // The count of the session (the drawing, its steps and the model a save keeps) is held beside the file's, **by what it has grown by since the open**: a few kilobytes for an edit.
    assert!(near(memory::held(), drawing + stencil), "an edit holds what the count grew by: {} against {}", memory::held(), drawing + stencil);
    // A save writes a file of other bytes and the model is the one that was read.
    a.execute("file.save", &json!({})).expect("a save");
    let saved = a.held_memory().drawing;
    assert!(near(memory::held(), saved + stencil), "{} against {}", memory::held(), saved + stencil);
    // A session that is dropped gives back what it held, in a thread that ends in a panic as well.
    let before = memory::held();
    let path = first.to_owned();
    let outcome = std::thread::spawn(move || {
        let mut c = Session::new();
        c.execute("file.open", &json!({ "path": path })).expect("a thread's own session opens the drawing");
        assert!(memory::held() > before, "the thread's session holds");
        panic!("a thread that ends in a panic while its session holds a drawing");
    })
    .join();
    assert!(outcome.is_err());
    assert_eq!(memory::held(), before, "the unwind gave back what the thread's session held");
    drop(a);
    assert_eq!(memory::held(), stencil, "a dropped session gives back its drawing");
    // A close gives back the stencil, and a drawing opened over another gives back the first.
    let listed = b.execute("stencil.list", &json!({})).expect("the list");
    let id =
        listed["stencils"].as_array().and_then(|s| s.iter().find(|s| s["builtin"] == json!(false))).and_then(|s| s["id"].as_str().map(str::to_owned));
    b.execute("stencil.close", &json!({ "id": id.expect("the open stencil") })).expect("the stencil closes");
    assert_eq!(memory::held(), 0, "a closed stencil is given back");
    b.execute("file.open", &json!({ "path": first })).expect("a drawing");
    let one = memory::held();
    b.execute("file.open", &json!({ "path": first })).expect("the same drawing again");
    assert_eq!(memory::held(), one, "a drawing opened over another leaves the one");
    b.execute("file.new", &json!({})).expect("a new drawing");
    assert!(near(memory::held(), 0), "and a new drawing holds no file, and the count of a blank one: {}", memory::held());
    println!("\nLEDGER ok drawing={drawing} stencil={stencil}");
}

/// Runs the child's `scenario` on the two files under `ulimit -v 3000000`: whether it ended well, and what it printed.
fn run_child(scenario: &str, first: &str, second: &str) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {LIMIT_KIB} && exec \"$0\" --exact vsdx_memory::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg(exe)
        .env(SCENARIO, scenario)
        .env(FIRST, first)
        .env(SECOND, second)
        .output()
        .expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

/// The `RESULT` line a child printed.
fn result_line(stdout: &str) -> String {
    stdout.lines().find(|l| l.starts_with("RESULT")).unwrap_or("no RESULT line").to_owned()
}

/// What a refusal for memory beside what is held says: the class, what is held and what to do, and no part of the file.
fn assert_said_what_is_held(line: &str) {
    assert!(line.contains("second=refused"), "{line}");
    assert!(line.contains("too_much_memory") || line.contains("not enough memory"), "{line}");
    assert!(line.contains("the open drawing and its file hold"), "{line}");
    assert!(line.contains("close a stencil, or the drawing"), "{line}");
}

/// The number after `key` in a line.
fn field(line: &str, key: &str) -> u64 {
    line.split(key)
        .nth(1)
        .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("no {key} in {line}"))
}

fn kib(line: &str) -> u64 {
    line.split("VmPeak=").nth(1).and_then(|rest| rest.split("KiB").next()).and_then(|n| n.parse().ok()).expect("a VmPeak")
}

/// A child that ended well, a refusal in words that say what is held, and an address space with the margin.
fn assert_refused_in_words(scenario: &str, first: &str, second: &str) -> String {
    let (ended_well, stdout) = run_child(scenario, first, second);
    assert!(ended_well, "the child `{scenario}` aborted under `ulimit -v {LIMIT_KIB}`: {stdout}");
    let line = result_line(&stdout);
    assert_said_what_is_held(&line);
    assert!(kib(&line) < MOST_KIB, "the address space reached {} KiB of the {LIMIT_KIB}: {line}", kib(&line));
    stdout
}

#[test]
fn a_second_file_read_beside_a_drawing_of_80000_shapes_is_refused_in_words_that_say_what_is_held_and_does_not_abort() {
    let dir = Dir::new();
    let bytes = heavy(None);
    let (first, second) = (dir.write("a.vsdx", &bytes), dir.write("b.vsdx", &bytes));
    drop(bytes);
    let stdout = assert_refused_in_words("drawing_then_drawing", &first, &second);
    // The drawing that was open is open still.
    assert!(result_line(&stdout).contains("pages=20"), "{stdout}");
}

#[test]
fn a_stencil_read_beside_a_drawing_of_80000_shapes_is_refused_in_words_that_say_what_is_held_and_does_not_abort() {
    let dir = Dir::new();
    let (drawing, stencil) = (heavy(None), heavy(Some(STENCIL_TYPE)));
    let (first, second) = (dir.write("a.vsdx", &drawing), dir.write("b.vssx", &stencil));
    drop((drawing, stencil));
    let stdout = assert_refused_in_words("drawing_then_stencil", &first, &second);
    assert!(result_line(&stdout).contains("pages=20"), "{stdout}");
}

#[test]
fn a_drawing_read_beside_a_stencil_of_80000_shapes_is_refused_in_words_that_say_what_is_held_and_does_not_abort() {
    let dir = Dir::new();
    let (stencil, drawing) = (heavy(Some(STENCIL_TYPE)), heavy(None));
    let (first, second) = (dir.write("a.vssx", &stencil), dir.write("b.vsdx", &drawing));
    drop((stencil, drawing));
    assert_refused_in_words("stencil_then_drawing", &first, &second);
}

#[test]
fn a_file_read_in_a_session_of_its_own_beside_the_drawing_of_another_is_refused_in_words_and_does_not_abort() {
    // The window reads File > Open, `diacraft FILE` and the control channel's `file.open` in a session of its own on a worker thread, while its own session holds the drawing: the
    // two sessions are one process, and the drawing of the first is held for the second (the second read it with the whole budget and aborted the process).
    let dir = Dir::new();
    let bytes = heavy(None);
    let (first, second) = (dir.write("a.vsdx", &bytes), dir.write("b.vsdx", &bytes));
    drop(bytes);
    let stdout = assert_refused_in_words("two_sessions_drawing", &first, &second);
    let window = stdout.lines().find(|l| l.starts_with("WINDOW")).expect("a WINDOW line");
    assert!(window.contains("pages=20"), "the window's drawing is open still: {window}");
}

#[test]
fn a_stencil_read_in_a_session_of_its_own_beside_the_drawing_of_another_is_refused_in_words_and_does_not_abort() {
    let dir = Dir::new();
    let (drawing, stencil) = (heavy(None), heavy(Some(STENCIL_TYPE)));
    let (first, second) = (dir.write("a.vsdx", &drawing), dir.write("b.vssx", &stencil));
    drop((drawing, stencil));
    assert_refused_in_words("two_sessions_stencil", &first, &second);
}

#[test]
fn a_stencil_the_window_reads_again_for_its_tiles_beside_the_one_it_holds_is_refused_in_words_and_does_not_abort() {
    // `Scratch::open` reads each opened stencil again, by its path, in a session it keeps: beside the stencil the window's session holds, a stencil of 80,000 shapes does not fit
    // twice, and the tiles are not drawn from it (the window shows its masters without them), where the second read aborted.
    let dir = Dir::new();
    let stencil = heavy(Some(STENCIL_TYPE));
    let path = dir.write("s.vssx", &stencil);
    drop(stencil);
    assert_refused_in_words("scratch", &path, &path);
}

#[test]
fn two_files_read_at_the_same_moment_are_one_read_and_one_refusal_in_words_and_neither_aborts() {
    // A read held nothing until it ended, so two reads of 80,000 shapes that ran at the same moment were each given the whole budget, and a process under
    // `ulimit -v 3000000` aborted ("memory allocation of 1256 bytes failed"). The window can do it (Cancel, then Open, while the cancelled thread still parses) and so can a script.
    let dir = Dir::new();
    let bytes = heavy(None);
    let path = dir.write("a.vsdx", &bytes);
    drop(bytes);
    let (ended_well, stdout) = run_child("race", &path, &path);
    assert!(ended_well, "the child aborted, or its assertions failed, under `ulimit -v {LIMIT_KIB}`: {stdout}");
    let line = stdout.lines().find(|l| l.starts_with("RACE ok=")).unwrap_or("no RACE line");
    assert!(line.starts_with("RACE ok=1 refused=1 "), "one of the two reads is made, and the other is refused: {line}");
    assert!(kib(line) < MOST_KIB, "the address space reached {} KiB of the {LIMIT_KIB}: {line}", kib(line));
    let refusal = stdout.lines().find(|l| l.starts_with("RACE refused")).unwrap_or("no refusal line");
    assert!(
        refusal.contains("problem=out_of_memory") && refusal.contains("not enough memory"),
        "the refusal is in words, and is not a file that is too big: {refusal}"
    );
    let held = stdout.lines().find(|l| l.starts_with("RACE held=")).unwrap_or("no held line");
    let (held_now, drawing) = (field(held, "held="), field(held, "drawing="));
    assert!(
        held_now >= drawing && held_now - drawing < 1 << 20,
        "the process holds the drawing that was read and nothing of the read that ended: {held}"
    );
    assert!(stdout.contains("RACE after=0"), "every hold is given back when the sessions are gone: {stdout}");
}

#[test]
fn a_read_made_while_another_is_running_is_refused_in_words_that_say_a_file_is_being_read_and_is_read_when_that_one_has_ended() {
    let dir = Dir::new();
    let path = dir.write("a.vsdx", &small(None));
    let (ended_well, stdout) = run_child("in_flight", &path, &path);
    assert!(ended_well, "the child's assertions failed: {stdout}");
    let line = result_line(&stdout);
    assert!(line.contains("second=refused") && line.contains("problem=out_of_memory"), "{line}");
    assert!(line.contains("another file is being read") && line.contains("wait for it to finish"), "{line}");
    assert!(!line.contains("close a stencil"), "closing a drawing is not what helps here: {line}");
    assert!(stdout.contains("AGAIN ok"), "the same file opens when the read has ended: {stdout}");
}

#[test]
fn a_file_opened_while_another_is_read_waits_for_it_and_is_read_when_it_has_ended() {
    // A read that finds the budget held by a read that is running waits for it (the window's next Open after a Cancel, while the cancelled thread still parses: it
    // was told to wait for a small file, and the existing test of the progress card holds that it is not), and is read against what that one leaves, or refused in words if the two
    // do not fit together.
    let dir = Dir::new();
    let path = dir.write("a.vsdx", &small(None));
    let (ended_well, stdout) = run_child("waits_for_the_read", &path, &path);
    assert!(ended_well, "the child's assertions failed: {stdout}");
    let line = result_line(&stdout);
    assert!(line.contains("second=ok") && line.contains("pages=1"), "the file was read when the read that held the budget ended: {line}");
    assert!(stdout.contains("WAITED waiting=0 reserved=0"), "and nobody waits and nothing is reserved: {stdout}");
}

#[test]
fn an_edit_made_while_a_read_is_running_is_made_and_keeps_every_undo_step() {
    // A read holds all that is left of the budget while it runs, and the edit guard of a session takes what the rest of the process holds from the room of its model: a read must not
    // count there, or the person who cancels an Open and goes on drawing loses undo steps (or an edit) for the seconds the cancelled thread still parses.
    let dir = Dir::new();
    let path = dir.write("a.vsdx", &small(None));
    let (ended_well, stdout) = run_child("edit_beside_a_read", &path, &path);
    assert!(ended_well, "the child's assertions failed: {stdout}");
    let line = stdout.lines().find(|l| l.starts_with("EDIT ")).unwrap_or("no EDIT line");
    assert!(line.starts_with("EDIT ok=true "), "the edit is made: {line}");
    assert!(line.contains("reserved=true"), "while a read holds the budget: {line}");
    let (before, after) = (field(line, "undo_before="), field(line, "undo_after="));
    assert_eq!(after, before + 1, "and the steps are all kept, and its own is one more: {line}");
}

#[test]
fn a_file_too_big_on_its_own_is_told_so_beside_a_drawing_and_one_that_would_fit_alone_is_told_there_is_not_enough_memory_beside_it() {
    // A refusal for memory beside what is held said `out_of_memory` whenever anything was held, to a file that no amount of closing would let in. The reader gives no figure of what a
    // file needs, only the report of how far it got, and the engine judges from that (`cmd::file::judged_alone`).
    let dir = Dir::new();
    let drawing = typical_drawing(10, 4_000, None);
    let huge = typical_drawing(25, 4_000, None);
    let middling = typical_drawing(15, 4_000, None);
    let (first, huge_path, middling_path) = (dir.write("a.vsdx", &drawing), dir.write("huge.vsdx", &huge), dir.write("middling.vsdx", &middling));
    drop((drawing, huge, middling));
    // 100,000 shapes beside one of 40,000: the file is refused alone too (it is more than 88,000), and closing the drawing would not open it.
    let (ended_well, stdout) = run_child("drawing_then_drawing", &first, &huge_path);
    assert!(ended_well, "the child aborted: {stdout}");
    let line = result_line(&stdout);
    assert!(line.contains("second=refused") && line.contains("problem=too_big"), "a file that is too big alone is too big beside a drawing: {line}");
    assert!(line.contains("this file is too big to open safely (too_much_memory)"), "as it is told alone: {line}");
    assert!(!line.contains("not enough memory beside"), "{line}");
    assert!(!line.contains("close a stencil"), "{line}");
    // 60,000 shapes beside one of 80,000 (1.7 GB): the file is read alone, and not beside the drawing.
    let big = heavy(None);
    let big_path = dir.write("b.vsdx", &big);
    drop(big);
    let (ended_well, stdout) = run_child("drawing_then_drawing", &big_path, &middling_path);
    assert!(ended_well, "the child aborted: {stdout}");
    let line = result_line(&stdout);
    assert!(
        line.contains("second=refused") && line.contains("problem=out_of_memory"),
        "a file that fits alone is not enough memory beside the drawing: {line}"
    );
    assert_said_what_is_held(&line);
}

#[test]
fn a_file_whose_last_page_shows_big_pictures_is_not_enough_memory_beside_a_drawing_and_opens_alone() {
    // The pictures are the last of the file and the refused read has taken in none of them: the figures of the XML that were taken in were scaled to the whole package, pictures included,
    // and a file that opens alone was told too big beside a drawing (the review). Only the XML is scaled.
    let dir = Dir::new();
    let held = typical_drawing(15, 4_000, None);
    let (first, second) = (dir.write("held.vsdx", &held), dir.write("photos.vsdx", &shapes_then_pictures(10, 4_000, 2, 50)));
    drop(held);
    let (ended_well, stdout) = run_child("beside_then_alone", &first, &second);
    assert!(ended_well, "the child aborted, or its assertions failed: {stdout}");
    let line = result_line(&stdout);
    assert!(line.contains("second=refused") && line.contains("problem=out_of_memory"), "beside 60,000 shapes the file is not enough memory: {line}");
    assert_said_what_is_held(&line);
    assert!(stdout.contains("ALONE ok pages=11"), "and it opens alone: {stdout}");
    assert!(kib(&line) < MOST_KIB, "{line}");
}

#[test]
fn a_read_waits_for_the_read_that_holds_the_budget_and_takes_no_part_of_what_a_drawing_gives_back_meanwhile() {
    // A drawing is closed while a read waits for the one that holds the budget: a part of the budget is free, and a read that took it was given too little and refused at once
    // (the review: a small file opened after a cancelled big one, then a new drawing, was refused where main opens it).
    let dir = Dir::new();
    let (first, second) = (dir.write("a.vsdx", &small(None)), dir.write("b.vsdx", &small(None)));
    let (ended_well, stdout) = run_child("partial_room", &first, &second);
    assert!(ended_well, "the child's assertions failed: {stdout}");
    assert!(stdout.contains("PARTIAL finished_before_the_read_ended=false"), "the read took the part that was given back, and ended: {stdout}");
    assert!(stdout.contains("PARTIAL read=ok"), "and is read when the read that held the budget has ended: {stdout}");
}

#[test]
fn a_template_read_waits_for_the_read_that_holds_the_budget_as_file_open_does() {
    let dir = Dir::new();
    let path = dir.write("t.vstx", &typical_drawing(1, 10, Some(TEMPLATE_TYPE)));
    let (ended_well, stdout) = run_child("template_waits", &path, &path);
    assert!(ended_well, "the child's assertions failed: {stdout}");
    let line = result_line(&stdout);
    assert!(line.contains("second=ok") && line.contains("pages=1"), "the template was read when the read that held the budget ended: {line}");
    assert!(stdout.contains("WAITED waiting=0 reserved=0"), "{stdout}");
}

#[test]
fn a_stencil_is_read_beside_a_drawing_of_80000_shapes_that_is_being_read_and_neither_aborts() {
    // `stencil.open` runs on the window's thread and must not wait for a read: while a drawing is read (the open thread's, or one a Cancel left running) a stencil of some masters is
    // opened beside it, as it was before reads held the budget (the review: MEDIUM).
    let dir = Dir::new();
    let heavy_drawing = heavy(None);
    let (first, second) = (dir.write("a.vsdx", &heavy_drawing), dir.write("s.vssx", &small(Some(STENCIL_TYPE))));
    drop(heavy_drawing);
    let (ended_well, stdout) = run_child("stencil_beside_a_read", &first, &second);
    assert!(ended_well, "the child aborted, or its assertions failed, under `ulimit -v {LIMIT_KIB}`: {stdout}");
    let line = result_line(&stdout);
    assert!(line.contains("second=ok"), "the stencil is opened beside the drawing that is being read: {line}");
    let big = stdout.lines().find(|l| l.starts_with("BIG ")).unwrap_or("no BIG line");
    assert!(big.starts_with("BIG ok=true "), "and the drawing is read: {big}");
    assert!(kib(big) < MOST_KIB, "the address space reached {} KiB of the {LIMIT_KIB}: {big}", kib(big));
}

#[test]
fn a_file_with_no_room_for_its_own_bytes_is_refused_for_memory_before_its_package_is_read() {
    // A file that is not a package at all, with less room than it has bytes: the reader is not asked, and the refusal is that there is no room and not that it is no package.
    let dir = Dir::new();
    let path = dir.write("junk.vsdx", &[7u8; 1_000]);
    let (ended_well, stdout) = run_child("no_room", &path, &path);
    assert!(ended_well, "the child's assertions failed: {stdout}");
    let line = result_line(&stdout);
    assert!(line.contains("second=refused") && line.contains("problem=out_of_memory"), "{line}");
}

#[test]
fn the_ledger_of_a_process_counts_what_every_session_holds_and_gives_it_back_when_a_session_a_thread_or_a_close_lets_go() {
    let dir = Dir::new();
    let (first, second) = (dir.write("a.vsdx", &small(None)), dir.write("s.vssx", &small(Some(STENCIL_TYPE))));
    let (ended_well, stdout) = run_child("ledger", &first, &second);
    assert!(ended_well, "the child's assertions failed or it aborted: {stdout}");
    assert!(stdout.contains("LEDGER ok"), "{stdout}");
}

#[test]
fn what_is_held_is_what_the_process_holds_and_never_less_than_what_the_session_says_and_each_read_is_given_the_rest() {
    let held = HeldMemory { drawing: 1_000, stencils: 234, process: 5_000 };
    assert_eq!((held.total(), held.elsewhere()), (5_000, 3_766));
    assert_eq!(held.read_limits().max_memory, diacraft_vsdx::MAX_MEMORY - 5_000);
    // A process figure under what the session itself says is not believed: the session's own is the least.
    let low = HeldMemory { drawing: 1_000, stencils: 234, process: 10 };
    assert_eq!((low.total(), low.elsewhere()), (1_234, 0));
    assert_eq!(HeldMemory::default().read_limits().max_memory, diacraft_vsdx::MAX_MEMORY);
    // More than the budget is held: nothing is left, and not a number that wrapped.
    assert_eq!(HeldMemory { drawing: u64::MAX, stencils: u64::MAX, process: u64::MAX }.read_limits().max_memory, 0);
    let words = held.words();
    assert!(words.contains("hold 1000 bytes, the open stencils 234 and the rest of the process 3766 of the 2600000000 bytes"), "{words}");
}

#[test]
fn the_memory_a_session_holds_is_the_files_bytes_and_the_model_it_read_as_and_stays_with_the_drawing_when_it_is_saved() {
    let bytes = small(None);
    let (_, report) = diacraft_vsdx::read(&bytes, &diacraft_vsdx::Limits::default()).expect("a drawing");
    assert!(report.stats.memory_bytes > 10_000, "the model of a shape of a diagram is charged: {:?}", report.stats);
    let dir = Dir::new();
    let path = dir.write("a.vsdx", &bytes);
    let mut s = Session::new();
    // (The process's figure is shared with the other tests of this binary when they run in one process: the session's own figures are what is exact.)
    assert_eq!((s.held_memory().drawing, s.held_memory().stencils), (0, 0), "a session with no drawing holds nothing");
    s.execute("file.open", &json!({ "path": path })).expect("the drawing opens");
    assert_eq!((s.held_memory().drawing, s.held_memory().stencils), (bytes.len() as u64 + report.stats.memory_bytes, 0));
    assert!(s.held_memory().process >= s.held_memory().drawing);
    // An edit and a save: the file written is a different file, and the model is the one that was read (what it has grown by is not counted).
    s.execute("shape.move", &json!({ "ids": [1], "dx": 1.0, "dy": 0.0 })).expect("the shape moves");
    s.execute("file.save", &json!({})).expect("the drawing is saved");
    let written = s.source().expect("the file the drawing has now").bytes().len() as u64;
    assert_eq!(s.held_memory().drawing, written + report.stats.memory_bytes);
    // A new drawing holds no file and no model.
    s.execute("file.new", &json!({})).expect("a new drawing");
    assert_eq!(s.held_memory().drawing, 0);
}

#[test]
fn the_memory_a_session_holds_for_a_stencil_is_the_model_it_read_as_until_the_stencil_is_closed() {
    let bytes = small(Some(STENCIL_TYPE));
    let (_, report) = diacraft_vsdx::read(&bytes, &diacraft_vsdx::Limits::default()).expect("a stencil");
    let dir = Dir::new();
    let path = dir.write("s.vssx", &bytes);
    let mut s = Session::new();
    s.execute("file.new", &json!({})).expect("a drawing to open the stencil beside");
    let opened = s.execute("stencil.open", &json!({ "path": path })).expect("the stencil opens");
    assert_eq!(s.held_memory().stencils, report.stats.memory_bytes);
    assert!(s.held_memory().stencils > 10_000);
    // A second stencil adds its own, and a close takes it away.
    let second = s.execute("stencil.open", &json!({ "path": dir.write("t.vssx", &bytes) })).expect("a second stencil");
    assert_eq!(s.held_memory().stencils, 2 * report.stats.memory_bytes);
    let id = second["id"].as_str().expect("an id");
    s.execute("stencil.close", &json!({ "id": id })).expect("the stencil closes");
    assert_eq!(s.held_memory().stencils, report.stats.memory_bytes);
    // The same file opened again is the stencil that is open, and adds nothing.
    s.execute("stencil.open", &json!({ "path": path })).expect("the same stencil");
    assert_eq!(s.held_memory().stencils, report.stats.memory_bytes);
    assert_eq!(opened["already_open"], json!(false));
}
