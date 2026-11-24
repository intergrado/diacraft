//! What saving a big new picture into a package costs in memory: each case in a child process under
//! an address-space limit (`ulimit -v 3000000`), as `fodg_memory.rs` and `write_memory.rs` run theirs. A process that is out of memory *aborts*, and a child that aborts fails
//! the parent: a save that cannot have the memory must end as an error of a class, never as an abort.
//!
//! Four cases: one picture of the most a part of a package may hold (the container's cap of 256 MiB, less a little) is saved, and the peak is measured, and so is it from a
//! template saved as a drawing (a kind is asked for, which is how a new drawing is saved); three pictures of
//! 250 MB are saved (750 MB, which the package may hold and the memory gate admits); four of them (a gigabyte, the most a package may hold in all, and more than the memory the
//! gate allows beside the model) are refused as a limit before any of the copies is made, with the model alone held.

#![cfg(target_os = "linux")]

use std::process::Command;

use diacraft_odg::{Limits, OriginalPackage, WriteOptions, WriteWork, write_counted};

use crate::write_picture::{PNG_HEAD, noise, picture, three};

const SCENARIO: &str = "ODG_PICTURE_MEMORY_SCENARIO";

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("this test needs /proc/self/status (Linux): {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or_else(|| panic!("no {key} line in /proc/self/status"))
}

/// The save of a drawing of three shapes with new pictures of `sizes` bytes each (each its own bytes), and what it ended as: its class, the estimate it made and, when
/// it was made, the package, which the caller reads back once the model is gone (the model is dropped when this returns).
fn save_pictures(sizes: &[usize], as_drawing_from_template: bool) -> (String, u64, Option<Vec<u8>>) {
    // A template saved as a drawing is how a new drawing is saved (the kind is asked for, and the package made is opened again to be sure it states it).
    let mut o = if as_drawing_from_template {
        let mut d = crate::write_core::drawing_of(3);
        d.meta = Some(crate::write_core::full_meta());
        crate::write_core::open_bytes(crate::build::package(crate::build::TEMPLATE, d.files()))
    } else {
        three()
    };
    let first = o.index.next_id(0).unwrap();
    for (k, size) in sizes.iter().enumerate() {
        // The bytes are made, put in an `Arc` and the buffer dropped, so that the model holds them once.
        let bytes = noise(&PNG_HEAD, *size, k as u64 + 1);
        crate::write_new::push_shape(&mut o.doc, picture(first + k as u32, &format!("P{k}"), (1.0 + k as f64, 8.0, 0.5, 0.5), &bytes));
    }
    let mut work = WriteWork::default();
    let saved = write_counted(
        &o.doc,
        Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }),
        &WriteOptions {
            limits: Limits::default(),
            variant: as_drawing_from_template.then_some(diacraft_odg::Variant::Drawing),
            ..WriteOptions::default()
        },
        &mut work,
    );
    let class = saved.as_ref().err().map_or("saved".to_owned(), |e| e.class().to_owned());
    if let Ok(saved) = &saved {
        // The package holds each picture whole: the sizes are the directory's, and no entry is inflated.
        let held: u64 = diacraft_container::zip::Archive::read(&saved.bytes, &diacraft_container::Limits::default())
            .map(|a| a.entries().iter().filter(|e| e.name.starts_with("Pictures/")).map(|e| e.size).sum())
            .unwrap_or(0);
        assert_eq!(held, sizes.iter().sum::<usize>() as u64, "every picture is in the package, whole");
    }
    (class, work.estimated_memory, saved.ok().map(|w| w.bytes))
}

/// The child's scenarios, one per call.
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    println!("\nSTART VmSize={}KiB", status_kib("VmSize"));
    let (class, gate, made) = match scenario.as_str() {
        // The most a part of a package holds, less a little: the container's cap is 256 MiB.
        "one_picture_at_the_part_cap" => save_pictures(&[256 * 1024 * 1024 - 1024], false),
        "template_one_picture_at_the_part_cap_as_a_drawing" => save_pictures(&[256 * 1024 * 1024 - 1024], true),
        "three_pictures_of_250_mb" => save_pictures(&[250_000_000; 3], false),
        "four_pictures_of_250_mb" => save_pictures(&[250_000_000; 4], false),
        other => panic!("no scenario {other}"),
    };
    println!("\nRESULT {scenario} class={class} gate={gate} VmHWM={}KiB", status_kib("VmHWM"));
    let expected = if scenario == "four_pictures_of_250_mb" { "write_limit" } else { "saved" };
    assert_eq!(class, expected, "{scenario}");
    // The package made is read as the reader reads one, after the line above (its peak is the save's) and with the model let go: the pictures are held by the file, by
    // what the read inflates and by the model it makes, so this read peaks higher than the save did (2.45 GB for three of 250 MB, against 1.72 GB).
    if let Some(made) = made {
        crate::write_core::assert_reads_back(&scenario, &made);
    }
}

/// Runs the child's `scenario` under `ulimit -v limit_kib`: whether it ended well, and what it printed.
fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact picture_memory::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

#[test]
fn a_new_picture_of_the_most_a_part_may_hold_is_saved_under_the_address_limit() {
    let (ended_well, stdout) = run_child("one_picture_at_the_part_cap", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn a_new_picture_of_the_most_a_part_may_hold_is_saved_under_the_address_limit_when_a_template_is_saved_as_a_drawing() {
    let (ended_well, stdout) = run_child("template_one_picture_at_the_part_cap_as_a_drawing", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn three_new_pictures_of_250_mb_are_saved_under_the_address_limit() {
    let (ended_well, stdout) = run_child("three_pictures_of_250_mb", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn four_new_pictures_of_250_mb_are_refused_as_a_limit_and_do_not_abort_under_the_address_limit() {
    let (ended_well, stdout) = run_child("four_pictures_of_250_mb", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}
