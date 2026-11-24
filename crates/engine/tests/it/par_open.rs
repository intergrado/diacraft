//! What the threads of an open cost and buy, and that an open is the same at any count of them. **The timings are `#[ignore]`d**: run by hand, alone, in a release build, one process for each thread count (the count is the engine's, from
//! `DIACRAFT_THREADS`, read once for a process),
//! `PAR_FORMAT=vsdx PAR_SHAPES=12000 DIACRAFT_THREADS=4 cargo test --release -p diacraft-engine --test it -- --ignored par_open_times --nocapture --test-threads=1`.
//! `PAR_FORMAT` is `vsdx`, `odg`, `diacraft` (the same drawing as `vsdx` saved as `.diacraft`) or `pictures` (a drawing that holds large pictures, as `.vsdx`); `PAR_SHAPES` the shapes,
//! in `PAR_PAGES` pages (three), and `PAR_RUNS` the runs (five). It prints the least of the runs of `file.open`, the machine's load before and after, and the size of the file.

use std::process::Command;
use std::time::Instant;

use diacraft_engine::Session;
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder, typical_shape};
use serde_json::json;

use crate::native_common::Dir;
use crate::odg_build;

/// The machine's load, the average of the last minute (`/proc/loadavg`).
fn load() -> f64 {
    std::fs::read_to_string("/proc/loadavg").ok().and_then(|t| t.split_whitespace().next().and_then(|v| v.parse().ok())).unwrap_or(0.0)
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

/// A VSDX drawing of `pages` pages and `shapes` shapes in all, with `pictures` pictures of `each` bytes in its package.
fn vsdx_with(shapes: u32, pages: u32, pictures: usize, each: usize) -> Vec<u8> {
    let per = shapes.div_ceil(pages.max(1));
    let mut builder = VsdxBuilder::new();
    for page in 1..=pages {
        let body: String = (0..per).map(|i| typical_shape((page - 1) * per + i)).collect();
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

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0)
}

/// The file of a scenario: made in `dir`, or, when `PAR_KEEP` names a folder, made there once as `<format>-<shapes>-<pages>` and used again (a profile of an open holds no work of making the file).
fn scenario_file(dir: &Dir, format: &str, shapes: u32, pages: u32) -> String {
    let extension = match format {
        "odg" | "odg-pictures" => "odg",
        "diacraft" | "diacraft-pictures" => "diacraft",
        _ => "vsdx",
    };
    let kept = std::env::var("PAR_KEEP").ok().map(|folder| {
        let _ = std::fs::create_dir_all(&folder);
        format!("{folder}/{format}-{shapes}-{pages}.{extension}")
    });
    if let Some(kept) = kept.as_ref().filter(|p| std::path::Path::new(p).exists()) {
        return kept.clone();
    }
    let made = match format {
        "vsdx" => dir.write("a.vsdx", &vsdx_with(shapes, pages, 0, 0)),
        "pictures" => dir.write("a.vsdx", &vsdx_with(shapes / 10, pages, 8, 16 << 20)),
        "odg" => dir.write("a.odg", &odg_with(shapes as usize, pages as usize)),
        // The same drawing with large pictures, as an `.odg` (the pictures are files of the package, kept in the model) and as a `.diacraft` (the pictures are blobs).
        "odg-pictures" => {
            let mut drawing = odg_build::Drawing::new();
            for page in 0..pages as usize {
                let body: String = (0..shapes as usize / pages as usize)
                    .map(|i| {
                        odg_build::rect(&format!("s{page}_{i}"), (f64::from((i % 40) as u32), f64::from((i / 40 % 60) as u32), 0.9, 0.6), "Label")
                    })
                    .collect();
                drawing = drawing.page(&body);
            }
            for k in 0..8u64 {
                let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
                bytes.extend(picture(16 << 20, 0x9E37_79B9_7F4A_7C15 ^ (k + 1)));
                drawing = drawing.file(&format!("Pictures/p{k}.png"), &bytes);
            }
            dir.write("a.odg", &drawing.bytes())
        }
        "diacraft" | "diacraft-pictures" => {
            let source = if format == "diacraft" {
                dir.write("a.vsdx", &vsdx_with(shapes, pages, 0, 0))
            } else {
                dir.write("a.vsdx", &vsdx_with(shapes, pages, 8, 16 << 20))
            };
            let mut s = Session::new();
            s.execute("file.open", &json!({ "path": source })).unwrap_or_else(|e| panic!("open: {e}"));
            let out = dir.path("a.diacraft");
            s.execute("file.save_as", &json!({ "path": out })).unwrap_or_else(|e| panic!("save: {e}"));
            out
        }
        other => panic!("no scenario {other}"),
    };
    match kept {
        Some(kept) => {
            std::fs::copy(&made, &kept).unwrap_or_else(|e| panic!("keep {kept}: {e}"));
            kept
        }
        None => made,
    }
}

#[test]
#[ignore = "timed: run by hand, alone, in a release build, with the scenario in the environment"]
fn par_open_times() {
    let env = |key: &str, default: &str| std::env::var(key).unwrap_or_else(|_| default.to_owned());
    let format = env("PAR_FORMAT", "vsdx");
    let shapes: u32 = env("PAR_SHAPES", "12000").parse().unwrap();
    let pages: u32 = env("PAR_PAGES", "3").parse().unwrap();
    let runs: usize = env("PAR_RUNS", "5").parse().unwrap();
    let dir = Dir::new();
    let path = scenario_file(&dir, &format, shapes, pages);
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let before = load();
    let mut times: Vec<f64> = Vec::new();
    for run in 0..runs {
        let mut s = Session::new();
        let started = Instant::now();
        let result = s.execute("file.open", &json!({ "path": path }));
        let spent = started.elapsed().as_secs_f64() * 1000.0;
        assert!(result.is_ok(), "open {run}: {:?}", result.err().map(|e| e.to_string().chars().take(300).collect::<String>()));
        times.push(spent);
    }
    let least = times.iter().copied().fold(f64::INFINITY, f64::min);
    println!(
        "PAR_TIME format={format} shapes={shapes} pages={pages} threads={} best_ms={least:.0} all={:?} load_before={before:.1} load_after={:.1} bytes={size} vm_peak_mib={} rss_peak_mib={}",
        std::env::var("DIACRAFT_THREADS").unwrap_or_else(|_| "default".to_owned()),
        times.iter().map(|t| t.round() as u64).collect::<Vec<_>>(),
        load(),
        status_kib("VmPeak") >> 10,
        status_kib("VmHWM") >> 10,
    );
}

/// The stages of an open of a `.vsdx` as the reader gives them, for where the time goes (`PAR_STAGES=1`): the package read alone, then the document read from the bytes.
#[test]
#[ignore = "timed: run by hand"]
fn par_open_stages() {
    let env = |key: &str, default: &str| std::env::var(key).unwrap_or_else(|_| default.to_owned());
    let (shapes, pages): (u32, u32) = (env("PAR_SHAPES", "12000").parse().unwrap(), env("PAR_PAGES", "3").parse().unwrap());
    let dir = Dir::new();
    let path = scenario_file(&dir, &env("PAR_FORMAT", "vsdx"), shapes, pages);
    let bytes = std::fs::read(&path).unwrap();
    let limits = diacraft_vsdx::Limits::default();
    for _ in 0..3 {
        let t = Instant::now();
        let package = diacraft_opc::Package::read(&bytes, &limits.opc).unwrap();
        let package_ms = t.elapsed().as_secs_f64() * 1000.0;
        drop(package);
        let t = Instant::now();
        let (doc, _) = diacraft_vsdx::read(&bytes, &limits).unwrap();
        let read_ms = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        let held = diacraft_sheetxml::model::held_bytes(&doc);
        let held_ms = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        drop(doc);
        let drop_ms = t.elapsed().as_secs_f64() * 1000.0;
        println!("PAR_STAGES package_ms={package_ms:.0} read_ms={read_ms:.0} held_bytes_ms={held_ms:.0} drop_ms={drop_ms:.0} held={}", held >> 20);
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ the same open at any count of threads

const CHILD: &str = "PAR_OPEN_CHILD";

/// The child of the test below: opens the file named in `PAR_OPEN_CHILD` through the engine (`DIACRAFT_THREADS` is the count) and prints what the open said and a hash of the document it made.
#[test]
#[ignore = "run by the test below in a process of its own"]
fn par_open_child() {
    let Ok(path) = std::env::var(CHILD) else { return };
    let mut s = Session::new();
    let said = match s.execute("file.open", &json!({ "path": path })) {
        Ok(value) => value.to_string(),
        Err(e) => format!("refused: {e}"),
    };
    let doc = s.document().map(|d| format!("{d:?}")).unwrap_or_default();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in doc.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    println!("PAR_OPEN_DONE {said} document={hash:016x} len={}", doc.len());
}

/// What a process of its own says it opened at `threads`.
fn opened_at(path: &str, threads: usize) -> String {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let out = Command::new(exe)
        .args(["--exact", "par_open::par_open_child", "--ignored", "--nocapture", "--test-threads=1"])
        .env(CHILD, path)
        .env("DIACRAFT_THREADS", threads.to_string())
        .output()
        .expect("run the test binary");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let line = text.lines().find_map(|l| l.find("PAR_OPEN_DONE ").map(|at| l[at..].to_owned()));
    line.unwrap_or_else(|| panic!("the child ended with {:?}: {text} {}", out.status, String::from_utf8_lossy(&out.stderr)))
}

#[test]
fn a_file_opened_by_the_engine_is_the_same_document_and_the_same_words_at_any_number_of_threads() {
    // A drawing of each format, and a file the open refuses, opened in a process for each count of threads (`DIACRAFT_THREADS`, read once for a process): the result of `file.open` and the document
    // are the same at 1, 2, 7 and 31.
    let dir = Dir::new();
    let vsdx = dir.write("a.vsdx", &vsdx_with(600, 12, 0, 0));
    let odg = dir.write("a.odg", &odg_with(600, 6));
    let pictures = dir.write("b.vsdx", &vsdx_with(60, 6, 6, 200_000));
    let diacraft = {
        let mut s = Session::new();
        s.execute("file.open", &json!({ "path": pictures })).unwrap_or_else(|e| panic!("open: {e}"));
        let out = dir.path("a.diacraft");
        s.execute("file.save_as", &json!({ "path": out })).unwrap_or_else(|e| panic!("save: {e}"));
        out
    };
    let broken = {
        let mut bytes = std::fs::read(&vsdx).unwrap();
        let at = bytes.len() / 2;
        bytes[at] ^= 0xFF;
        dir.write("broken.vsdx", &bytes)
    };
    for (what, path) in [("vsdx", &vsdx), ("odg", &odg), ("vsdx with pictures", &pictures), ("diacraft", &diacraft), ("a damaged vsdx", &broken)] {
        let one = opened_at(path, 1);
        for threads in [2, 7, 31] {
            assert_eq!(opened_at(path, threads), one, "{what} at {threads} threads");
        }
        assert!(what == "a damaged vsdx" || !one.contains("refused"), "{what}: {one}");
    }
}
