//! What the threads of a save cost and buy. **Timed, so `#[ignore]`d**: run by hand, alone, in a release build, one process for each thread count (the count is the engine's, from
//! `DIACRAFT_THREADS`, read once for a process),
//! `PAR_FORMAT=diacraft PAR_SHAPES=12000 DIACRAFT_THREADS=4 cargo test --release -p diacraft-engine --test it -- --ignored par_save_times --nocapture --test-threads=1`.
//! `PAR_FORMAT` is `vsdx` (a save into the file it was read from, after an edit of the first shape of each page, or of every shape with `PAR_EDIT=all`), `diacraft` (the same drawing saved as `.diacraft`), `odg` (a drawing read from an `.odg`
//! and saved into it) or `pictures` (a drawing that holds large pictures, saved as `.diacraft`); `PAR_SHAPES` the shapes, in three pages, and `PAR_RUNS` the runs (five). It prints the
//! least of the runs, the machine's load before and after, and the size of the file written (`PAR_FRESH=1` opens the drawing in a session of its own before each run: every run is a first save).
//!
//! **The file is the same bytes at any thread count** ([`the_saves_of_each_format_are_the_same_bytes_at_any_thread_count`]): each count is a child process of its own (the count is read from the
//! environment, which a test does not set in its own process), which saves the drawing and prints a digest of the file; the digests are the same at 1, 2, 7 and 31 threads.

use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use diacraft_doc::{Cell, CellName, Value};
use diacraft_engine::{EngineError, Session};
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder};
use serde_json::json;

use crate::native_common::{Dir, PINNED};
use crate::odg_build;

/// The machine's load, the average of the last minute (`/proc/loadavg`).
fn load() -> f64 {
    std::fs::read_to_string("/proc/loadavg").ok().and_then(|t| t.split_whitespace().next().and_then(|v| v.parse().ok())).unwrap_or(0.0)
}

/// The edit that every run makes before it saves, so that a save has every page to write again: the first shape of each page moved, or, with `PAR_EDIT=all`, every shape of every page (each shape
/// and its sheet is copied, which holds the drawing twice: for the sizes that leave room for it).
fn nudge_all(s: &mut Session, x: usize) -> Result<(), EngineError> {
    let all = std::env::var("PAR_EDIT").is_ok_and(|v| v == "all");
    s.edit("nudge", |doc| {
        for p in 0..doc.pages.len() {
            let Some(page) = doc.pages.get_mut(p) else { continue };
            let page = Arc::make_mut(page);
            for i in 0..if all { page.shapes.len() } else { page.shapes.len().min(1) } {
                if let Some(shape) = page.shapes.get_mut(i) {
                    Arc::make_mut(&mut Arc::make_mut(shape).sheet)
                        .cells
                        .insert(CellName::new("PinX"), Cell::constant(Value::from_text(&format!("{x}.5"))));
                }
            }
        }
        Ok(())
    })
}

/// Bytes of a picture of a kind that deflates to about half its size and takes the compressor's time: four bits in each byte.
fn picture(len: usize, mut seed: u64) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::with_capacity(len);
    while bytes.len() < len {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        bytes.extend(seed.to_le_bytes().iter().map(|b| b & 0x0F));
    }
    bytes.truncate(len);
    bytes
}

/// A shape of the kind a diagram has: fourteen cells, two formulas, a path of five rows and a text, **its path its own** (a value that depends on `i`): the shape the capacity tests
/// (`vsdx_save_peak.rs`: 76,000 of them are saved where the address space allows) are measured on, which a reader that shares what is equal shares nothing of.
fn shape(i: usize) -> String {
    let (x, y) = (format!("{}.{}", i % 997, i % 89), format!("{}.{}", i / 997, i % 83));
    let cells = format!(
        "<Cell N=\"PinX\" V=\"{x}\"/><Cell N=\"PinY\" V=\"{y}\"/><Cell N=\"Width\" V=\"1\"/><Cell N=\"Height\" V=\"0.75\"/><Cell N=\"LocPinX\" V=\"0.5\" F=\"Width*0.5\"/><Cell N=\"LocPinY\" V=\"0.375\" F=\"Height*0.5\"/><Cell N=\"Angle\" V=\"0\"/><Cell N=\"FlipX\" V=\"0\"/><Cell N=\"FlipY\" V=\"0\"/><Cell N=\"ResizeMode\" V=\"0\"/><Cell N=\"LineWeight\" V=\"0.01\"/><Cell N=\"LineColor\" V=\"0\"/><Cell N=\"FillForegnd\" V=\"1\"/><Cell N=\"FillPattern\" V=\"1\"/>"
    );
    let points = [("MoveTo", "0", "0"), ("LineTo", "Width", "0"), ("LineTo", "Width", "Height"), ("LineTo", "0", "Height"), ("LineTo", "0", "0")];
    let rows: String = points
        .iter()
        .enumerate()
        .map(|(ix, (t, px, py))| {
            format!("<Row T=\"{t}\" IX=\"{}\"><Cell N=\"X\" V=\"1\" F=\"{px}\"/><Cell N=\"Y\" V=\"{i}\" F=\"{py}\"/></Row>", ix + 1)
        })
        .collect();
    format!(
        "<Shape ID=\"{}\" Type=\"Shape\" Name=\"Process\" NameU=\"Process\">{cells}<Section N=\"Geometry\" IX=\"0\"><Cell N=\"NoFill\" V=\"0\"/><Cell N=\"NoLine\" V=\"0\"/>{rows}</Section><Text><cp IX=\"0\"/>Label</Text></Shape>",
        i + 1
    )
}

/// A VSDX drawing of `pages` pages and `shapes` shapes in all, with `pictures` pictures of `each` bytes in its package.
fn vsdx_with(shapes: u32, pages: u32, pictures: usize, each: usize) -> Vec<u8> {
    let per = shapes.div_ceil(pages.max(1));
    let mut builder = VsdxBuilder::new();
    for page in 1..=pages {
        let body: String = (0..per).map(|i| shape(((page - 1) * per + i) as usize)).collect();
        builder = builder.page(PageSpec::new(page, "P").shapes(&body));
    }
    let mut extra = Pkg::new().default_type("bmp", "image/bmp");
    for k in 0..pictures {
        extra = extra.part_without_override(&format!("/visio/media/image{k}.bmp"), picture(each, 0x9E37_79B9_7F4A_7C15 ^ (k as u64 + 1)));
    }
    if pictures > 0 {
        builder = builder.with(extra);
    }
    builder.build()
}

/// An `.odg` of `pages` pages and `shapes` shapes in all.
fn odg_with(shapes: usize, pages: usize) -> Vec<u8> {
    let per = shapes.div_ceil(pages.max(1));
    let mut drawing = odg_build::Drawing::new();
    for page in 0..pages {
        let body: String = (0..per)
            .map(|i| odg_build::rect(&format!("s{page}_{i}"), (f64::from((i % 40) as u32), f64::from((i / 40 % 60) as u32), 0.9, 0.6), "Label"))
            .collect();
        drawing = drawing.page(&body);
    }
    drawing.bytes()
}

fn how<T>(result: &Result<T, EngineError>) -> String {
    match result {
        Ok(_) => "ok".to_owned(),
        Err(e) => e.to_string().replace(['\n', '\r'], " ").chars().take(200).collect(),
    }
}

/// What a scenario writes: the source file's bytes and name, and the name the save gives the file written.
fn scenario(format: &str, shapes: u32, pages: u32) -> (Vec<u8>, &'static str, &'static str) {
    match format {
        "vsdx" => (vsdx_with(shapes, pages, 0, 0), "a.vsdx", "a.vsdx"),
        "diacraft" => (vsdx_with(shapes, pages, 0, 0), "a.vsdx", "out.diacraft"),
        "pictures" => {
            let env = |key: &str, default: usize| std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default);
            (vsdx_with(shapes / 10, pages, env("PAR_PICTURES", 8), env("PAR_PICTURE_BYTES", 16 << 20)), "a.vsdx", "out.diacraft")
        }
        "odg" => (odg_with(shapes as usize, pages as usize), "a.odg", "a.odg"),
        other => panic!("no scenario {other}"),
    }
}

#[test]
#[ignore = "timed: run by hand, alone, in a release build, with the scenario in the environment"]
fn par_save_times() {
    let env = |key: &str, default: &str| std::env::var(key).unwrap_or_else(|_| default.to_owned());
    let format = env("PAR_FORMAT", "diacraft");
    let shapes: u32 = env("PAR_SHAPES", "12000").parse().unwrap();
    let pages: u32 = env("PAR_PAGES", "3").parse().unwrap();
    let runs: usize = env("PAR_RUNS", "5").parse().unwrap();
    let (source, name, target) = scenario(&format, shapes, pages);
    let dir = Dir::new();
    let path = dir.write(name, &source);
    drop(source);
    let mut s = Session::new();
    s.pin_clock(Some(PINNED));
    s.execute("file.open", &json!({ "path": path })).unwrap_or_else(|e| panic!("open: {e}"));
    let out = dir.path(target);
    let before = load();
    let mut times: Vec<f64> = Vec::new();
    let mut size = 0usize;
    let fresh = std::env::var("PAR_FRESH").is_ok_and(|v| v == "1");
    for run in 0..runs {
        if fresh && run > 0 {
            // A session of its own: the drawing the last run edited is let go before this one is read, as a drawing of the size of the capacity tests does not leave room for two.
            drop(s);
            s = Session::new();
            s.pin_clock(Some(PINNED));
            s.execute("file.open", &json!({ "path": path })).unwrap_or_else(|e| panic!("open again: {e}"));
        }
        nudge_all(&mut s, run + 1).unwrap_or_else(|e| panic!("nudge {run}: {e}"));
        let started = Instant::now();
        let result = if target == name { s.execute("file.save", &json!({})) } else { s.execute("file.save_as", &json!({ "path": out })) };
        let spent = started.elapsed().as_secs_f64() * 1000.0;
        assert!(result.is_ok(), "save {run}: {}", how(&result));
        times.push(spent);
        size = std::fs::metadata(if target == name { &path } else { &out }).map(|m| m.len() as usize).unwrap_or(0);
    }
    let least = times.iter().copied().fold(f64::INFINITY, f64::min);
    println!(
        "PAR_TIME format={format} shapes={shapes} pages={pages} threads={} best_ms={least:.0} all={:?} load_before={before:.1} load_after={:.1} bytes={size}",
        std::env::var("DIACRAFT_THREADS").unwrap_or_else(|_| "default".to_owned()),
        times.iter().map(|t| t.round() as u64).collect::<Vec<_>>(),
        load()
    );
}

/// The digest of the file a scenario saves: a drawing opened, edited everywhere and saved, once, with the clock pinned.
#[test]
#[ignore = "run by `the_saves_of_each_format_are_the_same_bytes_at_any_thread_count`: alone it needs the scenario in the environment"]
fn par_save_digest() {
    use std::hash::{Hash, Hasher};
    let env = |key: &str, default: &str| std::env::var(key).unwrap_or_else(|_| default.to_owned());
    let format = env("PAR_FORMAT", "diacraft");
    let (shapes, pages): (u32, u32) = (env("PAR_SHAPES", "1200").parse().unwrap(), env("PAR_PAGES", "4").parse().unwrap());
    let (source, name, target) = scenario(&format, shapes, pages);
    let dir = Dir::new();
    let path = dir.write(name, &source);
    drop(source);
    let mut s = Session::new();
    s.pin_clock(Some(PINNED));
    s.execute("file.open", &json!({ "path": path })).unwrap_or_else(|e| panic!("open: {e}"));
    nudge_all(&mut s, 1).unwrap();
    let out = dir.path(target);
    let result = if target == name { s.execute("file.save", &json!({})) } else { s.execute("file.save_as", &json!({ "path": out })) };
    assert!(result.is_ok(), "save: {}", how(&result));
    let bytes = std::fs::read(if target == name { &path } else { &out }).unwrap();
    // SipHash with its fixed keys: the same in every process.
    let mut hasher = std::hash::DefaultHasher::new();
    bytes.hash(&mut hasher);
    println!("DIGEST {:016x} {}", hasher.finish(), bytes.len());
}

#[test]
fn the_saves_of_each_format_are_the_same_bytes_at_any_thread_count() {
    let exe = std::env::current_exe().expect("the path of this test binary");
    for (format, extra) in [("vsdx", None), ("diacraft", None), ("pictures", Some(("PAR_PICTURE_BYTES", "1500000")))] {
        let mut seen: Vec<(String, String)> = Vec::new();
        for threads in ["1", "2", "7", "31"] {
            let mut child = Command::new(&exe);
            child
                .args(["--exact", "par_save::par_save_digest", "--ignored", "--nocapture", "--test-threads=1"])
                .env("PAR_FORMAT", format)
                .env("DIACRAFT_THREADS", threads);
            if let Some((key, value)) = extra {
                child.env(key, value).env("PAR_PICTURES", "5");
            }
            let out = child.output().expect("run the child");
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            assert!(out.status.success(), "{format} at {threads} threads failed:\n{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
            let digest = stdout.lines().find_map(|l| l.split_once("DIGEST ").map(|(_, rest)| rest.to_owned())).expect("a DIGEST line");
            seen.push((threads.to_owned(), digest));
        }
        let (_, first) = seen.first().expect("a digest").clone();
        for (threads, digest) in &seen {
            assert_eq!(digest, &first, "{format}: the file saved at {threads} threads is not the file saved at 1 ({seen:?})");
        }
    }
}

/// The counts of threads of the process it runs in, printed for [`the_frame_the_save_and_the_open_read_diacraft_threads_alike`]: the renderer's (`diacraft_render::default_threads`, and what
/// a frame takes of it), the save's (`diacraft_engine::threads::save_threads`, and what a save that holds nothing more takes of it), and the open's (`diacraft_container::par::default_threads`,
/// which the engine's `open_threads` is).
#[test]
#[ignore = "run by `the_frame_the_save_and_the_open_read_diacraft_threads_alike`: alone it prints the counts of its own process"]
fn par_thread_counts() {
    let frame = diacraft_render::default_threads();
    let frame_takes = diacraft_render::Parallel::with_threads(frame).usable_threads();
    let save = diacraft_engine::threads::save_threads();
    let save_takes = diacraft_engine::threads::save_threads_within(0);
    let open = diacraft_container::par::default_threads();
    println!("COUNTS {frame} {frame_takes} {save} {save_takes} {open}");
}

/// A frame, a save and an open read `DIACRAFT_THREADS` by one function (`diacraft_container::par::default_threads`). **They come to the
/// same count for every setting**: the cores less one when it is unset or no whole number, held to 1 to 64, and 1 (the single-threaded way) for 0 and 1. Each setting is a child process: a
/// test does not set the environment of its own process.
#[test]
fn the_frame_the_save_and_the_open_read_diacraft_threads_alike() {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    let unset = cores.saturating_sub(1).clamp(1, 64);
    let settings: [(Option<&str>, usize); 12] = [
        (None, unset),
        (Some("1"), 1),
        (Some("0"), 1),
        (Some("2"), 2),
        (Some(" 7 "), 7),
        (Some("31"), 31),
        (Some("64"), 64),
        (Some("65"), 64),
        (Some("99999999999999999999999"), unset),
        (Some("-3"), unset),
        (Some("many"), unset),
        (Some(""), unset),
    ];
    for (setting, want) in settings {
        let mut child = Command::new(&exe);
        child.args(["--exact", "par_save::par_thread_counts", "--ignored", "--nocapture", "--test-threads=1"]);
        match setting {
            Some(value) => child.env("DIACRAFT_THREADS", value),
            None => child.env_remove("DIACRAFT_THREADS"),
        };
        let out = child.output().expect("run the child");
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(out.status.success(), "{setting:?}: the child failed:\n{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
        // The harness prints the name of the test on the line the child's own words begin.
        let line = stdout.lines().find_map(|l| l.split_once("COUNTS ").map(|(_, rest)| rest)).expect("a COUNTS line");
        let counts: Vec<usize> = line.split_whitespace().map(|n| n.parse().expect("a count")).collect();
        assert_eq!(
            counts,
            vec![want; 5],
            "{setting:?}: the frame's count, what a frame takes, the save's count, what a save takes and the open's count"
        );
    }
}

/// What a frame and a save asked for 31 threads take in the process it runs in, beside what the frame may hold, printed for
/// [`under_an_address_limit_a_frame_and_a_save_are_held_by_the_rule_of_an_open`].
#[test]
#[ignore = "run by `under_an_address_limit_a_frame_and_a_save_are_held_by_the_rule_of_an_open` under `ulimit -v`"]
fn par_thread_room() {
    let frame = diacraft_render::Parallel::with_threads(31);
    let room = frame.room();
    let frame_takes = frame.usable_threads();
    let save_takes = diacraft_engine::threads::save_threads_within(room);
    let rule = diacraft_container::spare::threads_fit(31, room);
    println!("ROOM {frame_takes} {save_takes} {rule}");
}

/// **Under an address-space limit a frame and a save take the threads the rule of an open gives** (`diacraft_container::spare::threads_fit`: 192 MiB a thread in an eighth of what
/// the limit leaves beside the address space in use and the work's own room). A frame that took half of what was left at 80 MiB a thread drew the thumbnail of an `.odg` on threads whose
/// arenas the save after it needed, and the save aborted (`odg_memory`, 50,000 new shapes on 31 threads). Each limit is a child process of its own.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn under_an_address_limit_a_frame_and_a_save_are_held_by_the_rule_of_an_open() {
    let exe = std::env::current_exe().expect("the path of this test binary");
    for (limit_kib, fewest, most) in [(3_000_000u64, 1usize, 2usize), (16_000_000, 2, 30)] {
        let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact par_save::par_thread_room --ignored --nocapture --test-threads=1");
        let out = Command::new("/bin/sh").arg("-c").arg(script).arg(&exe).env("DIACRAFT_THREADS", "31").output().expect("run /bin/sh");
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(out.status.success(), "{limit_kib} KiB: the child failed:\n{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
        let line = stdout.lines().find_map(|l| l.split_once("ROOM ").map(|(_, rest)| rest)).expect("a ROOM line");
        let counts: Vec<usize> = line.split_whitespace().map(|n| n.parse().expect("a count")).collect();
        assert_eq!(counts.len(), 3, "three counts: {line}");
        let (frame, save, rule) = (counts[0], counts[1], counts[2]);
        assert_eq!((frame, save), (rule, rule), "{limit_kib} KiB: what a frame takes and what a save takes are what the rule gives");
        assert!((fewest..=most).contains(&rule), "{limit_kib} KiB: {rule} threads of 31");
    }
}
