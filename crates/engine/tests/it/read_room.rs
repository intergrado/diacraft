//! The room for a file is taken before the file is loaded (the scan's MEDIUM on main after a change): in children under `ulimit -v 3000000`, as `native_memory.rs`
//! and `vsdx_memory.rs` run theirs, since a process that is out of memory *aborts*.
//!
//! **The regression.** `file.open` loaded the file whole, up to the package cap, **before** its read took or waited for its reservation of the budget, so a read that was running held all the
//! budget while a second open loaded its file beside it, and two large opens at the same moment held two files at once and could pass 3 GB. Now the size of the file is looked at first (a path
//! that names nothing is refused at once, before a read that is running is waited for), the reservation is taken, **a file larger than the room it holds is refused from its size, in words, and not
//! a byte of it is read**, and the file is loaded held to that room. The children read files of 900 MB (junk: they are refused after they are read, which is the point), and the peak of the
//! process says whether a file was loaded: `VmHWM` stays small when it was not, and is one file's when two opens ran one after the other.
//!
//! The cases: a read that holds the budget, and a second open of a 900 MB file by each of the four ways a file is read (`file.open` of a VSDX-looking file and of a `.diacraft`-looking one,
//! `file.new` of a template path, `stencil.open`): refused in words (`out_of_memory`) and never loaded; and two sessions opening a 900 MB file each at the same moment, on two threads: each ends
//! as a refusal in words, the child never aborts, and its address space peaks at one file's, not two.

#![cfg(target_os = "linux")]

use std::io::Write;
use std::process::Command;
use std::time::Duration;

use diacraft_engine::{EngineError, Session};
use serde_json::json;

use crate::native_common::Dir;

const SCENARIO: &str = "READ_ROOM_SCENARIO";

/// A line of `/proc/self/status`, in kibibytes: a missing one is a failure and not a zero.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("this test needs /proc/self/status (Linux): {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or_else(|| panic!("no {key} line in /proc/self/status"))
}

/// The first bytes of a `.diacraft` file: a stored `mimetype` entry that names the drawing type, as a file manager's magic rule reads it. What follows is junk.
fn diacraft_head() -> Vec<u8> {
    let media = b"application/vnd.diacraft.drawing";
    let mut head = Vec::new();
    head.extend_from_slice(&[0x50, 0x4B, 0x03, 0x04, 10, 0, 0, 0, 0, 0, 0, 0, 0x21, 0]);
    head.extend_from_slice(&[0, 0, 0, 0]);
    head.extend_from_slice(&u32::try_from(media.len()).unwrap().to_le_bytes());
    head.extend_from_slice(&u32::try_from(media.len()).unwrap().to_le_bytes());
    head.extend_from_slice(&8u16.to_le_bytes());
    head.extend_from_slice(&0u16.to_le_bytes());
    head.extend_from_slice(b"mimetype");
    head.extend_from_slice(media);
    head
}

/// Writes a file of `size` bytes that starts with `head` and goes on as junk, a megabyte at a time: the child never holds it.
fn write_junk(dir: &Dir, name: &str, head: &[u8], size: u64) -> String {
    let path = dir.path(name);
    let mut file = std::fs::File::create(&path).unwrap();
    let mut chunk = vec![0u8; 1 << 20];
    let mut seed = 0x2545_F491_4F6C_DD1Du64 ^ size;
    for byte in &mut chunk {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        *byte = seed.to_le_bytes()[0];
    }
    file.write_all(head).unwrap();
    let mut written = head.len() as u64;
    while written < size {
        let take = usize::try_from((size - written).min(chunk.len() as u64)).unwrap();
        file.write_all(&chunk[..take]).unwrap();
        written += take as u64;
    }
    file.sync_all().unwrap();
    path
}

/// What an open ended as, for the `RESULT` line.
fn how<T>(result: &Result<T, EngineError>) -> String {
    match result {
        Ok(_) => "ok".to_owned(),
        Err(e) => e.to_string().replace(['\n', '\r'], " ").chars().take(220).collect(),
    }
}

const MB900: u64 = 900 << 20;

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    let dir = Dir::new();
    println!("\nSTART VmSize={}KiB", status_kib("VmSize"));
    match scenario.as_str() {
        // A read is running and holds all the budget: a second open of a file of 900 MB, by each of the four ways a file is read, is refused from its size, in words that say a file is being
        // read, and is never loaded: the peak of the process stays far under one file's.
        "refused_unread" => {
            let vsdx = write_junk(&dir, "big.vsdx", b"PK\x03\x04junk", MB900);
            let native = write_junk(&dir, "big.diacraft", &diacraft_head(), MB900);
            let running = diacraft_engine::memory::reserve_read();
            assert_eq!(running.beside_reads(), 0);
            diacraft_engine::memory::set_read_wait(Duration::ZERO);
            let mut words = Vec::new();
            for path in [&vsdx, &native] {
                let mut s = Session::new();
                let opened = s.execute("file.open", &json!({ "path": path }));
                words.push(("file.open", how(&opened)));
                match &opened {
                    Err(EngineError::CannotOpen { problem, .. }) => assert_eq!(problem.word(), "out_of_memory", "{}", how(&opened)),
                    other => panic!("file.open of {path} beside a read that holds the budget: {other:?}"),
                }
                let started = s.execute("file.new", &json!({ "template": path }));
                words.push(("file.new", how(&started)));
                assert!(matches!(&started, Err(EngineError::CannotOpen { problem, .. }) if problem.word() == "out_of_memory"), "{}", how(&started));
                let stencil = s.execute("stencil.open", &json!({ "path": path }));
                words.push(("stencil.open", how(&stencil)));
                assert!(matches!(&stencil, Err(EngineError::CannotOpen { problem, .. }) if problem.word() == "out_of_memory"), "{}", how(&stencil));
            }
            let peak = status_kib("VmHWM");
            println!(
                "\nRESULT refused_unread class=ok VmHWM={peak}KiB VmPeak={}KiB first={}",
                status_kib("VmPeak"),
                words.first().map(|w| w.1.clone()).unwrap_or_default()
            );
            assert!(
                words.iter().all(|(_, w)| w.contains("not enough memory beside what is open") && w.contains("another file is being read")),
                "{words:?}"
            );
            assert!(
                peak < 400_000 && status_kib("VmPeak") < 700_000,
                "a file of 900 MB was loaded: the peak is {peak} KiB resident, {} KiB of address space",
                status_kib("VmPeak")
            );
            drop(running);
            // With the budget given back the same file is loaded, and refused as what it is: junk.
            let mut s = Session::new();
            let junk = s.execute("file.open", &json!({ "path": vsdx }));
            assert!(junk.is_err() && !how(&junk).contains("not enough memory"), "{}", how(&junk));
            assert!(status_kib("VmHWM") > 800_000, "and it was read this time");
        }
        // Two sessions on two threads open a file of 900 MB each at the same moment: the second waits for the first to give back the room it holds, so one file is loaded at a time. Each ends as
        // a refusal in words (the files are junk), the child never aborts, and its peak is one file's.
        "two_at_once" => {
            let a = write_junk(&dir, "a.vsdx", b"PK\x03\x04junk", MB900);
            let b = write_junk(&dir, "b.diacraft", &diacraft_head(), MB900);
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let spawn = |path: String| {
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let mut s = Session::new();
                    barrier.wait();
                    how(&s.execute("file.open", &json!({ "path": path })))
                })
            };
            let (first, second) = (spawn(a), spawn(b));
            let (first, second) = (first.join().expect("the first thread did not abort"), second.join().expect("the second thread did not abort"));
            let peak = status_kib("VmHWM");
            println!("\nRESULT two_at_once class=ok VmHWM={peak}KiB VmPeak={} first={first} second={second}", status_kib("VmPeak"));
            for words in [&first, &second] {
                assert!(words.contains("cannot open") || words.contains("failed"), "{words}");
                assert!(
                    words.contains("not a drawing file")
                        || words.contains("too big")
                        || words.contains("damaged")
                        || words.contains("not enough memory"),
                    "in words: {words}"
                );
            }
            // The address space is what the limit is on, and a file's buffer is reserved whole before it is filled (the resident peak of two threads that overlap only in part is less than that).
            let reserved = status_kib("VmPeak");
            assert!(reserved < 1_500_000, "two files of 900 MB were held at once: the address space peaked at {reserved} KiB");
            assert!(reserved > 800_000, "and one was loaded: {reserved} KiB");
        }
        other => panic!("no scenario {other}"),
    }
}

/// Runs the child's `scenario` under `ulimit -v limit_kib`: whether it ended well, and what it printed.
fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact read_room::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

fn result_line(stdout: &str) -> String {
    stdout.lines().find(|l| l.contains("RESULT")).unwrap_or("no RESULT line").to_owned()
}

#[test]
fn a_file_of_900_mb_beside_a_read_that_holds_the_budget_is_refused_from_its_size_and_never_loaded() {
    let (ended_well, stdout) = run_child("refused_unread", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    eprintln!("{}", result_line(&stdout));
}

#[test]
fn two_opens_of_files_of_900_mb_at_the_same_moment_load_one_file_at_a_time_and_do_not_abort() {
    let (ended_well, stdout) = run_child("two_at_once", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    eprintln!("{}", result_line(&stdout));
}
