//! What the model's layout costs an edit after a `.diacraft` open (item 1, found and fixed): the same drawing of 12,000 distinct shapes is opened from
//! its `.vsdx` and from its `.diacraft`, and 300 one-shape moves and 100 fills are run through the engine's commands, as CPU time, in a process of its own for each run.
//! found that the sharing table of the `.vsdx` reader (one boxed stream and one `Vec` for each unit) spread the model in memory so that the edits after that read were slower; the
//! `.diacraft` decoder's table (`sheetxml::model::share`) had the same shape, and the edits after a `.diacraft` open cost **1.31 to 1.47 times** those after a `.vsdx` open.
//!
//! **What the profile says** (`perf` 7.3.4, beast, 2026-11-21; a release build with line tables, the edit window only, both sides pinned to the cores of one cache group; the figures of
//! a quiet and of a loaded machine differ, the counts of events do not). An edit works over the whole page: `calc` asks, for every formula of every shape, whether it reads beyond
//! its own sheet (`list_cross_readers`, `classify`, `reach`), so each edit touches 1.6 million small objects of the model (the cells, the trees of the formulas, the sections, the rows)
//! and makes and lets go of small objects of its own, which is why 55 to 60 percent of the cycles are in `malloc`, `realloc` and `free` and the rest in that walk, **on both sides**.
//! What differs is not the work: the `.diacraft` side retires **4 percent fewer instructions** (80.5 against 83.9 billion), and has **twice the misses of the translation of
//! addresses** (71 against 37.5 million `dTLB` load misses), 16 percent more cache misses and 20 percent more demand fills from memory. A decode that shares nothing has the
//! translation misses of the `.vsdx` side (37 million), and so has one that enters nothing in its table; one that enters the units with no streams has 54 million. So the cost is
//! **the small allocations of the table, which the decode lets go of at its end**: they were all over the model, and the small objects of the edits were made in the holes they left.
//! The fix is the one the `.vsdx` reader's table has: no entry is an allocation of its own (`share.rs`); with it the translation misses are 37 million on both
//! sides and the instructions are equal.
//!
//! **What is left**, and what the bound below is: the cost is 1.12 to 1.27 times, 1.20 on the mean (eight runs of the old decoder, the new one and the `.vsdx` side at one time on
//! separate cores, load 31 to 49), where the old table's was 1.31 to 1.47, 1.37 on the mean. A drawing the `.vsdx` reader makes has the objects of a shape in one block of memory
//! that an edit walks from one end to the other. The `.diacraft` decoder lets go of a tree for every shape it reads, and the objects of the next shape that fit the chunks the tree
//! left are made in them: the objects of a shape are over a hundred kilobytes, and the walk jumps 64 KiB or more four times for each shape, against none (49,620 of 1.58 million steps
//! against 343), so the same lines are fetched without the prefetcher's help (demand fills from memory are 10 to 20 percent over). A decode that shares nothing has it too, so it is
//! not the table. Cutting the allocations of the walk in `calc` (a `Vec` for each formula in `reach`, 45 percent of the instructions of an edit) makes the edits of both sides faster (a quarter on a quiet machine, up to half on a loaded one) and
//! does not change the ratio. Tried and not kept, none with a gain that was measured: the map of the cells of a sheet or a section built before what follows it, the units
//! interned as the `.vsdx` reader does, the tree of a child let go after the next one is read, and the trees let go in a bunch.
//!
//! **What an edit costs now**. An edit no longer walks the page: `calc` keeps the readers of other sheets from one edit to the next (`ReaderIndex`, owned by the session) and
//! compares the page in step with the copy before, so 300 moves cost 0.07 s where they cost 10.8 s (a release build with line tables, both sides pinned to one cache group, load 4 to 12, the least
//! of five: `.vsdx` 0.069 s and `.diacraft` 0.076 s, 1.11; 100 fills 3.6 and 3.7 ms, 1.01), and the layout of the model that the walk made so costly is hidden. What an edit costs is counted in
//! `edit_index.rs` (the formulas `calc` was asked), and this test is the check that the two sides still cost alike.
//!
//! Timed, so it is `#[ignore]`d: run by hand, alone, on a quiet machine,
//! `cargo test -p diacraft-engine --test it -- --ignored an_edit_after_a_diacraft_open --nocapture --test-threads=1` (not nextest: it ends a test at 120 seconds). Each run prints the machine's load beside its figures and waits (two
//! minutes at the most) while the load is over 32; `NATIVE_SPEED_CPUS` names the cores both sides are pinned to (`taskset -c`), which a profile wants (on a processor whose cache
//! groups differ, the two sides should have the same one). **It holds the `.diacraft` side to 1.25 times the `.vsdx` side (the least of the five runs of each)**: a bound that the
//! figures above keep to and the old table's (1.35 on the least of five) do not. The last run of it, in the test profile (`cargo test`), not pinned, load 10: 300 moves 104.1 and 117.4 ms of
//! CPU (1.13), 100 fills 4.97 and 5.24 ms (1.05), the yardstick after the edits 31 to 37 ms on both sides (before a change, load 27 to 48: 300 moves 20.8 and 23.6 s, 100 fills 6.1 and 6.4 s).

use std::process::Command;

use diacraft_testkit::package::typical_drawing;
use serde_json::json;

use crate::native_common::Dir;
use crate::run;

const FILE: &str = "NATIVE_SPEED_FILE";
const MOVES: usize = 300;
const FILLS: usize = 100;
/// The most the least run of the `.diacraft` side may be, over the least run of the `.vsdx` side, in percent.
const MOST_PERCENT: u64 = 125;

/// The CPU time the process has used (user and system), in hundredths of a second.
fn cpu_ticks() -> u64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat.rsplit(')').next().unwrap_or("").split_whitespace().collect();
    fields.get(11).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) + fields.get(12).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0)
}

/// The CPU time this thread has used, in nanoseconds (the scheduler's own count, `/proc/thread-self/schedstat`).
fn thread_ns() -> u64 {
    std::fs::read_to_string("/proc/thread-self/schedstat").ok().and_then(|t| t.split_whitespace().next().and_then(|v| v.parse().ok())).unwrap_or(0)
}

/// The machine's load, the average of the last minute (`/proc/loadavg`).
fn load() -> f64 {
    std::fs::read_to_string("/proc/loadavg").ok().and_then(|t| t.split_whitespace().next().and_then(|v| v.parse().ok())).unwrap_or(0.0)
}

/// Waits while the machine's load is over 32 (on a machine of 32 threads, a run would be measured beside as many others), two minutes at the most.
fn wait_for_a_quiet_machine() {
    for _ in 0..24 {
        if load() <= 32.0 {
            return;
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}

/// A yardstick: a fixed amount of work that has nothing to do with a document (small boxes made and let go of, a hash map, integer arithmetic), timed in the same process beside the
/// edits, so that a run on a machine whose cores are shared with others can be told from a run that is slower. Its CPU time in nanoseconds.
fn yardstick() -> u64 {
    let started = thread_ns();
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let mut sink = 0u64;
    let mut map = std::collections::HashMap::new();
    for round in 0..24u64 {
        let mut boxes: Vec<Box<[u64; 4]>> = Vec::new();
        for i in 0..40_000u64 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            boxes.push(Box::new([x, i, round, 0]));
            map.insert(x & 0x3FFF, i);
        }
        sink = boxes.iter().fold(sink, |acc, b| acc.wrapping_add(b[0] ^ b[1]));
    }
    std::hint::black_box(sink);
    thread_ns() - started
}

/// Tells a profiler that records on request (`perf record --control`, `perf stat --control`) to start or stop: writes `word` to the fifo that `NATIVE_SPEED_PERF_CTL` names, if it
/// names one.
fn perf_control(word: &str) {
    if let Ok(fifo) = std::env::var("NATIVE_SPEED_PERF_CTL") {
        // A profiler that is not there leaves nothing to tell.
        let _ = std::fs::write(fifo, format!("{word}\n"));
    }
}

/// Opens the file in `NATIVE_SPEED_FILE`, moves `MOVES` shapes of their own and fills `FILLS` others, and prints the CPU time of each, and of the yardstick before the open and after
/// the edits.
#[test]
#[ignore = "run by the test below in a process of its own"]
fn child() {
    let Ok(path) = std::env::var(FILE) else { return };
    let yard_before = yardstick();
    let mut s = diacraft_engine::Session::new();
    let opened = cpu_ticks();
    run(&mut s, "file.open", json!({ "path": path }));
    let opening = cpu_ticks() - opened;
    let ids: Vec<u32> = s.document().unwrap().pages.iter().next().unwrap().shapes.iter().map(|sh| sh.id).collect();
    assert!(ids.len() >= MOVES + FILLS, "{} shapes", ids.len());
    // Shapes spread over the page, in an order that is not the order they were read in.
    let pick = |k: usize, salt: usize| ids[(k * 37 + salt) % ids.len()];
    perf_control("enable");
    let (started, started_ns) = (cpu_ticks(), thread_ns());
    for k in 0..MOVES {
        run(&mut s, "shape.move", json!({"ids": [pick(k, 0)], "dx": 0.25, "dy": 0.0}));
    }
    let (moves, moves_ns) = (cpu_ticks() - started, thread_ns() - started_ns);
    let (started, started_ns) = (cpu_ticks(), thread_ns());
    for k in 0..FILLS {
        run(&mut s, "shape.format", json!({"ids": [pick(k, 11)], "fill": format!("#{:06X}", 0x10_0000 + k * 977)}));
    }
    let (fills, fills_ns) = (cpu_ticks() - started, thread_ns() - started_ns);
    perf_control("disable");
    let yard_after = yardstick();
    println!(
        "SPEED open={opening} moves={moves} fills={fills} moves_ns={moves_ns} fills_ns={fills_ns} yard_before_ns={yard_before} yard_after_ns={yard_after}"
    );
}

/// Writes the drawing the timed test uses as `a.vsdx` and as `a.diacraft` in the folder `NATIVE_SPEED_KEEP` names, for a profiler to run a child on.
#[test]
#[ignore = "writes two files for a run by hand: NATIVE_SPEED_KEEP names the folder"]
fn keep_files() {
    let Ok(dir) = std::env::var("NATIVE_SPEED_KEEP") else { return };
    let (vsdx, diacraft) = (format!("{dir}/a.vsdx"), format!("{dir}/a.diacraft"));
    std::fs::write(&vsdx, typical_drawing(1, 12_000, None)).unwrap();
    let mut s = diacraft_engine::Session::new();
    run(&mut s, "file.open", json!({ "path": vsdx }));
    run(&mut s, "file.save_as", json!({ "path": diacraft }));
}

/// What a child printed: its CPU time in hundredths of a second for the open (`open`) and in microseconds for the edits (`moves`, `fills`: the scheduler's count for the thread, since
/// an edit costs what it touched now and the hundredths of a second the process's clock gives are a tenth of the figure), the yardstick's nanoseconds before the open and after the
/// edits, and the load of the machine when it began.
#[derive(Clone, Copy)]
struct Timing {
    open: u64,
    moves: u64,
    fills: u64,
    yard: (u64, u64),
    load: f64,
}

impl Timing {
    /// The run as a line: the load it began at, the open in hundredths of a second, the edits in milliseconds, and the yardstick's in milliseconds.
    fn line(&self) -> String {
        format!(
            "load {:.1}: open {}, 300 moves {:.1} ms, 100 fills {:.1} ms, yardstick {} ms before and {} ms after",
            self.load,
            self.open,
            self.moves as f64 / 1_000.0,
            self.fills as f64 / 1_000.0,
            self.yard.0 / 1_000_000,
            self.yard.1 / 1_000_000
        )
    }
}

fn run_child(path: &str) -> Timing {
    wait_for_a_quiet_machine();
    let load = load();
    let exe = std::env::current_exe().expect("the path of this test binary");
    let args = ["--exact", "native_speed::child", "--ignored", "--nocapture", "--test-threads=1"];
    let mut command = match std::env::var("NATIVE_SPEED_CPUS") {
        Ok(cpus) => {
            let mut pinned = Command::new("taskset");
            pinned.args(["-c", &cpus]).arg(exe);
            pinned
        }
        Err(_) => Command::new(exe),
    };
    let out = command.args(args).env(FILE, path).output().expect("run the child");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{text}");
    let field = |name: &str| -> u64 {
        text.split(name)
            .nth(1)
            .and_then(|r| r.split_whitespace().next())
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("no `{name}` in {text}"))
    };
    Timing {
        open: field("open="),
        moves: field("moves_ns=") / 1_000,
        fills: field("fills_ns=") / 1_000,
        yard: (field("yard_before_ns="), field("yard_after_ns=")),
        load,
    }
}

#[test]
#[ignore = "timed: 12,000 shapes, 300 moves and 100 fills, ten times; run by hand, alone"]
fn an_edit_after_a_diacraft_open_costs_no_more_than_after_a_vsdx_open() {
    let dir = Dir::new();
    let vsdx = dir.write("a.vsdx", &typical_drawing(1, 12_000, None));
    let diacraft = dir.path("a.diacraft");
    let mut s = diacraft_engine::Session::new();
    run(&mut s, "file.open", json!({ "path": vsdx }));
    run(&mut s, "file.save_as", json!({ "path": diacraft }));
    drop(s);
    // Five rounds, each of the two in an order that alternates (the load of the machine moves from one run to the next by as much as the figures differ). Each run is told with the
    // load it began at and the yardstick's time before and after it.
    let mut vsdx_runs: Vec<Timing> = Vec::new();
    let mut diacraft_runs: Vec<Timing> = Vec::new();
    for round in 0..5 {
        let (v, d) = if round % 2 == 0 {
            let v = run_child(&vsdx);
            (v, run_child(&diacraft))
        } else {
            let d = run_child(&diacraft);
            (run_child(&vsdx), d)
        };
        println!("SPEED round {round}: vsdx, {}", v.line());
        println!("SPEED round {round}: diacraft, {}", d.line());
        vsdx_runs.push(v);
        diacraft_runs.push(d);
    }
    let range = |runs: &[Timing], pick: fn(&Timing) -> u64| (runs.iter().map(pick).min().unwrap_or(0), runs.iter().map(pick).max().unwrap_or(0));
    let (moves_v, moves_d) = (range(&vsdx_runs, |t| t.moves), range(&diacraft_runs, |t| t.moves));
    let (fills_v, fills_d) = (range(&vsdx_runs, |t| t.fills), range(&diacraft_runs, |t| t.fills));
    let loads = |runs: &[Timing]| (runs.iter().map(|t| t.load).fold(f64::MAX, f64::min), runs.iter().map(|t| t.load).fold(0.0, f64::max));
    println!(
        "SPEED moves: vsdx {moves_v:?}, diacraft {moves_d:?}; fills: vsdx {fills_v:?}, diacraft {fills_d:?} (the least and the most of five, in microseconds of CPU); the load: vsdx {:?}, diacraft {:?}",
        loads(&vsdx_runs),
        loads(&diacraft_runs)
    );
    assert!(
        moves_d.0 * 100 <= moves_v.0 * MOST_PERCENT,
        "300 moves after a .diacraft open: at the least {} against {:?} after a .vsdx open",
        moves_d.0,
        moves_v
    );
    assert!(
        fills_d.0 * 100 <= fills_v.0 * MOST_PERCENT,
        "100 fills after a .diacraft open: at the least {} against {:?} after a .vsdx open",
        fills_d.0,
        fills_v
    );
}
