//! What the engine's `.odg` save costs in memory: measured, not claimed, each case in a child process under an address-space limit, as the
//! `odg` crate's `write_memory.rs` runs its children. A process that is out of memory *aborts*, and a child that aborts is a failure of the parent.
//!
//! **What the engine holds.** The writer's gate (`diacraft_odg::write`) counts the model it is given, the file as it is read again beside it, the
//! package it makes and the model's growth since the read. The engine holds more than the writer is told of: the drawing and its undo history (which
//! share what they do not change), the file's bytes (`OpenFile`), the index and the pages as the last save had them, and a copy of the new shapes
//! flattened for the save (`odgfile/flatten.rs`), which the writer **does** count, since the copy is the model it is given. These cases are the
//! ones that show that the gate still sees what the engine holds: a drawing of 300,000 shapes that is edited and saved, one with 150,000 new shapes
//! and one whose cost is in text the reader's figures do not see, each at `ulimit -v 3000000`, end as a save or as a refusal in words, never as an abort.
//!
//! **Measured on beast on 2026-11-19, release build, each child on its own, one at a time, free and under `ulimit -v 3000000`** (both finish; the
//! process's peak resident size, a GB is the kB it prints divided by a million; the child builds its input in the same process; a free run and a limited one differ
//! by 0.3 percent at most). None of these inputs holds a part nothing names, so the read that inflates only what the drawing names left them as they were, and
//! the refusal each ends in is the one its name says, which the `words` of the `RESULT` line show. `rects_300k`: saved, and the second save is no edit, 1.79 GB (
//! 2026-11-22, the test profile: the case was 350,000 rectangles, 1.99 GB, until the `.odg` model was held at 64 bytes an item, and the gate refuses that save now).
//! `rects_350k_all_edited`: refused by the engine's gate, naming the history ("the save would hold more than 2766 MiB of memory: the drawing and the file read again
//! beside it, and about 4..."), 2.43 GB. `new_150k`: refused by the flattening ("more than 6000000 cells and rows to copy"), 1.40 GB. `new_50k`: saved, 2.20 GB.
//! `heavy_250`: refused by the engine's gate ("the save would hold more than 2766 MiB of memory (the drawing and the file read again beside it)"), 1.46 GB. `heavy_100`:
//! saved, 0.53 GB. The three children of `odg_hold.rs`, all refused in words for memory: `format_groups` (four rounds) 1.98 GB, `group` (six edits) 2.44 GB and
//! `saved_pages` (four) 1.91 GB. A save that was made is read back through the reader once the session is gone, and reads.

#![cfg(target_os = "linux")]

use std::process::Command;
use std::sync::Arc;

use diacraft_container::zip::{ArchiveWriter, Method};
use diacraft_engine::{EngineError, Session};
use serde_json::json;

use crate::odg_build::{DRAWING, Dir, Drawing};

const SCENARIO: &str = "ODG_ENGINE_MEMORY_SCENARIO";

/// A line of `/proc/self/status`, in kibibytes: a missing one is a failure and not a zero.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("this test needs /proc/self/status (Linux): {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or_else(|| panic!("no {key} line in /proc/self/status"))
}

/// A package of one page of `n` rectangles, the `k`-th at `k` to a few centimetres, each a short `draw:rect`.
fn rects(n: usize) -> Vec<u8> {
    let body: String = (1..=n)
        .map(|k| {
            format!("<draw:rect draw:name=\"s{k}\" svg:x=\"{}.{}cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\"/>", k % 997, k % 89 + 1)
        })
        .collect();
    Drawing::new().page(&body).bytes()
}

/// A package of `heavy` rectangles each with an attribute of the file's own of 1 MB of random letters (which neither compresses nor shares): the cost of
/// the file is in text the reader's figures of items do not see. With 250 it is about 189 MB.
fn heavy(count: usize, rows: usize) -> Vec<u8> {
    let mut seed = 12_345u64;
    let alphabet = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut random = |len: usize| -> String {
        (0..len)
            .map(|_| {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                alphabet[((seed >> 33) % 62) as usize] as char
            })
            .collect()
    };
    let kept: String = (0..count)
        .map(|k| {
            format!(
                "<draw:rect ext:data=\"{}\" draw:name=\"h{k}\" svg:x=\"{}mm\" svg:y=\"1cm\" svg:width=\"1cm\" svg:height=\"1cm\"/>",
                random(1_000_000),
                k % 100
            )
        })
        .collect();
    // And `rows` custom shapes of every kind of row, each with 2,000 glue points, which put the reader's items near their cap.
    let points: String = (0..2_000).map(|i| format!("{} {}", i * 10, i * 5)).collect::<Vec<_>>().join(" ");
    let path = "M 0 0 L 100 100 C 1 2 3 4 5 6 Q 1 2 3 4 U 10800 10800 100 100 0 360 W 0 0 100 100 10 10 50 50 ".repeat(40);
    let custom: String = (0..rows)
        .map(|k| {
            format!(
                "<draw:custom-shape svg:x=\"{}mm\" svg:width=\"4cm\" svg:height=\"2cm\"><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:modifiers=\"1 2\" draw:glue-points=\"{points}\" draw:text-areas=\"0 0 21600 10800\" draw:enhanced-path=\"{path}\"><draw:handle draw:handle-position-x=\"$0\" draw:handle-position-y=\"top\"/></draw:enhanced-geometry><text:p>t</text:p></draw:custom-shape>",
                k % 200
            )
        })
        .collect();
    let drawing = Drawing::new().page(&format!("{kept}{custom}"));
    // The same package, written by hand so that the part is deflated once.
    let mut w = ArchiveWriter::new();
    w.add("mimetype", DRAWING.as_bytes(), Method::Stored).unwrap();
    w.add("content.xml", drawing.content_xml().as_bytes(), Method::Deflated).unwrap();
    w.add("styles.xml", drawing.styles_xml().as_bytes(), Method::Deflated).unwrap();
    let manifest = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{DRAWING}\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/></manifest:manifest>"
    );
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).unwrap();
    w.finish().unwrap()
}

/// What a save ended as, for the `RESULT` line: `saved`, or the first words of the refusal.
fn how(result: &Result<serde_json::Value, EngineError>) -> String {
    match result {
        Ok(v) => format!("saved(edited={})", v["edited"]),
        Err(e) => {
            let text = e.to_string();
            if text.contains("past a limit") || text.contains("of memory") { "refused(limit)".to_owned() } else { format!("refused({text})") }
        }
    }
}

/// The first words of a refusal, on a `RESULT` line (`-` for a save that was made): which gate refused tells whether the case still reaches the one its name says,
/// since `how` calls every refusal that names a limit or memory `refused(limit)`.
fn words(result: &Result<serde_json::Value, EngineError>) -> String {
    match result {
        Ok(_) => "-".to_owned(),
        Err(e) => e.to_string().replace(['\n', '\r'], " ").chars().take(160).collect(),
    }
}

/// A save that was made gives a file the reader reads under its default limits (a save never writes a file the reader refuses), checked once the session that made it is
/// gone, so that the check costs one read and not a drawing beside it: the child has no more than 3 GB of address space. A `READBACK` line says what the file read as.
fn assert_reads_back(what: &str, bytes: &[u8]) {
    let (read, report) = diacraft_odg::read_partial(bytes, &diacraft_odg::Limits::default());
    println!("\nREADBACK {what} class={} items={} file={}", read.as_ref().err().map_or("read", |e| e.class()), report.work.items, bytes.len());
    assert!(read.is_ok(), "{what}: the file the save made is refused by the reader: {:?}", read.err().map(|e| e.class()));
}

fn open(bytes: &[u8]) -> (Session, Dir) {
    let dir = Dir::new();
    let path = dir.write("a.odg", bytes);
    let mut s = Session::new();
    s.execute("file.open", &json!({ "path": path })).unwrap_or_else(|e| panic!("the file did not open: {e}"));
    (s, dir)
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    println!("\nSTART VmSize={}KiB", status_kib("VmSize"));
    match scenario.as_str() {
        // A drawing of 300,000 rectangles, near the most the save's gate admits of them (350,000 were saved at 54 bytes an item), opened by the engine, one moved, saved.
        "rects_300k" => {
            let bytes = rects(300_000);
            let (mut s, dir) = open(&bytes);
            drop(bytes);
            let first = s.document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().id;
            s.execute("shape.move", &json!({ "ids": [first], "dx": 1.0, "dy": 0.0 })).unwrap();
            let result = s.execute("file.save", &json!({}));
            let ended = how(&result);
            // A second save of the same drawing, through the index the first gave, is no edit (when the first saved).
            let again = result.is_ok().then(|| s.execute("file.save", &json!({})));
            println!(
                "\nRESULT rects_300k class={ended} again={} VmHWM={}KiB VmPeak={}KiB file={}",
                again.as_ref().map_or("none".to_owned(), how),
                status_kib("VmHWM"),
                status_kib("VmPeak"),
                dir.read("a.odg").len()
            );
            assert!(ended.starts_with("saved") || ended == "refused(limit)", "{ended}");
            if result.is_ok() {
                drop(s);
                assert_reads_back("rects_300k", &dir.read("a.odg"));
            }
        }
        // The undo history holds the drawing as it was before an edit, and the engine holds it beside the one it saves: every shape of 350,000 is edited
        // (as a select-all and a move is), so that the history holds a whole second drawing that shares nothing with the first, and the drawing is saved.
        "rects_350k_all_edited" => {
            let bytes = rects(350_000);
            let (mut s, _dir) = open(&bytes);
            drop(bytes);
            let times: usize = std::env::var("ODG_EDITS").ok().and_then(|n| n.parse().ok()).unwrap_or(3);
            for _ in 0..times {
                s.edit("Test edit", |doc| {
                    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
                    let count = page.shapes.len();
                    for at in 0..count {
                        let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
                        let sheet = Arc::make_mut(&mut shape.sheet);
                        let pin = sheet.cells.get("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap();
                        let moved = diacraft_doc::Cell::constant(diacraft_doc::Value::from_text(&format!("{}", pin + 0.5))).with_unit("CM");
                        sheet.cells.insert(diacraft_doc::CellName::new("PinX"), moved);
                    }
                    Ok(())
                })
                .unwrap();
            }
            println!("\nEDITED {times} times: VmHWM={}KiB VmRSS={}KiB", status_kib("VmHWM"), status_kib("VmRSS"));
            let result = s.execute("file.save", &json!({}));
            let history = result.as_ref().err().is_some_and(|e| e.to_string().contains("the engine keeps beside them"));
            println!(
                "\nRESULT {scenario} class={} says_history={history} edits={times} VmHWM={}KiB VmPeak={}KiB words={}",
                how(&result),
                status_kib("VmHWM"),
                status_kib("VmPeak"),
                words(&result)
            );
        }
        // A small drawing and many new shapes: each is an instance of a master that the save flattens (cells, sections and all) into a copy of the
        // drawing, which the writer counts as the model it is given. 150,000 is past the engine's own cap on the flattening and refused by it; 50,000 is
        // under it and held to what the writer's gate allows.
        "new_150k" | "new_50k" => {
            let count: u32 = if scenario == "new_50k" { 50_000 } else { 150_000 };
            let (mut s, dir) = open(&rects(3));
            let made = s.execute("shape.drop", &json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 })).unwrap();
            let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
            s.edit("Test edit", |doc| {
                let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
                let template = page.shapes.iter().find(|x| x.id == id).unwrap().clone();
                for k in 1..count {
                    page.shapes.push(Arc::new(diacraft_doc::Shape { id: id + k, ..(*template).clone() }));
                }
                Ok(())
            })
            .unwrap();
            let result = s.execute("file.save", &json!({}));
            let ended = how(&result);
            println!(
                "\nRESULT {scenario} class={ended} VmHWM={}KiB VmPeak={}KiB words={}",
                status_kib("VmHWM"),
                status_kib("VmPeak"),
                words(&result)
            );
            assert!(ended.starts_with("saved") || ended == "refused(limit)" || ended.contains("too many to write"), "{ended}");
            if result.is_ok() {
                drop(s);
                assert_reads_back(&scenario, &dir.read("a.odg"));
            }
        }
        // The file whose cost is in text: 250 attributes of 1 MB, and 175 shapes with long outlines, which put the reader's items near their cap. The
        // reader reads it; the save that follows is refused in words. With 100 attributes and no outlines the save is made.
        "heavy_250" | "heavy_100" => {
            let bytes = if scenario == "heavy_250" { heavy(250, 175) } else { heavy(100, 0) };
            let (mut s, dir) = open(&bytes);
            drop(bytes);
            let first = s.document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().id;
            s.execute("shape.move", &json!({ "ids": [first], "dx": 1.0, "dy": 0.0 })).unwrap();
            let result = s.execute("file.save", &json!({}));
            let ended = how(&result);
            println!(
                "\nRESULT {scenario} class={ended} VmHWM={}KiB VmPeak={}KiB words={}",
                status_kib("VmHWM"),
                status_kib("VmPeak"),
                words(&result)
            );
            assert!(ended.starts_with("saved") || ended == "refused(limit)", "{ended}");
            if result.is_ok() {
                drop(s);
                assert_reads_back(&scenario, &dir.read("a.odg"));
            }
        }
        other => panic!("no scenario {other}"),
    }
}

/// Runs the child's `scenario` under `ulimit -v limit_kib`: whether it ended well, and what it printed.
fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    run_child_on(scenario, limit_kib, None)
}

/// [`run_child`] with `DIACRAFT_THREADS` set to `threads` in the child (else the child has the parent's environment).
fn run_child_on(scenario: &str, limit_kib: u64, threads: Option<&str>) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact odg_memory::child --ignored --nocapture --test-threads=1");
    let mut child = Command::new("/bin/sh");
    child.arg("-c").arg(script).arg(exe).env(SCENARIO, scenario);
    if let Some(threads) = threads {
        child.env("DIACRAFT_THREADS", threads);
    }
    let out = child.output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

/// The `RESULT` line a child printed.
fn result_line(stdout: &str) -> String {
    stdout.lines().find(|l| l.starts_with("RESULT")).unwrap_or("no RESULT line").to_owned()
}

#[test]
fn an_edit_and_a_save_of_300000_rectangles_through_the_engine_ends_as_a_save_or_a_refusal_and_does_not_abort() {
    let (ended_well, stdout) = run_child("rects_300k", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    let line = result_line(&stdout);
    assert!(line.contains("class=saved(edited=true)") && line.contains("again=saved(edited=false)"), "{line}");
    eprintln!("{line}");
}

#[test]
fn a_save_after_every_shape_of_350000_was_edited_three_times_ends_as_a_save_or_a_refusal_in_words_and_does_not_abort() {
    // Three whole-drawing edits of a drawing of 1.4 GB: each is a copy of every shape, and the step before it holds the other. **The history is held beside the drawing and counted with
    // it**: the engine lets the oldest undo steps go to make room for the edits, so that the save does not meet three earlier drawings (it aborted the process before
    // the engine counted them; it was refused naming the history while the edits were made and kept). What is held is what a save ends as in words, and never an abort.
    let (ended_well, stdout) = run_child("rects_350k_all_edited", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    let line = result_line(&stdout);
    assert!(line.contains("class=saved(") || line.contains("class=refused(limit)"), "{line}");
    eprintln!("{line}");
}

#[test]
fn a_save_with_150000_new_shapes_is_refused_by_the_flattening_in_words_and_does_not_abort() {
    let (ended_well, stdout) = run_child("new_150k", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    let line = result_line(&stdout);
    assert!(line.contains("class=refused(") && line.contains("too many to write to an .odg in one save"), "{line}");
    eprintln!("{line}");
}

#[test]
fn a_save_with_50000_new_shapes_flattened_ends_as_a_save_or_a_refusal_and_does_not_abort() {
    let (ended_well, stdout) = run_child("new_50k", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    eprintln!("{}", result_line(&stdout));
}

/// The same save **asked for 31 threads**. The thumbnail of the page of 50,003 shapes is drawn on threads before the save, and an arena a thread made keeps its address space for
/// the rest of the process; a frame that took the threads half of what the limit left allowed (eleven or more of 80 MiB) aborted the save that followed (12 threads and more aborted, 8 saved
/// at a peak of 2,803,224 KiB of 3,000,000). The threads of a frame, a save and an open are held by one rule (`diacraft_container::spare::threads_fit`).
#[test]
fn a_save_with_50000_new_shapes_flattened_on_31_threads_ends_as_a_save_or_a_refusal_and_does_not_abort() {
    let (ended_well, stdout) = run_child_on("new_50k", 3_000_000, Some("31"));
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    eprintln!("{}", result_line(&stdout));
}

#[test]
fn a_save_of_the_file_with_100_big_attributes_through_the_engine_saves() {
    let (ended_well, stdout) = run_child("heavy_100", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    let line = result_line(&stdout);
    assert!(line.contains("class=saved(edited=true)"), "{line}");
    eprintln!("{line}");
}

#[test]
fn a_save_of_the_file_with_250_big_attributes_through_the_engine_is_refused_in_words_and_does_not_abort() {
    let (ended_well, stdout) = run_child("heavy_250", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    let line = result_line(&stdout);
    assert!(line.contains("class=refused(limit)"), "{line}");
    eprintln!("{line}");
}
