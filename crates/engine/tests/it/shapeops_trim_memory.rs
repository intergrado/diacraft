//! Trim and Join in an address space that is bounded (SHAPEOPS-CAPS): a crafted drawing is finished or refused in words, and the process is not aborted by an allocation
//! it cannot make. Each scenario runs in a child of this test binary under `ulimit -v 3000000`, as `vsdx_memory.rs` does, since a test that is killed for its memory is no
//! test result.
//!
//! The case that found the bug: the path library takes each shape it is given as a thing of its own on the sweep, and the memory of a sweep of many *shapes* that all lie
//! across one another's heights is about 90 bytes for the square of their number: 20,000 separate short segments of one drawing, given as 20,000 shapes, were 24 GB. So Trim
//! gives the library one shape holding every path, whose cost is the paths'.

use std::process::Command;

use serde_json::json;

use crate::shapeops::{ids, line, session_with};
use crate::shapeops_join_trim_hostile::comb;

/// The address space the child may take, in KiB: the engine's own tests of memory take 3,000,000.
const LIMIT_KIB: u64 = 3_000_000;

const SCENARIO: &str = "DIACRAFT_SHAPEOPS_SCENARIO";

/// The peak of the address space of this process, in KiB.
fn vm_peak_kib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status.lines().find(|l| l.starts_with("VmPeak")).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// The scenarios the child runs: each prints a line that starts with `RESULT`.
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("a scenario");
    let seg = |i: u32| ((f64::from(i) * 0.1, 0.0), (f64::from(i) * 0.1, 0.05));
    let (command, shapes, params) = match scenario.as_str() {
        // The most separate segments a Trim makes pieces of, in one shape: 9,999 pieces.
        "comb9999" => ("shape.trim", vec![comb(1, 9_999, seg)], json!({"ids": [1]})),
        // Twice and ten times what a command makes pieces of, in one shape: refused in words.
        "comb20000" => ("shape.trim", vec![comb(1, 20_000, seg)], json!({"ids": [1]})),
        "comb90000" => ("shape.trim", vec![comb(1, 90_000, seg)], json!({"ids": [1]})),
        // Many shapes of one segment each that lie across one another's heights, which is what a sweep of many shapes costs the square of.
        "lines4096" => {
            let shapes: Vec<_> = (0..4_096_u32).map(|i| line(i + 1, (f64::from(i) * 0.1, 0.0), (f64::from(i) * 0.1, 0.05)).build()).collect();
            ("shape.trim", shapes, json!({"ids": (1..=4_096_u32).collect::<Vec<_>>()}))
        }
        "joinlines4096" => {
            let shapes: Vec<_> = (0..4_096_u32).map(|i| line(i + 1, (f64::from(i) * 0.1, 0.0), (f64::from(i) * 0.1, 0.05)).build()).collect();
            ("shape.join", shapes, json!({"ids": (1..=4_096_u32).collect::<Vec<_>>()}))
        }
        other => panic!("no scenario {other}"),
    };
    let mut s = session_with(shapes);
    let started = std::time::Instant::now();
    let result = s.execute(command, &params);
    let took = started.elapsed();
    let what = match &result {
        Ok(done) => format!("ok made={}", ids(done, "ids").len()),
        Err(error) => format!("refused {error}"),
    };
    println!("\nRESULT {what} took={took:?} VmPeak={}KiB", vm_peak_kib());
}

/// Runs the child's `scenario` under `ulimit -v`: whether it ended well, and what it printed.
fn run_child(scenario: &str) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {LIMIT_KIB} && exec \"$0\" --exact shapeops_trim_memory::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

fn result_line(stdout: &str) -> String {
    stdout.lines().find(|l| l.starts_with("RESULT")).unwrap_or("no RESULT line").to_owned()
}

fn peak(line: &str) -> u64 {
    line.split("VmPeak=").nth(1).and_then(|rest| rest.split("KiB").next()).and_then(|n| n.parse().ok()).unwrap_or(u64::MAX)
}

/// The child ended well and did what `expect` says (`ok` or `refused`), within the address space.
fn assert_ends_well(scenario: &str, expect: &str) -> String {
    let (ended_well, stdout) = run_child(scenario);
    assert!(ended_well, "the child `{scenario}` aborted under `ulimit -v {LIMIT_KIB}`: {stdout}");
    let line = result_line(&stdout);
    assert!(line.starts_with(&format!("RESULT {expect}")), "{scenario}: {line}");
    assert!(peak(&line) < LIMIT_KIB, "{scenario}: the address space reached {} KiB of the {LIMIT_KIB}: {line}", peak(&line));
    line
}

#[test]
fn a_comb_of_9999_separate_segments_is_trimmed_to_9999_pieces_in_a_bounded_address_space() {
    let line = assert_ends_well("comb9999", "ok made=9999");
    eprintln!("{line}");
}

#[test]
fn a_comb_of_20000_and_of_90000_separate_segments_is_refused_in_words_in_a_bounded_address_space() {
    for scenario in ["comb20000", "comb90000"] {
        let line = assert_ends_well(scenario, "refused");
        assert!(line.contains("more than 10000 shapes"), "{line}");
    }
}

#[test]
fn four_thousand_ninety_six_shapes_of_one_segment_each_are_trimmed_in_a_bounded_address_space() {
    assert_ends_well("lines4096", "ok made=4096");
}

#[test]
fn join_of_4096_separate_lines_is_made_in_a_bounded_address_space() {
    assert_ends_well("joinlines4096", "ok made=1");
}
