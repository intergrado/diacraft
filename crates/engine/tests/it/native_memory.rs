//! What `.diacraft` costs in memory, in children under `ulimit -v 3000000` (the plan's D3: a refusal is not an abort). A process that is out of memory **aborts**, and a child that
//! aborts is a failure of the parent, so each case runs in a process of its own with an address-space limit, as `odg_memory.rs` does.
//!
//! **The memory is counted per process**: a read and a save are given what the process leaves of the budget (`Limits::beside`), where the process's total holds the open
//! drawing and its file, the open stencils and every other session. The cases: a file at the cap that opens alone; the same file read beside another session (a hold of 1.9 GB, as a drawing of
//! 80,000 shapes and its history hold), which is refused in words that say what is held; a save beside it, refused in words and writing nothing; and two sessions, each with a drawing, one of which
//! opens a large file. **Each child asserts its outcome** (refused for memory, and in words that say what is held), so a read or a save that ignored what the process holds fails.

#![cfg(target_os = "linux")]

use std::process::Command;
use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Format, Part, PartRole, RawAttrs, Value};
use diacraft_engine::memory::Hold;
use diacraft_engine::{EngineError, Session};
use diacraft_native::{Kind, State, WriteOptions};
use serde_json::json;

use crate::native_common::Dir;

const SCENARIO: &str = "NATIVE_ENGINE_MEMORY_SCENARIO";

/// A line of `/proc/self/status`, in kibibytes: a missing one is a failure and not a zero.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("this test needs /proc/self/status (Linux): {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or_else(|| panic!("no {key} line in /proc/self/status"))
}

/// A document of one blank page holding `count` parts of `each` bytes that nothing models (embeddings, kept as bytes and charged to the blob pool). Each begins as a ZIP file does, so that the
/// package stores them and does not spend minutes deflating what does not shrink. A part of an entry may be at most 256 MiB, and the blobs of an open 384 MiB in all.
fn heavy_document(count: usize, each: usize) -> Document {
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut doc = Document::new_blank();
    for k in 0..count {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.try_reserve_exact(each).expect("the child has room for its own input");
        bytes.extend_from_slice(b"PK\x03\x04");
        while bytes.len() < each {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            bytes.extend_from_slice(&seed.to_le_bytes());
        }
        bytes.truncate(each);
        doc.parts.push(Arc::new(Part {
            format: Format::Native,
            name: Arc::from(format!("embeddings/big{k}.bin")),
            media_type: Arc::from("application/octet-stream"),
            role: PartRole::Other,
            manifest: RawAttrs::default(),
            rels: Arc::new(Vec::new()),
            bytes: Arc::from(bytes),
        }));
    }
    doc
}

/// The package of [`heavy_document`].
fn big_package(count: usize, each: usize) -> Vec<u8> {
    big_package_of(Kind::Drawing, count, each)
}

/// The package of [`heavy_document`], of the `kind` (a template, to start a drawing from).
fn big_package_of(kind: Kind, count: usize, each: usize) -> Vec<u8> {
    let doc = heavy_document(count, each);
    diacraft_native::write(&doc, &State::default(), None, &WriteOptions { kind, ..WriteOptions::default() })
        .unwrap_or_else(|e| panic!("the package of {count} parts of {each} bytes is not written: {e}"))
        .bytes
}

/// The edit of everything: the move of every shape of the first page (each shape and its sheet is copied), one step of the history.
fn nudge_all(s: &mut Session, x: usize) -> Result<(), EngineError> {
    s.edit("nudge", |doc| {
        let Some(page) = doc.pages.get_mut(0) else { return Ok(()) };
        let page = Arc::make_mut(page);
        for i in 0..page.shapes.len() {
            if let Some(shape) = page.shapes.get_mut(i) {
                Arc::make_mut(&mut Arc::make_mut(shape).sheet).cells.insert(CellName::new("PinX"), Cell::constant(Value::from_text(&format!("{x}"))));
            }
        }
        Ok(())
    })
}

/// A VSDX file of one page of `count` shapes, of the ids 1 to `count`.
fn many_shapes_vsdx(count: u32) -> Vec<u8> {
    let shapes: String = (1..=count)
        .map(|k| {
            diacraft_testkit::package::shape(
                k,
                &format!(
                    r##"<Cell N="PinX" V="{}"/><Cell N="PinY" V="{}"/><Cell N="Width" V="0.5"/><Cell N="Height" V="0.25"/><Cell N="FillForegnd" V="#aabbcc"/>"##,
                    1 + k % 200,
                    1 + k / 200
                ),
            )
        })
        .collect();
    diacraft_testkit::package::VsdxBuilder::new()
        .page(diacraft_testkit::package::PageSpec::new(1, "P").sheet(r#"<Cell N="PageWidth" V="11"/><Cell N="PageHeight" V="8.5"/>"#).shapes(&shapes))
        .build()
}

/// What an open or a save ended as, for the `RESULT` line: `ok`, or the first words of the refusal.
fn how<T>(result: &Result<T, EngineError>) -> String {
    match result {
        Ok(_) => "ok".to_owned(),
        Err(e) => e.to_string().replace(['\n', '\r'], " ").chars().take(200).collect(),
    }
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    let dir = Dir::new();
    println!("\nSTART VmSize={}KiB", status_kib("VmSize"));
    match scenario.as_str() {
        // A file of 340 MB of two parts that nothing models: the package is within the file cap, and a session alone opens it and gives its part back as bytes.
        "alone" => {
            let path = dir.write("big.diacraft", &big_package(2, 170 << 20));
            let mut s = Session::new();
            let result = s.execute("file.open", &json!({ "path": path }));
            println!("\nRESULT alone class={} VmHWM={}KiB VmPeak={}KiB", how(&result), status_kib("VmHWM"), status_kib("VmPeak"));
            assert!(result.is_ok(), "{}", how(&result));
            assert_eq!(s.document().unwrap().parts.len(), 2);
            assert_eq!(result.unwrap()["file"]["skipped_count"], 2);
        }
        // The same file beside what the rest of the process holds (another session's drawing and its history, as a hold of 1.9 GB): the read is given what is left of the budget, and a
        // file that needs more is refused in words that say what is held. The open drawing is as it was.
        "beside" => {
            let path = dir.write("big.diacraft", &big_package(2, 170 << 20));
            let mut s = Session::new();
            s.execute("file.new", &json!({})).unwrap();
            s.execute("page.add", &json!({"name": "Kept"})).unwrap();
            let kept = s.document().cloned();
            let others = Hold::new(1_900_000_000);
            let result = s.execute("file.open", &json!({ "path": path }));
            let words = how(&result);
            println!("\nRESULT beside class={words} VmHWM={}KiB VmPeak={}KiB", status_kib("VmHWM"), status_kib("VmPeak"));
            // **The outcome is asserted, not accepted**: the same file opens alone (the child above), so beside this hold it is refused, for memory and in words that say so. A read that was
            // not given what the process leaves of the budget (`Limits::beside`) would open it, and this fails.
            match &result {
                Err(EngineError::CannotOpen { problem, .. }) => {
                    assert_eq!(problem.word(), "out_of_memory", "{words}");
                    assert!(words.contains("not enough memory beside what is open"), "{words}");
                    assert_eq!(s.document().cloned(), kept, "the open drawing is as it was");
                }
                other => panic!("the file opened or was refused for another reason beside a hold of 1.9 GB: {other:?}"),
            }
            // With the hold given back, the same file opens.
            drop(others);
            let opened = s.execute("file.open", &json!({ "path": path }));
            assert!(opened.is_ok(), "{}", how(&opened));
        }
        // A save beside what the rest of the process holds, of a drawing that is not small: it is refused in words and writes nothing, and the same drawing saves once the hold is given back.
        "save_beside" => {
            let mut s = Session::new();
            s.execute("file.new", &json!({})).unwrap();
            let doc = heavy_document(1, 120 << 20);
            s.edit("Test edit", |d| {
                *d = doc.clone();
                Ok(())
            })
            .unwrap();
            let others = Hold::new(2_300_000_000);
            let target = dir.path("out.diacraft");
            let result = s.execute("file.save_as", &json!({ "path": target }));
            let words = how(&result);
            println!("\nRESULT save_beside class={words} VmHWM={}KiB VmPeak={}KiB", status_kib("VmHWM"), status_kib("VmPeak"));
            // **Asserted, not accepted**: a save that is given the room the process leaves (`Limits::for_save`) is refused beside this hold, in words, and writes nothing; the same drawing
            // saves alone (below).
            match &result {
                Err(e) => {
                    assert!(e.to_string().contains("not enough memory beside what is open"), "{words}");
                    assert!(!dir.files().contains(&"out.diacraft".to_owned()), "a refused save writes nothing");
                }
                Ok(_) => panic!("the save was made beside a hold of 2.3 GB"),
            }
            drop(others);
            let saved = s.execute("file.save_as", &json!({ "path": target }));
            assert!(saved.is_ok(), "alone it saves: {}", how(&saved));
            assert!(diacraft_native::sniff(&dir.read("out.diacraft")).is_some());
        }
        // Two sessions, each with a drawing; the one opens a file of 200 MB beside the other, in a process that holds both: opened, or refused in words, and the other session is untouched.
        "two_sessions" => {
            let path = dir.write("big.diacraft", &big_package(1, 200 << 20));
            let mut a = Session::new();
            a.execute("file.new", &json!({})).unwrap();
            for k in 0..200 {
                a.execute(
                    "shape.drop",
                    &json!({"master": "rectangle", "page": 1, "x": 1.0 + f64::from(k % 40) * 0.2, "y": 1.0 + f64::from(k / 40) * 0.5}),
                )
                .unwrap();
            }
            let others = Hold::new(1_500_000_000);
            let mut b = Session::new();
            b.execute("file.new", &json!({})).unwrap();
            let held_before = diacraft_engine::memory::held();
            let result = b.execute("file.open", &json!({ "path": path }));
            println!(
                "\nRESULT two_sessions class={} VmHWM={}KiB VmPeak={}KiB held={held_before}",
                how(&result),
                status_kib("VmHWM"),
                status_kib("VmPeak")
            );
            // Asserted: beside the other session's drawing and a hold of 1.5 GB the file of 200 MB is refused for memory, in words, and the other session is untouched.
            match &result {
                Err(EngineError::CannotOpen { problem, .. }) => assert_eq!(problem.word(), "out_of_memory", "{}", how(&result)),
                other => panic!("the file opened or was refused for another reason: {other:?}"),
            }
            assert_eq!(a.document().unwrap().pages.iter().next().unwrap().shapes.len(), 200);
            drop(others);
            // With nothing held beside it the same file opens.
            let opened = b.execute("file.open", &json!({ "path": path }));
            assert!(opened.is_ok(), "{}", how(&opened));
            assert_eq!(a.document().unwrap().pages.iter().next().unwrap().shapes.len(), 200);
        }
        // (c): the pass that makes the content stamp is made only when it fits beside what the process holds. A VSDX file is opened, and the rest of the process holds 1.7 GB, which
        // leaves no room for the tree of a part (the room is 1,622,855,680 bytes): the stamp is none, no pass is made, and the drawing reads as edited. The answer is kept, not asked again.
        "stamp_no_room" => {
            let path = dir.write("a.vsdx", &crate::native_common::vsdx_drawing());
            let mut s = Session::new();
            s.execute("file.open", &json!({ "path": path })).unwrap();
            assert!(s.content_stamp_pending());
            let others = Hold::new(1_700_000_000);
            assert_eq!(s.content_stamp(), None, "no room beside 1.7 GB: no stamp");
            assert_eq!(s.stamp_work().passes, 0, "and no pass was made");
            assert!(!s.content_stamp_pending(), "the question is answered");
            assert_eq!(s.content_unchanged(), None, "a model writer reads it as edited");
            drop(others);
            assert_eq!(s.content_stamp(), None, "the answer is kept");
            // A session that is asked with room has one.
            let mut t = Session::new();
            t.execute("file.open", &json!({ "path": path })).unwrap();
            assert!(t.content_stamp().is_some());
            assert_eq!(t.stamp_work().passes, 1);
            println!("\nRESULT stamp_no_room class=ok VmHWM={}KiB VmPeak={}KiB", status_kib("VmHWM"), status_kib("VmPeak"));
        }
        "kept_read" => kept_read(&dir),
        "template_beside" => template_beside(&dir),
        other => panic!("no scenario {other}"),
    }
}

/// The blocker of the re-review: a VSDX drawing is opened and **everything in it is edited** (a move of every shape, enough times that the history's own limit of bytes lets go of
/// the step that held the document the drawing was read as, so that **only the pending stamp holds it**), and saved; a second file is then opened beside it. The document the stamp keeps
/// is memory the process holds, and is counted: the read is given what is left of the budget beside it, which holds a file of 150 MB, so it **opens, and the child does not abort**. The save made no pass, and the
/// drawing still reads as edited when it is asked.
fn kept_read(dir: &Dir) {
    let count = 30_000u32;
    let a_path = dir.write("a.vsdx", &many_shapes_vsdx(count));
    let second = dir.write("second.diacraft", &big_package(1, 150 << 20));
    let mut a = Session::new();
    a.execute("file.open", &json!({ "path": a_path })).unwrap();
    assert!(a.content_stamp_pending());
    for k in 0..8 {
        nudge_all(&mut a, k + 1).unwrap_or_else(|e| panic!("nudge {k}: {e}"));
    }
    a.execute("file.save", &json!({})).unwrap();
    assert_eq!(a.stamp_work().passes, 0, "the save made no pass");
    assert!(a.content_stamp_pending());
    let steps = a.history().unwrap().undo_len();
    assert!(
        steps < 8,
        "the history let go of the step that held the document the drawing was read as, so that only the stamp holds it: {steps} steps are left"
    );
    let held = a.model_held().unwrap();
    let exact = a.model_held_exact().unwrap();
    assert!(held >= exact, "the count is not under what is held: {held} against {exact}");
    let before = diacraft_engine::memory::held();
    let mut b = Session::new();
    let result = b.execute("file.open", &json!({ "path": second }));
    println!(
        "\nRESULT kept_read class={} held={held} exact={exact} process_before={before} VmHWM={}KiB VmPeak={}KiB",
        how(&result),
        status_kib("VmHWM"),
        status_kib("VmPeak")
    );
    // **Opened**: 560 MB is held and the file is 150 MB of a budget of 2.5 GB, which a process that did not keep the document the drawing was read as opens as well. (Never an abort:
    // the child would be gone.) The first session is untouched.
    assert!(result.is_ok(), "{}", how(&result));
    assert_eq!(a.document().unwrap().pages.iter().next().unwrap().shapes.len(), count as usize);
    assert_eq!(a.content_unchanged(), Some(false), "edited after it was read, and saved");
    assert_eq!(a.stamp_work().passes, 2, "the stamp from the document the drawing was read as, and the content now");
}

/// `start_from_native` (`file.new` of a `.diacraft-template` path): the read takes the room beside what the process holds, as `file.open` does. A template of 340 MB beside a hold
/// of 1.9 GB is refused for memory, in words, and the drawing that was open is as it was; with the hold given back it starts a drawing.
fn template_beside(dir: &Dir) {
    let path = dir.write("big.diacraft-template", &big_package_of(Kind::Template, 2, 170 << 20));
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("page.add", &json!({"name": "Kept"})).unwrap();
    let kept = s.document().cloned();
    let others = Hold::new(1_900_000_000);
    let result = s.execute("file.new", &json!({ "template": path }));
    let words = how(&result);
    println!("\nRESULT template_beside class={words} VmHWM={}KiB VmPeak={}KiB", status_kib("VmHWM"), status_kib("VmPeak"));
    match &result {
        Err(EngineError::CannotOpen { problem, .. }) => {
            assert_eq!(problem.word(), "out_of_memory", "{words}");
            assert!(words.contains("not enough memory beside what is open"), "{words}");
            assert_eq!(s.document().cloned(), kept, "the open drawing is as it was");
        }
        other => panic!("the template started a drawing or was refused for another reason beside a hold of 1.9 GB: {other:?}"),
    }
    drop(others);
    let started = s.execute("file.new", &json!({ "template": path }));
    assert!(started.is_ok(), "alone it starts a drawing: {}", how(&started));
}

/// Runs the child's `scenario` under `ulimit -v limit_kib`: whether it ended well, and what it printed.
fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact native_memory::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

/// The `RESULT` line a child printed.
fn result_line(stdout: &str) -> String {
    stdout.lines().find(|l| l.contains("RESULT")).unwrap_or("no RESULT line").to_owned()
}

#[test]
fn a_diacraft_file_of_340_mb_opens_alone_under_the_address_limit() {
    let (ended_well, stdout) = run_child("alone", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    let line = result_line(&stdout);
    assert!(line.contains("class=ok"), "{line}");
    eprintln!("{line}");
}

#[test]
fn the_same_file_beside_what_the_process_holds_is_refused_for_memory_in_words_and_opens_once_it_is_given_back() {
    let (ended_well, stdout) = run_child("beside", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    let line = result_line(&stdout);
    assert!(!line.contains("no RESULT"), "{stdout}");
    eprintln!("{line}");
}

#[test]
fn a_save_beside_what_the_process_holds_is_refused_in_words_and_writes_nothing_and_saves_once_it_is_given_back() {
    let (ended_well, stdout) = run_child("save_beside", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    eprintln!("{}", result_line(&stdout));
}

#[test]
fn a_content_stamp_that_does_not_fit_beside_what_the_process_holds_is_none_and_reads_as_edited() {
    let (ended_well, stdout) = run_child("stamp_no_room", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert!(result_line(&stdout).contains("class=ok"), "{stdout}");
}

#[test]
fn two_sessions_each_with_a_drawing_share_one_budget_and_a_file_beside_them_is_refused_for_memory_in_words() {
    let (ended_well, stdout) = run_child("two_sessions", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    eprintln!("{}", result_line(&stdout));
}

#[test]
fn a_vsdx_edited_everywhere_and_saved_keeps_the_document_it_was_read_as_counted_and_a_second_file_beside_it_never_aborts() {
    let (ended_well, stdout) = run_child("kept_read", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    let line = result_line(&stdout);
    assert!(!line.contains("no RESULT"), "{stdout}");
    eprintln!("{line}");
}

#[test]
fn a_template_started_from_a_diacraft_file_takes_its_room_beside_what_the_process_holds_and_is_refused_in_words() {
    let (ended_well, stdout) = run_child("template_beside", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert!(!result_line(&stdout).contains("no RESULT"), "{stdout}");
}
